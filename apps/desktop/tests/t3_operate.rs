//! Stage 2 end-to-end: the real Bukno app, driven through egui_kittest and its
//! AccessKit tree, working in a real T3 server: new chats with Codex and
//! Claude, approvals allowed and denied, a question, queue, steer, Stop and
//! Resume, a restart in the middle of a run, and lost replies.
//!
//! Runs only with `BUKNO_T3_LIVE=1`; `scripts/t3-operate-e2e.sh` prepares a
//! disposable project and a pairing link. See `e2e/scenarios/t3-operate.md`.
//! It uses a little real Codex and Claude usage. It never restarts T3.
//!
//! Ways this could fail through the real app, enumerated before the test
//! (rows from e2e/scenarios/t3-operate-failure-paths.md):
//!
//! S1 The sign-in lacks the operate scope, so nothing can be sent (O1).
//! S2 A new chat or message runs twice, or never reaches T3 (O4, O19).
//! S3 An approval card does not appear, Enter answers it by accident, or the
//!    answer (allow or deny) is not what T3 records (O7, O9).
//! S4 A question's answer is sent as its label instead of its value, or the
//!    card never clears (O21).
//! S5 A queued message starts before the turn ends, Steer or Steer now does
//!    not reach the running turn, or the labels claim what T3 did not do (O6).
//! S6 Stop does not stop, or starts queued messages instead of holding them;
//!    Resume does not release them (O10, O11).
//! S7 After a restart the draft is lost, the open chat is not reopened, or a
//!    running turn cannot be stopped (O15, O16).
//! S8 A lost reply duplicates a message on retry, or a message T3 did run is
//!    shown as unsent, including when Bukno quits before the reply (O3, O15).
//! S9 Sending while disconnected writes anything, or loses the draft (O5).
//! S10 A dirty file in the workspace changes, or chat text and tokens reach
//!    the log, state folder or evidence (O18, O20).

mod support;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bukno_desktop::app::{BuknoApp, View};
use bukno_t3_client::http::Http;
use bukno_t3_client::hub::{OutboxState, ThreadView};
use bukno_t3_client::pairing::normalize_address;
use bukno_t3_client::secret::{SystemKeychain, TokenVault};
use bukno_t3_client::thread::PendingKind;
use bukno_t3_client::{ConnectionStatus, PairingStatus};
use egui::{Key, Modifiers};
use serde_json::{Value, json};
use support::*;

const COMPOSER: &str = "bukno-composer";

fn thread(app: &BuknoApp) -> Option<ThreadView> {
    app.t3.as_ref()?.open_thread().cloned()
}

fn connected(app: &BuknoApp) -> bool {
    app.t3
        .as_ref()
        .and_then(|t| t.view.environments.first().cloned())
        .is_some_and(|e| e.status == ConnectionStatus::Connected { current: true })
}

fn idle(app: &BuknoApp) -> bool {
    thread(app).is_some_and(|t| t.current && t.active_run.is_none() && t.queued.iter().all(|q| q.held))
}

/// The open chat's newest user message with this text, and what T3 did with it.
fn user_row(app: &BuknoApp, text: &str) -> Option<(String, Option<String>)> {
    thread(app)?.rows.iter().rev().find_map(|r| match &r.item.kind {
        bukno_t3_client::model::TurnKind::UserMessage { text: t, intent, .. } if t == text => {
            Some((intent.clone(), r.item.run_id.clone()))
        }
        _ => None,
    })
}

/// T3's full projection of a chat, from its own HTTP endpoint.
fn projection(truth: &Truth, thread: &str) -> Value {
    truth.get(&format!("/api/orchestration/threads/{thread}"))["projection"].clone()
}

/// How often T3 holds a user message with exactly this text in the chat.
fn copies(projection: &Value, text: &str) -> usize {
    projection["turnItems"]
        .as_array()
        .map(|items| items.iter().filter(|i| i["type"] == "user_message" && i["text"] == text).count())
        .unwrap_or(0)
}

