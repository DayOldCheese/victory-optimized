# Mandy

**An Xbox Series compatibility layer for Windows that plays a Series title from your own copy, tuned and measured
on a mid-range NVIDIA PC.** One window: pick the game, pick a graphics quality, press Play.

Mandy is SophieRain's project. It began as **victory-optimized**, which SophieRain wrote, and grew into what it is
now: the best measured pieces of several community layers combined into one build, with Mandy's own app, launch
logic and performance work on top, each step measured on real hardware.

> [!IMPORTANT]
> **This repository contains no game files, code or assets**, and no NVIDIA or AMD files. You need your own copy of
> the game. Mandy is not affiliated with or endorsed by the game's developer or publisher, by Microsoft, NVIDIA
> or AMD.

## Where it stands

Measured on one PC: **Ryzen 5 5600X (6 cores), 32 GB RAM, RTX 3060 12 GB, Windows 11**, in the open world, from the
layer's own frame-rate lines.

| Mandy, step by step (2026-11-27) | Frame rate in the world |
|---|---|
| Mandy's first tree (Belobug's AMD line, tuned) | 2-3 fps |
| Moved onto VIctory-NVIDIA's tree, Series S, DLSS | 3-9 fps |
| Windows page file sized for the game, helper threads for 6 cores | 6.4 fps median, peaks of 19.9 |
| Every pipeline compiled ahead | 13.2 fps median, 15-18 held for minutes |
| **Mandy's app: the launch picked from the hardware** | **16.3 fps median** (11.7-20.0) |
| **With FSR 3 frame generation (2x)** | **15.0 fps rendered, about 30 fps shown** |

From about 3 fps to about 30 shown in a day. The next targets are 30 rendered, then 60.

## What Mandy brings

**The app** (`apps/victory-gui`, written for Mandy)

- **One screen.** The game's executable, a graphics quality (Auto, Performance, Quality (Series X), Ultra (Series X
  with the game's own ray tracing)) and Play. Build and the NVIDIA download only show when they are needed. The
  frame rate is live, and a compact diagnostic log is one click (repeats cut, every warning, error and frame-rate
  line kept: a 3.8 MB log becomes about 270 KB).
- **The launch is worked out from the hardware** (`optimal.rs`). From the card's name and video memory and the
  processor's threads: Series X by default on 16 GB cards and on 12 GB RTX 40/50 cards, Series S elsewhere; the
  DLSS mode; frame generation (DLSS on RTX 40/50, AMD FSR 3 elsewhere); helper threads cut to the counts measured
  best on 6-core processors, at the raised priority of the thread that submits the frames. Every rule names the
  run it rests on.
- **Nothing left running.** Each launch runs in its own Windows job object, so Stop ends the game and every worker
  it started. Before a launch the app finds layer processes left from earlier ones (a measured run had a hidden
  game and a 67-minute preload still going under the next game) and offers to end them, rather than start a
  second game on top.
- **Shaders before the game, without the hour of pipelines.** A launcher option (`-PreloadShaders`) translates
  the shaders ahead with the run's own switches and leaves the pipelines to the driver's cache, which keeps
  them from one run to the next.

**The layer**

