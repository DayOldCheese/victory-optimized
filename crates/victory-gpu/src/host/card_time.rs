//! How long our own lists of the copies in the card's memory take on the card:
//! the sends of title bytes to the copies with their hash passes (`SEND`), and
//! the compares of the ranges a submission's shaders may write (`COMPARE`).
//!
//! Why: in a heavy world frame on an RTX 3060 (2026-11-28 12:29) the submitting
//! thread spent 39.6 ms of 80 waiting for flights, the card was busy 76-82% of
//! the frame, and of that busy time an unknown part was ours: 99 hash and 359
//! compare passes a frame. Whether moving them to a compute queue of their own,
//! or joining them, is worth it depends on that part, which nothing measured:
//! `VICTORY_GPU_TIMING` times every list but turns late wait off.
//!
//! Each such list writes a timestamp when it begins and one when it ends, into a
//! pair of a ring, resolved into memory the CPU reads. A pair is added to its
//! kind's sum when the ring comes round to it again (1,024 pairs: some 25
//! frames after it was written, long after its list ran), and at the frame line
//! for those whose both stamps are in. Two timestamps and one resolve a list:
//! on unless `VICTORY_CARD_TIME=0`.
//!
//! The title's submissions too ([`around`]): each between two lists of ours
//! that write one stamp each, recorded once per stamp and sent again and
//! again, by the kind of the title queue it came from (`TITLE_DIRECT`,
//! `TITLE_COMPUTE`, `TITLE_COPY`). How long the title's compute work is on the
//! card is what a host compute queue beside the direct one could overlap
//! with its drawing (`two_queues`, `VICTORY_ASYNC_QUEUE=plan`); nothing
//! measured it in a run with late wait on.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// The sends with their hash passes.
pub(super) const SEND: usize = 0;
/// The compares after a submission's lists.
pub(super) const COMPARE: usize = 1;
/// The title's submissions, by its queue's list type.
pub(super) const TITLE_DIRECT: usize = 2;
pub(super) const TITLE_COMPUTE: usize = 3;
pub(super) const TITLE_COPY: usize = 4;
const KINDS: usize = 5;

const PAIRS: u32 = 1024;

struct Ring {
    heap: ID3D12QueryHeap,
    /// Read-back memory, mapped for the life of the process.
    back: ID3D12Resource,
    data: *mut u64,
    next: u32,
    /// The kind of each pair, `u8::MAX` while unused.
    kinds: Vec<u8>,
    /// For [`around`]: the allocator its stamp lists were recorded with (never
    /// reset), and the list of each stamp, recorded when first wanted.
    allocator: Option<ID3D12CommandAllocator>,
    stamps: Vec<Option<ID3D12CommandList>>,
}

// SAFETY: the ring is only touched under its lock; the mapping lives as long
// as the buffer, which the ring holds.
unsafe impl Send for Ring {}

static RING: Mutex<Option<Ring>> = Mutex::new(None);
/// Ticks and lists of each kind since the last frame line.
static TICKS: [AtomicU64; KINDS] = [const { AtomicU64::new(0) }; KINDS];
static LISTS: [AtomicU64; KINDS] = [const { AtomicU64::new(0) }; KINDS];

fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_CARD_TIME").is_none_or(|v| v != "0"))
}

fn make(device: &ID3D12Device) -> Option<Ring> {
    let mut heap: Option<ID3D12QueryHeap> = None;
    let mut back: Option<ID3D12Resource> = None;
    let readback = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: u64::from(PAIRS) * 16,
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
            .CreateQueryHeap(&D3D12_QUERY_HEAP_DESC { Type: D3D12_QUERY_HEAP_TYPE_TIMESTAMP, Count: PAIRS * 2, NodeMask: 0 }, &mut heap)
            .ok()?;
        device.CreateCommittedResource(&readback, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut back).ok()?;
        let back = back?;
        let mut data = std::ptr::null_mut();
        back.Map(0, Some(&D3D12_RANGE { Begin: 0, End: PAIRS as usize * 16 }), Some(&mut data)).ok()?;
        std::ptr::write_bytes(data.cast::<u64>(), 0, PAIRS as usize * 2);
        Some(Ring { heap: heap?, back, data: data.cast(), next: 0, kinds: vec![u8::MAX; PAIRS as usize], allocator: None, stamps: vec![None; PAIRS as usize * 2] })
    }
}

/// Adds pair `i` to its kind's sum if both its stamps are in, and clears it.
fn take(ring: &mut Ring, i: usize) {
    let kind = ring.kinds[i];
    if kind == u8::MAX {
        return;
    }
    // SAFETY: two words of the mapped buffer, in it.
    let (begin, end) = unsafe { (ring.data.add(i * 2).read_volatile(), ring.data.add(i * 2 + 1).read_volatile()) };
    if begin == 0 || end == 0 {
        return;
    }
    TICKS[usize::from(kind)].fetch_add(end.saturating_sub(begin), Ordering::Relaxed);
    LISTS[usize::from(kind)].fetch_add(1, Ordering::Relaxed);
    // SAFETY: as above.
    unsafe {
        ring.data.add(i * 2).write_volatile(0);
        ring.data.add(i * 2 + 1).write_volatile(0);
    }
    ring.kinds[i] = u8::MAX;
}

