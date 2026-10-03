//! Stage 1 end-to-end: the real Bukno app, driven through egui_kittest and its
//! AccessKit tree, reading a real T3 server through the read-only client.
//!
//! Runs only with `BUKNO_T3_LIVE=1`; see `e2e/scenarios/t3-read-only.md` for
//! the inputs (address, fresh pairing links, thread IDs) and how to run it.
//! It never starts or restarts the T3 server.
//!
//! Ways this could fail through the real app, enumerated before the test
//! (rows from e2e/scenarios/t3-client-failure-paths.md):
//!
//! T1 Pairing stores nothing, stores the token outside the keychain, or the
//!    token shows up in the log, the state folder or the evidence (F19).
//! T2 The sidebar's projects or chats differ from T3's own shell snapshot, or
//!    the models differ from the server's config.
//! T3 A long chat's messages differ from T3's own full thread snapshot, or
//!    older history never loads.
//! T4 Live updates from a running chat do not appear.
//! T5 After a silent network loss, a dropped connection, or a Bukno restart,
//!    items are duplicated or missing compared with T3's snapshot (F12-F15).
//! T6 Bukno sends anything but the read-only methods (F18).
//! T7 A wrong address, a revoked sign-in or an unknown message type crashes
//!    the app or shows no clear message (F3, F6, F9).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bukno_desktop::app::{BuknoApp, View};
use bukno_desktop::sources::T3ChatRef;
use bukno_desktop::transcript::document::Role;
use bukno_platform::paths::AppPaths;
use bukno_platform::process::InstanceLock;
use bukno_t3_client::http::Http;
use bukno_t3_client::pairing::{normalize_address, socket_url};
use bukno_t3_client::rpc::{ReadOnlyMethod, Session};
use bukno_t3_client::secret::{SystemKeychain, TokenVault};
use bukno_t3_client::{ConnectionStatus, PairingStatus};
use egui::{Event, Id, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const STEP_DT: f32 = 1.0 / 60.0;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

struct Run {
    evidence: PathBuf,
    state: PathBuf,
    work: PathBuf,
    checks: Vec<Value>,
    log: Vec<Value>,
    shots: usize,
    started: Instant,
}

impl Run {
    fn check(&mut self, id: &str, pass: bool, observed: Value) {
        eprintln!("[{}] {id}: {observed}", if pass { "pass" } else { "FAIL" });
        self.checks.push(json!({"id": id, "status": if pass { "pass" } else { "fail" }, "observed": observed}));
        self.write();
    }

    fn note(&mut self, step: &str, observed: Value) {
        eprintln!("  {step}: {observed}");
        self.log.push(json!({"t": self.started.elapsed().as_secs_f64(), "step": step, "observed": observed}));
        self.write();
    }

    fn write(&self) {
        let failed = self.checks.iter().any(|c| c["status"] == "fail");
        let report = json!({
            "scenario": "t3-read-only",
            "status": if failed { "fail" } else { "pass" },
            "verification": "real T3 server on this machine, read through the real Bukno app UI driven by egui_kittest; comparisons use T3's own HTTP snapshots",
            "t3_pinned_revision": bukno_t3_client::pinned::SOURCE_REVISION,
            "t3_pinned_server_version": bukno_t3_client::pinned::SERVER_VERSION,
            "checks": self.checks,
            "log": self.log,
            "seconds": self.started.elapsed().as_secs_f64(),
        });
        let _ = std::fs::write(self.evidence.join("result.json"), serde_json::to_string_pretty(&report).unwrap());
    }
}

struct App {
    harness: Harness<'static, BuknoApp>,
    _lock: InstanceLock,
}

fn launch(run: &Run) -> App {
    let paths = AppPaths { state_dir: run.state.clone(), work_dir: Some(run.work.clone()), overridden: true };
    let lock = InstanceLock::acquire(&run.state).expect("state lock");
    let store = bukno_storage::Store::open(&run.state).expect("open store");
    let harness = Harness::builder()
        .with_size(Vec2::new(1440.0, 900.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(1_000_000)
        .with_step_dt(STEP_DT)
        .wgpu()
        .build_eframe(move |cc| BuknoApp::real(&cc.egui_ctx, paths, store));
    App { harness, _lock: lock }
}

impl App {
    fn app(&mut self) -> &mut BuknoApp {
        self.harness.state_mut()
    }

    fn pump(&mut self, ms: u64) {
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until {
            self.harness.step();
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    fn wait(&mut self, timeout: Duration, mut done: impl FnMut(&BuknoApp) -> bool) -> bool {
        let until = Instant::now() + timeout;
        while Instant::now() < until {
            self.harness.step();
            if done(self.harness.state()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn shot(&mut self, run: &mut Run, name: &str) {
        run.shots += 1;
        self.pump(150);
        let file = run.evidence.join("screens").join(format!("{:02}-{name}.png", run.shots));
        if let Ok(image) = self.harness.render() {
            let _ = image.save(file);
        }
    }

    fn click(&mut self, label: &str) -> bool {
        let Some(node) = self.harness.query_by_label(label) else {
            return false;
        };
        node.click();
        self.pump(150);
        true
    }

    /// Replace a text field's content through keyboard events.
    fn type_into(&mut self, id: &str, text: &str) {
        self.harness.ctx.memory_mut(|m| m.request_focus(Id::new(id)));
        self.pump(60);
        self.harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        self.harness.key_press(egui::Key::Backspace);
        self.harness.event(Event::Text(text.into()));
        self.pump(60);
    }

    fn environment(&self) -> Option<Arc<bukno_t3_client::EnvironmentView>> {
        self.harness.state().t3.as_ref()?.view.environments.first().cloned()
    }

    /// User and assistant message texts in the document, in order.
    fn messages(&self) -> Vec<(bool, String)> {
        self.harness
            .state()
            .doc
            .messages
            .iter()
            .filter(|m| !m.meta.as_deref().is_some_and(|meta| meta.starts_with("Work")))
            .map(|m| (m.role == Role::User, m.source.clone()))
            .collect()
    }
}

// ----- Independent reads of T3 ------------------------------------------------

/// (ID, title) pairs of projects or chats.
type IdTitles = BTreeSet<(String, String)>;

struct Truth {
    rt: tokio::runtime::Runtime,
    http: Http,
    base: url::Url,
    env_id: String,
}

impl Truth {
    fn token(&self) -> bukno_t3_client::secret::Secret {
        SystemKeychain.load(&self.env_id).unwrap().expect("token in keychain")
    }

    fn get(&self, path: &str) -> Value {
        self.rt.block_on(self.http.get_json(&self.base, &self.token(), path)).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// Active projects and chats as T3's own HTTP shell snapshot has them.
    fn shell(&self) -> (IdTitles, IdTitles) {
        let snapshot = self.get("/api/orchestration/shell");
        let pairs = |list: &Value, title: &str| -> BTreeSet<(String, String)> {
            list.as_array()
                .unwrap()
                .iter()
                .filter(|t| {
                    t.get("archivedAt").is_none_or(Value::is_null) && t.get("deletedAt").is_none_or(Value::is_null)
                })
                .map(|t| (t["id"].as_str().unwrap().to_owned(), t[title].as_str().unwrap_or("").to_owned()))
                .collect()
        };
        (pairs(&snapshot["projects"], "title"), pairs(&snapshot["threads"], "title"))
    }

    /// Messages of a chat from T3's full HTTP thread snapshot, as Bukno shows them.
    fn messages(&self, thread: &str) -> (Vec<(bool, String)>, Vec<String>) {
        let snapshot = self.get(&format!("/api/orchestration/threads/{thread}"));
        let projection = &snapshot["projection"];
        let runs: std::collections::HashMap<String, String> = projection["runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["id"].as_str().unwrap().to_owned(), r["status"].as_str().unwrap().to_owned()))
            .collect();
        let mut messages = Vec::new();
        let mut ids = Vec::new();
        for row in projection["visibleTurnItems"].as_array().unwrap() {
            let item = &row["item"];
            let run = item["runId"].as_str().and_then(|r| runs.get(r)).map(String::as_str);
            if run == Some("rolled_back") {
                continue;
            }
            ids.push(item["id"].as_str().unwrap().to_owned());
            match item["type"].as_str().unwrap() {
                "user_message" => {
                    if item["inputIntent"] == "queued_turn" && run == Some("cancelled") {
                        ids.pop();
                        continue;
                    }
                    messages.push((true, item["text"].as_str().unwrap().to_owned()));
                }
                "assistant_message" => messages.push((false, item["text"].as_str().unwrap().to_owned())),
                "proposed_plan" => messages.push((false, item["markdown"].as_str().unwrap().to_owned())),
                _ => {}
            }
        }
        (messages, ids)
    }

    fn models(&self) -> BTreeSet<(String, Vec<String>)> {
        let token = self.token();
        self.rt.block_on(async {
            let ticket = self.http.websocket_ticket(&self.base, &token).await.unwrap();
            let session = Session::connect(&socket_url(&self.base, &ticket), Arc::new(|_: &str| {})).await.unwrap();
            let config = session.call(ReadOnlyMethod::GetConfig, json!({})).await.unwrap();
            config["providers"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["enabled"] == true)
                .map(|p| {
                    let models = p["models"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|m| m["slug"].as_str().unwrap().to_owned())
                        .collect();
                    (p["instanceId"].as_str().unwrap().to_owned(), models)
                })
                .collect()
        })
    }
}

/// Rows of the open chat in Bukno: (item id, status).
fn open_rows(app: &App) -> Vec<(String, String)> {
    app.harness
        .state()
        .t3
        .as_ref()
        .and_then(|t| t.open_thread())
        .map(|t| t.rows.iter().map(|r| (r.item.id.clone(), r.item.status.clone())).collect())
        .unwrap_or_default()
}

/// Compare the open chat with T3's snapshot until they agree or time runs out.
fn compare_open(app: &mut App, truth: &Truth, thread: &str, timeout: Duration) -> (bool, Value) {
    let until = Instant::now() + timeout;
    loop {
        app.pump(300);
        let (expected, expected_ids) = truth.messages(thread);
        let shown = app.messages();
        let ids: Vec<String> = open_rows(app).into_iter().map(|(id, _)| id).collect();
        let unique = ids.iter().collect::<BTreeSet<_>>().len() == ids.len();
        let same = shown == expected && ids == expected_ids;
        if (same && unique) || Instant::now() > until {
            let first_difference = shown.iter().zip(&expected).position(|(a, b)| a != b);
            return (
                same && unique,
                json!({
                    "messages_shown": shown.len(), "messages_in_t3": expected.len(),
                    "rows_shown": ids.len(), "rows_in_t3": expected_ids.len(),
                    "row_ids_unique": unique, "first_message_difference": first_difference,
                    "missing_rows": expected_ids.iter().filter(|i| !ids.contains(i)).count(),
                    "extra_rows": ids.iter().filter(|i| !expected_ids.contains(i)).count(),
                }),
            );
        }
    }
}

// ----- A local relay that can cut, drop and inject --------------------------

const FORWARD: u8 = 0;
/// Keep sockets open but pass nothing: a silent network loss.
const BLACKHOLE: u8 = 1;
/// Refuse new connections: the server is gone.
const REFUSE: u8 = 2;

#[derive(Default)]
struct RelayShared {
    connections: Mutex<Vec<tokio::task::AbortHandle>>,
    inject: Mutex<Vec<String>>,
    /// The latest `subscribeThread` request ID a client sent.
    thread_request: Mutex<Option<u64>>,
    notify: tokio::sync::Notify,
}

struct Relay {
    port: u16,
    mode: Arc<AtomicU8>,
    shared: Arc<RelayShared>,
}

impl Relay {
    fn start(rt: &tokio::runtime::Runtime, upstream: String) -> Self {
        let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let port = listener.local_addr().unwrap().port();
        let mode = Arc::new(AtomicU8::new(FORWARD));
        let shared = Arc::new(RelayShared::default());
        let (m, s) = (mode.clone(), shared.clone());
        rt.spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else { return };
                if m.load(Ordering::SeqCst) == REFUSE {
                    drop(client);
                    continue;
                }
                let Ok(server) = TcpStream::connect(&upstream).await else { continue };
                let (cr, cw) = client.into_split();
                let (sr, sw) = server.into_split();
                let up = tokio::spawn(client_to_server(cr, sw, m.clone(), s.clone()));
                let down = tokio::spawn(server_to_client(sr, cw, m.clone(), s.clone()));
                s.connections.lock().unwrap().extend([up.abort_handle(), down.abort_handle()]);
            }
        });
        Self { port, mode, shared }
    }

    fn set(&self, mode: u8) {
        self.mode.store(mode, Ordering::SeqCst);
    }

    /// Cut every open connection at once, as a crashed server would.
    fn drop_all(&self) {
        for handle in self.shared.connections.lock().unwrap().drain(..) {
            handle.abort();
        }
    }

    fn inject(&self, frames: Vec<String>) {
        self.shared.inject.lock().unwrap().extend(frames);
        self.shared.notify.notify_waiters();
    }
}

async fn hold_while_blackholed(mode: &AtomicU8) {
    while mode.load(Ordering::SeqCst) == BLACKHOLE {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// One complete WebSocket frame at the start of `buf`: (length, opcode, payload).
fn frame(buf: &[u8]) -> Option<(usize, u8, Vec<u8>)> {
    if buf.len() < 2 {
        return None;
    }
    let opcode = buf[0] & 0x0f;
    let masked = buf[1] & 0x80 != 0;
    let (len, mut at) = match buf[1] & 0x7f {
        126 if buf.len() >= 4 => (u16::from_be_bytes([buf[2], buf[3]]) as usize, 4),
        127 if buf.len() >= 10 => (u64::from_be_bytes(buf[2..10].try_into().unwrap()) as usize, 10),
        126 | 127 => return None,
        n => (n as usize, 2),
    };
    let mask = if masked {
        if buf.len() < at + 4 {
            return None;
        }
        at += 4;
        Some([buf[at - 4], buf[at - 3], buf[at - 2], buf[at - 1]])
    } else {
        None
    };
    if buf.len() < at + len {
        return None;
    }
    let mut payload = buf[at..at + len].to_vec();
    if let Some(mask) = mask {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= mask[i % 4];
        }
    }
    Some((at + len, opcode, payload))
}

fn text_frame(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = vec![0x81];
    if bytes.len() < 126 {
        out.push(bytes.len() as u8);
    } else if bytes.len() < 65_536 {
        out.push(126);
        out.extend((bytes.len() as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend((bytes.len() as u64).to_be_bytes());
    }
    out.extend(bytes);
    out
}

async fn client_to_server(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    mode: Arc<AtomicU8>,
    shared: Arc<RelayShared>,
) {
    let mut seen = Vec::new();
    let mut upgraded = false;
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let Ok(n) = from.read(&mut chunk).await else { return };
        if n == 0 {
            return;
        }
        hold_while_blackholed(&mode).await;
        if to.write_all(&chunk[..n]).await.is_err() {
            return;
        }
        // Read along (without changing anything) to learn the thread request ID.
        seen.extend_from_slice(&chunk[..n]);
        if !upgraded {
            let Some(end) = seen.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
            let head: Vec<u8> = seen.drain(..end + 4).collect();
            if !String::from_utf8_lossy(&head).to_ascii_lowercase().contains("upgrade: websocket") {
                // Plain HTTP: nothing to read along with.
                seen.clear();
                continue;
            }
            upgraded = true;
        }
        while let Some((len, opcode, payload)) = frame(&seen) {
            seen.drain(..len);
            if opcode != 1 {
                continue;
            }
            let Ok(value) = serde_json::from_slice::<Value>(&payload) else { continue };
            if value["_tag"] == "Request" && value["tag"] == "orchestration.subscribeThread" {
                *shared.thread_request.lock().unwrap() = value["id"].as_u64();
            }
        }
    }
}

async fn server_to_client(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    mode: Arc<AtomicU8>,
    shared: Arc<RelayShared>,
) {
    let mut buf = Vec::new();
    let mut upgraded = false;
    let mut plain = false;
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        tokio::select! {
            read = from.read(&mut chunk) => {
                let Ok(n) = read else { return };
                if n == 0 {
                    return;
                }
                hold_while_blackholed(&mode).await;
                buf.extend_from_slice(&chunk[..n]);
                if plain {
                    // An ordinary HTTP exchange: pass it through untouched.
                    if to.write_all(&buf).await.is_err() {
                        return;
                    }
                    buf.clear();
                    continue;
                }
                if !upgraded {
                    let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
                    let head: Vec<u8> = buf.drain(..end + 4).collect();
                    if to.write_all(&head).await.is_err() {
                        return;
                    }
                    if !head.starts_with(b"HTTP/1.1 101") {
                        plain = true;
                        if to.write_all(&buf).await.is_err() {
                            return;
                        }
                        buf.clear();
                        continue;
                    }
                    upgraded = true;
                }
                // Forward whole frames only, so injected frames land between them.
                while let Some((len, _, _)) = frame(&buf) {
                    let whole: Vec<u8> = buf.drain(..len).collect();
                    if to.write_all(&whole).await.is_err() {
                        return;
                    }
                }
            }
            () = shared.notify.notified() => {}
        }
        if upgraded && buf.is_empty() {
            let pending: Vec<String> = shared.inject.lock().unwrap().drain(..).collect();
            for text in pending {
                if to.write_all(&text_frame(&text)).await.is_err() {
                    return;
                }
            }
        }
    }
}

// ----- The flow ----------------------------------------------------------------

fn t3_cli(args: &[&str]) -> String {
    let output = Command::new(env("BUKNO_T3_CLI"))
        .arg(env("BUKNO_T3_CLI_ENTRY"))
        .args(args)
        .env("ELECTRON_RUN_AS_NODE", "1")
        .output()
        .expect("t3 CLI");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Active sessions with this client label, from `t3 auth session list`: (ID, scopes).
fn bukno_sessions(label: &str) -> Vec<(String, String)> {
    t3_cli(&["auth", "session", "list"])
        .split("\n\n")
        .filter(|block| block.lines().any(|l| l.trim().starts_with("client:") && l.contains(label)))
        .filter_map(|block| {
            let id = block.lines().find(|l| !l.trim().is_empty())?.trim().to_owned();
            let scopes =
                block.lines().find(|l| l.trim().starts_with("scopes:"))?.trim().trim_start_matches("scopes:").trim();
            Some((id, scopes.to_owned()))
        })
        .collect()
}

/// Commands T3 accepted after `since` (ISO time), from its own receipts table.
fn command_receipts(since: &str) -> Value {
    let script = format!(
        "import sqlite3,json\nc=sqlite3.connect('file:{}/.t3/userdata/statev2.sqlite?mode=ro',uri=True)\nrows=c.execute(\"select command_type, accepted_at, status from orchestration_v2_command_receipts where accepted_at >= ? order by accepted_at\",('{since}',)).fetchall()\nprint(json.dumps(rows))",
        std::env::var("HOME").unwrap()
    );
    let output = Command::new("python3").arg("-c").arg(script).output().expect("python3");
    serde_json::from_slice(&output.stdout).unwrap_or(json!(String::from_utf8_lossy(&output.stderr)))
}

fn files_containing(dir: &Path, needle: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&path) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(bytes) = std::fs::read(&p)
                && bytes.windows(needle.len()).any(|w| w == needle)
            {
                found.push(p.display().to_string());
            }
        }
    }
    found
}

fn now_iso() -> String {
    bukno_t3_client::time::now_utc()
}

#[test]
fn t3_read_only_flow() {
    if std::env::var("BUKNO_T3_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set BUKNO_T3_LIVE=1 (see e2e/scenarios/t3-read-only.md)");
        return;
    }
    let address = env("BUKNO_T3_ADDRESS");
    let links_file = PathBuf::from(env("BUKNO_T3_LINKS_FILE"));
    let links: Vec<String> = std::fs::read_to_string(&links_file).unwrap().lines().map(str::to_owned).collect();
    let _ = std::fs::remove_file(&links_file);
    assert!(links.len() >= 2, "two fresh pairing links are needed");
    let long_thread = env("BUKNO_T3_LONG_THREAD");
    let paged_thread = env("BUKNO_T3_PAGED_THREAD");
    let live_thread = env("BUKNO_T3_LIVE_THREAD");
    let live_seconds: u64 = std::env::var("BUKNO_T3_LIVE_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(45);

    let evidence = PathBuf::from(env("BUKNO_EVIDENCE_DIR"));
    let root = std::env::temp_dir().join(format!("bukno-t3-e2e-{}", std::process::id()));
    std::fs::create_dir_all(evidence.join("screens")).unwrap();
    let mut run = Run {
        state: root.join("state"),
        work: root.join("work"),
        evidence,
        checks: Vec::new(),
        log: Vec::new(),
        shots: 0,
        started: Instant::now(),
    };
    std::fs::create_dir_all(&run.work).unwrap();
    let test_started = now_iso();
    // T3 names a paired session after the pairing link's label when it has one.
    let label = std::env::var("BUKNO_T3_SESSION_LABEL").unwrap_or_else(|_| "Bukno end-to-end check".into());
    let sessions_before: BTreeSet<String> = bukno_sessions(&label).into_iter().map(|s| s.0).collect();

    // ----- Pair --------------------------------------------------------------
    let mut app = launch(&run);
    app.wait(Duration::from_secs(20), |a| a.setup.is_some());
    app.app().show_setup = false;
    app.pump(300);
    let opened = app.click("Add a T3 server");
    run.check("environments screen opens from the sidebar", opened && app.app().show_environments, json!(opened));
    app.shot(&mut run, "add-environment-empty");

    // Wrong address: nothing answers. The link is not sent anywhere.
    app.type_into("env-address", "http://127.0.0.1:9");
    app.type_into("env-link", &links[0]);
    app.click("Connect");
    let failed = app.wait(Duration::from_secs(20), |a| {
        a.t3.as_ref().is_some_and(|t| matches!(t.view.pairing, PairingStatus::Failed(_)))
    });
    let message = format!("{:?}", app.app().t3.as_ref().unwrap().view.pairing);
    run.check("F6 wrong address shows a clear message", failed && message.contains("Nothing answered"), json!(message));
    app.shot(&mut run, "wrong-address");

    app.type_into("env-address", &address);
    app.type_into("env-link", &links[0]);
    app.click("Connect");
    let paired = app.wait(Duration::from_secs(30), |a| {
        a.t3.as_ref().is_some_and(|t| matches!(t.view.pairing, PairingStatus::Paired { .. }))
    });
    let link_cleared = app.app().t3.as_ref().unwrap().form.link.is_empty();
    run.check(
        "pairs with a fresh pairing link",
        paired && link_cleared,
        json!({"paired": paired, "link_field_cleared": link_cleared}),
    );
    let connected = app.wait(Duration::from_secs(30), |a| {
        a.t3.as_ref()
            .and_then(|t| t.view.environments.first().cloned())
            .is_some_and(|e| e.status == ConnectionStatus::Connected { current: true } && !e.providers.is_empty())
    });
    let environment = app.environment().expect("environment");
    let env_id = environment.saved.environment_id.clone();
    run.check(
        "connects and catches up",
        connected,
        json!({"status": format!("{:?}", environment.status), "label": environment.saved.label, "address": environment.saved.address}),
    );
    app.shot(&mut run, "paired-status-and-models");
    let token = SystemKeychain.load(&env_id).ok().flatten();
    run.check(
        "T1 token is in the system keychain, read-only scope",
        token.is_some() && environment.saved.scope == "orchestration:read",
        json!({"keychain_entry": token.is_some(), "scope": environment.saved.scope}),
    );
    let session = bukno_sessions(&label).into_iter().find(|s| !sessions_before.contains(&s.0));
    run.check(
        "T6 the server holds Bukno's session as read only",
        session.as_ref().is_some_and(|(_, scopes)| scopes == "orchestration:read"),
        json!({"session_scopes": session.as_ref().map(|s| s.1.clone())}),
    );
    app.click("Done");

    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let truth = Truth { http: Http::new(), base: normalize_address(&address).unwrap(), env_id: env_id.clone(), rt };
    // The test's own reads send the token too: check the server first, as the app does.
    let answering = truth.rt.block_on(truth.http.descriptor(&truth.base)).expect("descriptor");
    assert_eq!(answering.environment_id, env_id, "the address answers as the paired environment");

    // ----- Lists --------------------------------------------------------------
    let (mut lists_ok, mut lists_observed) = (false, json!(null));
    for _ in 0..5 {
        app.pump(500);
        let env = app.environment().unwrap();
        let projects: BTreeSet<(String, String)> =
            env.projects.iter().map(|p| (p.id.clone(), p.title.clone())).collect();
        let threads: BTreeSet<(String, String)> = env.threads.iter().map(|t| (t.id.clone(), t.title.clone())).collect();
        let (t3_projects, t3_threads) = truth.shell();
        lists_ok = projects == t3_projects && threads == t3_threads;
        lists_observed = json!({
            "projects": projects.len(), "projects_in_t3": t3_projects.len(),
            "chats": threads.len(), "chats_in_t3": t3_threads.len(),
            "chats_only_in_bukno": threads.difference(&t3_threads).count(),
            "chats_only_in_t3": t3_threads.difference(&threads).count(),
            "top_level_chats_in_sidebar": env.threads.iter().filter(|t| t.parent_thread_id().is_none()).count(),
            "delegated_chats_counted_under_parents": env.threads.iter().filter(|t| t.parent_thread_id().is_some()).count(),
        });
        if lists_ok {
            break;
        }
    }
    run.check("T2 projects and chats match T3's shell snapshot", lists_ok, lists_observed);
    let models: BTreeSet<(String, Vec<String>)> = app
        .environment()
        .unwrap()
        .providers
        .iter()
        .filter(|p| p.enabled)
        .map(|p| (p.instance_id.clone(), p.models.iter().map(|m| m.slug.clone()).collect()))
        .collect();
    let t3_models = truth.models();
    run.check(
        "T2 available models match the server's config",
        models == t3_models,
        json!(models.iter().map(|(p, m)| format!("{p}: {} models", m.len())).collect::<Vec<_>>()),
    );
    app.shot(&mut run, "sidebar-lists");

    // ----- One long chat ------------------------------------------------------
    let long_title = app
        .environment()
        .unwrap()
        .threads
        .iter()
        .find(|t| t.id == long_thread)
        .map(|t| (t.title.clone(), t.model_selection.instance().to_owned()));
    let (title, instance) = long_title.expect("long thread in the list");
    let provider = if instance.contains("claude") { "Claude" } else { "Codex" };
    let env_view = app.environment().unwrap();
    let children = env_view.threads.iter().filter(|t| t.parent_thread_id() == Some(long_thread.as_str())).count();
    let delegated = match children {
        0 => String::new(),
        1 => ", with 1 delegated chat".to_owned(),
        n => format!(", with {n} delegated chats"),
    };
    let row_label = format!("{title}, {provider} chat on {}, read only{delegated}", env_view.saved.label);
    let clicked = app.click(&row_label);
    let loaded = app.wait(Duration::from_secs(30), |a| {
        a.view == View::Remote && a.t3.as_ref().and_then(|t| t.open_thread()).is_some_and(|t| t.loaded && t.current)
    });
    run.check("long chat opens from the sidebar", clicked && loaded, json!({"clicked": clicked, "loaded": loaded}));
    let (same, observed) = compare_open(&mut app, &truth, &long_thread, Duration::from_secs(5));
    run.check("T3 long chat matches T3's full snapshot", same, observed);
    app.shot(&mut run, "long-chat-end");

    // ----- Older history in pages ----------------------------------------------
    app.app().select_remote(T3ChatRef { environment: env_id.clone(), thread: paged_thread.clone() });
    app.wait(Duration::from_secs(30), App::thread_current_static);
    let first_window = open_rows(&app).len();
    let had_more = app.app().t3.as_ref().and_then(|t| t.open_thread()).is_some_and(|t| t.has_more_history);
    app.shot(&mut run, "paged-chat-first-window");
    let mut pages = 0;
    while app.app().t3.as_ref().and_then(|t| t.open_thread()).is_some_and(|t| t.has_more_history) && pages < 10 {
        if !app.click("Load older messages") {
            break;
        }
        pages += 1;
        app.wait(Duration::from_secs(20), |a| {
            a.t3.as_ref().and_then(|t| t.open_thread()).is_some_and(|t| !t.loading_history)
        });
    }
    let (same, mut observed) = compare_open(&mut app, &truth, &paged_thread, Duration::from_secs(5));
    observed["first_window_rows"] = json!(first_window);
    observed["pages_loaded"] = json!(pages);
    run.check("T3 older history loads in pages and then matches", had_more && pages > 0 && same, observed);
    app.shot(&mut run, "paged-chat-all-history");

    // ----- Live updates ---------------------------------------------------------
    app.app().select_remote(T3ChatRef { environment: env_id.clone(), thread: live_thread.clone() });
    app.wait(Duration::from_secs(30), App::thread_current_static);
    let before = open_rows(&app);
    let revision_before = app.app().t3.as_ref().and_then(|t| t.open_thread()).map_or(0, |t| t.revision);
    eprintln!("LIVE WINDOW OPEN for {live_seconds} s: make the live chat do something in T3");
    app.pump(live_seconds * 1000);
    let after = open_rows(&app);
    let revision_after = app.app().t3.as_ref().and_then(|t| t.open_thread()).map_or(0, |t| t.revision);
    let new_rows = after.iter().filter(|r| !before.iter().any(|b| b.0 == r.0)).count();
    run.check(
        "T4 live updates appear while the chat runs in T3",
        new_rows > 0,
        json!({"rows_before": before.len(), "rows_after": after.len(), "new_rows": new_rows, "revisions": revision_after - revision_before}),
    );
    app.shot(&mut run, "live-chat");
    let (same, observed) = compare_open(&mut app, &truth, &live_thread, Duration::from_secs(20));
    run.check("T4 live chat matches T3 after updates", same, observed);

    // ----- Restart Bukno, through the relay ------------------------------------
    drop(app);
    let relay = Relay::start(
        &truth.rt,
        truth.base.host_str().map(|h| format!("{h}:{}", truth.base.port().unwrap_or(80))).unwrap(),
    );
    let saved_path = run.state.join("t3-environments.json");
    let saved = std::fs::read_to_string(&saved_path).unwrap();
    let relay_address = format!("http://127.0.0.1:{}/", relay.port);
    std::fs::write(&saved_path, saved.replace(&truth.base.to_string(), &relay_address)).unwrap();
    run.note("restart through relay", json!({"relay": relay_address}));
    let mut app = launch(&run);
    app.wait(Duration::from_secs(20), |a| a.setup.is_some());
    app.app().show_setup = false;
    let reconnected = app.wait(Duration::from_secs(30), |a| {
        a.t3.as_ref()
            .and_then(|t| t.view.environments.first().cloned())
            .is_some_and(|e| e.status == ConnectionStatus::Connected { current: true })
    });
    app.app().select_remote(T3ChatRef { environment: env_id.clone(), thread: live_thread.clone() });
    app.wait(Duration::from_secs(30), App::thread_current_static);
    let (same, observed) = compare_open(&mut app, &truth, &live_thread, Duration::from_secs(20));
    run.check(
        "T5 after restarting Bukno the token comes from the keychain and the chat matches",
        reconnected && same,
        observed,
    );

    // ----- Silent network loss --------------------------------------------------
    let connections = app.environment().unwrap().connections;
    relay.set(BLACKHOLE);
    eprintln!("LIVE WINDOW OPEN (network cut) for 30 s: make the live chat do something in T3");
    let noticed = app.wait(Duration::from_secs(40), |a| {
        a.t3.as_ref()
            .and_then(|t| t.view.environments.first().cloned())
            .is_some_and(|e| matches!(e.status, ConnectionStatus::Reconnecting { .. }))
    });
    app.shot(&mut run, "network-cut-reconnecting");
    run.note(
        "network cut noticed",
        json!({"noticed": noticed, "status": format!("{:?}", app.environment().unwrap().status)}),
    );
    app.pump(5_000);
    relay.drop_all();
    relay.set(FORWARD);
    let back = app.wait(Duration::from_secs(60), |a| {
        a.t3.as_ref()
            .and_then(|t| t.view.environments.first().cloned())
            .is_some_and(|e| e.status == ConnectionStatus::Connected { current: true } && e.connections > connections)
    });
    app.wait(Duration::from_secs(30), App::thread_current_static);
    let duplicates_seen = app.app().t3.as_ref().and_then(|t| t.open_thread()).map_or(0, |t| t.counts.duplicates);
    let (same, mut observed) = compare_open(&mut app, &truth, &live_thread, Duration::from_secs(30));
    observed["noticed_within_40s"] = json!(noticed);
    observed["duplicates_dropped_by_sequence"] = json!(duplicates_seen);
    run.check(
        "T5 silent network loss: reconnects and catches up with no duplicate or missing items",
        noticed && back && same,
        observed,
    );
    app.shot(&mut run, "network-restored");

    // ----- Dropped connections (server gone, then back) ------------------------
    let connections = app.environment().unwrap().connections;
    relay.set(REFUSE);
    relay.drop_all();
    app.pump(6_000);
    relay.set(FORWARD);
    let back = app.wait(Duration::from_secs(60), |a| {
        a.t3.as_ref()
            .and_then(|t| t.view.environments.first().cloned())
            .is_some_and(|e| e.status == ConnectionStatus::Connected { current: true } && e.connections > connections)
    });
    app.wait(Duration::from_secs(30), App::thread_current_static);
    let (same, observed) = compare_open(&mut app, &truth, &live_thread, Duration::from_secs(30));
    run.check("T5 dropped connections: reconnects and matches T3", back && same, observed);

    // ----- Unknown message types ------------------------------------------------
    app.app().select_remote(T3ChatRef { environment: env_id.clone(), thread: long_thread.clone() });
    app.wait(Duration::from_secs(30), App::thread_current_static);
    app.pump(500);
    let request = *relay.shared.thread_request.lock().unwrap();
    let (last, ordinal) = {
        let thread = app.app().t3.as_ref().unwrap().open_thread().unwrap().clone();
        (thread.last_sequence.unwrap_or(0), thread.rows.iter().map(|r| r.item.ordinal).max().unwrap_or(0))
    };
    let at = now_iso();
    let item = json!({
        "type": "hologram", "id": "turn-item:injected-hologram", "threadId": long_thread, "runId": null,
        "nodeId": null, "providerThreadId": null, "providerTurnId": null, "nativeItemRef": null,
        "parentItemId": null, "ordinal": ordinal + 1, "status": "completed", "title": null,
        "startedAt": null, "completedAt": null, "updatedAt": at
    });
    relay.inject(vec![
        json!({"_tag": "Hologram", "note": "injected by the end-to-end check"}).to_string(),
        json!({"_tag": "Chunk", "requestId": request, "values": [{"kind": "teleport"}]}).to_string(),
        json!({"_tag": "Chunk", "requestId": request, "values": [
            {"kind": "event", "sequence": last + 1, "event": {"id": "evt-injected-1", "type": "turn-item.updated", "threadId": long_thread, "occurredAt": at, "payload": item}},
            {"kind": "event", "sequence": last + 2, "event": {"id": "evt-injected-2", "type": "galaxy.updated", "threadId": long_thread, "occurredAt": at, "payload": {}}}
        ]})
        .to_string(),
    ]);
    let shown = app.wait(Duration::from_secs(10), |a| {
        a.doc.messages.iter().any(|m| m.source.contains("Unsupported item: `hologram`"))
    });
    let log = std::fs::read_to_string(run.state.join("t3-client.log")).unwrap_or_default();
    let unknown_tag_logged = log.contains("unknown tag Some(\"Hologram\")");
    let unknown_event_logged = log.contains("unknown thread event type galaxy.updated");
    let still_connected = app.environment().unwrap().status == ConnectionStatus::Connected { current: true };
    run.check(
        "T7 unknown message types are shown or logged, not a crash",
        shown && unknown_tag_logged && unknown_event_logged && still_connected && request.is_some(),
        json!({"unsupported_row_shown": shown, "unknown_frame_logged": unknown_tag_logged, "unknown_event_logged": unknown_event_logged, "still_connected": still_connected}),
    );
    app.shot(&mut run, "unknown-item");

    // ----- Read only ----------------------------------------------------------------
    let log = std::fs::read_to_string(run.state.join("t3-client.log")).unwrap_or_default();
    let sent: BTreeSet<String> = log
        .lines()
        .filter_map(|l| l.split("rpc: send request ").nth(1))
        .filter_map(|rest| rest.split_whitespace().nth(1).map(str::to_owned))
        .collect();
    let allowed: BTreeSet<String> = ReadOnlyMethod::ALL.iter().map(|m| m.tag().to_owned()).collect();
    run.check(
        "T6 Bukno sent only read-only methods",
        !sent.is_empty() && sent.is_subset(&allowed),
        json!({"methods_sent": sent}),
    );
    run.note("T3 command receipts during the run (other clients, such as T3 itself)", command_receipts(&test_started));

    // ----- Revoked sign-in --------------------------------------------------------
    let session = session.map(|s| s.0);
    if let Some(id) = &session {
        t3_cli(&["auth", "session", "revoke", id]);
    }
    app.app().t3.as_ref().unwrap().reconnect(&env_id);
    let refused = app.wait(Duration::from_secs(30), |a| {
        a.t3.as_ref().and_then(|t| t.view.environments.first().cloned()).is_some_and(
            |e| matches!(&e.status, ConnectionStatus::NeedsPairing { reason } if reason.contains("no longer accepts")),
        )
    });
    run.check(
        "F3 revoked sign-in shows a clear message",
        session.is_some() && refused,
        json!({"status": format!("{:?}", app.environment().unwrap().status)}),
    );
    app.shot(&mut run, "revoked");

    // Pair again with the second link, so the run ends with a working pairing.
    app.app().show_environments = true;
    app.pump(200);
    app.type_into("env-address", &address);
    app.type_into("env-link", &links[1]);
    app.click("Connect");
    let paired_again = app.wait(Duration::from_secs(30), |a| {
        a.t3.as_ref().is_some_and(|t| matches!(t.view.pairing, PairingStatus::Paired { .. }))
    }) && app.wait(Duration::from_secs(30), |a| {
        a.t3.as_ref()
            .and_then(|t| t.view.environments.first().cloned())
            .is_some_and(|e| e.status == ConnectionStatus::Connected { current: true })
    });
    run.check("pairs again after revocation", paired_again, json!(paired_again));
    app.shot(&mut run, "paired-again");
    app.click("Done");

    // ----- No token anywhere it should not be -------------------------------------
    let tokens: Vec<_> = [token, SystemKeychain.load(&env_id).ok().flatten()].into_iter().flatten().collect();
    drop(app);
    std::fs::copy(run.state.join("t3-client.log"), run.evidence.join("t3-client.log")).ok();
    std::fs::copy(run.state.join("t3-environments.json"), run.evidence.join("t3-environments.json")).ok();
    // Every secret, in every form it could be written: bearer tokens, whole
    // pairing links, and the one-time credential inside each link, plain and
    // percent-encoded. Searched in the state folder and the evidence.
    let mut secrets: Vec<String> = tokens.iter().map(|t| t.expose().to_owned()).collect();
    for link in &links {
        secrets.push(link.clone());
        let parsed = bukno_t3_client::pairing::parse_pairing(None, link).expect("pairing link");
        let credential = parsed.credential.expose().to_owned();
        secrets.push(url::form_urlencoded::byte_serialize(credential.as_bytes()).collect());
        secrets.push(credential);
    }
    let mut leaks = Vec::new();
    for secret in &secrets {
        for dir in [&run.state, &run.evidence] {
            leaks.extend(files_containing(dir, secret.as_bytes()));
        }
    }
    leaks.sort();
    leaks.dedup();
    run.check(
        "T1 no token or pairing link in the log, state folder or evidence",
        leaks.is_empty() && !tokens.is_empty(),
        json!({"files": leaks, "secret_forms_searched": secrets.len()}),
    );
    run.write();
    eprintln!("evidence in {}", run.evidence.display());
    assert!(!run.checks.iter().any(|c| c["status"] == "fail"), "some checks failed; see result.json");
}

impl App {
    fn thread_current_static(app: &BuknoApp) -> bool {
        app.t3.as_ref().and_then(|t| t.open_thread()).is_some_and(|t| t.loaded && t.current)
    }
}
