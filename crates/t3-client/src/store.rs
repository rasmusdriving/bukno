//! The saved list of paired environments. Addresses, IDs and expiry only;
//! tokens live in the system keychain.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedEnvironment {
    pub environment_id: String,
    pub label: String,
    /// Base address, such as `http://100.127.119.35:3773/`.
    pub address: String,
    pub server_version: String,
    /// When the server said the sign-in expires, in seconds since 1970.
    pub token_expires_at: u64,
    pub scope: String,
    pub paired_at: u64,
}

impl SavedEnvironment {
    /// Whether the sign-in may send, answer and stop. Sign-ins from Stage 1
    /// only hold `orchestration:read` and must be paired again.
    pub fn can_operate(&self) -> bool {
        self.scope.split_whitespace().any(|s| s == crate::http::OPERATE_SCOPE)
    }
}

pub struct EnvironmentStore {
    path: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    version: u32,
    environments: Vec<SavedEnvironment>,
}

impl EnvironmentStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Result<Vec<SavedEnvironment>, String> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice::<File>(&bytes)
                .map(|f| f.environments)
                .map_err(|e| format!("{} could not be read: {e}", self.path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(format!("{} could not be read: {e}", self.path.display())),
        }
    }

    /// Replace the file atomically.
    pub fn save(&self, environments: &[SavedEnvironment]) -> Result<(), String> {
        let file = File { version: 1, environments: environments.to_vec() };
        let bytes = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
        let temp = self.path.with_extension("json.tmp");
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&temp, bytes).map_err(|e| format!("{}: {e}", temp.display()))?;
        std::fs::rename(&temp, &self.path).map_err(|e| format!("{}: {e}", self.path.display()))
    }
}
