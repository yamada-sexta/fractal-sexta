//! Local room-history search over the SDK's existing event cache.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, atomic::{AtomicBool, Ordering}},
};

use gtk::glib;
use matrix_sdk::{
    Room as MatrixRoom, check_validity_of_replacement_events,
    deserialized_responses::TimelineEvent,
};
use ruma::{
    MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedUserId,
    events::{
        AnySyncMessageLikeEvent, AnySyncTimelineEvent, SyncMessageLikeEvent,
        room::message::{MessageType, OriginalSyncRoomMessageEvent, Relation},
    },
    html::{Html, HtmlSanitizerMode, NodeRef, RemoveReplyFallback},
    serde::Raw,
};

use crate::session::is_supported_message;

/// A three-way presence filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PresenceFilter {
    #[default]
    Any,
    Include,
    Exclude,
}

impl PresenceFilter {
    fn matches(self, present: bool) -> bool {
        match self {
            Self::Any => true,
            Self::Include => present,
            Self::Exclude => !present,
        }
    }
}

/// Criteria applied together to each message. Attachment kinds are image, video,
/// audio, and file, in that order.
#[derive(Clone, Debug, Default)]
pub struct RoomSearchFilter {
    /// A Unicode case-folded substring.
    pub text: String,
    pub sender: Option<OwnedUserId>,
    pub links: PresenceFilter,
    pub attachments: PresenceFilter,
    pub kinds: [PresenceFilter; 4],
}

impl RoomSearchFilter {
    pub fn matches(&self, message: &SearchMessage) -> bool {
        self.sender
            .as_ref()
            .is_none_or(|sender| *sender == message.sender)
            && message.search_text.contains(self.text.as_str())
            && self.links.matches(message.has_link)
            && self.attachments.matches(message.attachment_kind.is_some())
            && message
                .attachment_kind
                .is_none_or(|kind| self.kinds[kind] != PresenceFilter::Exclude)
            && (!self.kinds.contains(&PresenceFilter::Include)
                || message
                    .attachment_kind
                    .is_some_and(|kind| self.kinds[kind] == PresenceFilter::Include))
    }
}

/// One searchable message with its effective cached content.
#[derive(Debug)]
pub struct SearchMessage {
    pub event_id: OwnedEventId,
    pub sender: OwnedUserId,
    pub timestamp: MilliSecondsSinceUnixEpoch,
    preview: String,
    search_text: String,
    has_link: bool,
    attachment_kind: Option<usize>,
}

/// A room cache snapshot. No plaintext search data is written to disk.
#[derive(Debug, Default)]
pub struct RoomSearchSnapshot {
    pub messages: Vec<Arc<SearchMessage>>,
    pub event_ids: HashSet<OwnedEventId>,
    pub undecryptable: usize,
}

impl RoomSearchSnapshot {
    /// Read persisted and in-memory events without fetching from the homeserver.
    pub async fn load(room: &MatrixRoom, cancelled: Arc<AtomicBool>) -> Result<Self, String> {
        check_cancelled(&cancelled)?;
        let (cache, _handles) = room.event_cache().await.map_err(|e| e.to_string())?;
        // shortcut: scans the room cache in memory, use an index if large-room searches become slow.
        let mut events = {
            let lock = room
                .client()
                .event_cache_store()
                .lock()
                .await
                .map_err(|e| e.to_string())?;
            let store = lock
                .as_clean()
                .ok_or_else(|| "Event cache is being refreshed; try again".to_owned())?;
            store
                .get_room_events(room.room_id(), None, None)
                .await
                .map_err(|e| e.to_string())?
        };
        check_cancelled(&cancelled)?;
        events.extend(cache.events().await.map_err(|e| e.to_string())?);
        let rules = room.clone_info().room_version_rules_or_default();
        crate::RUNTIME
            .spawn_blocking(move || Self::from_events(events, &rules.redaction, &cancelled))
            .await
            .map_err(|e| e.to_string())?
    }

    /// Drain already-searched disk pages before returning a genuinely older page.
    pub async fn load_older(
        room: &MatrixRoom,
        known_event_ids: &HashSet<OwnedEventId>,
        cancelled: &AtomicBool,
    ) -> Result<bool, String> {
        let (cache, _handles) = room.event_cache().await.map_err(|e| e.to_string())?;
        // shortcut: paging grows the SDK cache, use a separate gap cursor if memory becomes a problem.
        let pagination = cache.pagination();
        loop {
            check_cancelled(cancelled)?;
            let page = pagination.run_backwards_once(100).await.map_err(|e| e.to_string())?;
            check_cancelled(cancelled)?;
            if page.reached_start || page.events.is_empty() || page.events.iter().any(|event| {
                event.event_id().is_some_and(|id| !known_event_ids.contains(id))
            }) {
                return Ok(page.reached_start);
            }
            tokio::task::yield_now().await;
        }
    }

