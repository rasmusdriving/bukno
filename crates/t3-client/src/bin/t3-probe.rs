//! A small command-line peer for checking the client against a real server.
//!
//! It uses the same keychain entries and environment list as the app, and the
//! same read-only methods. Pairing links are read from standard input so they
//! never appear in a process list or shell history.
//!
//! ```text
//! t3-probe pair --state DIR --address http://host:port      (link on stdin)
//! t3-probe record --state DIR --env ID --out FILE [--thread ID] [--seconds N] [--no-ack]
//! t3-probe socket-protocol --address http://host:port --protocol N
//! t3-probe forget --env ID                                 (removes the keychain entry)
//! ```

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use bukno_t3_client::http::Http;
use bukno_t3_client::pairing::{normalize_address, parse_pairing};
use bukno_t3_client::rpc::{Method, Session, StreamEvent};
use bukno_t3_client::secret::{SystemKeychain, TokenVault};
use bukno_t3_client::store::{EnvironmentStore, SavedEnvironment};
use bukno_t3_client::{Log, T3Error, time};
use serde_json::json;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn log() -> Log {
    Arc::new(|line: &str| eprintln!("{} {line}", time::now_utc()))
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("pair") => pair(&args).await,
        Some("record") => record(&args).await,
        Some("socket-protocol") => socket_protocol(&args).await,
        Some("forget") => match arg(&args, "--env") {
            Some(env) => SystemKeychain.delete(&env).map_err(|e| e.user_message()),
            None => Err("--env ID is required".to_owned()),
        },
        _ => Err("usage: t3-probe pair|record|socket-protocol|forget …".to_owned()),
    };
    if let Err(e) = result {
        eprintln!("t3-probe: {e}");
        std::process::exit(1);
    }
}

async fn pair(args: &[String]) -> Result<(), String> {
    let state = PathBuf::from(arg(args, "--state").ok_or("--state DIR is required")?);
    let address = arg(args, "--address").unwrap_or_default();
    let mut link = String::new();
    std::io::stdin().lock().read_line(&mut link).map_err(|e| e.to_string())?;
    let request = parse_pairing(Some(&address), &link).map_err(|e| e.user_message())?;
    let http = Http::new();
    let descriptor = http.descriptor(&request.base).await.map_err(|e| e.user_message())?;
    let access =
        http.exchange(&request.base, &request.credential, "Bukno probe").await.map_err(|e| e.user_message())?;
    SystemKeychain.save(&descriptor.environment_id, &access.token).map_err(|e| e.user_message())?;
    let store = EnvironmentStore::new(state.join("t3-environments.json"));
    let mut all = store.load()?;
    all.retain(|e| e.environment_id != descriptor.environment_id);
    all.push(SavedEnvironment {
        environment_id: descriptor.environment_id.clone(),
        label: descriptor.label.clone(),
        address: request.base.to_string(),
        server_version: descriptor.server_version,
        token_expires_at: access.expires_at_epoch,
        scope: access.scope.clone(),
        paired_at: time::now_epoch_secs(),
    });
    store.save(&all)?;
    eprintln!("paired with {} ({}), scope {}", descriptor.label, descriptor.environment_id, access.scope);
    Ok(())
}

async fn record(args: &[String]) -> Result<(), String> {
    let state = PathBuf::from(arg(args, "--state").ok_or("--state DIR is required")?);
    let env = arg(args, "--env").ok_or("--env ID is required")?;
    let out = PathBuf::from(arg(args, "--out").ok_or("--out FILE is required")?);
    let seconds: u64 = arg(args, "--seconds").and_then(|s| s.parse().ok()).unwrap_or(10);
    let ack = !args.iter().any(|a| a == "--no-ack");
    let thread = arg(args, "--thread");
    let saved = EnvironmentStore::new(state.join("t3-environments.json"))
        .load()?
        .into_iter()
        .find(|e| e.environment_id == env)
        .ok_or("that environment is not saved")?;
    // The same checked setup as the app: identity, protocol and expiry first.
    let (session, _) = bukno_t3_client::hub::connect_saved(&Http::new(), &SystemKeychain, &saved, log())
        .await
        .map_err(|e| e.user_message())?;
    let mut file = std::fs::File::create(&out).map_err(|e| e.to_string())?;
    let mut write = |stream: &str, value: &serde_json::Value| {
        let line = json!({"at": time::now_utc(), "stream": stream, "value": value});
        let _ = writeln!(file, "{line}");
    };

    let config = session.call(Method::GetConfig, json!({})).await.map_err(|e| e.user_message())?;
    write("server.getConfig", &config);
    let mut shell = session
        .subscribe(Method::SubscribeShell, json!({"requestCompletionMarker": true}))
        .await
        .map_err(|e| e.user_message())?;
    let mut thread_sub = match &thread {
        Some(id) => Some(
            session
                .subscribe(
                    Method::SubscribeThread,
                    json!({"threadId": id, "requestCompletionMarker": true, "acceptBoundedSnapshot": true}),
                )
                .await
                .map_err(|e| e.user_message())?,
        ),
        None => None,
    };
    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    let mut chunks = 0;
    loop {
        tokio::select! {
            () = &mut deadline => break,
            event = shell.next() => match event {
                Some(StreamEvent::Values(values)) => {
                    chunks += 1;
                    for v in &values { write("subscribeShell", v); }
                    if ack { shell.ack(); }
                }
                Some(StreamEvent::Failed(e)) => { eprintln!("shell stream failed: {e:?}"); break; }
                _ => { eprintln!("shell stream ended"); break; }
            },
            event = async { match thread_sub.as_mut() { Some(s) => s.next().await, None => std::future::pending().await } } => match event {
                Some(StreamEvent::Values(values)) => {
                    chunks += 1;
                    for v in &values { write("subscribeThread", v); }
                    if ack && let Some(s) = &thread_sub { s.ack(); }
                }
                Some(StreamEvent::Failed(e)) => { eprintln!("thread stream failed: {e:?}"); thread_sub = None; }
                _ => { eprintln!("thread stream ended"); thread_sub = None; }
            },
        }
    }
    eprintln!(
        "recorded {chunks} chunks; frames in {}, requests out {}, acks out {}, unknown frames {}",
        session.stats.frames_in.load(std::sync::atomic::Ordering::Relaxed),
        session.stats.requests_out.load(std::sync::atomic::Ordering::Relaxed),
        session.stats.acks_out.load(std::sync::atomic::Ordering::Relaxed),
        session.stats.unknown_frames.load(std::sync::atomic::Ordering::Relaxed),
    );
    Ok(())
}

/// Ask for a socket with another protocol number, to see the refusal.
async fn socket_protocol(args: &[String]) -> Result<(), String> {
    let base =
        normalize_address(&arg(args, "--address").ok_or("--address is required")?).map_err(|e| e.user_message())?;
    let protocol = arg(args, "--protocol").unwrap_or_else(|| "1".into());
    let mut url = base.clone();
    let _ = url.set_scheme("ws");
    url.set_path("/ws");
    url.query_pairs_mut().append_pair("orchestrationProtocol", &protocol);
    match tokio_tungstenite_connect(&url).await {
        Ok(()) => Err("the socket opened, which was not expected".into()),
        Err(e) => {
            eprintln!("refused as expected: {}", e.user_message());
            Ok(())
        }
    }
}

async fn tokio_tungstenite_connect(url: &url::Url) -> Result<(), T3Error> {
    Session::connect(url, log()).await.map(|_| ())
}
