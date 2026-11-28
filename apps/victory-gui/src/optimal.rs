//! The best launch for this PC, worked out from its hardware: one choice
//! for the player (the graphics preset), everything else picked here.
//!
//! What each rule rests on:
//! - Series S on cards that the Series X profile overfills: on an RTX 3060
//!   12 GB the Series X profile at 1440p left 13,000 textures waiting with
//!   "the card has no room for more" (run 2026-11-27 21:09), the Series S
//!   profile ran 5-9 fps with textures loading (21:24).
//! - Frame generation on, 2 frames shown for each one rendered (`-FrameGen`):
//!   DLSS frame generation on RTX 40 and later, AMD's FSR 3 on other cards
//!   (the launcher's `-FgType auto`). Mandy renders 15-20 fps on an RTX 3060
//!   with a Ryzen 5 5600X (run 2026-11-27 23:16), about what FSR 3 needs to
//!   look right; on, it holds a fifth of the card's memory back from textures
//!   (the launcher's VICTORY_TEXTURE_ROOM_PERCENT=80).
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

/// Choices the game's F1 menu saves into the same file, which the launcher
/// puts over its options at every start: taken out at each launch from the
/// app, so that the graphics quality chosen here is the one that runs (a run
/// on 2026-11-28 12:15 had F1's Series X and 5x frame generation over the
/// app's Series S and 2x, and FSR 3, which does 2x at most, stayed off). A
/// change in F1 still holds until the next launch from the app.
pub const MENU_CHOICES: [&str; 8] = [
    "VICTORY_MENU_CONSOLE",
    "VICTORY_MENU_RES",
    "VICTORY_MENU_S_RES",
    "VICTORY_MENU_DLSS",
    "VICTORY_MENU_FRAMEGEN",
    "VICTORY_MENU_FG_TYPE",
    "VICTORY_MENU_FPS",
    "VICTORY_MFG_UNLOCKER",
];

/// The title's memory in place, on trial: its windows outside buffers are
/// buffers placed over the title's own memory (`VICTORY_INPLACE=1`: not 2,
/// which the layer's late wait rules out), that memory a driver heap the
/// card reads fast (`VICTORY_UMA_DRIVER`, from XCL through insenzaboor), and
/// the refreshes of what is read in place left out (`VICTORY_UMA_DIRECT`).
/// The submitting thread spent 17.6 ms of 48 refreshing and 7.7 asking the
/// write watch in a world frame on an RTX 3060 with a Ryzen 5 5600X (run
/// 2026-11-28 11:31). The layer self-tests the driver heap before the
/// title's first code and turns it off for the run if that fails.
///
/// Level 1, not 2: level 2 needs late wait off, and a 9-minute run so (2026-11-28
/// 12:45) fell to 6.0 fps from 9-12, the submitting thread up to 48.7 ms, and
/// FSR 3 frame generation stayed off (it needs the line of submissions late
/// wait runs).
///
/// Off at every start now (unset): level 1 never gained a frame. In world
/// frames of 3,000-6,000 draws on the RTX 3060 the runs without it made 11.2
/// and 14.0 fps (10:30, 11:24) with the card's part 12.9 and 3.3 ms; the runs
/// with it 11.1 and 10.6 fps (12:23, 14:05) with 45 ms, and the submissions
/// waited for flights 1.7-4.9 times a frame for title memory read, 0 without
/// it (48-87 ms a frame waiting for the card, 10-21 without). The layer now
/// counts which of its reads waits (UMA_DIRECT's reads in place, a copy at its
/// place, a copy of ours on the card) for a later trial.
pub const IN_PLACE: [(&str, &str); 3] = [("VICTORY_INPLACE", "1"), ("VICTORY_UMA_DRIVER", "1"), ("VICTORY_UMA_DIRECT", "1")];

