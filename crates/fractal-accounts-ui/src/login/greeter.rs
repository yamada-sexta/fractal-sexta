use adw::{prelude::*, subclass::prelude::*};
use gtk::glib;

use crate::{components::OfflineBanner, utils::Broadcaster};

mod imp {
    use std::cell::Cell;

    use glib::subclass::InitializingObject;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/org/gnome/Fractal/ui/login/greeter.ui")]
    #[properties(wrapper_type = super::Greeter)]
    pub struct Greeter {
        /// Whether the user can go back from the login flow.
        #[property(get, set = Self::set_can_go_back, explicit_notify)]
        can_go_back: Cell<bool>,
        /// Sender for the signal to go back from the login flow.
        pub(super) go_back_sender: Broadcaster<()>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Greeter {
        const NAME: &'static str = "Greeter";
        type Type = super::Greeter;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            OfflineBanner::ensure_type();

            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);

            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }

        fn instance_init(obj: &InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Greeter {}

    impl WidgetImpl for Greeter {}

    impl NavigationPageImpl for Greeter {
        fn shown(&self) {
            self.grab_focus();
        }
    }

    impl AccessibleImpl for Greeter {}

    #[gtk::template_callbacks]
    impl Greeter {
        /// Set whether the user can go back from the login view.
        fn set_can_go_back(&self, can_go_back: bool) {
            if self.can_go_back.get() == can_go_back {
                return;
            }

            self.can_go_back.set(can_go_back);

            self.obj().notify_can_go_back();
        }

        /// Go back from the login view.
        #[template_callback]
        fn go_back(&self) {
            if self.can_go_back.get() {
                self.go_back_sender.send(());
            }
        }
    }
}

glib::wrapper! {
    /// The welcome screen of the app.
    pub struct Greeter(ObjectSubclass<imp::Greeter>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Greeter {
    /// The tag for this page.
    pub(super) const TAG: &str = "greeter";

    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Connect to the signal to go back from the login view.
    pub(super) fn connect_go_back<F>(&self, callback: F)
    where
        F: Fn() + 'static,
    {
        self.imp().go_back_sender.connect(move |()| callback());
    }
}

#[cfg(test)]
mod tests {
    use super::Greeter;
    use crate::utils::tests::load_gresources;

    #[gtk::test]
    fn construct() {
        // Check that the GObject is constructed successfully, i.e. the template file is
        // valid.
        load_gresources();
        Greeter::new();
    }
}
