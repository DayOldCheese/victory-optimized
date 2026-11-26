# Changelog

Changes of this fork on top of `fxceless/victory-fork` (base `dd3a14b`,
2026-11-25). Measurements and commands are in PROGRESS.md.

## Unreleased: release candidate 1 (2026-11-26)

### Fixed
- **Indirect draw budget is deterministic across thread groups.** Under
  budget saturation, which draws survived depended on GPU group scheduling,
  so it could change between frames. Records now keep the budget in record
  order, as if clamped one at a time.
- **BC7 colour channels (BCPack):** rotation applied only in BC7 modes 4–5.
  Ported from upstream `06702fe`.
- **Lighting maths:** `v_mad_legacy_f32` / `v_mac_legacy_f32` are fused, as
  RDNA 2 runs them, with the legacy zero-factor rule. Ported from upstream
  `f6a09d3`.
- **16-bit results:** SDWA WORD_0 with SEXT sign-extends (VIctory #3124).
  Ported from upstream `58de9bf`, lowering hunk only.
- **`inplace` test:** allows for the host's asynchronous release of an
  opened heap's pages (about 12 ms measured) instead of assuming it is
  immediate.

### Performance (measured on an RTX 3060 / Ryzen 5 5600X)
- Multi-group indirect clamps (counted `ExecuteIndirect` over 64 records):
  - 5–17× less GPU time when the budget fits;
  - about 30 % less once it is spent.
- `changed_chunks` (`VICTORY_COPY_CHANGED=1`) compares each byte once:
  - removes the +80 % double scan the earlier fast path had for late
    changes;
  - 4–10 % slower when changes are dense.
- BCPack decode 10–12 % faster (lane step inlined).
- Single-record and windowed clamps: unchanged (within noise).

### Tests and benchmarks added
- Indirect budget:
  - order across 41 groups;
  - exact multi-group charge;
  - budget already spent.
- Texture layouts: 400 seeded descriptions over all 16 swizzle modes. No
  layout bug found; XOR modes must refuse copies.
- SDWA sign extension, run on the host GPU and WARP.
- Benchmarks:
  - `bench_multi_group_clamps`;
  - `bench_changed_chunks` (three forms);
  - BCPack lanes and blocks timed apart.

### Inherited from the earlier six patches of this fork
- Memory comparison fast path, now replaced (see above).
- Fewer temporary allocations in memory remapping.
- Bounded texture queue.
- In-place submission range merging.
- Per-pipeline completion notification.
- `VICTORY_FRAME_LOG_FILE`.

### Known issues
- An intermittent access violation inside `d3d10warp.dll` when several
  copy-queue WARP tests run at once (pre-existing; a crashed run prints no
  failure line).
- A rare failure of `watch::tests::a_write_is_reported_at_every_checked_view`
  (1 in 38 runs).
- 16-bit typed views (upstream `half_views`) not ported.
- Ray tracing: unchanged; see `docs/fork-ray-tracing.md`.