    fn from_events(
        input_events: Vec<TimelineEvent>,
        rules: &ruma::room_version_rules::RedactionRules,
        cancelled: &AtomicBool,
    ) -> Result<Self, String> {
        let mut events = HashMap::new();
        let mut snapshot = Self::default();
        for event in input_events {
            check_cancelled(cancelled)?;
            if let Some(id) = event.event_id() {
                snapshot.event_ids.insert(id.to_owned());
                events.insert(id.to_owned(), event);
            }
        }
        let mut redacted = HashSet::new();
        let mut messages = HashMap::new();
        let mut edits: HashMap<OwnedEventId, Vec<OriginalSyncRoomMessageEvent>> = HashMap::new();
        for (id, event) in &events {
            check_cancelled(cancelled)?;
            match event.raw().deserialize() {
                Ok(AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
                    message,
                ))) => match message {
                    SyncMessageLikeEvent::Original(message) => {
                        if let Some(Relation::Replacement(replacement)) = &message.content.relates_to {
                            edits
                                .entry(replacement.event_id.clone())
                                .or_default()
                                .push(message);
                        } else if is_supported_message(&message.content.msgtype) {
                            // The SDK filters originals before applying edits.
                            messages.insert(id.clone(), message);
                        }
                    }
                    SyncMessageLikeEvent::Redacted(_) => {
                        redacted.insert(id.clone());
                    }
                },
                Ok(AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomRedaction(
                    event,
                ))) => {
                    if let Some(id) = event.redacts(rules) {
                        redacted.insert(id.to_owned());
                    }
                }
                Ok(AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomEncrypted(
                    SyncMessageLikeEvent::Original(_),
                ))) => {
                    snapshot.undecryptable += 1;
                }
                _ => {}
            }
        }

        let now = MilliSecondsSinceUnixEpoch::now();
        for (id, mut message) in messages {
            check_cancelled(cancelled)?;
            if redacted.contains(&id) {
                continue;
            }
            let original = &events[&id];
            // Legacy cache entries may lack the SDK's corrected timestamp.
            message.origin_server_ts = original.timestamp.unwrap_or(message.origin_server_ts).min(now);
            let mut candidates = edits.remove(&id).unwrap_or_default();
            if let Some(bundled) = message.unsigned.relations.replace.as_deref() {
                candidates.push(bundled.clone());
            }
            candidates.sort_by_key(|edit| edit.origin_server_ts);
            if let Some(edit) = candidates.into_iter().rev()
                .take_while(|_| !cancelled.load(Ordering::Relaxed))
                .find(|edit| {
                if redacted.contains(&edit.event_id) {
                    return false;
                }
                let cached = events.get(&edit.event_id);
                let raw = if let Some(cached) = cached {
                    cached.raw().clone()
                } else {
                    let Ok(Some(unsigned)) = original.raw().get_field::<serde_json::Value>("unsigned")
                    else {
                        return false;
                    };
                    let Some(bundled) = unsigned
                        .get("m.relations")
                        .and_then(|relations| relations.get("m.replace"))
                    else {
                        return false;
                    };
                    let Ok(raw) = Raw::<AnySyncTimelineEvent>::from_json_string(bundled.to_string())
                    else {
                        return false;
                    };
                    raw
                };
                let encryption = cached.and_then(TimelineEvent::encryption_info);
                check_validity_of_replacement_events(
                    original.raw(),
                    original.encryption_info().map(|info| &**info),
                    &raw,
                    encryption.map(|info| &**info),
                )
                .is_ok()
            }) && let Some(Relation::Replacement(replacement)) = edit.content.relates_to
            {
                message.content.apply_replacement(replacement.new_content);
            }

            if !is_supported_message(&message.content.msgtype) {
                continue;
            }
            let is_reply = message.content.relates_to.as_ref().is_some_and(|relation| {
                matches!(relation, Relation::Reply(_))
                    || matches!(relation, Relation::Thread(thread) if thread.in_reply_to.is_some())
            });
            message.content.msgtype.sanitize(
                HtmlSanitizerMode::Compat,
                if is_reply {
                    RemoveReplyFallback::Yes
                } else {
                    RemoveReplyFallback::No
                },
            );
            snapshot.messages.push(Arc::new(SearchMessage::new(message, cancelled)?));
        }
        snapshot.messages.sort_by(|a, b| {
            b.timestamp
                .cmp(&a.timestamp)
                .then_with(|| b.event_id.cmp(&a.event_id))
        });
        debug_assert!(
            snapshot
                .messages
                .iter()
                .all(|message| !redacted.contains(&message.event_id)
                    && snapshot.event_ids.contains(&message.event_id))
        );
        check_cancelled(cancelled)?;
        Ok(snapshot)
    }
}

