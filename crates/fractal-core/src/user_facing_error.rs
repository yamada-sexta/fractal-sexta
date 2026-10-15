use std::time::{Duration, SystemTime};

use gettextrs::gettext;
use matrix_sdk::{ClientBuildError, Error, HttpError};
use ruma::api::error::{ErrorBody, ErrorKind, RetryAfter, StandardErrorBody};

use crate::ngettext_f;

pub trait UserFacingError {
    fn to_user_facing(&self) -> String;
}

impl UserFacingError for HttpError {
    fn to_user_facing(&self) -> String {
        if let HttpError::Reqwest(error) = self {
            // TODO: Add more information based on the error
            if error.is_timeout() {
                gettext("Connection timed out.")
            } else {
                gettext("Could not connect to the homeserver.")
            }
        } else if let Some(ErrorBody::Standard(StandardErrorBody { kind, message, .. })) =
            self.as_client_api_error().map(|error| &error.body)
        {
            match kind {
                ErrorKind::Forbidden => gettext("Invalid credentials."),
                ErrorKind::UserDeactivated => gettext("Account deactivated."),
                ErrorKind::LimitExceeded(limit_exceeded) => {
                    if let Some(retry_after) = &limit_exceeded.retry_after {
                        let duration = match retry_after {
                            RetryAfter::Delay(duration) => *duration,
                            RetryAfter::DateTime(until) => until
                                .duration_since(SystemTime::now())
                                // An error means that the date provided is in the past, which
                                // doesn't make sense. Let's not panic anyway and default to 1
                                // second.
                                .unwrap_or_else(|_| Duration::from_secs(1)),
                        };
                        let secs = duration.as_secs() as u32;
                        ngettext_f(
                            // Translators: Do NOT translate the content between '{' and '}',
                            // this is a variable name.
                            "Rate limit exceeded, retry in 1 second.",
                            "Rate limit exceeded, retry in {n} seconds.",
                            secs,
                            &[("n", &secs.to_string())],
                        )
                    } else {
                        gettext("Rate limit exceeded, try again later.")
                    }
                }
                _ => {
                    // TODO: The server may not give us pretty enough error message. We should
                    // add our own error message.
                    message.clone()
                }
            }
        } else {
            gettext("Unexpected connection error.")
        }
    }
}

impl UserFacingError for Error {
    fn to_user_facing(&self) -> String {
        match self {
            Error::DecryptorError(_) => gettext("Could not decrypt the event."),
            Error::Http(http_error) => http_error.to_user_facing(),
            _ => gettext("Unexpected error."),
        }
    }
}

impl UserFacingError for ClientBuildError {
    fn to_user_facing(&self) -> String {
        match self {
            ClientBuildError::Url(_) => gettext("Invalid URL."),
            ClientBuildError::AutoDiscovery(_) => gettext("Could not discover homeserver."),
            ClientBuildError::Http(err) => err.to_user_facing(),
            ClientBuildError::SqliteStore(_) => gettext("Could not open the store."),
            _ => gettext("Unexpected error."),
        }
    }
}

#[cfg(target_os = "linux")]
impl UserFacingError for oo7::Error {
    fn to_user_facing(&self) -> String {
        match self {
            oo7::Error::File(error) => error.to_user_facing(),
            oo7::Error::DBus(error) => error.to_user_facing(),
        }
    }
}

#[cfg(target_os = "linux")]
impl UserFacingError for oo7::file::Error {
    fn to_user_facing(&self) -> String {
        use oo7::file::Error;

        match self {
            Error::FileHeaderMismatch(_)
            | Error::VersionMismatch(_)
            | Error::NoData
            | Error::MacError
            | Error::HashedAttributeMac(_)
            | Error::GVariantDeserialization(_)
            | Error::SaltSizeMismatch(..)
            | Error::ChecksumMismatch
            | Error::AlgorithmMismatch(_)
            | Error::IncorrectSecret
            | Error::Crypto(_)
            | Error::Utf8(_)
            | Error::PartiallyCorruptedKeyring { .. } => {
                gettext("The secret storage file is corrupted.")
            }
            Error::NoParentDir(_) | Error::NoDataDir => {
                gettext("Could not access the secret storage file location.")
            }
            Error::Io(_) => {
                gettext("An unexpected error occurred when accessing the secret storage file.")
            }
            Error::TargetFileChanged(_) => {
                gettext("The secret storage file has been changed by another process.")
            }
            Error::Portal(ashpd::Error::Portal(ashpd::PortalError::Cancelled(_))) => gettext(
                "The request to the Flatpak Secret Portal was cancelled. Make sure to accept any prompt asking to access it.",
            ),
            Error::Portal(ashpd::Error::PortalNotFound(_)) => gettext(
                "The Flatpak Secret Portal is not available. Make sure xdg-desktop-portal is installed, and it is at least at version 1.5.0.",
            ),
            Error::Portal(_) => gettext(
                "An unexpected error occurred when interacting with the D-Bus Secret Portal backend.",
            ),
            Error::WeakKey(_) => {
                gettext("The Flatpak Secret Portal provided a key that is too weak to be secure.")
            }
            Error::Locked => gettext("The collection or item is locked."),
            // Can only occur when using APIs that we do not use.
            Error::InvalidItemIndex(_) | Error::InvalidKeyLength { .. } => unreachable!(),
        }
    }
}

#[cfg(target_os = "linux")]
impl UserFacingError for oo7::dbus::Error {
    fn to_user_facing(&self) -> String {
        use oo7::dbus::{Error, ServiceError};

        match self {
            Error::Deleted => gettext("The item was deleted."),
            Error::Service(s) => match s {
                ServiceError::ZBus(_) => gettext(
                    "An unexpected error occurred when interacting with the D-Bus Secret Service.",
                ),
                ServiceError::IsLocked(_) => gettext("The collection or item is locked."),
                ServiceError::NoSession(_) => {
                    gettext("The D-Bus Secret Service session does not exist.")
                }
                ServiceError::NoSuchObject(_) => gettext("The collection or item does not exist."),
            },
            Error::Dismissed => gettext(
                "The request to the D-Bus Secret Service was cancelled. Make sure to accept any prompt asking to access it.",
            ),
            Error::NotFound(_) => gettext(
                "Could not access the default collection. Make sure a keyring was created and set as default.",
            ),
            Error::ZBus(_) | Error::Crypto(_) | Error::IO(_) => gettext(
                "An unexpected error occurred when interacting with the D-Bus Secret Service.",
            ),
        }
    }
}
