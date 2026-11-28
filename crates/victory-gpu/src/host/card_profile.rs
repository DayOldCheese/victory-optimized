//! Where the card's time goes, by the title's pipelines, in a release run
//! with late wait on (`VICTORY_CARD_PROFILE=1`; off unless asked).
//!
//! Why: in a world frame on an RTX 3060 (run 2026-11-28 14:32) the title's
//! submissions took 75.7 ms of the card from its direct queue and 18.5 from
//! its compute queues (`card_time`), in a frame of 112: a Series S frame at
//! 540 lines, on a card about three times that console's. What of it is
//! which pass was not known: `VICTORY_GPU_TIMING` (diagnostics builds) times
//! every list alone with late wait off, which is not the run being measured.
//!
//! A list writes a timestamp where its pipeline changes ([`HostList::profile_mark`],
//! called where the host pipeline is set) and one before it is closed; the
//! time from one stamp to the next goes to the pipeline set at the first.
//! Stamps go to blocks of a ring ([`PER`] each, a list takes another when
//! one is full), resolved into memory the CPU reads; a block is added to the
//! sums when its stamps are all in (at the frame line, or when the ring
//! comes round to it). Work of one pipeline overlaps the next on the card,
//! so an interval is the time between the two pipelines' starts as the card
//! saw them: the sums say where the card is, not what each draw costs alone.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// Stamps of a block; the last is kept for the stamp that ends it.
const PER: u32 = 512;
const BLOCKS: u32 = 512;

pub(crate) fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        let on = std::env::var_os("VICTORY_CARD_PROFILE").is_some_and(|v| v == "1");
        if on {
            tracing::info!("the card's time is summed by the title's pipelines (VICTORY_CARD_PROFILE=1)");
        }
        on
    })
}

/// A list's open block: its index, and the pipeline of each interval begun.
#[derive(Default)]
pub(crate) struct Open {
    block: Option<u32>,
    keys: Vec<u64>,
}

struct Ring {
    heap: ID3D12QueryHeap,
    back: ID3D12Resource,
    data: *mut u64,
    next: u32,
    /// The intervals' pipelines of each block handed in (empty: none).
    blocks: Vec<Vec<u64>>,
    /// Ticks and intervals by pipeline since the last frame line.
    sums: HashMap<u64, (u64, u64)>,
    /// What each pipeline is called.
    names: HashMap<u64, String>,
    /// Blocks whose list ran out of blocks of its own, and blocks taken while
    /// their stamps were not all in.
    lost: [u64; 2],
}

// SAFETY: only touched under its lock; the mapping lives as long as the
// buffer, which the ring holds.
unsafe impl Send for Ring {}

static RING: Mutex<Option<Ring>> = Mutex::new(None);

fn make(device: &ID3D12Device) -> Option<Ring> {
    let count = PER * BLOCKS;
    let mut heap: Option<ID3D12QueryHeap> = None;
    let mut back: Option<ID3D12Resource> = None;
    let readback = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: u64::from(count) * 8,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    // SAFETY: plain object creation and a mapping of a read-back buffer.
    unsafe {
        device
            .CreateQueryHeap(&D3D12_QUERY_HEAP_DESC { Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP, Count: count, NodeMask: 0 }, &mut heap)
            .ok()?;
        device.CreateCommittedResource(&readback, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut back).ok()?;
        let back = back?;
        let mut data = std::ptr::null_mut();
        back.Map(0, Some(&D3D12_RANGE { Begin: 0, End: count as usize * 8 }), Some(&mut data)).ok()?;
        std::ptr::write_bytes(data.cast::<u64>(), 0, count as usize);
        Some(Ring {
            heap: heap?,
            back,
            data: data.cast(),
            next: 0,
            blocks: vec![Vec::new(); BLOCKS as usize],
            sums: HashMap::new(),
            names: HashMap::new(),
            lost: [0; 2],
        })
    }
}

impl Ring {
    fn stamp(&self, q: u32) -> u64 {
        // SAFETY: a word of the mapped buffer, in it.
        unsafe { self.data.add(q as usize).read_volatile() }
    }

    /// Adds block `b` to the sums if its stamps are all in (`force`: and
    /// lets it go either way), and clears it.
    fn take(&mut self, b: u32, force: bool) {
        let keys = &self.blocks[b as usize];
        if keys.is_empty() {
            return;
        }
        let first = b * PER;
        let n = keys.len() as u32;
        let all_in = (0..=n).all(|i| self.stamp(first + i) != 0);
        if !all_in && !force {
            return;
        }
        if all_in {
            for (i, &key) in keys.iter().enumerate() {
                let (a, z) = (self.stamp(first + i as u32), self.stamp(first + i as u32 + 1));
                let sum = self.sums.entry(key).or_default();
                sum.0 += z.saturating_sub(a);
                sum.1 += 1;
            }
        } else {
            self.lost[1] += 1;
        }
        // SAFETY: words of the mapped buffer, in it.
        unsafe { std::ptr::write_bytes(self.data.add(first as usize), 0, n as usize + 1) };
        self.blocks[b as usize].clear();
    }

