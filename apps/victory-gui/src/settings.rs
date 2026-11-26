//! The GUI's own settings, kept in `%APPDATA%\victory-optimized\gui.toml`
//! (outside the repository, so paths on this machine are never committed).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    /// The repository: where cargo and git run.
    pub repo: PathBuf,
    /// The title's executable, as the user chose it.
    pub game_exe: String,
    /// Arguments for the title, after `--`.
    pub game_args: String,
    /// `stable`, `performance` or `experimental`.
    pub profile: String,
    /// Switches the user set by hand: variable -> value (an empty value
    /// means "off": the profile's value is not used).
    pub overrides: BTreeMap<String, String>,
}

fn file() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("victory-optimized").join("gui.toml"))
}

/// The repository this GUI belongs to: the first folder above its own
/// executable (or the current folder) that holds the workspace.
fn find_repo() -> PathBuf {
    let starts = [std::env::current_exe().ok(), std::env::current_dir().ok()];
    for start in starts.into_iter().flatten() {
        for dir in start.ancestors() {
            if dir.join("Cargo.toml").is_file() && dir.join("crates").join("victory-core").is_dir() {
                return dir.to_path_buf();
            }
        }
    }
    std::env::current_dir().unwrap_or_default()
}

impl Settings {
    pub fn load() -> Settings {
        let mut s: Settings = file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| toml::from_str(&t).ok()).unwrap_or_default();
        // An empty or relative path would resolve against wherever the GUI
        // was started from: only an absolute one that holds the workspace.
        if !s.repo.is_absolute() || !s.repo.join("Cargo.toml").is_file() {
            s.repo = find_repo();
        }
        if victory_core::profiles::Profile::parse(&s.profile).is_none() {
            s.profile = "stable".into();
        }
        s
    }

    pub fn save(&self) -> Result<(), String> {
        let f = file().ok_or("no APPDATA folder")?;
        if let Some(dir) = f.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&f, text).map_err(|e| e.to_string())
    }

    /// The environment a launch gets: the overrides; the profile itself is
    /// passed to `victory run --profile`, which keeps variables already set.
    /// An override that turns a profile switch off is passed as "0".
    pub fn launch_env(&self) -> Vec<(String, String)> {
        self.overrides.iter().map(|(k, v)| (k.clone(), if v.is_empty() { "0".into() } else { v.clone() })).collect()
    }
}


#[cfg(test)]
mod tests {
    #[test]
    fn an_empty_repo_path_is_replaced_by_the_workspace() {
        let s = super::Settings { repo: std::path::PathBuf::new(), ..Default::default() };
        assert!(!s.repo.is_absolute());
        let found = super::find_repo();
        assert!(found.is_absolute() && found.join("crates").join("victory-core").is_dir(), "{found:?}");
    }
}
