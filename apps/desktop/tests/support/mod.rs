//! Shared pieces of the live T3 end-to-end checks: the app harness, T3's own
//! snapshots as the source of truth, a relay that can cut and drop
//! connections, and T3's command line.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bukno_desktop::app::BuknoApp;
use bukno_desktop::transcript::document::Role;
use bukno_platform::paths::AppPaths;
use bukno_platform::process::InstanceLock;
use bukno_t3_client::http::Http;
use bukno_t3_client::pairing::socket_url;
use bukno_t3_client::rpc::{Method, Session};
use bukno_t3_client::secret::{SystemKeychain, TokenVault};
use egui::{Event, Id, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const STEP_DT: f32 = 1.0 / 60.0;

pub fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

pub struct Run {
    pub scenario: &'static str,
    pub evidence: PathBuf,
    pub state: PathBuf,
    pub work: PathBuf,
    pub checks: Vec<Value>,
    pub log: Vec<Value>,
    pub shots: usize,
    pub started: Instant,
    /// Records a demo video of the run when `BUKNO_VIDEO_DIR` is set.
    pub video: Option<Arc<Mutex<Recorder>>>,
}

impl Run {
    /// Name the step on screen in the demo video, and in the log.
    pub fn caption(&mut self, text: &str) {
        eprintln!("== {text}");
        if let Some(video) = &self.video {
            video.lock().unwrap().caption(text);
        }
    }

    pub fn check(&mut self, id: &str, pass: bool, observed: Value) {
        eprintln!("[{}] {id}: {observed}", if pass { "pass" } else { "FAIL" });
        self.checks.push(json!({"id": id, "status": if pass { "pass" } else { "fail" }, "observed": observed}));
        self.write();
    }

    pub fn note(&mut self, step: &str, observed: Value) {
        eprintln!("  {step}: {observed}");
        self.log.push(json!({"t": self.started.elapsed().as_secs_f64(), "step": step, "observed": observed}));
        self.write();
    }

    pub fn write(&self) {
        let failed = self.checks.iter().any(|c| c["status"] == "fail");
        let report = json!({
            "scenario": self.scenario,
            "status": if failed { "fail" } else { "pass" },
            "verification": "real T3 server on this machine, through the real Bukno app UI driven by egui_kittest; comparisons use T3's own HTTP snapshots",
            "t3_pinned_revision": bukno_t3_client::pinned::SOURCE_REVISION,
            "t3_pinned_server_version": bukno_t3_client::pinned::SERVER_VERSION,
            "checks": self.checks,
            "log": self.log,
            "seconds": self.started.elapsed().as_secs_f64(),
        });
        let _ = std::fs::write(self.evidence.join("result.json"), serde_json::to_string_pretty(&report).unwrap());
    }
}

pub struct App {
    pub harness: Harness<'static, BuknoApp>,
    pub _lock: InstanceLock,
    pub video: Option<Arc<Mutex<Recorder>>>,
}

/// Frames of the real app as rendered by the harness, with a drawn pointer,
/// for a demo video at 10 frames a second. Interaction is recorded in real
/// time: a frame that took longer than 100 ms to capture is repeated to fill
/// the gap. While waiting on a provider one frame stands for 500 ms, so waits
/// play five times faster. A new caption holds the picture for two seconds.
/// `captions.srt` names each step. Frames are written on a separate thread.
pub struct Recorder {
    pub dir: PathBuf,
    pub frames: usize,
    last: Option<Instant>,
    pub pointer: egui::Pos2,
    captions: Vec<(usize, String)>,
    hold: usize,
    writer: Option<(std::sync::mpsc::Sender<FrameJob>, std::thread::JoinHandle<()>)>,
}

/// A picture and the frame files it fills.
type FrameJob = (image::RgbaImage, Vec<PathBuf>);

pub const VIDEO_FPS: usize = 10;
const CAPTION_HOLD_FRAMES: usize = 2 * VIDEO_FPS;

impl Recorder {
    pub fn from_env() -> Option<Arc<Mutex<Self>>> {
        let dir = PathBuf::from(std::env::var_os("BUKNO_VIDEO_DIR")?);
        std::fs::create_dir_all(dir.join("frames")).expect("video frame folder");
        let (tx, rx) = std::sync::mpsc::channel::<FrameJob>();
        let handle = std::thread::spawn(move || {
            for (image, paths) in rx {
                let Some((first, copies)) = paths.split_first() else { continue };
                let _ = image.save(first);
                for copy in copies {
                    // Repeats are links to the same picture, not new files.
                    let _ = std::fs::hard_link(first, copy);
                }
            }
        });
        Some(Arc::new(Mutex::new(Self {
            dir,
            frames: 0,
            last: None,
            pointer: egui::pos2(720.0, 450.0),
            captions: Vec::new(),
            hold: 0,
            writer: Some((tx, handle)),
        })))
    }

    pub fn caption(&mut self, text: &str) {
        self.captions.push((self.frames, text.to_owned()));
        self.hold = CAPTION_HOLD_FRAMES;
        self.write_captions();
    }

    fn due(&self, interval: Duration) -> bool {
        self.last.is_none_or(|t| t.elapsed() >= interval)
    }

    /// Add a picture that stands for `interval` of the run.
    fn add(&mut self, mut image: image::RgbaImage, interval: Duration) {
        draw_pointer(&mut image, self.pointer);
        let late = self.last.map_or(1, |t| {
            (t.elapsed().as_secs_f64() / interval.as_secs_f64().max(0.001)).round().clamp(1.0, 10.0) as usize
        });
        let copies = late + std::mem::take(&mut self.hold);
        let paths: Vec<PathBuf> =
            (self.frames..self.frames + copies).map(|i| self.dir.join("frames").join(format!("{i:06}.png"))).collect();
        if let Some((tx, _)) = &self.writer {
            let _ = tx.send((image, paths));
        }
        self.frames += copies;
        self.last = Some(Instant::now());
    }

    /// Wait until every frame is on disk.
    pub fn finish(&mut self) {
        if let Some((tx, handle)) = self.writer.take() {
            drop(tx);
            let _ = handle.join();
        }
        self.write_captions();
    }

    fn write_captions(&self) {
        let stamp = |frame: usize| {
            let ms = frame * 1000 / VIDEO_FPS;
            format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
        };
        let mut out = String::new();
        for (i, (start, text)) in self.captions.iter().enumerate() {
            let end = self.captions.get(i + 1).map_or(self.frames.max(start + 1), |(next, _)| *next).max(start + 1);
            out.push_str(&format!("{}\n{} --> {}\n{text}\n\n", i + 1, stamp(*start), stamp(end)));
        }
        let _ = std::fs::write(self.dir.join("captions.srt"), out);
    }
}

/// A plain arrow pointer with a dark outline, tip at `at`.
fn draw_pointer(image: &mut image::RgbaImage, at: egui::Pos2) {
    let shape = [(0.0, 0.0), (0.0, 17.0), (4.5, 13.0), (7.5, 20.0), (10.0, 19.0), (7.0, 12.5), (12.5, 12.5)];
    let inside = |x: f32, y: f32, grow: f32| {
        // Point in polygon, on the shape scaled up by `grow` around its middle.
        let pts: Vec<(f32, f32)> =
            shape.iter().map(|(px, py)| ((px - 5.0) * grow + 5.0, (py - 9.0) * grow + 9.0)).collect();
        let mut inside = false;
        let mut j = pts.len() - 1;
        for i in 0..pts.len() {
            let (xi, yi) = pts[i];
            let (xj, yj) = pts[j];
            if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
        inside
    };
    for dy in -2..24 {
        for dx in -2..16 {
            let (x, y) = (at.x as i64 + dx, at.y as i64 + dy);
            if x < 0 || y < 0 || x >= i64::from(image.width()) || y >= i64::from(image.height()) {
                continue;
            }
            let (fx, fy) = (dx as f32 + 0.5, dy as f32 + 0.5);
            let color = if inside(fx, fy, 1.0) {
                Some([250, 250, 250, 255])
            } else if inside(fx, fy, 1.25) {
                Some([20, 20, 20, 255])
            } else {
                None
            };
            if let Some(c) = color {
                image.put_pixel(x as u32, y as u32, image::Rgba(c));
            }
        }
    }
}

pub fn launch(run: &Run) -> App {
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
    App { harness, _lock: lock, video: run.video.clone() }
}

impl App {
    pub fn app(&mut self) -> &mut BuknoApp {
        self.harness.state_mut()
    }

    pub fn pump(&mut self, ms: u64) {
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until {
            self.harness.step();
            self.capture(Duration::from_millis(100));
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    pub fn wait(&mut self, timeout: Duration, mut done: impl FnMut(&BuknoApp) -> bool) -> bool {
        let until = Instant::now() + timeout;
        while Instant::now() < until {
            self.harness.step();
            self.capture(Duration::from_millis(500));
            if done(self.harness.state()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// Take a video frame if one is due.
    pub fn capture(&mut self, interval: Duration) {
        let Some(video) = self.video.clone() else { return };
        if !video.lock().unwrap().due(interval) {
            return;
        }
        if let Ok(image) = self.harness.render() {
            video.lock().unwrap().add(image, interval.max(Duration::from_millis(100)));
        }
    }

    /// Glide the drawn pointer to `to`, for the video.
    pub fn move_pointer(&mut self, to: egui::Pos2) {
        let Some(video) = self.video.clone() else { return };
        let from = video.lock().unwrap().pointer;
        let steps = ((to - from).length() / 60.0).clamp(3.0, 8.0) as usize;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let eased = t * t * (3.0 - 2.0 * t);
            video.lock().unwrap().pointer = from + (to - from) * eased;
            self.harness.step();
            self.capture(Duration::ZERO);
        }
    }

    pub fn shot(&mut self, run: &mut Run, name: &str) {
        run.shots += 1;
        self.pump(150);
        let file = run.evidence.join("screens").join(format!("{:02}-{name}.png", run.shots));
        if let Ok(image) = self.harness.render() {
            let _ = image.save(file);
        }
    }

    /// Click the first control with this accessible label.
    pub fn click(&mut self, label: &str) -> bool {
        let Some(node) = self.harness.query_all_by_label(label).next() else {
            return false;
        };
        node.scroll_to_me();
        self.pump(120);
        let Some(rect) = self.harness.query_all_by_label(label).next().map(|n| n.rect()) else {
            return false;
        };
        self.move_pointer(rect.center());
        let Some(node) = self.harness.query_all_by_label(label).next() else {
            return false;
        };
        node.hover();
        node.click();
        self.pump(150);
        true
    }

    /// Click the first control whose label contains `part`, such as a chat
    /// row whose label ends with its working state.
    pub fn click_contains(&mut self, part: &str) -> bool {
        let label = self.label_containing(part);
        label.is_some_and(|l| self.click(&l))
    }

    /// The full label of the first control whose label contains `part`.
    pub fn label_containing(&self, part: &str) -> Option<String> {
        self.harness
            .query_all_by_label_contains(part)
            .next()
            .and_then(|n| n.accesskit_node().label().map(|l| l.to_string()))
    }

    /// Click the first sidebar chat under `project` whose label contains
    /// `part`. Chat titles repeat across projects; rows follow their project.
    pub fn click_chat_in(&mut self, project: &str, part: &str) -> bool {
        // The project may sit below the visible part of the sidebar.
        if let Some(node) = self.harness.query_all_by_label(project).next() {
            node.scroll_to_me();
        }
        self.pump(200);
        let Some(header) = self.harness.query_all_by_label(project).next().map(|n| n.rect()) else {
            return false;
        };
        let label = self
            .harness
            .query_all_by_label_contains(part)
            .filter(|n| n.rect().top() > header.top())
            .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
            .and_then(|n| n.accesskit_node().label().map(|l| l.to_string()));
        let Some(label) = label else { return false };
        if let Some(node) = self
            .harness
            .query_all_by_label(&label)
            .filter(|n| n.rect().top() > header.top())
            .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        {
            node.scroll_to_me();
        }
        self.pump(200);
        let Some(header) = self.harness.query_all_by_label(project).next().map(|n| n.rect()) else {
            return false;
        };
        // The label is unique once its working state is included only if no
        // twin sits above this project; pick ours by position.
        let Some(rect) = self
            .harness
            .query_all_by_label(&label)
            .filter(|n| n.rect().top() > header.top())
            .map(|n| n.rect())
            .min_by(|a, b| a.top().total_cmp(&b.top()))
        else {
            return false;
        };
        self.move_pointer(rect.center());
        self.harness.event(Event::PointerMoved(rect.center()));
        self.harness.event(Event::PointerButton {
            pos: rect.center(),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        self.harness.event(Event::PointerButton {
            pos: rect.center(),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        self.pump(150);
        true
    }

    /// Hover a control, such as a sidebar row whose buttons show on hover.
    pub fn hover(&mut self, label: &str) -> bool {
        if let Some(node) = self.harness.query_all_by_label(label).next() {
            node.scroll_to_me();
        }
        self.pump(120);
        let Some(rect) = self.harness.query_all_by_label(label).next().map(|n| n.rect()) else {
            return false;
        };
        self.move_pointer(rect.center());
        if let Some(node) = self.harness.query_all_by_label(label).next() {
            node.hover();
        }
        self.pump(120);
        true
    }

    /// Type into the focused field a few characters at a time, as a person would.
    pub fn type_visibly(&mut self, id: &str, text: &str) {
        self.harness.ctx.memory_mut(|m| m.request_focus(Id::new(id)));
        self.pump(60);
        let chars: Vec<char> = text.chars().collect();
        let chunk = if self.video.is_some() { 3 } else { chars.len().max(1) };
        for part in chars.chunks(chunk) {
            self.harness.event(Event::Text(part.iter().collect()));
            self.harness.step();
            self.capture(Duration::from_millis(40));
        }
        self.pump(80);
    }

    /// Replace a text field's content through keyboard events.
    pub fn type_into(&mut self, id: &str, text: &str) {
        self.harness.ctx.memory_mut(|m| m.request_focus(Id::new(id)));
        self.pump(60);
        self.harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        self.harness.key_press(egui::Key::Backspace);
        self.harness.event(Event::Text(text.into()));
        self.pump(60);
    }

    pub fn environment(&self) -> Option<Arc<bukno_t3_client::EnvironmentView>> {
        self.harness.state().t3.as_ref()?.view.environments.first().cloned()
    }

    /// User and assistant message texts in the document, in order.
    pub fn messages(&self) -> Vec<(bool, String)> {
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
pub type IdTitles = BTreeSet<(String, String)>;

pub struct Truth {
    pub rt: tokio::runtime::Runtime,
    pub http: Http,
    pub base: url::Url,
    pub env_id: String,
}

impl Truth {
    pub fn token(&self) -> bukno_t3_client::secret::Secret {
        SystemKeychain.load(&self.env_id).unwrap().expect("token in keychain")
    }

    pub fn get(&self, path: &str) -> Value {
        self.rt.block_on(self.http.get_json(&self.base, &self.token(), path)).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// Active projects and chats as T3's own HTTP shell snapshot has them.
    pub fn shell(&self) -> (IdTitles, IdTitles) {
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
    pub fn messages(&self, thread: &str) -> (Vec<(bool, String)>, Vec<String>) {
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

    pub fn models(&self) -> BTreeSet<(String, Vec<String>)> {
        let token = self.token();
        self.rt.block_on(async {
            let ticket = self.http.websocket_ticket(&self.base, &token).await.unwrap();
            let session = Session::connect(&socket_url(&self.base, &ticket), Arc::new(|_: &str| {})).await.unwrap();
            let config = session.call(Method::GetConfig, json!({})).await.unwrap();
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
pub fn open_rows(app: &App) -> Vec<(String, String)> {
    app.harness
        .state()
        .t3
        .as_ref()
        .and_then(|t| t.open_thread())
        .map(|t| t.rows.iter().map(|r| (r.item.id.clone(), r.item.status.clone())).collect())
        .unwrap_or_default()
}

/// Compare the open chat with T3's snapshot until they agree or time runs out.
pub fn compare_open(app: &mut App, truth: &Truth, thread: &str, timeout: Duration) -> (bool, Value) {
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

pub const FORWARD: u8 = 0;
/// Keep sockets open but pass nothing: a silent network loss.
pub const BLACKHOLE: u8 = 1;
/// Refuse new connections: the server is gone.
pub const REFUSE: u8 = 2;
/// Pass requests to T3 but hold back everything it sends: a reply lost after
/// the server already acted.
pub const DROP_REPLIES: u8 = 3;

#[derive(Default)]
pub struct RelayShared {
    pub connections: Mutex<Vec<tokio::task::AbortHandle>>,
    pub inject: Mutex<Vec<String>>,
    /// The latest `subscribeThread` request ID a client sent.
    pub thread_request: Mutex<Option<u64>>,
    pub notify: tokio::sync::Notify,
}

pub struct Relay {
    pub port: u16,
    pub mode: Arc<AtomicU8>,
    pub shared: Arc<RelayShared>,
}

impl Relay {
    pub fn start(rt: &tokio::runtime::Runtime, upstream: String) -> Self {
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

    pub fn set(&self, mode: u8) {
        self.mode.store(mode, Ordering::SeqCst);
    }

    /// Cut every open connection at once, as a crashed server would.
    pub fn drop_all(&self) {
        for handle in self.shared.connections.lock().unwrap().drain(..) {
            handle.abort();
        }
    }

    pub fn inject(&self, frames: Vec<String>) {
        self.shared.inject.lock().unwrap().extend(frames);
        self.shared.notify.notify_waiters();
    }
}

pub async fn hold_while_blackholed(mode: &AtomicU8) {
    while mode.load(Ordering::SeqCst) == BLACKHOLE {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub async fn hold_replies(mode: &AtomicU8) {
    while matches!(mode.load(Ordering::SeqCst), BLACKHOLE | DROP_REPLIES) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// One complete WebSocket frame at the start of `buf`: (length, opcode, payload).
pub fn frame(buf: &[u8]) -> Option<(usize, u8, Vec<u8>)> {
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

pub fn text_frame(text: &str) -> Vec<u8> {
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

pub async fn client_to_server(
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

pub async fn server_to_client(
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
                hold_replies(&mode).await;
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

pub fn t3_cli(args: &[&str]) -> String {
    let output = Command::new(env("BUKNO_T3_CLI"))
        .arg(env("BUKNO_T3_CLI_ENTRY"))
        .args(args)
        .env("ELECTRON_RUN_AS_NODE", "1")
        .output()
        .expect("t3 CLI");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Active sessions with this client label, from `t3 auth session list`: (ID, scopes).
pub fn bukno_sessions(label: &str) -> Vec<(String, String)> {
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
pub fn command_receipts(since: &str) -> Value {
    let script = format!(
        "import sqlite3,json\nc=sqlite3.connect('file:{}/.t3/userdata/statev2.sqlite?mode=ro',uri=True)\nrows=c.execute(\"select command_type, accepted_at, status from orchestration_v2_command_receipts where accepted_at >= ? order by accepted_at\",('{since}',)).fetchall()\nprint(json.dumps(rows))",
        std::env::var("HOME").unwrap()
    );
    let output = Command::new("python3").arg("-c").arg(script).output().expect("python3");
    serde_json::from_slice(&output.stdout).unwrap_or(json!(String::from_utf8_lossy(&output.stderr)))
}

pub fn files_containing(dir: &Path, needle: &[u8]) -> Vec<String> {
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

pub fn now_iso() -> String {
    bukno_t3_client::time::now_utc()
}
