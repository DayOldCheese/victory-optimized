# Third-party code

Mandy, as VIctory, is licensed under GPL-3.0-or-later (`LICENSE`). Code taken from other
projects is listed here, with its license and where it is. Each such file
names its origin (repository and commit) in its header. Commits taken whole
from other forks of VIctory name the commit and its author in their message
and are not repeated here.

| Source | License | What | Where |
|---|---|---|---|
| [XCL](https://github.com/xcl-dev/xcl), `crates/xcl/src/hle/d3d12x/uma_driver.rs` at commit `06f90c198` (as written in `f2248f068` by Kalmarv and `f78e05bcc` by Prime1997) | GPL-3.0-or-later (from `06f90c198`, "Docs: relicense XCL to GPL-3.0-or-later"; GPL-2.0-only before it, from which nothing was taken). Copyright the XCL contributors. | Memory remade as a driver `CUSTOM` write-back L0 heap mapped at its own address: the snapshot of written pages, the release, the grooming of free address space so the driver's mapping lands in the hole, the write-watched heap, the restore; and its tests (the failed write watch, the failed restore, the grooming for a window under 1 GiB) | `crates/victory-gpu/src/uma_driver.rs` (`VICTORY_UMA_DRIVER`) |
| [VIctory-Hydra](https://github.com/ToLIManl/VIctory-Hydra) by ToLIManl, `crates/victory-loader/src/engine.rs` and `patch.rs` at commit `85e7e07d` (written in `a3f529bc` and `ecbd78be`). Its header names `HSgod/ps5-fpsunlocker-gta6` as the idea behind the shadow and volumetric settings. | GPL-3.0-or-later (its `LICENSE` and `Cargo.toml` at `85e7e07d`) | The title engine's own quality settings, each site compared byte for byte before it is written: shadow atlas size, volumetric map size, level-of-detail scale, small-object culling, vehicle detail bias; and their tests | `crates/victory-loader/src/engine.rs`, `crates/victory-loader/src/patch.rs` (`VICTORY_LOW_SHADOWS`, `VICTORY_LOW_VOLUMETRIC`, `VICTORY_LOD_SCALE`, `VICTORY_SMALL_CULL`, `VICTORY_VEHICLE_LOD_BIAS`) |
| [VIctory-Hydra](https://github.com/ToLIManl/VIctory-Hydra) by ToLIManl, `crates/victory-shader/src/emit/bitcode/opt.rs` (its passes, lines 1-3076; Mandy's tests kept) and `crates/victory-gpu/src/host/lean.rs` at commit `85e7e07d` | GPL-3.0-or-later | DXIL passes: extended folds, memory forwarding, loop-invariant hoisting, redundant barrier removal (`VICTORY_SHADER_OPT`). Lean compares: declared ranges read whole once in so many submissions (`VICTORY_CARD_COMPARE_EVERY`; its compares ahead of sends not ported) | `crates/victory-shader/src/emit/bitcode/opt.rs`, `crates/victory-gpu/src/host/lean.rs` and its hooks in `crates/victory-gpu/src/host.rs` |

The license was checked at `06f90c198`: its `LICENSE` is the GPL version 3
text, its `Cargo.toml` says `GPL-3.0-or-later`, and the ported file carries
no header of its own (no other notice to keep).
