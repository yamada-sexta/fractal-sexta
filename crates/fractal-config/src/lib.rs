#![recursion_limit = "256"]
use std::{borrow::Cow, fmt};
/// The profile that was built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum AppProfile {
    /// A stable release.
    Stable,
    /// A beta release.
    Beta,
    /// A development release.
    Devel,
}

impl AppProfile {
    /// The string representation of this `AppProfile`.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Devel => "devel",
        }
    }

    /// Whether this `AppProfile` should use the `.devel` CSS class on windows.
    pub fn should_use_devel_class(self) -> bool {
        matches!(self, Self::Devel)
    }

    /// The name of the directory where to put data for this profile.
    pub fn dir_name(self) -> Cow<'static, str> {
        match self {
            AppProfile::Stable => Cow::Borrowed("fractal-sexta"),
            _ => Cow::Owned(format!("fractal-sexta-{self}")),
        }
    }
}

impl fmt::Display for AppProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

include!(concat!(
    env!("FRACTAL_BUILD_DIR"),
    "/crates/fractal-app/src/fractal-config.rs"
));

pub const SETTINGS_KEY_CURRENT_SESSION: &str = "current-session";

pub const APP_NAME: &str = "Fractal Sexta";

pub const APP_HOMEPAGE_URL: &str = "https://github.com/yamada-sexta/fractal-sexta";

#[cfg(test)]
mod profile_tests {
    use super::AppProfile;

    #[test]
    fn profile_data_directories_are_separate_from_upstream() {
        assert_eq!(AppProfile::Stable.dir_name(), "fractal-sexta");
        assert_eq!(AppProfile::Beta.dir_name(), "fractal-sexta-beta");
        assert_eq!(AppProfile::Devel.dir_name(), "fractal-sexta-devel");
    }
}
