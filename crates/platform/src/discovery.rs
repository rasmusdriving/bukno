//! Finding installed tools without an interactive shell (section 15).
//!
//! A Finder launch gets a minimal PATH, so Bukno looks in the places user
//! installers put engines, then in the inherited PATH. It never sources shell
//! startup files.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Where an engine came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// The path that was found, possibly a wrapper script or symlink.
    pub path: PathBuf,
    /// The native executable to launch.
    pub native: PathBuf,
    /// Set when `path` is an npm package's JavaScript wrapper.
    pub npm_package: Option<PathBuf>,
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Version-named directories, newest first ("24.13.1" before "22.22.3").
fn versions_newest_first(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<(Vec<u64>, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().trim_start_matches('v').to_owned();
            let parts: Option<Vec<u64>> = name.split('.').map(|p| p.parse().ok()).collect();
            parts.map(|p| (p, e.path()))
        })
        .collect();
    dirs.sort_by(|a, b| b.0.cmp(&a.0));
    dirs.into_iter().map(|(_, p)| p).collect()
}

/// Candidate locations for an executable named `name`, in search order:
/// user installers first, then the inherited PATH.
pub fn candidates(name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = home() {
        for root in [".local/share/mise/installs/node", ".nvm/versions/node", ".local/share/fnm/node-versions"] {
            for version in versions_newest_first(&home.join(root)) {
                out.push(version.join("bin").join(name));
                out.push(version.join("installation/bin").join(name));
            }
        }
        for dir in [".volta/bin", ".bun/bin", ".npm-global/bin", ".local/bin", ".cargo/bin"] {
            out.push(home.join(dir).join(name));
        }
    }
    for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
        out.push(Path::new(dir).join(name));
    }
    if let Some(path) = std::env::var_os("PATH") {
        out.extend(std::env::split_paths(&path).map(|dir| dir.join(name)));
    }
    out
}

/// The first usable engine named `name`, resolving npm wrappers to their
/// native binary so no extra Node process runs.
pub fn find_engine(name: &str) -> Option<Found> {
    candidates(name).into_iter().find_map(|path| inspect(&path))
}

/// Check one path: it must exist and be executable; wrappers are resolved.
pub fn inspect(path: &Path) -> Option<Found> {
    if !is_executable(path) {
        return None;
    }
    let real = std::fs::canonicalize(path).ok()?;
    if let Some((native, package)) = npm_native(&real) {
        return Some(Found { path: path.to_owned(), native, npm_package: Some(package) });
    }
    if is_script(&real) {
        // A shim we cannot see through (for example a version manager's). Using
        // it would need that manager's environment, so skip it.
        return None;
    }
    Some(Found { path: path.to_owned(), native: real, npm_package: None })
}

fn is_script(path: &Path) -> bool {
    let mut head = [0u8; 2];
    std::fs::File::open(path).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok() && head == *b"#!"
}

/// For `@openai/codex/bin/codex.js`, the native binary the wrapper would start.
fn npm_native(real: &Path) -> Option<(PathBuf, PathBuf)> {
    if real.extension().is_none_or(|e| e != "js") {
        return None;
    }
    let package = real.parent()?.parent()?.to_owned();
    let triple = target_triple()?;
    let platform_package = format!("codex-{}", npm_platform()?);
    let binary = if cfg!(windows) { "codex.exe" } else { "codex" };
    let roots = [
        package.join("node_modules/@openai").join(&platform_package),
        package.parent()?.join(&platform_package),
        package.clone(),
    ];
    roots
        .iter()
        .map(|root| root.join("vendor").join(triple).join("bin").join(binary))
        .find(|p| is_executable(p))
        .map(|native| (native, package))
}

fn target_triple() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        _ => return None,
    })
}

fn npm_platform() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("windows", "x86_64") => "win32-x64",
        ("windows", "aarch64") => "win32-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        _ => return None,
    })
}

/// A working Git: one that answers `--version`. The macOS `/usr/bin/git`
/// shim fails while the Xcode license is not accepted, so real installs come first.
pub fn find_git() -> Result<PathBuf, String> {
    let mut tried = Vec::new();
    let mut list = vec![
        PathBuf::from("/opt/homebrew/bin/git"),
        PathBuf::from("/usr/local/bin/git"),
        PathBuf::from("/Library/Developer/CommandLineTools/usr/bin/git"),
    ];
    list.extend(candidates("git"));
    list.push(PathBuf::from("/usr/bin/git"));
    for path in list {
        if !is_executable(&path) || tried.contains(&path) {
            continue;
        }
        let ok = std::process::Command::new(&path)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            return Ok(path);
        }
        tried.push(path);
    }
    Err(if tried.iter().any(|p| p == Path::new("/usr/bin/git")) {
        "Git is not working. If you have not accepted the Xcode license, run `sudo xcodebuild -license` or install the Command Line Tools.".into()
    } else {
        "Git is not installed.".into()
    })
}

/// A PATH for an engine launched from Finder: the inherited PATH plus the
/// engine's own folder and the common install folders, without duplicates.
pub fn engine_path(engine: &Found) -> OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = engine.path.parent() {
        dirs.push(dir.to_owned());
    }
    if let Some(dir) = engine.native.parent() {
        dirs.push(dir.to_owned());
    }
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.extend(["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(PathBuf::from));
    let mut seen = Vec::new();
    dirs.retain(|d| {
        let fresh = !seen.contains(d);
        seen.push(d.clone());
        fresh
    });
    std::env::join_paths(dirs).unwrap_or_default()
}
