//! A client for a T3 Code server.
//!
//! Stage 1 of the T3 backend plan: pair with an existing server, read its
//! projects, chats and models, and follow live updates. Stage 2: start chats,
//! send, answer, stop, queue and steer through the typed commands in
//! [`command`]. The socket only carries the methods in [`rpc::Method`], and
//! HTTP calls are the sign-in exchange plus GET reads.
//!
//! Built against the T3 revision and wire format recorded in `PINNED.md`.

pub mod command;
pub mod error;
pub mod http;
pub mod hub;
pub mod model;
pub mod pairing;
pub mod rpc;
pub mod secret;
pub mod shell;
pub mod store;
pub mod thread;
pub mod time;

pub use error::T3Error;

/// Where the client writes its log lines. Lines never contain a token or a
/// pairing link.
pub type Log = std::sync::Arc<dyn Fn(&str) + Send + Sync>;
pub use hub::{ConnectionStatus, EnvironmentView, Hub, HubView, PairingStatus};

/// The T3 build this client was written and checked against.
pub mod pinned {
    pub const SOURCE_REVISION: &str = "737993303d36e10674c54b95e5bd3826682c99c7";
    pub const SERVER_VERSION: &str = "0.0.46-nightly.20261004.2644";
    pub const EFFECT_VERSION: &str = "4.0.0-rc.115";
    /// The only orchestration protocol this client speaks.
    pub const ORCHESTRATION_PROTOCOL: u32 = 2;
}