    /// A block for a list: the next of the ring, after what it held is taken.
    fn block(&mut self) -> u32 {
        let b = self.next;
        self.next = (b + 1) % BLOCKS;
        self.take(b, true);
        b
    }

    /// Ends block `b` after `open.keys` intervals: its last stamp and the
    /// resolve, recorded into `list`, and the block handed in.
    fn end(&mut self, list: &ID3D12GraphicsCommandList, b: u32, keys: Vec<u64>) {
        let first = b * PER;
        let n = keys.len() as u32;
        // SAFETY: an open list, our heap and read-back buffer, in range.
        unsafe {
            list.EndQuery(&self.heap, D3D12_QUERY_TYPE_TIMESTAMP, first + n);
            list.ResolveQueryData(&self.heap, D3D12_QUERY_TYPE_TIMESTAMP, first, n + 1, &self.back, u64::from(first) * 8);
        }
        self.blocks[b as usize] = keys;
    }
}

/// The pipeline `key` (named `name`) is set in `list` from here on.
pub(crate) fn mark(device: &ID3D12Device, list: &ID3D12GraphicsCommandList, open: &mut Open, key: u64, name: &str) {
    let mut guard = RING.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = make(device);
    }
    let Some(ring) = guard.as_mut() else { return };
    ring.names.entry(key).or_insert_with(|| name.to_owned());
    if let Some(b) = open.block
        && open.keys.len() as u32 + 1 >= PER
    {
        // Full: ended here, and the list goes on in another.
        let keys = std::mem::take(&mut open.keys);
        ring.end(list, b, keys);
        open.block = None;
        ring.lost[0] += 1;
    }
    let b = match open.block {
        Some(b) => b,
        None => {
            let b = ring.block();
            open.block = Some(b);
            b
        }
    };
    let q = b * PER + open.keys.len() as u32;
    // SAFETY: an open list and our heap, in range.
    unsafe { list.EndQuery(&ring.heap, D3D12_QUERY_TYPE_TIMESTAMP, q) };
    open.keys.push(key);
}

/// Before `list` is closed: its open block ended.
pub(crate) fn close(list: &ID3D12GraphicsCommandList, open: &mut Open) {
    let Some(b) = open.block.take() else { return };
    let keys = std::mem::take(&mut open.keys);
    let mut guard = RING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ring) = guard.as_mut() {
        ring.end(list, b, keys);
    }
}

/// A list reset without being closed: its open block is given up.
pub(crate) fn forget(open: &mut Open) {
    open.block = None;
    open.keys.clear();
}

/// For the frame line: the card's milliseconds a frame by pipeline name, the
/// most first, `most` of them, with the intervals a frame; and the sum of all.
pub(crate) fn line(frames: f64, frequency: u64) {
    if !on() || frames <= 0.0 {
        return;
    }
    let mut guard = RING.lock().unwrap_or_else(|e| e.into_inner());
    let Some(ring) = guard.as_mut() else { return };
    for b in 0..BLOCKS {
        ring.take(b, false);
    }
    let sums = std::mem::take(&mut ring.sums);
    let mut by_name: HashMap<&str, (u64, u64)> = HashMap::new();
    for (key, (ticks, n)) in &sums {
        let name = ring.names.get(key).map_or("?", String::as_str);
        let e = by_name.entry(name).or_default();
        e.0 += ticks;
        e.1 += n;
    }
    let ms = |ticks: u64| ticks as f64 * 1e3 / frequency.max(1) as f64 / frames;
    let all: u64 = by_name.values().map(|v| v.0).sum();
    let intervals: u64 = by_name.values().map(|v| v.1).sum();
    let mut top: Vec<(&str, (u64, u64))> = by_name.into_iter().collect();
    top.sort_unstable_by_key(|t| std::cmp::Reverse(t.1.0));
    let shown: Vec<String> = top
        .iter()
        .take(16)
        .map(|(name, (ticks, n))| {
            let name: String = name.chars().take(90).collect();
            format!("{name} {:.2} ms in {:.0}", ms(*ticks), *n as f64 / frames)
        })
        .collect();
    let lost = std::mem::take(&mut ring.lost);
    tracing::info!(
        "the card's time by the title's pipelines per frame (VICTORY_CARD_PROFILE): {:.1} ms in {:.0} intervals of {} pipelines; \
         the most: {}; ({} lists went on in another block, {} blocks let go before their stamps were in)",
        ms(all),
        intervals as f64 / frames,
        top.len(),
        shown.join(", "),
        lost[0],
        lost[1],
    );
}
