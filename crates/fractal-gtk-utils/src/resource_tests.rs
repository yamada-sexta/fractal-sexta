//! Collection of test helpers.

use gtk::gio;
use tracing::debug;

const BUILD_DIR: &str = env!("FRACTAL_BUILD_DIR");

/// Load all the `GResource` files of the application.
///
/// For this to work, meson must be used to generate those files first, this is
/// the default behavior with the `cargo-test` target.
pub fn load_gresources() {
    let gresource_files = &[
        "data/resources/resources.gresource",
        "crates/fractal-app/src/ui-resources.gresource",
    ];

    for file in gresource_files {
        let path = format!("{BUILD_DIR}/{file}");
        debug!(path, "Loading gresource file");

        let resource = gio::Resource::load(path).expect("Could not load gresource file");
        gio::resources_register(&resource);
    }
}
