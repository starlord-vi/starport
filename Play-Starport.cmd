@echo off
rem GPL-3.0-or-later. Game files and generated data belong outside this repo.
rem Default to the smaller console profile. Explicit --console overrides it.
if not defined STARPORT_SERIES_S set "STARPORT_SERIES_S=1"
rem Opt-in: set STARPORT_SKIP_TOP_MIP=1, or pass --skip-top-mip.
if not defined STARPORT_SKIP_TOP_MIP set "STARPORT_SKIP_TOP_MIP=0"
python "%~dp0scripts\starport.py" --seconds 0 %*
pause
