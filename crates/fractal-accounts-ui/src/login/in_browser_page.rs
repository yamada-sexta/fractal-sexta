use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::{components::OfflineBanner, utils::Broadcaster};

mod imp {
    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/gnome/Fractal/ui/login/in_browser_page.ui")]
    #[properties(wrapper_type = super::LoginInBrowserPage)]
    pub struct LoginInBrowserPage {
        #[template_child]
        continue_btn: TemplateChild<gtk::Button>,
        /// Sender for the signal to open the URL in a browser.
        pub(super) launch_url_sender: Broadcaster<()>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LoginInBrowserPage {
        const NAME: &'static str = "LoginInBrowserPage";
        type Type = super::LoginInBrowserPage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            OfflineBanner::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for LoginInBrowserPage {}

    impl WidgetImpl for LoginInBrowserPage {
        fn grab_focus(&self) -> bool {
            self.continue_btn.grab_focus()
        }
    }

    impl NavigationPageImpl for LoginInBrowserPage {
        fn shown(&self) {
            self.grab_focus();
        }
    }

    #[gtk::template_callbacks]
    impl LoginInBrowserPage {
        /// Launch the URL to log in with the browser.
        #[template_callback]
        fn launch_url(&self) {
            self.launch_url_sender.send(());
        }
    }
}

glib::wrapper! {
    /// A page to log the user in via the browser.
    pub struct LoginInBrowserPage(ObjectSubclass<imp::LoginInBrowserPage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl LoginInBrowserPage {
    /// The tag for this page.
    pub(super) const TAG: &str = "in-browser";

    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Connect to the signal to open the URL in a browser.
    pub(super) fn connect_launch_url<F>(&self, callback: F)
    where
        F: Fn(()) + 'static,
    {
        self.imp().launch_url_sender.connect(callback);
    }
}

#[cfg(test)]
mod tests {
    use super::LoginInBrowserPage;
    use crate::utils::tests::load_gresources;

    #[gtk::test]
    fn construct() {
        // Check that the GObject is constructed successfully, i.e. the template file is
        // valid.
        load_gresources();
        LoginInBrowserPage::new();
    }
}
