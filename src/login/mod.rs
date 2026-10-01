use std::net::{Ipv4Addr, Ipv6Addr};

use adw::{prelude::*, subclass::prelude::*};
use as_variant::as_variant;
use gettextrs::gettext;
use gtk::{glib, glib::clone};
use matrix_sdk::{
    Client, ClientBuildError, Error,
    authentication::oauth::{
        ClientRegistrationData,
        registration::{ApplicationType, ClientMetadata, Localized, OAuthGrantType},
    },
    config::RequestConfig,
    utils::local_server::{LocalServerRedirectHandle, QueryString},
};
use ruma::{api::client::session::get_login_types::v3::LoginType, serde::Raw};
use tracing::{error, warn};
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
    in_browser_page::LoginInBrowserPage,
    local_server::spawn_local_server,
    method_page::{LoginMatrixData, LoginMatrixPasswordData, LoginMethodPage, LoginMethodPageData},
    session_setup_view::SessionSetupView,
};
use crate::{
    APP_HOMEPAGE_URL, APP_NAME, Application, RUNTIME, SETTINGS_KEY_CURRENT_SESSION, Window,
    components::OfflineBanner, prelude::*, secret::Secret, session::Session, spawn, spawn_tokio,
    toast, utils::AbortableHandle,
};

/// A page of the login stack.
///
/// The `Ord` implementation follows the order of the pages in the login flow,
/// so it can be used to check its progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
        pub(super) greeter: TemplateChild<Greeter>,
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
        /// A handle to the local server to wait for the redirect in the
        /// browser.
        local_server_handle: RefCell<Option<LocalServerRedirectHandle>>,
        /// The data to use to log in with the browser.
        in_browser_data: RefCell<Option<LoginInBrowserData>>,
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
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Login {
        fn constructed(&self) {
            self.parent_constructed();

            self.navigation.connect_popped(clone!(
                #[weak(rename_to = imp)]
                self,
                move |_, _| {
                    imp.navigation_popped();
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

            self.method_page.connect_login_data(clone!(
                #[weak(rename_to = imp)]
                self,
                move |data| {
                    spawn!(async move {
                        imp.log_in_with_matrix(data).await;
                        imp.method_page.set_is_loading(false);
                    });
                }
            ));

            self.in_browser_page.connect_launch_url(clone!(
                #[weak(rename_to = imp)]
                self,
                move |()| {
                    spawn!(async move {
                        if imp.launch_in_browser_url().await.is_err() {
                            imp.navigation.pop();
                        }
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
        fn visible_page(&self) -> LoginPage {
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
        fn session_setup(&self) -> Option<SessionSetupView> {
            self.navigation
                .find_page(LoginPage::SessionSetup.tag())
                .and_downcast()
        }

        /// Handle when the user went back in the login flow.
        fn navigation_popped(&self) {
            // Abort the ongoing task, if any.
            self.abort_handle.abort();

            let visible_page = self.visible_page();

            if visible_page <= LoginPage::InBrowser {
                // Drop the session because it is bound to the account.
                self.drop_session();
            }

            if visible_page <= LoginPage::Method {
                // Drop the in-browser data because we need to recompute it for retries.
                self.local_server_handle.take();
                self.in_browser_data.take();
            }

            if visible_page <= LoginPage::Homeserver {
                // Drop the client because it is bound to the homeserver.
                self.drop_client();
                // Clear the user credentials for security.
                self.method_page.reset();
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

            self.local_server_handle.replace(Some(local_server_handle));
            self.in_browser_data
                .replace(Some(LoginInBrowserData::OAuth(authorization_data.url)));
            self.navigation.push_by_tag(LoginPage::InBrowser.tag());

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
                let domain_name = self
                    .homeserver_data
                    .borrow()
                    .as_ref()
                    .and_then(|data| as_variant!(data, HomeserverData::ServerName))
                    .cloned();

                let data = LoginMethodPageData {
                    homeserver_url: &client.homeserver(),
                    domain_name: domain_name.as_deref(),
                    supports_sso,
                };

                self.show_method_page(data);
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
                    self.local_server_handle.replace(Some(local_server_handle));
                    self.in_browser_data
                        .replace(Some(LoginInBrowserData::MatrixSso(url)));
                    self.navigation.push_by_tag(LoginPage::InBrowser.tag());
                    Ok(())
                }
                Err(error) => {
                    warn!("Could not build Matrix SSO URL: {error}");
                    Err(())
                }
            }
        }

        /// Show the page to chose a login method with the given data.
        fn show_method_page(&self, data: LoginMethodPageData<'_>) {
            self.method_page.update(data);
            self.navigation.push_by_tag(LoginPage::Method.tag());
        }

        /// Log in with the given Matrix native flow.
        async fn log_in_with_matrix(&self, data: LoginMatrixData) {
            let Some(client) = self.client().await else {
                toast!(self.obj(), gettext("Could not set up login"));
                return;
            };

            match data {
                LoginMatrixData::Password(password_data) => {
                    self.log_in_with_matrix_password(client, password_data)
                        .await;
                }
                LoginMatrixData::Sso => {
                    if self.init_matrix_sso_login(client).await.is_err() {
                        toast!(self.obj(), gettext("Could not set up login"));
                    }
                }
            }
        }

        /// Log in with the Matrix native password flow.
        async fn log_in_with_matrix_password(
            &self,
            client: Client,
            password_data: LoginMatrixPasswordData,
        ) {
            let matrix_auth = client.matrix_auth();
            let handle = spawn_tokio!(async move {
                matrix_auth
                    .login_username(&password_data.username, &password_data.password)
                    .initial_device_display_name(APP_NAME)
                    .send()
                    .await
            });

            let Some(result) = self.abort_handle.await_task(handle).await else {
                // The task was aborted.
                return;
            };

            match result {
                Ok(_) => {
                    let _ = self.create_session(client).await;
                }
                Err(error) => {
                    warn!("Could not log in: {error}");
                    toast!(self.obj(), error.to_user_facing());
                }
            }
        }

        /// Open the current URL to log in with the browser.
        async fn launch_in_browser_url(&self) -> Result<(), ()> {
            let Some(data) = self.in_browser_data.borrow().clone() else {
                error!("Could not launch URI: missing in-browser data");
                toast!(self.obj(), gettext("Could not open URL"));
                return Err(());
            };

            if let Err(error) = gtk::UriLauncher::new(data.url())
                .launch_future(self.obj().root().and_downcast_ref::<gtk::Window>())
                .await
            {
                error!("Could not launch URI: {error}");
                toast!(self.obj(), gettext("Could not open URL"));
                return Err(());
            }

            let Some(local_server_handle) = self.local_server_handle.take() else {
                // If we don't have the server handle, we are already waiting for the redirect.
                return Ok(());
            };

            let handle = spawn_tokio!(async move { local_server_handle.await });

            let Some(result) = self.abort_handle.await_task(handle).await else {
                // The task was aborted.
                return Ok(());
            };

            // This should show a notification for the user to go back to this app.
            if let Some(window) = self.obj().root().and_downcast::<gtk::Window>() {
                window.present();
            }

            let Some(query_string) = result else {
                warn!("Could not log in: missing query string in redirect URI");
                toast!(self.obj(), gettext("An unexpected error occurred."));
                return Err(());
            };

            match &data {
                LoginInBrowserData::OAuth(_) => self.finish_oauth_login(query_string).await,
                LoginInBrowserData::MatrixSso(_) => {
                    self.finish_matrix_sso_login(query_string).await
                }
            }
        }

        /// Finish the OAuth 2.0 login process.
        async fn finish_oauth_login(&self, query_string: QueryString) -> Result<(), ()> {
            let Some(client) = self.client().await else {
                toast!(self.obj(), gettext("Could not set up login"));
                return Err(());
            };

            let oauth = client.oauth();
            let handle = spawn_tokio!(async move { oauth.finish_login(query_string.into()).await });

            let Some(result) = self.abort_handle.await_task(handle).await else {
                // The task was aborted.
                return Ok(());
            };

            match result {
                Ok(()) => self.create_session(client).await,
                Err(error) => {
                    warn!("Could not log in via OAuth 2.0: {error}");
                    toast!(self.obj(), error.to_user_facing());
                    Err(())
                }
            }
        }

        /// Finish the Matrix SSO login process.
        async fn finish_matrix_sso_login(&self, query_string: QueryString) -> Result<(), ()> {
            let Some(client) = self.client().await else {
                toast!(self.obj(), gettext("Could not set up login"));
                return Err(());
            };

            let matrix_auth = client.matrix_auth();
            let handle = spawn_tokio!(async move {
                matrix_auth
                    .login_with_sso_callback(query_string.into())
                    .map_err(|error| Error::UnknownError(error.into()))?
                    .initial_device_display_name(APP_NAME)
                    .await
            });

            let Some(result) = self.abort_handle.await_task(handle).await else {
                // The task was aborted.
                return Ok(());
            };

            match result {
                Ok(_) => self.create_session(client).await,
                Err(error) => {
                    warn!("Could not log in via SSO: {error}");
                    toast!(self.obj(), error.to_user_facing());
                    Err(())
                }
            }
        }

        /// Create the session after a successful login.
        pub(super) async fn create_session(&self, client: Client) -> Result<(), ()> {
            match Session::create(&client).await {
                Ok(session) => {
                    self.init_session(session).await;
                    Ok(())
                }
                Err(error) => {
                    warn!("Could not create session: {error}");
                    toast!(self.obj(), error.to_user_facing());
                    Err(())
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

            self.reset();
        }

        /// Connect to the signal to go back from this view.
        pub(super) fn connect_go_back<F>(&self, callback: F)
        where
            F: Fn() + 'static,
        {
            self.greeter.connect_go_back(clone!(
                #[weak(rename_to = imp)]
                self,
                move || {
                    callback();
                    imp.reset();
                }
            ));
        }

        /// Reset the login view.
        fn reset(&self) {
            // Reset homeserver data and page.
            self.homeserver_data.take();
            self.homeserver_page.reset();

            // Reinitialize UI.
            // This should clean more data with the resulting call to
            // `navigation_popped()`.
            self.navigation.pop_to_tag(LoginPage::Greeter.tag());
        }
    }
}

glib::wrapper! {
    /// A widget managing the login flow.
    pub struct Login(ObjectSubclass<imp::Login>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Login {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Set whether the user can go back from this view.
    pub(super) fn set_can_go_back(&self, can_go_back: bool) {
        self.imp().greeter.set_can_go_back(can_go_back);
    }

    /// Connect to the signal to go back from this view.
    pub(super) fn connect_go_back<F>(&self, callback: F)
    where
        F: Fn() + 'static,
    {
        self.imp().connect_go_back(callback);
    }
}

/// Data for logging in via the browser.
#[derive(Debug, Clone)]
enum LoginInBrowserData {
    /// Log in via the OAuth 2.0 API with the given URL.
    OAuth(Url),

    /// Log in via the Matrix native SSO API with the given URL.
    MatrixSso(String),
}

impl LoginInBrowserData {
    /// Get the URL to open in the browser.
    fn url(&self) -> &str {
        match self {
            Self::OAuth(url) => url.as_str(),
            Self::MatrixSso(url) => url,
        }
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

#[cfg(test)]
mod tests {
    use super::Login;
    use crate::utils::tests::load_gresources;

    #[gtk::test]
    fn construct() {
        // Check that the GObject is constructed successfully, i.e. the template file is
        // valid.
        load_gresources();
        Login::new();
    }
}
