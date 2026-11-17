"""Build source checkouts or package existing API Level 4 distribution files."""
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

dist = root / 'dist'
dist.mkdir(exist_ok=True)
source_checkout = (root / 'Cargo.toml').is_file()
manifest_path = root / 'manifest.json' if source_checkout else dist / 'manifest.json'
manifest = json.loads(manifest_path.read_text(encoding='utf-8'))

if source_checkout:
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
    target = Path(env.get('CARGO_TARGET_DIR', 'target'))
    if not target.is_absolute():
        target = root / target
    shutil.copy2(target / 'wasm32-wasip2/release/pocket_news.wasm', dist / manifest['entry'])
    shutil.copy2(manifest_path, dist / 'manifest.json')
    shutil.copy2(root / manifest['icon'], dist / manifest['icon'])

# Remove stale output left by earlier package layouts.
license_dist = dist / 'licenses'
if not dist.resolve().is_relative_to(root) or license_dist.resolve().parent != dist.resolve():
    raise SystemExit('License output must stay inside the plugin dist directory')
if license_dist.exists():
    shutil.rmtree(license_dist)

if options.package:
    if source_checkout:
        subprocess.run([sys.executable, str(root / 'scripts/package-source.py')], check=True)
    abp = dist / f'PocketNews-{manifest["version"]}.abp'
    with zipfile.ZipFile(abp, 'w', zipfile.ZIP_DEFLATED, compresslevel=9, strict_timestamps=False) as archive:
        for name in [manifest['entry'], 'manifest.json', manifest['icon']]:
            archive.write(dist / name, name)
    print(f"{abp} ({abp.stat().st_size / 1_000_000:.2f} MB)")
    if source_checkout:
        source = dist / f'PocketNews-{manifest["version"]}-source.zip'
        print(f"{source} ({source.stat().st_size / 1_000_000:.2f} MB, source)")
else:
    wasm = dist / manifest['entry']
    print(f"{wasm} ({wasm.stat().st_size / 1_000_000:.2f} MB)")
