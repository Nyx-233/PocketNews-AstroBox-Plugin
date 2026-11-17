"""Package the standalone plugin's source, models, build scripts, and licenses."""
from pathlib import Path
import json
import zipfile

root = Path(__file__).resolve().parents[1]
version = json.loads((root / 'manifest.json').read_text(encoding='utf-8'))['version']
dist = root / 'dist'
dist.mkdir(exist_ok=True)
output = dist / f'PocketNews-{version}-source.zip'
files = [root / name for name in (
    'LICENSE', 'README.md', 'THIRD_PARTY_NOTICES.md', '.gitignore', 'index.txt',
    'Cargo.toml', 'Cargo.lock', 'manifest.json', 'icon.png',
)]
for directory in ('src', 'scripts', 'wit', 'licenses', 'training', 'tests', 'examples'):
    files.extend(path for path in (root / directory).rglob('*')
                 if path.is_file() and '__pycache__' not in path.parts
                 and path.suffix != '.pyc')
with zipfile.ZipFile(output, 'w', zipfile.ZIP_DEFLATED, compresslevel=9,
                     strict_timestamps=False) as archive:
    for path in sorted(set(files)):
        archive.write(path, path.relative_to(root).as_posix())
print(output)
