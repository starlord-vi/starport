# Black-world capture, 27 November 2026

Evidence: `A:/starport/runtime/starport/runs/20261126T212614.275439Z/`,
commit `2aef2171558559427420b4c036fbbd6f8a4c51c0`, backend SHA-256
`a691510712ce773bdb1fb9f15f41e02ffba2953552663b196a0bb0ae14e13478`.
The user ran the game. Exit 0; original metadata and binary integrity checks
both true. No agent game launch or automated input was used.

## Observed

The scene input is already almost black in frames 1324, 1358 and 1414.
Frame 1414's interface input holds readable pink credits; the final composition
preserves those credits over the black scene. This places the principal scene
failure before presentation. It does not establish that every earlier pass is
empty or rule out additional rendering faults.

World drawing began at frame 1318; 98 subsequent frames were recorded. The
last 40 have median 649.136 ms, mean 666.224 ms (1.501 FPS). These are capture
timings, not an uninstrumented performance result. Late log intervals show
407-458 ms GPU idle while the submitting thread works, and 17-23% GPU busy.
The heap-buffer phase includes the texture-queue pass, so its 130-354 ms must
not be attributed entirely to changed descriptor mirroring without finer timing.

Pipeline waits reached 0 ms; texture backlog fell to 2,779 at sampled frame
1413. Ambient audit still reports a matched resolve and recorded override,
without proof of downstream diffuse correctness. Cold compilation and texture
backlog alone do not explain the persistent black scene in this run.

## Concrete compatibility gap

Twenty compute shader hashes repeatedly fail with `no DXIL part`. Reading the
original containers from the local `cache/pipelines/known-shaders.bin` shows
ACCC/FSPX native compute parts instead of DXIL (for example `6a6c7b7fde59134f`,
`f46117391dd81461`, `8cbd7f5b9ce7530c`, `ba578b59c8c28a57`). These are the
FidelityFX upscaler family identified in the reference source.

The Starport profile at `data/7952987A/storage/settings/settings.xml` has
`fsrQuality=2`; the user's working reference profile at
`A:/starport/runtime/coolfox-opt/data/7952987A/storage/settings/settings.xml`
has `fsrQuality=0`. Its other quality settings differ too, so this is a focused
candidate fix, not proof of visual recovery. Series S identity remains selected.

A local parser experiment was based on GPL-3 reference revision
`7df4a505d37a8a6c5d83a8e9e412532d994c3582` (Belobug, beloooo), reviewed through
`https://git.gtahub.st/Mskh/codec-d--cool_fox-OPTIMIZED` at
`808342e93a268ded13e67f4aca016214c6d3230d`.
Of 29 recorded ACCC containers, 13 validated and created hardware compute PSOs;
16 still failed on descriptor reconstruction, dot-product instructions,
special operands, lane masks, image atomics or packed operations. This was a
failed acceptance test. The experiment was removed from runnable source;
no broad Wave32/Wave64 translator changes were shipped. Generated audit data
remains outside Git in `runtime/starport/renderer-audit/20261127-accc/`.

## Implemented next test

The launcher now selects the title's native-rendering configuration by setting
only `fsrQuality=0`, with an exact backup, atomic replacement, metadata and
`--fsr preserve` opt-out. It does not copy the reference's lower texture/LOD
settings. Native rendering can increase GPU workload relative to functioning
upscaling, so this is not an established FPS gain.

`--capture-targets` now enables existing intermediate-target dumps on selected
world frames. PNG filenames identify frame/resource for correlation with the
operation trace; numerical target logs precede each PNG save. This is available
if the native path remains black. The renderer source/binary stay at `2aef217`
to retain the existing build-keyed shader/pipeline cache for this setting test.

The user must confirm visible world output and obtain a separate ordinary run
for performance. Runtime confirmation is pending.
