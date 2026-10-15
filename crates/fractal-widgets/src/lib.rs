#![recursion_limit = "256"]
pub use fractal_backend::{intent, secret, session, session_list};
pub use fractal_config::*;
pub use fractal_core::{
    MEDIA_FILE_NOTIFIER, RUNTIME, gettext_f, i18n, ngettext_f, spawn, spawn_tokio,
    user_facing_error,
};
pub use fractal_gtk_utils::{contrib, system_settings};
pub mod account_switcher;
pub mod components;
pub mod media_viewer;
pub mod utils;
pub mod session_view {
    pub use crate::media_viewer::MediaViewer;
}
pub mod prelude {
    pub use fractal_backend::prelude::*;
    pub use fractal_core::{ChildPropertyExt, IsABin};

    pub use crate::{
        components::{
            CameraExt, ContextMenuBinExt, ContextMenuBinImpl, PillWidgetExt, ToastableDialogExt,
            ToastableDialogImpl,
        },
        utils::{
            matrix::{MatrixIdUriWidgetExt, MediaMessageSaveExt},
            string::{LinkifierWidgetExt, PangoMentionExt},
        },
    };
}

pub mod navigation;
