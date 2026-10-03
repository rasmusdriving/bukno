//! The parts of the Codex app-server protocol Bukno uses, read tolerantly.
//!
//! Messages are JSON-RPC 2.0 without the version field, one per line. Fields
//! are read by name from the raw JSON, unknown fields are ignored, and
//! unknown values keep their text, so a newer engine that adds fields or
//! values does not break parsing. The raw value stays available for
//! diagnostics. Method names and shapes were checked against the schema
//! that Codex 0.158.0 generates (`codex app-server generate-json-schema`).

use serde_json::{Value, json};

/// Largest line accepted from the engine (section 17).
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    /// A reply to one of Bukno's requests.
    Response {
        id: i64,
        result: Result<Value, RpcError>,
    },
    /// The engine asks Bukno something and waits for the answer.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Parse one line. Returns None for anything that is not a recognizable message.
pub fn parse(line: &[u8]) -> Option<Incoming> {
    let value: Value = serde_json::from_slice(line).ok()?;
    let obj = value.as_object()?;
    let method = obj.get("method").and_then(Value::as_str);
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    match (obj.get("id"), method) {
        (Some(id), Some(method)) => Some(Incoming::Request { id: id.clone(), method: method.to_owned(), params }),
        (None, Some(method)) => Some(Incoming::Notification { method: method.to_owned(), params }),
        (Some(id), None) => {
            let id = id.as_i64()?;
            let result = match obj.get("error") {
                Some(error) => Err(RpcError {
                    code: error.get("code").and_then(Value::as_i64).unwrap_or_default(),
                    message: error.get("message").and_then(Value::as_str).unwrap_or("unknown error").to_owned(),
                }),
                None => Ok(obj.get("result").cloned().unwrap_or(Value::Null)),
            };
            Some(Incoming::Response { id, result })
        }
        (None, None) => None,
    }
}

pub fn request(id: i64, method: &str, params: Value) -> String {
    json!({"id": id, "method": method, "params": params}).to_string()
}

pub fn notification(method: &str) -> String {
    json!({"method": method}).to_string()
}

pub fn reply(id: &Value, result: Value) -> String {
    json!({"id": id, "result": result}).to_string()
}

pub fn reply_error(id: &Value, code: i64, message: &str) -> String {
    json!({"id": id, "error": {"code": code, "message": message}}).to_string()
}

/// String field helper.
pub fn str_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// The command a shell approval shows. Codex wraps commands as
/// `/bin/zsh -lc '<command>'`; the inner command is what the user cares about.
pub fn display_command(command: &str) -> String {
    for prefix in ["/bin/zsh -lc ", "/bin/bash -lc ", "bash -lc ", "zsh -lc ", "/bin/sh -c "] {
        if let Some(rest) = command.strip_prefix(prefix) {
            let rest = rest.trim();
            if rest.len() >= 2 && rest.starts_with('\'') && rest.ends_with('\'') {
                return rest[1..rest.len() - 1].replace("'\\''", "'");
            }
            return rest.to_owned();
        }
    }
    command.to_owned()
}

/// A model from `model/list`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub is_default: bool,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

pub fn models(result: &Value) -> Vec<ModelInfo> {
    result
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| !m.get("hidden").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|m| {
            let id = str_at(m, "id").or_else(|| str_at(m, "model"))?.to_owned();
            Some(ModelInfo {
                display_name: str_at(m, "displayName").unwrap_or(&id).to_owned(),
                is_default: m.get("isDefault").and_then(Value::as_bool).unwrap_or(false),
                efforts: m
                    .get("supportedReasoningEfforts")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| str_at(e, "reasoningEffort").map(str::to_owned))
                    .collect(),
                default_effort: str_at(m, "defaultReasoningEffort").map(str::to_owned),
                id,
            })
        })
        .collect()
}

/// Account as `account/read` reports it, without the email address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Account {
    ChatGpt { plan: String },
    ApiKey,
    Other(String),
    SignedOut,
}

pub fn account(result: &Value) -> Account {
    let Some(account) = result.get("account").filter(|a| !a.is_null()) else {
        return Account::SignedOut;
    };
    match str_at(account, "type") {
        Some("chatgpt") => Account::ChatGpt { plan: str_at(account, "planType").unwrap_or("unknown").to_owned() },
        Some("apiKey") => Account::ApiKey,
        Some(other) => Account::Other(other.to_owned()),
        None => Account::Other("unknown".into()),
    }
}

/// The model `config/read` reports as the user's default, if any.
pub fn config_model(result: &Value) -> Option<String> {
    result.get("config").and_then(|c| str_at(c, "model")).map(str::to_owned)
}
