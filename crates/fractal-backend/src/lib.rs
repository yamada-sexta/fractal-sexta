#![recursion_limit = "256"]
pub mod intent;
pub mod secret;
pub mod session;
pub mod session_list;
pub mod components {
    pub mod avatar {
        pub mod data;
        pub mod image;
    }
    pub mod pill {
        pub mod at_room;
        pub mod source;
    }
    pub use avatar::{
        data::AvatarData,
        image::{AvatarImage, AvatarUriSource},
    };
    pub use fractal_gtk_utils::camera::{Camera, CameraExt, QrCodeScanner};
    pub use pill::{at_room::AtRoom, source::*};
}
pub mod utils;
pub use fractal_config::*;
pub use fractal_core::{RUNTIME, gettext_f, ngettext_f, spawn, spawn_tokio, user_facing_error};
pub use fractal_gtk_utils::system_settings;
pub mod prelude {
    pub use fractal_core::{
        string::{OptionStringExt, StrExt, StrMutExt},
        user_facing_error::UserFacingError,
    };
    pub use fractal_gtk_utils::LocationExt;
    pub use fractal_media::media::image::GlycinFrameExt;

    pub use crate::{
        components::{CameraExt, PillSourceExt, PillSourceImpl},
        secret::SecretExt,
        session::{TimelineItemExt, UserExt},
        session_list::SessionInfoExt,
        utils::matrix::ext_traits::*,
    };
}
