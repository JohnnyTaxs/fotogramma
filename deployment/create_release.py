"""Create or update a GitHub release and upload every ZIP asset via its API."""

from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import urllib.error
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
NOTES_DIR = ROOT / "docs" / "releases"


def release_notes(tag: str) -> str:
    """Return the notes file for this tag, empty when none is published."""
    notes = NOTES_DIR / f"{tag}.md"
    if notes.is_file():
        return notes.read_text(encoding="utf-8").strip()
    return ""


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

    def create_release(self, tag: str, body: str = "") -> dict:
        path = f"/repos/{self.repository}/releases"
        payload = {"tag_name": tag, "name": f"Fotogramma {tag}", "draft": False, "prerelease": False}
        if body:
            payload["body"] = body
        else:
            payload["generate_release_notes"] = True
        try:
            return self.request("POST", path, payload)
        except RuntimeError as error:
            if "HTTP 422" not in str(error) and "HTTP 409" not in str(error):
                raise
        release = self.request("GET", f"{path}/tags/{urllib.parse.quote(tag, safe='')}")
        if body and release.get("body") != body:
            release = self.request("PATCH", f"{path}/{release['id']}", {"body": body})
        return release

    def delete_asset_if_present(self, release: dict, name: str) -> None:
        for asset in release.get("assets", []):
            if asset.get("name") == name:
                self.request("DELETE", f"/repos/{self.repository}/releases/assets/{asset['id']}")

    def upload_asset(self, release: dict, path: Path) -> None:
        self.delete_asset_if_present(release, path.name)
        query = urllib.parse.urlencode({"name": path.name})
        upload_url = release.get(
            "upload_url",
            f"{self.api_url}/repos/{self.repository}/releases/{release['id']}/assets",
        ).split("{", 1)[0]
        self.request(
            "POST",
            f"{upload_url}?{query}",
            path.read_bytes(),
            content_type="application/zip",
        )


def main() -> int:
    if len(sys.argv) != 2:
        raise SystemExit("Usage: python deployment/create_release.py <asset-directory>")
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("FORGEJO_TOKEN")
    if not token:
        raise SystemExit("GITHUB_TOKEN or FORGEJO_TOKEN is missing")
    repository = os.environ.get("GITHUB_REPOSITORY")
    api_url = os.environ.get("GITHUB_API_URL", "https://api.github.com")
    tag = os.environ.get("GITHUB_REF_NAME")
    if not repository or not tag:
        raise SystemExit("GITHUB_REPOSITORY and GITHUB_REF_NAME are required")

    assets = sorted(Path(sys.argv[1]).glob("*.zip"))
    if not assets:
        raise SystemExit("No ZIP archives to publish")
    client = ReleaseClient(api_url, repository, token)
    release = client.create_release(tag, release_notes(tag))
    for asset in assets:
        client.upload_asset(release, asset)
        print(f"Uploaded: {asset.name}")
    print(f"Release updated: {release.get('html_url', release.get('url', tag))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
