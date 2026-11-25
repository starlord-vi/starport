# Series S, streaming residency and shader audit (2026-11-25)

Historical first-patch audit. For current defaults, multi-mip selection,
descriptor-patched reclamation and exact user-run commands, use
[VERIFY.md](../VERIFY.md); it supersedes the earlier defaults below.

This change was checked through CLI compilation, unit/integration tests and
offline shader validation. The game and `Play-Starport.cmd` were not launched.
Resolution, world FPS and visual improvements require the user's next run.

## Presets and A/B tests

`A:/starport/Play-Starport.cmd` defaults `STARPORT_SERIES_S` to `1` only when
the variable is unset. `STARPORT_SKIP_TOP_MIP` defaults to `0`. The Python
runner explicitly carries both values into the child and records them in
`launch.json`; its environment cleanup does not discard these settings.

Commands for the user to run from `A:/starport`:

```powershell
# Series S, full texture chain (new default)
.\Play-Starport.cmd --console series-s --no-skip-top-mip
# Series X comparison
.\Play-Starport.cmd --console series-x --no-skip-top-mip
# Series S with the optional texture safety valve
.\Play-Starport.cmd --console series-s --skip-top-mip
```

The equivalent environment switches are `STARPORT_SERIES_S=0|1` and
`STARPORT_SKIP_TOP_MIP=0|1`. Explicit runner options win over the environment.
The backend also honors `STARPORT_SERIES_S` when `run --console` is absent;
an explicit backend `--console` wins. No other performance flags were enabled.

Series S reuses the existing console identity path (analytics, registry
`ConsoleMode`, DXGI adapter pools). Its title pool is 7,776 MiB, entirely
standard memory, versus Series X's 10 GiB GPU-optimal plus 3.5 GiB standard.
`XMemGetWorkingSetStatistics` and `GlobalMemoryStatusEx` share the same usable
title limit, including memory transferred to the system. They count tracked
guest allocations instead of the entire Windows process's private bytes:
unique XMem physical pages, private committed title reservations, default
XMem allocations and GPU-module guest heap/descriptor backing. Mapped aliases
are not counted twice. The Windows memory API hooks track native title
VirtualAlloc/VirtualFree as well. This is allocation accounting, not a claim
to know every guest stack/image byte or each bandwidth class's live usage.
Windows virtual address-space size stays distinct from physical memory.

The title chooses its rendering settings from this profile. No fixed 900p or
1080p size is forced, and a lower internal resolution has not been observed yet.

## Residency and staging

The streaming admission threshold is 80% of the actual DXGI local budget,
including when less than 2 GiB of streaming textures exists. A once-per-second
LRU sweep under pressure chooses CPU-backed textures unused for 90 presented
frames, oldest first, and aims for 72% to leave headroom. GPU-written textures
are excluded. `ID3D12Device::Evict` keeps their descriptors and addresses intact;
synchronous `MakeResident` restores them before their next submission.

Every submission records its texture residency fence number, including explicit
texture states, retained resource references, and all materialized descriptors
of its retained shader-visible heaps. A texture whose last submission has not
finished cannot be evicted. Descriptor publication restores residency too.
Recorded lists retain the host heap object itself, so releasing the title's
heap before submission cannot lose its texture references or residency census.
The heap maintains a reference-counted set of streaming resources, so submission
does not scan its empty slots. Released dynamic slots are made null and release
their retained resources after their owning list finishes and is reset.
This conservatively protects unknown bindless accesses: a bound heap's textures
are an active working set. LRU cannot make an oversized active heap fit in VRAM.
Evict also does not promise that Windows discards every backing byte from RAM.
The smaller profile and optional mip clamp address that working-set size.

CPU detiling images are released after copying into an immutable GPU upload
snapshot. The snapshot remains alive until its submission fence completes,
then is released even if the title retains the closed list. Hash/write-watch
metadata remains for unchanged textures. Exceptional read scratch buffers above
16 MiB are released after use; small buffers remain reusable. Uploads recorded
in the original list, which may be replayed, retain their original lifetimes.

The existing asynchronous statistics log now includes:

- `evicted_textures`: cumulative eviction events, including repeat evictions.
- `evicted_texture_bytes` and `restored_textures`: cumulative bytes/events.
- `waiting_texture_slots`: the most recent active heap queue's census. These
  are descriptor slots, not a count of unique textures.

## Optional top-mip clamp

`STARPORT_SKIP_TOP_MIP=1` affects synthetic placed SRVs of CPU-streamed BC
textures with at least two mips and width or height >= 2048. Only non-MSAA 2D
images, including arrays/cube views, qualify. Real title resource objects,
uncompressed aliases and render/depth/UAV targets keep their full allocation.
This restriction avoids changing GPU-written resources or copy destinations.

The host allocation starts at source mip 1. Uploads preserve the original
console layout and array stride, selecting source mip `host_mip + 1`.
SRVs intersect their original mip range with the retained chain; array slices
and cube faces stay the same. View metadata reports the resulting dimensions.
Dropping the largest level removes roughly 75% of a large 2D mip chain's texel
bytes; alignment and small tails reduce the actual allocation saving. Full
source-layout hashing is still used, preserving the existing remap protocol.

## Shader provenance and remaining blocker

Reviewed and selectively adapted, preserving the inherited GPL-3.0-or-later
license and credits:

