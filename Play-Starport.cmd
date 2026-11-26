@echo off
rem GPL-3.0-or-later. Game files and generated data belong outside this repo.
rem Default to the smaller console profile. Explicit --console overrides it.
if not defined STARPORT_SERIES_S set "STARPORT_SERIES_S=1"
rem Mip default: skip 2 on <=1080p, retain at least 3. --skip-mips 0 disables.
rem STARPORT_SKIP_TOP_MIP=0/1 remains an explicit compatibility override.
rem Renderer A/B commands and all defaults/kill switches: VERIFY.md and VERIFY2.md.
rem Device-removal diagnosis: --d3d12-debug --dred (slow; diagnostics build only).
rem Saved upscaler settings are preserved by default. --fsr quality-zero
rem sets fsrQuality=0 with an exact backup for A/B; it still runs ACCC kernels.
rem --fsr off is a legacy spelling of quality-zero, not an upscaler bypass.
rem --capture --capture-targets additionally dumps selected intermediate targets.
rem Default performance profile: relay stage 2, wide/readback joins, GPU-side
rem comparison for >=1 MiB title buffers, and a 64-pass unchanged-slot census.
rem Pass --preset balanced to disable those defaults for an A/B. Explicit
rem VICTORY_* values override every throughput default.
rem Pass 2 defaults: progressive tails=1, evictor V2=1, selfwrite=reset (safe compare).
rem STARPORT_SKIP_MIPS_TARGET_DIM=1024 explicitly selects a 1024px streaming target; 0 restores fixed N.
rem STARPORT_PARALLEL_WORK=0, STARPORT_XXH3=0, STARPORT_RETAINED_MIP_READS=0 restore old CPU paths.
rem STARPORT_MEM_BUDGET_MB=8192/6144 also caps idle caches; STARPORT_TRIM_CACHES=0 disables that cap.
rem STARPORT_BENCH=1 enables F8 30-second scene summaries. Lazy readback is not implemented.
rem Series S two-plane presentation is implemented with offline GPU coverage;
rem its world behavior still requires your run. --console series-x remains the baseline.
rem STARPORT_MAX_RES=0 disables the monitor ceiling; STARPORT_SKIP_MIPS=0 keeps all mips.
rem STARPORT_LEGACY_RESIDENCY=1, STARPORT_DIRTY_TRACKING=0, STARPORT_FRAMES_IN_FLIGHT=1 restore conservative paths.
python "%~dp0scripts\starport.py" --seconds 0 --preset throughput %*
pause
