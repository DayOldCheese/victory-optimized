# Mandy

Mandy runs an Xbox Series title natively on Windows, from files you supply, on
a **mid-range NVIDIA PC**. It is a fork of [VIctory](https://github.com/Marvisak/VIctory),
built on [Belobug's AMD line](https://github.com/Belobug/VIctory-Belobug-AMD)
(credits below). It adds a desktop app that builds, launches, measures and
reports runs, and it is tuned and measured on an **RTX 3060 12 GB**.

**This repository contains no game files, code or assets.** You need your own
copy of the game. Mandy is not affiliated with or endorsed by the game's
developer or publisher, or by Microsoft.

## The goal

Console-quality pictures first, at a playable frame rate: **15 fps in the
world** next, 30 and then 60 after that, on an RTX 3060 12 GB with 32 GB of
RAM. Memory comes first: on this card the title fills video memory and spills
into the PC's.

## Where it stands (2026-11-27)

All figures come from one PC: **Ryzen 5 5600X, 32 GB RAM, RTX 3060 12 GB,
Windows 11**. Settings were the Play profile, Series X profile, 4K output and no
ray tracing. Each figure is read from the layer's own log of that run.

| Run | World fps | What the log showed |
|---|---|---|
| 1: first run of a new build | 1.7–2.6 | about 500 ms of each frame waiting for pipelines; about 2,000 draws a frame |
| 2: second run | 0.1–0.6 | the same wait. The driver took about 5.6 s for each slow pipeline, and about 1,700 were still queued after 5 minutes |
| 3: after Warm Pipelines | **2.8–3.5, steady** | **0 ms waiting for pipelines**; 9,000–11,000 draws a frame; video memory full (about 10 of 11 GB) with **7–9 GB spilled into the PC's memory** |

Most of the step from run 2 to run 3 came from compiling the pipelines ahead of
time: the frame stopped waiting for the driver's compiler. The limit now is
video memory. The next runs test the Series S profile and more of the title's
buffers kept in the card's own memory.

Known problems:

- The driver refuses three compute programs because they keep the console's
  64-lane waves, which NVIDIA does not run. Their effects are missing.
- About 480 draws a frame have no pipeline.
- Textures are evicted and loaded again while video memory is full.

## What we built

- **A desktop app** (`apps/victory-gui`). It builds the layer, runs the tests and
  benchmarks, launches the title with a profile, shows the frame rate live, and
  exports a diagnostic log. It also checks commits for secrets and game files
  before you commit. The app began in our earlier tree and was ported onto this
  one.
- **Run profiles** (`victory-core::profiles`). Play, Ray-traced light, Safe and
  Layer defaults are the switch sets of `scripts/play.ps1`, so that a run from
  the app matches a run from the script. Each switch can be overridden per run.
- **Pipelines made ahead by default.** The Play profile writes down every
  pipeline the title creates (`VICTORY_PIPELINE_BOOK=1`), so that Warm Pipelines
  can compile them before a run. That is what removed the 500 ms wait in each
  frame (run 3 above).
- **Progress for long tasks.** Warm Pipelines and Translate All Shaders show a
  bar with the count, the elapsed time, the time left at the rate so far, and
  whether the task finished. `victory warm` reports every 25 pipelines
  (it used to report every 1,000).
- **A diagnostic log small enough to share.** Lines that repeat are kept to the
  first five and the last of each kind. Every warning, error and frame-rate line
  is kept. A 3.8 MB log comes out at 270 KB.
- **Frame rate read from the layer's own summary line** in the title's output.
  The app estimates nothing.
- **Refused pipelines say why.** A compute pipeline the driver refuses is logged
  with the wave size and thread-group size it asks for, read from the program's
  own container. That is how the 64-lane programs above were identified.
- **NVIDIA review.** The AMD-specific assumptions in the shader translator and
  the Direct3D 12 back end (wave size, RDNA float shortcuts, descriptor heap
  limits) were checked against 32-lane NVIDIA hosts.

## How to run it

You need Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with shader
model 6.6 and mesh shaders, Rust (the toolchain is pinned), the MSVC build
tools, and your own copy of the title.

```powershell
git clone https://github.com/SomphHail/Mandy
cd Mandy
copy victory.example.toml victory.toml      # then set your paths, outside this folder
cargo build --release -p victory-gui
target\release\victory-gui.exe
```

In the app:

1. **Build**. It builds `target\diag\victory.exe`, the build every figure above
   comes from.
2. **Play** tab: choose the title's executable and the **Play** profile.
3. The first time, press **Translate All Shaders**, then launch once until you
   reach the world (this writes down the pipelines), then press
   **Warm Pipelines**. Repeat Warm Pipelines after a driver update or a new
   build.
4. **Launch Game**. Afterwards, **Export diagnostic log** saves the compact log.

Without the app, `scripts\play.ps1 -Exe <path>` runs the same settings. The
upstream documentation is in [docs/README-7900XTX.md](docs/README-7900XTX.md)
and [docs/README-fxceless.md](docs/README-fxceless.md), and every `VICTORY_...`
switch is described in a comment where it is read.

## Credits

Mandy is built on other people's work. The history is kept, and commits taken
from elsewhere name their source.

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors: the loader, the runtime services, the Direct3D 12 back end and
  the shader recompiler.
- **[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)**:
  the tree Mandy is built on, and its contributors.
- **[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)**:
  the renderer ports, the measured settings and the README kept in
  [docs/README-7900XTX.md](docs/README-7900XTX.md).
- **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)**,
  **[DexNom/VIctory](https://github.com/DexNom/VIctory)**,
  **[FQY55kai/ZJOU-VICTORY](https://github.com/FQY55kai/ZJOU-VICTORY)**,
  **[myokex/victory-amd](https://github.com/myokex/victory-amd)**, **Geckomacabre**
  and **cxy0072019-sketch**, as credited in
  [docs/README-7900XTX.md](docs/README-7900XTX.md).
- **[XCL](https://github.com/ivall/xcl)** (GPL-2.0-only) was read for how the same
  title behaves under another layer. None of its code is here.
- Mandy's changes were made with [Claude Code](https://claude.com/claude-code).

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE). The one file it does not
cover is named in [docs/asset-policy.md](docs/asset-policy.md).
