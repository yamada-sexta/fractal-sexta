//! Linux API to store the data of a session, using the Secret Service or Secret
//! portal.

use std::{collections::HashMap, str::FromStr};

use matrix_sdk::authentication::oauth::ClientId;
use oo7::{AsAttributes, Item, Keyring, Secret};
use ruma::{OwnedDeviceId, OwnedUserId};
use serde::Deserialize;
use tracing::{debug, error, info};
use url::Url;
use zeroize::Zeroizing;

use super::{SecretError, SecretExt, SessionTokens, StoredSession};
use crate::{APP_ID, PROFILE, gettext_f, prelude::*, spawn_tokio, utils::matrix};

/// The current version of the stored session.
const CURRENT_VERSION: u8 = 7;
/// The minimum supported version for the stored sessions.
///
/// The history of supported versions is:
///
/// - 6: released with Fractal 9 on Oct 30, 2024.
/// - 7: released with Fractal 11 on May 1, 2025.
const MIN_SUPPORTED_VERSION: u8 = 6;

/// Keys used in the Linux secret backend.
mod keys {
    /// The attribute for the schema in the Secret Service.
    pub(super) const XDG_SCHEMA: &str = "xdg:schema";
    /// The attribute for the profile of the app.
    pub(super) const PROFILE: &str = "profile";
    /// The attribute for the version of the stored session.
    pub(super) const VERSION: &str = "version";
    /// The attribute for the URL of the homeserver.
    pub(super) const HOMESERVER: &str = "homeserver";
    /// The attribute for the user ID.
    pub(super) const USER: &str = "user";
    /// The attribute for the device ID.
    pub(super) const DEVICE_ID: &str = "device-id";
    /// The attribute for the session ID.
    pub(super) const ID: &str = "id";
    /// The attribute for the client ID.
    pub(super) const CLIENT_ID: &str = "client-id";
}

/// Secret API under Linux.
pub struct LinuxSecret;

impl SecretExt for LinuxSecret {
    async fn restore_sessions() -> Result<Vec<StoredSession>, SecretError> {
        let handle = spawn_tokio!(async move { restore_sessions_inner().await });
        match handle.await.expect("task was not aborted") {
            Ok(sessions) => Ok(sessions),
            Err(error) => {
                error!("Could not restore previous sessions: secret error: {error}");
                Err(error.into())
            }
        }
    }

    async fn store_session(session: StoredSession) -> Result<(), SecretError> {
        let handle = spawn_tokio!(async move { session.store().await });
        match handle.await.expect("task was not aborted") {
            Ok(()) => Ok(()),
            Err(error) => {
                error!("Could not store session: {error}");
                Err(error.into())
            }
        }
    }

    async fn delete_session(session: StoredSession) {
        let (attributes, _) = session.into_parts();

        spawn_tokio!(async move {
            if let Err(error) = attributes.delete_matching_item().await {
                error!("Could not delete session data from secret backend: {error}");
            }
        })
        .await
        .expect("task was not aborted");
    }
}

async fn restore_sessions_inner() -> Result<Vec<StoredSession>, Box<oo7::Error>> {
    let keyring = Keyring::new().await?;

    keyring.unlock().await?;

    let items = keyring
        .search_items(&HashMap::from([
            (keys::XDG_SCHEMA, APP_ID),
            (keys::PROFILE, PROFILE.as_str()),
        ]))
        .await?;

    let mut sessions = Vec::with_capacity(items.len());

    for item in items {
        item.unlock().await?;

        match StoredSession::try_from_secret_item(item).await {
            Ok(Some(session)) => sessions.push(session),
            Ok(None) => {
                // The session is invalid, we cannot do anything with it.
            }
            Err(error) => {
                error!("Could not restore previous session: secret error: {error}");
            }
        }
    }

    Ok(sessions)
}

impl StoredSession {
    /// Construct a `StoredSession` from its attributes and passphrase.
    fn from_parts(attributes: Attributes, passphrase: Zeroizing<String>) -> Self {
        let Attributes {
            version: _,
            homeserver,
            user_id,
            device_id,
            id,
            client_id,
        } = attributes;

        Self {
            homeserver,
            user_id,
            device_id,
            id,
            client_id,
            passphrase,
        }
    }

    /// Split this stored session between its attributes and passphrase.
    fn into_parts(self) -> (Attributes, Zeroizing<String>) {
        let Self {
            homeserver,
            user_id,
            device_id,
            id,
            client_id,
            passphrase,
        } = self;

        (
            Attributes {
                version: CURRENT_VERSION,
                homeserver,
                user_id,
                device_id,
                id,
                client_id,
            },
            passphrase,
        )
    }