- [Marvisak/VIctory PR 179](https://github.com/Marvisak/VIctory/pull/179),
  `3248948c9fb13e3e242c6518052ca522a9944f92`: lazy interpolation SSA indexing
  and branch/vertex regression cases. Existing weighted interpolation remains.
- [Marvisak/VIctory PR 180](https://github.com/Marvisak/VIctory/pull/180),
  `aa692580ed69e4fa20b8312df55027108fe8d7c7`: structural twin-load matching
  (including equivalent readfirstlane offsets) and explicit quad gradients for
  implicit compute samples. Existing pixel helper and NVIDIA Wave32/two-wave
  handling remain. Bias handling also preserves packed-coordinate/offset rules.

The upstream size-only fallback's assumed Texture2D kind was not adopted.
Instead the host maintains size fields per actual view in four extra planes
of the existing stride metadata buffer (32 MiB extra per view heap). Shaders
can read those fields without binding a texture of an assumed dimension.
Size-only queries use a separate root binding to that sidecar and do not
request uploads of every texture in the heap.
Unsupported address/format dwords remain unsupported. The layout follows
[AMD's RDNA 2 ISA, table 45](https://www.amd.com/content/dam/amd/en/documents/radeon-tech-docs/instruction-set-architectures/rdna2-shader-instruction-set-architecture.pdf).

Packed interpolation now indexes a first half if another instruction needs its
value, including p2_f16 preserving the destination's other 16 bits. This fixes
the observed live first-half failure while retaining optimized ordinary pairs.

All four reported pixel containers translate and pass DXIL validation offline:
`e5b2121a02f4f741`, `2fd01fca16993001`, `6b6db9f1a526daeb`,
`38bf0814e721a1b3`. No proprietary fixtures are stored in this repository.

The four compute containers `2da4beb386509b77`, `369cbe906ea72764`,
`02384a60fda6f2d0`, `6b82bc7d22016cb5` also pass DXIL validation, but
NVIDIA compute PSO creation still fails. The D3D12 debug layer specifically
reports a requested wave width of 64 against a device maximum of 32.
These BVH programs use shared memory and lane operations in divergent groups
larger than one console wave. PR 180's compute sampling fix does not resolve
that synchronization problem. Wave64 metadata was not blindly removed, unsafe
wave-wide retries were not enabled, and the RT completeness guard remains.
These four pipelines are a remaining implementation blocker, not fixed shaders.

## Validation and evidence

Evidence directory:
`A:/starport/runtime/starport/renderer-audit/20261125-residency/`.
Normal affected-crate tests, Clippy and policy results are recorded there.
New tests cover fence/age eligibility, a real Evict/MakeResident texture replay,
CPU staging reclamation, mip/SRV intersection, actual array-view dimensions,
guest allocation accounting, twin loads, interpolation and compute gradients.

Final checks:

| Check | Result |
| --- | --- |
| Workspace check, all targets/features | Passed |
| `victory-core` | 12 passed |
| `victory-gpu` | 135 passed, 1 benchmark ignored |
| `victory-hle` | 102 unit tests and 1 documentation test passed |
| `victory-shader` | 220 reported passing, 2 benchmarks ignored |
| Workspace Clippy, all targets/features, warnings denied | Passed |
| Asset policy | 180 files passed |
| Reported containers, offline DXIL validation | 8/8 passed |
| Additional NVIDIA compute PSO diagnostic | 4/4 still refused (Wave64) |

The affected-crate harness reports 470 passing tests and 3 ignored benchmarks.
Four of the shader checks return early because console/compiler dependencies
are unavailable; they are internal skips within that reported passing count.
`crate-tests-final.log` and the subsequent `ownership-tests-final.log` record
the full crate runs and the final heap-lifetime regression. `dxil-final.log`
and `compute-pso-final.log` keep validation success separate from the remaining
pipeline failures. The optimized diagnostic backend was rebuilt locally without
executing it; `build.log` records that compilation.

The optional local-data audits are Cargo tests in `apps/victory/src/translate.rs`:

```powershell
$env:STARPORT_SHADER_INPUTS = 'A:/starport/files/shaders.rpf;A:/starport/files/XboxBvhBuilder.dll;A:/starport/exe/game_scarlett_master.exe'
$env:STARPORT_SHADER_AUDIT_OUT = 'A:/starport/runtime/starport/renderer-audit/20261125-residency'
cargo test -p victory --all-features reported_shaders_validate_offline -- --ignored --nocapture --test-threads=1
# Separate diagnostic; currently fails on the four Wave64 compute PSOs:
cargo test -p victory --all-features reported_compute_pipelines_validate_offline -- --ignored --nocapture --test-threads=1
```

They read containers without mapping or executing the game. The second only
creates/checks host pipelines; it dispatches no title shader. Use the repository's
configured Cargo target directory and VICTORY_DXIL validator path.

## Files changed in this step

- `A:/starport/Play-Starport.cmd`
- `scripts/starport.py`
- `apps/victory/src/main.rs`, `run.rs`, `translate.rs`
- `crates/victory-gpu/src/console.rs`, `lib.rs`, `residency.rs`, `resource.rs`,
  `host.rs`, `texture.rs`, `descriptor.rs`, `descriptor/host.rs`, `draw.rs`,
  `pipeline/host.rs`
- `crates/victory-hle/src/modules/kernel.rs`, `xmem.rs`
- `crates/victory-shader/src/io.rs`, `emit/lower.rs`, `emit/mod.rs`,
  `emit/shader.rs`, `emit/waterfall_tests.rs`
- `docs/starport-residency.md`, `docs/starport-provenance.md`,
  `docs/starport-validation.md`

Paths other than the launcher are relative to `A:/starport/port`.
Pre-existing ambient, remap-lock, input, renderer and logging changes remain.