/// At the start of one of our lists of `kind`: its pair, to give to [`end`].
pub(super) fn begin(device: &ID3D12Device, list: &ID3D12GraphicsCommandList, kind: usize) -> Option<u32> {
    if !on() {
        return None;
    }
    let mut guard = RING.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = make(device);
    }
    let ring = guard.as_mut()?;
    let i = next_pair(ring, kind);
    // SAFETY: an open list and our query heap.
    unsafe { list.EndQuery(&ring.heap, D3D12_QUERY_TYPE_TIMESTAMP, i * 2) };
    Some(i)
}

/// The ring's next pair, for `kind`.
fn next_pair(ring: &mut Ring, kind: usize) -> u32 {
    let i = ring.next;
    ring.next = (i + 1) % PAIRS;
    // What the pair held from the ring's last round: its lists ran long ago.
    take(ring, i as usize);
    // SAFETY: two words of the mapped buffer; the pair is ours until the
    // ring comes round.
    unsafe {
        ring.data.add(i as usize * 2).write_volatile(0);
        ring.data.add(i as usize * 2 + 1).write_volatile(0);
    }
    ring.kinds[i as usize] = kind as u8;
    i
}

/// The closed list that writes stamp `q` and resolves it, recorded the
/// first time it is wanted: the same commands each time it is sent.
fn stamp_list(device: &ID3D12Device, ring: &mut Ring, q: u32) -> Option<ID3D12CommandList> {
    if let Some(list) = &ring.stamps[q as usize] {
        return Some(list.clone());
    }
    // SAFETY: plain object creation; a list recorded and closed here, with
    // our query heap and read-back buffer, in range.
    let list = unsafe {
        if ring.allocator.is_none() {
            ring.allocator = device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT).ok();
        }
        let allocator = ring.allocator.as_ref()?;
        let list: ID3D12GraphicsCommandList = device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator, None).ok()?;
        list.EndQuery(&ring.heap, D3D12_QUERY_TYPE_TIMESTAMP, q);
        list.ResolveQueryData(&ring.heap, D3D12_QUERY_TYPE_TIMESTAMP, q, 1, &ring.back, u64::from(q) * 8);
        list.Close().ok()?;
        windows::core::Interface::cast::<ID3D12CommandList>(&list).ok()?
    };
    ring.stamps[q as usize] = Some(list.clone());
    Some(list)
}

/// For a submission of the title's (`title`: its queue's list type, 0
/// direct, 2 compute, 3 copy): the list to send before its lists and the
/// one to send after them.
pub(super) fn around(device: &ID3D12Device, title: Option<u32>) -> Option<(ID3D12CommandList, ID3D12CommandList)> {
    let kind = match title? {
        0 => TITLE_DIRECT,
        2 => TITLE_COMPUTE,
        3 => TITLE_COPY,
        _ => return None,
    };
    if !on() {
        return None;
    }
    let mut guard = RING.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = make(device);
    }
    let ring = guard.as_mut()?;
    let i = next_pair(ring, kind);
    let lists = (stamp_list(device, ring, i * 2), stamp_list(device, ring, i * 2 + 1));
    match lists {
        (Some(ahead), Some(behind)) => Some((ahead, behind)),
        _ => {
            ring.kinds[i as usize] = u8::MAX;
            None
        }
    }
}

/// At the end of that list, before it is closed.
pub(super) fn end(list: &ID3D12GraphicsCommandList, pair: Option<u32>) {
    let Some(i) = pair else { return };
    let guard = RING.lock().unwrap_or_else(|e| e.into_inner());
    let Some(ring) = guard.as_ref() else { return };
    // SAFETY: an open list, our query heap and read-back buffer, in range.
    unsafe {
        list.EndQuery(&ring.heap, D3D12_QUERY_TYPE_TIMESTAMP, i * 2 + 1);
        list.ResolveQueryData(&ring.heap, D3D12_QUERY_TYPE_TIMESTAMP, i * 2, 2, &ring.back, u64::from(i) * 16);
    }
}

/// For the frame line: milliseconds a frame on the card, and lists (or
/// submissions) a frame, of each kind (`SEND`, `COMPARE`, `TITLE_...`);
/// `None` when off or nothing ran.
pub(super) fn per_frame(frames: f64, frequency: u64) -> Option<[(f64, f64); KINDS]> {
    if !on() || frames <= 0.0 {
        return None;
    }
    if let Some(ring) = RING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        for i in 0..PAIRS as usize {
            take(ring, i);
        }
    }
    let ms = |k: usize| TICKS[k].swap(0, Ordering::Relaxed) as f64 * 1e3 / frequency.max(1) as f64 / frames;
    let n = |k: usize| LISTS[k].swap(0, Ordering::Relaxed) as f64 / frames;
    let out: [(f64, f64); KINDS] = std::array::from_fn(|k| (ms(k), n(k)));
    out.iter().any(|o| o.1 > 0.0).then_some(out)
}