    /// Try to construct a `StoredSession` from an item from the secret backend.
    ///
    /// Returns `Err` if a problem occurred with the secret backend, `Ok(None)`
    /// if the item is not a valid stored session, and `Ok(Some(_))` if it is a
    /// valid stored session.
    async fn try_from_secret_item(item: Item) -> Result<Option<Self>, Box<oo7::Error>> {
        let Some(attributes) = Attributes::try_from_stored_map(item.attributes().await?) else {
            return Ok(None);
        };

        let version = attributes.version;

        let Some(SecretData {
            passphrase,
            access_token,
        }) = SecretData::try_from_secret(version, &item.secret().await?)
        else {
            return Ok(None);
        };

        let session = Self::from_parts(attributes, passphrase.into());

        if version < MIN_SUPPORTED_VERSION {
            session.delete_unsupported(version, access_token).await;
            return Ok(None);
        }

        if version < CURRENT_VERSION {
            session.apply_migrations(version, item, access_token).await;
        }

        Ok(Some(session))
    }

    /// Migrate this session to the current version.
    ///
    /// Any error that occurs is logged.
    async fn apply_migrations(&self, from_version: u8, item: Item, access_token: Option<String>) {
        info!(
            "Found session {} for user {} with old version {from_version}, applying migrations…",
            self.id, self.user_id,
        );

        if from_version < 7 {
            // Version 7 moves the access token to a separate file. Only the passphrase is
            // stored as the secret now.
            info!("Migrating to version 7…");

            let (attributes, passphrase) = self.clone().into_parts();
            let secret = oo7::Secret::text(passphrase);

            spawn_tokio!(async move {
                if let Err(error) = item.set_secret(secret).await {
                    error!("Could not store updated session secret: {error}");
                }

                if let Err(error) = item.set_attributes(&attributes).await {
                    error!("Could not store updated session attributes: {error}");
                }
            })
            .await
            .expect("task was not aborted");

            if let Some(access_token) = access_token {
                let session_tokens = SessionTokens {
                    access_token,
                    refresh_token: None,
                };
                self.store_tokens(session_tokens).await;
            }
        }
    }

    /// Store this session in the secret backend.
    async fn store(self) -> Result<(), Box<oo7::Error>> {
        let keyring = Keyring::new().await?;

        let (attributes, passphrase) = self.into_parts();
        let secret = oo7::Secret::text(passphrase);

        keyring
            .create_item(
                &gettext_f(
                    // Translators: Do NOT translate the content between '{' and '}', this is a
                    // variable name.
                    "Fractal Sexta: Matrix credentials for {user_id}",
                    &[("user_id", attributes.user_id.as_str())],
                ),
                &attributes,
                secret,
                true,
            )
            .await?;

        Ok(())
    }

    /// Delete this unsupported session and all its data.
    async fn delete_unsupported(self, version: u8, access_token: Option<String>) {
        info!(
            "Found stored session {} for Matrix user {} with version {version} that is no longer supported, removing…",
            self.id, self.user_id
        );

        // Try to log it out.
        if let Some(access_token) = access_token {
            debug!("Logging out stored session {}…", self.id);

            let session = self.clone();
            let tokens = SessionTokens {
                access_token,
                refresh_token: None,
            };

            spawn_tokio!(async move {
                match matrix::client_with_stored_session(session, tokens).await {
                    Ok(client) => {
                        if let Err(error) = client.logout().await {
                            error!("Could not log out session: {error}");
                        }
                    }
                    Err(error) => {
                        error!("Could not build client to log out session: {error}");
                    }
                }
            })
            .await
            .expect("task was not aborted");
        }

        self.delete().await;
    }
}

/// The attributes stored in the secret backend.
struct Attributes {
    /// The version of the stored session.
    version: u8,

    /// The URL of the homeserver where the account lives.
    homeserver: Url,

    /// The unique identifier of the user.
    user_id: OwnedUserId,

    /// The unique identifier of the session on the homeserver.
    device_id: OwnedDeviceId,

    /// The unique local identifier of the session.
    ///
    /// This is the name of the directories where the session data lives.
    id: String,

    /// The unique identifier of the client with the homeserver.
    client_id: Option<ClientId>,
}