/// The layer's own switches that ToLIManl/VIctory-Hydra's launcher
/// (`Play-VIctory.cmd`, GPL-3.0-or-later) turns on and Mandy's base launcher
/// does not, at its values: its measured set on an RTX 3060 12 GB with a
/// Ryzen 5 5600X (25.9-30.8 fps in light, 16.9-20.9 in heavy world scenes).
/// Among them the card copies of the title's buffers and windows
/// (Maykzera/victory-rtx3060 measured 1.5 GB of buffers on the card at 11 to
/// 21.8 fps), the constants heap read in place, submissions prepared on a
/// thread of their own up to 3 frames ahead and in larger batches. Mandy's
/// helper-thread counts ([`crate::settings::SIX_CORES`]) stay over them.
pub const HYDRA_SET: [(&str, &str); 21] = [
    ("VICTORY_ASYNC_SUBMIT", "1"),
    ("VICTORY_SUBMIT_FRAMES", "3"),
    ("VICTORY_JOIN_PARTS", "24"),
    ("VICTORY_JOIN_LISTS", "128"),
    ("VICTORY_CARD_BUFFERS_MB", "1536"),
    // 256 MB here, not VIctory-Hydra's 1024: on an RTX 3060 12 GB with frame
    // generation on (a fifth of the card held back) the 2.5 GB of buffers and
    // windows left textures too little room in a long session: 197,000 "no
    // room" by its end and 19 fps falling to 11 from the minute they began
    // (run 2026-11-28 10:52). The buffers keep their 1.5 GB.
    ("VICTORY_CARD_WINDOWS_MB", "256"),
    ("VICTORY_PORT_CONSTANTS", "1"),
    ("VICTORY_HEAP_SYNC_EARLY", "1"),
    ("VICTORY_LAND_SKIP", "1"),
    ("VICTORY_LAND_THREADS", "4"),
    ("VICTORY_WIDE_MERGE", "1"),
    ("VICTORY_PLACED_ORDER", "1"),
    ("VICTORY_FILL_TITLE", "2"),
    ("VICTORY_ASK_EARLY", "2"),
    ("VICTORY_KEEP_SHOWN", "1"),
    ("VICTORY_WAIT_RESIDENT", "0"),
    ("VICTORY_LIBRARY_STATIC_SAMPLERS", "1"),
    ("VICTORY_DSTORAGE_IO", "handles"),
    ("VICTORY_DSTORAGE_WORKERS", "3"),
    ("VICTORY_PIPELINE_LOWEST", "0"),
    // Not VIctory-Hydra's 0: up to 1 GB of the title's windows in card memory
    // the CPU writes directly (a D3D12 GPU upload heap), so they need no copy
    // and no list of changes (a captured frame 36 -> 32 ms, the card's lists 29
    // -> 26 ms; shadow.rs). Only with Resizable BAR on in the BIOS (the layer
    // asks D3D12 for GPU upload heaps and does without them otherwise), and
    // only while the card is under four fifths of its budget.
    ("VICTORY_WINDOWS_BAR_MB", "1024"),
];

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
    args.push("-FrameGen".into());
    let mut set: Vec<(String, String)> = HYDRA_SET.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    let mut unset: Vec<String> = MENU_CHOICES.iter().map(|k| (*k).to_owned()).collect();
    unset.extend(IN_PLACE.iter().map(|(k, _)| (*k).to_owned()));
    if hw.threads > 0 && hw.threads <= 12 {
        set.extend(crate::settings::SIX_CORES.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())));
    } else {
        unset.extend(crate::settings::SIX_CORES.iter().map(|(k, _)| (*k).to_owned()));
    }
    // MFG Unlocker is not used (the player's choice: FSR 3 frame generation).
    unset.push("VICTORY_MFG_UNLOCKER_DLL".into());
    // The two-queue plan, a dry run that only counts (everything still runs on
    // the direct queue): how many of the title's compute submissions a real
    // compute queue on the card would take, and what they would wait for.
    // It decides whether that queue is built.
    set.push(("VICTORY_ASYNC_QUEUE".into(), "plan".into()));
    // Where the card's time goes, by the title's pipelines (two timestamps
    // where a list's pipeline changes): the title's work took 94 ms of the
    // card in a 112 ms world frame on an RTX 3060 (run 2026-11-28 14:32).
    set.push(("VICTORY_CARD_PROFILE".into(), "1".into()));
    // A pipeline sync whose global UAV barrier orders nothing (nothing since
    // the last one stored) is left out: the card no longer drains for it.
    set.push(("VICTORY_SYNC_QUIET".into(), "1".into()));
    if graphics == Graphics::Ultra {
        set.push((RAY_TRACING.into(), "1".into()));
    } else {
        unset.push(RAY_TRACING.into());
    }
    Plan { graphics, args, set, unset }
}

/// `text` (a settings file of `VICTORY_X=value` lines) with the plan's lines
/// set and unset; its other lines stay.
/// The settings Mandy's F1 menu saves (`crates/victory-gpu/src/host/menu.rs`,
/// `ITEMS`): the only lines of the file kept from before.
pub const F1_KEPT: [&str; 5] = ["VICTORY_FPS_LINE", "VICTORY_PANEL", "VICTORY_FULLSCREEN", "VICTORY_MOUSE_CAPTURE", "VICTORY_IME_ENGLISH"];

