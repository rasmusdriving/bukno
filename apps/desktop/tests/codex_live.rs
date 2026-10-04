//! Pass 1 end-to-end: the real Bukno app, driven through egui_kittest and its
//! AccessKit tree, with the real coordinator, store and installed Codex
//! engine, using the existing Codex login.
//!
//! Runs only with `BUKNO_E2E_LIVE=1` (use `cargo xtask e2e --provider codex
//! --scenario pass1`), because it spends a little real usage. Everything else
//! skips it.
//!
//! Ways the Pass 1 flow could fail through the real app, enumerated before
//! this test (row numbers from e2e/scenarios/pass1-codex-failure-paths.md):
//!
//! L1 The project's chat never reaches Codex, or streams nothing (C02, C08, C09).
//! L2 A declined command runs anyway, or an allowed one does not (approval card).
//! L3 Bukno or Codex touches the unrelated dirty file (C31).
//! L4 Stop does not stop the turn, or the chat stays busy afterwards (C26).
//! L5 The engine dies mid-turn and the run is resent, or left unexplained (C19, C20).
//! L6 A typed draft is lost across quit and relaunch.
//! L7 After relaunch the chat starts a new Codex thread and forgets earlier context.
//! L8 A projectless chat writes outside its own folder, or loses its session.
//! L9 Quit leaves a Codex process running, or a second launch can take the
//!    state folder (C35, C38).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use bukno_core::decision::DecisionKind;
use bukno_desktop::app::{BuknoApp, QuitFlow, View};
use bukno_desktop::components::composer::composer_id;
use bukno_platform::paths::AppPaths;
use bukno_platform::process::{InstanceLock, LockError};
use bukno_runtime::UiCommand;
use egui::{Event, Key, Modifiers, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};

const STEP_DT: f32 = 1.0 / 60.0;
const TURN_TIMEOUT: Duration = Duration::from_secs(300);
const DRAFT: &str = "Utkast som ska överleva omstarten, med å, ä och ö";

