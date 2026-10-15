use std::collections::BTreeSet;

use gtk::{glib, prelude::*, subclass::prelude::*};
use indexmap::IndexSet;
use ruma::{OwnedServerName, events::media_preview_config::MediaPreviews};
use serde::{Deserialize, Serialize};
use tracing::info;

use super::SidebarSectionName;
use crate::session_list::SessionListSettings;

/// The current version of the stored session settings.
const CURRENT_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct StoredSessionSettings {
    /// The version of the stored settings.
    #[serde(default)]
    pub(super) version: u8,

    /// Custom servers to explore.
    #[serde(default, skip_serializing_if = "IndexSet::is_empty")]
    explore_custom_servers: IndexSet<OwnedServerName>,

    /// Whether notifications are enabled for this session.
    #[serde(
        default = "ruma::serde::default_true",
        skip_serializing_if = "ruma::serde::is_true"
    )]
    notifications_enabled: bool,

    /// Whether public read receipts are enabled for this session.
    #[serde(
        default = "ruma::serde::default_true",
        skip_serializing_if = "ruma::serde::is_true"
    )]
    public_read_receipts_enabled: bool,

    /// Whether typing notifications are enabled for this session.
    #[serde(
        default = "ruma::serde::default_true",
        skip_serializing_if = "ruma::serde::is_true"
    )]
    typing_enabled: bool,

    /// The sections that are expanded.
    #[serde(default)]
    sections_expanded: SectionsExpanded,

    /// Which rooms display media previews for this session.
    ///
    /// Legacy setting from version 0 of the stored settings.
    #[serde(skip_serializing)]
    pub(super) media_previews_enabled: Option<MediaPreviewsSetting>,

    /// Whether to display avatars in invites.
    ///
    /// Legacy setting from version 0 of the stored settings.
    #[serde(skip_serializing)]
    pub(super) invite_avatars_enabled: Option<bool>,
}

impl Default for StoredSessionSettings {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            explore_custom_servers: Default::default(),
            notifications_enabled: true,
            public_read_receipts_enabled: true,
            typing_enabled: true,
            sections_expanded: Default::default(),
            media_previews_enabled: Default::default(),
            invite_avatars_enabled: Default::default(),
        }
    }
}

