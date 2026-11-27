# Offline checks and user-run verification

Do not use an automated game launch. The user runs the comparisons below.
Keep the same save, camera, cache and workload for each comparison. Compare
world-only windows; menu FPS does not establish gameplay performance.

Offline results, test counts, expected negative PSO diagnostic and build hash
are recorded in [resource-pipeline validation](docs/resource-pipeline-validation.md).

## Exact ordered comparisons (user runs these)

These commands are for you to run from `A:\starport\port`, after the offline
build. Close each run before starting the next command. Use Series X for this
controlled sequence; Series S now has a two-plane compositor, but mixing that
change into the residency baseline would confound the comparison. Keep the
same save, camera and warmed shader cache, and collect at least 60 seconds and
120 presented world frames in the same scene. Cold eviction needs 90 frames;
at ~1 FPS this comparison needs over two minutes. The runner records flags
in each run's settings/evidence.
This baseline deliberately disables the new optimizations; conservative dirty
tracking off can be slower than the previously measured 1.3–1.4 FPS build.

For a reproducible `0x887A0005` device removal whose underlying reason is
`0x887A0001`, rerun the same profile once with the CPU debug layer and DRED:

```powershell
.\Play-Starport.cmd --preset balanced --d3d12-debug --dred
```

This is deliberately not a performance run. `VICTORY_D3D12_DEBUG=1` captures
the first invalid API/resource-state message; `VICTORY_D3D12_DRED=1` records
the last GPU breadcrumbs and page-fault allocation names. The runner also
forwards these environment variables for manual combinations. Do not enable
GPU-based validation unless ordinary debug+DRED still produces no command:
`VICTORY_D3D12_GBV=1` is substantially more intrusive.

```powershell
Set-Location A:\starport\port
Remove-Item Env:STARPORT_SKIP_TOP_MIP -ErrorAction SilentlyContinue
$env:STARPORT_SERIES_S = '0'
$env:STARPORT_TELEMETRY = '1'
$env:STARPORT_WAVE64_TRACE = '1'
$env:STARPORT_AMBIENT_TRACE = '1'
$env:STARPORT_WATCHDOG = '1'
$env:STARPORT_WATCHDOG_SECONDS = '10'
$env:STARPORT_WATCHDOG_STACKS = '0'
$env:STARPORT_MEM_BUDGET_MB = '0'
$env:STARPORT_MAX_RES = '0'
$env:STARPORT_SKIP_MIPS = '0'
$env:STARPORT_SKIP_MIPS_MIN_DIM = '1024'
$env:STARPORT_FALLBACK_COLOR = 'black'
$env:STARPORT_RESIDENCY = '1'
$env:STARPORT_LEGACY_RESIDENCY = '1'
$env:STARPORT_VRAM_TARGET_PCT = '85'
$env:STARPORT_DIRTY_TRACKING = '0'
$env:STARPORT_REBAR_UPLOAD = '0'
$env:STARPORT_FRAMES_IN_FLIGHT = '1'

# A: explicit legacy baseline (balanced preset held fixed throughout).
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6

# B: same baseline + resolution ceiling.
$env:STARPORT_MAX_RES = '1920x1080'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6

# C: B + omit two eligible streaming mips.
$env:STARPORT_SKIP_MIPS = '2'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6

# D: C + missing-SRV color diagnostic. Compare the same black surfaces.
$env:STARPORT_FALLBACK_COLOR = 'grey'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6

# E: D + descriptor-patched reclamation.
$env:STARPORT_LEGACY_RESIDENCY = '0'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6

# F: E + fine buffer pages / changed texture subresources.
$env:STARPORT_DIRTY_TRACKING = '1'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6

# G: demand readback is DEFERRED. Keep F's readbacks; no demand flag exists.

# H: F + bounded presentation flights (guest CPU outputs stay synchronous).
$env:STARPORT_FRAMES_IN_FLIGHT = '2'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6
```

For timing after collecting diagnostics, set both trace flags to `0`, restore
fallback color to `black`, and repeat both sides of whichever A/B pair matters.
Keep telemetry on. Logs are under `A:\starport\runtime\starport\runs\<stamp>`.
Use `starport::frame` and the world-only interval, not the runner's all-frame
summary, for median/p95 frame time. Check `STARPORT_LOG_LOSS` and sequence gaps;
discard comparisons with missing evidence. No game run was performed by Codex.