struct Run {
    root: PathBuf,
    evidence: PathBuf,
    state: PathBuf,
    work: PathBuf,
    fixture: PathBuf,
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
    }

    fn write(&self) {
        let failed = self.checks.iter().any(|c| c["status"] == "fail");
        let report = json!({
            "scenario": "pass1",
            "provider": "codex",
            "status": if failed { "fail" } else { "pass" },
            "verification": "real Codex engine with the existing login, driven through the real app UI by egui_kittest",
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

    /// Run frames in real time for `ms`.
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
        let file = run.evidence.join("screens").join(format!("{:02}-{name}.png", run.shots));
        if let Ok(image) = self.harness.render() {
            let _ = image.save(file);
        }
    }

    /// Type into the composer through the same events a keyboard sends, then press Enter.
    fn send(&mut self, text: &str) {
        self.type_text(text);
        self.harness.key_press(Key::Enter);
        self.pump(100);
    }

    fn type_text(&mut self, text: &str) {
        self.harness.ctx.memory_mut(|m| m.request_focus(composer_id()));
        self.pump(50);
        self.harness.event(Event::Text(text.into()));
        self.pump(50);
    }

    fn click(&mut self, label: &str) -> bool {
        let Some(node) = self.harness.query_by_label(label) else {
            return false;
        };
        node.click();
        self.pump(100);
        true
    }

    fn transcript_text(&mut self) -> String {
        let doc = &self.harness.state().doc;
        doc.messages.iter().map(|m| m.source.clone()).collect::<Vec<_>>().join("\n---\n")
    }

    fn idle(&self) -> bool {
        let app = self.harness.state();
        app.run.is_none() && app.pending_submit.is_none() && app.extra.wait.is_none()
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(git_path())
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Bukno E2E")
        .env("GIT_AUTHOR_EMAIL", "e2e@bukno.invalid")
        .env("GIT_COMMITTER_NAME", "Bukno E2E")
        .env("GIT_COMMITTER_EMAIL", "e2e@bukno.invalid")
        .output()
        .expect("git");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn git_path() -> PathBuf {
    bukno_platform::discovery::find_git().expect("a working git")
}

fn sha256(path: &Path) -> Option<String> {
    let out = Command::new("shasum").args(["-a", "256"]).arg(path).output().ok()?;
    String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(str::to_owned)
}

/// Owned engine processes: anything running the Codex app-server whose
/// parent is this test process (Bukno spawns it directly).
fn engine_processes() -> Vec<Value> {
    let me = std::process::id().to_string();
    let out = Command::new("ps").args(["-axo", "pid=,ppid=,pgid=,command="]).output().expect("ps");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let rows: Vec<Vec<String>> = text
        .lines()
        .map(|l| {
            let mut parts = l.split_whitespace();
            let pid = parts.next().unwrap_or_default().to_owned();
            let ppid = parts.next().unwrap_or_default().to_owned();
            let pgid = parts.next().unwrap_or_default().to_owned();
            vec![pid, ppid, pgid, parts.collect::<Vec<_>>().join(" ")]
        })
        .collect();
    let leaders: Vec<String> =
        rows.iter().filter(|r| r[1] == me && r[3].contains("app-server")).map(|r| r[0].clone()).collect();
    rows.iter()
        .filter(|r| leaders.contains(&r[0]) || leaders.contains(&r[2]))
        .map(|r| {
            json!({
                "pid": r[0],
                "ppid": r[1],
                "pgid": r[2],
                // The engine Bukno started, as opposed to a tool it runs.
                "engine": leaders.contains(&r[0]),
                "command": r[3].chars().take(160).collect::<String>(),
            })
        })
        .collect()
}

fn db_summary(state: &Path) -> Value {
    let conn =
        rusqlite::Connection::open_with_flags(state.join("state.sqlite"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open db");
    let rows = |sql: &str| -> Vec<Value> {
        let mut stmt = conn.prepare(sql).unwrap();
        let n = stmt.column_count();
        stmt.query_map([], |r| {
            Ok(Value::Array(
                (0..n)
                    .map(|i| {
                        r.get::<_, rusqlite::types::Value>(i).map(|v| json!(format!("{v:?}"))).unwrap_or(Value::Null)
                    })
                    .collect(),
            ))
        })
        .unwrap()
        .filter_map(Result::ok)
        .collect()
    };
    json!({
        "runs": rows("SELECT r.task_id, r.attempt, r.state, r.turn_id IS NOT NULL, r.dismissed FROM run r ORDER BY r.created_at, r.attempt"),
        "deliveries": rows("SELECT task_id, state FROM message_delivery ORDER BY updated_at"),
        "sessions": rows("SELECT task_id, thread_id, latest_turn_id FROM provider_session"),
        "decisions": rows("SELECT kind, state FROM pending_decision ORDER BY updated_at"),
        "drafts": rows("SELECT task_id, length(text), revision FROM draft"),
        "items": rows("SELECT task_id, kind, completed, length(text) FROM transcript_item ORDER BY task_id, seq"),
    })
}

/// Answer every approval until the run ends: deny commands that mention
/// `deny`, allow everything else. Returns what was asked and answered.
fn answer_until_done(app: &mut App, run: &mut Run, deny: Option<&str>, tag: &str) -> Vec<Value> {
    let mut answered = Vec::new();
    let until = Instant::now() + TURN_TIMEOUT;
    let mut started = false;
    while Instant::now() < until {
        app.pump(100);
        let state = app.harness.state();
        if state.run.is_some() {
            started = true;
        }
        if let Some(decision) = state.extra.decisions.first().cloned() {
            if decision.state != bukno_core::decision::DecisionState::Pending {
                continue;
            }
            let (what, refuse) = match &decision.kind {
                DecisionKind::Command { command, .. } => {
                    (format!("command: {command}"), deny.is_some_and(|d| command.contains(d)))
                }
                DecisionKind::FileChange { files, .. } => (format!("file change: {files:?}"), false),
                DecisionKind::Question { questions } => (format!("question: {}", questions.len()), true),
                DecisionKind::Access { what, .. } => (format!("access: {what}"), true),
            };
            app.shot(run, &format!("{tag}-approval-{}", answered.len() + 1));
            let label = if refuse {
                if matches!(decision.kind, DecisionKind::Question { .. }) { "Skip" } else { "Deny" }
            } else {
                "Allow once"
            };
            let clicked = app.click(label);
            answered.push(json!({"asked": what, "answer": label, "clicked": clicked}));
            run.note("decision", json!({"asked": what, "answer": label}));
            app.pump(300);
            continue;
        }
        if started && app.idle() {
            break;
        }
    }
    answered
}

#[test]
fn codex_pass1_flow() {
    if std::env::var("BUKNO_E2E_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set BUKNO_E2E_LIVE=1 (cargo xtask e2e --provider codex --scenario pass1)");
        return;
    }
    let evidence = std::env::var_os("BUKNO_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("codex-live"));
    let root = std::env::temp_dir().join(format!("bukno-e2e-{}", std::process::id()));
    std::fs::create_dir_all(evidence.join("screens")).unwrap();
    let mut run = Run {
        state: root.join("state"),
        work: root.join("work åäö"),
        fixture: root.join("Bukno fixture åäö"),
        root: root.clone(),
        evidence,
        checks: Vec::new(),
        log: Vec::new(),
        shots: 0,
        started: Instant::now(),
    };
    std::fs::create_dir_all(&run.work).unwrap();

    // Fixture: a small Git project with an unrelated uncommitted change and a binary file.
    let fx = run.fixture.clone();
    std::fs::create_dir_all(fx.join("src")).unwrap();
    std::fs::write(fx.join("README.md"), "# Testprojekt\n\nEn liten fixtur för Bukno.\n").unwrap();
    std::fs::write(fx.join("src/lib.rs"), "pub fn hej() -> &'static str {\n    \"hej\"\n}\n").unwrap();
    std::fs::write(fx.join("notes.txt"), "Committed notes.\n").unwrap();
    std::fs::write(fx.join("logo.bin"), [0u8, 159, 146, 150, 255, 0, 1, 2]).unwrap();
    git(&fx, &["init", "-q"]);
    git(&fx, &["add", "."]);
    git(&fx, &["commit", "-q", "-m", "Fixture"]);
    std::fs::write(fx.join("notes.txt"), "Committed notes.\nAn unrelated edit the agent must not touch.\n").unwrap();
    let dirty_hash = sha256(&fx.join("notes.txt"));
    let start_tree = git(&fx, &["status", "--porcelain"]);
    run.note("fixture", json!({"path": fx.display().to_string(), "status": start_tree, "notes_sha256": dirty_hash}));

    // ----- First launch ----------------------------------------------------
    let mut app = launch(&run);
    let ready = app.wait(Duration::from_secs(20), |a| a.setup.is_some() && a.engine.is_some());
    let engine = app.app().engine.clone();
    run.check(
        "E01 engine discovered without a shell",
        ready && engine.as_ref().is_some_and(|e| e.version.is_some()),
        json!({"engine": format!("{engine:?}")}),
    );
    app.shot(&mut run, "launch");

    // A second launch on the same state folder is refused (C35).
    let second = InstanceLock::acquire(&run.state);
    run.check(
        "C35 second instance refused",
        matches!(second, Err(LockError::Held(_))),
        json!(format!("{:?}", second.as_ref().err().map(|e| e.to_string()))),
    );

    app.app().send_command(UiCommand::AddProject { path: fx.clone() });
    let added = app.wait(Duration::from_secs(10), |a| a.projects.len() == 1);
    let project = app.app().projects.first().cloned();
    run.check("project added", added, json!(format!("{project:?}")));
    let project = project.expect("project");
    app.app().new_chat(Some(project.id));
    app.pump(200);
    app.shot(&mut run, "new-project-chat");

    // ----- A: streamed work, one declined and one allowed command -----------
    let prompt = "Do these three steps in order. 1) Create a file named hello.txt whose only content is the line: Hej från Bukno 2) Run the shell command `touch declined.txt` 3) Run the shell command `touch allowed.txt`. Run each shell command on its own, not combined. Do not touch notes.txt. Then reply with one short sentence saying what happened.";
    app.send(prompt);
    let opened = app.wait(Duration::from_secs(20), |a| a.view == View::Chat && a.selected.is_some());
    run.check("A message accepted into a new project chat", opened, json!({}));
    let project_task = app.app().selected.expect("chat selected");
    let streamed =
        app.wait(TURN_TIMEOUT, |a| a.run.as_ref().is_some_and(|r| r.has_output) || !a.extra.decisions.is_empty());
    run.note("first output or approval", json!(streamed));
    app.shot(&mut run, "a-working");
    let engine_during = engine_processes();
    let answered = answer_until_done(&mut app, &mut run, Some("declined"), "a");
    app.pump(500);
    app.shot(&mut run, "a-done");
    let reply = app.transcript_text();
    let hello = std::fs::read_to_string(fx.join("hello.txt")).ok();
    run.check(
        "E02 file written by Codex",
        hello.as_deref().map(str::trim) == Some("Hej från Bukno"),
        json!({"hello.txt": hello}),
    );
    run.check(
        "E04 declined command did not run, allowed one did",
        !fx.join("declined.txt").exists() && fx.join("allowed.txt").exists(),
        json!({"declined_exists": fx.join("declined.txt").exists(), "allowed_exists": fx.join("allowed.txt").exists(), "answered": answered}),
    );
    run.check(
        "C31 unrelated dirty file unchanged",
        sha256(&fx.join("notes.txt")) == dirty_hash,
        json!({"before": dirty_hash, "after": sha256(&fx.join("notes.txt"))}),
    );
    run.check("A reply streamed into the transcript", reply.len() > prompt.len() + 10, json!({"chars": reply.len()}));

    // ----- B: Stop during a long reply ---------------------------------------
    app.send("Write the numbers from 1 to 3000, one per line, with nothing else. Do not run any commands.");
    let streaming = app.wait(TURN_TIMEOUT, |a| a.run.as_ref().is_some_and(|r| r.has_output));
    app.pump(800);
    app.shot(&mut run, "b-streaming");
    let clicked = app.click("Stop");
    let stopped = app.wait(Duration::from_secs(30), |a| a.run.is_none());
    app.pump(300);
    app.shot(&mut run, "b-stopped");
    run.check(
        "E05 Stop ends the turn",
        streaming && clicked && stopped,
        json!({"streaming": streaming, "clicked": clicked}),
    );

    // ----- B2: the engine dies mid-turn ---------------------------------------
    app.send("Count from 1 to 2000, one number per line, with nothing else. Do not run any commands.");
    let busy = app.wait(TURN_TIMEOUT, |a| a.run.as_ref().is_some_and(|r| r.has_output));
    let leaders: Vec<String> = engine_processes()
        .iter()
        .filter(|p| p["engine"] == json!(true))
        .filter_map(|p| p["pid"].as_str().map(str::to_owned))
        .collect();
    for pid in &leaders {
        let _ = Command::new("kill").args(["-9", pid]).status();
    }
    let unknown = app.wait(Duration::from_secs(30), |a| a.extra.unknown.is_some() || a.run.is_none());
    app.pump(300);
    app.shot(&mut run, "b2-engine-lost");
    // Bukno reconnects by itself to look the message up; it never resends it.
    let reconciled = app.wait(Duration::from_secs(60), |a| {
        a.extra.unknown.as_ref().is_some_and(|u| !u.explanation.contains("Checking")) || a.extra.unknown.is_none()
    });
    app.pump(500);
    let explanation = app.app().extra.unknown.as_ref().map(|u| u.explanation.clone());
    app.shot(&mut run, "b2-reconciled");
    run.check(
        "C19/C20 engine loss is explained and reconciled, not resent",
        busy && !leaders.is_empty() && unknown && reconciled,
        json!({"killed": leaders, "explanation": explanation}),
    );
    if app.app().extra.unknown.is_some() {
        app.click("Dismiss");
    }

    // ----- C: a draft that must survive quit -----------------------------------
    app.type_text(DRAFT);
    app.pump(1_500);
    app.shot(&mut run, "c-draft");

    // ----- Projectless chat -----------------------------------------------------
    app.app().new_chat(None);
    app.pump(200);
    app.send(
        "Create a file named notes.md whose only content is the line: projektlös. Then reply with one short sentence.",
    );
    let opened = app.wait(Duration::from_secs(20), |a| a.view == View::Chat && a.selected != Some(project_task));
    let loose_task = app.app().selected;
    let loose_answered = answer_until_done(&mut app, &mut run, None, "e03");
    app.pump(500);
    app.shot(&mut run, "e03-done");
    let chat_folder = loose_task.map(|t| run.work.join("chats").join(t.hex()).join("workspace"));
    let notes = chat_folder.as_ref().and_then(|f| std::fs::read_to_string(f.join("notes.md")).ok());
    run.check(
        "E03 projectless chat works in its own folder",
        opened && notes.as_deref().map(str::trim) == Some("projektlös") && !fx.join("notes.md").exists(),
        json!({"folder": chat_folder.map(|f| f.display().to_string()), "notes.md": notes, "answered": loose_answered}),
    );

    // ----- Quit -----------------------------------------------------------------
    app.app().confirm_quit();
    let quit = app.wait(Duration::from_secs(30), |a| a.quit == QuitFlow::Done);
    let after_quit = engine_processes();
    run.check(
        "C38 quit ends the engine and leaves nothing running",
        quit && after_quit.is_empty(),
        json!({"during": engine_during, "after": after_quit}),
    );
    drop(app);
    std::fs::write(
        run.evidence.join("processes.json"),
        serde_json::to_string_pretty(&json!({"during_first_turn": engine_during, "after_first_quit": after_quit}))
            .unwrap(),
    )
    .unwrap();
    let first_db = db_summary(&run.state);

    // ----- D: relaunch, draft restored, chat resumes with its context ---------
    let mut app = launch(&run);
    let loaded = app.wait(Duration::from_secs(20), |a| a.chats.len() == 2);
    let title = app.app().chats.iter().find(|c| c.task == project_task).map(|c| c.title.clone()).unwrap_or_default();
    app.app().select_chat(project_task);
    let restored = app.wait(Duration::from_secs(10), |a| a.composer.text == DRAFT);
    app.pump(300);
    app.shot(&mut run, "d-relaunched");
    run.check(
        "L6 draft restored after quit and relaunch",
        loaded && restored,
        json!({"title": title, "composer": app.app().composer.text.clone()}),
    );

    // Replace the draft with a follow-up that needs the earlier context.
    app.harness.ctx.memory_mut(|m| m.request_focus(composer_id()));
    app.pump(50);
    app.harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    app.pump(50);
    app.send("Without running any command or reading any file, tell me the exact line you wrote into hello.txt earlier in this chat.");
    let answered_d = answer_until_done(&mut app, &mut run, Some(""), "d");
    app.pump(500);
    app.shot(&mut run, "d-resumed");
    let resumed_text = app.transcript_text();
    let tail: String = resumed_text.chars().rev().take(400).collect::<String>().chars().rev().collect();
    run.check(
        "E06 resumed chat remembers earlier context",
        tail.contains("Hej från Bukno"),
        json!({"reply_tail": tail, "answered": answered_d}),
    );

    if let Some(loose) = loose_task {
        app.app().select_chat(loose);
        app.pump(500);
        app.send("Without running any command or reading any file, what was the only line you wrote into notes.md?");
        answer_until_done(&mut app, &mut run, Some(""), "e03b");
        app.pump(500);
        app.shot(&mut run, "e03-resumed");
        let text = app.transcript_text();
        let tail: String = text.chars().rev().take(300).collect::<String>().chars().rev().collect();
        run.check(
            "E03 projectless chat resumes after relaunch",
            tail.contains("projektlös"),
            json!({"reply_tail": tail}),
        );
    }

    app.app().confirm_quit();
    let quit = app.wait(Duration::from_secs(30), |a| a.quit == QuitFlow::Done);
    let after = engine_processes();
    run.check("C38 second quit leaves nothing running", quit && after.is_empty(), json!({"after": after}));
    drop(app);

    // ----- Evidence ---------------------------------------------------------------
    let second_db = db_summary(&run.state);
    let sessions_kept = first_db["sessions"] == second_db["sessions"]
        || first_db["sessions"].as_array().map(|s| s.len()) == second_db["sessions"].as_array().map(|s| s.len());
    run.check(
        "L7 same Codex threads after relaunch",
        sessions_kept,
        json!({"before": first_db["sessions"], "after": second_db["sessions"]}),
    );
    std::fs::write(
        run.evidence.join("db.json"),
        serde_json::to_string_pretty(&json!({"after_first_quit": first_db, "after_second_quit": second_db})).unwrap(),
    )
    .unwrap();
    let patch = git(&fx, &["diff"]);
    let status = git(&fx, &["status", "--porcelain"]);
    std::fs::write(
        run.evidence.join("changes.patch"),
        format!("# git status --porcelain\n{status}\n# git diff\n{patch}"),
    )
    .unwrap();
    let files = json!({
        "hello.txt": std::fs::read_to_string(fx.join("hello.txt")).ok(),
        "allowed.txt": fx.join("allowed.txt").exists(),
        "declined.txt": fx.join("declined.txt").exists(),
        "notes.txt_sha256": {"before": dirty_hash, "after": sha256(&fx.join("notes.txt"))},
    });
    std::fs::write(run.evidence.join("files.json"), serde_json::to_string_pretty(&files).unwrap()).unwrap();
    run.note("root", json!(run.root.display().to_string()));
    run.write();
    let failed: Vec<_> = run.checks.iter().filter(|c| c["status"] == "fail").map(|c| c["id"].clone()).collect();
    assert!(failed.is_empty(), "failed checks: {failed:?}");
}

/// E17 and the second preset. A newer Codex that fails at startup shows the
/// last working version; Use it pins that exact file and a task completes;
/// Try latest version clears the pin. Then "Ask before changes" asks before
/// a file is written, and a denied change leaves no file.
///
/// Failure paths enumerated first:
/// V1 A broken newer engine leaves Bukno stuck or names no way back (C04).
/// V2 Reverting changes the installed engine, or does not stick (section 9a).
/// V3 Try latest version keeps the pin.
/// V4 "Ask before changes" writes without asking, or writes after a Deny.
#[test]
fn codex_revert_and_preset_flow() {
    if std::env::var("BUKNO_E2E_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set BUKNO_E2E_LIVE=1");
        return;
    }
    let base = std::env::var_os("BUKNO_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("codex-live"));
    let evidence = base.join("revert-and-preset");
    let root = std::env::temp_dir().join(format!("bukno-e2e-revert-{}", std::process::id()));
    std::fs::create_dir_all(evidence.join("screens")).unwrap();
    let mut run = Run {
        state: root.join("state"),
        work: root.join("work"),
        fixture: root.join("unused"),
        root: root.clone(),
        evidence,
        checks: Vec::new(),
        log: Vec::new(),
        shots: 0,
        started: Instant::now(),
    };
    std::fs::create_dir_all(&run.work).unwrap();
    std::fs::create_dir_all(&run.state).unwrap();

    // The broken "newer" engine, and a record that the installed one worked.
    let broken = root.join("broken-codex");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../e2e/fixtures/broken-codex.c");
    let built = Command::new("cc").arg(&source).arg("-o").arg(&broken).status().expect("cc");
    assert!(built.success(), "build the broken engine");
    let real = bukno_platform::discovery::find_engine("codex").expect("installed Codex").native;
    let real_hash = sha256(&real).expect("hash");
    let version_out = Command::new(&real).arg("--version").output().expect("version");
    let real_version = String::from_utf8_lossy(&version_out.stdout).split_whitespace().last().unwrap().to_owned();
    std::fs::write(
        run.state.join("engines.json"),
        serde_json::to_string_pretty(&json!({"codex": {"lastWorking": {
            "version": real_version, "path": real.display().to_string(), "sha256": real_hash
        }, "pinned": null, "chosen": null}}))
        .unwrap(),
    )
    .unwrap();
    // SAFETY: set before the app starts any threads that read it.
    unsafe { std::env::set_var("BUKNO_CODEX_PATH", &broken) };

    let mut app = launch(&run);
    app.wait(Duration::from_secs(20), |a| a.engine.is_some() && a.setup.is_some());
    app.app().show_setup = true;
    app.pump(200);
    app.click("Check again");
    let failed = app.wait(Duration::from_secs(30), |a| {
        a.engine.as_ref().is_some_and(|e| e.state == bukno_runtime::engines::EngineState::NotWorking)
    });
    app.pump(300);
    app.shot(&mut run, "broken-engine");
    let view = app.app().engine.clone();
    let offers = view.as_ref().and_then(|v| v.revert.clone());
    run.check(
        "V1 broken newer engine names the last working version",
        failed
            && view.as_ref().and_then(|v| v.version.clone()).as_deref() == Some("0.999.0")
            && matches!(&offers, Some(bukno_runtime::engines::Revert::UseFile { version, .. }) if *version == real_version),
        json!(format!("{view:?}")),
    );

    let clicked = app.click(&format!("Use {real_version}"));
    let pinned = app.wait(Duration::from_secs(10), |a| a.engine.as_ref().is_some_and(|e| e.pinned.is_some()));
    app.click("Check again");
    let ready = app.wait(Duration::from_secs(30), |a| {
        a.engine.as_ref().is_some_and(|e| e.state == bukno_runtime::engines::EngineState::Ready)
    });
    app.pump(300);
    app.shot(&mut run, "reverted");
    run.check(
        "V2 Use the last working version pins it and it starts",
        clicked && pinned && ready && sha256(&real).as_deref() == Some(real_hash.as_str()),
        json!(format!("{:?}", app.app().engine)),
    );

    app.click("Continue with Codex");
    app.app().new_chat(None);
    app.pump(200);
    app.send("Reply with exactly the word: ready");
    answer_until_done(&mut app, &mut run, Some(""), "revert");
    app.pump(300);
    let text = app.transcript_text();
    app.shot(&mut run, "task-on-pinned-engine");
    run.check(
        "E17 a task completes on the pinned version",
        text.to_lowercase().contains("ready"),
        json!({"transcript_chars": text.len()}),
    );

    // The second preset, chosen through the permission menu.
    let opened = app.click("Permissions: Ask before commands");
    let chose = app.click("Ask before changes");
    let set = app
        .wait(Duration::from_secs(5), |a| a.setup.as_ref().is_some_and(|s| s.preset == "codex.read-only.on-request"));
    app.shot(&mut run, "preset-chosen");
    let folder = app.app().selected.map(|t| run.work.join("chats").join(t.hex()).join("workspace")).expect("chat");
    app.send("Create a file named blocked.txt in the current folder containing the word hi. Then reply with one short sentence.");
    let answered = answer_until_done(&mut app, &mut run, Some(""), "preset");
    app.pump(300);
    app.shot(&mut run, "preset-denied");
    let asked = answered.iter().any(|a| a["answer"] == "Deny");
    run.check(
        "V4 Ask before changes asks first, and a denied change writes nothing",
        opened && chose && set && asked && !folder.join("blocked.txt").exists(),
        json!({"answered": answered, "blocked_exists": folder.join("blocked.txt").exists()}),
    );

    app.app().show_setup = true;
    app.pump(200);
    app.click("Try latest version");
    let cleared = app.wait(Duration::from_secs(10), |a| a.engine.as_ref().is_some_and(|e| e.pinned.is_none()));
    app.pump(200);
    app.shot(&mut run, "try-latest");
    run.check(
        "V3 Try latest version clears the pin",
        cleared && app.app().engine.as_ref().and_then(|e| e.version.clone()).as_deref() == Some("0.999.0"),
        json!(format!("{:?}", app.app().engine)),
    );
    app.app().show_setup = false;
    app.app().confirm_quit();
    let quit = app.wait(Duration::from_secs(30), |a| a.quit == QuitFlow::Done);
    run.check("quit cleanly", quit && engine_processes().is_empty(), json!({}));
    drop(app);
    run.note("root", json!(run.root.display().to_string()));
    run.write();
    let failed: Vec<_> = run.checks.iter().filter(|c| c["status"] == "fail").map(|c| c["id"].clone()).collect();
    assert!(failed.is_empty(), "failed checks: {failed:?}");
}
