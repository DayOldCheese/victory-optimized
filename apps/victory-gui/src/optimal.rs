//! The best launch for this PC, worked out from its hardware: one choice
//! for the player (the graphics preset), everything else picked here.
//!
//! What each rule rests on:
//! - Series S on cards that the Series X profile overfills: on an RTX 3060
//!   12 GB the Series X profile at 1440p left 13,000 textures waiting with
//!   "the card has no room for more" (run 2026-11-27 21:09), the Series S
//!   profile ran 5-9 fps with textures loading (21:24).
//! - Frame generation is made ready only where DLSS frame generation runs
//!   (RTX 40 and later): made ready, it holds back 14% of the card's memory
//!   from textures (the launcher's VICTORY_TEXTURE_ROOM_PERCENT=86), and
//!   AMD's FSR 3 in its place smears below about 15 rendered fps.
//! - Fewer helper threads on processors of 12 threads or fewer (see
//!   [`crate::settings::SIX_CORES`]).
//! - Shaders translated before the game (`-PreloadShaders`). Not the full
//!   preload's pipelines: making every pipeline earlier runs used took far
//!   too long on a Ryzen 5 5600X; the driver's own cache and the game's
//!   loading make the ones it needs.
//! - Ray tracing only when asked for (Ultra): the title's ray-tracing
//!   structure builder runs, but slowly and with faults on NVIDIA cards today
//!   (VIctory-NVIDIA's own menu: "RAY TRACING (SLOW, BROKEN)").

/// What the player picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Graphics {
    /// Chosen from the hardware.
    Auto,
    /// Series S profile: lightest.
    Performance,
    /// Series X profile at 1440p: the console's best picture.
    Quality,
    /// Quality with the title's own ray tracing (slow, experimental).
    Ultra,
}

impl Graphics {
    pub const ALL: [Graphics; 4] = [Graphics::Auto, Graphics::Performance, Graphics::Quality, Graphics::Ultra];

    pub fn name(self) -> &'static str {
        match self {
            Graphics::Auto => "auto",
            Graphics::Performance => "performance",
            Graphics::Quality => "quality",
            Graphics::Ultra => "ultra",
        }
    }

    pub fn parse(s: &str) -> Graphics {
        Graphics::ALL.into_iter().find(|g| g.name() == s).unwrap_or(Graphics::Auto)
    }

    pub fn label(self) -> &'static str {
        match self {
            Graphics::Auto => "Auto",
            Graphics::Performance => "Performance",
            Graphics::Quality => "Quality (Series X)",
            Graphics::Ultra => "Ultra (Series X + ray tracing)",
        }
    }

    pub fn note(self) -> &'static str {
        match self {
            Graphics::Auto => "Picked from your graphics card and processor.",
            Graphics::Performance => "Series S profile: 1080p, lightest on video memory, the fastest.",
            Graphics::Quality => "Series X profile at 1440p: the console's best picture. Needs a strong card and 12 GB or more.",
            Graphics::Ultra => "Series X with the game's own ray-traced light. Experimental: slow, and with faults on NVIDIA cards today.",
        }
    }
}

/// The PC, as far as the choice needs it.
#[derive(Clone, Debug, Default)]
pub struct Hardware {
    pub gpu_name: String,
    pub vram_gb: f64,
    pub threads: usize,
}

impl Hardware {
    /// Read from this PC.
    pub fn here() -> Hardware {
        let (vram_gb, gpu_name) = crate::sysinfo::adapter().unwrap_or((0.0, String::new()));
        let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
        Hardware { gpu_name, vram_gb, threads }
    }

    /// An RTX 40 or 50 card: DLSS frame generation runs there.
    pub fn frame_generation(&self) -> bool {
        // The model number after "RTX ": 4070, 5060 and so on.
        let n = self.gpu_name.to_ascii_uppercase();
        n.find("RTX ").is_some_and(|i| {
            let model: String = n[i + 4..].chars().take_while(char::is_ascii_digit).collect();
            model.len() == 4 && (model.starts_with('4') || model.starts_with('5'))
        })
    }

    /// Strong enough for the Series X profile by default: 16 GB of video
    /// memory, or 12 GB on an RTX 40 or 50 card (the Series X profile at
    /// 1440p was measured at 26-30 fps on an RTX 5070 12 GB).
    fn series_x_class(&self) -> bool {
        self.vram_gb >= 15.5 || (self.vram_gb >= 11.5 && self.frame_generation())
    }
}

/// How to launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The preset this comes to (never Auto).
    pub graphics: Graphics,
    /// Arguments for VIctory-NVIDIA's launcher after `-Exe <path>`.
    pub args: Vec<String>,
    /// Lines its settings file must hold (`VICTORY_X=value`).
    pub set: Vec<(String, String)>,
    /// Lines its settings file must not hold.
    pub unset: Vec<String>,
}

/// The switches [`plan`] manages in the settings file.
pub const RAY_TRACING: &str = "VICTORY_BVH_BUILDER";

