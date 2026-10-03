//! Bukno developer commands. Not shipped with the app.
//!
//!   cargo xtask doctor                      report tools and locations
//!   cargo xtask dev [--scenario NAME]       run the app in synthetic scenario mode
//!   cargo xtask check                       format, lint, build, Windows compile check
//!   cargo xtask package --platform macos    build Bukno.app
//!   cargo xtask e2e --provider synthetic --scenario pass0-ui
//!   cargo xtask bench --workload scroll|idle|orb|startup
//!
//! Evidence goes to BUKNO_ARTIFACTS_DIR (set by scripts/bootstrap.sh) under
//! <date>/<build>/<platform>/<provider>/<scenario>/, as section 20 describes.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

type Result<T = ()> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("doctor") => doctor(),
        Some("dev") => dev(&args[1..]),
        Some("check") => check(),
        Some("package") => package(&args[1..]).map(|_| ()),
        Some("e2e") => e2e(&args[1..]),
        Some("bench") => bench(&args[1..]),
        _ => Err("usage: cargo xtask <doctor|dev|check|package|e2e|bench> [options]".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn cargo() -> Command {
    let mut command = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    command.current_dir(repo());
    command
}

fn run(command: &mut Command) -> Result {
    let status = command.status().map_err(|e| format!("could not start {command:?}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{command:?} failed with {status}")) }
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).current_dir(repo()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn target_dir() -> PathBuf {
    let metadata = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().ok();
    metadata
        .and_then(|m| serde_json::from_slice::<Value>(&m.stdout).ok())
        .and_then(|v| v["target_directory"].as_str().map(PathBuf::from))
        .unwrap_or_else(|| repo().join("target"))
}

fn artifacts_dir() -> PathBuf {
    std::env::var_os("BUKNO_ARTIFACTS_DIR").map_or_else(|| target_dir().join("bukno-evidence"), PathBuf::from)
}

fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

/// Short commit, with "-dirty" when the working tree has changes.
fn build_id() -> String {
    let commit = output("git", &["rev-parse", "--short=10", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = output("git", &["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    if dirty { format!("{commit}-dirty") } else { commit }
}

/// Today's UTC date as YYYY-MM-DD, without a date library.
fn today() -> String {
    let days = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64 / 86_400;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn evidence_dir(provider: &str, scenario: &str) -> PathBuf {
    artifacts_dir().join(today()).join(build_id()).join(platform()).join(provider).join(scenario)
}

fn write_json(path: &Path, value: &Value) -> Result {
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(path, serde_json::to_string_pretty(value).unwrap()).map_err(|e| format!("{}: {e}", path.display()))
}

fn manifest(dir: &Path, scenario: &str, provider: &str) -> Result {
    let dirty_files = output("git", &["status", "--porcelain"]).unwrap_or_default();
    write_json(
        &dir.join("manifest.json"),
        &json!({
            "scenario": scenario,
            "commit": output("git", &["rev-parse", "HEAD"]),
            "dirty_files": dirty_files.lines().collect::<Vec<_>>(),
            "os": output("sw_vers", &["-productVersion"])
                .or_else(|| output("cmd", &["/c", "ver"]))
                .or_else(|| output("uname", &["-sr"])),
            "os_build": output("sw_vers", &["-buildVersion"]),
            "hardware": output("sysctl", &["-n", "machdep.cpu.brand_string"])
                .or_else(|| output("uname", &["-m"])),
            "memory_bytes": output("sysctl", &["-n", "hw.memsize"]),
            "rustc": output("rustc", &["-V"]),
            "date_utc": today(),
            "engines": if provider == "synthetic" {
                "none: synthetic scenario mode makes no engine connections"
            } else {
                "installed Codex app-server; see engine.json for the version"
            },
        }),
    )
}

fn doctor() -> Result {
    let fonts = repo().join("assets/fonts");
    println!("Bukno doctor");
    println!("  repository:       {}", repo().display());
    println!("  rustc:            {}", output("rustc", &["-V"]).unwrap_or_else(|| "missing".into()));
    println!("  cargo target:     {}", target_dir().display());
    println!(
        "  local config:     {}",
        if repo().join(".cargo/config.local.toml").exists() { "present" } else { "missing, run scripts/bootstrap.sh" }
    );
    println!("  DEVELOPER_DIR:    {}", std::env::var("DEVELOPER_DIR").unwrap_or_else(|_| "(default Xcode)".into()));
    println!("  artifacts:        {}", artifacts_dir().display());
    println!(
        "  design tokens:    {}",
        if repo().join("docs/design/system/tokens.json").exists() { "found" } else { "missing" }
    );
    for font in ["Geist-Regular.ttf", "Geist-Medium.ttf", "Geist-SemiBold.ttf", "GeistMono-Regular.ttf", "OFL.txt"] {
        println!("  font {font:<22} {}", if fonts.join(font).exists() { "found" } else { "missing" });
    }
    println!("  engines:          not used in Pass 0");
    if target_dir().starts_with(repo()) {
        return Err("compiler output is inside the repository; run scripts/bootstrap.sh".into());
    }
    Ok(())
}

fn dev(args: &[String]) -> Result {
    let scenario = option(args, "--scenario").unwrap_or("short");
    run(cargo().args(["run", "-p", "bukno-desktop", "--release", "--", "--scenario", scenario]))
}

fn check() -> Result {
    run(cargo().args(["fmt", "--all", "--check"]))?;
    run(cargo().args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]))?;
    run(cargo().args(["build", "--workspace"]))?;
    // Windows: compiles the whole workspace for the target; linking and the
    // real UI smoke check need a Windows machine. The bundled SQLite is C and
    // needs the Windows SDK to compile, so this type-check uses SQLite's
    // bundled bindings without compiling it. The Windows CI job builds it for real.
    run(cargo()
        .args(["check", "--workspace", "--all-targets", "--target", "x86_64-pc-windows-msvc"])
        .env("LIBSQLITE3_SYS_USE_PKG_CONFIG", "1")
        .env("SQLITE3_LIB_DIR", target_dir()))?;
    println!("check: format, lint, {} build and Windows compile check passed", platform());
    Ok(())
}

/// Build Bukno.app in the target directory and return its path.
fn package(args: &[String]) -> Result<PathBuf> {
    match option(args, "--platform").unwrap_or(platform()) {
        "macos" => {}
        other => return Err(format!("packaging for {other} lands in its platform pass")),
    }
    run(cargo().args(["build", "--release", "-p", "bukno-desktop"]))?;
    let target = target_dir();
    let app = target.join("bukno-package/Bukno.app");
    let _ = std::fs::remove_dir_all(&app);
    let macos = app.join("Contents/MacOS");
    std::fs::create_dir_all(&macos).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(app.join("Contents/Resources")).map_err(|e| e.to_string())?;
    std::fs::copy(target.join("release/bukno"), macos.join("bukno")).map_err(|e| e.to_string())?;
    let version = env!("CARGO_PKG_VERSION");
    let plist = std::fs::read_to_string(repo().join("packaging/macos/Info.plist"))
        .map_err(|e| e.to_string())?
        .replace("{{VERSION}}", version);
    std::fs::write(app.join("Contents/Info.plist"), plist).map_err(|e| e.to_string())?;
    std::fs::copy(repo().join("assets/fonts/OFL.txt"), app.join("Contents/Resources/Geist-OFL.txt"))
        .map_err(|e| e.to_string())?;
    // Ad-hoc signature so the bundle launches from Finder on Apple Silicon.
    run(Command::new("codesign").args(["--force", "--sign", "-", "--timestamp=none"]).arg(&app))?;
    println!("package: {}", app.display());
    Ok(app)
}

fn e2e(args: &[String]) -> Result {
    let provider = option(args, "--provider").unwrap_or("synthetic");
    let scenario = option(args, "--scenario").unwrap_or("pass0-ui");
    if (provider, scenario) == ("codex", "pass1") {
        return e2e_codex_pass1();
    }
    if provider != "synthetic" || scenario != "pass0-ui" {
        return Err(
            "End-to-end scenarios: --provider synthetic --scenario pass0-ui, or --provider codex --scenario pass1"
                .into(),
        );
    }
    let dir = evidence_dir(provider, scenario);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    manifest(&dir, scenario, provider)?;
    let started = Instant::now();
    let status = cargo()
        .args(["test", "-p", "bukno-desktop", "--test", "ui_checks", "--", "--test-threads=1"])
        .env("BUKNO_EVIDENCE_DIR", dir.join("checks"))
        .status()
        .map_err(|e| e.to_string())?;
    let mut checks = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir.join("checks")) {
        let mut names: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        names.sort();
        for path in names {
            let result = std::fs::read_to_string(path.join("result.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok());
            checks.push(json!({
                "check": path.file_name().unwrap().to_string_lossy(),
                "status": result.as_ref().map_or(json!("no result written (failed before the end)"), |r| r["result"]["status"].clone()),
            }));
        }
    }
    write_json(
        &dir.join("result.json"),
        &json!({
            "scenario": scenario,
            "status": if status.success() { "pass" } else { "fail" },
            "mode": "egui_kittest driving the real app through its AccessKit tree; synthetic data, no engines",
            "seconds": started.elapsed().as_secs_f64(),
            "checks": checks,
            "not_covered_here": ["VoiceOver speech", "real IME input methods", "Finder launch", "native menus", "Windows UI"],
        }),
    )?;
    println!("e2e evidence: {}", dir.display());
    if status.success() { Ok(()) } else { Err("UI checks failed; see the evidence folder".into()) }
}

/// Launch the packaged app with an autopilot, sample its memory, collect metrics.
/// The Pass 1 Codex flow through the real app with the installed engine and
/// the existing Codex login. Spends a little real usage; never runs in CI.
fn e2e_codex_pass1() -> Result {
    let dir = evidence_dir("codex", "pass1");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    manifest(&dir, "pass1", "codex")?;
    let codex = output("codex", &["--version"]);
    write_json(&dir.join("engine.json"), &json!({ "codex_version_on_path": codex }))?;
    std::fs::write(
        dir.join("procedure.md"),
        "# Pass 1 Codex flow\n\n\
         Driven by `apps/desktop/tests/codex_live.rs` through the real app UI (egui_kittest), with the real\n\
         coordinator, SQLite store and installed Codex app-server, using the existing Codex login. Reasoning\n\
         effort is set to low through BUKNO_CODEX_EFFORT to keep usage small.\n\n\
         1. Create a Git fixture named with spaces and Swedish letters, with an unrelated uncommitted edit.\n\
         2. Launch, add the fixture as a project, send a task that writes hello.txt and runs two commands.\n\
         3. Deny `touch declined.txt`, allow `touch allowed.txt`; check the files and the dirty file's hash.\n\
         4. Stop a long reply. Kill the engine during another reply; check the unknown outcome is explained.\n\
         5. Type a draft, start a projectless chat that writes notes.md, quit and check no engine is left.\n\
         6. Relaunch: the draft is back; both chats resume and answer from their earlier context.\n\n\
         Not covered here: Finder launch (checked by hand with the packaged app), VoiceOver, real IME.\n",
    )
    .map_err(|e| e.to_string())?;
    let status = cargo()
        .args(["test", "-p", "bukno-desktop", "--test", "codex_live", "--", "--nocapture", "--test-threads=1"])
        .env("BUKNO_E2E_LIVE", "1")
        .env("BUKNO_CODEX_EFFORT", "low")
        .env("BUKNO_EVIDENCE_DIR", &dir)
        .status()
        .map_err(|e| e.to_string())?;
    println!("e2e evidence: {}", dir.display());
    if status.success() { Ok(()) } else { Err("the Codex pass1 flow failed; see result.json".into()) }
}

fn bench(args: &[String]) -> Result {
    let workload = option(args, "--workload").unwrap_or("idle");
    let samples: usize = option(args, "--samples").and_then(|s| s.parse().ok()).unwrap_or(3);
    let (scenario, pilot, measure, linger) = match workload {
        "scroll" => ("streaming", "scroll", 20.0, 12.0),
        "idle" => ("long-chat", "idle", 30.0, 0.0),
        "orb" => ("working", "orb", 30.0, 0.0),
        "startup" => ("long-chat", "screenshot", 0.0, 0.0),
        other => return Err(format!("unknown workload {other}")),
    };
    let app = package(&["--platform".into(), platform().into()])?;
    let binary = app.join("Contents/MacOS/bukno");
    let dir = evidence_dir("synthetic", &format!("bench-{workload}"));
    manifest(&dir, workload, "synthetic")?;
    let mut runs = Vec::new();
    // One warm-up, then the measured samples (section 21).
    for sample in 0..=samples {
        let run_dir = if sample == 0 { dir.join("warmup") } else { dir.join(format!("sample-{sample}")) };
        std::fs::create_dir_all(&run_dir).map_err(|e| e.to_string())?;
        let launched = Instant::now();
        let mut child = Command::new(&binary)
            .args(["--scenario", scenario, "--size", "1440x900"])
            .env("BUKNO_AUTOPILOT", pilot)
            .env("BUKNO_EVIDENCE_DIR", &run_dir)
            .env("BUKNO_SETTLE_SECS", if workload == "startup" { "0.5" } else { "3" })
            .env("BUKNO_MEASURE_SECS", measure.to_string())
            .env("BUKNO_LINGER_SECS", linger.to_string())
            .spawn()
            .map_err(|e| e.to_string())?;
        let pid = child.id();
        let mut footprints = Vec::new();
        let sample_every = Duration::from_secs(2);
        let mut next = Instant::now() + sample_every;
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                if !status.success() {
                    return Err(format!("app exited with {status} during {workload}"));
                }
                break;
            }
            if Instant::now() >= next {
                if let Some(mb) = footprint_mb(pid) {
                    footprints.push(json!({ "t": launched.elapsed().as_secs_f64(), "phys_footprint_mb": mb }));
                }
                next += sample_every;
            }
            if launched.elapsed() > Duration::from_secs(180) {
                let _ = child.kill();
                return Err(format!("{workload} did not finish in 180 s"));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let wall = launched.elapsed().as_secs_f64();
        let metrics = std::fs::read_to_string(run_dir.join("metrics.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok());
        let run = json!({ "sample": sample, "warmup": sample == 0, "wall_seconds": wall, "footprint": footprints, "metrics": metrics });
        write_json(&run_dir.join("run.json"), &run)?;
        runs.push(run);
    }
    write_json(
        &dir.join("metrics.json"),
        &json!({
            "workload": workload,
            "metric": "macOS phys_footprint from /usr/bin/footprint (app process only; Pass 0 has no engine children)",
            "binary": binary,
            "runs": runs,
        }),
    )?;
    println!("bench evidence: {}", dir.display());
    Ok(())
}

/// Physical footprint in MB, the macOS metric section 21 asks for.
fn footprint_mb(pid: u32) -> Option<f64> {
    let text = output("footprint", &["-p", &pid.to_string()])?;
    let line = text.lines().find(|l| l.trim_start().starts_with("phys_footprint:"))?;
    let value = line.split(':').nth(1)?.trim();
    let (number, unit) = value.split_once(' ')?;
    let n: f64 = number.parse().ok()?;
    Some(match unit {
        "KB" => n / 1024.0,
        "MB" => n,
        "GB" => n * 1024.0,
        _ => n / (1024.0 * 1024.0),
    })
}
