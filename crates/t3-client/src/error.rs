//! Everything that can go wrong talking to a T3 server, in words for the user.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum T3Error {
    /// The address could not be read as an address.
    BadAddress { detail: String },
    /// The pairing link has no token in it.
    BadPairingLink { detail: String },
    /// HTTPS needs TLS support, which arrives after Stage 1.
    HttpsUnsupported,
    /// Nothing answered, or the connection failed on the way.
    Unreachable { detail: String },
    /// Something answered, but it is not a T3 server.
    NotT3 { detail: String },
    /// The address now belongs to a different T3 environment.
    WrongEnvironment { expected: String, actual: String },
    /// The server speaks another orchestration protocol.
    ProtocolMismatch { server: u32 },
    /// The pairing link was refused: expired, already used, or not for this server.
    PairingRejected,
    /// The saved sign-in was refused: revoked in T3, or expired there.
    SignInRejected,
    /// The saved sign-in is past the expiry the server gave when pairing.
    SignInExpired,
    /// No saved sign-in for this environment.
    NotPaired,
    /// The keychain could not save or read the sign-in.
    Keychain { detail: String },
    /// The server answered with an unexpected error.
    Server { status: u16, detail: String },
    /// The server sent something Bukno could not read.
    Decode { detail: String },
    /// The socket closed or stopped answering.
    Disconnected { detail: String },
    /// A request was refused by the server's RPC layer.
    Rpc { detail: String },
}

impl T3Error {
    /// Whether retrying the same connection later can help.
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Unreachable { .. } | Self::Disconnected { .. } | Self::Server { .. } | Self::Rpc { .. })
    }

    /// The user must pair again before anything else can work.
    pub fn needs_pairing(&self) -> bool {
        matches!(self, Self::SignInRejected | Self::SignInExpired | Self::NotPaired)
    }

    pub fn user_message(&self) -> String {
        match self {
            Self::BadAddress { detail } => format!("That address cannot be used: {detail}."),
            Self::BadPairingLink { detail } => format!("That pairing link cannot be used: {detail}."),
            Self::HttpsUnsupported => {
                "HTTPS addresses are not supported yet. Use the server's http:// address, such as its Tailscale IP and port."
                    .into()
            }
            Self::Unreachable { detail } => format!("Nothing answered at this address ({detail})."),
            Self::NotT3 { detail } => format!("This address is not a T3 server ({detail})."),
            Self::WrongEnvironment { expected, actual } => format!(
                "This address now belongs to a different T3 server (expected {expected}, found {actual}). Bukno did not send your sign-in to it."
            ),
            Self::ProtocolMismatch { server } => format!(
                "This T3 server uses orchestration protocol {server}; this Bukno supports protocol {}.",
                crate::pinned::ORCHESTRATION_PROTOCOL
            ),
            Self::PairingRejected => {
                "T3 refused this pairing link. It has expired or was already used. Create a new link in T3 and paste it again."
                    .into()
            }
            Self::SignInRejected => {
                "T3 no longer accepts this computer's sign-in. It may have been revoked. Pair again with a new link.".into()
            }
            Self::SignInExpired => "The sign-in for this server has expired. Pair again with a new link.".into(),
            Self::NotPaired => "This server is not paired yet. Paste a pairing link to connect.".into(),
            Self::Keychain { detail } => format!("Bukno could not use the system keychain ({detail}). Nothing was saved."),
            Self::Server { status, detail } => format!("T3 answered with an error ({status}: {detail})."),
            Self::Decode { detail } => format!("T3 sent something Bukno could not read ({detail})."),
            Self::Disconnected { detail } => format!("The connection to T3 was lost ({detail})."),
            Self::Rpc { detail } => format!("T3 refused a request ({detail})."),
        }
    }
}

impl fmt::Display for T3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl std::error::Error for T3Error {}
