# VIctory Optimized

Experimental source-only research fork of VIctory, focused on general-purpose graphics compatibility, regression testing and performance instrumentation. Not a stable release.

## Changes

See [CHANGELOG.md](CHANGELOG.md) for the full list. In short:

- **Correctness:**
  - indirect draws keep their budget in record order (it depended on GPU
    scheduling before);
  - BC7 rotation fixed in BCPack;
  - legacy multiply-add as RDNA 2 runs it;
  - SDWA 16-bit sign extension.

  The last three are ported from upstream with attribution.
- **Performance, measured on an RTX 3060:**
  - multi-group indirect clamps 5–17× cheaper;
  - BCPack decode 10–12 % faster;
  - a double memory scan removed.
- **Tests:**
  - a seeded texture-layout sweep over every swizzle mode;
  - budget-order tests;
  - on-card benchmarks.

Measurements, commands and what remains open are in
[PROGRESS.md](PROGRESS.md); next steps in [ROADMAP.md](ROADMAP.md).
Everything experimental (ray tracing, indirect windows and gathers,
in-place memory, copy-changed) stays off by default.

## Validation

On Windows 11, Ryzen 5 5600X, RTX 3060:

- `cargo build --locked --release -p victory --features diagnostics`
  succeeds.
- `cargo test --locked -p victory-gpu --release`: 191 passed, 0 failed,
  7 ignored in every run that completes.
- Known exceptions, documented in PROGRESS.md:
  - an intermittent crash inside the software rasterizer (WARP) when
    several copy-queue tests run at once (a crashed run prints no failure
    line, so check the exit code);
  - one rare failure of a `watch` test.

No compatibility guarantees are made. Use only trusted, authorised test
fixtures and the Rust toolchain specified by the workspace.

## Attribution and licence

Based on [fxceless/victory-fork](https://github.com/fxceless/victory-fork), itself based on [Marvisak/VIctory](https://github.com/Marvisak/VIctory). Original upstream credits are preserved in UPSTREAM-CREDITS.md. Changes in this archive are dated 2026-11-26 and were developed with AI assistance. GPL-3.0-or-later; see LICENSE.

Fixes ported from upstream name their source commit in the commit message and CHANGELOG.md. `crates/victory-formats/src/bcpack-tables.bin` (the BCPack format's constant tables) is inherited from upstream unchanged.

No proprietary game executables, assets, personal logs or screenshots are included. Use only authorised software and test data.

## Desktop GUI

`cargo build --release -p victory-gui`, then run `target\release\victory-gui.exe`:
- **Dashboard:** build, tests, benchmarks, live CPU/GPU/RAM/VRAM.
- **Play:** launch or stop a title with the stable, performance or
  experimental profile; warm the shader cache.
- **Settings:** each switch with what it does; experimental ones marked.
- **Diagnostics:** live FPS and frame-time graph from the layer's frame log,
  errors, console, export.
- **Git:** changes, history, audit before commit, push only after typing the
  branch name.

It drives `cargo`, `git` and `victory.exe`; the layer itself is unchanged,
and the command line works as before.
