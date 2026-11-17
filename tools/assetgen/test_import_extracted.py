"""Integrity and resumability checks using real files and hardlinks."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import import_extracted as importer


class ImportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / 'source'
        self.source.mkdir()

    def test_package_only_resumed_output_is_preserved(self):
        output = {'relativePath': 'a.glb', 'bytes': 3, 'sha256': 'abc'}
        records = [{'kind': 'package', 'status': 'resumed', 'outputs': [output]}]
        (self.source / 'conversion-manifest.jsonl').write_text('\n'.join(map(json.dumps, records)))
        exports, count = importer.collect_outputs(self.source)
        self.assertEqual(count, 1)
        self.assertEqual(exports['a.glb']['sha256'], 'abc')

    def test_verified_hardlink_survives_source_unlink_and_repeat_import(self):
        path = self.source / 'a.png'
        path.write_bytes(b'original extraction')
        record = {'sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'bytes': path.stat().st_size}
        with patch.object(importer, 'OUTPUT', self.root / 'output'):
            assets, errors = importer.import_outputs(self.source, {'a.png': record})
            self.assertFalse(errors)
            repeated, errors = importer.import_outputs(self.source, {'a.png': record})
            self.assertEqual(assets, repeated)
            self.assertFalse(errors)
            target = importer.OUTPUT / assets[0]['url']
            self.assertTrue(path.samefile(target))
            path.unlink()
            self.assertEqual(target.read_bytes(), b'original extraction')

    def test_mismatch_never_creates_an_asset(self):
        (self.source / 'a.png').write_bytes(b'changed')
        with patch.object(importer, 'OUTPUT', self.root / 'output'):
            assets, errors = importer.import_outputs(self.source, {'a.png': {'sha256': '0'*64, 'bytes': 7}})
            self.assertFalse(assets)
            self.assertEqual(len(errors), 1)

    def test_path_outside_source_is_rejected(self):
        with patch.object(importer, 'OUTPUT', self.root / 'output'):
            with self.assertRaises(ValueError):
                importer.import_outputs(self.source, {'../secret': {}})


if __name__ == '__main__':
    unittest.main()
