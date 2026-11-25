@echo off
rem GPL-3.0-or-later. Game files and generated data belong outside this repo.
rem Default to the smaller console profile. Explicit --console overrides it.
if not defined STARPORT_SERIES_S set "STARPORT_SERIES_S=1"
rem Mip default: skip 2 on <=1080p, retain at least 3. --skip-mips 0 disables.
rem STARPORT_SKIP_TOP_MIP=0/1 remains an explicit compatibility override.
python "%~dp0scripts\starport.py" --seconds 0 %*
pause