pub fn plan(asked: Graphics, hw: &Hardware) -> Plan {
    let graphics = match asked {
        Graphics::Auto if hw.series_x_class() => Graphics::Quality,
        Graphics::Auto => Graphics::Performance,
        g => g,
    };
    let mut args: Vec<String> = Vec::new();
    // DLSS: Performance renders a quarter of the pixels; Series X keeps
    // Quality where the card has the memory, Balanced on 12 GB.
    let dlss = match graphics {
        Graphics::Performance => "performance",
        _ if hw.vram_gb >= 15.5 => "quality",
        _ => "balanced",
    };
    if graphics == Graphics::Performance {
        args.push("-SeriesS".into());
    } else {
        args.extend(["-Res".into(), "1440".into()]);
    }
    args.extend(["-Dlss".into(), dlss.into()]);
    args.push("-PreloadShaders".into());
    if !hw.frame_generation() {
        args.push("-NoFrameGenReady".into());
    }
    let mut set = Vec::new();
    let mut unset = Vec::new();
    if hw.threads > 0 && hw.threads <= 12 {
        set.extend(crate::settings::SIX_CORES.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())));
    } else {
        unset.extend(crate::settings::SIX_CORES.iter().map(|(k, _)| (*k).to_owned()));
    }
    if graphics == Graphics::Ultra {
        set.push((RAY_TRACING.into(), "1".into()));
    } else {
        unset.push(RAY_TRACING.into());
    }
    Plan { graphics, args, set, unset }
}

/// Where VIctory-Hydra's launcher (`Play-VIctory.cmd`, which keeps any
/// `VICTORY_` switch already set) finds what it does not carry itself.
pub struct HydraPaths {
    pub game_dir: String,
    /// NVIDIA's Streamline and DLSS files, if any (its own `nvidia-sdk`, or
    /// VIctory-NVIDIA's beside it).
    pub streamline: Option<String>,
    /// AMD's FidelityFX files, likewise.
    pub fidelityfx: Option<String>,
}

/// VIctory-Hydra's launch for this PC: the preset it comes to and the
/// switches over its launcher's defaults (which are already its measured
/// set for an RTX 3060 with a Ryzen 5 5600X: 6-core helper threads and so
/// on). Only what the preset and the hardware change is set here.
pub fn hydra_env(asked: Graphics, hw: &Hardware, paths: &HydraPaths) -> (Graphics, Vec<(String, String)>) {
    let p = plan(asked, hw);
    let mut env: Vec<(String, String)> = vec![("VICTORY_GAME_DIR".into(), paths.game_dir.clone())];
    let set = |env: &mut Vec<(String, String)>, k: &str, v: &str| env.push((k.to_owned(), v.to_owned()));
    // The frame-rate line (`victory_gpu::host`) and the frame's account
    // (`copystat`) are kept: its own default, warnings only, drops both.
    set(&mut env, "VICTORY_LOG", "warn,victory_gpu::host=info,victory_gpu::copystat=info,victory_gpu::list::thin=info");
    match p.graphics {
        Graphics::Performance | Graphics::Auto => {
            set(&mut env, "VICTORY_CONSOLE", "series-s");
            set(&mut env, "VICTORY_OUTPUT", "1920x1080");
        }
        Graphics::Quality | Graphics::Ultra => {
            set(&mut env, "VICTORY_CONSOLE", "series-x");
            set(&mut env, "VICTORY_OUTPUT", "2560x1440");
        }
    }
    let dlss = p.args.windows(2).find(|w| w[0] == "-Dlss").map_or("balanced", |w| w[1].as_str()).to_owned();
    if let Some(sl) = &paths.streamline {
        set(&mut env, "VICTORY_STREAMLINE_DIR", sl);
        set(&mut env, "VICTORY_DLSS", "1");
        set(&mut env, "VICTORY_DLSS_MODE", &dlss);
    }
    // FSR 3 frame generation only where frame generation is worth it (see
    // `plan`); its launcher turns it on whenever AMD's files are there, and
    // then keeps a fifth of the card from textures.
    if let Some(fx) = &paths.fidelityfx {
        set(&mut env, "VICTORY_FIDELITYFX_DIR", fx);
    }
    if !hw.frame_generation() {
        set(&mut env, "VICTORY_FSR_FRAMEGEN", "0");
    }
    if p.graphics == Graphics::Ultra {
        set(&mut env, RAY_TRACING, "1");
        set(&mut env, "VICTORY_SKIP_RT_TRACE", "0");
    }
    (p.graphics, env)
}

