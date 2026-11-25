# Second-pass verification

The game is never launched by Codex. Commands in this document are for the
user. Keep Series X, the same save, warmed cache, camera and world state.
The 19:07 UTC run had DIRTY_TRACKING=0: its uploads are conservative whole
refreshes, not proof of a dirty-tracking self-write loop. Compare against the
18:55/18:51 runs only after checking their actual settings and draw counts.

## Task 0: first collect reproducible evidence

From `A:\starport\port` in PowerShell:

```powershell
$env:STARPORT_SERIES_S = '0'
$env:STARPORT_MAX_RES = '1920x1080'
$env:STARPORT_SKIP_MIPS = '2'
$env:STARPORT_FALLBACK_COLOR = 'black'
$env:STARPORT_LEGACY_RESIDENCY = '0'
$env:STARPORT_DIRTY_TRACKING = '1'
$env:STARPORT_FRAMES_IN_FLIGHT = '1'
$env:STARPORT_MEM_BUDGET_MB = '0'
$env:STARPORT_TELEMETRY = '1'
$env:STARPORT_SUBPHASES = '1'
$env:STARPORT_MEMORY_EVERY_N = '1'
$env:STARPORT_BENCH = '1'
$env:STARPORT_WAVE64_TRACE = '0'
$env:STARPORT_AMBIENT_TRACE = '0'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6
```

In the fixed world scene, press **F8** once, then leave the scene/input fixed
for 30 seconds. F8 is consumed on the title/host window only in bench mode;
auto-repeat does not restart it. Press again after releasing to restart a
capture. The JSON is saved after the first present at/after the deadline.
Intervals crossing the trigger/deadline are excluded; no-present stalls can
delay completion. This does not lock the camera or automatically identify
gameplay. At very low FPS a 30-second sample has correspondingly few frames.

The runner defaults the report to `runs/<stamp>/bench.json`; a later capture
in that run replaces it. `STARPORT_BENCH_OUT` can override the destination,
which is checked against the asset policy before any write. Direct backend
use without the runner defaults to the process-specific temporary directory
report `starport-bench-<pid>.json`. No output is allowed inside a source tree.

| Flag | Default | Disable / scope |
| --- | --- | --- |
| STARPORT_SUBPHASES | 1 | 0 disables periodic timers/accounting when BENCH is also 0 |
| STARPORT_BENCH | 0 | 0 disables F8 capture; 1 collects timings even if SUBPHASES=0 |
| STARPORT_MEMORY_EVERY_N | 30 | 1..600 presents between DXGI/system/process queries; use 1 for these slow world comparisons |
| STARPORT_BENCH_OUT | runner's per-run bench.json | Optional output override, outside all source trees |

`starport::subphases` reports median/p95 microseconds of **per-present totals**
for hashing, actual write-watch kernel queries, title list recording/translation,
texture/card upload recording, host staging/buffer allocation and merge/scan.
It also reports preparing, CPU fence wait, copy-back and present-to-present
frame time. Recording can happen outside preparing; timers are inclusive
operation durations summed across workers and can overlap. They are not
mutually exclusive slices or GPU timestamps. An interval stores at most 8,192
samples and reports discarded capacity samples explicitly. Discard such
comparisons if capacity or log-loss counters are nonzero.

`starport::memory` reports tracked native allocation lifetimes (including COM
clones retained by lists), queried allocation sizes for texture/RT-depth,
buffers, upload and readback heaps, descriptor address-stride payload estimates,
and tracked PSO object count. Exact driver-private PSO/descriptor allocation
overhead is unavailable; `driver_pso_bytes` is JSON null, not a fabricated zero.
Main allocation helpers/hosted textures are covered; direct internal resources,
swapchain images and driver caches can add untracked bytes. DXGI local/non-local
Budget/CurrentUsage is the total process accounting, including those categories.
System physical total/available and process working set/private commit are
queried independently. Query failure is null. Check `memory_sample_frame`
when interpreting a snapshot with N>1.

Every ~10 seconds `starport::texture_histogram` reports tracked allocation
bytes by **host mip0** max dimension: <=64,128,256,512,1024,2048,4096,larger.
The first eight buckets are BC; the next eight are non-BC. This is allocated
memory, not guaranteed resident VRAM, and host mip0 may already be rebased.

The benchmark JSON contains per-phase median/p95, complete frame sample count,
draw recording count over those intervals, memory snapshots at capture start
and completion, and explicit overlap/unknown-byte metadata. F8 does not infer
that different scenes have equal work. Compare recorded draws as well as frame
times, and keep the same flags/cache for a controlled pair.

The remaining task comparisons will be added with their implementations.
