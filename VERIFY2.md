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
$env:STARPORT_SKIP_MIPS_TARGET_DIM = '0' # preserve fixed-N pass-1 baseline
$env:STARPORT_PROGRESSIVE_TEXTURES = '0' # pass-1 admission for the baseline
$env:STARPORT_EVICTOR_V2 = '0' # pass-1 descriptor-patched eligibility
$env:STARPORT_FALLBACK_COLOR = 'black'
$env:STARPORT_LEGACY_RESIDENCY = '0'
$env:STARPORT_DIRTY_TRACKING = '1'
$env:STARPORT_SELFWRITE_FILTER = 'off' # preserve pass-1 refresh behavior
$env:STARPORT_PARALLEL_WORK = '0'
$env:STARPORT_XXH3 = '0'
$env:STARPORT_RETAINED_MIP_READS = '0'
$env:STARPORT_TRIM_CACHES = '0'
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

## Task 1: real mip tails, then promotion

Hold the baseline scene/flags fixed and add:

```powershell
$env:STARPORT_PROGRESSIVE_TEXTURES = '1'
$env:STARPORT_TAIL_BUDGET_MB = '512'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6
```

Repeat F8 in the same scene. Progressive mode defaults to 1; 0 restores
pass-1 admission. Tail budget defaults to 512 MiB (0 disables progressive
admission; maximum 4096). Only CPU-owned synthetic placed BC 2D/array/cube
views qualify. GPU-written targets, uncompressed resources and unsupported
layouts keep the existing path. A source truncated before a <=64px tail with
three mips remains ineligible; small <=64px chains retain their source mips.

The initial resource contains the smallest four available mips, or three when
needed to reach <=64px. Only source pages containing their actual BC blocks
are read, under the existing nonblocking mapping lock; missing pages leave
the slot waiting. Pixels cannot be recovered from completely unmapped data.
No all-zero fill is advertised as a recovered tail. A real immutable snapshot
is retained until its upload and CPU staging is released afterward. Native
allocation bytes, including committed-resource alignment, reserve the tail
budget before allocation; a tiny tail can still cost 64 KiB on the GPU. A COM
lease keeps that reservation until the resource's final native release.

Promotion runs at an idle submission boundary, newest descriptor-use frame
first, then smaller target allocation, then newer creation. It requires local
DXGI headroom for the full target in addition to the old tail. At most eight
attempts and about 16 ms of sorting/query/copy work are budgeted per frame;
the final initialization operation can exceed that soft time limit. A failed
attempt retries after two seconds. Missing full source mips, dynamic descriptor
slots, CPU leases or recorded-list references postpone promotion.

Static descriptor patches are transactional: failed replacement restores the
real tail. A successful full image is initialized on a serialized private
queue/fence before descriptors are republished, so previously recorded lists
cannot sample uninitialized promoted pixels. Ordinary pressure eviction
excludes tails; known source overwrite invalidation can still release them.
Per-allocation upload hashes suppress a duplicate full upload after private
initialization. SRVs and size metadata use the actual effective mip generation.

Check `tail_bytes`, `tail_allocations`, `promoted_textures`,
`unavailable_tail_attempts`, `promotion_upload_bytes`, waiting queue categories,
local/non-local DXGI usage and the F8 phase summary. Tail allocation count
describes backing, not proof a particular draw sampled it. Initial tail uploads
are included in mip-rebase/first-upload attribution; promotion has its own
reason. The offline integration test covers partially mapped source pages,
native lease rejection with descriptor rollback, successful GPU promotion,
matching size metadata and exact raw BC readback bytes. Lower backlog, real
blurry surfaces and frame-time changes still require the user's run.

## Task 2: stop pressure-driven churn

```powershell
$env:STARPORT_EVICTOR_V2 = '1'
$env:STARPORT_EVICT_MIN_BYTES = '2097152'
$env:STARPORT_EVICT_MIN_RESIDENCY_MS = '2000'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6
```