fn request_records(projection: &Value) -> Vec<Value> {
    projection["runtimeRequests"]
        .as_array()
        .map(|r| {
            r.iter().map(|r| json!({"kind": r["kind"], "status": r["status"], "decision": r["decision"], "answers": r["answers"]})).collect()
        })
        .unwrap_or_default()
}

/// Type a message into the composer and press Enter (or Alt+Enter to steer).
fn send(app: &mut App, text: &str, steer: bool) {
    app.type_visibly(COMPOSER, text);
    if steer {
        app.harness.key_press_modifiers(Modifiers::ALT, Key::Enter);
    } else {
        app.harness.key_press(Key::Enter);
    }
    app.pump(200);
}

fn wait_pending(app: &mut App, timeout: Duration) -> Option<bukno_t3_client::thread::PendingRequest> {
    app.wait(timeout, |a| thread(a).is_some_and(|t| !t.pending.is_empty()));
    thread(app.harness.state()).and_then(|t| t.pending.first().cloned())
}

/// Start a new chat in the project with the given model, and send its first message.
fn new_chat(app: &mut App, project: &str, server: &str, model_label: &str, text: &str) -> Option<String> {
    app.hover(project);
    if !app.click(&format!("New chat in {project} on {server}")) {
        return None;
    }
    app.pump(200);
    let current = app.harness.state().remote_new.as_ref().and_then(|n| n.model.clone());
    let picker = app.label_containing("Model: ").unwrap_or_default();
    if !picker.ends_with(model_label.split(" · ").nth(1).unwrap_or(model_label)) || current.is_none() {
        app.click(&picker);
        app.pump(200);
        app.click(model_label);
    }
    app.pump(200);
    send(app, text, false);
    let opened = app
        .wait(Duration::from_secs(150), |a| a.view == View::Remote && thread(a).is_some_and(|t| t.loaded && t.current));
    opened.then(|| app.harness.state().remote.as_ref().map(|r| r.thread.clone())).flatten()
}

