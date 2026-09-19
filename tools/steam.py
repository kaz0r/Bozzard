#!/usr/bin/env python3
"""Build, stage and run the Steam/Spacewar reference game without global SDK setup."""
import argparse
import json
from pathlib import Path
import platform
import subprocess
import shlex

ROOT = Path(__file__).resolve().parents[1]

def run(args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['build', 'run', 'editor', 'test', 'package'])
    parser.add_argument('--release', action='store_true')
    parser.add_argument('--prepare-only', action='store_true', help='Build and stage a native launcher without opening the app')
    parser.add_argument('--join-lobby', type=int)
    parser.add_argument('--output', type=Path, help='New portable package directory (package only)')
    args = parser.parse_args()
    profile = ['--release'] if args.release else []
    if args.action == 'test':
        run(['cargo', 'test', '--locked', '-p', 'bozzard-network'], cwd=ROOT)
        run(['cargo', 'check', '--locked', '-p', 'bozzard-player', '-p', 'bozzard-editor-app', '--features', 'bozzard-player/steam,bozzard-editor-app/steam'], cwd=ROOT)
        return
    package = 'bozzard-editor-app' if args.action == 'editor' else 'bozzard-player'
    binary_name = 'bozzard-editor' if args.action == 'editor' else 'bozzard-player'
    run(['cargo', 'build', '--locked', '-p', package, '--features', 'steam', *profile], cwd=ROOT)
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1', '--features', package + '/steam'], cwd=ROOT))
    target = Path(metadata['target_directory']) / ('release' if args.release else 'debug')
    system = platform.system()
    exe = target / (binary_name + ('.exe' if system == 'Windows' else ''))
    project = ROOT / 'examples/demo/flap-woods-multiplayer.bozzard.json'
    # A native exec wrapper can be added to Steam's library for overlay launch testing.
    # It also avoids launching a different application through shared App ID 480.
    launcher_name = 'edit-steam' if args.action == 'editor' else 'play-steam'
    if system == 'Windows':
        native_launcher = target / (launcher_name + '.cmd')
        native_launcher.write_text(f'@echo off\ncd /d "%~dp0"\n"%~dp0{exe.name}" --project "{project}" %*\n')
    else:
        native_launcher = target / (launcher_name + '.sh')
        native_launcher.write_text(f'#!/bin/sh\nset -eu\ncd -- "$(dirname -- "$0")"\nexec "./{exe.name}" --project {shlex.quote(str(project))} "$@"\n')
        native_launcher.chmod(0o755)
    if args.prepare_only:
        print(f'Native Steam launcher: {native_launcher}')
        print('For overlay testing, enable the Steam overlay and add this launcher to your Steam library. Launch it through Steam.')
        return
    if args.action == 'package':
        if args.output is None:
            parser.error('package requires --output NEW_DIRECTORY')
        output = args.output.resolve()
        run([str(exe), '--export-project', str(project), '--export-dir', str(output)], cwd=target)
        print(f'Steam development package: {output}')
    elif args.action in ('run', 'editor'):
        command = [str(exe), '--project', str(project)]
        if args.join_lobby is not None:
            command += ['--join-lobby', str(args.join_lobby)]
        run(command, cwd=target)
    else:
        print(f'Native Steam player and SDK library staged in {target}')

if __name__ == '__main__':
    main()
