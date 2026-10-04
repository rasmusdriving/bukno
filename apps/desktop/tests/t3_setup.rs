//! Native onboarding against real T3 discovery, download and startup.
//! Failure paths were recorded in e2e/scenarios/t3-setup-failure-paths.md
//! before implementation. Run with scripts/t3-setup-e2e.sh.

mod support;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bukno_desktop::app::View;
use bukno_t3_client::local::SetupStatus;
use bukno_t3_client::secret::{SystemKeychain, TokenVault};
use serde_json::json;
use support::*;

#[test]
fn automatic_t3_onboarding() {
    if std::env::var("BUKNO_T3_SETUP_LIVE").as_deref() != Ok("1") {
        eprintln!("skipped: run scripts/t3-setup-e2e.sh");
        return;
    }
    let mode = env("BUKNO_T3_SETUP_MODE");
    let root = PathBuf::from(env("BUKNO_T3_SETUP_ROOT"));
    let mut run = Run {
        scenario: "t3-automatic-setup",
        state: root.join("state"),
        work: root.join("chats"),
        evidence: PathBuf::from(env("BUKNO_EVIDENCE_DIR")),
        checks: Vec::new(),
        log: Vec::new(),
        shots: 0,
        started: Instant::now(),
        video: None,
    };
    std::fs::create_dir_all(&run.work).unwrap();
    std::fs::create_dir_all(run.evidence.join("screens")).unwrap();
    // Keep the user's existing keychain entry unchanged after attached checks.
    let original_id = if mode == "attached" {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let state: serde_json::Value = serde_json::from_slice(
            &std::fs::read(PathBuf::from(env("BUKNO_T3_HOME")).join("userdata/server-runtime.json")).unwrap(),
        )
        .unwrap();
        let base = bukno_t3_client::pairing::normalize_address(state["origin"].as_str().unwrap()).unwrap();
        Some(rt.block_on(bukno_t3_client::http::Http::new().descriptor(&base)).unwrap().environment_id)
    } else {
        None
    };
    let original_token = original_id.as_ref().and_then(|id| SystemKeychain.load(id).ok().flatten());
    if mode == "invalid-runtime" {
        let dir = PathBuf::from(env("BUKNO_T3_HOME")).join("userdata");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("server-runtime.json"),
            serde_json::to_vec(&json!({
                "version": 1, "pid": std::process::id(), "origin": "http://127.0.0.1:9",
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let mut app = launch(&run);
    app.app().show_setup = true;
    let initial = app.wait(Duration::from_secs(60), |a| a.t3.as_ref().is_some_and(|t| !t.view.local_setup.busy()));
    let status = app.app().t3.as_ref().unwrap().view.local_setup.clone();
    run.check("automatic discovery finishes without manual input", initial, json!(format!("{status:?}")));
    app.shot(&mut run, "onboarding");
    if mode == "invalid-runtime" {
        run.check(
            "a live invalid runtime blocks startup and sends no credential",
            matches!(status, SetupStatus::Failed(_))
                && app.app().t3.as_ref().unwrap().view.environments.is_empty()
                && !run.state.join("t3-runtime").exists(),
            json!(format!("{status:?}")),
        );
        run.check(
            "advanced connection is reachable from onboarding",
            app.click("Advanced connection") && app.app().show_environments,
            json!(true),
        );
        app.shot(&mut run, "advanced-fallback");
        drop(app);
        assert!(run.checks.iter().all(|c| c["status"] == "pass"));
        return;
    }
    if mode == "download" || mode == "offline" {
        run.check(
            "missing T3 offers one download button",
            status == SetupStatus::Missing && app.click("Download T3 Code"),
            json!(format!("{status:?}")),
        );
        app.shot(&mut run, "downloading");
    }
    if mode == "offline" {
        let failed = app.wait(Duration::from_secs(30), |a| {
            a.t3.as_ref().is_some_and(|t| matches!(t.view.local_setup, SetupStatus::Failed(_)))
        });
        let staging = run.state.join("t3-runtime");
        let clean = !staging.exists() || std::fs::read_dir(&staging).unwrap().next().is_none();
        run.check(
            "offline download reports an error and removes its partial installation",
            failed && clean && app.app().t3.as_ref().unwrap().view.environments.is_empty(),
            json!({"error":format!("{:?}",app.app().t3.as_ref().unwrap().view.local_setup),"staging_removed":clean}),
        );
        app.shot(&mut run, "offline-download");
        drop(app);
        assert!(run.checks.iter().all(|c| c["status"] == "pass"));
        return;
    }
    app.wait(Duration::from_secs(360), |a| {
        a.t3.as_ref()
            .is_some_and(|t| t.ready_environment().is_some() || matches!(t.view.local_setup, SetupStatus::Failed(_)))
    });
    let connected = app.app().t3.as_ref().and_then(|t| t.ready_environment()).is_some();
    let status = app.app().t3.as_ref().unwrap().view.local_setup.clone();
    run.check("T3 starts, pairs and provides the real project/model snapshot", connected, json!(format!("{status:?}")));
    app.shot(&mut run, "connected");
    let Some(env) = app.environment() else {
        drop(app);
        panic!("setup did not connect: {status:?}");
    };
    let id = env.saved.environment_id.clone();
    let paired_at = env.saved.paired_at;
    let token = SystemKeychain.load(&id).unwrap().unwrap();
    run.check(
        "sign-in stays in the system keychain",
        env.can_operate() && SystemKeychain.load(&id).unwrap().is_some(),
        json!({"scope":env.saved.scope}),
    );
    run.check(
        "credentials do not appear in Bukno state or evidence",
        files_containing(&run.state, token.expose().as_bytes()).is_empty()
            && files_containing(&run.evidence, token.expose().as_bytes()).is_empty(),
        json!(true),
    );
    if connected {
        let clicked = app.click("Continue");
        let composer = app.wait(Duration::from_secs(30), |a| a.view == View::RemoteNew && !a.show_setup);
        run.check(
            "Continue opens a T3 composer, registering a chat folder if needed",
            clicked && composer,
            json!({"clicked":clicked,"view":format!("{:?}",app.app().view)}),
        );
        app.shot(&mut run, "t3-composer");
    }
    let managed_record = run.state.join("t3-server/userdata/server-runtime.json");
    let pid_before = std::fs::read(&managed_record)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["pid"].as_u64());
    drop(app);
    let mut app = launch(&run);
    let resumed = app.wait(Duration::from_secs(60), |a| a.t3.as_ref().and_then(|t| t.ready_environment()).is_some());
    let resumed_env = app.environment().unwrap();
    run.check("Bukno restart reuses its environment and sign-in", resumed && resumed_env.saved.environment_id == id && resumed_env.saved.paired_at == paired_at, json!({"same_environment":resumed_env.saved.environment_id == id,"same_sign_in":resumed_env.saved.paired_at == paired_at}));
    if mode == "download" {
        let record: serde_json::Value = serde_json::from_slice(&std::fs::read(&managed_record).unwrap()).unwrap();
        run.check(
            "Bukno restart reuses the existing server process",
            record["pid"].as_u64() == pid_before,
            json!({"pid":pid_before,"same_pid":record["pid"].as_u64() == pid_before}),
        );
        let pid = record["pid"].as_u64().unwrap();
        drop(app);
        // This is the disposable managed server, never the user's attached T3.
        assert!(managed_record.starts_with(&root));
        std::process::Command::new("kill").args(["-TERM", &pid.to_string()]).status().unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        let mut restarted = launch(&run);
        let ready =
            restarted.wait(Duration::from_secs(90), |a| a.t3.as_ref().and_then(|t| t.ready_environment()).is_some());
        let record: serde_json::Value = serde_json::from_slice(&std::fs::read(&managed_record).unwrap()).unwrap();
        run.check("stopped managed T3 restarts automatically with the same chats and sign-in", ready && record["pid"].as_u64() != Some(pid) && restarted.environment().is_some_and(|e| e.saved.environment_id == id && e.saved.paired_at == paired_at && !e.projects.is_empty()), json!({"new_pid":record["pid"],"same_environment":restarted.environment().is_some_and(|e| e.saved.environment_id == id)}));
        restarted.app().show_setup = true;
        restarted.shot(&mut run, "managed-server-restarted");
        drop(restarted);
        std::process::Command::new("kill")
            .args(["-TERM", &record["pid"].as_u64().unwrap().to_string()])
            .status()
            .unwrap();
        let _ = SystemKeychain.delete(&id);
    } else {
        drop(app);
        if let Some(token) = original_token {
            SystemKeychain.save(&id, &token).unwrap();
        } else {
            SystemKeychain.delete(&id).unwrap();
        }
    }
    run.write();
    assert!(run.checks.iter().all(|c| c["status"] == "pass"), "see result.json");
}
