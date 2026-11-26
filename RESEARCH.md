# Research: other compatibility layers

Scope:
- what each project does differently;
- whether that is better *for this architecture*, judged against what
  PROGRESS.md measured;
- whether its licence lets us reuse code.

Nothing here has been merged. Code was reused only from our own upstream
(same licence); see CHANGELOG.md. "Not verified" marks claims not checked
against the source.

## How VIctory differs from all of them

VIctory runs an Xbox Series title's own x86-64 code natively on the PC and
replaces the console's D3D12.X with host D3D12. Its shaders are the
console's RDNA 2 machine code, translated to DXIL. That puts it closer to a
D3D12-on-D3D12 layer plus a GPU ISA compiler than to a whole-system
emulator. The two hard parts that have no direct equivalent elsewhere:

- console memory shared by CPU and GPU, mirrored into host buffers (the
  `shadow`/`watch` modules);
- title-written indirect arguments, which must be clamped before the host
  GPU reads them.

## Projects

| Project | What it is | Licence | Reuse |
|---|---|---|---|
| [fxceless/victory-fork](https://github.com/fxceless/victory-fork) | Our upstream | GPL-3.0-or-later | Yes, with attribution (done for 3 fixes) |
| [Marvisak/VIctory](https://github.com/Marvisak/VIctory) | Upstream's upstream | See UPSTREAM-CREDITS.md | Yes |
| [vkd3d-proton](https://github.com/HansKristian-Work/vkd3d-proton) | D3D12 → Vulkan | LGPL-2.1 | Ideas only; code would need LGPL terms kept per file |
| [DXVK](https://github.com/doitsujin/dxvk) | D3D8–11 → Vulkan | zlib | Compatible, but a different API layer |
| [Mesa](https://gitlab.freedesktop.org/mesa/mesa) (RADV, ACO) | AMD Vulkan driver and RDNA compiler | Mostly MIT | Compatible; most useful as an ISA-semantics reference |
| [Xenia](https://github.com/xenia-project/xenia) | Xbox 360 emulator | BSD-3-Clause | Compatible; ideas for memory watching |
| [Xodus](https://github.com/xodus-gaming) | Xbox PC / Game Pass titles on Linux (Rust, launched 2026-08) | Not verified | Not until checked |
| XWine1 ([github.com/xwine1](https://github.com/xwine1)) | Xbox One compatibility layer, not yet public | Unknown | No |
| "XCL" | Not found under that name | None | Ask for a link |

## Findings that matter here

### 1. Indirect execution: the largest measured cost

- **Ours:** every indirect draw gets its own clamp compute pass by default:
  7.3 µs per draw in `indirect::bench`, about 23 ms of GPU per frame at
  that benchmark's 3,200 draws, more than a 60 FPS frame (16.7 ms) by
  itself.
- **vkd3d-proton:** translates `ExecuteIndirect` by preprocessing the
  argument buffer in a compute pass once per call, then one
  multi-draw-indirect or device-generated-commands execution. Version 2.10
  notes a large gain for compute with root-parameter changes through
  `VK_NV_device_generated_commands_compute`, with caveats (opt-in;
  technically outside the Vulkan spec).
- **Lesson:** batch argument processing per call or per list, never per
  draw. Upstream's windows and gathers already do this. They are off
  because real-title runs lost the device. vkd3d-proton does not need a
  safety clamp (it trusts the application's arguments); we do, because a
  translation bug can leave garbage arguments. So their design is not
  "better" for us as is.
- **The open question is stability, not design.** The next step is the
  window check in an authorised title run (ROADMAP.md item 2).

### 2. Shader semantics: an RDNA 2 reference

- Mesa's ACO compiler and LLVM's AMDGPU documentation define instruction
  behaviour: legacy multiply rules, SDWA selects, denormal flushing.
- The three correctness fixes ported tonight were all such semantics gaps.
- **Recommendation:** use ACO's source and LLVM's instruction tables as the
  reference when writing `valu_tests`. Citing documented behaviour is not
  code reuse, and MIT is compatible either way.

### 3. Memory: CPU writes the GPU must see

- **Xenia** watches guest memory with page protection and uploads only the
  ranges written, invalidating its texture cache on writes. VIctory does
  the same (`watch.rs` guards; write watches).
- **DXVK and vkd3d-proton** sub-allocate memory from large chunks and
  stream uploads through ring buffers, rather than making a resource per
  upload.
- **Measured here:** none of this is a measured bottleneck in this tree's
  synthetic benchmarks. Upstream's own logs (in source comments) name
  "about 78 MB a frame crossed to the card" for whole-page refreshes.
  `VICTORY_COPY_CHANGED` (off by default) targets that. Validation needs a
  title run.

### 4. Command submission and synchronisation

- vkd3d-proton and DXVK both avoid full-pipeline barriers. They track
  per-resource state and batch barriers.
- `indirect::bench` measured −10 % from replacing per-execution state
  changes with batched ones in this tree (upstream's change). Our clamp
  still issues a global UAV barrier before each pass. Making it
  conditional, only after a UAV write since the last clamp, is a candidate,
  but it needs per-list write tracking.
- Not measured further.

### 5. Ray tracing

- vkd3d-proton maps DXR to Vulkan ray tracing. That does not apply here:
  the title does not use DXR. Its traversal is compute code calling the
  RDNA 2 instruction `image_bvh_intersect_ray`, which this fork emulates in
  DXIL (`docs/fork-ray-tracing.md`).
- Mesa RADV implements the same instruction family in hardware paths and
  has a software path for older GPUs. Its handling of edge cases (equal
  distances, rays through edges) is the best public reference for our
  "assumed" behaviours. Not verified line by line.

## Conclusion

No project is demonstrably better *for this architecture* across the
board. The two transferable ideas with evidence behind them are:
- per-call or per-list batching of indirect arguments (already
  prototyped upstream; blocked on stability);
- ACO and LLVM as the semantics reference for shader correctness (applied
  tonight).

Neither replaces measurement on an authorised title.
