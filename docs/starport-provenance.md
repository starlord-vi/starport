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
- victory-fork PR 188: a much larger Series S compositor/upscaler change;
  downloaded for review, not adopted or claimed working here.
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
