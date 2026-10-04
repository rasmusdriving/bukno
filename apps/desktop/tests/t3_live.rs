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

mod support;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use bukno_desktop::app::{BuknoApp, View};
use bukno_desktop::sources::T3ChatRef;
use bukno_t3_client::http::Http;
use bukno_t3_client::pairing::normalize_address;
use bukno_t3_client::rpc::Method;
use bukno_t3_client::secret::{SystemKeychain, TokenVault};
use bukno_t3_client::{ConnectionStatus, PairingStatus};
use serde_json::json;
use support::*;

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
        scenario: "t3-read-only",
        state: root.join("state"),
        work: root.join("work"),
        evidence,
        checks: Vec::new(),
        log: Vec::new(),
        shots: 0,
        started: Instant::now(),
        video: None,
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
        "T1 token is in the system keychain, with the read and operate scopes",
        token.is_some() && environment.saved.can_operate(),
        json!({"keychain_entry": token.is_some(), "scope": environment.saved.scope}),
    );
    let session = bukno_sessions(&label).into_iter().find(|s| !sessions_before.contains(&s.0));
    run.check(
        "T6 the server holds Bukno's session with the read and operate scopes",
        session.as_ref().is_some_and(|(_, scopes)| {
            scopes.contains("orchestration:read") && scopes.contains("orchestration:operate")
        }),
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
    let row_label = format!("{title}, {provider} chat on {}{delegated}", env_view.saved.label);
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
    // Since Stage 2 the sign-in may operate; this run must still only read.
    let allowed: BTreeSet<String> = Method::ALL.iter().filter(|m| !m.operates()).map(|m| m.tag().to_owned()).collect();
    run.check(
        "T6 Bukno sent only read methods during this read-only run",
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
