# Plan

Prioritised by expected value against cost, using what has been measured
(see PROGRESS.md). Items marked **approval** need a yes first (a
dependency or tool to install, or a major rewrite).

## Done

1. Deterministic indirect-argument budget across groups (fixes the flaky
   `indirect` test; no change on the existing benchmark).
2. `inplace` test waits, with a deadline, for the host's asynchronous
   release of an opened heap's pages.
3. `changed_chunks` compares each byte once (patch 1 cost +80 % when the
   change came late in the span).

## Next, in order

1. **Benchmark the multi-group clamp path.** Gathers (up to 4,096 records)
   and large counted executions now use the last-group scheme. The
   existing benchmark covers only single-group dispatches. Add a case to
   `indirect::bench` (64 groups, fitting and saturating) and compare
   against the old shader, which is kept in the bench as `OLD_SOURCE`.
2. **BCPack decode throughput (textures).** Measured 107 ns per BC1/BC4
   block and 366 ns per BC7 block on one core
   (`cargo test -p victory-formats --release --lib bcpack::tests::bcpack_throughput -- --ignored --nocapture`).
   The source already marks the lane-at-a-time decode as a shortcut. Two
   steps, cheapest first:
   - Make `Lane::step` cheaper: replace `Option` fields with plain flags
     and keep the round loop branch-light. The round-trip and
     noise tests guard the output.
   - Decode independent DirectStorage requests on worker threads, if
     `dstorage.rs` does not already. Needs reading that module first.

   Re-architecting the lane schedule itself is a major change:
   **approval**.
3. **Root cause of the WARP access violation** (PROGRESS.md, problem 3).
   Needs Debugging Tools for Windows (cdb/WinDbg) to take a full dump and
   name the buffer WARP writes to: **approval** to install. Until then a
   suite run passes only if the binary exits 0.
4. **In-place windows and title decommits** (`VICTORY_INPLACE`, off by
   default): when a title frees memory that a window was just released
   from, retry the free during the host's asynchronous release, as the
   test now does.
5. **Rendering correctness: lighting, depth, textures.** There are no title
   assets here, by design. An authorised fixture would be shader programs
   and textures of our own, run through `victory-shader` and the texture
   paths, with output compared against a reference. Before writing any:
   - inventory the existing shader tests (`victory-shader` `*_tests.rs`);
   - decide on a reference: the host compiler's output for equivalent
     HLSL, or hand-computed values.
6. **Phase 2 research (RESEARCH.md).** Not started: open-source compatibility
   layers (vkd3d-proton, DXVK, Mesa's radv/ACO for the console's GPU ISA)
   and their licences.
7. **Documentation:**
   - ARCHITECTURE.md: upstream's `docs/architecture.md` covers the crate
     map; still to add submission, synchronisation and memory flows.
   - CHANGELOG.md, ROADMAP.md, CONTRIBUTING.md, from this file and
     PROGRESS.md.

## Rules kept

- No assertion weakened or test removed; no failure hidden.
- No number without the command that produced it.
- Nothing pushed; checkpoints are local commits on `stabilise-baseline`.
