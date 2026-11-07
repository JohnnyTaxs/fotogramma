"""Create or update a GitHub release and upload every ZIP asset via its API."""

from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import urllib.error
import urllib.parse
import urllib.request


class ReleaseClient:
    def __init__(self, api_url: str, repository: str, token: str) -> None:
        self.api_url = api_url.rstrip("/")
        self.repository = repository
        self.token = token

    def request(self, method: str, path: str, payload=None, content_type="application/json"):
        url = path if path.startswith("http") else f"{self.api_url}{path}"
        data = None if payload is None else payload
        if isinstance(payload, (dict, list)):
            data = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(url, data=data, method=method)
        request.add_header("Authorization", f"token {self.token}")
        request.add_header("Accept", "application/vnd.github+json")
        if data is not None:
            request.add_header("Content-Type", content_type)
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                body = response.read()
                return json.loads(body) if body else {}
        except urllib.error.HTTPError as error:
            details = error.read().decode("utf-8", errors="replace")
            raise RuntimeError(f"GitHub API {method} {path}: HTTP {error.code}: {details}") from error

    def create_release(self, tag: str) -> dict:
        path = f"/repos/{self.repository}/releases"
        try:
            return self.request("POST", path, {"tag_name": tag, "name": f"Fotogramma {tag}", "draft": False, "prerelease": False})
        except RuntimeError as error:
            if "HTTP 422" not in str(error) and "HTTP 409" not in str(error):
                raise
        return self.request("GET", f"{path}/tags/{urllib.parse.quote(tag, safe='')}")

    def delete_asset_if_present(self, release: dict, name: str) -> None:
        for asset in release.get("assets", []):
            if asset.get("name") == name:
                self.request("DELETE", f"/repos/{self.repository}/releases/assets/{asset['id']}")

    def upload_asset(self, release: dict, path: Path) -> None:
        self.delete_asset_if_present(release, path.name)
        query = urllib.parse.urlencode({"name": path.name})
        self.request(
            "POST",
            f"/repos/{self.repository}/releases/{release['id']}/assets?{query}",
            path.read_bytes(),
            content_type="application/zip",
        )


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("Uso: python deployment/create_release.py <directory-asset>")
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("FORGEJO_TOKEN")
    if not token:
        raise SystemExit("GITHUB_TOKEN o FORGEJO_TOKEN mancante")
    repository = os.environ.get("GITHUB_REPOSITORY")
    api_url = os.environ.get("GITHUB_API_URL", "https://api.github.com")
    tag = os.environ.get("GITHUB_REF_NAME")
    if not repository or not tag:
        raise SystemExit("GITHUB_REPOSITORY e GITHUB_REF_NAME sono obbligatori")

    assets = sorted(Path(sys.argv[1]).glob("*.zip"))
    if not assets:
        raise SystemExit("Nessun archivio ZIP da pubblicare")
    client = ReleaseClient(api_url, repository, token)
    release = client.create_release(tag)
    for asset in assets:
        client.upload_asset(release, asset)
        print(f"Caricato: {asset.name}")
    print(f"Release aggiornata: {release.get('html_url', release.get('url', tag))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
