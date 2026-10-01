//! Composition root: resolve paths, pick the mode, open the window.
//!
//! Pass 0 has one mode, the synthetic scenario mode. It never contacts a
//! real engine and keeps its state in isolated temporary folders.

use bukno_desktop::BuknoApp;
use bukno_platform::paths::{self, AppPaths};
use bukno_runtime::synthetic::{SCENARIO_NAMES, Scenario};

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let mut scenario_name = std::env::var("BUKNO_SCENARIO").unwrap_or_else(|_| "short".into());
    let mut size = [1440.0_f32, 900.0];
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => scenario_name = args.next().unwrap_or_default(),
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
    let Some(scenario) = Scenario::named(&scenario_name) else {
        eprintln!("Unknown scenario {scenario_name}. Choose one of: {}", SCENARIO_NAMES.join(", "));
        std::process::exit(2);
    };
    let paths = synthetic_paths();
    eprintln!(
        "Bukno synthetic scenario {}: state {} work {}",
        scenario.name,
        paths.state_dir.display(),
        paths.work_dir.as_ref().map_or("-".into(), |p| p.display().to_string())
    );

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Bukno")
            .with_app_id("se.brilliantfuture.bukno")
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
        Box::new(move |cc| Ok(Box::new(BuknoApp::synthetic(&cc.egui_ctx, scenario, paths)))),
    )
}

/// Synthetic runs always use isolated folders: the override variables when
/// set, otherwise a fresh temporary folder for this process.
fn synthetic_paths() -> AppPaths {
    let resolved = paths::resolve().ok();
    let base = std::env::temp_dir().join(format!("bukno-synthetic-{}", std::process::id()));
    let state_dir = std::env::var_os(paths::STATE_DIR_ENV).map_or_else(|| base.join("state"), Into::into);
    let work_dir = std::env::var_os(paths::WORK_DIR_ENV).map_or_else(|| base.join("work"), Into::into);
    let _ = std::fs::create_dir_all(&state_dir);
    let _ = std::fs::create_dir_all(&work_dir);
    let _ = resolved;
    AppPaths { state_dir, work_dir: Some(work_dir), overridden: true }
}
