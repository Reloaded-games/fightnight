"""Stage the verified extraction library in the static build without duplicate payloads."""
import json
import os
from pathlib import Path
import shutil
from collections import defaultdict

ROOT = Path(__file__).resolve().parents[2]


def stage():
    """Prepare a portable static directory, using hardlinks on the same volume."""
    source = ROOT / 'assets/imported'
    destination = ROOT / 'dist/assets/imported'
    catalog_path = source / 'catalog.json'
    if not catalog_path.exists():
        print('Extraction library absent; authored game assets remain available.')
        return
    catalog = json.loads(catalog_path.read_text(encoding='utf-8'))
    preserved = catalog['summary'].get('sourceDirectoryVerified', False)
    if not catalog['summary'].get('allVerified', False) and not preserved:
        raise ValueError('Import contains integrity failures; inspect docs/imported/import-verification.json')
    (destination / 'files').mkdir(parents=True, exist_ok=True)
    keys = ('id', 'label', 'type', 'format', 'url', 'bytes', 'sha256', 'supported')
    groups = defaultdict(list)
    for asset in catalog['assets']:
        groups[asset['type']].append({key: asset[key] for key in keys})
    (destination / 'types').mkdir(exist_ok=True)
    for kind, entries in groups.items():
        (destination / 'types' / (kind + '.json')).write_text(json.dumps(entries, ensure_ascii=False), encoding='utf-8')
    compact = {'summary': catalog['summary'], 'assets': groups['mesh'],
               'groups': {kind: {'count': len(entries), 'url': './types/' + kind + '.json'} for kind, entries in groups.items()}}
    (source / 'library.json').write_text(json.dumps(compact, ensure_ascii=False), encoding='utf-8')
    shutil.copy2(source / 'library.json', destination / 'library.json')
    presets = []
    for slot, label in [('rifle', 'Assault_SunRose_Rifle'), ('shotgun', 'ThreeBarrel_Shotgun')]:
        match = next((a for a in groups['mesh'] if a['label'] == label and a['format'] == 'glb'), None)
        if match:
            presets.append({'slot': slot, **match})
    (destination / 'presets.json').write_text(json.dumps(presets), encoding='utf-8')
    if preserved:
        bindings = {}
        archive = ROOT / 'docs/imported/fortnite-analysis/assets/converted'
        for group, expected in catalog['summary']['sourceDirectories'].items():
            original = archive / expected['relative']
            stat = original.stat()
            if (stat.st_dev, stat.st_ino) != (expected['device'], expected['inode']):
                raise ValueError('Moved extraction directory identity changed')
            for base in (source, destination):
                link = base / 'sources' / group
                link.parent.mkdir(exist_ok=True)
                if link.is_symlink() or link.is_junction():
                    if link.resolve() != original.resolve():
                        raise ValueError('Existing source link points to another directory')
                elif link.exists():
                    raise ValueError('Refusing to overwrite an existing source directory')
                else:
                    os.symlink(original, link, target_is_directory=True)
            bindings[group] = {'path': str(original), 'device': stat.st_dev, 'inode': stat.st_ino, 'identityMatchesVerifiedDirectory': True}
        (ROOT / 'docs/imported/source-bindings.json').write_text(json.dumps(bindings, indent=2), encoding='utf-8')
        print(f'STAGED {len(catalog["assets"])} source entries; three source bindings verified; hashes checked on use.', flush=True)
        return
    files = {a['url'] for a in catalog['assets']}
    for number, relative in enumerate(sorted(files), 1):
        original, target = source / relative, destination / relative
        if target.exists():
            if not os.path.samefile(original, target):
                from import_extracted import digest
                if digest(original) != digest(target):
                    raise ValueError(f'Destination mismatch: {target}')
        else:
            try:
                os.link(original, target)
            except OSError:
                shutil.copy2(original, target)
        if number % 5000 == 0:
            print(f'STAGE {number}/{len(files)}', flush=True)
    print(f'STAGED {len(files)} unique files / {len(catalog["assets"])} catalog entries', flush=True)


if __name__ == '__main__':
    stage()
