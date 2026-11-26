# Starport source provenance

The local development branch starts at ztrx
`90b529f878a470fec9bf07b7be316bc769a12232`, retaining its Git history and
GPL-3.0-or-later licensing. That project credits Marvisak/VIctory and
fxceless/victory-fork. The inherited BCPack table exception remains described
in `asset-policy.md`; it is not presented as Starport-authored code.

Reviewed imported changes:

| Source | Pinned head | Applied work |
|---|---|---|
| [ztrx PR 36](https://github.com/Zettrox/ztrx/pull/36), FrancescoGelosoEterea; integer-blend work credits pablogonz12 | `1402bf7b821fc4ca9c28073debed28e8ee65fb0d` | Integer render-target blending and conservative LDS flag regions |
| [ztrx PR 94](https://github.com/Zettrox/ztrx/pull/94), JMJ0070 | `c8020f66dec029c88bfc04e749d62b129fc76768` | Adjacent GPU readback-copy coalescing, counters and tests |

The corresponding commits are retained as local `upstream/pr36` and
`upstream/pr94` refs. Downloaded patches and validation logs stay in the adjacent
`compat` directory. Their SHA-256 values are:

- PR 36: `6c10364ead0744d9623783560eebc2fdd83e11222167274e8f0056459697c30d`
- PR 94: `96bd6c2db9dc376261560cc083a38c70a847f7060b4c8cb749051038e2f2eac6`

Starport adds keyboard/mouse adaptation, focus and tap handling, local-device
user lookup, package-drive immutability, a bounded runner, source and binary
evidence, a cache-key dependency for the LDS switch and 14 GPU readback checks
of that lowering. Codex assisted with review, implementation and local testing.
The exact model identity was not exposed in this session beyond the Codex/GPT-6
agent description.

Other proposals inspected but not applied:

- ztrx PR 49: the proposed permlane lowering explicitly leaves inactive-source
  lane behavior incomplete; requires stronger semantics and execution coverage.
- victory-fork PR 188: that larger compositor/upscaler patch was downloaded
  for review and was not copied. A later two-plane implementation from the
  separately pinned cool_fox source below informed Starport's smaller,
  independently integrated presentation path.
- victory-fork PR 684 / commit
  `f6a09d3ba4ca425e71429ddbcb43b18ad507202a`: remapping every 32-bit integer
  typed-buffer view to its float counterpart was implemented and tested in
  isolation, then rejected after a matched Series X capture made both lead
  characters bald, turned eyes black and dropped clothing geometry. Evidence
  is retained outside Git under the `starport-bitexact` runtime state.
- XCL remains separate research/validation work. No GPL-2.0-only XCL source
  was copied into this GPL-3 project.

The tracked source tree contains no game executables, archives, converted
shaders, screenshots or saves. Runtime results must be identified by their
recorded executable/source hashes, rather than inferred from this document.

## Renderer audit adaptations

Reviewed `https://github.com/Belobug/VIctory-Belobug-AMD` at
`71652a2c15a0da68cd801890e1f71109670f9ca5` (GPL-3.0-or-later), retained as
`refs/remotes/research/belobug-audit`. `ambient.rs` adapts its constant
screen-probe override with allocation-owned descriptors and stricter view
checks. The pixel helper/quad rules adapt its `emit/lower.rs` behavior.
Material-mask publication uses the sequencing described in that source
with Starport's existing depth twins. No AMD-wide wave/residency changes or
external shader replacements were copied. See `starport-audit-patches.md`
for the switchable runtime configuration and validation scope.

## Residency and shader follow-up

Selectively adapted Marvisak/VIctory PR 179 at
`3248948c9fb13e3e242c6518052ca522a9944f92` and PR 180 at
`aa692580ed69e4fa20b8312df55027108fe8d7c7` (GPL-3.0-or-later).
See [starport-residency.md](starport-residency.md) for exact changes,
additional Starport lowering, offline evidence and the four unresolved
NVIDIA Wave64 compute PSOs. No upstream assumption of a size-only descriptor's
texture dimension or undefined dead-descriptor substitution was imported.

## cool_fox performance and Series S review

Reviewed the GPL-3 source at
`https://git.gtahub.st/Mskh/codec-d--cool_fox-OPTIMIZED`, pinned locally at
`808342e93a268ded13e67f4aca016214c6d3230d` as
`research/coolfox-rtx3050-opt`. Starport selectively adapted the following
exact revisions while preserving its NVIDIA Wave32, ambient, remap-lock,
bounded-flight and residency work:

- `b7ab9c49c035f5f7a26982e33a03d5f6314b8df7` (`local`): resumable pipeline
  books and out-of-process offline warming. Starport makes child start or
  incomplete output fatal, rolls back torn batch appends and includes its
  translator settings plus the DXGI driver in the bad/completed fingerprint.
- `a30829c4d245467d06df7aaebddc32f8fd954a82` (`local`): a separate bounded
  lane for pipelines measured at one second or slower. Requested pipelines
  remain first, and Starport retains a conservative default runtime worker
  count with an explicit override.
- `e05d4633a07340ccc89e82b1e3c64d2723ab0143` and
  `1ad6e67aa28f9418764413bc242baeeeef00341c` (`codec_d`): Series S scene and
  interface plane semantics and the corrected 72-byte retail-plane stride.
  Starport did not copy the source compositor wholesale; it integrated safe
  plane reads, resource-vtable validation, typed/typeless SRV checks,
  premultiplied blending and state restoration into its existing bounded,
  resolution-clamped presentation path.
- `8dd30ab472697c47b141c273c2a00f8ce1c00520` and
  `310c6eb6c16822ab95076aad61fce19bba86bb98` (`fxceless`): rotate would-be
  new texture allocations behind a bounded full-card walk and refresh an
  already allocated texture without growing VRAM. Starport adds explicit held
  state, immediate wakeup when room returns, focused queue coverage and
  telemetry, rather than importing the source's wider texture/upload system.

The source's aggressive low-memory launcher values—especially a 512-pixel
maximum texture dimension and small fixed pools—were rejected. Its own notes
associate those settings with black/blocky close faces and shadows, which
conflicts with Starport's visual-correctness goal. Upload-ahead, level-on-demand,
thin-buffer, catch-up and broad thread-count switches were likewise not made
defaults without isolated correctness and timing evidence. Upstream FPS claims
remain upstream evidence; no Starport world-performance claim is made from
offline tests.

## Sixteen-bit face-network buffers

Selectively adapted the typed-buffer half-view work from
`fxceless/victory-fork` commit
`58de9bf4c367d4e27f6dd5da523ca2f68951bc77` (GPL-3.0-or-later, pull request
927 by tippythetaidum). Starport independently integrates only the coherent
path needed here: fixed descriptor places are identified during lowering,
16-bit patterns travel through same-layout unsigned buffer views, those twin
views are isolated in fresh descriptor tables, and shader-cache serialization
records the places. The commit's unrelated indirect-draw, query, device,
occlusion and SDWA changes were not imported.
