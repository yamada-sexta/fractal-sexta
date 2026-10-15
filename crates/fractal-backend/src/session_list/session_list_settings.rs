use gtk::{gio, glib, prelude::*, subclass::prelude::*};
use indexmap::{IndexMap, IndexSet};
use tracing::error;

use crate::{
    secret::SESSION_ID_LENGTH,
    session::{SessionSettings, StoredSessionSettings},
};

mod imp {
    use std::cell::RefCell;

    use super::*;

    #[derive(Debug, Default)]
    pub struct SessionListSettings {
        /// The settings of the sessions.
        pub(super) sessions: RefCell<IndexMap<String, SessionSettings>>,
        pub(super) store: RefCell<Option<gio::Settings>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SessionListSettings {
        const NAME: &'static str = "SessionListSettings";
        type Type = super::SessionListSettings;
    }

    impl ObjectImpl for SessionListSettings {}
}

glib::wrapper! {
    /// The settings of the list of sessions.
    pub struct SessionListSettings(ObjectSubclass<imp::SessionListSettings>);
}

impl SessionListSettings {
    pub fn set_store(&self, store: &gio::Settings) {
        self.imp().store.replace(Some(store.clone()));
    }
    pub fn store(&self) -> gio::Settings {
        self.imp()
            .store
            .borrow()
            .clone()
            .expect("session settings store initialized before restoration")
    }

    /// Create a new `SessionListSettings`.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Load these settings from the application settings.
    pub fn load(&self) {
        let serialized = self.store().string("sessions");

        let stored_sessions =
            match serde_json::from_str::<Vec<(String, StoredSessionSettings)>>(&serialized) {
                Ok(stored_sessions) => stored_sessions,
                Err(error) => {
                    error!(
                        "Could not load sessions settings, fallback to default settings: {error}"
                    );
                    Default::default()
                }
            };

        // Do we need to update the settings?
        let mut needs_update = false;

        let sessions = stored_sessions
            .into_iter()
            .map(|(mut session_id, stored_session)| {
                // Session IDs have been truncated in version 6 of StoredSession.
                if session_id.len() > SESSION_ID_LENGTH {
                    session_id.truncate(SESSION_ID_LENGTH);
                    needs_update = true;
                }

                let session = SessionSettings::restore(&session_id, stored_session);
                session.set_owner(self);
                (session_id, session)
            })
            .collect();

        self.imp().sessions.replace(sessions);

        if needs_update {
            self.save();
        }
    }

    /// Save these settings in the application settings.
    pub fn save(&self) {
        let stored_sessions = self
            .imp()
            .sessions
            .borrow()
            .iter()
            .map(|(session_id, session)| (session_id.clone(), session.stored_settings()))
            .collect::<Vec<_>>();

        if let Err(error) = self.store().set_string(
            "sessions",
            &serde_json::to_string(&stored_sessions).unwrap(),
        ) {
            error!("Could not save sessions settings: {error}");
        }
    }

    /// Get or create the settings for the session with the given ID.
    pub fn get_or_create(&self, session_id: &str) -> SessionSettings {
        let sessions = &self.imp().sessions;

        if let Some(session) = sessions.borrow().get(session_id) {
            return session.clone();
        }

        let session = SessionSettings::new(session_id);
        session.set_owner(self);
        sessions
            .borrow_mut()
            .insert(session_id.to_owned(), session.clone());
        self.save();

        session
    }

    /// Remove the settings of the session with the given ID.
    pub fn remove(&self, session_id: &str) {
        self.imp().sessions.borrow_mut().shift_remove(session_id);
        self.save();
    }

    /// Get the list of session IDs stored in these settings.
    pub fn session_ids(&self) -> IndexSet<String> {
        self.imp().sessions.borrow().keys().cloned().collect()
    }
}

impl Default for SessionListSettings {
    fn default() -> Self {
        Self::new()
    }
}
