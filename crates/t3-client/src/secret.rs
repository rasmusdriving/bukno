//! Secrets: a wrapper that never prints, and the system keychain.

use std::fmt;

use crate::error::T3Error;

/// Keychain service name for every T3 sign-in Bukno saves.
pub const KEYCHAIN_SERVICE: &str = "io.github.rasmusdriving.bukno.t3";

/// A token or pairing credential. `Debug` prints `<redacted>`, and there is no
/// `Display`, so a secret cannot end up in a log line by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The raw value, for the one place that needs it (a request header or body).
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Where a bearer token for one environment is kept.
pub trait TokenVault: Send + Sync {
    fn load(&self, environment_id: &str) -> Result<Option<Secret>, T3Error>;
    fn save(&self, environment_id: &str, token: &Secret) -> Result<(), T3Error>;
    fn delete(&self, environment_id: &str) -> Result<(), T3Error>;
}

/// The operating system's keychain: Keychain Services on macOS, Credential
/// Manager on Windows, the Secret Service (GNOME Keyring, KWallet) on Linux.
pub struct SystemKeychain;

fn entry(environment_id: &str) -> Result<keyring::Entry, T3Error> {
    keyring::Entry::new(KEYCHAIN_SERVICE, &format!("environment:{environment_id}")).map_err(keychain_error)
}

fn keychain_error(error: keyring::Error) -> T3Error {
    T3Error::Keychain { detail: error.to_string() }
}

impl TokenVault for SystemKeychain {
    fn load(&self, environment_id: &str) -> Result<Option<Secret>, T3Error> {
        match entry(environment_id)?.get_password() {
            Ok(token) => Ok(Some(Secret::new(token))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain_error(e)),
        }
    }

    fn save(&self, environment_id: &str, token: &Secret) -> Result<(), T3Error> {
        let entry = entry(environment_id)?;
        entry.set_password(token.expose()).map_err(keychain_error)?;
        // Read it back: a locked or absent keyring can accept the write and lose it.
        match entry.get_password() {
            Ok(saved) if saved == token.expose() => Ok(()),
            Ok(_) => Err(T3Error::Keychain { detail: "the keychain returned a different value".into() }),
            Err(e) => Err(keychain_error(e)),
        }
    }

    fn delete(&self, environment_id: &str) -> Result<(), T3Error> {
        match entry(environment_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keychain_error(e)),
        }
    }
}