| Comparison | Check in console.log / system counters | Success signal requiring your run |
| --- | --- | --- |
| A → B | `starport::display`, reported mode, ambient target shape | Report/window/present ceiling <=1080p; smaller internal targets only if title honors it |
| B → C | `starport::mips`, rebased sizes, upload/live texture bytes | Lower allocations/uploads, intact textures; metadata tests alone cannot establish all visuals |
| C → D | `starport::fallback`, fallback writes, same black surfaces | Surface changes towards grey support a missing-albedo hypothesis |
| D → E | evicted/patch/pinned counters, waiting categories, live bytes, local/shared DXGI memory | Actual `evicted_textures > 0`, due backlog falling, live bytes below target; patches alone are insufficient |
| E → F | top upload entries/reasons, repeat bytes, preparing time | Fewer redundant bytes and lower preparing ms without corruption |
| F → G | Copy-back attribution | No reduction claimed: demand paging is not implemented |
| F → H | `frame_limit`, `frames_in_flight`, native `flights`, fence wait/copy-back phases | Bounded reuse, no stale CPU results; hazards may keep depth at one |
| Final scene | World frame intervals and visuals | Target <=200 ms / 5+ FPS and readable skin/room; unverified offline |

Optional, separate A/B pairs after H (revert each before trying the next):

```powershell
# Smaller reported pools, retaining Series X identity.
$env:STARPORT_MEM_BUDGET_MB = '8192'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6
$env:STARPORT_MEM_BUDGET_MB = '0'

# Optional CPU-visible VRAM upload heaps. Can increase local VRAM pressure.
$env:STARPORT_REBAR_UPLOAD = '1'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6
$env:STARPORT_REBAR_UPLOAD = '0'

# Separate Series S presentation check (not part of the Series X residency A/B).
$env:STARPORT_WATCHDOG_STACKS = '1'
.\Play-Starport.cmd --console series-s --preset throughput --ambient 0.6
$env:STARPORT_WATCHDOG_STACKS = '0'
```

For that Series S check, the first valid two-plane frame should log
`display output: compositing Series S scene and interface planes`. Verify both
the world and interface are visible, the overlay alpha is correct, and motion
does not bounce between plane sizes. Absence of the line means the title did
not provide a valid second plane; an integer-only or denied-SRV overlay logs a
fallback and intentionally presents the scene plane alone.

After at least one run with the default `VICTORY_PIPELINE_BOOK=1`, the following
CLI-only step precompiles only pipelines a prior run actually drew with. It
does not launch the title and resumes after interruption:

```powershell
$env:VICTORY_CACHE_DIR = 'A:\starport\runtime\starport\cache'
$env:VICTORY_DXIL = 'A:\starport\compat\tools_bin\dxc-1.9.2609\bin\x64\dxil.dll'
$env:VICTORY_PC_DXC = 'A:\starport\compat\tools_bin\dxc-1.9.2609\bin\x64\dxc.exe'
A:\starport\compat\starport_build\diag\victory.exe warm --used --jobs 8
```

Use the same backend build and driver for the next run. Set
`VICTORY_PIPELINE_BOOK=0` to measure the small recording path directly,
`VICTORY_PIPELINE_SLOW=0` to restore title creation order, or
`VICTORY_PIPELINE_WORKERS=1..64` to override the conservative runtime worker
count.

To bisect, undo only the last changed variable: MAX_RES=0, SKIP_MIPS=0,
FALLBACK_COLOR=black, LEGACY_RESIDENCY=1, DIRTY_TRACKING=0, or
FRAMES_IN_FLIGHT=1 respectively. RESIDENCY=0 disables both reclamation paths.
Remove MAX_RES to use the monitor default. Removing the N and alias settings
restores automatic mip selection. See each task below for scope and risks.

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
- `ambient_hits` counts the existing indirect-term override hook. The separate
  `starport::ambient` per-frame diagnostic identifies its first output binding. It does not establish
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
If initialization has submitted work but cannot confirm completion, it retains
that submission's objects/event and disables further color initialization for
the process. This bounded exceptional retention avoids freeing in-flight data.

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