impl Attributes {
    /// Try to extract the attributes from the given map.
    ///
    /// Returns `None` if the attributes could not be extracted from the map.
    fn try_from_stored_map(mut attributes: HashMap<String, String>) -> Option<Self> {
        // Return early if the version is unsupported.
        let version = Self::extract_required_attribute_as(&mut attributes, keys::VERSION)?;
        if version > CURRENT_VERSION {
            error!("Could not restore stored session: unsupported version {version}");
            return None;
        }

        // Extract the required attributes.
        let homeserver = Self::extract_required_attribute_as(&mut attributes, keys::HOMESERVER)?;
        let user_id = Self::extract_required_attribute_as(&mut attributes, keys::USER)?;
        let device_id = Self::extract_required_attribute(&mut attributes, keys::DEVICE_ID)?.into();
        let id = Self::extract_required_attribute(&mut attributes, keys::ID)?;

        // Extract the optional attribute.
        let client_id = attributes.remove(keys::CLIENT_ID).map(ClientId::new);

        Some(Self {
            version,
            homeserver,
            user_id,
            device_id,
            id,
            client_id,
        })
    }

    /// Extract the required attribute with the given key in the given map.
    ///
    /// Returns `None` if the attribute was not found and logs an error.
    fn extract_required_attribute(
        attributes: &mut HashMap<String, String>,
        key: &str,
    ) -> Option<String> {
        if let Some(attribute) = attributes.remove(key) {
            Some(attribute)
        } else {
            error!("Could not restore stored session: could not find `{key}` attribute");
            None
        }
    }

    /// Extract the required attribute with the given key in the given map, an
    /// try to parse it as `T`.
    ///
    /// Returns `None` if the attribute was not found or could not be parsed and
    /// logs an error.
    fn extract_required_attribute_as<T>(
        attributes: &mut HashMap<String, String>,
        key: &'static str,
    ) -> Option<T>
    where
        T: FromStr,
        T::Err: std::fmt::Display,
    {
        let string = Self::extract_required_attribute(attributes, key)?;

        match T::from_str(&string) {
            Ok(value) => Some(value),
            Err(error) => {
                error!(
                    "Could not restore stored session: could not parse `{key}` attribute: {error}"
                );
                None
            }
        }
    }

    /// Remove the item matching this attributes from the secret backend.
    ///
    /// Returns an error if a problem occurs with the secret backend.
    async fn delete_matching_item(&self) -> Result<(), Box<oo7::Error>> {
        let keyring = Keyring::new().await?;
        keyring.delete(self).await?;

        Ok(())
    }
}

impl AsAttributes for Attributes {
    fn as_attributes(&self) -> HashMap<String, String> {
        let Self {
            version,
            homeserver,
            user_id,
            device_id,
            id,
            client_id,
        } = self;

        let mut attributes = HashMap::from([
            (keys::HOMESERVER.to_owned(), homeserver.to_string()),
            (keys::USER.to_owned(), user_id.to_string()),
            (keys::DEVICE_ID.to_owned(), device_id.to_string()),
            (keys::ID.to_owned(), id.clone()),
            (keys::VERSION.to_owned(), version.to_string()),
            (keys::PROFILE.to_owned(), PROFILE.to_string()),
            (keys::XDG_SCHEMA.to_owned(), APP_ID.to_owned()),
        ]);

        if let Some(client_id) = &client_id {
            attributes.insert(keys::CLIENT_ID.to_owned(), client_id.as_str().to_owned());
        }

        attributes
    }
}

/// Secret data that is stored in the secret backend.
#[derive(Clone, Deserialize)]
struct SecretData {
    /// The passphrase used to encrypt the local databases.
    passphrase: String,

    /// The access token to provide to the homeserver for authentication.
    ///
    /// This is only stored in the secret backend until version 6.
    access_token: Option<String>,
}

impl SecretData {
    /// Try to extract the secret data for the given version from the given
    /// secret.
    ///
    /// Returns `None` if the secret data is invalid.
    fn try_from_secret(version: u8, secret: &Secret) -> Option<Self> {
        // Even if we store the secret as plain text, the file backend always returns a
        // blob so let's always treat it as bytes.
        let bytes = secret.as_bytes();

        if version <= 6 {
            let secret_data = serde_json::from_slice::<Self>(bytes)
                .inspect_err(|error| {
                    error!("Could not parse secret in stored session: {error}");
                })
                .ok()?;

            if secret_data.access_token.is_none() {
                error!("Could not parse secret in stored session: missing `access_token` field");
                return None;
            }

            Some(secret_data)
        } else {
            String::from_utf8(bytes.to_owned())
                .inspect_err(|error| {
                    error!("Could not parse secret in stored session: {error}");
                })
                .ok()
                .map(|passphrase| Self {
                    passphrase,
                    access_token: None,
                })
        }
    }
}

impl From<Box<oo7::Error>> for SecretError {
    fn from(value: Box<oo7::Error>) -> Self {
        Self::Service(value.to_user_facing())
    }
}
