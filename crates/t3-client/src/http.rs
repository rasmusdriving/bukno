//! The HTTP calls: server identity, sign-in exchange, socket tickets and
//! older history pages. Paths and shapes follow
//! `packages/contracts/src/environmentHttp.ts` at the pinned revision.

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use url::Url;

use crate::error::T3Error;
use crate::model::{Descriptor, HistoryPage};
use crate::secret::Secret;

const TIMEOUT: Duration = Duration::from_secs(10);
const PROTOCOL_HEADER: &str = "x-t3-orchestration-protocol";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const BOOTSTRAP_TOKEN_TYPE: &str = "urn:t3:params:oauth:token-type:environment-bootstrap";
const ACCESS_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:access_token";

/// Read chats, and operate them: send, answer, stop. Nothing else (no
/// terminals, reviews, relay or access management). The server refuses any
/// method outside these at call time.
pub const READ_SCOPE: &str = "orchestration:read";
pub const OPERATE_SCOPE: &str = "orchestration:operate";
pub const REQUESTED_SCOPES: &str = "orchestration:read orchestration:operate";

#[derive(Clone)]
pub struct Http {
    client: reqwest::Client,
}

/// A bearer token and when the server says it expires.
#[derive(Debug)]
pub struct AccessToken {
    pub token: Secret,
    pub expires_at_epoch: u64,
    pub scope: String,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

fn unreachable(e: &reqwest::Error) -> T3Error {
    // reqwest errors can include the URL; the URLs here never carry a secret.
    let detail = if e.is_timeout() {
        "timed out".to_owned()
    } else if e.is_connect() {
        "connection refused or no route".to_owned()
    } else {
        e.to_string()
    };
    T3Error::Unreachable { detail }
}

/// The error body T3 sends (`code`, `reason`, `_tag`), as short text.
fn error_detail(body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    match parsed {
        Some(v) => ["_tag", "code", "reason"]
            .iter()
            .filter_map(|k| v.get(*k).and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        None => body.chars().take(120).collect(),
    }
}

impl Http {
    pub fn new() -> Self {
        crate::local::install_tls_provider();
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("Bukno/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("HTTP client");
        Self { client }
    }

    fn url(base: &Url, path: &str) -> Url {
        let mut url = base.clone();
        url.set_path(path);
        url
    }

    /// Read the public server identity and check the protocol.
    pub async fn descriptor(&self, base: &Url) -> Result<Descriptor, T3Error> {
        let response = self
            .client
            .get(Self::url(base, "/.well-known/t3/environment"))
            .send()
            .await
            .map_err(|e| unreachable(&e))?;
        let status = response.status();
        if !status.is_success() {
            return Err(T3Error::NotT3 { detail: format!("HTTP {} for the T3 identity page", status.as_u16()) });
        }
        let body = response.text().await.map_err(|e| unreachable(&e))?;
        let descriptor: Descriptor = serde_json::from_str(&body)
            .map_err(|_| T3Error::NotT3 { detail: "it did not answer with a T3 environment description".into() })?;
        if descriptor.orchestration_protocol_version != crate::pinned::ORCHESTRATION_PROTOCOL {
            return Err(T3Error::ProtocolMismatch { server: descriptor.orchestration_protocol_version });
        }
        Ok(descriptor)
    }

    /// Exchange a one-time pairing credential for a bearer token that can read
    /// and operate chats.
    pub async fn exchange(&self, base: &Url, credential: &Secret, label: &str) -> Result<AccessToken, T3Error> {
        #[derive(Deserialize)]
        struct Response {
            access_token: String,
            token_type: String,
            expires_in: f64,
            scope: String,
        }
        let form = [
            ("grant_type", GRANT_TYPE),
            ("subject_token", credential.expose()),
            ("subject_token_type", BOOTSTRAP_TOKEN_TYPE),
            ("requested_token_type", ACCESS_TOKEN_TYPE),
            ("scope", REQUESTED_SCOPES),
            ("client_label", label),
            ("client_device_type", "desktop"),
            ("client_os", std::env::consts::OS),
        ];
        let response =
            self.client.post(Self::url(base, "/oauth/token")).form(&form).send().await.map_err(|e| unreachable(&e))?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|e| unreachable(&e))?;
        match status {
            200 => {}
            400 | 401 | 403 => return Err(T3Error::PairingRejected),
            _ => return Err(T3Error::Server { status, detail: error_detail(&body) }),
        }
        let parsed: Response =
            serde_json::from_str(&body).map_err(|e| T3Error::Decode { detail: format!("token response: {e}") })?;
        if parsed.token_type != "Bearer" {
            return Err(T3Error::Decode { detail: format!("token type {} is not supported", parsed.token_type) });
        }
        Ok(AccessToken {
            token: Secret::new(parsed.access_token),
            expires_at_epoch: crate::time::now_epoch_secs() + parsed.expires_in.max(0.0) as u64,
            scope: parsed.scope,
        })
    }

    /// A short-lived ticket for one socket connection.
    pub async fn websocket_ticket(&self, base: &Url, token: &Secret) -> Result<Secret, T3Error> {
        #[derive(Deserialize)]
        struct Response {
            ticket: String,
        }
        let response = self
            .client
            .post(Self::url(base, "/api/auth/websocket-ticket"))
            .bearer_auth(token.expose())
            .send()
            .await
            .map_err(|e| unreachable(&e))?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|e| unreachable(&e))?;
        match status {
            200 => {}
            401 | 403 => return Err(T3Error::SignInRejected),
            _ => return Err(T3Error::Server { status, detail: error_detail(&body) }),
        }
        let parsed: Response =
            serde_json::from_str(&body).map_err(|e| T3Error::Decode { detail: format!("ticket response: {e}") })?;
        Ok(Secret::new(parsed.ticket))
    }

    /// A read-only GET of any orchestration endpoint, as JSON. Used by the
    /// end-to-end checks to compare Bukno with T3's own HTTP snapshots.
    pub async fn get_json(&self, base: &Url, token: &Secret, path: &str) -> Result<Value, T3Error> {
        let response = self
            .client
            .get(Self::url(base, path))
            .bearer_auth(token.expose())
            .header(PROTOCOL_HEADER, crate::pinned::ORCHESTRATION_PROTOCOL.to_string())
            .send()
            .await
            .map_err(|e| unreachable(&e))?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|e| unreachable(&e))?;
        match status {
            200 => serde_json::from_str(&body).map_err(|e| T3Error::Decode { detail: format!("{path}: {e}") }),
            401 => Err(T3Error::SignInRejected),
            _ => Err(T3Error::Server { status, detail: error_detail(&body) }),
        }
    }

    /// One page of older timeline rows, chronological.
    pub async fn history_page(
        &self,
        base: &Url,
        token: &Secret,
        thread_id: &str,
        cursor: &str,
    ) -> Result<HistoryPage, T3Error> {
        let mut url = Self::url(base, "/api/orchestration/threads/");
        url.path_segments_mut()
            .map_err(|()| T3Error::BadAddress { detail: "unusable base".into() })?
            .pop_if_empty()
            .extend([thread_id, "history"]);
        let response = self
            .client
            .get(url)
            .query(&[("cursor", cursor)])
            .bearer_auth(token.expose())
            .header(PROTOCOL_HEADER, crate::pinned::ORCHESTRATION_PROTOCOL.to_string())
            .send()
            .await
            .map_err(|e| unreachable(&e))?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|e| unreachable(&e))?;
        match status {
            200 => {}
            401 => return Err(T3Error::SignInRejected),
            _ => return Err(T3Error::Server { status, detail: error_detail(&body) }),
        }
        let value: Value =
            serde_json::from_str(&body).map_err(|e| T3Error::Decode { detail: format!("history page: {e}") })?;
        HistoryPage::decode(&value).map_err(|detail| T3Error::Decode { detail })
    }
}