impl SearchMessage {
    fn new(event: OriginalSyncRoomMessageEvent, cancelled: &AtomicBool) -> Result<Self, String> {
        let msgtype = &event.content.msgtype;
        let attachment_kind = match msgtype {
            MessageType::Image(_) => Some(0),
            MessageType::Video(_) => Some(1),
            MessageType::Audio(_) => Some(2),
            MessageType::File(_) => Some(3),
            _ => None,
        };
        let (filename, caption, formatted) = match msgtype {
            MessageType::Image(m) => (Some(m.filename()), m.caption(), m.formatted_caption()),
            MessageType::Video(m) => (Some(m.filename()), m.caption(), m.formatted_caption()),
            MessageType::Audio(m) => (Some(m.filename()), m.caption(), m.formatted_caption()),
            MessageType::File(m) => (Some(m.filename()), m.caption(), m.formatted_caption()),
            MessageType::Text(m) => (None, Some(m.body.as_str()), m.formatted.as_ref()),
            MessageType::Notice(m) => (None, Some(m.body.as_str()), m.formatted.as_ref()),
            MessageType::Emote(m) => (None, Some(m.body.as_str()), m.formatted.as_ref()),
            _ => (None, Some(msgtype.body()), None),
        };
        // The renderer escapes source text, so only generated links contain an anchor.
        let has_link = caption.is_some_and(|text| {
            fractal_core::string::linkify(text).contains("<a ")
        });
        let has_link = has_link || match formatted {
            Some(formatted) => html_has_link(&Html::parse(&formatted.body), cancelled)?,
            None => false,
        };
        let preview = match (caption, filename) {
            (Some(caption), Some(filename)) => format!("{caption}\n{filename}"),
            _ => msgtype.body().to_owned(),
        };
        let search_text = glib::casefold(&preview).to_string();
        Ok(Self {
            event_id: event.event_id,
            sender: event.sender,
            timestamp: event.origin_server_ts,
            preview,
            search_text,
            has_link,
            attachment_kind,
        })
    }

    /// A bounded, plain-text excerpt containing the match, computed off the UI thread.
    pub fn excerpt(&self, folded_query: &str) -> String {
        let folded_start = self.search_text.find(folded_query).unwrap_or(0);
        let mut folded_offset = 0;
        let mut match_char = 0;
        for (index, ch) in self.preview.chars().enumerate() {
            let folded_len = if ch.is_ascii() {
                1
            } else {
                glib::casefold(ch.to_string()).len()
            };
            if folded_offset + folded_len > folded_start {
                match_char = index;
                break;
            }
            folded_offset += folded_len;
        }
        let start = match_char.saturating_sub(60);
        let mut excerpt = String::new();
        if start > 0 {
            excerpt.push('…');
        }
        let mut previous_space = false;
        for (index, ch) in self.preview.chars().skip(start).take(241).enumerate() {
            if index == 240 {
                excerpt.push('…');
                break;
            }
            if ch.is_whitespace() {
                if !previous_space { excerpt.push(' '); }
                previous_space = true;
            } else {
                excerpt.push(ch);
                previous_space = false;
            }
        }
        excerpt
    }
}

fn html_has_link(html: &Html, cancelled: &AtomicBool) -> Result<bool, String> {
    let mut nodes: Vec<NodeRef> = html.children().collect();
    while let Some(node) = nodes.pop() {
        check_cancelled(cancelled)?;
        if let Some(element) = node.as_element() {
            if element.name.local.as_ref() == "mx-reply" {
                continue;
            }
            if element.name.local.as_ref() == "a"
                && element.attrs.borrow().iter().any(|attr| {
                    attr.name.local.as_ref() == "href" && !attr.value.is_empty()
                })
            {
                return Ok(true);
            }
        }
        nodes.extend(node.children());
    }
    Ok(false)
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Search canceled".to_owned())
    } else {
        Ok(())
    }
}