/// `text` (a settings file of `VICTORY_X=value` lines) with the plan's lines
/// set and unset; its other lines stay.
pub fn apply(text: &str, p: &Plan) -> String {
    let managed = |l: &str| {
        l.split_once('=').is_some_and(|(n, _)| {
            let n = n.trim();
            p.set.iter().any(|(k, _)| k == n) || p.unset.iter().any(|k| k == n)
        })
    };
    let mut out: Vec<String> = text.lines().filter(|l| !managed(l)).map(str::to_owned).collect();
    out.extend(p.set.iter().map(|(k, v)| format!("{k}={v}")));
    let mut s = out.join("\r\n");
    if !s.is_empty() {
        s.push_str("\r\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pc(name: &str, vram: f64, threads: usize) -> Hardware {
        Hardware { gpu_name: name.into(), vram_gb: vram, threads }
    }

    #[test]
    fn a_3060_with_a_5600x_gets_the_light_set() {
        let p = plan(Graphics::Auto, &pc("NVIDIA GeForce RTX 3060", 11.8, 12));
        assert_eq!(p.graphics, Graphics::Performance);
        assert!(p.args.contains(&"-SeriesS".to_owned()));
        assert!(p.args.contains(&"-PreloadShaders".to_owned()));
        assert!(!p.args.contains(&"-Preload".to_owned()));
        assert!(p.args.contains(&"-NoFrameGenReady".to_owned()));
        assert!(p.set.iter().any(|(k, v)| k == "VICTORY_THIN_THREADS" && v == "4"));
        assert!(p.unset.contains(&RAY_TRACING.to_owned()));
    }

    #[test]
    fn strong_cards_get_series_x_and_frame_generation_stays_ready() {
        let p = plan(Graphics::Auto, &pc("NVIDIA GeForce RTX 5070", 11.9, 16));
        assert_eq!(p.graphics, Graphics::Quality);
        assert!(!p.args.contains(&"-SeriesS".to_owned()));
        assert!(!p.args.contains(&"-NoFrameGenReady".to_owned()));
        assert!(p.unset.iter().any(|k| k == "VICTORY_THIN_THREADS"));
        let big = plan(Graphics::Auto, &pc("NVIDIA GeForce RTX 3090", 24.0, 24));
        assert_eq!(big.graphics, Graphics::Quality);
        assert!(big.args.windows(2).any(|w| w == ["-Dlss", "quality"]));
    }

    #[test]
    fn ultra_turns_ray_tracing_on_and_the_others_off() {
        let hw = pc("NVIDIA GeForce RTX 3060", 11.8, 12);
        let ultra = plan(Graphics::Ultra, &hw);
        assert!(ultra.set.iter().any(|(k, v)| k == RAY_TRACING && v == "1"));
        let file = apply("# Saved by the F1 menu\r\nVICTORY_AMBIENT_SPECULAR=\r\n", &ultra);
        assert!(file.contains("VICTORY_AMBIENT_SPECULAR=\r\n") && file.contains("VICTORY_BVH_BUILDER=1"));
        let back = apply(&file, &plan(Graphics::Quality, &hw));
        assert!(!back.contains("VICTORY_BVH_BUILDER") && back.contains("VICTORY_AMBIENT_SPECULAR="));
        assert_eq!(back.matches("VICTORY_THIN_THREADS").count(), 1);
    }

    #[test]
    fn hydra_gets_the_preset_and_keeps_its_own_tuning() {
        let paths = HydraPaths { game_dir: "D:/Game".into(), streamline: Some("D:/sl".into()), fidelityfx: Some("D:/fx".into()) };
        let get = |env: &[(String, String)], k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let (g, env) = hydra_env(Graphics::Auto, &pc("NVIDIA GeForce RTX 3060", 11.8, 12), &paths);
        assert_eq!(g, Graphics::Performance);
        assert_eq!(get(&env, "VICTORY_CONSOLE").as_deref(), Some("series-s"));
        assert_eq!(get(&env, "VICTORY_DLSS_MODE").as_deref(), Some("performance"));
        assert_eq!(get(&env, "VICTORY_FSR_FRAMEGEN").as_deref(), Some("0"));
        assert_eq!(get(&env, "VICTORY_GAME_DIR").as_deref(), Some("D:/Game"));
        // Its own 6-core counts are left to it.
        assert_eq!(get(&env, "VICTORY_THIN_THREADS"), None);
        assert!(get(&env, "VICTORY_LOG").unwrap().contains("victory_gpu::host=info"));
        let (u, env) = hydra_env(Graphics::Ultra, &pc("NVIDIA GeForce RTX 4070", 12.0, 16), &HydraPaths { streamline: None, ..paths });
        assert_eq!(u, Graphics::Ultra);
        assert_eq!(get(&env, "VICTORY_CONSOLE").as_deref(), Some("series-x"));
        assert_eq!(get(&env, RAY_TRACING).as_deref(), Some("1"));
        assert_eq!(get(&env, "VICTORY_SKIP_RT_TRACE").as_deref(), Some("0"));
        assert_eq!(get(&env, "VICTORY_DLSS"), None);
        assert_eq!(get(&env, "VICTORY_FSR_FRAMEGEN"), None);
    }

    #[test]
    fn frame_generation_is_read_from_the_card_name() {
        assert!(pc("NVIDIA GeForce RTX 4070 SUPER", 12.0, 16).frame_generation());
        assert!(pc("NVIDIA GeForce RTX 5060 Ti", 16.0, 16).frame_generation());
        assert!(!pc("NVIDIA GeForce RTX 3080", 12.0, 16).frame_generation());
        assert!(!pc("NVIDIA GeForce RTX 2080 SUPER", 8.0, 12).frame_generation());
        assert!(!pc("AMD Radeon RX 7900 XTX", 24.0, 32).frame_generation());
    }
}
