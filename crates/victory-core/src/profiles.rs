//! Run profiles: named sets of the layer's `VICTORY_*` switches, in one
//! place for the command line (`victory run --profile`), the desktop GUI
//! and `scripts/run-fast.ps1`.
//!
//! Every switch here is read by the layer (each is an environment variable
//! some module reads once, at first use). The values are the ones upstream
//! (fxceless/victory-fork) measured its best runs with, as its
//! `README.md` and `diagnostics/README.md` of `e4245f3` report them. Nothing
//! here was measured in a title by this fork.

/// One switch: its variable, its value in a profile, and what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Switch {
    pub var: &'static str,
    pub value: &'static str,
    /// What it does and what upstream measured, in a line.
    pub note: &'static str,
    /// Linked by upstream to runs that lost the card, or not yet run in a
    /// title: only in the experimental profile.
    pub experimental: bool,
}

const fn s(var: &'static str, value: &'static str, note: &'static str) -> Switch {
    Switch { var, value, note, experimental: false }
}

const fn x(var: &'static str, value: &'static str, note: &'static str) -> Switch {
    Switch { var, value, note, experimental: true }
}

/// The switches of the performance profile.
pub const PERFORMANCE: &[Switch] = &[
    s("VICTORY_TEXTURE_BACK", "1", "Textures the card writes are copied back where the title reads them. Correctness: without it water is wrong and boats sink."),
    s("VICTORY_HEAP_WORKER", "1", "The texture pass on a thread of its own: world frames 6.2-6.7 -> 8.65-9.5 a second (upstream)."),
    s("VICTORY_LOOKUP_CACHE", "1", "A cache for finding a buffer's host copy: about 14 ms a frame (upstream run 293)."),
    s("VICTORY_PARALLEL_REFRESH", "1", "Texture checks on the helper threads: 8.7-9.4 ms less preparing a frame (upstream)."),
    s("VICTORY_SETTLE_ROOM", "1", "More room for submissions in flight: about 11.5-20 ms less preparing a frame (upstream)."),
    s("VICTORY_LEVELS_ON_DEMAND", "1", "Texture levels get memory when used."),
    s("VICTORY_ASYNC_SUBMIT", "1", "Submissions go to the card from a thread of their own."),
    s("VICTORY_QUEUE2", "1", "Copy queues beside the direct queue for uploads and fetches."),
    s("VICTORY_TEXTURE_NO_WAIT", "1", "Draws do not wait for texture uploads still on their way."),
    s("VICTORY_MERGE_GAP", "64", "Copies of title memory join across gaps of up to 64 bytes."),
    s("VICTORY_MERGE_RUNS", "4096", "Up to 4,096 runs merged a copy."),
    s("VICTORY_STAGING_POOL", "0", "No staging pool (upstream's measured setting)."),
    s("VICTORY_COPY_PLAIN", "1", "Title memory copied with plain loads where possible, not through the kernel."),
    s("VICTORY_JOIN_PARTS", "1", "Parts of a submission joined into one."),
    s("VICTORY_BACK_OWN_VIEW", "1", "Read-backs through a view of their own."),
    s("VICTORY_CARD_REHASH_RUNS", "1", "Card-side copies compared by runs."),
    s("VICTORY_CARD_WINDOWS_MB", "1024", "Card memory for copies of title memory. Upstream: 2048 left textures black when the card ran out of room (run 295); 1024 is its middle setting."),
    s("VICTORY_CARD_BUFFERS_MB", "1280", "Card memory for copies of title buffers."),
    s("VICTORY_CARD_BUFFERS_MIN_KB", "64", "Only buffers of 64 KiB or more get a card copy."),
];

/// What the experimental profile adds to the performance one.
pub const EXPERIMENTAL: &[Switch] = &[
    x("VICTORY_INDIRECT_WINDOW", "1", "Indirect draws clamped a window at a time: 0.33 us a draw against 7.3 in this fork's bench. In upstream's runs that lost the card."),
    x("VICTORY_INDIRECT_GATHER", "1", "Scattered indirect records clamped in one pass per list. Needs windows; in the same runs."),
];

/// A run profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// The layer's defaults: every switch off. Slowest, best understood.
    Stable,
    /// Upstream's measured set without the switches tied to lost cards.
    Performance,
    /// Performance plus indirect windows and gathers.
    Experimental,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Profile::Stable, Profile::Performance, Profile::Experimental];

    pub fn name(self) -> &'static str {
        match self {
            Profile::Stable => "stable",
            Profile::Performance => "performance",
            Profile::Experimental => "experimental",
        }
    }

    pub fn parse(name: &str) -> Option<Profile> {
        Profile::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(name))
    }

    /// The switches the profile sets, in order.
    pub fn switches(self) -> Vec<Switch> {
        match self {
            Profile::Stable => Vec::new(),
            Profile::Performance => PERFORMANCE.to_vec(),
            Profile::Experimental => PERFORMANCE.iter().chain(EXPERIMENTAL).copied().collect(),
        }
    }
}

/// Every switch any profile sets.
pub fn all_switches() -> impl Iterator<Item = &'static Switch> {
    PERFORMANCE.iter().chain(EXPERIMENTAL)
}

/// Sets the profile's switches in this process's environment, leaving any
/// switch the user set already as it is. Returns the ones set. Must run
/// before the layer reads them (each is read once, at first use).
pub fn apply(profile: Profile) -> Vec<Switch> {
    let mut set = Vec::new();
    for sw in profile.switches() {
        if std::env::var_os(sw.var).is_none() {
            // SAFETY: called at the start of `main`, before any thread of
            // ours or the title's is started.
            unsafe { std::env::set_var(sw.var, sw.value) };
            set.push(sw);
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_have_unique_switches_and_parse_by_name() {
        let mut vars: Vec<&str> = all_switches().map(|s| s.var).collect();
        let n = vars.len();
        vars.sort_unstable();
        vars.dedup();
        assert_eq!(vars.len(), n, "a switch is listed twice");
        assert!(vars.iter().all(|v| v.starts_with("VICTORY_")));
        for p in Profile::ALL {
            assert_eq!(Profile::parse(p.name()), Some(p));
        }
        assert!(Profile::Stable.switches().is_empty());
        assert!(Profile::Performance.switches().iter().all(|s| !s.experimental));
        assert!(Profile::Experimental.switches().iter().any(|s| s.experimental));
    }

    /// Every switch a profile sets is read somewhere in the layer's source:
    /// a renamed or removed switch fails here instead of doing nothing.
    #[test]
    fn every_switch_is_read_by_the_layer() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut sources = String::new();
        for dir in ["victory-gpu/src", "victory-hle/src", "victory-shader/src", "victory-loader/src"] {
            for entry in walk(&root.join(dir)) {
                sources.push_str(&std::fs::read_to_string(entry).unwrap_or_default());
            }
        }
        for sw in all_switches() {
            assert!(sources.contains(&format!("\"{}\"", sw.var)), "{} is not read anywhere", sw.var);
        }
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
        out
    }
}