When the local-card admission line is full, the bindless texture walk now
holds up to 1,025 would-be new allocations and continues through later queued
slots instead of stopping at the same 257th slot every frame. This is a
fairness pass, not a larger budget. An already allocated texture may still be
refreshed because that cannot create another host texture; slots held only by
pressure wake immediately when room returns. Telemetry exposes
`full_card_held_slots`, `full_card_reuploads` and `full_card_woken_slots`.
Set `VICTORY_FULL_CARD_PASS=0` and `VICTORY_REUPLOAD_WHEN_FULL=0` separately to
restore the old behavior for an A/B. A successful run should show the due
backlog rotating/falling and nonzero full-card refreshes without higher live
texture bytes.

Offline integration coverage patches a real descriptor, checks requeueing,
keeps a resource alive through CPU and recorded-list ownership, then releases
it after reset and rejects an obsolete allocation generation. LRU ordering,
range overlap and fence-age eligibility have focused tests.

## Task 6: fine dirty tracking and optional ReBAR uploads

For the prior-stage baseline set `STARPORT_DIRTY_TRACKING=0`; then compare
`1` (new default). Private write-watched allocations retain individual 4 KiB
page stamps instead of folding them into 64 KiB copies. Mapped-section guards
remain 64 KiB and deliberately mark all 16 constituent pages dirty; writes
through another alias still require the full-content texture hash. Existing
remap/no-wait safety checks and immutable per-submission snapshots remain.
Disabling dirty tracking uses conservative whole buffer refreshes and full
texture uploads. It is a correctness-preserving, potentially slow baseline.
Four-KiB journaling also increases per-page bookkeeping; compare preparing
time even if the uploaded-byte count falls. An offline pass is not a speed-up.

Texture hashes belong to the host allocation, survive staging recreation and
select only changed mip/array subresources. Selected footprints are packed
into one immutable upload buffer, retaining original destination subresource
indices. CPU staging is then released. A changed mip is still detiled/copied
as a whole mip; this does not implement arbitrary BC tile updates. Source
hashing/detiling can still cost CPU time. Unique-upload telemetry now also
belongs to the allocation rather than a temporary staging object's lifetime.

Check actual upload bytes/reasons in `starport::resource` and repeat bytes in
`starport::telemetry`; legacy "checked/staged" counters can remain high even
when fewer bytes are uploaded. A synthetic GPU/readback regression verifies
one changed byte reaches mip 1 while unchanged mips and recreated staging
produce no extra upload. Native write-watch coverage verifies a dirty 4 KiB
page does not refresh its 64 KiB neighbors; movie snapshot coverage remains.

