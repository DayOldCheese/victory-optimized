# Progress

Engineering log for VIctory Optimized. Measurements are from the
contributor's machine (Ryzen 5 5600X, RTX 3060 12 GB, Windows 11) unless a
line says otherwise. Nothing here is estimated: every number has the command
that produced it.

## Provenance of the six earlier patches

The repository holds one commit. Diffing it against upstream
(`fxceless/victory-fork`) found the base commit `dd3a14b` (2026-11-25,
"README: a frame capture built into the layer…"): 6 files, +173 / −29 lines.

| Patch | Where | Review |
|---|---|---|
| Memory comparison fast path | `shadow.rs` `changed_chunks` | Correct. Only used with `VICTORY_COPY_CHANGED=1` (off by default). The full-span `==` runs before the chunk loop, so when anything differs, the span up to the first difference is compared twice. The fast path only helps the all-equal case. **Open: not yet benchmarked for the changed case.** |
| Remap temporary allocations | `watch.rs` `remove_blocks` | Correct. Saves an allocation for 0–1 block ranges only. |
| Texture queue | `descriptor/host.rs` `TextureQueue::due` | Correct; has a test comparing against the full ordering. |
| Submission range merging | `host.rs` `join_ranges` | Correct; compacts in place. |
| Pipeline completion | `pipeline/make.rs` | Correct. A per-pipeline condvar replaces a global one, so a waiter no longer wakes for every other pipeline that finishes. |
| Frame diagnostics | `framelog.rs` | Correct. `VICTORY_FRAME_LOG_FILE` appends each reading. |

None of the six touches `indirect.rs` or `inplace.rs`: both unstable tests
below are inherited from upstream.

## Stability investigation (concluded 2026-11-26)

Baseline before any change, `cargo test --locked -p victory-gpu --release`:
188 passed in most runs. A loop over the test binary showed three separate
problems, not two.

### 1. `indirect::tests::warp_budget_saturates_without_wraparound_and_preserves_draw_fields`: fixed (implementation defect)

- **Symptom:** fails 36/40 run alone, about 1/25 inside the suite.
- **Cause:** the argument-clamp shader charged the list's budget once per
  64-thread group, each group by compare-and-swap when it ran. Once the
  budget ran out, the records that kept it belonged to whichever group the
  device happened to run first. The test has 128 records, two groups; on
  WARP the second group usually wins, so records 64 and 65 were kept
  instead of 1 and 2. This is a real defect, not a test problem: under
  budget saturation, which draws survive could change from frame to frame.
