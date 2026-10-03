//! Reading the address and pairing link the user pastes.
//!
//! Pairing links follow T3's `packages/shared/src/remote.ts`: the one-time
//! token is in the fragment (`#token=…`) or, from older servers, the query
//! (`?token=…`). Hosted links carry the backend address in a `host` parameter.

use url::Url;

use crate::error::T3Error;
use crate::secret::Secret;

const TOKEN_PARAM: &str = "token";
const HOSTED_HOST_PARAM: &str = "host";

/// Where to reach a server and the one-time credential to exchange there.
#[derive(Debug)]
pub struct PairingRequest {
    pub base: Url,
    pub credential: Secret,
}

/// Turn a typed address into the server's base URL (`http://host:port/`).
pub fn normalize_address(text: &str) -> Result<Url, T3Error> {
    let text = text.trim();
    if text.is_empty() {
        return Err(T3Error::BadAddress { detail: "it is empty".into() });
    }
    let with_scheme = if text.contains("://") { text.to_owned() } else { format!("http://{text}") };
    let mut url = Url::parse(&with_scheme).map_err(|e| T3Error::BadAddress { detail: e.to_string() })?;
    match url.scheme() {
        "http" => {}
        "ws" => url.set_scheme("http").map_err(|()| T3Error::BadAddress { detail: "unusable scheme".into() })?,
        "https" | "wss" => return Err(T3Error::HttpsUnsupported),
        other => return Err(T3Error::BadAddress { detail: format!("{other}:// is not a server address") }),
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(T3Error::BadAddress { detail: "it has no host name".into() });
    }
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

/// Read a pasted pairing link. `address`, when given, is where Bukno should
/// connect; it wins over the host inside the link, because a link made on the
/// server may name an address that only works there (such as 127.0.0.1).
pub fn parse_pairing(address: Option<&str>, link: &str) -> Result<PairingRequest, T3Error> {
    let link = link.trim();
    if link.is_empty() {
        return Err(T3Error::BadPairingLink { detail: "it is empty".into() });
    }
    let explicit = address.map(str::trim).filter(|a| !a.is_empty()).map(normalize_address).transpose()?;

    let Ok(url) = Url::parse(link) else {
        // A bare token: only usable with an address.
        if link.contains(char::is_whitespace) || link.contains('/') {
            return Err(T3Error::BadPairingLink { detail: "it is not a link or a token".into() });
        }
        let base = explicit
            .ok_or_else(|| T3Error::BadPairingLink { detail: "a bare token needs the server address too".into() })?;
        return Ok(PairingRequest { base, credential: Secret::new(link) });
    };

    let fragment: Vec<(String, String)> =
        url::form_urlencoded::parse(url.fragment().unwrap_or("").as_bytes()).into_owned().collect();
    let token = fragment
        .iter()
        .find(|(k, _)| k == TOKEN_PARAM)
        .map(|(_, v)| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .or_else(|| {
            url.query_pairs()
                .find(|(k, _)| k == TOKEN_PARAM)
                .map(|(_, v)| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        })
        .ok_or_else(|| T3Error::BadPairingLink { detail: "it has no token".into() })?;

    let hosted = url.query_pairs().find(|(k, _)| k == HOSTED_HOST_PARAM).map(|(_, v)| v.into_owned());
    let base = match (explicit, hosted) {
        (Some(base), _) => base,
        (None, Some(host)) => normalize_address(&host)?,
        (None, None) => normalize_address(&format!("{}://{}", url.scheme(), host_and_port(&url)?))?,
    };
    Ok(PairingRequest { base, credential: Secret::new(token) })
}

fn host_and_port(url: &Url) -> Result<String, T3Error> {
    let host = url.host_str().ok_or_else(|| T3Error::BadPairingLink { detail: "it has no host name".into() })?;
    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

/// The socket URL for a base address and a fresh ticket.
pub fn socket_url(base: &Url, ticket: &Secret) -> Url {
    let mut url = base.clone();
    // Base addresses are always http here, so this cannot fail.
    let _ = url.set_scheme("ws");
    url.set_path("/ws");
    url.query_pairs_mut()
        .append_pair("wsTicket", ticket.expose())
        .append_pair("orchestrationProtocol", &crate::pinned::ORCHESTRATION_PROTOCOL.to_string())
        .append_pair("clientSurface", "desktop")
        .append_pair("clientAppVersion", env!("CARGO_PKG_VERSION"));
    url
}

/// A URL with its query and fragment removed, for log lines.
pub fn redacted(url: &Url) -> String {
    let mut url = url.clone();
    if url.query().is_some() {
        url.set_query(Some("<redacted>"));
    }
    url.set_fragment(None);
    url.to_string()
}