V2 defaults to 1; 0 restores pass-1 eligibility while retaining the selected
progressive setting. Local pressure was already driven by **total DXGI local
CurrentUsage/Budget**, including render targets, buffers and driver allocations;
it is not based on the live-texture subtotal. The target remains 85% with
five percentage points of hysteresis. Category counters explain pressure but
are not subtracted to invent a different DXGI budget.

V2 excludes non-BC textures, allocations below 2 MiB, those uploaded/created
within two seconds, and a title texture after two re-uploads. MIN_BYTES accepts
bytes (0..4 GiB), MIN_RESIDENCY_MS accepts 0..60000; zero disables that particular
threshold. V2=0 disables all added filters. Re-upload count survives backing
replacement and counts dirty uploads after an allocation's initial upload;
the initial tail-to-full promotion does not count as churn.

Ordinary kernel writes still queue views for hashed refresh and do not force
excluded UI/movie resources through backing destruction. Explicit mapping
changes are separate hard invalidations; they bypass churn exclusions because
the old source mapping is no longer valid. All disposal retains the fence,
generation, recorded-list, dynamic-descriptor and CPU-lease checks. Failed V2
disposal restores unchanged descriptors transactionally rather than leaving a
fallback while recorded references pin the allocation.

Check `starport::eviction` for each completed release: reason, bytes, age,
re-upload count and patches. Reasons distinguish local pressure, guest writes
and mapping-generation changes. Check `eviction_skip_uncompressed/small/recent/
thrashing` and the existing actual eviction/pinned/queue counters. Skip counters
count policy decisions across sweeps, not unique textures. Every replacement
reads current guest pixels and validates content hashes; no evicted CPU pixel
cache is retained. A lower eviction count can be correct if it removes churn;
judge RAM/VRAM, due backlog and the same-scene bench together.

## Task 3: cap eligible streaming mip0 by dimension

Hold the preceding settings fixed and add:

```powershell
$env:STARPORT_SKIP_MIPS_TARGET_DIM = '1024'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6
```

An explicitly set target overrides fixed N. `0` disables the dimension rule
and restores fixed N. Values 16..16384 are accepted; malformed values disable
the dimension rule. With neither target nor an explicit legacy N/alias, the
new automatic target is 1024. An explicit `--skip-mips`, SKIP_MIPS or alias
keeps its previous meaning when TARGET_DIM is unset, preserving compatibility.
MIN_DIM remains an eligibility gate for both rules. Progressive tails remain
temporary smaller backing; promotion uses this same full-target calculation.

Only eligible CPU-owned placed BC images are rebased, with at least three
source mips retained. Short source chains can make the dimension target
unattainable: a bounded `starport::mips` warning identifies the shape, target
and actual first mip. SRVs and metadata report that actual shape rather than
pretending absent mips exist. Array/cube layer counts do not change. Native
typed GetDimensions sees the corresponding native SRV; size-only lowering
reads the same effective allocation generation, including progressive tails.
No translator behavior changed; unsupported descriptor operations still fail
with their shader diagnostic. Hard-coded title texel coordinates still need
visual comparison. Check target logs, live-byte histogram and the same F8 scene.

## Stage 4: memory budget and disposable staging caches

After task 3, compare these in order, restarting and using F8 each time:

```powershell
$env:STARPORT_TRIM_CACHES = '1'
$env:STARPORT_MEM_BUDGET_MB = '8192'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6
# Then repeat the identical scene with:
$env:STARPORT_MEM_BUDGET_MB = '6144'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6
```

TRIM_CACHES defaults to 1; 0 restores the previous cache caps independently
of the reported title budget. With MEM_BUDGET_MB unset/0 there is no new cache
limit. At 8192 MiB the idle CPU snapshot pool is capped at 128 MiB and idle
upload chunks at 64 MiB; at 6144 they are capped at 96/48 MiB. An oversized
snapshot is no longer exempt from that strict cap. Admission of optional kept
snapshots uses the same ceiling. Resources held by lists/fences are preserved;
upload chunks enter the pool only after the existing completion/reset path.

