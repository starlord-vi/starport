"""Local Starport build runner. GPL-3.0-or-later. No game data enters Git."""
import argparse
import datetime as dt
import csv
import hashlib
import json
import math
import os
import re
import statistics
from pathlib import Path
import shutil
import subprocess
import sys
import time


def sha256(path):
    with path.open('rb') as stream:
        digest = hashlib.sha256()
        for block in iter(lambda: stream.read(8 * 1024 * 1024), b''):
            digest.update(block)
        return digest.hexdigest()


def outside(path, repo):
    path = path.resolve()
    if path.is_relative_to(repo):
        raise ValueError(f'Game files and generated data must stay outside {repo}: {path}')
    return path


def package_view(exe, assets, state):
    package = state / 'package'
    package.mkdir(parents=True, exist_ok=True)
    files = []
    for source in [exe] + sorted(p for p in assets.rglob('*') if p.is_file()):
        relative = Path(source.name) if source == exe else source.relative_to(assets)
        target = package / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        binary = source.suffix.lower() in ('.exe', '.dll')
        digest = sha256(source) if binary else None
        if binary:
            if not target.exists() or sha256(target) != digest:
                shutil.copy2(source, target)
            if sha256(target) != digest:
                raise RuntimeError(f'Copied binary hash mismatch: {target}')
        elif not target.exists():
            os.link(source, target)
        elif not os.path.samefile(source, target):
            raise ValueError(f'Unexpected existing package asset: {target}')
        info = source.stat()
        files.append(dict(source=str(source), size=info.st_size,
                          mtime_ns=info.st_mtime_ns, sha256=digest))
    return package / exe.name, files


def summarize_frames(run):
    frames = []
    loss = {'dropped': 0, 'truncated': 0}
    for line in (run/'console.log').open(encoding='utf-8', errors='replace'):
        if 'starport::frame:' in line:
            values = dict((k, int(v)) for k,v in re.findall(r'(frame|elapsed_us|dt_us|draws|dispatches)=(\d+)', line))
            if len(values) == 5:
                frames.append(values)
        if line.startswith('STARPORT_LOG_LOSS '):
            for k,v in re.findall(r'(dropped|truncated)=(\d+)', line):
                loss[k] = max(loss[k], int(v))
    with (run/'frames.csv').open('w', newline='', encoding='utf-8') as out:
        writer = csv.DictWriter(out, fieldnames=['frame','elapsed_us','dt_us','draws','dispatches'])
        writer.writeheader()
        writer.writerows(frames)
    def stats(rows):
        ms = sorted(f['dt_us']/1000 for f in rows if f['dt_us'] > 0)
        if not ms:
            return {'frames': 0}
        return dict(frames=len(ms), median_ms=statistics.median(ms),
                    p95_ms=ms[math.ceil(len(ms)*.95)-1], mean_ms=statistics.mean(ms),
                    average_fps=1000/statistics.mean(ms))
    last = frames[-1]['elapsed_us'] if frames else 0
    gaps = sum(b['frame'] != a['frame']+1 for a,b in zip(frames,frames[1:]))
    return dict(all_frames=stats(frames[1:]),
                last_30_seconds=stats([f for f in frames[1:] if f['elapsed_us'] >= last-30_000_000]),
                log_loss=loss, frame_sequence_gaps=gaps,
                timing='CPU present-to-present wall time; not GPU execution time',
                note='No automatic claim that the last 30 seconds are gameplay')


