//! Composition root: resolve paths, pick the mode, open the window.
//!
//! A normal launch uses the app state folder on the internal drive and the
//! installed engines. `--scenario <name>` starts the labeled synthetic
//! scenario mode instead, which never contacts a real engine and keeps its
//! state in isolated temporary folders.

use bukno_desktop::BuknoApp;
use bukno_platform::paths::{self, AppPaths};
use bukno_platform::process::{self, InstanceLock, LockError, Signal};
use bukno_runtime::synthetic::{SCENARIO_NAMES, Scenario};

enum Start {
    Synthetic(Scenario, AppPaths),
    Real(AppPaths, bukno_storage::Store, InstanceLock),
    Blocked(AppPaths, String, String),
}

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let mut scenario_name = std::env::var("BUKNO_SCENARIO").ok();
    let mut size = [1440.0_f32, 900.0];
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => scenario_name = args.next(),
            "--size" => {
                if let Some((w, h)) = args.next().and_then(|v| v.split_once('x').map(|(w, h)| (w.parse(), h.parse())))
                    && let (Ok(w), Ok(h)) = (w, h)
                {
                    size = [w, h];
                }
            }
            // Finder adds a process serial number argument on older systems.
            other if other.starts_with("-psn_") => {}
            other => {
                eprintln!(
                    "Unknown argument {other}. Use --scenario <{}> and --size <W>x<H>.",
                    SCENARIO_NAMES.join("|")
                );
                std::process::exit(2);
            }
        }
    }
    let start = match scenario_name {
        Some(name) => {
            let Some(scenario) = Scenario::named(&name) else {
                eprintln!("Unknown scenario {name}. Choose one of: {}", SCENARIO_NAMES.join(", "));
                std::process::exit(2);
            };
            let paths = synthetic_paths();
            eprintln!(
                "Bukno synthetic scenario {}: state {} work {}",
                scenario.name,
                paths.state_dir.display(),
                paths.work_dir.as_ref().map_or("-".into(), |p| p.display().to_string())
            );
            Start::Synthetic(scenario, paths)
        }
        None => real_start(),
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Bukno")
            .with_app_id("io.github.rasmusdriving.bukno")
            .with_inner_size(size)
            .with_min_inner_size([640.0, 480.0])
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        // Repaint only on input or an explicit request; never in a loop.
        run_and_return: false,
        ..Default::default()
    };
    eframe::run_native(
        "Bukno",
        options,
        Box::new(move |cc| {
            Ok(Box::new(match start {
                Start::Synthetic(scenario, paths) => BuknoApp::synthetic(&cc.egui_ctx, scenario, paths),
                Start::Real(paths, store, lock) => {
                    // Held for the whole process; the OS releases it on exit.
                    std::mem::forget(lock);
                    BuknoApp::real(&cc.egui_ctx, paths, store)
                }
                Start::Blocked(paths, title, detail) => BuknoApp::blocked(&cc.egui_ctx, paths, title, detail),
            }))
        }),
    )
}

/// Open the app state for a normal launch, or explain why Bukno cannot.
fn real_start() -> Start {
    let paths = match paths::resolve() {
        Ok(paths) => paths,
        Err(e) => {
            let fallback = AppPaths { state_dir: std::env::temp_dir(), work_dir: None, overridden: false };
            return Start::Blocked(fallback, "Bukno cannot find its folder".into(), e.to_string());
        }
    };
    let lock = match InstanceLock::acquire(&paths.state_dir) {
        Ok(lock) => lock,
        Err(LockError::Held(pid)) => {
            let who =
                pid.map_or("Another copy of Bukno".to_owned(), |p| format!("Another copy of Bukno (process {p})"));
            return Start::Blocked(
                paths,
                "Bukno is already open".into(),
                format!("{who} is using this state folder. Switch to it, or quit it and open Bukno again."),
            );
        }
        Err(e) => return Start::Blocked(paths, "Bukno cannot start".into(), e.to_string()),
    };
    install_panic_cleanup(&paths);
    match bukno_storage::Store::open(&paths.state_dir) {
        Ok(store) => Start::Real(paths, store, lock),
        Err(e) => {
            let detail = format!(
                "{e}. Bukno has not changed anything. Your data is in {}.",
                paths.state_dir.join("state.sqlite").display()
            );
            Start::Blocked(paths, "Bukno cannot open its saved chats".into(), detail)
        }
    }
}

/// If Bukno itself panics, end the engine processes it started (section 16).
fn install_panic_cleanup(paths: &AppPaths) {
    let records = paths.state_dir.join("engine-processes.tsv");
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        for record in process::load_records(&records) {
            if process::is_alive(&record) {
                let _ = process::signal_group(record.pid, Signal::Kill);
            }
        }
        default(info);
    }));
}

/// Synthetic runs always use isolated folders: the override variables when
/// set, otherwise a fresh temporary folder for this process.
fn synthetic_paths() -> AppPaths {
    let base = std::env::temp_dir().join(format!("bukno-synthetic-{}", std::process::id()));
    let state_dir = std::env::var_os(paths::STATE_DIR_ENV).map_or_else(|| base.join("state"), Into::into);
    let work_dir = std::env::var_os(paths::WORK_DIR_ENV).map_or_else(|| base.join("work"), Into::into);
    let _ = std::fs::create_dir_all(&state_dir);
    let _ = std::fs::create_dir_all(&work_dir);
    AppPaths { state_dir, work_dir: Some(work_dir), overridden: true }
}
