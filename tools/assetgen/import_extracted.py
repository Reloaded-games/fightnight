"""Import every saved conversion into Fightnight, with SHA-256 and no source deletion.

Run from any directory: python tools/assetgen/import_extracted.py
Large source archives stay outside the web build. Content-addressed hardlinks avoid
duplicating the extraction on this disk; the links survive removal of source paths.
"""
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
import sys
from pathlib import Path
import time

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'fortnite/fortnite-analysis/assets/converted'
ARCHIVE = ROOT / 'docs/imported/fortnite-analysis/assets/converted'
OUTPUT = ROOT / 'assets/imported'
REPORTS = ROOT / 'docs/imported'


def digest(path):
    """Hash the actual file, streaming bounded chunks."""
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def collect_outputs(source):
    """Deduplicate successful exports from the complete append-only manifest."""
    exports = {}
    records = 0
    started = time.monotonic()
    with (source / 'conversion-manifest.jsonl').open(encoding='utf-8-sig') as stream:
        for line in stream:
            record = json.loads(line)
            records += 1
            if record.get('kind') in ('export', 'package'):
                for output in record.get('outputs', []):
                    relative = output['relativePath']
                    previous = exports.get(relative, {})
                    exports[relative] = {
                        **previous, **output, 'className': record.get('className') or previous.get('className'),
                        'objectPath': record.get('objectPath') or previous.get('objectPath'),
                        'package': record.get('virtualPath'),
                        'container': record.get('container'),
                    }
            if records % 100000 == 0:
                print(f'MANIFEST {records} records; {len(exports)} outputs; {time.monotonic()-started:.1f}s', flush=True)
    return exports, records


def classify(extension):
    """Describe actual decoder support, without treating native audio as playable."""
    if extension == 'glb':
        return 'mesh', True
    if extension in ('png', 'jpg', 'jpeg', 'webp'):
        return 'texture', True
    if extension in ('wav', 'ogg', 'mp3', 'flac'):
        return 'audio', True
    if extension == 'json':
        return 'data', True
    if extension in ('ueanim', 'uemodel'):
        return 'animation' if extension == 'ueanim' else 'skeleton', False
    return 'audio' if extension in ('rada', 'bnk', 'wem') else 'native', False


def import_outputs(source, exports):
    """Verify all media and preserve each unique payload via a same-volume hardlink."""
    files = OUTPUT / 'files'
    files.mkdir(parents=True, exist_ok=True)
    assets, errors, checked = [], [], {}
    root = source.resolve()
    def import_one(item):
        relative, record = item
        path = (source / relative).resolve()
        if not path.is_relative_to(root):
            raise ValueError(f'Path outside extraction: {relative}')
        try:
            signature = path.stat()
            expected = record['sha256'].lower()
            identity = (signature.st_dev, signature.st_ino, signature.st_size, signature.st_mtime_ns)
            actual = checked.get(identity)
            if actual is None:
                actual = digest(path)
                checked[identity] = actual
            if actual != expected or signature.st_size != record['bytes']:
                raise ValueError('Saved size or SHA-256 does not match actual file')
            extension = path.suffix.lower().lstrip('.')
            target = files / f'{actual}.{extension}'
            if not target.exists():
                try:
                    os.link(path, target)
                except FileExistsError:
                    pass  # Another verified payload with this same hash won the race.
            if not os.path.samefile(path, target) and digest(target) != actual:
                raise ValueError('Existing destination does not match content hash')
            kind, supported = classify(extension)
            label = path.stem
            return {
                'id': hashlib.sha256(relative.encode()).hexdigest()[:24],
                'label': label, 'type': kind, 'format': extension,
                'url': f'./files/{target.name}', 'bytes': signature.st_size,
                'sha256': actual, 'supported': supported,
                'source': relative, 'className': record.get('className'),
                'package': record.get('package'), 'objectPath': record.get('objectPath'),
            }, None
        except (OSError, ValueError) as error:
            return None, {'source': relative, 'error': str(error)}
    items = sorted(exports.items())
    with ThreadPoolExecutor(max_workers=8) as workers:
        # Bounded batches avoid hundreds of thousands of Future objects in memory.
        for offset in range(0, len(items), 128):
            for asset, error in workers.map(import_one, items[offset:offset + 128]):
                if asset:
                    assets.append(asset)
                if error:
                    errors.append(error)
            if offset % 1024 == 0:
                print(f'IMPORT {min(offset+128,len(items))}/{len(exports)}; verified {len(assets)}; errors {len(errors)}', flush=True)
    return assets, errors


def main():
    """Write the full catalog and a durable integrity report for the migration."""
    source = SOURCE if SOURCE.exists() else ARCHIVE
    REPORTS.mkdir(parents=True, exist_ok=True)
    cache = REPORTS / 'conversion-outputs.json'
    metadata_path = REPORTS / 'conversion-index.json'
    manifest_stat = (source / 'conversion-manifest.jsonl').stat()
    if '--from-index' in sys.argv:
        metadata = json.loads(metadata_path.read_text(encoding='utf-8'))
        if metadata['bytes'] != manifest_stat.st_size or metadata['mtimeNs'] != manifest_stat.st_mtime_ns:
            raise ValueError('Conversion manifest changed; rebuild its index')
        exports = json.loads(cache.read_text(encoding='utf-8'))
        records = metadata['records']
    else:
        exports, records = collect_outputs(source)
        cache.write_text(json.dumps(exports), encoding='utf-8')
        metadata_path.write_text(json.dumps({'bytes': manifest_stat.st_size, 'mtimeNs': manifest_stat.st_mtime_ns, 'records': records}), encoding='utf-8')
    for folder in ('probe', 'audio-animation-probe'):
        if (source / folder / 'conversion-manifest.jsonl').exists():
            probes, _ = collect_outputs(source / folder)
            for relative, record in probes.items():
                exports[folder + '/' + relative] = record
    assets, errors = import_outputs(source, exports)
    summary = {
        'date': '2026-11-16', 'manifestRecords': records,
        'exportedFiles': len(exports), 'verifiedAssets': len(assets),
        'uniqueFiles': len({a['url'] for a in assets}),
        'logicalBytes': sum(a['bytes'] for a in assets),
        'types': dict(Counter(a['type'] for a in assets)),
        'formats': dict(Counter(a['format'] for a in assets)),
        'errors': errors, 'allVerified': not errors,
    }
    catalog = {'schemaVersion': 1, 'summary': summary, 'assets': assets}
    (OUTPUT / 'catalog.json').write_text(json.dumps(catalog, ensure_ascii=False), encoding='utf-8')
    (REPORTS / 'import-verification.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
    print(json.dumps(summary, ensure_ascii=False)[:6000], flush=True)
    if errors:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
