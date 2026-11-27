//! The app's own settings, kept in `%APPDATA%\victory-optimized\gui.toml`
//! (outside the repository, so paths on this machine are never committed).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings {
    /// The repository: where Mandy's layer is built and launched from.
    pub repo: PathBuf,
    /// The title's executable, as the player chose it.
    pub game_exe: String,
    /// The graphics preset (`crate::optimal::Graphics`); the rest of the
    /// launch is picked from the hardware.
    #[serde(alias = "vn_graphics")]
    pub graphics: String,
}

/// Helper threads for a 6-core processor. The layer's defaults start 8 list
/// translators, 6 landing and 6 view threads and 8 pipeline workers beside
/// the title's own threads; these are the counts Maykzera/victory-rtx3060
/// measured best on an RTX 3060 with a Ryzen 5 5600X ("more helper threads
/// are worse"), and the helpers run at the submitting thread's raised
/// priority (`VICTORY_HELPER_PRIORITY`: the layer's notes saw them woken and
/// not running for 4-11 ms a frame while the title's workers held every
/// processor; scarlet-fps f52eefb82 does the same).
pub const SIX_CORES: [(&str, &str); 5] = [
    ("VICTORY_THIN_THREADS", "4"),
    ("VICTORY_UNDER_THREADS", "3"),
    ("VICTORY_VIEW_THREADS", "3"),
    ("VICTORY_PIPELINE_WORKERS", "4"),
    ("VICTORY_HELPER_PRIORITY", "1"),
];

fn file() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("victory-optimized").join("gui.toml"))
}

/// The repository this app belongs to: the first folder above its own
/// executable (or the current folder) that holds the layer's launcher.
fn find_repo() -> PathBuf {
    let starts = [std::env::current_exe().ok(), std::env::current_dir().ok()];
    for start in starts.into_iter().flatten() {
        for dir in start.ancestors() {
            if dir.join("Cargo.toml").is_file() && dir.join("scripts").join("play-nvidia.ps1").is_file() {
                return dir.to_path_buf();
            }
        }
    }
    std::env::current_dir().unwrap_or_default()
}

impl Settings {
    pub fn load() -> Settings {
        let mut s: Settings = file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| toml::from_str(&t).ok()).unwrap_or_default();
        // An empty or relative path would resolve against wherever the app
        // was started from: only an absolute one that holds the launcher.
        if !s.repo.is_absolute() || !s.repo.join("scripts").join("play-nvidia.ps1").is_file() {
            s.repo = find_repo();
        }
        s.graphics = crate::optimal::Graphics::parse(&s.graphics).name().to_owned();
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

    /// The layer's launcher.
    pub fn launcher(&self) -> PathBuf {
        self.repo.join("scripts").join("play-nvidia.ps1")
    }

    /// The layer's release build, which the launcher runs.
    pub fn layer_exe(&self) -> PathBuf {
        self.repo.join("target").join("release").join("victory.exe")
    }

    /// NVIDIA's Streamline and DLSS files, where the launcher looks for them.
    pub fn dlss_files(&self) -> bool {
        self.repo.join("nvidia-sdk").join("Streamline").join("bin").join("x64").join("sl.interposer.dll").is_file()
    }
}

#[cfg(test)]
mod tests {
    /// Settings written by earlier versions still load: the old preset name
    /// is taken, the fields that are gone are passed over.
    #[test]
    fn an_older_settings_file_still_loads() {
        let old = "game_exe = \"D:/Game/game.exe\"\nprofile = \"fast\"\nlayer = \"vnvidia\"\nvn_graphics = \"quality\"\n";
        let s: super::Settings = toml::from_str(old).unwrap();
        assert_eq!(s.game_exe, "D:/Game/game.exe");
        assert_eq!(s.graphics, "quality");
    }
}
