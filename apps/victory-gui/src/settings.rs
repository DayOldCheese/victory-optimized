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
    /// `play`, `raytracing`, `safe` or `stable` (`victory_core::profiles`).
    pub profile: String,
    /// Switches the user set by hand: variable -> value (an empty value
    /// means "off": the profile's value is not used).
    pub overrides: BTreeMap<String, String>,
    /// A checkout of NVIDIA/DLSS (its `include` and `lib`), for builds with
    /// DLSS; empty: looked for as `sdk\DLSS` beside the repository.
    pub dlss_sdk: String,
    /// Which layer Launch runs: `mandy` (this repository's own) or
    /// `vnvidia`: a checkout of trustno1-coder/VIctory-NVIDIA (foxhound's
    /// line), started through its own launcher with its tested settings.
    pub layer: String,
    /// That checkout; empty: `mandy-vn` beside the repository.
    pub vn_dir: String,
    /// Its Series S profile (`-SeriesS`): lighter on video memory.
    pub vn_series_s: bool,
    /// Its DLSS mode (`-Dlss`): off, dlaa, quality, balanced, performance.
    pub vn_dlss: String,
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
            s.profile = "play".into();
        }
        if !matches!(s.layer.as_str(), "mandy" | "vnvidia") {
            s.layer = "mandy".into();
        }
        if !matches!(s.vn_dlss.as_str(), "off" | "dlaa" | "quality" | "balanced" | "performance") {
            s.vn_dlss = "quality".into();
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

    /// The environment a launch gets: the profile's switches with the
    /// overrides over them (an override that turns a switch off is "0").
    /// With DLSS asked for, `VICTORY_DLSS_DIR` is the SDK's runtime folder
    /// unless set by hand.
    pub fn launch_env(&self) -> Vec<(String, String)> {
        let profile = victory_core::profiles::Profile::parse(&self.profile).unwrap_or(victory_core::profiles::Profile::Play);
        let mut env = profile.env(&self.overrides);
        let dlss = env.iter().any(|(k, v)| k == "VICTORY_UPSCALE" && v.eq_ignore_ascii_case("dlss"));
        if dlss
            && !env.iter().any(|(k, _)| k == "VICTORY_DLSS_DIR")
            && let Some(sdk) = self.dlss_sdk_dir()
        {
            env.push(("VICTORY_DLSS_DIR".into(), sdk.join("lib").join("Windows_x86_64").join("rel").display().to_string()));
        }
        env
    }

    /// The VIctory-NVIDIA checkout, if there is one (with its launcher).
    pub fn vn_checkout(&self) -> Option<PathBuf> {
        let dir = if self.vn_dir.trim().is_empty() { self.repo.parent()?.join("mandy-vn") } else { PathBuf::from(self.vn_dir.trim()) };
        dir.join("scripts").join("play-nvidia.ps1").is_file().then_some(dir)
    }

    /// Whether Launch runs VIctory-NVIDIA.
    pub fn uses_vn(&self) -> bool {
        self.layer == "vnvidia"
    }

    /// The DLSS SDK, if there is one: the one set, else `sdk\DLSS` beside
    /// the repository. Only one with the NGX loader library counts.
    pub fn dlss_sdk_dir(&self) -> Option<PathBuf> {
        let set = PathBuf::from(&self.dlss_sdk);
        let guess = self.repo.parent().map(|p| p.join("sdk").join("DLSS"));
        let dir = if self.dlss_sdk.trim().is_empty() { guess? } else { set };
        dir.join("lib").join("Windows_x86_64").join("x64").join("nvsdk_ngx_d.lib").is_file().then_some(dir)
    }
}


#[cfg(test)]
mod tests {
    #[test]
    fn the_dlss_sdk_is_found_beside_the_repository_and_named_to_the_title() {
        let top = tempfile::tempdir().unwrap();
        let repo = top.path().join("Mandy");
        std::fs::create_dir_all(&repo).unwrap();
        let mut s = super::Settings { repo: repo.clone(), profile: "play".into(), ..Default::default() };
        assert_eq!(s.dlss_sdk_dir(), None);
        let lib = top.path().join("sdk").join("DLSS").join("lib").join("Windows_x86_64").join("x64");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("nvsdk_ngx_d.lib"), b"").unwrap();
        assert_eq!(s.dlss_sdk_dir(), Some(top.path().join("sdk").join("DLSS")));
        // Only with DLSS asked for does the title get the runtime's folder.
        assert!(!s.launch_env().iter().any(|(k, _)| k == "VICTORY_DLSS_DIR"));
        s.overrides.insert("VICTORY_UPSCALE".into(), "dlss".into());
        let env = s.launch_env();
        let dir = env.iter().find(|(k, _)| k == "VICTORY_DLSS_DIR").map(|(_, v)| v.clone()).unwrap();
        assert!(dir.ends_with("rel"), "{dir}");
    }

    #[test]
    fn an_empty_repo_path_is_replaced_by_the_workspace() {
        let s = super::Settings { repo: std::path::PathBuf::new(), ..Default::default() };
        assert!(!s.repo.is_absolute());
        let found = super::find_repo();
        assert!(found.is_absolute() && found.join("crates").join("victory-core").is_dir(), "{found:?}");
    }
}
