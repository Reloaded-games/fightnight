"""Catalog the entire preserved extraction without rewriting 63 GB of payloads.

The source is subsequently moved by same-volume NTFS rename, checked by file ID.
This is conservation of the original files, not a claim that every original was
rehashed. Browser previews verify their declared SHA-256 before decoding.
"""
from collections import Counter
import hashlib
import json
from pathlib import Path, PurePosixPath
from urllib.parse import quote

from import_extracted import ROOT, SOURCE, ARCHIVE, OUTPUT, REPORTS, classify, collect_outputs


def main():
    source = SOURCE if SOURCE.exists() else ARCHIVE
    metadata = json.loads((REPORTS / 'conversion-index.json').read_text())
    stat = (source / 'conversion-manifest.jsonl').stat()
    if metadata['bytes'] != stat.st_size or metadata['mtimeNs'] != stat.st_mtime_ns:
        raise ValueError('Manifest changed after complete indexing')
    outputs = json.loads((REPORTS / 'conversion-outputs.json').read_text())
    for group in ('probe', 'audio-animation-probe'):
        extra, _ = collect_outputs(source / group)
        outputs.update({group + '/' + key: value for key, value in extra.items()})
    roots = {
        'main': 'converted', 'probe': 'probe/converted',
        'audio-animation-probe': 'audio-animation-probe/converted',
    }
    identities = {}
    for group, relative in roots.items():
        directory = source / relative
        if directory.is_symlink() or directory.is_junction():
            raise ValueError('Expected the original physical extraction directory')
        entry = directory.stat()
        identities[group] = {'relative': relative, 'device': entry.st_dev, 'inode': entry.st_ino}
    assets = []
    for relative, entry in outputs.items():
        path = PurePosixPath(relative)
        if path.is_absolute() or '..' in path.parts:
            raise ValueError('Source path escaped extraction')
        group = 'main' if relative.startswith('converted/') else path.parts[0]
        prefix = roots[group] + '/'
        if not relative.startswith(prefix):
            raise ValueError('Unexpected source prefix')
        payload = relative[len(prefix):]
        kind, supported = classify(path.suffix.lower().lstrip('.'))
        assets.append({
            'id': hashlib.sha256(relative.encode()).hexdigest()[:24],
            'label': path.stem, 'type': kind, 'format': path.suffix.lower().lstrip('.'),
            'url': './sources/' + group + '/' + quote(payload),
            'bytes': entry['bytes'], 'sha256': entry['sha256'].lower(),
            'supported': supported, 'source': relative,
            'className': entry.get('className'), 'package': entry.get('package'),
            'objectPath': entry.get('objectPath'),
        })
    assets.sort(key=lambda a: (a['label'].casefold(), a['source']))
    summary = {
        'date': '2026-11-16', 'manifestRecords': metadata['records'],
        'exportedFiles': len(assets), 'catalogAssets': len(assets),
        'uniquePayloadHashes': len({a['sha256'] for a in assets}),
        'logicalBytes': sum(a['bytes'] for a in assets),
        'types': dict(Counter(a['type'] for a in assets)),
        'formats': dict(Counter(a['format'] for a in assets)),
        'integrityMode': 'same-volume directory identity preservation; SHA-256 verified on browser preview/use',
        'allPayloadsHashed': False, 'sourceDirectories': identities,
        'sourceDirectoryVerified': True, 'errors': [],
    }
    OUTPUT.mkdir(exist_ok=True)
    with (OUTPUT / 'catalog.json').open('w', encoding='utf-8') as stream:
        json.dump({'schemaVersion': 2, 'summary': summary, 'assets': assets}, stream, ensure_ascii=False)
    (REPORTS / 'import-verification.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
    print(json.dumps(summary, ensure_ascii=False), flush=True)


if __name__ == '__main__':
    main()
