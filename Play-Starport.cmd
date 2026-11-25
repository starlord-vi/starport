@echo off
rem GPL-3.0-or-later. Game files and generated data belong outside this repo.
rem Default to the smaller console profile. Explicit --console overrides it.
if not defined STARPORT_SERIES_S set "STARPORT_SERIES_S=1"
rem Mip default: skip 2 on <=1080p, retain at least 3. --skip-mips 0 disables.
rem STARPORT_SKIP_TOP_MIP=0/1 remains an explicit compatibility override.
rem Renderer A/B commands and all defaults/kill switches: VERIFY.md.
rem Current world comparison: --console series-x. No Series S hang fix claimed.
rem STARPORT_MAX_RES=0 disables the monitor ceiling; STARPORT_SKIP_MIPS=0 keeps all mips.
rem STARPORT_LEGACY_RESIDENCY=1, STARPORT_DIRTY_TRACKING=0, STARPORT_FRAMES_IN_FLIGHT=1 restore conservative paths.
python "%~dp0scripts\starport.py" --seconds 0 %*
pause
