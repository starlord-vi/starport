# Offline checks and user-run verification

Do not use an automated game launch. The user runs the comparisons below.
Keep the same save, camera, cache and workload for each comparison. Compare
world-only windows; menu FPS does not establish gameplay performance.

## Task 1: attribution baseline

From PowerShell in the source repository:

```powershell
$env:STARPORT_TELEMETRY = '1' # default on; 0 disables the new collector
.\Play-Starport.cmd --console series-x --skip-top-mip
```

Every roughly two seconds of presents, `console.log` includes:

- `starport::telemetry`: one sampled presented frame's CPU phase times,
  actual outstanding submission flights, live texture allocation bytes,
  waiting descriptor slots, residency evictions and cumulative descriptor
  publications. Publication count includes normal writes, not only eviction.
- `starport::resource`: top 20 upload/readback attribution entries for that
  sampled frame, identified by allocation ID, guest address, dimensions and
  format (format 0 denotes an untyped buffer). A mixed first/repeated upload
  can occupy two reason entries. Overflow bytes are explicitly counted when
  more than 4,096 attribution entries occur in one frame.
- Upload totals distinguish first upload, dirty reupload and initial mip
  rebasing; demotion is reserved for the later resource replacement path.
  Unique bytes mean union of byte offsets copied at least once per host
  allocation, not content hashes or unique pixel values. Texture values
  include copy-footprint padding. Buffer values count CPU-shadow refresh
  traffic; this does not imply every byte crossed PCIe in a GPU upload.
- Readback reasons distinguish explicit guest texture-to-buffer copies,
  CPU merge scans of shader-written buffers and fetched GPU dirty pieces.
  Scan bytes are not PCIe bytes or necessarily bytes written back to RAM.
- `starport::refused` counts guarded compute *recording attempts*, not shader
  execution. Recording can precede submission; zero counts do not prove that
  all indirect dispatches are safe or implemented.
- `ambient_hits` counts the existing indirect-term override hook. Debug-level
  `starport::ambient` identifies its output resource. It does not establish
  a downstream albedo multiply or prove the diffuse composite is correct.

The per-frame bucket is reset on every present; only a sample is logged.
Phase measurements are CPU wall time, including fence waits, not GPU execution
timestamps. The existing log-loss counters must remain zero for comparisons.

## Task 2: resolution

Set `$env:STARPORT_MAX_RES = '0'` for the legacy uncapped baseline, then
`$env:STARPORT_MAX_RES = '1920x1080'` for the ceiling comparison. Removing
the variable selects the primary monitor's current mode. The runner forwards
this setting. Check `starport::display`, `display mode reported to the title`
and `presenting to a window`: reported/enumerated mode, top-level window
proposals and swapchain buffers are bounded. Oversized output is GPU-scaled,
not cropped or copied into an incompatible resource. Source render targets
remain title-owned: a smaller reported display does not prove the title
actually chooses smaller internal targets. Look for ambient target dimensions
and live-memory reductions in the user-run logs before claiming that effect.

