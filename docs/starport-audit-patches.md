# Renderer audit patch set, 25 November 2026

Targeted FPS and lighting changes for the user's next test. No game launch
or computer-use automation was performed. 30 FPS and scene appearance
remain unverified.

## Launch and comparison

Double-click `A:\starport\Play-Starport.cmd`. This uses the optimized
diagnostic build, INFO logging and keyboard/mouse controls, and runs until
you close the game window. F6 toggles mouse capture. Screen/audio capture
and D3D12 debug validation are disabled.

```powershell
python A:\starport\port\scripts\starport.py --seconds 0
python A:\starport\port\scripts\starport.py --seconds 0 --preset throughput
python A:\starport\port\scripts\starport.py --seconds 0 --ambient off
```

The default `balanced` preset enables checked nonblocking texture refresh,
adjacent-copy joining and parallel independent buffer refreshes.
`throughput` also enables relay mode 2 with its own implied stage, leaving
per-list GPU timing unset. `baseline` omits those preset overrides. Hold
the remaining settings fixed when comparing. The older explicit `--relay`
experiment separately selects late-wait stage 3 and wide copy-back.

Lighting defaults to a 0.6 constant indirect-light fallback with a warm
tint. This is not ray tracing. `--ambient off` preserves the title's
output; `--ambient 0` replaces it with zeros. `--material-depth late`
restores close-time material-depth publication for comparison.

## Changes

- CPU texture staging uses cacheable memory. Immutable GPU upload
  snapshots remain alive through GPU completion. This removes CPU reads
  from write-combined upload mappings. Detile slices respect the actual
  final-row allocation, including its omitted padding.
- The ambient helper follows a successfully issued, named screen-probe
  resolve with its translated root-1/176-dword UAV binding. Placed views
  use the existing descriptor resolver. It checks an R32_UINT,
  single-slice, non-MSAA 2D mip-0/plane-0 target and typed-store support.
  Each allocation owns an immutable descriptor instead of a wrapping ring.
  UAV barriers order the writes; guest PSO/root/heap state is restored.
- Compute-written console D16+stencil material twins publish after direct
  and indirect compute, before same-list lighting consumers. Existing
  stencil footprints and partial-write preservation remain. Each dirty
  version publishes once. Other depth families keep their prior path.
- Pixel quad-local swizzles/DPP use helper-safe quad reads. Ordinary pixel
  first-lane reads give helper lanes their own value. Existing waterfall,
  NVIDIA two-half/wave lowering, meshlet inputs and mip gathers remain.
- The submission-lock timer measures lock acquisition, excluding later
  settling, descriptor work and sweeping.

## Recorder

Each launch has a separate directory in `A:\starport\runtime\starport\runs`;
`latest.json` names it. `console.log` contains renderer phase summaries,
pipeline failures, lighting-hook confirmation, streaming waits/deferrals
and errors. Per-present records contain frame IDs, wall-clock intervals,
and issued draw/dispatch deltas. On exit, `frames.csv` and `summary.json`
include median/p95 and average FPS, with a separate last-30-second window.
That window is not automatically classified as gameplay. Host fence-wait
time is not GPU execution time.

Logging uses a bounded 1,024-event background queue, a 64-KiB buffer and
periodic flushes. Render threads do not wait for log disk I/O. Individual
events are capped at 16 KiB. `STARPORT_LOG_LOSS` reports queue drops and
truncation; those counters and frame-sequence gaps appear in the summary.
Orderly suspension flushes logs. An abrupt crash/power loss can lose the
last buffered interval. This records high-level diagnostics; expensive
captures and per-draw traces remain disabled.

## Sources and scope

The supplied renderer audit and Belobug diff were reviewed against the
local tree and `Belobug/VIctory-Belobug-AMD` at
`71652a2c15a0da68cd801890e1f71109670f9ca5`. The adapted ambient module and
pixel helper rules retain GPL-3.0-or-later provenance. Early material
publication uses Starport's existing twin/state machinery.

Single-resource RGB9E5 casting, resident-mip resource variants, physical
alias propagation, clamp hoisting and broader residency changes are
conditional proposals, not a ready combined patch. AMD-specific backing,
wave settings and external shader swaps were not imported. Existing
multi-mip/layer packed-light copies remain available.

Dividing a 1,730-ms frame by 3,864 draws does not establish a 440-us cost
inside every draw function: streaming waits, submission and copy-back are
also in that frame. The recorder separates these costs before a broad
lock/descriptor/allocation rewrite.

Validation is intentionally limited to compilation, focused logger,
shader, texture and material checks, Clippy and the required asset-policy
check. User gameplay testing supplies performance and visual acceptance.
