use std::net::{Ipv4Addr, Ipv6Addr};

use adw::{prelude::*, subclass::prelude::*};
use as_variant::as_variant;
use gettextrs::gettext;
use gtk::{gio, glib, glib::clone};
use matrix_sdk::{
    Client, ClientBuildError,
    authentication::oauth::{
        ClientRegistrationData,
        registration::{ApplicationType, ClientMetadata, Localized, OAuthGrantType},
    },
    config::RequestConfig,
    utils::local_server::LocalServerRedirectHandle,
};
use ruma::{OwnedServerName, api::client::session::get_login_types::v3::LoginType, serde::Raw};
use tracing::warn;
use url::Url;

mod advanced_dialog;
mod greeter;
mod homeserver_page;
mod in_browser_page;
mod local_server;
mod method_page;
mod session_setup_view;

use self::{
    greeter::Greeter,
    homeserver_page::{HomeserverData, LoginHomeserverPage},
    in_browser_page::{LoginInBrowserData, LoginInBrowserPage},
    local_server::spawn_local_server,
    method_page::LoginMethodPage,
    session_setup_view::SessionSetupView,
};
use crate::{
    APP_HOMEPAGE_URL, APP_NAME, Application, RUNTIME, SETTINGS_KEY_CURRENT_SESSION, Window,
    components::OfflineBanner, prelude::*, secret::Secret, session::Session, spawn, spawn_tokio,
    toast, utils::AbortableHandle,
};

/// A page of the login stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginPage {
    /// The greeter page.
    Greeter,
    /// The homeserver page.
    Homeserver,
    /// The page to select a login method.
    Method,
    /// The page to log in with the browser.
    InBrowser,
    /// The session setup stack.
    SessionSetup,
    /// The login is completed.
    Completed,
}

impl LoginPage {
    /// Get the tag for this page.
    const fn tag(self) -> &'static str {
        match self {
            Self::Greeter => Greeter::TAG,
            Self::Homeserver => LoginHomeserverPage::TAG,
            Self::Method => LoginMethodPage::TAG,
            Self::InBrowser => LoginInBrowserPage::TAG,
            Self::SessionSetup => SessionSetupView::TAG,
            Self::Completed => "completed",
        }
    }

    /// Get the page matching the given tag.
    ///
    /// Panics if the tag does not match any of the variants.
    fn from_tag(tag: &str) -> Self {
        match tag {
            Greeter::TAG => Self::Greeter,
            LoginHomeserverPage::TAG => Self::Homeserver,
            LoginMethodPage::TAG => Self::Method,
            LoginInBrowserPage::TAG => Self::InBrowser,
            SessionSetupView::TAG => Self::SessionSetup,
            "completed" => Self::Completed,
            _ => panic!("Unknown LoginPage: {tag}"),
        }
    }
}

mod imp {
    use std::cell::RefCell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/gnome/Fractal/ui/login/mod.ui")]
    #[properties(wrapper_type = super::Login)]
    pub struct Login {
        #[template_child]
        navigation: TemplateChild<adw::NavigationView>,
        #[template_child]
        greeter: TemplateChild<Greeter>,
        #[template_child]
        homeserver_page: TemplateChild<LoginHomeserverPage>,
        #[template_child]
        method_page: TemplateChild<LoginMethodPage>,
        #[template_child]
        in_browser_page: TemplateChild<LoginInBrowserPage>,
        #[template_child]
        done_button: TemplateChild<gtk::Button>,
        /// The data to use to reach the homeserver.
        homeserver_data: RefCell<Option<HomeserverData>>,
        /// The Matrix client used to log in.
        client: RefCell<Option<Client>>,
        /// The session that was just logged in.
        session: RefCell<Option<Session>>,
        /// An abort handle for the ongoing action.
        abort_handle: AbortableHandle,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Login {
        const NAME: &'static str = "Login";
        type Type = super::Login;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            OfflineBanner::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_css_name("login");
            klass.set_accessible_role(gtk::AccessibleRole::Group);

            klass.install_action_async("login.sso", None, |obj, _, _| async move {
                let imp = obj.imp();

                let Some(client) = imp.client().await else {
                    toast!(obj, gettext("Could not set up login"));
                    return;
                };

                if imp.init_matrix_sso_login(client).await.is_err() {
                    toast!(obj, gettext("Could not set up login"));
                }
            });
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Login {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            let monitor = gio::NetworkMonitor::default();
            monitor.connect_network_changed(clone!(
                #[weak]
                obj,
                move |_, available| {
                    obj.action_set_enabled("login.sso", available);
                }
            ));
            obj.action_set_enabled("login.sso", monitor.is_network_available());

            self.navigation.connect_visible_page_notify(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.visible_page_changed();
                }
            ));