#[test]
fn t3_operate_flow() {
    if std::env::var("BUKNO_T3_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: set BUKNO_T3_LIVE=1 (see e2e/scenarios/t3-operate.md)");
        return;
    }
    let address = env("BUKNO_T3_ADDRESS");
    let links_file = PathBuf::from(env("BUKNO_T3_LINKS_FILE"));
    let links: Vec<String> = std::fs::read_to_string(&links_file).unwrap().lines().map(str::to_owned).collect();
    let _ = std::fs::remove_file(&links_file);
    let project_root = PathBuf::from(env("BUKNO_T3_PROJECT_ROOT"));
    let project_title = env("BUKNO_T3_PROJECT_TITLE");
    let codex = std::env::var("BUKNO_T3_CODEX_MODEL").unwrap_or_else(|_| "Codex · GPT 6.1 Sol".into());
    let claude = std::env::var("BUKNO_T3_CLAUDE_MODEL").unwrap_or_else(|_| "Claude · Claude Sonnet 5.5".into());
    let dirty_file = project_root.join("notes.md");
    let dirty_before = std::fs::read(&dirty_file).expect("the disposable project has a dirty notes.md");

    let evidence = PathBuf::from(env("BUKNO_EVIDENCE_DIR"));
    let root = std::env::temp_dir().join(format!("bukno-t3-operate-{}", std::process::id()));
    std::fs::create_dir_all(evidence.join("screens")).unwrap();
    let mut run = Run {
        scenario: "t3-operate",
        state: root.join("state"),
        work: root.join("work"),
        evidence,
        checks: Vec::new(),
        log: Vec::new(),
        shots: 0,
        started: Instant::now(),
        video: Recorder::from_env(),
    };
    std::fs::create_dir_all(&run.work).unwrap();
    let test_started = bukno_t3_client::time::now_utc();

    // ----- Pair ------------------------------------------------------------------
    run.caption("Bukno, the native app, pairs with the T3 server on this computer");
    let mut app = launch(&run);
    app.wait(Duration::from_secs(20), |a| a.setup.is_some());
    app.app().show_setup = false;
    app.pump(600);
    app.click("Add a T3 server");
    app.type_into("env-address", &address);
    app.type_into("env-link", &links[0]);
    app.pump(300);
    app.click("Connect");
    let paired = app.wait(Duration::from_secs(30), |a| {
        a.t3.as_ref().is_some_and(|t| matches!(t.view.pairing, PairingStatus::Paired { .. }))
    }) && app.wait(Duration::from_secs(30), |a| {
        connected(a) && a.t3.as_ref().is_some_and(|t| !t.view.environments[0].providers.is_empty())
    });
    let environment = app.environment().expect("environment");
    let server = environment.saved.label.clone();
    let env_id = environment.saved.environment_id.clone();
    run.check(
        "S1 paired with the read and operate scopes",
        paired && environment.can_operate(),
        json!({"scope": environment.saved.scope}),
    );
    app.pump(800);
    app.shot(&mut run, "paired");
    app.click("Done");
    app.pump(400);

    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let truth = Truth { http: Http::new(), base: normalize_address(&address).unwrap(), env_id: env_id.clone(), rt };
    assert_eq!(truth.rt.block_on(truth.http.descriptor(&truth.base)).unwrap().environment_id, env_id);
    let shell = truth.get("/api/orchestration/shell");
    let project_id = shell["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["title"] == project_title.as_str())
        .and_then(|p| p["id"].as_str())
        .expect("the disposable project is registered in T3")
        .to_owned();
    run.note("project", json!({"id": project_id, "root": project_root}));

    // ----- Codex: new chat and an approval, allowed ------------------------------
    run.caption("A new Codex chat, started from Bukno. T3 asks before commands");
    let first = "Run the shell command `ls` in this folder, then tell me how many entries it printed. Keep it short.";
    let codex_thread = new_chat(&mut app, &project_title, &server, &codex, first).unwrap_or_default();
    run.check("S2 a new Codex chat opens in Bukno", !codex_thread.is_empty(), json!({"thread": codex_thread}));
    let pending = wait_pending(&mut app, Duration::from_secs(120));
    run.caption("Codex wants to run a command: the approval card docks above the composer");
    app.pump(1200);
    app.shot(&mut run, "codex-approval");
    // Enter in the empty composer must not answer the card.
    app.harness.ctx.memory_mut(|m| m.request_focus(egui::Id::new(COMPOSER)));
    app.harness.key_press(Key::Enter);
    app.pump(600);
    let still = thread(app.harness.state()).is_some_and(|t| !t.pending.is_empty());
    run.check(
        "S3 an approval appears, and Enter in the composer does not answer it",
        pending.as_ref().is_some_and(|p| matches!(p.kind, PendingKind::Approval { .. })) && still,
        json!({"pending": pending.as_ref().map(|p| format!("{:?}", p.kind)), "still_pending_after_enter": still}),
    );
    run.caption("Allow once");
    app.click("Allow once");
    let answered = app.wait(Duration::from_secs(120), idle);
    app.pump(800);
    app.shot(&mut run, "codex-allowed");
    let p = projection(&truth, &codex_thread);
    let records = request_records(&p);
    run.check(
        "S3 T3 records the approval as accepted, and the turn finishes",
        answered && records.iter().any(|r| r["decision"] == "accept"),
        json!({"requests": records}),
    );
    run.check(
        "S2 the first message reached T3 exactly once",
        copies(&p, first) == 1,
        json!({"copies": copies(&p, first), "runs": p["runs"].as_array().map(Vec::len)}),
    );

    // ----- Codex: deny ----------------------------------------------------------
    run.caption("A follow-up; this time the command is denied");
    let second = "Now run `git status --short` and report what it shows.";
    send(&mut app, second, false);
    let pending = wait_pending(&mut app, Duration::from_secs(120));
    app.pump(800);
    app.click("Deny");
    let finished = app.wait(Duration::from_secs(120), idle);
    app.pump(800);
    app.shot(&mut run, "codex-denied");
    let p = projection(&truth, &codex_thread);
    let records = request_records(&p);
    run.check(
        "S3 a denied command is recorded as declined",
        pending.is_some() && finished && records.iter().any(|r| r["decision"] == "decline"),
        json!({"requests": records}),
    );

    // ----- Codex: queue, steer, Steer now, Stop and Resume ------------------------
    run.caption("A longer turn: Codex runs `sleep 40` after approval");
    let long = "Run `sleep 40` in the shell, then reply with the single word finished.";
    send(&mut app, long, false);
    if wait_pending(&mut app, Duration::from_secs(120)).is_some() {
        app.pump(500);
        app.click("Allow once");
    }
    let running = app.wait(Duration::from_secs(60), |a| thread(a).is_some_and(|t| t.active_run.is_some()));
    app.pump(1500);

    run.caption("While it works: Enter queues a message for the next turn");
    let queued_text = "After that, also tell me today's weekday.";
    send(&mut app, queued_text, false);
    let queued = app
        .wait(Duration::from_secs(30), |a| thread(a).is_some_and(|t| t.queued.iter().any(|q| q.text == queued_text)));
    app.pump(1200);
    app.shot(&mut run, "queued");

    run.caption("Steer now moves the queued message into the running turn");
    app.click("Steer now");
    let promoted = app.wait(Duration::from_secs(30), |a| {
        user_row(a, queued_text).is_some_and(|(intent, _)| intent == "promoted_queued_to_steer")
    });
    app.pump(1200);

    run.caption("Alt+Enter steers a new message straight into the turn");
    let steer_text = "Also mention the word banana in your reply.";
    send(&mut app, steer_text, true);
    let steered =
        app.wait(Duration::from_secs(30), |a| user_row(a, steer_text).is_some_and(|(intent, _)| intent == "steer"));
    app.pump(1500);
    app.shot(&mut run, "steered");
    let steer_caption = app
        .harness
        .state()
        .doc
        .messages
        .iter()
        .any(|m| m.source == steer_text && m.meta.as_deref() == Some("Added to the running turn"));
    let p = projection(&truth, &codex_thread);
    run.check(
        "S5 queue, Steer now and Steer reach T3 as queued, promoted and steered",
        running && queued && promoted && steered && steer_caption,
        json!({"running": running, "queued_with_text": queued, "promoted": promoted, "steered": steered, "steer_caption": steer_caption,
               "intents": p["turnItems"].as_array().map(|i| i.iter().filter(|i| i["type"] == "user_message").map(|i| i["inputIntent"].clone()).collect::<Vec<_>>())}),
    );

    run.caption("Queue one more, then Stop: the turn stops and the queue waits");
    let held_text = "Then list the files again.";
    send(&mut app, held_text, false);
    app.wait(Duration::from_secs(30), |a| thread(a).is_some_and(|t| t.queued.iter().any(|q| q.text == held_text)));
    app.pump(800);
    let active = thread(app.harness.state()).and_then(|t| t.active_run.clone());
    app.click("Stop");
    let stopped = app.wait(Duration::from_secs(60), |a| {
        thread(a).is_some_and(|t| t.active_run.is_none() && t.queued.iter().any(|q| q.held && q.text == held_text))
    });
    app.pump(1500);
    app.shot(&mut run, "stopped-queue-held");
    let stopped_status =
        active.as_ref().and_then(|r| thread(app.harness.state()).and_then(|t| t.run_status.get(r).cloned()));
    run.check(
        "S6 Stop interrupts the turn and holds the queued message",
        stopped && stopped_status.as_deref() == Some("interrupted"),
        json!({"run_status": stopped_status, "held": stopped}),
    );
    run.caption("Resume sends the waiting message");
    app.click("Resume");
    let resumed = app.wait(Duration::from_secs(60), |a| {
        thread(a).is_some_and(|t| t.queued.is_empty())
            && user_row(a, held_text)
                .and_then(|(_, run)| run)
                .is_some_and(|r| thread(a).and_then(|t| t.run_status.get(&r).cloned()).is_some_and(|s| s != "queued"))
    });
    // Its turn may ask to run a command; answer so it can finish.
    let mut approvals = 0;
    while !app.wait(Duration::from_secs(5), idle) && approvals < 6 {
        if thread(app.harness.state()).is_some_and(|t| !t.pending.is_empty()) {
            app.click("Allow once");
            approvals += 1;
        }
    }
    app.wait(Duration::from_secs(120), idle);
    app.pump(800);
    app.shot(&mut run, "resumed");
    run.check("S6 Resume releases the held message", resumed, json!({"resumed": resumed}));

    // ----- Claude: new chat, approval and a question -----------------------------
    run.caption("A new Claude chat in the same project");
    let claude_first = "Use the shell to run `touch claude-was-here.txt`, then reply with the word done.";
    let claude_thread = new_chat(&mut app, &project_title, &server, &claude, claude_first).unwrap_or_default();
    let pending = wait_pending(&mut app, Duration::from_secs(120));
    app.pump(1000);
    app.shot(&mut run, "claude-approval");
    run.caption("Claude asks before running a command that writes a file; Allow once");
    app.click("Allow once");
    let claude_done = app.wait(Duration::from_secs(120), idle);
    app.pump(800);
    let p = projection(&truth, &claude_thread);
    run.check(
        "S2/S3 a new Claude chat runs, with an approval accepted in Bukno",
        !claude_thread.is_empty()
            && pending.is_some()
            && claude_done
            && records_accept(&p)
            && copies(&p, claude_first) == 1,
        json!({"thread": claude_thread, "requests": request_records(&p)}),
    );

    run.caption("Claude asks a question with choices; the answer goes back through T3");
    let question = "Use your AskUserQuestion tool to ask me whether I prefer tea or coffee, with exactly the options Tea and Coffee. Then reply with one sentence about my choice.";
    send(&mut app, question, false);
    let asked = wait_pending(&mut app, Duration::from_secs(120));
    let mut answered = false;
    if let Some(PendingKind::Question { questions, .. }) = asked.as_ref().map(|p| p.kind.clone()) {
        app.pump(1000);
        app.shot(&mut run, "claude-question");
        let option =
            questions.first().and_then(|q| q.options.iter().find(|o| o.label.to_lowercase().contains("tea")).cloned());
        if let Some(option) = option {
            app.click(&option.label);
            app.pump(300);
            app.click("Send answer");
            answered = app.wait(Duration::from_secs(120), idle);
            app.pump(800);
            let p = projection(&truth, &claude_thread);
            let records = request_records(&p);
            let sent = records.iter().any(|r| {
                r["kind"] == "user_input"
                    && r["status"] == "resolved"
                    && r["answers"].to_string().contains(option.answer())
            });
            run.check(
                "S4 a question is answered with the option's value",
                answered && sent,
                json!({"requests": records, "option": option.answer()}),
            );
        }
    }
    if !answered {
        // Approvals can come first; record honestly what happened.
        run.note("question not observed", json!({"pending": asked.map(|p| format!("{:?}", p.kind))}));
        while thread(app.harness.state()).is_some_and(|t| !t.pending.is_empty()) {
            app.click("Allow once");
            app.pump(500);
        }
        app.wait(Duration::from_secs(120), idle);
    }
    app.shot(&mut run, "claude-answered");

    // ----- Draft, restart in the middle of a run, Stop after restart -------------
    run.caption("A draft typed in the Claude chat, not sent");
    let draft = "This draft must survive a restart.";
    app.type_visibly(COMPOSER, draft);
    app.pump(800);
    let switched =
        app.click_chat_in(&project_title, &format!("{}, Codex chat on {server}", app_title(&app, &codex_thread)));
    app.pump(600);
    let on_codex = app.harness.state().remote.as_ref().is_some_and(|r| r.thread == codex_thread);
    run.note("switched to the Codex chat", json!({"clicked": switched, "on_codex_chat": on_codex}));
    run.caption("Back in the Codex chat: start a long turn, then quit Bukno while it runs");
    send(&mut app, "Run `sleep 60` in the shell, then reply done.", false);
    if wait_pending(&mut app, Duration::from_secs(120)).is_some() {
        app.click("Allow once");
    }
    let working_before = app.wait(Duration::from_secs(60), |a| thread(a).is_some_and(|t| t.active_run.is_some()));
    app.pump(1500);
    drop(app);

    // Relaunch through a relay, so connections can be cut later.
    let relay = Relay::start(
        &truth.rt,
        truth.base.host_str().map(|h| format!("{h}:{}", truth.base.port().unwrap_or(80))).unwrap(),
    );
    let saved_path = run.state.join("t3-environments.json");
    let saved = std::fs::read_to_string(&saved_path).unwrap();
    let relay_address = format!("http://127.0.0.1:{}/", relay.port);
    std::fs::write(&saved_path, saved.replace(&truth.base.to_string(), &relay_address)).unwrap();
    run.caption("Bukno restarted: it reopens the chat, still working in T3");
    let mut app = launch(&run);
    let reopened = app.wait(Duration::from_secs(60), |a| {
        a.view == View::Remote
            && a.remote.as_ref().is_some_and(|r| r.thread == codex_thread)
            && thread(a).is_some_and(|t| t.current && t.active_run.is_some())
    });
    app.pump(1500);
    app.shot(&mut run, "reopened-working");
    run.caption("Stop works after the restart");
    app.click("Stop");
    let stopped_after = app.wait(Duration::from_secs(60), |a| thread(a).is_some_and(|t| t.active_run.is_none()));
    app.pump(800);
    run.check(
        "S7 after a restart the running chat reopens and Stop works",
        working_before && reopened && stopped_after,
        json!({"working_before": working_before, "reopened": reopened, "stopped": stopped_after}),
    );
    let opened_claude =
        app.click_chat_in(&project_title, &format!("{}, Claude chat on {server}", app_title(&app, &claude_thread)));
    app.pump(800);
    let kept = app.harness.state().composer.text == draft;
    app.shot(&mut run, "draft-restored");
    run.check("S7 the unsent draft survives the restart", kept, json!({"clicked": opened_claude, "on_claude_chat": app.harness.state().remote.as_ref().is_some_and(|r| r.thread == claude_thread), "composer": app.harness.state().composer.text}));
    app.type_into(COMPOSER, "");
    app.click_chat_in(&project_title, &format!("{}, Codex chat on {server}", app_title(&app, &codex_thread)));
    app.wait(Duration::from_secs(30), |a| thread(a).is_some_and(|t| t.current) && idle(a));
    app.pump(600);

    // ----- Lost replies ----------------------------------------------------------
    run.caption("A reply lost after T3 ran the message: Bukno finds it in the chat");
    let lost_reply = "Reply with the word one.";
    relay.set(DROP_REPLIES);
    send(&mut app, lost_reply, false);
    app.pump(2500);
    relay.drop_all();
    relay.set(FORWARD);
    let settled = app.wait(Duration::from_secs(60), |a| {
        connected(a)
            && a.t3.as_ref().is_some_and(|t| t.pending_sends().is_empty())
            && a.composer.text.is_empty()
            && thread(a).is_some_and(|t| t.rows.iter().any(|r| matches!(&r.item.kind, bukno_t3_client::model::TurnKind::UserMessage { text, .. } if text == lost_reply)))
    });
    app.wait(Duration::from_secs(120), idle);
    app.pump(800);
    let p = projection(&truth, &codex_thread);
    let observed = app
        .environment()
        .map(|e| e.outbox.iter().any(|o| matches!(o.state, OutboxState::Accepted { observed: true, .. })))
        .unwrap_or(false);
    run.check(
        "S8 a message whose reply was lost is shown as sent, once",
        settled && copies(&p, lost_reply) == 1,
        json!({"settled": settled, "settled_from_the_chat": observed, "copies_in_t3": copies(&p, lost_reply)}),
    );

    run.caption("A request lost on the way: Bukno says it is not confirmed, and Send again is safe");
    let lost_request = "Reply with the word two.";
    relay.set(BLACKHOLE);
    send(&mut app, lost_request, false);
    app.pump(1500);
    relay.drop_all();
    relay.set(FORWARD);
    let unconfirmed = app.wait(Duration::from_secs(60), |a| {
        connected(a)
            && a.environment_has(|e| e.outbox.iter().any(|o| o.state == OutboxState::Unconfirmed))
            && a.composer.text == lost_request
    });
    app.pump(1500);
    app.shot(&mut run, "not-confirmed");
    app.click("Send again");
    let sent_again = app.wait(Duration::from_secs(60), |a| {
        a.t3.as_ref().is_some_and(|t| t.pending_sends().is_empty()) && a.composer.text.is_empty()
    });
    app.wait(Duration::from_secs(120), idle);
    app.pump(800);
    let p = projection(&truth, &codex_thread);
    run.check(
        "S8 a request that never arrived is offered again and then sent once",
        unconfirmed && sent_again && copies(&p, lost_request) == 1,
        json!({"unconfirmed": unconfirmed, "sent_again": sent_again, "copies_in_t3": copies(&p, lost_request)}),
    );

    run.caption("While T3 cannot be reached, Send is unavailable and the draft is kept");
    relay.set(REFUSE);
    relay.drop_all();
    app.wait(Duration::from_secs(30), |a| !connected(a));
    let offline = "This must not be sent while offline.";
    send(&mut app, offline, false);
    app.pump(1500);
    app.shot(&mut run, "offline-draft-kept");
    let kept_offline = app.harness.state().composer.text == offline
        && app.harness.state().remote_send_blocked().is_some_and(|b| b.starts_with("Not connected"));
    relay.set(FORWARD);
    app.wait(Duration::from_secs(60), connected);
    app.pump(800);
    let p = projection(&truth, &codex_thread);
    run.check(
        "S9 nothing is sent while disconnected, and the draft stays",
        kept_offline && copies(&p, offline) == 0,
        json!({"draft_kept": kept_offline, "copies_in_t3": copies(&p, offline)}),
    );
    app.type_into(COMPOSER, "");
    app.pump(300);

    // ----- Quit while a reply is lost; the relaunch settles it -------------------
    run.caption("A reply lost, then Bukno quits: after the relaunch it is found in the chat, not sent again");
    let across_restart = "Reply with the word three.";
    relay.set(DROP_REPLIES);
    send(&mut app, across_restart, false);
    app.pump(2500);
    let saved_pending = app.harness.state().t3.as_ref().is_some_and(|t| !t.pending_sends().is_empty());
    drop(app);
    relay.drop_all();
    relay.set(FORWARD);
    let mut app = launch(&run);
    let settled_after_restart = app.wait(Duration::from_secs(90), |a| {
        connected(a)
            && a.remote.as_ref().is_some_and(|r| r.thread == codex_thread)
            && a.t3.as_ref().is_some_and(|t| t.pending_sends().is_empty())
            && a.composer.text.is_empty()
            && thread(a).is_some_and(|t| t.current)
    });
    app.wait(Duration::from_secs(120), idle);
    app.pump(1200);
    app.shot(&mut run, "settled-after-restart");
    let p = projection(&truth, &codex_thread);
    run.check(
        "S8 a send whose reply was lost before quitting is settled after the relaunch, once, and its draft cleared",
        saved_pending && settled_after_restart && copies(&p, across_restart) == 1,
        json!({"saved_before_quit": saved_pending, "settled_and_cleared": settled_after_restart, "copies_in_t3": copies(&p, across_restart)}),
    );

    // ----- No duplicates, the dirty file, and leaks -------------------------------
    run.caption("Checks: every message once in T3, the dirty file untouched, no secrets on disk");
    for (thread_id, texts) in [
        (
            &codex_thread,
            vec![first, second, long, queued_text, steer_text, held_text, lost_reply, lost_request, across_restart],
        ),
        (&claude_thread, vec![claude_first, question]),
    ] {
        let p = projection(&truth, thread_id);
        let counts: Vec<(String, usize)> = texts.iter().map(|t| ((*t).to_owned(), copies(&p, t))).collect();
        run.check(
            &format!("S2 every message sent from Bukno is in T3 exactly once ({})", &thread_id[..8]),
            counts.iter().all(|(_, n)| *n == 1),
            json!({"copies": counts}),
        );
    }
    let dirty_after = std::fs::read(&dirty_file).unwrap_or_default();
    run.check(
        "S10 the uncommitted change in notes.md is untouched",
        dirty_after == dirty_before,
        json!({"bytes": dirty_after.len()}),
    );
    run.note("T3 command receipts during the run", command_receipts(&test_started));

    let token = SystemKeychain.load(&env_id).ok().flatten();
    run.caption("Done: Codex and Claude, worked from Bukno through T3");
    app.pump(2500);
    drop(app);
    std::fs::copy(run.state.join("t3-client.log"), run.evidence.join("t3-client.log")).ok();
    std::fs::copy(run.state.join("t3-client-state.json"), run.evidence.join("t3-client-state.json")).ok();
    let log = std::fs::read_to_string(run.state.join("t3-client.log")).unwrap_or_default();
    let texts_in_log: Vec<&str> = [first, second, long, queued_text, steer_text, claude_first, lost_reply]
        .into_iter()
        .filter(|t| log.contains(t))
        .collect();
    let mut secrets: Vec<String> = token.iter().map(|t| t.expose().to_owned()).collect();
    for link in &links {
        secrets.push(link.clone());
        let parsed = bukno_t3_client::pairing::parse_pairing(None, link).expect("pairing link");
        secrets.push(parsed.credential.expose().to_owned());
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
        "S10 no token, pairing link or chat text in the log; no secret in state or evidence",
        texts_in_log.is_empty() && leaks.is_empty(),
        json!({"chat_text_in_log": texts_in_log, "files_with_secrets": leaks}),
    );
    run.write();
    if let Some(video) = &run.video {
        video.lock().unwrap().finish();
    }
    let failed: Vec<_> = run.checks.iter().filter(|c| c["status"] == "fail").map(|c| c["id"].clone()).collect();
    assert!(failed.is_empty(), "failed checks: {failed:?}");
}

fn records_accept(p: &Value) -> bool {
    request_records(p).iter().any(|r| r["decision"] == "accept")
}

/// A chat's title as the sidebar shows it, for its accessible label.
fn app_title(app: &App, thread: &str) -> String {
    app.harness
        .state()
        .t3
        .as_ref()
        .and_then(|t| t.view.environments.first().cloned())
        .and_then(|e| e.threads.iter().find(|t| t.id == thread).map(|t| t.title.clone()))
        .unwrap_or_default()
}

trait EnvironmentHas {
    fn environment_has(&self, f: impl Fn(&bukno_t3_client::EnvironmentView) -> bool) -> bool;
}

impl EnvironmentHas for BuknoApp {
    fn environment_has(&self, f: impl Fn(&bukno_t3_client::EnvironmentView) -> bool) -> bool {
        self.t3.as_ref().and_then(|t| t.view.environments.first().cloned()).is_some_and(|e| f(&e))
    }
}
