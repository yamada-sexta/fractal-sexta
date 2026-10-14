use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::glib;
use matrix_sdk::sanitize_server_name;
use ruma::OwnedServerName;
use url::Url;

use super::advanced_dialog::LoginAdvancedDialog;
use crate::{
    components::{LoadingButton, OfflineBanner},
    gettext_f,
    utils::{Broadcaster, TemplateCallbacks},
};

mod imp {
    use std::cell::Cell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/gnome/Fractal/ui/login/homeserver_page.ui")]
    #[properties(wrapper_type = super::LoginHomeserverPage)]
    pub struct LoginHomeserverPage {
        #[template_child]
        homeserver_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        homeserver_help: TemplateChild<gtk::Label>,
        #[template_child]
        next_button: TemplateChild<LoadingButton>,
        /// Whether auto-discovery is enabled.
        #[property(get, set = Self::set_autodiscovery, explicit_notify, default = true)]
        autodiscovery: Cell<bool>,
        /// Whether this page is in its loading state.
        #[property(get, set = Self::set_is_loading, explicit_notify)]
        is_loading: Cell<bool>,
        /// Sender for the homeserver data.
        pub(super) homeserver_data_sender: Broadcaster<HomeserverData>,
    }

    impl Default for LoginHomeserverPage {
        fn default() -> Self {
            Self {
                homeserver_entry: Default::default(),
                homeserver_help: Default::default(),
                next_button: Default::default(),
                autodiscovery: Cell::new(true),
                is_loading: Cell::new(false),
                homeserver_data_sender: Default::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LoginHomeserverPage {
        const NAME: &'static str = "LoginHomeserverPage";
        type Type = super::LoginHomeserverPage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            OfflineBanner::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
            TemplateCallbacks::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for LoginHomeserverPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.update_text();
        }
    }

    impl WidgetImpl for LoginHomeserverPage {
        fn grab_focus(&self) -> bool {
            self.homeserver_entry.grab_focus()
        }
    }

    impl NavigationPageImpl for LoginHomeserverPage {
        fn shown(&self) {
            self.grab_focus();
        }
    }

    #[gtk::template_callbacks]
    impl LoginHomeserverPage {
        /// Set whether auto-discovery is enabled.
        fn set_autodiscovery(&self, autodiscovery: bool) {
            if self.autodiscovery.get() == autodiscovery {
                return;
            }

            self.autodiscovery.set(autodiscovery);

            self.update_text();
            self.update_next_state();
            self.obj().notify_autodiscovery();
        }

        /// Set the loading state of this page.
        fn set_is_loading(&self, loading: bool) {
            if self.is_loading.get() == loading {
                return;
            }

            self.is_loading.set(loading);

            self.obj().notify_is_loading();
        }

        /// Update the text of this page according to the current settings.
        fn update_text(&self) {
            if self.autodiscovery.get() {
                self.homeserver_entry.set_title(&gettext("Domain Name"));
                self.homeserver_help.set_markup(&gettext(
                    "The domain of your Matrix homeserver, for example gnome.org",
                ));
            } else {
                self.homeserver_entry.set_title(&gettext("Homeserver URL"));
                self.homeserver_help.set_markup(&gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "The URL of your Matrix homeserver, for example {address}",
                    &[(
                        "address",
                        "<span segment=\"word\">https://gnome.modular.im</span>",
                    )],
                ));
            }
        }

        /// The data to use to reach the homeserver.
        ///
        /// Returns `None` if the value in the entry is invalid.
        fn homeserver_data(&self) -> Option<HomeserverData> {
            HomeserverData::parse(&self.homeserver_entry.text(), self.autodiscovery.get())
        }

        /// Open the advanced dialog.
        #[template_callback]
        async fn open_advanced_dialog(&self) {
            let obj = self.obj();
            let dialog = LoginAdvancedDialog::new();

            obj.bind_property("autodiscovery", &dialog, "autodiscovery")
                .sync_create()
                .bidirectional()
                .build();

            dialog.run_future(&*obj).await;
        }

        /// Update the state of the "Next" button.
        #[template_callback]
        fn update_next_state(&self) {
            self.next_button
                .set_sensitive(self.homeserver_data().is_some());
        }

        /// Initiate the homeserver validation to proceed to the next step.
        #[template_callback]
        fn check_homeserver(&self) {
            let Some(data) = self.homeserver_data() else {
                return;
            };

            self.set_is_loading(true);
            self.homeserver_data_sender.send(data);
        }

        /// Reset this page.
        pub(super) fn reset(&self) {
            self.homeserver_entry.set_text("");
            self.set_is_loading(false);
            self.set_autodiscovery(true);
        }
    }
}

glib::wrapper! {
    /// The login page to provide the homeserver settings.
    pub struct LoginHomeserverPage(ObjectSubclass<imp::LoginHomeserverPage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LoginHomeserverPage {
    /// The tag for this page.
    pub(super) const TAG: &str = "homeserver";

    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Connect to changes of the homeserver data.
    pub(super) fn connect_homeserver_data<F>(&self, callback: F)
    where
        F: Fn(HomeserverData) + 'static,
    {
        self.imp().homeserver_data_sender.connect(callback);
    }

    /// Reset this page.
    pub(super) fn reset(&self) {
        self.imp().reset();
    }
}

/// The data to use to reach the homeserver.
#[derive(Debug, Clone)]
pub(super) enum HomeserverData {
    /// The server name to use for auto-discovery.
    ServerName(OwnedServerName),

    /// The URL of the homeserver.
    ///
    /// This means that auto-discovery is disabled.
    Url(Url),
}

impl HomeserverData {
    /// Try to construct an `HomeserverData` with the given string and
    /// auto-discovery setting.
    fn parse(s: &str, autodiscovery: bool) -> Option<Self> {
        if autodiscovery {
            sanitize_server_name(s).ok().map(Self::ServerName)
        } else {
            Url::parse(s).ok().map(Self::Url)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::{HomeserverData, LoginHomeserverPage};
    use crate::utils::tests::load_gresources;

    #[gtk::test]
    fn construct() {
        // Check that the GObject is constructed successfully, i.e. the template file is
        // valid.
        load_gresources();
        LoginHomeserverPage::new();
    }

    #[test]
    fn parse_homeserver_data() {
        assert_matches!(HomeserverData::parse("", true), None);
        assert_matches!(HomeserverData::parse("", false), None);

        assert_matches!(HomeserverData::parse("gnome", true), Some(_));
        assert_matches!(HomeserverData::parse("gnome", false), None);

        assert_matches!(HomeserverData::parse("gnome.org", true), Some(_));
        assert_matches!(HomeserverData::parse("gnome.org", false), None);

        assert_matches!(HomeserverData::parse("https://gnome.org", true), Some(_));
        assert_matches!(HomeserverData::parse("https://gnome.org", false), Some(_));
    }
}