`STARPORT_REBAR_UPLOAD=1` optionally selects CPU-visible GPU upload heaps;
default/kill switch `0` uses ordinary UPLOAD heaps. Support is queried through
D3D12 OPTIONS16, not assumed from the GPU model. Allocation also needs local
budget headroom; failure falls back to ordinary upload. Check the capability
log (`requested`, `supported`). This only changes upload-buffer placement;
it does not expose shader-written guest buffers directly or eliminate every
copy. Test it separately because it consumes the same limited local VRAM.
See [Microsoft's GPU upload heap specification](https://microsoft.github.io/DirectX-Specs/d3d/D3D12GPUUploadHeaps.html)
and [write-watch reset semantics](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-getwritewatch).

## Task 7: demand readback remains deferred

There is no working `STARPORT_READBACK=demand` switch in this build. Retain
the existing readback behavior for every comparison. The new attribution
shows explicit texture copies, dirty-piece fetches and buffer merge scans,
but cannot prove that an unobserved native guest CPU read never happens.
See [the read-barrier requirements](docs/demand-readback.md). No lower
copy-back byte count or demand-paging correctness is claimed from this task.

## Task 8: bounded presentation flights

Use `STARPORT_FRAMES_IN_FLIGHT=1` through the preceding comparisons, then
compare `2` (new default; accepted range 1..4). One forces synchronous legacy
submission even if older relay preset flags are set. The new path reuses the
inherited queue, fences and landing worker. Presentation slots cannot grow
beyond N and are reused only after their completion/landing watermark. Resize
settles flights and releases retained swapchain resources first.

This is deliberately limited: submissions with guest buffer shader outputs,
explicit CPU texture readbacks or unknown host callbacks still complete
synchronously. Without task 7's CPU read barriers, deferring those copy-backs
would expose stale native CPU reads. Pure output/presentation work can fly;
the single direct queue retains GPU ordering. A separate copy queue is not
introduced for aliasing resources without proven cross-queue hazards.

Check `frame_limit`, `frames_in_flight` (pending presented frames) and `flights`
(submission flights, a different count) in telemetry. Seeing fewer than N
frames pending is normal under hazards, diagnostics/captures, queue pressure or
a slow CPU. This does not promise fully parallel preparing/wait/copy-back for
the world's large shader-written buffers. Unit tests cover bounded slot reuse;
a headless GPU test sends owned copies asynchronously and checks their bytes
only after ordered completion. The movie regression remains passing.

## Task 9: Series S diagnosis and an X-identity memory limit

The watchdog is armed only by the user's normal game-run command, before
starting the title. `STARPORT_WATCHDOG=1` is the default; `0` disables it.
`STARPORT_WATCHDOG_SECONDS` defaults to 10 (range 1..600). Without a present,
`starport::watchdog` reports native/host fence watermarks, active submission
phase/age, pending pipelines, held queues, upload-resource counts and up to
200 recent phase/dispatch/SRV/UAV/fence events. Contended history events are
dropped and counted rather than delaying renderer threads.

Optional `STARPORT_WATCHDOG_STACKS=1` requests the loader's existing stack
dump helper, including title/main threads; default `0` avoids periodic thread
suspension. The helper preallocates unwinding storage, resumes each thread
before printing, and reports `module+rva`. No-present diagnostics cannot detect
a continuously presented black image. Series S presentation now validates both
retail plane records (72-byte stride), scales the scene and interface planes to
one output, and alpha-composites the premultiplied interface on the GPU. Invalid
overlay resources fall back to the scene plane. Offline tests cover
typed/typeless view selection, refused integer-only resources, scaling,
blending and state restoration; only the user's run establishes title-specific
correctness.

For a separate comparison, keep `--console series-x` and set
`STARPORT_MEM_BUDGET_MB=8192`. Unset/`0` disables the override. Accepted
values are 2048..65536 MiB, rounded down to 2 MiB and capped to the selected
profile. Only reported title pool budgets change; X/S identity and real host
DXGI VRAM budgets remain independent. The smaller total is split proportionally
between X's bandwidth classes, so DXGI adapter memory, working-set statistics,
GlobalMemoryStatusEx and transfer accounting use one consistent split. It is
not a hard host allocation cap and does not force resolution. Usage is not
fabricated or clamped to fit; a preallocated title pool can exceed a newly
reported limit. Compare boot and streaming behavior before calling it usable.

## Task 10: guarded compute dataflow

`STARPORT_WAVE64_TRACE=1` is the default; `0` disables the additional binding
trace. `starport::refused` now distinguishes direct and indirect recording
attempts per sampled frame. `starport::wave64` identifies each of the four
hashes, native resource-use metadata, statically resolved potential UAV
outputs, unresolved bindings and later bound potential consumers. Nothing
executes these refused shaders. Dynamic indices and object aliases are not
fully observable, so missing edges cannot prove that an output is unused.

See [the per-shader emulation design](docs/wave64-emulation.md). Its four PSO
failures remain expected; neither Wave32 forcing nor groupshared emulation is
enabled. Binding diagnostics can add CPU work while enabled; use the kill
switch for timing comparisons after collecting the needed evidence.

## Task 11: ambient hook reach

`STARPORT_AMBIENT_TRACE=1` defaults on; `0` disables this diagnostic independently
of telemetry. Each presented frame logs `matched_resolves`, `override_hits`,
the first `(shader hash, output object, format, view dimension)` tuple,
root/table offset and current ambient level. Zero hits are explicit. Multiple
resolve bindings count separately, but only the first binding is retained for
the frame, keeping memory and formatting bounded. Counts describe recorded
override work associated with a present, not GPU timestamps or a readback of
the resulting pixels. `VICTORY_AMBIENT` / `--ambient` behavior is unchanged;
neither the shader nor `ambient.rs` was altered by this diagnostic.

`downstream_albedo_multiply="unknown"` is intentional. The override writes a
constant indirect term to its matched UAV. This hook has no proof of the later
bindless diffuse/albedo consumer or its multiply. Grey fallback A/B evidence
can distinguish some missing-SRV cases, but cannot prove a working diffuse
composite or ray tracing. Use the trace kill switch for timing once collected.

## Black-world target capture

`Play-Starport.cmd --capture` saves the scene plane, interface plane and final
composition as `frame-N-scene.png`, `frame-N-interface.png` and `frame-N.png`.
To trace a scene that is already black before composition, use
`Play-Starport.cmd --capture --capture-targets`. This also saves intermediate
render, depth and UAV targets on the selected world frames (+2, +6, +30 and
+80 after the first frame with at least 500 host draws). Target filenames
identify the frame, index and resource used in the matching `frame-N-ops.txt`.
Small float targets log their channel values immediately before their PNG;
depth targets log stencil occupancy and R32_UINT targets log packed-light
occupancy. End-of-frame target dumps can show reused/overwritten allocations;
the operation trace is still needed to interpret their producers and readers.

Both capture modes synchronize GPU work and write images. Their FPS is not a
performance benchmark. Target dumps add substantial time and disk use; they
are disabled in ordinary runs.

## Upscaler settings and ACCC kernels

The launcher defaults to `--fsr preserve`. An explicit `--fsr quality-zero` changes only the saved
`graphics/fsrQuality` value to `0` in existing title settings under the selected
`--data` directory. It first stores an exact-byte, content-addressed
`settings.xml.starport-fsr-<hash>.bak` beside the file, then replaces settings
atomically. Formatting, resolution, texture settings and saves are preserved.
Malformed or ambiguous settings abort the launch. A new profile without any
settings emits a warning because the preset cannot be applied yet.

`--fsr preserve` performs no settings writes; it does not undo a previous
`--fsr quality-zero` (or its legacy alias `off`). For an A/B, restore the exact backup printed by the first launch
and pass `--fsr preserve`. The launcher records the mode, previous/new values
and backup path in `launch.json` and `summary.json` as `title_settings`.

Quality zero still issues ACCC compute dispatches in captured title frames;
it is not a reliable way to disable upscaling. The full ACCC/FSPX translation
path now passes all 29 locally recorded containers through DXIL validation
and NVIDIA hardware PSO creation. That offline check is not visual acceptance.
See [the updated capture findings](docs/black-world-20261127.md).

`STARPORT_TEXTURE_STAGE_MS=16` limits the opportunistic bindless texture pass
to a soft 16 ms slice (1..1000; unset/invalid retains the prior 250 ms).
It completes its current texture, leaves unprocessed slots queued and keeps
explicit table reads synchronous. The frame logger reports streaming time
inside the heap-buffer total; these two values must not be added together.
The throughput preset does not yet change this default.

`VICTORY_WINDOW_KB=256` and `VICTORY_WINDOW_TAILS=1` expose the existing finer
buffer-window path for memory A/B tests. Sized reads keep their complete
ranges; unsized root reads retain the full tail of their 4 MiB cell. Defaults
remain unchanged until a runtime comparison establishes correctness.

## CPU descriptor copies

`VICTORY_HEAP_SYNC=1` (default) discovers descriptor records copied directly
into shader-visible heaps. Those writes bypass descriptor API entry points.
The pass uses the existing shared dirty-page journal, keeps a separate cursor
per heap and compares kernel-read snapshots of the three identity words.
Untracked allocations are conservatively scanned whole. Failed reads remain
pending; capped-out title slots never overwrite host dynamic-table slots.
Synchronization runs before the bindless-buffer/texture pass and submission.
`VICTORY_HEAP_SYNC=0` restores the prior path for A/B diagnosis.

`starport::descriptors` logs pages scanned, changed/mirrored records and CPU
time per frame. This timing overlaps preparing and heap-buffer timing.
No executable store hooks or reduced-frequency fallback are used.

Runtime-owned descriptor memory uses a private `MEM_WRITE_WATCH` reservation.
The first synchronization scans it once; later passes query dirty pages and
retain independent cursors in the shared journal. Allocation failure falls
back to ordinary memory plus conservative full scans, reported explicitly.
Placed heaps retain their title-owned backing.

## Exact buffer refresh ranges

`STARPORT_LIGHT_REFRESH=1` (default) remembers the last and largest exact
refreshed ranges. A refreshed partial page does not initialize its neighbors.
Worker jobs can be skipped only when a published submission query proves
every byte already current, and no in-flight shader write overlaps the range.
Real guest writes still force refreshes. `0` restores the older refresh path.
`starport::refresh` reports skipped jobs and their byte spans, not saved time.

`VICTORY_CARD_WINDOWS_MB=512` enables the existing GPU-local copies of loose
title-memory windows for an A/B. Unset/`0` keeps the old location. The parser
accepts 0..4096 MiB; invalid values disable it. Admission also requires the
existing host capability checks and 20% local-budget headroom. Restart to
compare: each shadow chooses its location at creation. The CPU staging side
remains allocated, so shared-memory usage does not disappear by this switch.

## Captured shader checks

Whole-wave mask simulation preserves both Wave64 mask halves and the FI/BC
rules of `v_permlanex16`. Tests cover inactive sources, preserved destinations,
high-half clobbers, partial restores and an executed NVIDIA Wave32 prefix sum.
Source: Marvisak/VIctory #3393 at `9be9a09724925900670e4b954d64a1a0f31d441c`.

Pixel signature rows retain console element order to match DXIL element IDs.
Unread packed attributes inherit the used sibling's interpolation mode only
when used modes agree; used attributes are never flattened to hide conflicts.
This is the narrow signature portion of #3408 at
`5696ced75b4a6eb494861daac57ff17f9913f6aa`, with conservative interpolation.

The ignored `captured_world_pixels_validate_offline` test requires external
`STARPORT_PIXEL_INPUTS`, an explicit `STARPORT_PIXEL_EXPECTED_COUNT`, and
`STARPORT_SHADER_AUDIT_OUT`. It asserts every fixture translates as a pixel
shader and validates. DXIL dumps stay outside Git. This is not a graphics
pipeline or rendered-pixel equivalence test.

The launcher now uses the release backend by default. `--capture`,
`--d3d12-debug` or `--dred` select the diagnostics backend automatically;
an explicit `--backend` always wins. Their executable fingerprints differ,
so shader warm-up must use the same backend that the run will use.

The 2026-11-27 D3D12 debug run exposed two binding errors. Presentation
tables/constants/samplers now have pixel visibility, matching their read
states (message 538). Guest pipeline root signatures derive direct heap
flags from each emitted SFI0 feature mask, so resource-only compute does
not require an absent sampler heap (message 1419). Both hardware debug
regressions failed before their fixes and passed afterward. These are
binding correctness results; they do not establish correct world lighting.

Buffer descriptors read only for their validity/NUM_RECORDS field now receive
that field from the original HLE SRV/UAV declaration. Structured and typed
views retain element counts; raw views count bytes. Null views return zero,
overflow is rejected, and the console format word remains unsupported rather
than borrowing unrelated host bits. No buffer upload is required for this
metadata. A mandatory NVIDIA dispatch regression checks zero and nonzero
branches against CPU results.

The ignored `captured_depth_reduce_shader_validates_and_creates_pipeline`
test requires external `STARPORT_DESCRIPTOR_COUNT_FIXTURE`. The recorded
`ceb5eb68cd99cb07` kernel now translates, validates and creates an NVIDIA PSO.
Disassembly identifies depth/min reduction, not an RT-builder kernel. This
fixture check does not execute the kernel with a complete game's inputs.

An exact wave-wide branch retry is also enabled for compute kernels still
requiring Wave64 when their group contains exactly one 64-lane guest wave.
The existing branch proof and multiple-guest-wave rejection remain in place.
`VICTORY_WAVE_WIDE=0` disables it for A/B; no global WaveSize clamp is used.
The recorded `369cbe906ea72764` culling kernel fails NVIDIA PSO creation
with the retry disabled and passes with it enabled, with approximate
local-wave lowering disabled in both checks. A hardware LDS regression
checks branch results at both sides of lane 32. This is not whole-scene
equivalence or approval to widen the 128/1024-thread failing kernels.