`starport::cache_budget` logs these limits and cumulative released bytes.
Those are deallocations, not a claim of equivalent immediate working-set or
DXGI reduction. Private allocator/driver retention remains measurable only
through memory telemetry. Lower caps may increase allocation/copy time.

The title budget queries remain consistent. Native physical sections already
reserve their full size and commit when mapped/written on Windows. **No new
guest commit cap or demand-commit fault handler is implemented**: a mapped or
live guest page cannot be discarded just because the reported budget fell.
Changing those contracts needs the managed alias/allocation work described
below. RAM below 85% therefore remains a user-run target, not an offline result.

## Stage 5 / task 4a: stop self-merge re-upload notices

For a separate comparison after the preceding texture changes:

```powershell
$env:STARPORT_SELFWRITE_FILTER = 'reset'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6
```

`reset|compare|off` is accepted, default `reset`. **Reset requests currently
use the compare safety net**, explicitly logged by `starport::selfwrite`:
native guest threads remain concurrent, so no safe post-merge ResetWriteWatch
epoch exists. `compare` selects that path directly; `off` restores pass-1
refreshes. No reset is issued after the source snapshot, so a guest write in
the snapshot-to-comparison window retains its dirty notice for the next query.

Filtering applies only to already initialized dirty ranges of buffers that
have actually participated in a host merge. Initial/unwatched ranges keep
the conservative old copy path. Adjacent dirty pages are kernel-read in
bounded 256 KiB batches, then compared in individual 4 KiB pieces against the
host's current merged bytes. Equal pages leave upload/dirty-snapshot state
unchanged; differing page runs are copied and marked for upload. One small
thread-local scratch buffer is reused, without retaining another full buffer
or a copy per eviction. DIRTY_TRACKING must be 1 for the journaled path.

Check per-frame `selfwrite_checked_bytes`, `selfwrite_equal_bytes`,
`selfwrite_forwarded_bytes`, dirty-reupload attribution and the new
`selfwrite_compare` timing. Source snapshot reads are still CPU traffic even
when a GPU upload is skipped. Actual guest edits still upload; near-zero
dirty bytes are not promised for buffers the game genuinely updates. This
filter does not itself remove merge scans or establish a 70% copy-back saving.
Windows regressions exercise a real self-merge followed by no refresh, a
subsequent guest edit that does refresh, and a write after snapshot capture
that remains dirty. GPU changes in the merge test are mocked explicitly.

## Task 4b–d: lazy readback is not enabled

The native [readback conformance fixtures](docs/pass2-readback-boundary.md)
pass: NOACCESS/guard faults compose with private write-watch; a mapped host-only
alias can publish just a touched page while the guest remains inaccessible.
Those mapped aliases do not provide native GetWriteWatch. Production coverage
of all aliases, kernel I/O, allocation/decommit semantics and independent fault
workers is still missing. Existing eager readback is retained. There is no
working READBACK=lazy switch or adaptive fault policy in this build; do not
interpret the two fixture tests as a working game integration. The requested
lazy A/B stage must be skipped until those contracts are implemented.

## Task 5: bounded CPU work and retained-mip reads

After the self-write comparison, test these changes separately, with the
same F8 scene. Restart between changes; switches are cached per process.

```powershell
$env:STARPORT_PARALLEL_WORK = '1'
$env:STARPORT_WORKER_THREADS = '12' # this machine has 16 logical processors
$env:STARPORT_XXH3 = '1'
$env:STARPORT_RETAINED_MIP_READS = '1'
.\Play-Starport.cmd --console series-x --preset balanced --ambient 0.6
```