mod imp {
    use std::{
        cell::{OnceCell, RefCell},
        marker::PhantomData,
    };

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SessionSettings)]
    pub struct SessionSettings {
        /// The ID of the session these settings are for.
        #[property(get, construct_only)]
        session_id: OnceCell<String>,
        /// The stored settings.
        pub(super) stored_settings: RefCell<StoredSessionSettings>,
        pub(super) owner: glib::WeakRef<SessionListSettings>,
        /// Whether notifications are enabled for this session.
        #[property(get = Self::notifications_enabled, set = Self::set_notifications_enabled, explicit_notify, default = true)]
        notifications_enabled: PhantomData<bool>,
        /// Whether public read receipts are enabled for this session.
        #[property(get = Self::public_read_receipts_enabled, set = Self::set_public_read_receipts_enabled, explicit_notify, default = true)]
        public_read_receipts_enabled: PhantomData<bool>,
        /// Whether typing notifications are enabled for this session.
        #[property(get = Self::typing_enabled, set = Self::set_typing_enabled, explicit_notify, default = true)]
        typing_enabled: PhantomData<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SessionSettings {
        const NAME: &'static str = "SessionSettings";
        type Type = super::SessionSettings;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SessionSettings {}

    impl SessionSettings {
        /// Whether notifications are enabled for this session.
        fn notifications_enabled(&self) -> bool {
            self.stored_settings.borrow().notifications_enabled
        }

        /// Set whether notifications are enabled for this session.
        fn set_notifications_enabled(&self, enabled: bool) {
            if self.notifications_enabled() == enabled {
                return;
            }

            self.stored_settings.borrow_mut().notifications_enabled = enabled;
            self.obj().owner().save();
            self.obj().notify_notifications_enabled();
        }

        /// Whether public read receipts are enabled for this session.
        fn public_read_receipts_enabled(&self) -> bool {
            self.stored_settings.borrow().public_read_receipts_enabled
        }

        /// Set whether public read receipts are enabled for this session.
        fn set_public_read_receipts_enabled(&self, enabled: bool) {
            if self.public_read_receipts_enabled() == enabled {
                return;
            }

            self.stored_settings
                .borrow_mut()
                .public_read_receipts_enabled = enabled;
            self.obj().owner().save();
            self.obj().notify_public_read_receipts_enabled();
        }

        /// Whether typing notifications are enabled for this session.
        fn typing_enabled(&self) -> bool {
            self.stored_settings.borrow().typing_enabled
        }

        /// Set whether typing notifications are enabled for this session.
        fn set_typing_enabled(&self, enabled: bool) {
            if self.typing_enabled() == enabled {
                return;
            }

            self.stored_settings.borrow_mut().typing_enabled = enabled;
            self.obj().owner().save();
            self.obj().notify_typing_enabled();
        }

        /// Apply the migration of the stored settings from version 0 to version
        /// 1.
        pub fn apply_version_1_migration(&self) {
            {
                let mut stored_settings = self.stored_settings.borrow_mut();

                if stored_settings.version > 0 {
                    return;
                }

                info!(
                    session = self.obj().session_id(),
                    "Migrating store session to version 1"
                );

                stored_settings.media_previews_enabled.take();
                stored_settings.invite_avatars_enabled.take();
                stored_settings.version = 1;
            }

            self.obj().owner().save();
        }
    }
}

glib::wrapper! {
    /// The settings of a [`Session`](super::Session).
    pub struct SessionSettings(ObjectSubclass<imp::SessionSettings>);
}

impl SessionSettings {
    pub fn set_owner(&self, owner: &SessionListSettings) {
        self.imp().owner.set(Some(owner));
    }
    fn owner(&self) -> SessionListSettings {
        self.imp()
            .owner
            .upgrade()
            .expect("session settings owner is alive")
    }

    /// Create a new `SessionSettings` for the given session ID.
    pub fn new(session_id: &str) -> Self {
        glib::Object::builder()
            .property("session-id", session_id)
            .build()
    }

    /// Restore existing `SessionSettings` with the given session ID and stored
    /// settings.
    pub fn restore(session_id: &str, stored_settings: StoredSessionSettings) -> Self {
        let obj = Self::new(session_id);
        *obj.imp().stored_settings.borrow_mut() = stored_settings;
        obj
    }

    /// The stored settings.
    pub fn stored_settings(&self) -> StoredSessionSettings {
        self.imp().stored_settings.borrow().clone()
    }

    /// Apply the migration of the stored settings from version 0 to version 1.
    pub fn apply_version_1_migration(&self) {
        self.imp().apply_version_1_migration();
    }

    /// Delete the settings from the application settings.
    pub fn delete(&self) {
        self.owner().remove(&self.session_id());
    }

    /// Custom servers to explore.
    pub fn explore_custom_servers(&self) -> IndexSet<OwnedServerName> {
        self.imp()
            .stored_settings
            .borrow()
            .explore_custom_servers
            .clone()
    }

    /// Set the custom servers to explore.
    pub fn set_explore_custom_servers(&self, servers: IndexSet<OwnedServerName>) {
        if self.explore_custom_servers() == servers {
            return;
        }

        self.imp()
            .stored_settings
            .borrow_mut()
            .explore_custom_servers = servers;
        self.owner().save();
    }

    /// Whether the section with the given name is expanded.
    pub fn is_section_expanded(&self, section_name: SidebarSectionName) -> bool {
        self.imp()
            .stored_settings
            .borrow()
            .sections_expanded
            .is_section_expanded(section_name)
    }

    /// Set whether the section with the given name is expanded.
    pub fn set_section_expanded(&self, section_name: SidebarSectionName, expanded: bool) {
        self.imp()
            .stored_settings
            .borrow_mut()
            .sections_expanded
            .set_section_expanded(section_name, expanded);
        self.owner().save();
    }
}

/// The sections that are expanded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SectionsExpanded(BTreeSet<SidebarSectionName>);

impl SectionsExpanded {
    /// Whether the section with the given name is expanded.
    pub fn is_section_expanded(&self, section_name: SidebarSectionName) -> bool {
        self.0.contains(&section_name)
    }

    /// Set whether the section with the given name is expanded.
    pub fn set_section_expanded(&mut self, section_name: SidebarSectionName, expanded: bool) {
        if expanded {
            self.0.insert(section_name);
        } else {
            self.0.remove(&section_name);
        }
    }
}

impl Default for SectionsExpanded {
    fn default() -> Self {
        Self(BTreeSet::from([
            SidebarSectionName::VerificationRequest,
            SidebarSectionName::InviteRequest,
            SidebarSectionName::Invited,
            SidebarSectionName::Favorite,
            SidebarSectionName::Normal,
            SidebarSectionName::LowPriority,
        ]))
    }
}

/// Setting about which rooms display media previews.
///
/// Legacy setting from version 0 of the stored settings.
#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct MediaPreviewsSetting {
    /// The default setting for all rooms.
    #[serde(default)]
    pub(super) global: MediaPreviewsGlobalSetting,
}

/// Possible values of the global setting about which rooms display media
/// previews.
///
/// Legacy setting from version 0 of the stored settings.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum MediaPreviewsGlobalSetting {
    /// All rooms show media previews.
    All,
    /// Only private rooms show media previews.
    #[default]
    Private,
    /// No rooms show media previews.
    None,
}

impl From<MediaPreviewsGlobalSetting> for MediaPreviews {
    fn from(value: MediaPreviewsGlobalSetting) -> Self {
        match value {
            MediaPreviewsGlobalSetting::All => Self::On,
            MediaPreviewsGlobalSetting::Private => Self::Private,
            MediaPreviewsGlobalSetting::None => Self::Off,
        }
    }
}
