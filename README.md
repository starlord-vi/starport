# Starport

Experimental Windows compatibility work based on **ztrx** commit
`90b529f878a470fec9bf07b7be316bc769a12232`. GPL-3.0-or-later; upstream
history and credits are retained. Starport's repository is
[starlord-vi/starport](https://github.com/starlord-vi/starport).

On the local RTX 5060 / i5-13400F / Windows 11 machine, the account screen,
animated intro, first-boot settings and background art have rendered, and
keyboard navigation works. The latest user-supplied Series X world run averaged
**1.26 FPS over its final 30 seconds**, with a 714 ms median frame interval.
Black/gooey surfaces and shadows remain reported. Four formerly failing pixel
shaders now create host pipelines, but four Wave64 compute pipelines still fail
on NVIDIA and the RT builder remains incomplete. Lighting, boats, usable world
performance and 30/60 FPS gameplay are not solved. A separate Series S run at
30 FPS exercised a menu/loading workload, not the world.

**Bring your own game files.** Executables, archives, shader dumps, captures,
saves and caches stay outside this repository. The inherited format-table
exception and its licensing scope are documented in [the asset policy](docs/asset-policy.md).

## Build and run locally

Use Windows x64, the Rust toolchain pinned in `rust-toolchain.toml`, the MSVC
C++ build tools, Python 3.10+ and a compatible D3D12 GPU (see the inherited
requirements below). From an existing workspace directory, clone and build:

```powershell
git clone https://github.com/starlord-vi/starport.git port
cd port
$env:CARGO_TARGET_DIR = [IO.Path]::GetFullPath('..\compat\starport_build')
cargo build --profile diag -p victory --features diagnostics
```

The default layout is relative to the repository's parent directory:

```text
workspace/
  port/                         this source repository
  exe/game_scarlett_master.exe   your original game executable
  files/                        your original package files
  compat/starport_build/diag/    compiled victory.exe
  compat/tools_bin/dxc-1.9.2609/bin/x64/
                                DXC's dxc.exe, dxcompiler.dll and dxil.dll
  runtime/starport/              generated package view, saves, cache and logs
```

The DXC path above is the runner's current expected layout; supply those DXC
tools separately. From the repository, run `./Play-Starport.cmd`. It defaults
to the Series S profile and keyboard/mouse controls, runs until you close the
game and records lightweight diagnostics. Explicit options override the profile:

```powershell
.\Play-Starport.cmd --console series-x --no-skip-top-mip
.\Play-Starport.cmd --console series-s --skip-top-mip
```

`STARPORT_SERIES_S=0|1` changes the launcher's default console profile;
`STARPORT_SKIP_MIPS=0..12` controls streaming mip omission (automatic default:
2 at a display ceiling <=1080p, otherwise 0; at least 3 mips remain).
`STARPORT_SKIP_TOP_MIP=0|1` remains an explicit alias for N=0|1.
See [the ordered verification guide](VERIFY.md) for kill switches and comparisons.
The title chooses resolution and texture pools from the console
profile; the spoof does not force a particular resolution. Override `--game`,
`--assets`, `--backend` and `--state` for another layout. Direct Python use
(`python scripts\starport.py --seconds 0 --console series-s`) also works;
without a console option or environment flag, that runner defaults to Series X.
See [residency and shader notes](docs/starport-residency.md) and
[the audit patch notes](docs/starport-audit-patches.md) for presets and logging.

Game binaries are verified copies in a separate package view; archives are hard
links. The runtime rejects mutations of the console's G: package drive. Native
file reads remain real reads; saves and cache live outside this repository.
Each run keeps its executable hash, source diff, new source files, settings,
console log and original-file integrity results. `--capture` adds game-derived
PNG evidence outside Git. The timer requests normal title suspension before
exit; a failed timeout is reported as forced termination.

## Keyboard and mouse

The launcher explicitly selects keyboard/mouse, even with a dormant pad present.
The console UI still displays controller glyphs; the adapter supplies that input.

| Keyboard/mouse | Console input |
|---|---|
| Enter / Backspace | A / B |
| X / Y | X / Y |
| Esc / Tab | Menu / View |
| Arrow keys | D-pad |
| WASD | Left stick |
| IJKL | Right stick |
| Q / E | Left / right shoulder |
| F / H | Left / right stick click |
| Z / C | Left / right trigger |
| F6 | Capture/release relative mouse look |
| Captured mouse movement | Right stick, bounded velocity |
| Captured right/left mouse button | Left/right trigger |

Losing focus releases mouse confinement and clears its state. F6 is initially
off. Short keyboard taps are retained for 80 ms; held keys remain held. This is
a virtual-pad adapter, not native PC camera code. Physical mouse feel and every
gameplay action still require in-game verification.

## Reviewed changes and remaining work

See [local validation](docs/starport-validation.md) and
[source provenance](docs/starport-provenance.md) for measured limits and credits.
The latest affected-crate checks reported 470 passing tests, 3 ignored
benchmarks and 4 compiler-dependent internal skips within that passing count.
Workspace check, Clippy with warnings denied and the asset policy passed.
Eight reported shader containers pass offline DXIL validation; four compute
pipelines still fail the separate NVIDIA PSO creation diagnostic.

- [ztrx #36](https://github.com/Zettrox/ztrx/pull/36): valid integer-target
  blend state and a conservative LDS flag-region lowering for wave32 hosts.
  Starport adds 14 hardware dispatch/readback comparisons and includes the
  lowering switch in shader-cache identity.
- [ztrx #94](https://github.com/Zettrox/ztrx/pull/94): coalesce exactly adjacent
  source/destination readbacks; retain the CPU merge boundaries. Tests include
  real WARP copies and byte-for-byte readback. No synchronization is removed.
- Local: keyboard/mouse, tap retention, connected-device local-user lookup,
  immutable package opens, bounded runs, source/binary evidence and title-bar
  pending-pipeline counts.

`--relay` enables the inherited experimental asynchronous submission settings;
`--bvh` requests the inherited RT-builder path. Both remain opt-in pending local
world measurements. Series S is now the launcher's default memory profile,
but its world rendering and additional presentation/upscaler path remain
unverified. Series X remains available for comparison.

Warm shader caches avoid repeated translation, but discrete GPU memory copies,
readback merges and submission waits also need profiling. We do not shorten the
pipeline wait budget to claim higher FPS: dropping unfinished passes can remove
geometry or one-time lighting work. The switchable `--ambient 0.6` default is
a constant indirect-light fallback, not ray tracing; `--ambient off` disables
it. Boat height and RT correctness remain open. No assertions were weakened.

## Inherited ztrx documentation

The following status is the upstream author's report, not local validation.

### ztrx

**ztrx** runs the Xbox Series build of the game natively on Windows PCs with
NVIDIA graphics cards, from game files you supply yourself.

**This repository contains no game files or assets.** You need your own copy
of the game. Not affiliated with or endorsed by its developer or publisher.

## Status (2026-11-25)

Tested on Windows 11 with an **NVIDIA GeForce RTX 3050** (8 GB):

| | |
|---|---|
| Boot, first-boot screens, intro, opening credits | works |
| Prologue | playable from the cell to the boat chase: cell, cafeteria, robbery, police chase (wanted level, minimap), boat, binoculars |
| Stability | 1 h 30 min+ in one session without a crash |
| Frame rate | 1-3 fps in the world (the first run is slower while shaders translate; they are cached after that) |
| Known problems | anything in shadow is black and bright scenes overexpose (the game's ray-traced lighting does not run yet); some blocky artifacts; boats sit low in the water |

## Requirements

- Windows 10/11 x64, a CPU with AVX2
- A Direct3D 12 GPU with shader model 6.6 and mesh shaders (NVIDIA RTX 20 series or newer)
- Rust (the version in `rust-toolchain.toml`)
- Several GB of free Windows commit memory (raise the paging file if `crashes.log` reports error 1455)

## Build

```powershell
cargo build --release -p victory
```

## Play

```powershell
powershell -ExecutionPolicy Bypass -File scripts\play.ps1 -Game "<your game folder>\<the game's exe>"
```

`scripts\play.ps1` keeps the shader cache and saves in folders you choose
(`-Cache`, `-Data`) and presses no buttons for you. Controller: XInput pads
(a DualShock/DualSense through DS4Windows works). With no pad connected the
keyboard acts as one while the game window has focus.

## Faster settings (optional, off by default)

`VICTORY_RELAY=2`, or `VICTORY_LATE_WAIT=3 VICTORY_WIDE_BACK=1` cut frame
time on the test machines. Set them in PowerShell before running the script.

## Debugging switches

| Switch | Effect |
|---|---|
| `VICTORY_CAPTURE=<dir>` + `VICTORY_CAPTURE_FRAMES=<list>` | save those frames as PNG (diagnostics build) |
| `VICTORY_KEYS=30:Enter,35:X` | press keys at those seconds (diagnostics build) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, report the commands in flight |
| `VICTORY_PIPELINE_WAIT_MS=24` | shorter per-frame wait for pipelines still being built |

Diagnostics build: `cargo build --profile diag -p victory --features diagnostics`

## License

GPL-3.0-or-later (see `LICENSE`). Based on VIctory (Marvisak) and victory-fork (fxceless).
