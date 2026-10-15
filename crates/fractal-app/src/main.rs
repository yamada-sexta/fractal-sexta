#![doc(
    html_logo_url = "https://raw.githubusercontent.com/yamada-sexta/fractal-sexta/main/data/icons/org.gnome.Fractal.svg",
    html_favicon_url = "https://raw.githubusercontent.com/yamada-sexta/fractal-sexta/main/data/icons/org.gnome.Fractal-symbolic.svg"
)]
#![recursion_limit = "256"]

mod application;
mod config;
mod error_page;
mod window;
pub use application::*;
pub use config::*;
pub use fractal_accounts_ui::{account_chooser_dialog, account_settings, login};
pub use fractal_backend::{intent, secret, session, session_list};
pub use fractal_core::{
    MEDIA_FILE_NOTIFIER, RUNTIME, gettext_f, i18n, ngettext_f, spawn, spawn_tokio,
    user_facing_error,
};
pub use fractal_gtk_utils::{contrib, system_settings};
pub use fractal_session_ui::session_view;
pub use fractal_verification_ui::identity_verification_view;
pub use fractal_widgets::{account_switcher, components, prelude, toast, utils};
use gettextrs::*;
use gtk::{IconTheme, gdk::Display, gio};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
pub use window::Window;

fn main() {
    // Initialize logger, debug is carried out via debug!, info!, warn! and error!.
    // Default to the INFO level for this crate and WARN for everything else.
    // It can be overridden with the RUST_LOG environment variable.
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("fractal=info,warn"));

    tracing_subscriber::registry()
        .with(fmt::layer().with_filter(env_filter))
        .init();

    // Prepare i18n
    // Safety: `setlocale` is safe to call because the program is single-threaded.
    unsafe { setlocale(LocaleCategory::LcAll, "") };
    bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR).expect("Invalid argument passed to bindtextdomain");
    textdomain(GETTEXT_PACKAGE).expect("Invalid string passed to textdomain");

    gtk::glib::set_application_name(APP_NAME);

    gtk::init().expect("Could not start GTK4");
    gst::init().expect("Could not initialize gst");

    #[cfg(target_os = "linux")]
    aperture::init(APP_ID);

    let res = gio::Resource::load(RESOURCES_FILE).expect("Could not load gresource file");
    gio::resources_register(&res);
    let ui_res = gio::Resource::load(UI_RESOURCES_FILE).expect("Could not load UI gresource file");
    gio::resources_register(&ui_res);

    IconTheme::for_display(&Display::default().unwrap())
        .add_resource_path("/org/gnome/Fractal/icons");

    let app = Application::new();
    app.run();
}