- **Upstream:** upstream hit the same failure ("another two in five runs of
  seven") and relaxed the assertion to accept any two draws. Not done here.
- **Fix:** `indirect.rs`, clamp shader.
  - Dispatches of 64 records or fewer (one group) are unchanged: one read
    and one compare-and-swap.
  - Larger dispatches use a scheme where no group waits for another:
    1. Each group stores its records as fitting.
    2. It adds its cost sum to budget word 3, sets word 4 if that sum
       crosses the budget, and arrives at word 5.
    3. The last group to arrive is the only one that reads or writes the
       budget word. If everything fits, it adds the total. If not, it walks
       the records in order and empties every record from the first one
       that does not fit.
  - The result equals clamping records one at a time, in order.
  - Words 3–5 are reset by the last group.
  - `dst_args` is `globallycoherent` so the emptying stores land after the
    other groups' stores.
- **Evidence:**
  - The original test: 0/200 failures alone, 0/100 for the three budget
    tests together.
  - New test `warp_budget_goes_to_the_first_records_across_groups` (41
    groups, partly spent budget, zero-cost records past the cut-off): fails
    20/20 against the old shader, passes with the new one.
  - New test `warp_budget_that_fits_adds_every_group_once` (6 groups, exact
    total).
- **Performance** (`cargo test -p victory-gpu --release --lib indirect::bench -- --ignored --nocapture`, three runs each, GPU ms per frame of 3,200 draws):

  | Case | Before | First version | Final |
  |---|---|---|---|
  | one at a time, budget in mapped memory | 24.5–25.5 | 34.8–37.2 (+45 %) | 24.6–25.1 |
  | windows of 32, mapped | 1.04–1.05 | 1.27–1.28 | 1.05–1.06 |
  | one at a time, budget on the card | 20.9–21.5 | 22.7–24.5 | 21.1–21.4 |
  | windows of 32, card | 0.94–0.95 | 0.89–0.90 | 0.94–0.95 |

  The first version charged single-group dispatches with the multi-group
  scheme: about seven atomics plus device barriers each, against two. The
  final version keeps the old single-group path, so this benchmark (only
  single-group dispatches) shows no change, within noise. The multi-group
  path, used by gathers (up to 4,096 records) and large counted executions,
  has no benchmark yet. **Open.**

### 2. `inplace::tests::warp_a_title_buffer_in_place_shows_its_bytes_at_its_offset`: fixed (timing assumption in the test)

- **Symptom:** 0/50 alone, about 2/10 inside the suite.
  `VirtualFree(MEM_DECOMMIT)` returns 0.
- **Cause, measured with a temporary probe:** after the opened heap's
  reference count reached zero, decommit was refused with error 87 (pages
  still locked) for about 12 ms in 3 of 12 suite runs. The runtime/WARP
  lets go of the pages asynchronously after the last release.
- **Fix:** the test still requires the decommit to succeed, but waits up to
  2 s for the release (retrying only on error 87) and fails with the error
  and time waited otherwise.
- **Production note:** in-place windows are off by default
  (`VICTORY_INPLACE`). With them on, a title decommit right after a window
  is let go could fail the same way. This is already documented as a
  shortcut in `inplace.rs`; **open.**

### 3. Access violation inside `d3d10warp.dll`: unresolved, pre-existing

- **Symptom:** the test process dies with `0xC0000005` (bash exit code 139)
  and prints no failure line, so pass/fail counts alone do not show it.
- **Rate:**
  - Original code: 9/40 full-suite runs.
  - With the fixes above: 3/40, 3/30, 6/20 in different batches (same
    order of magnitude; the batches are too small to say the rate changed).
  - Serial (`--test-threads=1`): 0/15.
- **Where:**
  - A temporary unhandled-exception filter (removed) caught a *write* fault
    at `d3d10warp.dll+0xa842f` (another at `+0xa89b3`) on a WARP thread-pool
    thread, with no frames of ours on that stack.
  - Tests in flight at both recorded crashes:
    - `indirect::tests::warp_count_buffer_ignores_garbage_tail_and_preserves_source`
    - `queue2::tests::warp_a_fetch_through_the_copy_queue_brings_what_the_direct_queue_brings`
    - `spread::tests::this_process_has_threads_and_they_have_masks`
    - `watch::mapping_tests::reads_in_place_and_remappings_wait_for_each_other`
- **Reproduction:**
  - `queue2::tests::warp` alone (5 tests that use copy queues, cross-queue
    fence waits and leaked per-test `Second` lane threads): 3/60.
  - Any single queue2 test: 0/60.
  - Any pair: 0/40 each.
  - Leave-one-out: 0–1/60.
  - It needs several multi-queue tests at once on the one process-wide WARP
    device.
- **Not yet known:** whether this is a WARP bug or a use-after-free of
  ours (a GPU write into memory a finished test released). Reading the
  queue2 tests found no early release (each drains with `land()`).
- **Next step, when time is given:** a WinDbg/cdb session with a full dump
  of one crash, to name the destination buffer. This needs the Windows
  debugger installed (your approval). Until then, a run only counts as
  passing if the binary exits 0. Tests are not serialised to hide it.

### Baseline now

Every suite run that completes: **190 passed, 0 failed, 6 ignored**. The
two added tests are new; 188 + 2. Plus problem 3 above: some runs crash
instead of completing.

`cargo build --locked --release -p victory --features diagnostics`:
succeeds.

## Memory: `changed_chunks` double scan (patch 1 revisited)

Used to send only changed bytes of a card-side copy (`VICTORY_COPY_CHANGED=1`,
off by default). Patch 1 compared the whole span before the chunk loop. Now
equal 4 KiB blocks (16 chunks) are skipped with one compare each, and every
byte is compared once. A randomised test (300 spans, 5 chunk sizes) checks
the output is identical to upstream's.

`cargo test -p victory-gpu --release --lib shadow::merge_tests::bench_changed_chunks -- --ignored --nocapture`
(best of five, 64 KiB spans, three runs):

| Case | upstream | patch 1 | now |
|---|---|---|---|
| all equal | 1071–1073 ns | 602–807 ns | 694–876 ns |
| one change at byte 65,000 | 1122–1145 ns | 2019–2060 ns | 1052–1088 ns |
| a change every 8 KiB | 1241–1259 ns | 1244–1255 ns | 1291–1376 ns |

Patch 1 cost +80 % when the change lies late in the span. The new form
removes that, keeps most of patch 1's all-equal gain, and costs 4–10 % when
changes are dense. Whether this matters in a frame depends on how often the
switch is on and how big the spans are; there is no title data here to
measure that.

## Next

Performance profiling and rendering correctness (see the user's direction
of 2026-11-26): memory management, GPU use, textures, lighting. Measured
with authorised fixtures only (the crate's own synthetic GPU benchmarks,
WARP and host-GPU tests); no title files.

## Textures: BCPack decode profile (2026-11-26)

`cargo test -p victory-formats --release --lib bcpack::tests::bcpack_throughput -- --ignored --nocapture`
now also times the lanes (rANS and schedule) alone. 16,384 synthetic blocks,
two runs each, ns per block:

| Format | Before: total (lanes + blocks) | `Lane::step` inlined |
|---|---|---|
| BC1 | 106–108 (88 + 18–20) | 93–96 (80–81 + 12–16) |
| BC4 | 104 (89 + 15) | 94–96 (81–83 + 13–14) |
| BC7 | 365–366 (251 + 114) | 340–341 (228–229 + 111–113) |

- **Measured:** 10–12 % from `#[inline(always)]` on `Lane::step`. All 25
  format tests pass, including round trips, the reference rANS decode and
  noise.
- **Where the time goes:** the lanes take 69–87 % of it, about 7 ns per
  lane step, which points to branch mispredictions in the step and round
  loop rather than arithmetic.
- **Rewrite, not justified yet:**
  - The source's suggestion to decode lanes apart does not work as stated:
    which lane takes the next byte depends on each lane's decode state.
  - What would pay off is a SIMD step of all lanes per round. It is
    theoretical, a few times faster in the lanes by estimate, and high risk
    (bit-exact schedule).
  - DirectStorage already decodes requests in parallel on a worker pool of
    one thread per core.
  - Do it only if an authorised run's `dstorage` debug line shows
    `bcpack_workers` near the worker count.

## Textures: layout sweep over every swizzle mode (2026-11-26)

The existing round-trip test covered 10 hand-picked descriptions and none of
the XOR, `_T`, Z or R modes. New test
`texture::tests::random_descriptions_round_trip_in_every_swizzle_mode`:
- 400 seeded descriptions: all 16 modes, BC (4×4, 8/16-byte) and plain
  elements, odd sizes up to 300×200, whole mip chains, arrays, volumes.
- Every addressable mode must round-trip with each element at its own
  address.
- The XOR modes, which have no addressing yet (a documented shortcut), must
  refuse copies with `Unsupported` and never place bytes wrongly.

**Result: no layout bug found.** This is a measured negative: the sweep
passes. It stays as a regression net.

## Gaps measured but not changed (2026-11-26)

- **Indirect draws (GPU sync, the largest measured cost):**
  - By default every indirect draw gets its own clamp pass: 7.3 µs per
    draw in `indirect::bench`, about 23 ms of GPU time per frame at that
    benchmark's 3,200 draws.
  - Windows (`VICTORY_INDIRECT_WINDOW=1`) are 0.33 µs per draw, 22× less.
  - They are off upstream because runs with them on lost the device.
    Validating them needs an authorised title run; see PLAN.md.
- **Upstream correctness fixes since the fork base (`dd3a14b`):**
  - BC7 rotation applied only to modes 4–5 (BCPack);
  - legacy multiply-add as RDNA 2 runs it (`f6a09d3`, lighting maths);
  - SDWA 16-bit sign extension;
  - 16-bit and 32-bit integer typed views.

  The first two apply cleanly on their own. The 16-bit views fix also needs
  changes in `victory-gpu` (descriptor tables, draw, shader cache);
  importing only its shader half would break rendering, so it was not done.
  Porting any of them awaits the user's approval (the import step was
  stopped by the permission system).