            self.homeserver_page.connect_homeserver_data(clone!(
                #[weak(rename_to = imp)]
                self,
                move |data| {
                    spawn!(async move {
                        imp.check_homeserver_data(data).await;
                        imp.homeserver_page.set_is_loading(false);
                    });
                }
            ));
        }

        fn dispose(&self) {
            self.drop_client();
            self.drop_session();
        }
    }

    impl WidgetImpl for Login {
        fn grab_focus(&self) -> bool {
            match self.visible_page() {
                LoginPage::Greeter => self.greeter.grab_focus(),
                LoginPage::Homeserver => self.homeserver_page.grab_focus(),
                LoginPage::Method => self.method_page.grab_focus(),
                LoginPage::InBrowser => self.in_browser_page.grab_focus(),
                LoginPage::SessionSetup => {
                    if let Some(session_setup) = self.session_setup() {
                        session_setup.grab_focus()
                    } else {
                        false
                    }
                }
                LoginPage::Completed => self.done_button.grab_focus(),
            }
        }
    }

    impl BinImpl for Login {}
    impl AccessibleImpl for Login {}

    #[gtk::template_callbacks]
    impl Login {
        /// The visible page of the view.
        pub(super) fn visible_page(&self) -> LoginPage {
            LoginPage::from_tag(
                &self
                    .navigation
                    .visible_page()
                    .expect("Login navigation view should always have a visible page")
                    .tag()
                    .expect("Login navigation page should always have a tag"),
            )
        }

        /// Get the session setup view, if any.
        pub(super) fn session_setup(&self) -> Option<SessionSetupView> {
            self.navigation
                .find_page(LoginPage::SessionSetup.tag())
                .and_downcast()
        }

        /// The visible page changed.
        fn visible_page_changed(&self) {
            // Abort the ongoing task, if any. There should not be any ongoing task while we
            // are going forward, so if a task is aborted it means that we are going
            // backward.
            self.abort_handle.abort();

            match self.visible_page() {
                LoginPage::Homeserver => {
                    // Drop the client because it is bound to the homeserver.
                    self.drop_client();
                    // Drop the session because it is bound to the homeserver and account.
                    self.drop_session();
                    self.method_page.clean();
                }
                LoginPage::Method => {
                    // Drop the session because it is bound to the account.
                    self.drop_session();
                }
                _ => {}
            }
        }

        /// Try to build a client with the current homeserver.
        ///
        /// Returns `Ok(None)` if the task was aborted.
        async fn build_client(&self) -> Result<Option<Client>, ClientBuildError> {
            let Some(homeserver_data) = self.homeserver_data.borrow().clone() else {
                return Err(ClientBuildError::MissingHomeserver);
            };

            let handle = spawn_tokio!(async move {
                let mut builder =
                    Client::builder().request_config(RequestConfig::new().retry_limit(2));
                let mut check_homeserver_manually = false;

                match homeserver_data {
                    HomeserverData::ServerName(server_name) => {
                        builder = builder.server_name_or_homeserver_url(server_name);
                    }
                    HomeserverData::Url(url) => {
                        builder = builder.respect_login_well_known(false).homeserver_url(url);
                        check_homeserver_manually = true;
                    }
                }

                let client = builder.build().await?;

                if check_homeserver_manually {
                    // Call the `GET /versions` endpoint to make sure that the URL belongs to a
                    // Matrix homeserver.
                    client.server_versions().await?;
                }

                Ok(client)
            });

            let Some(result) = self.abort_handle.await_task(handle).await else {
                // The task was aborted.
                return Ok(None);
            };

            match result {
                Ok(client) => {
                    self.client.replace(Some(client.clone()));
                    Ok(Some(client))
                }
                Err(error) => {
                    warn!("Could not discover homeserver: {error}");
                    Err(error)
                }
            }
        }

        /// The Matrix client.
        pub(super) async fn client(&self) -> Option<Client> {
            if let Some(client) = self.client.borrow().clone() {
                return Some(client);
            }

            // If the client was dropped, try to recreate it.
            self.build_client().await.ok().flatten()
        }

        /// Drop the Matrix client.
        fn drop_client(&self) {
            if let Some(client) = self.client.take() {
                // The `Client` needs to access a tokio runtime when it is dropped.
                let _guard = RUNTIME.enter();
                drop(client);
            }
        }

        /// Drop the session and clean up its data from the system.
        fn drop_session(&self) {
            if let Some(session) = self.session.take() {
                spawn!(async move {
                    let _ = session.log_out().await;
                });
            }
        }

        /// Spawn a local server for listening to redirects.
        ///
        /// Returns `Ok(None)` if the task was aborted.
        async fn spawn_local_server(&self) -> Result<Option<(Url, LocalServerRedirectHandle)>, ()> {
            let handle = spawn_tokio!(async move { spawn_local_server().await });

            let Some(result) = self.abort_handle.await_task(handle).await else {
                // The task was aborted.
                return Ok(None);
            };

            result.map(Some)
        }

        /// Check if the homeserver data that was entered by the user allows us
        /// to log in.
        async fn check_homeserver_data(&self, homeserver_data: HomeserverData) {
            self.homeserver_data.replace(Some(homeserver_data));

            let client = match self.build_client().await {
                Ok(Some(client)) => client,
                Ok(None) => {
                    // The task was aborted.
                    return;
                }
                Err(error) => {
                    toast!(self.obj(), error.to_user_facing());
                    return;
                }
            };

            if self.discover_login_api(client).await.is_err() {
                toast!(self.obj(), gettext("Could not set up login"));
            }
        }

        /// Discover the login API supported by the homeserver.
        async fn discover_login_api(&self, client: Client) -> Result<(), ()> {
            // First, check if the server supports the OAuth 2.0 API.
            let oauth = client.oauth();
            let handle = spawn_tokio!(async move { oauth.server_metadata().await });

            let Some(result) = self.abort_handle.await_task(handle).await else {
                // The task was aborted.
                return Ok(());
            };

            match result {
                Ok(_) => {
                    // Prefer the OAuth 2.0 API.
                    self.init_oauth_login(client).await
                }
                Err(error) => {
                    if error.is_not_supported() {
                        // Fallback to the Matrix native API.
                        self.init_matrix_login(client).await
                    } else {
                        warn!("Could not get authorization server metadata: {error}");
                        Err(())
                    }
                }
            }
        }

        /// Prepare to log in via the OAuth 2.0 API.
        async fn init_oauth_login(&self, client: Client) -> Result<(), ()> {
            let Some((redirect_uri, local_server_handle)) = self.spawn_local_server().await? else {
                // The task was aborted.
                return Ok(());
            };

            let oauth = client.oauth();
            let handle = spawn_tokio!(async move {
                oauth
                    .login(redirect_uri, None, Some(client_registration_data()), None)
                    .build()
                    .await
            });

            let Some(result) = self.abort_handle.await_task(handle).await else {
                // The task was aborted.
                return Ok(());
            };

            let authorization_data = match result {
                Ok(authorization_data) => authorization_data,
                Err(error) => {
                    warn!("Could not construct OAuth 2.0 authorization URL: {error}");
                    return Err(());
                }
            };

            self.show_in_browser_page(
                local_server_handle,
                LoginInBrowserData::Oauth(authorization_data),
            );

            Ok(())
        }

        /// Prepare to log in via the Matrix native API.
        async fn init_matrix_login(&self, client: Client) -> Result<(), ()> {
            let matrix_auth = client.matrix_auth();
            let handle = spawn_tokio!(async move { matrix_auth.get_login_types().await });

            let Some(result) = self.abort_handle.await_task(handle).await else {
                // The task was aborted.
                return Ok(());
            };

            let login_types = match result {
                Ok(response) => response.flows,
                Err(error) => {
                    warn!("Could not get available Matrix login types: {error}");
                    return Err(());
                }
            };

            let supports_password = login_types
                .iter()
                .any(|login_type| matches!(login_type, LoginType::Password(_)));
            let supports_sso = login_types
                .iter()
                .any(|login_type| matches!(login_type, LoginType::Sso(_)));

            if supports_password {
                let server_name = self
                    .homeserver_data
                    .borrow()
                    .as_ref()
                    .and_then(|data| as_variant!(data, HomeserverData::ServerName))
                    .cloned();

                self.show_method_page(&client.homeserver(), server_name.as_ref(), supports_sso);
                Ok(())
            } else {
                self.init_matrix_sso_login(client).await
            }
        }

        /// Prepare to log in via the Matrix SSO API.
        async fn init_matrix_sso_login(&self, client: Client) -> Result<(), ()> {
            let Some((redirect_uri, local_server_handle)) = self.spawn_local_server().await? else {
                // The task was aborted.
                return Ok(());
            };

            let matrix_auth = client.matrix_auth();
            let handle = spawn_tokio!(async move {
                matrix_auth
                    .get_sso_login_url(redirect_uri.as_str(), None)
                    .await
            });

            let Some(result) = self.abort_handle.await_task(handle).await else {
                // The task was aborted.
                return Ok(());
            };

            match result {
                Ok(url) => {
                    let url = Url::parse(&url).expect("Matrix SSO URL should be a valid URL");
                    self.show_in_browser_page(local_server_handle, LoginInBrowserData::Matrix(url));
                    Ok(())
                }
                Err(error) => {
                    warn!("Could not build Matrix SSO URL: {error}");
                    Err(())
                }
            }
        }

        /// Show the page to chose a login method with the given data.
        fn show_method_page(
            &self,
            homeserver: &Url,
            server_name: Option<&OwnedServerName>,
            supports_sso: bool,
        ) {
            self.method_page
                .update(homeserver, server_name, supports_sso);
            self.navigation.push_by_tag(LoginPage::Method.tag());
        }

        /// Show the page to log in with the browser with the given data.
        fn show_in_browser_page(
            &self,
            local_server_handle: LocalServerRedirectHandle,
            data: LoginInBrowserData,
        ) {
            self.in_browser_page.set_up(local_server_handle, data);
            self.navigation.push_by_tag(LoginPage::InBrowser.tag());
        }

        /// Create the session after a successful login.
        pub(super) async fn create_session(&self) {
            let client = self.client().await.expect("client should be constructed");

            match Session::create(&client).await {
                Ok(session) => {
                    self.init_session(session).await;
                }
                Err(error) => {
                    warn!("Could not create session: {error}");
                    toast!(self.obj(), error.to_user_facing());

                    self.navigation.pop();
                }
            }
        }

        /// Initialize the given session.
        async fn init_session(&self, session: Session) {
            let setup_view = SessionSetupView::new(&session);
            setup_view.connect_completed(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_| {
                    imp.navigation.push_by_tag(LoginPage::Completed.tag());
                }
            ));
            self.navigation.push(&setup_view);

            self.drop_client();
            self.session.replace(Some(session.clone()));

            // Save ID of logging in session to GSettings
            let settings = Application::default().settings();
            if let Err(err) =
                settings.set_string(SETTINGS_KEY_CURRENT_SESSION, session.session_id())
            {
                warn!("Could not save current session: {err}");
            }

            let session_info = session.info().clone();

            if Secret::store_session(session_info).await.is_err() {
                toast!(self.obj(), gettext("Could not store session"));
            }

            session.prepare().await;
        }

        /// Finish the login process and show the session.
        #[template_callback]
        fn finish_login(&self) {
            let Some(window) = self.obj().root().and_downcast::<Window>() else {
                return;
            };

            if let Some(session) = self.session.take() {
                window.add_session(session);
            }

            self.clean();
        }

        /// Reset the login stack.
        pub(super) fn clean(&self) {
            // Clean pages.
            self.homeserver_page.reset();
            self.method_page.clean();

            // Clean data.
            self.drop_client();
            self.drop_session();

            // Reinitialize UI.
            self.navigation.pop_to_tag(LoginPage::Greeter.tag());
            self.unfreeze();
        }

        /// Freeze the login screen.
        pub(super) fn freeze(&self) {
            self.navigation.set_sensitive(false);
        }

        /// Unfreeze the login screen.
        pub(super) fn unfreeze(&self) {
            self.navigation.set_sensitive(true);
        }
    }
}

