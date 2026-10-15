#![recursion_limit = "256"]
pub mod animated_image_paintable;
pub mod ext_traits;
pub mod media;
pub mod media_message;
pub use fractal_config::DISABLE_GLYCIN_SANDBOX;
pub use fractal_core::{RUNTIME, gettext_f, spawn, spawn_tokio};
pub mod components {
    pub use crate::{animated_image_paintable::AnimatedImagePaintable, media_message::ContentType};
}
pub mod utils {
    pub use fractal_core::*;
    pub use fractal_gtk_utils::*;

    pub use crate::media;
    pub mod matrix {
        pub use crate::{ext_traits::*, media_message::*};
    }
}
pub mod prelude {
    pub use fractal_core::string::{OptionStringExt, StrExt, StrMutExt};

    pub use crate::{ext_traits::*, media::image::GlycinFrameExt, media_message::*};
}
