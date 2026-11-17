"""Exercise the actual native decoder and cache guards with preserved source copies."""
import array
import hashlib
import json
import shutil
import subprocess
import tempfile
import unittest
import wave
from pathlib import Path
from urllib.parse import unquote
from native_audio import NativeAudio, ROOT, digest


class NativeAudioTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.decoder = ROOT / 'target/native-audio/decode_rada.exe'
        if not cls.decoder.exists():
            raise RuntimeError('Build the local decoder before running these tests')
        cls.catalog = json.loads((ROOT / 'dist/assets/imported/types/audio.json').read_text('utf-8'))

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='fightnight-audio-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.entry = next(e for e in self.catalog if e['id'] == 'a805539e442e5088a498c849').copy()
        self.source = self.root / 'docs/imported/fortnite-analysis/assets/converted/beep.rada'
        self.source.parent.mkdir(parents=True)
        shutil.copyfile(ROOT / 'assets/imported' / unquote(self.entry['url']), self.source)
        self.entry['url'] = '../../docs/imported/fortnite-analysis/assets/converted/beep.rada'
        self.converter = NativeAudio(self.root, self.decoder)
        self.converter.entries = {self.entry['id']: self.entry}

    def test_real_pcm_frame_count_and_non_silence(self):
        before = digest(self.source)
        result = self.converter.convert(self.entry['id'])
        wav_path = self.root / 'dist/assets/imported' / result['url']
        with wave.open(str(wav_path), 'rb') as wav:
            self.assertEqual((wav.getnchannels(), wav.getframerate(), wav.getnframes()), (1, 48000, 34702))
            samples = array.array('h', wav.readframes(wav.getnframes()))
        self.assertGreater(max(abs(n) for n in samples), 100)
        self.assertEqual(before, digest(self.source))
        self.assertEqual(result, self.converter.convert(self.entry['id']))

    def test_rejects_changed_original_and_changed_cache(self):
        result = self.converter.convert(self.entry['id'])
        wav = self.root / 'dist/assets/imported' / result['url']
        wav.write_bytes(wav.read_bytes()[:-1] + b'X')
        with self.assertRaisesRegex(ValueError, 'Cached WAV integrity'):
            self.converter.convert(self.entry['id'])
        self.source.write_bytes(self.source.read_bytes() + b'X')
        with self.assertRaisesRegex(ValueError, 'Source SHA-256/size'):
            self.converter.convert(self.entry['id'])

    def test_rejects_unknown_ids_and_archive_escape(self):
        for identifier in ['../secret', 'a'*24]:
            with self.assertRaises(ValueError): self.converter.convert(identifier)
        self.entry['url'] = '../../README.md'
        with self.assertRaisesRegex(ValueError, 'outside preserved archive'):
            self.converter.convert(self.entry['id'])

    def test_decoder_rejects_truncation_and_invalid_seek_envelope(self):
        source = self.source.read_bytes()
        changed = bytearray(source); changed[172] = 255
        for i, data in enumerate([source[:100], changed]):
            input_path = self.root / f'bad-{i}.rada'; output = self.root / f'bad-{i}.wav'
            input_path.write_bytes(data)
            result = subprocess.run([str(self.decoder), str(input_path), str(output)], capture_output=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(output.exists())


if __name__ == '__main__': unittest.main()
