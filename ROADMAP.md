# Roadmap

Ordered by measured or expected value. Items marked *needs a title run*
can only be validated with an authorised title, which this repository does
not contain.

## Next
1. **16-bit typed views** (upstream `half_views`, `58de9bf`). Port the
   shader and `victory-gpu` halves together (descriptor tables, draw,
   shader cache). Validate with upstream's tests plus a
   `translated_compute_matches_pc` case.
2. **Indirect windows and gathers by default** (*needs a title run*). The
   bench gives 0.33 µs per draw against 7.3 µs clamped one at a time:
   about 23 ms of GPU per frame at 3,200 draws. They are off because runs
   with them on lost the device. Validate with `VICTORY_INDIRECT_WINDOW=1`
   and `VICTORY_INDIRECT_WINDOW_CHECK=1`, plus the driver watch
   (`drvwatch`).
3. **Root cause of the WARP access violation.** Needs cdb/WinDbg and a
   full dump to name the buffer written.
4. **Cost of the emulated BVH node test on the GPU.** A timestamped
   dispatch harness over many groups, reusing the 311 reference cases.

## Later
- **BCPack SIMD lane decode.** Only if a title run shows `bcpack_workers`
  near the worker count; it is about 7 ns per lane step now, and a few
  times faster is an estimate, not a measurement.
- **Ray tracing** (`docs/fork-ray-tracing.md`): the builder pipelines that
  still ask for 64-lane waves, the five untranslated shadow-mask trace
  programs, and title pages without a host copy. All off by default.
- **In-place windows** (`VICTORY_INPLACE`): retry title decommits during
  the host's asynchronous page release.
- **XOR swizzle addressing** (`_X`, `_T`) for titles that copy into or out
  of render targets.