/// The settings file for this start, written whole: the plan's switches and
/// the F1 menu's own lines ([`F1_KEPT`]) of `text`, nothing else. Merging into
/// what was there kept a switch an earlier start had set (late wait 0, from a
/// trial): frame generation stayed off for a whole run (2026-11-28 13:43).
pub fn apply(text: &str, p: &Plan) -> String {
    let kept = |l: &str| {
        l.split_once('=').is_some_and(|(n, _)| {
            let n = n.trim();
            F1_KEPT.contains(&n) && !p.set.iter().any(|(k, _)| k == n) && !p.unset.iter().any(|k| k == n)
        })
    };
    let mut out = vec!["# Written by the Mandy app at every start; the F1 menu adds its own lines.".to_owned()];
    out.extend(text.lines().filter(|l| kept(l)).map(str::to_owned));
    out.extend(p.set.iter().map(|(k, v)| format!("{k}={v}")));
    let mut s = out.join("\r\n");
    s.push_str("\r\n");
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
        assert!(p.args.contains(&"-FrameGen".to_owned()));
        assert!(!p.args.contains(&"-NoFrameGenReady".to_owned()));
        assert!(p.set.iter().any(|(k, v)| k == "VICTORY_THIN_THREADS" && v == "4"));
        assert!(p.set.iter().any(|(k, v)| k == "VICTORY_CARD_BUFFERS_MB" && v == "1536"));
        // Level 1 with late wait on (level 2 needs it off, which cost a third of
        // the frame rate and frame generation).
        let get = |k: &str| p.set.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("VICTORY_INPLACE"), None);
        assert!(p.unset.iter().any(|k| k == "VICTORY_INPLACE") && p.unset.iter().any(|k| k == "VICTORY_UMA_DIRECT"));
        assert_eq!(get("VICTORY_LATE_WAIT"), None);
        // F1's old console and frame generation, and a switch left from an
        // earlier start, do not survive; the F1 menu's own lines do.
        let file = apply("VICTORY_MENU_CONSOLE=series-x
VICTORY_MENU_FRAMEGEN=5
VICTORY_LATE_WAIT=0
VICTORY_FULLSCREEN=1
", &p);
        assert!(!file.contains("VICTORY_MENU_CONSOLE") && !file.contains("VICTORY_MENU_FRAMEGEN"));
        assert!(!file.contains("VICTORY_LATE_WAIT"), "{file}");
        assert!(file.contains("VICTORY_FULLSCREEN=1"), "the F1 menu's lines stay");
        // Each switch once.
        let mut names: Vec<&String> = p.set.iter().map(|(k, _)| k).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), p.set.len());
        assert!(p.unset.contains(&RAY_TRACING.to_owned()));
    }

    #[test]
    fn strong_cards_get_series_x_and_frame_generation_stays_ready() {
        let p = plan(Graphics::Auto, &pc("NVIDIA GeForce RTX 5070", 11.9, 16));
        assert_eq!(p.graphics, Graphics::Quality);
        assert!(!p.args.contains(&"-SeriesS".to_owned()));
        assert!(p.args.contains(&"-FrameGen".to_owned()));
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
        let file = apply("# Saved by the F1 menu\r\nVICTORY_FPS_LINE=0\r\nVICTORY_AMBIENT_SPECULAR=\r\n", &ultra);
        assert!(file.contains("VICTORY_FPS_LINE=0\r\n") && file.contains("VICTORY_BVH_BUILDER=1"));
        assert!(!file.contains("VICTORY_AMBIENT_SPECULAR"), "not the F1 menu's any more");
        let back = apply(&file, &plan(Graphics::Quality, &hw));
        assert!(!back.contains("VICTORY_BVH_BUILDER") && back.contains("VICTORY_FPS_LINE=0"));
        assert_eq!(back.matches("VICTORY_THIN_THREADS").count(), 1);
    }

    #[test]
    fn mfg_unlocker_is_never_used() {
        let p = plan(Graphics::Auto, &pc("NVIDIA GeForce RTX 3060", 11.8, 12));
        assert!(!p.args.contains(&"-FrameGenX".to_owned()));
        assert!(p.unset.iter().any(|k| k == "VICTORY_MFG_UNLOCKER_DLL"));
        assert!(p.unset.iter().any(|k| k == "VICTORY_MFG_UNLOCKER"));
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
