use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::glib;
use ruma::ServerName;
use url::Url;

use crate::{
    components::{LoadingButton, OfflineBanner},
    gettext_f,
    utils::{Broadcaster, TemplateCallbacks},
};

mod imp {
    use std::cell::Cell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/gnome/Fractal/ui/login/method_page.ui")]
    #[properties(wrapper_type = super::LoginMethodPage)]
    pub struct LoginMethodPage {
        #[template_child]
        title: TemplateChild<gtk::Label>,
        #[template_child]
        homeserver_url: TemplateChild<gtk::Label>,
        #[template_child]
        username_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        password_entry: TemplateChild<adw::PasswordEntryRow>,
        #[template_child]
        sso_button: TemplateChild<gtk::Button>,
        #[template_child]
        next_button: TemplateChild<LoadingButton>,
        /// Whether this page is in its loading state.
        #[property(get, set = Self::set_is_loading, explicit_notify)]
        is_loading: Cell<bool>,
        /// Sender for the login data.
        pub(super) login_data_sender: Broadcaster<LoginMatrixData>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LoginMethodPage {
        const NAME: &'static str = "LoginMethodPage";
        type Type = super::LoginMethodPage;
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
    impl ObjectImpl for LoginMethodPage {}

    impl WidgetImpl for LoginMethodPage {
        fn grab_focus(&self) -> bool {
            self.username_entry.grab_focus()
        }
    }

    impl NavigationPageImpl for LoginMethodPage {
        fn shown(&self) {
            self.grab_focus();
        }
    }

    #[gtk::template_callbacks]
    impl LoginMethodPage {
        /// Set the loading state of this page.
        fn set_is_loading(&self, loading: bool) {
            if self.is_loading.get() == loading {
                return;
            }

            self.is_loading.set(loading);

            self.obj().notify_is_loading();
        }

        /// Update this page with the given data.
        pub(super) fn update(&self, data: LoginMethodPageData<'_>) {
            let title = if let Some(domain_name) = data.domain_name {
                gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "Log in to {domain_name}",
                    &[(
                        "domain_name",
                        &format!("<span segment=\"word\">{domain_name}</span>"),
                    )],
                )
            } else {
                gettext("Log in")
            };
            self.title.set_markup(&title);

            let homeserver_url = data.homeserver_url.as_str().trim_end_matches('/');
            self.homeserver_url.set_label(homeserver_url);

            self.sso_button.set_visible(data.supports_sso);

            self.update_next_state();
        }

        /// The data for logging in via the password flow.
        ///
        /// Returns `None` if the data is invalid.
        fn password_data(&self) -> Option<LoginMatrixPasswordData> {
            LoginMatrixPasswordData::new(self.username_entry.text(), self.password_entry.text())
        }

        /// Update the state of the "Next" button.
        #[template_callback]
        fn update_next_state(&self) {
            self.next_button
                .set_sensitive(self.password_data().is_some());
        }

        /// Log in with the password login flow.
        #[template_callback]
        fn log_in_with_password(&self) {
            let Some(password_data) = self.password_data() else {
                return;
            };

            self.set_is_loading(true);
            self.login_data_sender
                .send(LoginMatrixData::Password(password_data));
        }

        /// Log in with the SSO login flow.
        #[template_callback]
        fn log_in_with_sso(&self) {
            self.login_data_sender.send(LoginMatrixData::Sso);
        }

        /// Reset this page.
        pub(super) fn reset(&self) {
            self.username_entry.set_text("");
            self.password_entry.set_text("");
            self.set_is_loading(false);
        }
    }
}

glib::wrapper! {
    /// The login page allowing to login via password or to choose a SSO provider.
    pub struct LoginMethodPage(ObjectSubclass<imp::LoginMethodPage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LoginMethodPage {
    /// The tag for this page.
    pub(super) const TAG: &str = "method";

    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Update this page with the given data.
    pub(super) fn update(&self, data: LoginMethodPageData<'_>) {
        self.imp().update(data);
    }

    /// Connect to changes of the login data.
    pub(super) fn connect_login_data<F>(&self, callback: F)
    where
        F: Fn(LoginMatrixData) + 'static,
    {
        self.imp().login_data_sender.connect(callback);
    }

    /// Reset this page.
    pub(super) fn reset(&self) {
        self.imp().reset();
    }
}

/// The data necessary to populate the [`LoginMethodPage`].
#[derive(Debug, Clone, Copy)]
pub(super) struct LoginMethodPageData<'a> {
    /// The URL used to connect to the homeserver.
    pub(super) homeserver_url: &'a Url,

    /// The domain name of the homeserver, if it is known.
    pub(super) domain_name: Option<&'a ServerName>,

    /// Whether the homeserver supports login via SSO.
    pub(super) supports_sso: bool,
}

/// The data to use to log in with the homeserver using one of the native Matrix
/// flows.
#[derive(Debug, Clone)]
pub(super) enum LoginMatrixData {
    /// Log in using the password flow.
    Password(LoginMatrixPasswordData),

    /// Log in using the SSO flow.
    Sso,
}

/// The data to use to log in with the homeserver via the password flow.
#[derive(Debug, Clone)]
pub(super) struct LoginMatrixPasswordData {
    /// The username.
    pub(super) username: glib::GString,

    /// The password.
    pub(super) password: glib::GString,
}

impl LoginMatrixPasswordData {
    /// Construct the data from the given username and password.
    ///
    /// Returns `None` if either the username or password is invalid.
    fn new(username: glib::GString, password: glib::GString) -> Option<Self> {
        if username.is_empty() || password.is_empty() {
            return None;
        }

        Some(Self { username, password })
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use gtk::glib::gformat;

    use super::{LoginMatrixPasswordData, LoginMethodPage};
    use crate::utils::tests::load_gresources;

    #[gtk::test]
    fn construct() {
        // Check that the GObject is constructed successfully, i.e. the template file is
        // valid.
        load_gresources();
        LoginMethodPage::new();
    }

    #[test]
    fn construct_matrix_login_password_data() {
        assert_matches!(
            LoginMatrixPasswordData::new(gformat!(""), gformat!("pass")),
            None
        );
        assert_matches!(
            LoginMatrixPasswordData::new(gformat!("user"), gformat!("")),
            None
        );
        assert_matches!(
            LoginMatrixPasswordData::new(gformat!("user"), gformat!("pass")),
            Some(_)
        );
    }
}