| Flag | Default | Kill switch / effect |
| --- | --- | --- |
| STARPORT_PARALLEL_WORK | 1 | 0 restores legacy scheduling and the old default of unbatched snapshots/scans |
| STARPORT_WORKER_THREADS | max(1, logical processors - 4) | 1 runs the new work inline; explicit values clamp to 1..logical processors |
| STARPORT_XXH3 | 1 | 0 restores full BLAKE3 hashes |
| STARPORT_RETAINED_MIP_READS | 1 | 0 restores the original full source read for promoted BC images; initial real tails still require sparse reads |

One dedicated Rayon pool bounds CPU snapshot/copy/dirty-scan and per-mip hash
work. This cap does not include title threads or existing PSO/I/O workers.
Explicit VICTORY_WIDE_BACK overrides snapshot batching. Content checks use
xxh3-128, never DefaultHasher; the hash is a change detector, not a security
boundary. Uploads still retain their original source stride and GPU footprints.

Promoted rebased progressive BC images can read just pages containing retained
mips, instead of copying discarded top mips. Private write-watch clean ranges
skip this work; mapped aliases without a reliable native dirty journal still
read/hash their retained pixels conservatively. A dirty mip is hashed as a
whole, not claimed as a new general per-4-KiB tiled-texture hash cache. Existing
buffer dirty-page comparison and batched large-region GetWriteWatch remain.
The sparse block/page traversal can cost CPU time; compare the hashing and
preparing medians with RETAINED_MIP_READS=0 if it dominates.

This is the CPU-work portion of task 5. There is **no new upload copy queue**,
and buffer-output submissions still serialize for eager CPU visibility. FIF=2
can use the existing presentation slots for eligible submissions; it does not
establish the requested lazy buffer overlap. Keep FIF=1 for these comparisons.
Skip the requested lazy/FIF async stages until the memory contracts above are
implemented. No lazy switch or fence is bypassed to manufacture overlap.

New library dependencies are pinned by Cargo.lock: Rayon 1.12.0 (rayon-core
1.13.0, MIT OR Apache-2.0; [source](https://github.com/rayon-rs/rayon)) and
xxhash-rust 0.8.21 (BSL-1.0; [source](https://github.com/DoumanAsh/xxhash-rust)).
These are dependency uses, with no copied upstream renderer implementation.

## Optional diagnostic warning

STARPORT_EXPOSURE_WARNING defaults to 1; 0 disables a one-time warning when
grey/white fallback SRVs are selected with VICTORY_AMBIENT above 0.3. It does
not change either the fallback pixels or the ambient shader/level. Use black
for the controlled comparisons; grey remains useful only to identify missing
texture sampling.

## Read the comparisons together

Keep the same save, camera, cache, resolution, ambient and draw workload. The
requested order is baseline, progressive, V2+target1024, memory8192/6144,
selfwrite, then CPU workers/hash/mip reads. Lazy/adaptive readback and genuine
buffer FIF overlap remain unavailable; skip those two requested A/B stages.
Do not interpret an ignored env var as a successful async test.

For every F8 capture, keep bench.json together with settings/console logs.
Compare median/p95 frame/preparing/fence/copy-back and subphase times, recorded
draws, eviction/overwrite counts, waiting_texture_slots and due/partial/unmapped,
tail_bytes/tail_allocations/promoted_textures, dirty_reupload_bytes,
selfwrite_checked/equal/forwarded bytes, merge-scan bytes per frame, tracked
live bytes, DXGI local/non-local Budget/CurrentUsage, physical available RAM
and process private commit. Existing periodic counters supplement the phase
JSON; cumulative byte counts must be differenced over the same interval.

Desired user-run signals: real blurry content, fewer waiting slots, reduced
repeat uploads, lower shared GPU usage, RAM below 85%, and <=400 ms/frame
(<=200 stretch). A >=70% copy-back reduction is still a target for future
demand readback, not an effect established by the self-write upload filter.
If the new data shows translation dominating preparing, the next CPU change
should be a bounded reuse cache for validated immutable command/descriptor
translations, invalidated by descriptor generation, source mapping and PSO
changes; first isolate its cost from hashing and merge scans with these timers.
