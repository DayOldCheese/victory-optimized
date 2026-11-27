//! Where the time inside a list call goes (diagnostics only): named spans
//! of the code that translates draws, each with the clock time it took and
//! the share of that the thread actually ran (the rest it waited on a lock,
//! the card, another thread, or was not scheduled). Spans may nest; each
//! counts all of its own time.
//!
//! Why: on an RTX 3060 with a Ryzen 5 5600X an indirect execution took 250
//! us and a mesh dispatch 221 us to translate, against 17-29 us for a draw,
//! and the translating threads ran 23% of their time. These say which part
//! waits.
//!
//! Without the `diagnostics` feature a span is nothing and costs nothing.

/// The spans, by number.
#[derive(Clone, Copy)]
#[repr(u8)]
pub(crate) enum Id {
    IndirectArguments,
    IndirectPrepare,
    IndirectRecord,
    IndirectAfter,
    MeshPrepare,
    MeshRecord,
    InputBuffers,
    EarlyTextures,
    Heaps,
    Stages,
    AfterStages,
}

#[cfg(feature = "diagnostics")]
const NAMES: [&str; 11] = [
    "indirect: arguments refreshed and clamped",
    "indirect: prepared",
    "indirect: recorded",
    "indirect: after",
    "mesh dispatch: prepared",
    "mesh dispatch: recorded",
    "vertex and index buffers refreshed",
    "textures picked by index (early_textures)",
    "heaps and root signature",
    "the stages' user data, tables and views",
    "after the stages",
];

#[cfg(feature = "diagnostics")]
mod on {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    pub(super) const N: usize = super::NAMES.len();
    pub(super) static COUNT: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];
    pub(super) static WALL: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];
    pub(super) static RAN: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn QueryThreadCycleTime(thread: *mut std::ffi::c_void, cycles: *mut u64) -> i32;
        fn GetCurrentThread() -> *mut std::ffi::c_void;
    }

    #[inline]
    pub(super) fn ran() -> u64 {
        let mut c = 0u64;
        // SAFETY: the pseudo handle of this thread and a valid out pointer.
        unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut c) };
        c
    }

    #[inline]
    pub(super) fn now() -> u64 {
        // SAFETY: x86_64 always has the time-stamp counter.
        unsafe { std::arch::x86_64::_rdtsc() }
    }

    /// Time-stamp cycles per microsecond, from the clock since the first use.
    pub(super) fn per_us() -> f64 {
        static START: OnceLock<(std::time::Instant, u64)> = OnceLock::new();
        let (t, c) = *START.get_or_init(|| (std::time::Instant::now(), now()));
        let us = t.elapsed().as_secs_f64() * 1e6;
        if us < 1e5 { 0.0 } else { now().wrapping_sub(c) as f64 / us }
    }

    pub(super) fn add(id: usize, wall: u64, ran: u64) {
        COUNT[id].fetch_add(1, Relaxed);
        WALL[id].fetch_add(wall, Relaxed);
        RAN[id].fetch_add(ran, Relaxed);
    }
}

/// A span being timed: its time is counted when it is dropped.
#[cfg(feature = "diagnostics")]
pub(crate) struct Span(u8, u64, u64);

#[cfg(feature = "diagnostics")]
impl Drop for Span {
    fn drop(&mut self) {
        on::add(usize::from(self.0), on::now().wrapping_sub(self.1), on::ran().wrapping_sub(self.2));
    }
}

#[cfg(not(feature = "diagnostics"))]
pub(crate) struct Span;

// (So that a span may be ended early with `drop` in either build.)
#[cfg(not(feature = "diagnostics"))]
impl Drop for Span {
    #[inline]
    fn drop(&mut self) {}
}

/// Starts timing `id` until the value is dropped.
#[inline]
#[must_use]
pub(crate) fn span(id: Id) -> Span {
    #[cfg(feature = "diagnostics")]
    {
        // Calibrate early.
        let _ = on::per_us;
        Span(id as u8, on::now(), on::ran())
    }
    #[cfg(not(feature = "diagnostics"))]
    {
        let _ = id;
        Span
    }
}

/// For the frame-rate line: the spans of the last stretch, per frame.
pub(crate) fn line(frames: f64) {
    #[cfg(feature = "diagnostics")]
    {
        use std::sync::atomic::Ordering::Relaxed;
        let per_us = on::per_us();
        if per_us <= 0.0 || frames <= 0.0 {
            return;
        }
        let parts: Vec<String> = (0..on::N)
            .filter_map(|i| {
                let n = on::COUNT[i].swap(0, Relaxed);
                let wall = on::WALL[i].swap(0, Relaxed) as f64;
                let ran = on::RAN[i].swap(0, Relaxed) as f64;
                (n > 0).then(|| {
                    format!(
                        "{} {:.0}x {:.1} ms ({:.1} us each, ran {:.0}%)",
                        NAMES[i],
                        n as f64 / frames,
                        wall / per_us / frames / 1000.0,
                        wall / per_us / n as f64,
                        if wall > 0.0 { 100.0 * ran / wall } else { 0.0 }
                    )
                })
            })
            .collect();
        if !parts.is_empty() {
            tracing::info!("draw translation spans per frame: {}", parts.join("; "));
        }
    }
    #[cfg(not(feature = "diagnostics"))]
    let _ = frames;
}
