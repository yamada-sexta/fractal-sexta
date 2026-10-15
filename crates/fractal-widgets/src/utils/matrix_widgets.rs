use ruma::html::{Children, Html, NodeRef, StrTendril, matrix::MatrixElement};
use tracing::debug;

use crate::{
    components::{AvatarImageSafetySetting, Pill},
    session::Room,
    utils::matrix::MatrixIdUri,
};
pub trait MatrixIdUriWidgetExt {
    fn into_pill(self, room: &Room) -> Option<Pill>;
}
impl MatrixIdUriWidgetExt for MatrixIdUri {
    /// Try to construct a [`Pill`] from this ID in the given room.
    fn into_pill(self, room: &Room) -> Option<Pill> {
        match self {
            Self::Room(room_uri) => {
                let session = room.session()?;

                let pill =
                    if let Some(uri_room) = session.room_list().get_by_identifier(&room_uri.id) {
                        // We do not need to watch safety settings for local rooms, they will be
                        // watched automatically.
                        Pill::new(&uri_room, AvatarImageSafetySetting::None, None)
                    } else {
                        Pill::new(
                            &session.remote_cache().room(room_uri),
                            AvatarImageSafetySetting::MediaPreviews,
                            Some(room.clone()),
                        )
                    };

                Some(pill)
            }
            Self::User(user_id) => {
                // We should have a strong reference to the list wherever we show a user pill,
                // so we can use `get_or_create_members()`.
                let user = room.get_or_create_members().get_or_create(user_id);

                // We do not need to watch safety settings for users.
                Some(Pill::new(&user, AvatarImageSafetySetting::None, None))
            }
            Self::Event(_) => None,
        }
    }
}
/// Find mentions in the given HTML string.
///
/// Returns a list of `(pill, mention_content)` tuples.
pub fn find_html_mentions(html: &str, room: &Room) -> Vec<(Pill, StrTendril)> {
    let mut mentions = Vec::new();
    let html = Html::parse(html);

    append_children_mentions(&mut mentions, html.children(), room);

    mentions
}

/// Find mentions in the given child nodes and append them to the given list.
fn append_children_mentions(
    mentions: &mut Vec<(Pill, StrTendril)>,
    children: Children,
    room: &Room,
) {
    for node in children {
        if let Some(mention) = node_as_mention(&node, room) {
            mentions.push(mention);
            continue;
        }

        append_children_mentions(mentions, node.children(), room);
    }
}

/// Try to convert the given node to a mention.
///
/// This does not recurse into children.
fn node_as_mention(node: &NodeRef, room: &Room) -> Option<(Pill, StrTendril)> {
    // Mentions are links.
    let MatrixElement::A(anchor) = node.as_element()?.to_matrix().element else {
        return None;
    };

    // Mentions contain Matrix URIs.
    let id = MatrixIdUri::try_from(anchor.href?).ok()?;

    // Mentions contain one text child node.
    let child = node.children().next()?;

    if child.next_sibling().is_some() {
        return None;
    }

    let content = child.as_text()?.borrow().clone();
    let pill = id.into_pill(room)?;

    Some((pill, content))
}

use gettextrs::gettext;
use gtk::{gio, glib, prelude::*};
use matrix_sdk::Client;
use tracing::error;

use crate::{prelude::*, toast, utils::matrix::MediaMessage};
pub trait MediaMessageSaveExt: Sized {
    async fn save_to_file(
        self,
        timestamp: &glib::DateTime,
        client: &Client,
        parent: &impl IsA<gtk::Widget>,
    );
}
impl MediaMessageSaveExt for MediaMessage {
    /// Save the content of the media to a file selected by the user.
    ///
    /// Shows a dialog to the user to select a file on the system.
    async fn save_to_file(
        self,
        timestamp: &glib::DateTime,
        client: &Client,
        parent: &impl IsA<gtk::Widget>,
    ) {
        let filename = self.filename(timestamp);

        let data = match self.into_content(client).await {
            Ok(data) => data,
            Err(error) => {
                error!("Could not retrieve media file: {error}");
                toast!(parent, error.to_user_facing());

                return;
            }
        };

        let dialog = gtk::FileDialog::builder()
            .title(gettext("Save File"))
            .modal(true)
            .accept_label(gettext("Save"))
            .initial_name(filename)
            .build();

        match dialog
            .save_future(parent.root().and_downcast_ref::<gtk::Window>())
            .await
        {
            Ok(file) => {
                if let Err(error) = file.replace_contents(
                    &data,
                    None,
                    false,
                    gio::FileCreateFlags::REPLACE_DESTINATION,
                    gio::Cancellable::NONE,
                ) {
                    error!("Could not save file: {error}");
                    toast!(parent, gettext("Could not save file"));
                }
            }
            Err(error) => {
                if error.matches(gtk::DialogError::Dismissed) {
                    debug!("File dialog dismissed by user");
                } else {
                    error!("Could not access file: {error}");
                    toast!(parent, gettext("Could not access file"));
                }
            }
        }
    }
}

impl MediaMessageSaveExt for crate::utils::matrix::VisualMediaMessage {
    /// Save the content of the media to a file selected by the user.
    ///
    /// Shows a dialog to the user to select a file on the system.
    async fn save_to_file(
        self,
        timestamp: &glib::DateTime,
        client: &Client,
        parent: &impl IsA<gtk::Widget>,
    ) {
        MediaMessage::from(self)
            .save_to_file(timestamp, client, parent)
            .await;
    }
}
