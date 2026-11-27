//! Run profiles: named sets of the layer's `VICTORY_*` switches, for the
//! desktop GUI (`apps/victory-gui`).
//!
//! The sets are `scripts/play.ps1`'s, the settings this fork's measured runs
//! were made with: "play" is the script's default, "safe" its `-Safe`, and
//! "ray tracing" its `-RayTracing` on top of the default. Every switch is read
//! by the layer once, at first use; each is described where it is read.

/// One switch: its variable, its value in a profile, and what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Switch {
    pub var: &'static str,
    pub value: &'static str,
    /// What it does, in a line.
    pub note: &'static str,
    /// Costs frame rate or is not part of the default set: shown apart.
    pub experimental: bool,
}

const fn s(var: &'static str, value: &'static str, note: &'static str) -> Switch {
    Switch { var, value, note, experimental: false }
}

const fn x(var: &'static str, value: &'static str, note: &'static str) -> Switch {
    Switch { var, value, note, experimental: true }
}

/// What every profile but "stable" sets (`play.ps1`'s `$settings`).
pub const BASE: &[Switch] = &[
    s("VICTORY_TEXTURE_NO_WAIT", "1", "Texture refreshes do not wait for title memory to hold still."),
    s("VICTORY_RELAY", "2", "What shaders wrote comes back on a thread of its own, beside the submitting one."),
    s("VICTORY_WIDE_BACK", "1", "Wide read-backs of what shaders wrote."),
    s("VICTORY_CARD_REHASH_RUNS", "1", "Card-side copies compared by runs."),
    s("VICTORY_JOIN_COPIES", "1", "Copies of title memory that continue each other become one."),
    s("VICTORY_INDIRECT_WINDOW", "1", "Indirect draws read their records from a window, not a pass each."),
    s("VICTORY_RELAY_COPY", "1", "The relay's copies on the copy queue."),
    s("VICTORY_CARD_STEPS", "2", "Buffer fills that need bytes still on the card are done on the card."),
    s("VICTORY_INDIRECT_BATCH", "2", "Indirect records checked in batches."),
    s("VICTORY_MAP_THREADS", "1", "Memory mapping spread over threads."),
    s("VICTORY_TITLE_PRIORITIES", "0", "The title's thread priorities are left as Windows sets them."),
    s("VICTORY_CARD_AWAKE", "1", "The card is kept from idling down between frames."),
    s("VICTORY_SHOW_AFTER", "1", "A slot shows a texture only after its upload ran (ended a device loss every ~15 minutes)."),
    s("VICTORY_AMBIENT", "0.8", "The ambient light stand-in's strength (rooms not too dark without ray tracing)."),
    s("VICTORY_D3D12_DRED", "light", "The driver records what the card was doing, for a device loss."),
];

/// What "play" adds (`play.ps1` without `-Safe`): the newer, faster ways.
pub const PLAY: &[Switch] = &[
    s("VICTORY_THIN", "1", "List calls are written down on the title's threads and translated by eight of ours."),
    s("VICTORY_JOIN_PARTS", "8", "Up to eight parts of a submission joined into one."),
    s("VICTORY_UPLOAD_AHEAD", "1", "Uploads are prepared ahead of the submission that needs them."),
    // Not in play.ps1: on here so that Warm Pipelines (`victory warm --used`) has
    // the pipelines to make. On the RTX 3060 the driver took 5.6 s for each slow
    // one and ~1,700 were still queued after 5 minutes of play (two runs,
    // 2026-11-26): made in the title they cost the world its frames.
    s("VICTORY_PIPELINE_BOOK", "1", "The pipelines the title creates are written down (in the cache folder), for Warm Pipelines to make ahead of a run."),
];

/// What "safe" adds (`play.ps1 -Safe`): the newer ways left out.
pub const SAFE: &[Switch] = &[
    s("VICTORY_UNDER_WIDE", "0", "No reads under wide write-backs."),
    s("VICTORY_LAND_ECHO", "0", "No echo of landings."),
];

/// What "fast" adds to "play", with the Series S console profile: more of
/// the title's buffers kept in the card's own memory, where shaders read them
/// without crossing the bus and the card compares them itself. Measured by
/// Maykzera/victory-rtx3060 on an RTX 5070 in Series S at half scale: 11.0 to
/// 21.8 fps from these two alone; 14-17 fps on an RTX 3060 with a Ryzen 5
/// 5600X with their whole set. Not yet measured here.
pub const FAST: &[Switch] = &[
    x("VICTORY_CARD_BUFFERS_MIN_MB", "1", "Title buffers from 1 MB up are copied to the card's memory (16 MB up by default on cards up to 12 GB)."),
    x("VICTORY_CARD_BUFFERS_MB", "1536", "Up to 1.5 GB of them (512 MB by default on cards up to 12 GB)."),
];

