import io
import json
import os
import sys
import tempfile
import threading
import unittest
from contextlib import redirect_stdout
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from unittest.mock import patch
from urllib.parse import parse_qs, urlsplit
import urllib.request
from urllib.request import ProxyHandler, build_opener

from deployment.create_release import main as create_release


class ReleaseHandler(BaseHTTPRequestHandler):
    release = None
    assets = []
    next_asset_id = 1

    def log_message(self, *_args):
        pass

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        if self.path == "/api/v3/repos/JohnnyTax/fotogramma-desktop/releases":
            if self.release is not None:
                self.send_error(422, "Already exists")
                return
            payload = json.loads(body)
            self.__class__.release = {"id": 42, "tag_name": payload["tag_name"], "html_url": "https://github.com/JohnnyTaxs/fotogramma/releases/tag/v1.0.0", "assets": []}
            self._json(201, self.release)
            return
        if self.path.startswith("/api/v3/repos/JohnnyTax/fotogramma-desktop/releases/42/assets"):
            name = parse_qs(urlsplit(self.path).query)["name"][0]
            asset = {"id": self.next_asset_id, "name": name, "size": len(body)}
            self.__class__.next_asset_id += 1
            self.__class__.assets.append(asset)
            self._json(201, asset)
            return
        self.send_error(404)

    def do_GET(self):
        if self.path == "/api/v3/repos/JohnnyTax/fotogramma-desktop/releases/tags/v1.0.0":
            if self.release is None:
                self.send_error(404)
            else:
                self._json(200, {**self.release, "assets": self.assets})
            return
        self.send_error(404)

    def do_DELETE(self):
        asset_id = int(self.path.rsplit("/", 1)[-1])
        self.__class__.assets = [asset for asset in self.assets if asset["id"] != asset_id]
        self.send_response(204)
        self.end_headers()

    def _json(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class CreateReleaseTests(unittest.TestCase):
    def setUp(self):
        ReleaseHandler.release = None
        ReleaseHandler.assets = []
        ReleaseHandler.next_asset_id = 1
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), ReleaseHandler)
        self.port = self.server.server_address[1]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.previous_opener = urllib.request._opener
        urllib.request._opener = build_opener(ProxyHandler({}))

    def tearDown(self):
        urllib.request._opener = self.previous_opener
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)

    def test_create_and_update_release_assets_without_logging_token(self):
        with tempfile.TemporaryDirectory() as directory:
            Path(directory, "Fotogramma-Linux-x86_64.zip").write_bytes(b"linux")
            Path(directory, "Fotogramma-Windows-amd64.zip").write_bytes(b"windows")
            env = {
                "GITHUB_TOKEN": "secret-token",
                "GITHUB_API_URL": f"http://127.0.0.1:{self.port}/api/v3",
                "GITHUB_REPOSITORY": "JohnnyTax/fotogramma-desktop",
                "GITHUB_REF_NAME": "v1.0.0",
                "NO_PROXY": "127.0.0.1,localhost",
                "no_proxy": "127.0.0.1,localhost",
            }
            with patch.dict(os.environ, env, clear=True), patch.object(sys, "argv", ["create_release.py", directory]):
                output = io.StringIO()
                with redirect_stdout(output):
                    create_release()
                    create_release()
        self.assertEqual(len(ReleaseHandler.assets), 2)
        self.assertNotIn("secret-token", output.getvalue())
        self.assertEqual(ReleaseHandler.release["tag_name"], "v1.0.0")


if __name__ == "__main__":
    unittest.main()
