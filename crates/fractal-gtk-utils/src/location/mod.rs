//! Location API.

use std::cfg_select;

use futures_util::Stream;
use geo_uri::GeoUri;

#[cfg(target_os = "linux")]
mod linux;

cfg_select! {
    target_os = "linux" => {
        /// The secret API.
        pub type Location = linux::LinuxLocation;
    }
    _ => {
        /// The secret API.
        pub type Location = unimplemented::UnimplementedLocation;
    }
}

/// Trait implemented by location backends.
pub trait LocationExt {
    /// Whether the location API is available.
    fn is_available(&self) -> bool;

    /// Initialize the location API.
    async fn init(&self) -> Result<(), LocationError>;

    /// Listen to a stream of location updates.
    async fn updates_stream(&self) -> Result<impl Stream<Item = GeoUri> + '_, LocationError>;
}

/// The fallback location API, used on platforms where it is unimplemented.
#[cfg(not(target_os = "linux"))]
mod unimplemented {
    use super::*;

    #[derive(Debug)]
    pub struct UnimplementedLocation;

    impl LocationExt for UnimplementedLocation {
        /// Whether the location API is available.
        fn is_available(&self) -> bool {
            false
        }

        /// Initialize the location API.
        async fn init(&self) -> Result<(), LocationError> {
            unimplemented!()
        }

        /// Listen to a stream of location updates.
        async fn updates_stream(&self) -> Result<impl Stream<Item = GeoUri> + '_, LocationError> {
            unimplemented!()
        }
    }
}

/// High-level errors that can occur while fetching the location.
#[derive(Debug, Clone, Copy)]
pub enum LocationError {
    /// The user cancelled the request to get the location.
    Cancelled,
    /// The location services are disabled on the system.
    Disabled,
    /// Another error occurred.
    Other,
}
