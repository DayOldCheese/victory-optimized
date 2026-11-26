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

## Release sprint (2026-11-26 evening)

### Correctness ports from upstream (fxceless/victory-fork, GPL-3.0-or-later)

Each is its own commit naming the upstream commit, and each test was shown
to fail without its fix.

| Fix | Upstream | Test |
|---|---|---|
| BCPack: BC7 rotation only in modes 4–5 (other modes swapped colour channels) | `06702fe` | `bc7_rotation_applies_only_to_modes_4_and_5` (upstream's) |
| Legacy multiply-add fused as on RDNA 2; a zero factor makes the product +0 (lighting maths) | `f6a09d3` | `legacy_mad_fuses_its_product_and_flushes_its_sources` (upstream's) |
| SDWA WORD_0 SEXT sign-extends 16-bit results (VIctory #3124) | `58de9bf` (hunk only) | `sdwa_word0_sext_extends_a_16_bit_result` (ours; on the RTX 3060 without the fix: `0x0000ffff`, want `0xffffffff`) |

**Not ported:** 16-bit values through typed views (`half_views`, same
upstream commit). It spans 23 files in `victory-shader` and `victory-gpu`
(+1,884 lines); half of it would break rendering.

### Multi-group indirect clamps, measured

New bench: `VICTORY_CLAMP_BASELINE=<file> cargo test -p victory-gpu --release --lib bench_multi_group_clamps -- --ignored --nocapture`.
It records 200 clamps a list on the RTX 3060, budget on the card, ms per
list, best of 5, two runs. The baseline is the shader of commit `29bdaa5`,
extracted to a file.

| Records per clamp | Baseline | Last-group scheme | + spent-budget fast path |
|---|---|---|---|
| 65 | 1.34–1.44 | 0.81–0.87 | 1.00–1.07 |
| 65, budget runs out | 0.86–0.94 | 2.21–2.37 | 0.64–0.69 |
| 1,024 | 4.52–4.89 | 0.71–0.77 | 0.89–0.97 |
| 1,024, budget runs out | 0.77–0.85 | 11.5–11.7 | 0.53–0.58 |
| 4,096 | 16.6–17.4 | 0.78–0.80 | 0.95–1.01 |
| 4,096, budget runs out | 0.86–0.88 | 44.1 | 0.62–0.62 |

- **Before the fast path:** after a list's budget ran out, every later
  clamp repeated the in-order walk.
- **The fast path:** each group reads the budget first (only the last
  group writes it). If it was already spent, the group empties its own
  costly records and the walk is skipped. Test:
  `warp_budget_goes_to_the_first_records_across_groups`, budget spent
  before the dispatch.
- **Measured:** 5–17× less GPU time for fitting multi-group clamps, about
  30 % less when the budget is spent. These run in the default
  configuration for counted `ExecuteIndirect` calls over 64 records.

### Rare test failure observed, unresolved

`watch::tests::a_write_is_reported_at_every_checked_view` failed once in 38
suite runs and not in 30 more. `watch.rs` is unchanged since the initial
commit. Not investigated further.

### Release candidate validation (2026-11-26, late)

- `cargo test --locked --workspace --release`: every crate passes. 594
  passed, 0 failed:
  - victory-gpu 191;
  - victory-shader 217;
  - victory-hle 98;
  - victory-formats 27;
  - victory-loader 27 + 4 integration;
  - the rest 34.
- `cargo build --locked --release -p victory --features diagnostics`:
  succeeds.
- **Sustained 60 FPS was not measured.** No authorised title workload
  exists in this environment. The synthetic benchmarks say where frame
  time goes, not what a frame costs. The largest such cost is the per-draw
  clamp pass (about 23 ms of GPU at 3,200 indirect draws, by itself over
  16.7 ms). See RESEARCH.md §1 and ROADMAP.md item 2.

## Frame-rate regression report: 0.4 FPS (2026-11-26, open)

- **Reported:** about 0.4 FPS in a title run after today's commits. The
  first report said black screen; corrected to low FPS.
- **Not yet reproduced:** there is no title workload here.
- **Ruled out so far:**
  - **Multi-group clamp on hardware:** its three budget tests now also run
    on the RTX 3060 (`card_budget_*`) and pass. The benchmark gives it
    5–17× less GPU time than before.
  - **CPU-side changes:** none on a default-config hot path
    (`changed_chunks` is off by default; BCPack got faster).
- **Prime explanation, unconfirmed:** a cold shader cache.
  - The cache key includes `victory_shader::emit::translation_fingerprint()`.
  - Three shader-translator commits today changed it, so the first run
    after updating re-translates every shader and recompiles its
    pipelines. Draws wait on that, within each frame's wait budget.
  - Expected: slow first run, normal second run. `victory translate --warm all`
    fills the cache ahead of time.
- **To isolate the clamp in one build:** `VICTORY_CLAMP=legacy` restores
  the clamp shader of `29bdaa5` unchanged.
- **Data needed from a run:** `VICTORY_FRAME_LOG_FILE=<file>` (frame-time
  readings) with and without `VICTORY_CLAMP=legacy`, after a warm cache.

## Upstream integration (branch `upstream-integration`, 2026-11-26)

- **What came in:** upstream `dd3a14b..e4245f3`, 85 commits, verified by
  `git rev-list --count`: code, docs and CI. Its `diagnostics/` run logs
  (868k lines of title runs) are left out.
- **Conflicts, resolved by hand:**
  - `framelog.rs`: both kept.
  - `pipeline/make.rs`: upstream's deny list with our per-pipeline
    condvar.
  - `watch.rs`: upstream's per-block remap bookkeeping, which is a
    correctness need; our helper kept for the aliased set.
  - `valu_tests.rs`: both tests kept.
- **Silent merge problems found by review, not by git:**
  1. Upstream's weakened assertion in the budget test came in with a
     clean merge. Restored to the strict one.
  2. Upstream's shader features (counted gather tables, dispatches spread
     past 65,535 groups, compute diagnostics) were dropped except a
     comment, while its Rust side, which sets their flags, came in. They
     were ported into this fork's restructured clamp by hand. Upstream's
     own test `warp_spread_records_split_large_x_and_keep_small_unspread`
     passes, with all budget tests (WARP and RTX 3060).
- **Tests:** workspace 0 failures except
  `xtask policy::repository_passes_policy`. Upstream fails it too: its own
  `host.rs` (528,954 bytes) and `lower.rs` (535,588) exceed its 512 KiB
  per-file rule. Documented, not weakened.
- **Release build:** succeeds.
- **Profiles (new):** `victory run --profile stable|performance|experimental`
  sets upstream's measured switch set from one list
  (`victory-core::profiles`). `scripts/run-fast.ps1` uses it. A test
  checks that every listed switch is read somewhere in the layer.
  - Performance adds what upstream measured since the fork base: texture
    worker, lookup cache, parallel refresh, settle room, and
    `VICTORY_TEXTURE_BACK=1` (correct water).
  - Upstream's measured world frame rate with that set: 8.2–9.5 FPS. Not
    measured here.

## User run report: 0.9 FPS, CPU 75 %, GPU 1 % (2026-11-26, evening)

Reported from the GUI: 0.9 FPS (1,111 ms frames), CPU 75 %, GPU 1 %, VRAM
8.7 GB, 2.9 GB of 32 GB RAM free.

- **RAM:** upstream's own README (line 706 at `e4245f3`) reports about
  24 GB in use during play on a 32 GB PC with 1–5 GB free, at 8–9.5 world
  FPS. Low free RAM alone does not explain a tenfold gap. It is still a
  risk (paging) and a target (memory mirroring); see below.
- **CPU busy, GPU idle:** the pattern of shaders and pipelines still being
  translated and compiled. That happens when the shader cache is cold
  (the upstream merge changed the translator fingerprint again), and when
  draws wait for pipelines. Not yet confirmed: the frame log's per-frame
  breakdown would show it.
- **Profile:** the GUI starts on the stable profile (all switches off,
  upstream: 3–6 s a frame). Which profile the run used is not known.
- **Needed to go further:** one run after `Warm Shader Cache`, on the
  performance profile, with its diagnostic log exported from the GUI.

### GUI fixed before this report

The Branch field showed "os error 123" when the GUI had no saved
settings: an empty repository path passed the check because
`"" + Cargo.toml` resolved against the folder the GUI was started from,
and git then ran in an empty-named folder. Only an absolute path is now
accepted. Test: `an_empty_repo_path_is_replaced_by_the_workspace`.
Verified by a screenshot of the GUI started from `C:\`.

## Diagnostic log, performance profile, world play (2026-11-26, 18:07–18:09)

The user exported a GUI diagnostic log: about 110 s of world frames on the performance profile. The game itself was not run or inspected here. Everything below comes from the layer's own frame log.

### Summary: the card is now the limit, not the CPU

| measure | usual frame | long frames |
|---|---|---|
| frame time (median) | 166–300 ms (3–6 FPS) | 330–800 ms |
| card busy (direct queue, own clock) | 92–100 % | gaps of 20–240 ms |
| card time of our lists per frame | 180–290 ms | — |
| of it the title's own lists | 100–200 ms | 200–550 ms |
| of it our send-and-hash and compare lists | 30–45 ms | 60–160 ms |
| submit thread preparing | 70–110 ms | 100–235 ms |
| submit thread waiting for the card | 0 ms | 120–285 ms |

The GPU-idle pattern of the earlier 0.9 FPS report is gone. With the performance profile and a warm cache ("pipelines: 0 made"), the card is about 95 % busy. The frame now waits on card time, and only the long frames add CPU waits.

### Bottlenecks, largest first

1. **Our per-indirect-command clamps serialize the card.** Each frame has 4,000–11,000 indirect commands bounded. Each one is our clamp dispatch between two UAV barriers, each barrier a wait for every earlier shader write. That is up to about 20,000 full-pipeline drains a frame. At 5–10 µs each, that is 50–200 ms of card time, the same size as the gap to console speed. The two remedies the layer already has were both off in this run:
   - `VICTORY_CLAMP_LEAN=1` drops the second wait (argued exact in `indirect.rs`, `clamp_lean`);
   - the experimental `VICTORY_INDIRECT_WINDOW` / `VICTORY_INDIRECT_GATHER` batch the clamps per list. The log shows "0 read from a window", so they were not active.

   This is the first thing to measure.
2. **Our card-copy hashing costs 15–20 % of card time.** Send-and-hash and compare lists take 30–45 ms a frame. Each frame sends 70–110 MB up in about 2,000–3,000 ranges, compares about 11,000–18,000 pieces on the card and fetches 1,100–2,300 pieces back. This comes from `CARD_BUFFERS_MB` and `CARD_REHASH_RUNS` in the performance profile. Fewer, larger runs, or skipping the compare for ranges the write watch reports as clean, would cut it.
3. **Memory is at its limit.**
   - The process holds 24.2–24.7 GB private.
   - The PC has 2.4–3.1 GB free and **only 300–1,250 MB left to commit**.
   - VRAM is at 9.6 of 10.8 GB.
   - 600–700 textures show no level (black) for lack of room.
   - One frame had 184,000 page faults.

   A commit failure would crash the run. Paging explains some of the long frames. Upload buffers are churned: 49→56 GB made so far.
4. **Dynamic descriptor slots ran out (a correctness problem).** At 18:08:33–36 the fewest free slots fell to 30 of 262,144. Tables were refused, and draws and indirect commands were dropped: "no free dynamic slots", plus "indirect kind 0/1/2/10: prepare failed", up to 377 a frame. This is visible as missing geometry. The ring needs to be larger, or released sooner.
5. **CPU preparation, 70–110 ms a frame on the submit thread:**
   - write-watch askings 15–40 ms;
   - list steps 30–65 ms;
   - refreshes 12–38 ms.

   On the recording threads:
   - making descriptor tables takes 70–160 ms a frame (summed over all six threads);
   - waits for the `HostHeap::mirror` lock take 45–80 ms a frame;
   - a third of the tables repeat one made earlier in the same list. The counter in `draw.rs` names this "a table cache within a list would serve these".

   This runs alongside the card, so it matters once item 1 is fixed.
6. **Long frames: synchronous waits.**
   - The submit thread waits 120–285 ms for the card on "a synchronous submission or present".
   - The present call spends 100–420 ms in "lines and settling".
   - Fence signals land 1.2–1.3 frames (200–450 ms) after the title's Signal.

   The title pipelines about one frame ahead. A frame that waits for the card in full then doubles.

### Not the cause in this run

- Pipeline creation: 0 made or loaded in every frame.
- DirectStorage: slowest request under 155 ms, none over 250 ms.
- Slow cores: 0 found.
- Texture-refresh waits for memory: 0.

### Next measurement (the user can run this; no build change is needed)

1. `VICTORY_CLAMP_LEAN=alt` on the performance profile. It switches every 20 s and says so in the log. Compare the `cardmeter` "a list of ours running" lines and the frame times between the halves.
2. Then the experimental profile, which adds the indirect window and gather. Check that the "read from a window" and "read a record gathered" counts become non-zero, and compare card time.
3. If the card time drops, make the winner part of the performance profile.
