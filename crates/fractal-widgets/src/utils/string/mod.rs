//! Widget-producing mention rendering.
pub use fractal_core::string::*;

use crate::{
    components::{AvatarImageSafetySetting, LabelWithWidgets, Pill},
    prelude::*,
    session::Room,
    utils::matrix::MatrixIdUri,
};
pub trait PangoMentionExt {
    /// Append the given URI as a mention, if it is one.
    ///
    /// Returns the created [`Pill`], it the URI was added as a mention.
    fn maybe_append_mention(&mut self, uri: impl TryInto<MatrixIdUri>, room: &Room)
    -> Option<Pill>;

    /// Append the given string and replace `@room` with a mention.
    ///
    /// Returns the created [`Pill`], it `@room` was found.
    fn append_and_replace_at_room(&mut self, s: &str, room: &Room) -> Option<Pill>;
}
impl PangoMentionExt for String {
    fn maybe_append_mention(
        &mut self,
        uri: impl TryInto<MatrixIdUri>,
        room: &Room,
    ) -> Option<Pill> {
        let pill = uri.try_into().ok().and_then(|uri| uri.into_pill(room))?;

        self.push_str(LabelWithWidgets::PLACEHOLDER);

        Some(pill)
    }

    fn append_and_replace_at_room(&mut self, s: &str, room: &Room) -> Option<Pill> {
        if let Some(pos) = find_at_room(s) {
            self.push_str(&(&s[..pos]).escape_markup());
            self.push_str(LabelWithWidgets::PLACEHOLDER);
            self.push_str(&(&s[pos + AT_ROOM.len()..]).escape_markup());

            // We do not need to watch safety settings for mentions, rooms will be watched
            // automatically.
            Some(room.at_room().to_pill(AvatarImageSafetySetting::None, None))
        } else {
            self.push_str(&s.escape_markup());
            None
        }
    }
}
pub trait LinkifierWidgetExt<'a> {
    fn detect_mentions(
        self,
        room: &'a Room,
        pills: &'a mut Vec<Pill>,
        detect_at_room: bool,
    ) -> Self;
}
impl<'a> LinkifierWidgetExt<'a> for Linkifier<'a> {
    fn detect_mentions(
        self,
        room: &'a Room,
        pills: &'a mut Vec<Pill>,
        detect_at_room: bool,
    ) -> Self {
        self.with_mentions(LabelWithWidgets::PLACEHOLDER, detect_at_room, move |uri| {
            let pill = if uri == AT_ROOM {
                Some(room.at_room().to_pill(AvatarImageSafetySetting::None, None))
            } else {
                MatrixIdUri::parse(uri)
                    .ok()
                    .and_then(|id| id.into_pill(room))
            };
            if let Some(pill) = pill {
                pills.push(pill);
                true
            } else {
                false
            }
        })
    }
}