def main():
    repo = Path(__file__).resolve().parents[1]
    root = repo.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game', type=Path, default=root/'exe'/'game_scarlett_master.exe')
    parser.add_argument('--assets', type=Path, default=root/'files')
    parser.add_argument('--state', type=Path, default=root/'runtime'/'starport')
    parser.add_argument('--backend', type=Path,
                        default=root/'compat'/'starport_build'/'diag'/'victory.exe')
    parser.add_argument('--seconds', type=int, default=600)
    parser.add_argument('--preset', choices=('baseline','balanced','throughput'), default='balanced')
    parser.add_argument('--ambient', default='0.6', help='Constant indirect-light fallback level, or off; not ray tracing')
    parser.add_argument('--material-depth', choices=('early','late'), default='early')
    parser.add_argument('--console', choices=('series-x', 'series-s'),
                        default='series-s' if os.environ.get('STARPORT_SERIES_S') == '1' else 'series-x')
    parser.add_argument('--skip-top-mip', action=argparse.BooleanOptionalAction,
                        default=None,
                        help='Compatibility alias for --skip-mips 1; --no-skip-top-mip means 0')
    parser.add_argument('--skip-mips', type=int, choices=range(13), help='Omit N top streaming mips; 0 disables; default automatic')
    parser.add_argument('--capture', action='store_true', help='Diagnostic PNG captures; perturbs timings')
    parser.add_argument('--capture-every', type=int, help='Dense native captures every N presents; requires --capture')
    parser.add_argument('--data', type=Path, help='Separate test profile; existing saves are preserved')
    parser.add_argument('--cache', type=Path, help='Shared shader cache for controlled profile comparisons')
    parser.add_argument('--texture-full-hash', action='store_true', help='Check aliased texture contents every submission')
    parser.add_argument('--texture-no-wait', action='store_true', help='Diagnostic nonblocking texture refresh')
    parser.add_argument('--relay', action='store_true', help='Opt-in upstream asynchronous submission path')
    parser.add_argument('--bvh', action='store_true', help='Diagnostic RT builder attempt; incomplete upstream')
    parser.add_argument('--strict-waits', action='store_true', help='Keep unresolved queue waits; never force at present')
    args = parser.parse_args()
    mip_override = args.skip_mips
    if mip_override is None and args.skip_top_mip is not None:
        mip_override = int(args.skip_top_mip)
    if mip_override is None:
        mip_override = os.environ.get('STARPORT_SKIP_MIPS')
    if mip_override is None and os.environ.get('STARPORT_SKIP_TOP_MIP') in ('0', '1'):
        mip_override = os.environ['STARPORT_SKIP_TOP_MIP']
    if not 0 <= args.seconds <= 86400:
        parser.error('--seconds must be 0 (until you close the game) or 1..86400')
    try:
        if args.ambient != 'off' and not (math.isfinite(float(args.ambient)) and 0 <= float(args.ambient) <= 16):
            raise ValueError()
    except ValueError:
        parser.error('--ambient must be off or a finite number from 0 to 16')
    if args.capture_every is not None and (not args.capture or args.capture_every < 1):
        parser.error('--capture-every requires --capture and a positive interval')
    exe, assets, state = [outside(p, repo) for p in (args.game, args.assets, args.state)]
    data = outside(args.data or state/'data', repo)
    cache = outside(args.cache or state/'cache', repo)
    for path in (data, cache):
        if path == assets or path.is_relative_to(assets) or path == exe.parent or path.is_relative_to(exe.parent):
            parser.error('Writable test profiles and caches must be separate from original game files')
    if not exe.is_file() or not assets.is_dir() or not args.backend.is_file():
        parser.error('Game executable, asset directory or built backend is missing')
    if state == assets or state.is_relative_to(assets) or assets.is_relative_to(state):
        parser.error('State and original assets must be separate directories')
    game, originals = package_view(exe, assets, state)
    stamp = dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%S.%fZ')
    run = state / 'runs' / stamp
    run.mkdir(parents=True)
    env = {k:v for k,v in os.environ.items() if not k.upper().startswith(('VICTORY_', 'STARPORT_'))}
    settings = {
        'VICTORY_LOG': 'info',
        'STARPORT_RECORD': '1',
        'STARPORT_TELEMETRY': os.environ.get('STARPORT_TELEMETRY', '1'),
        'STARPORT_FALLBACK_COLOR': os.environ.get('STARPORT_FALLBACK_COLOR', 'black'),
        'STARPORT_MAX_RES': os.environ.get('STARPORT_MAX_RES', ''),
        'STARPORT_SERIES_S': '1' if args.console == 'series-s' else '0',
        'VICTORY_AMBIENT': args.ambient,
        'VICTORY_MATERIAL_DEPTH': args.material_depth,
        'VICTORY_INPUT': 'keyboard-mouse',
        'VICTORY_CACHE_DIR': str(cache),
        'VICTORY_DATA_DIR': str(data),
        'VICTORY_GAME_DIR': str(game.parent),
        'VICTORY_DXIL': str(root/'compat'/'tools_bin'/'dxc-1.9.2609'/'bin'/'x64'/'dxil.dll'),
        'VICTORY_PC_DXC': str(root/'compat'/'tools_bin'/'dxc-1.9.2609'/'bin'/'x64'/'dxc.exe'),
    }
    if mip_override is not None:
        settings['STARPORT_SKIP_MIPS'] = str(mip_override)
    if 'STARPORT_SKIP_MIPS_MIN_DIM' in os.environ:
        settings['STARPORT_SKIP_MIPS_MIN_DIM'] = os.environ['STARPORT_SKIP_MIPS_MIN_DIM']
    for name in ('STARPORT_RESIDENCY', 'STARPORT_LEGACY_RESIDENCY', 'STARPORT_VRAM_TARGET_PCT',
                 'STARPORT_DIRTY_TRACKING', 'STARPORT_REBAR_UPLOAD', 'STARPORT_FRAMES_IN_FLIGHT'):
        if name in os.environ:
            settings[name] = os.environ[name]
    if args.preset != 'baseline':
        settings.update(VICTORY_TEXTURE_NO_WAIT='1', VICTORY_JOIN_COPIES='1', VICTORY_WIDE_REFRESH='1')
    if args.preset == 'throughput':
        # Relay's own default stage; do not accidentally override it with
        # LATE_WAIT=3 or enable diagnostic per-list GPU timing.
        settings['VICTORY_RELAY'] = '2'
    if args.capture:
        (run/'frames').mkdir()
        settings.update(VICTORY_CAPTURE=str(run/'frames'), VICTORY_CAPTURE_WORLD='1')
        if args.capture_every:
            settings['VICTORY_CAPTURE_EVERY'] = str(args.capture_every)
    if args.relay:
        settings.update(VICTORY_RELAY='2', VICTORY_LATE_WAIT='3', VICTORY_WIDE_BACK='1')
    if args.bvh:
        settings['VICTORY_BVH_BUILDER'] = '1'
    if args.texture_full_hash:
        settings['VICTORY_TEXTURE_GUARD'] = '0'
    if args.texture_no_wait:
        settings['VICTORY_TEXTURE_NO_WAIT'] = '1'
    if args.strict_waits:
        settings['VICTORY_WAIT_FORCE'] = '0'
    env.update(settings)
    backend_hash = sha256(args.backend)
    backend = state/'backends'/backend_hash/'victory.exe'
    backend.parent.mkdir(parents=True, exist_ok=True)
    if not backend.exists():
        shutil.copy2(args.backend, backend)
    if sha256(backend) != backend_hash:
        raise RuntimeError('Backend snapshot hash mismatch')
    command = [str(backend), 'run', str(game), '--execute', '--console', args.console]
    if args.seconds:
        command += ['--seconds', str(args.seconds)]
    patch = subprocess.check_output(['git', '-C', str(repo), 'diff', '--binary'])
    (run/'source.diff').write_bytes(patch)
    new_sources = {}
    untracked = subprocess.check_output(['git', '-C', str(repo), 'ls-files', '--others',
                                         '--exclude-standard', '-z']).split(b'\0')
    for name in filter(None, untracked):
        relative = Path(os.fsdecode(name))
        source = repo / relative
        destination = run/'source-new'/relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
        new_sources[relative.as_posix()] = sha256(source)
    metadata = dict(command=command, environment=settings,
                    base_commit=subprocess.check_output(['git','-C',str(repo),'rev-parse','HEAD'], text=True).strip(),
                    backend_sha256=backend_hash, source_diff_sha256=hashlib.sha256(patch).hexdigest(),
                    new_source_sha256=new_sources,
                    original_files=originals, visual_verified=False, input_verified=False)
    (run/'launch.json').write_text(json.dumps(metadata, indent=2), encoding='utf-8')
    (state/'latest.json').write_text(json.dumps(dict(run=str(run)), indent=2), encoding='utf-8')
    print(f'Run evidence: {run}', flush=True)
    started = time.monotonic()
    forced = False
    with (run/'console.log').open('wb') as log:
        process = subprocess.Popen(command, cwd=game.parent, env=env, stdout=log,
                                   stderr=subprocess.STDOUT, creationflags=subprocess.CREATE_NO_WINDOW)
        metadata['pid'] = process.pid
        (run/'launch.json').write_text(json.dumps(metadata, indent=2), encoding='utf-8')
        try:
            code = process.wait(timeout=args.seconds+60 if args.seconds else None)
        except (subprocess.TimeoutExpired, KeyboardInterrupt):
            forced = True
            process.terminate()
            code = process.wait(timeout=20)
    intact = all(Path(f['source']).stat().st_size == f['size']
                 and Path(f['source']).stat().st_mtime_ns == f['mtime_ns'] for f in originals)
    hashes = all(sha256(Path(f['source'])) == f['sha256'] for f in originals if f['sha256'])
    metadata.update(exit_code=code, forced_termination=forced,
                    elapsed_seconds=time.monotonic()-started,
                    original_metadata_unchanged=intact, original_binary_hashes_unchanged=hashes)
    metadata['recording'] = summarize_frames(run)
    (run/'summary.json').write_text(json.dumps(metadata, indent=2), encoding='utf-8')
    print(f'Exit {code}; forced={forced}; original checks: {intact}, {hashes}', flush=True)
    print(f'Frame timings: {run / "frames.csv"}; summary: {run / "summary.json"}', flush=True)
    if not intact or not hashes:
        raise RuntimeError('Original file integrity check failed')
    return code if not forced else 124


if __name__ == '__main__':
    sys.exit(main())