/// What "ray tracing" adds to "play" (`play.ps1 -RayTracing`).
pub const RAY_TRACING: &[Switch] = &[x(
    "VICTORY_BVH_BUILDER",
    "1",
    "The title's own ray-traced indirect light: lit interiors, about a third slower (README).",
)];

/// A run profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// The layer's defaults: every switch off.
    Stable,
    /// `play.ps1 -Safe`: the first thing to try when something is wrong.
    Safe,
    /// `play.ps1`: the set the fork's measurements are of.
    Play,
    /// `play.ps1 -RayTracing`: the title's ray-traced light on.
    RayTracing,
    /// Play in the Series S profile, with more buffers on the card ([`FAST`]).
    Fast,
}

impl Profile {
    pub const ALL: [Profile; 5] = [Profile::Play, Profile::Fast, Profile::RayTracing, Profile::Safe, Profile::Stable];

    pub fn name(self) -> &'static str {
        match self {
            Profile::Stable => "stable",
            Profile::Safe => "safe",
            Profile::Play => "play",
            Profile::RayTracing => "raytracing",
            Profile::Fast => "fast",
        }
    }

    /// The console model the title is told it runs on (`victory run
    /// --console`); `None`: the default, Series X.
    pub fn console(self) -> Option<&'static str> {
        match self {
            Profile::Fast => Some("series-s"),
            _ => None,
        }
    }

    pub fn parse(name: &str) -> Option<Profile> {
        Profile::ALL.into_iter().find(|p| p.name().eq_ignore_ascii_case(name))
    }

    /// The switches the profile sets, in order.
    pub fn switches(self) -> Vec<Switch> {
        let parts: &[&[Switch]] = match self {
            Profile::Stable => &[],
            Profile::Safe => &[BASE, SAFE],
            Profile::Play => &[BASE, PLAY],
            Profile::RayTracing => &[BASE, PLAY, RAY_TRACING],
            Profile::Fast => &[BASE, PLAY, FAST],
        };
        parts.iter().flat_map(|p| p.iter().copied()).collect()
    }

    /// The environment a launch gets: the profile's switches, then the
    /// user's overrides over them (`None`: left to the profile; an empty
    /// value: the switch off, passed as "0").
    pub fn env(self, overrides: &std::collections::BTreeMap<String, String>) -> Vec<(String, String)> {
        let mut env: std::collections::BTreeMap<String, String> =
            self.switches().into_iter().map(|s| (s.var.to_owned(), s.value.to_owned())).collect();
        for (k, v) in overrides {
            env.insert(k.clone(), if v.is_empty() { "0".into() } else { v.clone() });
        }
        env.into_iter().collect()
    }
}

/// Every switch any profile sets, each once.
pub fn all_switches() -> impl Iterator<Item = &'static Switch> {
    BASE.iter().chain(PLAY).chain(SAFE).chain(FAST).chain(RAY_TRACING)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_unknown_names_are_refused() {
        for p in Profile::ALL {
            assert_eq!(Profile::parse(p.name()), Some(p));
        }
        assert_eq!(Profile::parse("PLAY"), Some(Profile::Play));
        assert_eq!(Profile::parse("performance"), None);
    }

    /// The sets are play.ps1's, so a switch must not appear twice.
    #[test]
    fn no_switch_is_listed_twice() {
        let mut seen = std::collections::BTreeSet::new();
        for sw in all_switches() {
            assert!(seen.insert(sw.var), "{} listed twice", sw.var);
        }
    }

    #[test]
    fn profiles_build_on_each_other_as_the_script_does() {
        assert!(Profile::Stable.switches().is_empty());
        let play = Profile::Play.switches();
        assert_eq!(play.len(), BASE.len() + PLAY.len());
        let rt = Profile::RayTracing.switches();
        assert_eq!(&rt[..play.len()], &play[..]);
        assert_eq!(rt.last().unwrap().var, "VICTORY_BVH_BUILDER");
        assert!(Profile::Safe.switches().iter().all(|s| !PLAY.contains(s)));
        let fast = Profile::Fast.switches();
        assert_eq!(&fast[..play.len()], &play[..]);
        assert_eq!(Profile::Fast.console(), Some("series-s"));
        assert_eq!(Profile::Play.console(), None);
    }

    #[test]
    fn overrides_win_and_an_empty_override_is_off() {
        let mut o = std::collections::BTreeMap::new();
        o.insert("VICTORY_THIN".to_owned(), String::new());
        o.insert("VICTORY_PANEL".to_owned(), "1".to_owned());
        let env = Profile::Play.env(&o);
        let get = |k: &str| env.iter().find(|(v, _)| v == k).map(|(_, x)| x.as_str());
        assert_eq!(get("VICTORY_THIN"), Some("0"));
        assert_eq!(get("VICTORY_PANEL"), Some("1"));
        assert_eq!(get("VICTORY_RELAY"), Some("2"));
    }
}
