# VIctory Optimized

Experimental source-only research fork of VIctory, focused on general-purpose graphics compatibility, regression testing and performance instrumentation. Not a stable release.

## Changes

- Memory comparison fast path and synthetic benchmark.
- Reduced temporary allocations in memory remapping.
- Bounded texture scheduling and submission range merging.
- Pipeline completion notification improvements.
- Optional persistent frame diagnostics.

## Validation

The contributor's Windows build previously reported 188 passing GPU tests, zero failures and six ignored tests after these changes. This reconstructed archive has not been independently compiled or benchmarked. No performance or compatibility guarantees are made.

For development with trusted, authorised test fixtures, use the Rust toolchain specified by the workspace. The GPU regression suite is `cargo test --locked -p victory-gpu --release`.

## Attribution and licence

Based on [fxceless/victory-fork](https://github.com/fxceless/victory-fork), itself based on [Marvisak/VIctory](https://github.com/Marvisak/VIctory). Original upstream credits are preserved in UPSTREAM-CREDITS.md. Changes in this archive are dated 2026-11-26 and were developed with AI assistance. GPL-3.0-or-later; see LICENSE.

No proprietary game executables, assets, personal logs or screenshots are included. Use only authorised software and test data.
