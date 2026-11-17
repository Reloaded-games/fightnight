"""Serve the local game plus bounded, catalog-only audio conversion on loopback."""
import argparse
import json
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

from assetgen.native_audio import NativeAudio, ROOT


def make_handler(converter):
    class Handler(SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(converter.root / 'dist'), **kwargs)

        def do_GET(self):
            path = urlsplit(self.path).path
            if not path.startswith('/api/imported/audio/'):
                return super().do_GET()
            # Expensive native work is available only from this local origin.
            if self.headers.get('Sec-Fetch-Site') == 'cross-site':
                self.send_error(403)
                return
            try:
                data = converter.convert(path.removeprefix('/api/imported/audio/'))
                status = 200
            except Exception as error:
                data, status = {'error': str(error)}, 422
            payload = json.dumps(data).encode('utf-8')
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(payload)))
            self.send_header('Cache-Control', 'no-store')
            self.end_headers()
            self.wfile.write(payload)
    return Handler


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--port', type=int, default=5196)
    args = parser.parse_args()
    server = ThreadingHTTPServer(('127.0.0.1', args.port), make_handler(NativeAudio(ROOT)))
    print(f'Fightnight: http://127.0.0.1:{server.server_port}/', flush=True)
    server.serve_forever()