- **Measured choice of base.** Three trees were run on the same PC: Mandy's first tree, VIctory-NVIDIA's and
  VIctory-Hydra's. VIctory-NVIDIA's ran best here (VIctory-Hydra's gave a black screen on this PC), so it is Mandy's
  base, with Mandy's app on top and the others' work ported over piece by piece.
- **Descriptor slots in slabs** (`VICTORY_DYNAMIC_SLAB`, on): a command list takes 256 dynamic descriptor slots
  under one lock and hands them to its tables itself, where every table used to take the heap's lock; the 4
  translating threads stop queuing on it. Ported from VIctory-Hydra; the frame's preparing time fell from 47 to 44
  ms here.
- **Fills read a placed copy's source** (`VICTORY_PLACED_FORWARD`, on): a fill that a placed copy in the same
  list covers reads the copy's source instead of waiting for the copy to land. Ported from VIctory-Hydra, where it
  removed about 21 ms a frame of such waits.
- **Diagnosis that found the limits.** Per-command timing of the list translators and per-step timing of the
  draw path (in Mandy's history) showed where frames went; the logs showed the Windows commit limit throttling
  texture loading (54,865 uploads turned down in one run, 5,711 after the page file was sized), the CPU
  oversubscribed by helper threads, and pipelines compiled mid-play as the main source of stutter.

## How to run it

You need Windows 10 or 11 (x64), a CPU with AVX2, 32 GB of RAM, a Direct3D 12 GPU with shader model 6.6 and mesh
shaders (an NVIDIA RTX card for DLSS), Rust (the toolchain is pinned) with the MSVC build tools, and your own copy
of the game.

```powershell
git clone https://github.com/SomphHail/Mandy
cd Mandy
cargo build --release --locked -p victory-gui
target\release\victory-gui.exe
```

In the app: **Build Mandy** (a few minutes, once), **Get NVIDIA DLSS files** (NVIDIA's own files from NVIDIA's
GitHub, about 70 MB, once), choose the game's executable, leave the graphics quality on **Auto**, and press
**Play**. The first start of a new build translates the shaders for a few minutes; later starts take one or two.

Tips from the measured runs:

- **Windows page file:** set a custom size on an SSD (16384 MB initial, 49152 MB maximum). With Windows' default,
  commit memory ran out and the layer slowed its texture loading for the whole run.
- **Close big programs** (browsers) before playing: the game and the layer use about 24 of 32 GB.
- In the game, **F1** opens the layer's settings menu and **F3** its stats panel.

## Credits

Mandy combines and builds on the work of many people. History is kept, and commits taken from elsewhere name
their source.

- **SophieRain**: Mandy's author. Wrote the original victory-optimized, which Mandy grew out of; designed and
  directed Mandy's app, the hardware-based launch, the choice of base and every port and measurement above.
- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its contributors: the loader, the runtime
  services, the Direct3D 12 back end and the shader recompiler that every tree here is built on.
- **[trustno1-coder/VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA)**: Mandy's base layer since
  2026-11-27, with its F1 menu, F3 panel, DLSS and frame generation handling and launcher
  ([its README](docs/README-VIctory-NVIDIA.md)); and through it
  **[readmet3xt-lab/foxhound](https://github.com/readmet3xt-lab/foxhound)** ([its README](docs/README-foxhound.md))
  and the trees foxhound credits: **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)**,
  **[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)**,
  **[codec-d/cool_fox](https://github.com/codec-d/cool_fox)**, **[DexNom/VIctory](https://github.com/DexNom/VIctory)**,
  **[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)** (also Mandy's first base),
  **[FQY55kai/ZJOU-VICTORY](https://github.com/FQY55kai/ZJOU-VICTORY)**, **[myokex/victory-amd](https://github.com/myokex/victory-amd)**
  and the others named there.
- **[ToLIManl/VIctory-Hydra](https://github.com/ToLIManl/VIctory-Hydra)**: the descriptor slabs and placed-copy
  forwarding ported above, with their tests.
- **[Maykzera/victory-rtx3060](https://github.com/Maykzera/victory-rtx3060)**: the helper-thread counts measured
  on the same hardware; **[clankenjoyer/scarlet-fps](https://github.com/clankenjoyer/scarlet-fps)**: helpers at
  the submitting thread's priority.
- NVIDIA Streamline, DLSS and Reflex are NVIDIA's, and AMD FidelityFX is AMD's; both are fetched from their makers
  and loaded at run time, never part of this repository.
- Mandy's code was written by SophieRain together with [Claude Code](https://claude.com/claude-code).

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE); the one file that is not covered is named in
[docs/asset-policy.md](docs/asset-policy.md).
