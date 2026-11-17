"""Build the API Level 4 component and package an installable AstroBox ABP."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import zipfile

root = Path(__file__).resolve().parents[1]
args = argparse.ArgumentParser()
args.add_argument('--package', action='store_true')
options = args.parse_args()
env = os.environ.copy()
cargo = shutil.which('cargo')
if not cargo:
    for local_tools in (root / '.tools', root.parent / 'news/.tools'):
        if (local_tools / 'cargo/bin/cargo.exe').is_file():
            cargo = str(local_tools / 'cargo/bin/cargo.exe')
            env['CARGO_HOME'] = str(local_tools / 'cargo')
            env['RUSTUP_HOME'] = str(local_tools / 'rustup')
            break
if not cargo:
    raise SystemExit('Rust is required. Install rustup, then run: rustup target add wasm32-wasip2')
subprocess.run([cargo, 'build', '--locked', '--target', 'wasm32-wasip2', '--release'], cwd=root, env=env, check=True)
dist = root / 'dist'
dist.mkdir(exist_ok=True)
manifest = json.loads((root / 'manifest.json').read_text(encoding='utf-8'))
target = Path(env.get('CARGO_TARGET_DIR', 'target'))
if not target.is_absolute():
    target = root / target
shutil.copy2(target / 'wasm32-wasip2/release/pocket_news.wasm', dist / manifest['entry'])
shutil.copy2(root / 'manifest.json', dist / 'manifest.json')
shutil.copy2(root / manifest['icon'], dist / manifest['icon'])
# Replace rather than merge, so licenses of removed dependencies do not linger in the package.
license_dist = dist / 'licenses'
if not dist.resolve().is_relative_to(root) or license_dist.resolve().parent != dist.resolve():
    raise SystemExit('License output must stay inside the plugin dist directory')
shutil.rmtree(license_dist, ignore_errors=True)
shutil.copytree(root / 'licenses', license_dist)
if options.package:
    subprocess.run([sys.executable, str(root / 'scripts/package-source.py')], check=True)
    source = dist / f'PocketNews-{manifest["version"]}-source.zip'
    abp = dist / f'PocketNews-{manifest["version"]}.abp'
    with zipfile.ZipFile(abp, 'w', zipfile.ZIP_DEFLATED, compresslevel=9, strict_timestamps=False) as archive:
        for name in [manifest['entry'], 'manifest.json', manifest['icon']]:
            archive.write(dist / name, name)
        for file in sorted((dist / 'licenses').rglob('*')):
            if file.is_file():
                archive.write(file, file.relative_to(dist).as_posix())
    print(f"{abp} ({abp.stat().st_size / 1_000_000:.2f} MB)")
    print(f"{source} ({source.stat().st_size / 1_000_000:.2f} MB, source)")
else:
    wasm = dist / manifest["entry"]
    print(f"{wasm} ({wasm.stat().st_size / 1_000_000:.2f} MB)")
