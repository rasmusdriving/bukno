//! Preferences in `<app state>/config.toml`. Never credentials.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// Projectless chats, attachments and outputs. Chosen during onboarding.
    pub work_folder: Option<PathBuf>,
    /// The permission preset new Codex chats start with.
    pub codex_preset: Option<String>,
}

pub fn path(state_dir: &Path) -> PathBuf {
    state_dir.join("config.toml")
}

pub fn load(state_dir: &Path) -> Config {
    let Ok(text) = std::fs::read_to_string(path(state_dir)) else {
        return Config::default();
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return Config::default();
    };
    let s = |k: &str| table.get(k).and_then(|v| v.as_str()).map(str::to_owned);
    Config { work_folder: s("work_folder").map(PathBuf::from), codex_preset: s("codex_preset") }
}

pub fn save(state_dir: &Path, config: &Config) -> std::io::Result<()> {
    let mut table = toml::Table::new();
    if let Some(folder) = &config.work_folder {
        table.insert("work_folder".into(), toml::Value::String(folder.display().to_string()));
    }
    if let Some(preset) = &config.codex_preset {
        table.insert("codex_preset".into(), toml::Value::String(preset.clone()));
    }
    let text = format!(
        "# Bukno preferences. Written by Bukno; safe to read.\n{}",
        toml::to_string(&table).unwrap_or_default()
    );
    std::fs::create_dir_all(state_dir)?;
    let target = path(state_dir);
    let tmp = target.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, target)
}