glib::wrapper! {
    /// A widget managing the login flows.
    pub struct Login(ObjectSubclass<imp::Login>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Login {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The Matrix client.
    async fn client(&self) -> Option<Client> {
        self.imp().client().await
    }

    /// Freeze the login screen.
    fn freeze(&self) {
        self.imp().freeze();
    }

    /// Unfreeze the login screen.
    fn unfreeze(&self) {
        self.imp().unfreeze();
    }

    /// Create the session after a successful login.
    async fn create_session(&self) {
        self.imp().create_session().await;
    }
}

/// Client registration data for the OAuth 2.0 API.
fn client_registration_data() -> ClientRegistrationData {
    // Register the IPv4 and IPv6 localhost APIs as we use a local server for the
    // redirection.
    let ipv4_localhost_uri = Url::parse(&format!("http://{}/", Ipv4Addr::LOCALHOST))
        .expect("IPv4 localhost address should be a valid URL");
    let ipv6_localhost_uri = Url::parse(&format!("http://[{}]/", Ipv6Addr::LOCALHOST))
        .expect("IPv6 localhost address should be a valid URL");

    let client_uri =
        Url::parse(APP_HOMEPAGE_URL).expect("application homepage URL should be a valid URL");

    let mut client_metadata = ClientMetadata::new(
        ApplicationType::Native,
        vec![OAuthGrantType::AuthorizationCode {
            redirect_uris: vec![ipv4_localhost_uri, ipv6_localhost_uri],
        }],
        Localized::new(client_uri, None),
    );
    client_metadata.client_name = Some(Localized::new(APP_NAME.to_owned(), None));

    Raw::new(&client_metadata)
        .expect("client metadata should serialize to JSON successfully")
        .into()
}
