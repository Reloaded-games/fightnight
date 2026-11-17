"""Verified, on-demand local RADA conversion. The decoder stays outside the web build."""
import hashlib
import json
import os
import subprocess
import threading
import uuid
import wave
from pathlib import Path
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    """Hash the actual bytes, including an existing cached conversion."""
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


class NativeAudio:
    """Resolve catalog IDs only; reject missing, changed and incomplete source payloads."""
    def __init__(self, root=ROOT, decoder=None):
        self.root = Path(root)
        self.decoder = Path(decoder or self.root / 'target/native-audio/decode_rada.exe')
        self.entries = None
        self.lock = threading.Lock()

    def convert(self, asset_id):
        """Convert one bounded sound atomically and return source/output integrity metadata."""
        if not isinstance(asset_id, str) or len(asset_id) != 24 or any(c not in '0123456789abcdef' for c in asset_id):
            raise ValueError('Invalid catalog ID')
        with self.lock:
            if self.entries is None:
                catalog = json.loads((self.root / 'dist/assets/imported/types/audio.json').read_text('utf-8'))
                self.entries = {entry['id']: entry for entry in catalog}
            entry = self.entries.get(asset_id)
            if not entry or entry['format'] != 'rada':
                raise ValueError('RADA entry not found')
            source = (self.root / 'assets/imported' / unquote(entry['url'])).resolve()
            archive = (self.root / 'docs/imported/fortnite-analysis/assets/converted').resolve()
            if not source.is_relative_to(archive):
                raise ValueError('Source outside preserved archive')
            if source.stat().st_size != entry['bytes'] or digest(source) != entry['sha256']:
                raise ValueError('Source SHA-256/size mismatch')
            cache = self.root / 'dist/assets/imported/audio-cache'
            cache.mkdir(parents=True, exist_ok=True)
            target = cache / (entry['sha256'] + '.wav')
            receipt = target.with_suffix('.json')
            if target.exists():
                if not receipt.exists() or digest(target) != json.loads(receipt.read_text())['sha256']:
                    raise ValueError('Cached WAV integrity mismatch')
            else:
                if not self.decoder.is_file():
                    raise ValueError('Run scripts/build-native-audio.ps1 first')
                temporary = cache / (uuid.uuid4().hex + '.wav')
                native_source = '\\\\?\\' + str(source) if os.name == 'nt' else str(source)
                result = subprocess.run([str(self.decoder), native_source, str(temporary)], capture_output=True, text=True, timeout=45)
                if result.returncode:
                    raise ValueError(result.stderr.strip() or 'Native audio decode failed')
                with wave.open(str(temporary), 'rb') as wav:
                    metadata = {'frames': wav.getnframes(), 'sampleRate': wav.getframerate(), 'channels': wav.getnchannels()}
                    if wav.getsampwidth() != 2 or not metadata['frames']:
                        raise ValueError('Invalid decoded PCM')
                output_hash = digest(temporary)
                os.replace(temporary, target)
                receipt.write_text(json.dumps({**metadata, 'sha256': output_hash, 'sourceSha256': entry['sha256']}), encoding='utf-8')
            result = json.loads(receipt.read_text('utf-8'))
            return {**result, 'id': entry['id'], 'label': entry['label'], 'url': './audio-cache/' + target.name}


if __name__ == '__main__':
    import sys
    converter = NativeAudio()
    for identifier in sys.argv[1:]:
        print(json.dumps(converter.convert(identifier)), flush=True)