The downscaler's shaders/PSOs compile offline. A synthetic four-quadrant test
checks actual GPU output, at two scale factors. Each scaled
present owns its descriptors until completion; resize first
settles outstanding flights and releases cached back-buffer references.
The blit preserves the full image because [D3D12 CopyResource requires matching
dimensions and does not stretch](https://learn.microsoft.com/en-us/windows/win32/api/d3d12/nf-d3d12-id3d12graphicscommandlist-copyresource).

## Task 3: streaming mip selection

Use `--skip-mips 0` in the baseline, then `--skip-mips 2`. The new ceiling and
all other settings must stay fixed during that comparison. The equivalent
environment variable is `STARPORT_SKIP_MIPS=0..12`; `0` is the kill switch.
Unset defaults to 2 when the display ceiling is at most 1920x1080, otherwise
0. `STARPORT_SKIP_MIPS_MIN_DIM` defaults to 1024. Only synthetic, placed,
non-MSAA BC 2D textures/arrays/cubes with CPU-owned contents qualify; actual
title resources and GPU-written outputs keep their allocation. At least three
mips remain, even if N exceeds the available chain.

`STARPORT_SKIP_TOP_MIP=1` and `--skip-top-mip` remain aliases for N=1; their
`0`/`--no-skip-top-mip` forms disable omission. Explicit `--skip-mips` wins,
then an explicit CLI alias, then the N environment setting, then the alias
environment setting, then automatic selection. The launcher no longer inserts
an implicit `STARPORT_SKIP_TOP_MIP=0`, which would mask the new default.

Check `starport::mips` for chosen N/threshold and `mip_rebase_bytes` plus host
dimensions in `starport::resource`. Source array strides remain unchanged;
uploads choose source mip `host_mip+N`. Typed GetDimensions uses the rebased
host SRV, while size-only descriptor reads use the same retained range in the
metadata sidecar. Array/cube layer counts remain unchanged. No shader bytecode
is patched to pretend unsupported descriptor fields exist; unsupported reads
still report a translation failure. A unit test checks metadata at N=2 and
tail preservation. This does not establish correctness of every title shader's
hard-coded texel coordinates; compare visuals in the same world scene.

## Task 4: missing-texture colors

Hold the preceding settings fixed; set `$env:STARPORT_FALLBACK_COLOR = 'grey'`
(also `magenta`, `white`, or `black`). Default/kill switch `black` preserves
the old typed null behavior. The runner forwards this setting. Watch
`starport::fallback` and `cumulative_fallback_descriptor_writes` in each
telemetry sample. If a previously black surface changes to the diagnostic
color, that supports a missing-SRV hypothesis for that surface. It does not
prove the later lighting composite or RT is correct.

Only missing/waiting SRVs with a known valid texture dimension are colored.
Buffers, UAV writes, invalid array counts and MSAA views lacking their sample
count keep typed nulls; redirecting UAV writes into a shared debug texture
would introduce data races. Float, uint and sint SRVs use compatible numeric
formats; integer grey is converted to that integer format. Normalized sampling
gets a constant, but out-of-bounds integer texel loads can still read zero.
The small immutable fallback cache is capped at 64 shape/format variants;
unsupported cases log a warning and retain a typed null. Initializing it uses
a private queue/fence so descriptor publication cannot recursively acquire the
normal submission lock. GPU readback tests verify grey/magenta/white texels.

Actual draws *sampling* an indexed fallback cannot be counted by knowing that
a heap contains one: bindless indices are selected in shaders. No misleading
sample counter is emitted. The descriptor-write counter is cumulative and
does not represent live missing slots or sampled draws.

## Task 5: descriptor-patched reclamation

Baseline the earlier stages with `$env:STARPORT_LEGACY_RESIDENCY = '1'`.
Then set it to `'0'` (new default) while holding resolution, mips and fallback
color fixed. `STARPORT_RESIDENCY=0` disables reclamation; legacy `1` restores
the previous conservative Evict/MakeResident path. `STARPORT_VRAM_TARGET_PCT`
defaults to 85 (accepted range 50..95); admission and reclamation use the same
DXGI local budget threshold, with a five-percent hysteresis margin.

The new path does not Evict bindless resources. At an idle sweep under the
submission lock, it orders cold CPU-owned textures by last descriptor
publication/upload/creation, patches their known static descriptors to typed
nulls (or the selected debug fallback), and queues those slots for re-admission
after 30 frames. Ninety frames without such use makes a pressure candidate.
Known kernel writes and mapping changes invalidate overlapping shadows;
invalidations are prioritized even below budget. Ordinary content changes
still use the immutable snapshot/hash/remap protocol. The write journal is
bounded; exceptionally many changes may coalesce into a conservative range.

Backing is released only after all submissions complete and every recorded
list, dynamic slot and CPU lease lets go. A recorded list can be replayed even
after its last fence completes, so its resources remain pinned until reset.
Owned texture leases replace the old borrowed-pointer helpers. Staging is
generation-aware, preventing a new shadow from inheriting an old "uploaded"
state. No mip tail is synthesized here; the selected fallback is used instead.

Check `evicted_textures`, `eviction_patched_descriptors`,
`eviction_pinned_candidates`, `overwrite_invalidations`,
`demotion_reupload_bytes`, live texture bytes and waiting slots. Reclamation
success requires actual evictions and lower live memory; patched descriptors
alone do not prove memory was freed. Waiting counts also include unmapped or
partly streamed textures; inspect the separate queue categories. Counters can
rise before the next frame re-admits waiting textures. Existing local/shared
DXGI statistics include other allocations; shared memory is not by itself a
measurement of spilled textures. Lower shared usage and FPS gains require the
user's controlled world run, not an offline assertion.

Offline integration coverage patches a real descriptor, checks requeueing,
keeps a resource alive through CPU and recorded-list ownership, then releases
it after reset and rejects an obsolete allocation generation. LRU ordering,
range overlap and fence-age eligibility have focused tests.

Further comparisons will be added as their hazard checks are completed.
