# CI/CD with GitHub Actions

The workflow `.github/workflows/build-release.yml` runs on every push to `main`, pull request, `v*` tag, and manual dispatch.

## Platforms

| Job | Runner | Output |
| --- | --- | --- |
| Linux | `ubuntu-latest` with `node:22-bookworm` | `Fotogramma-Linux-x86_64.zip` |
| Windows | `windows-latest` | `Fotogramma-Windows-amd64.zip` |
| macOS ARM | `macos-15` | `Fotogramma-Darwin-arm64.zip` |

**macOS Intel is intentionally excluded.** Architecture names come from `platform.machine()` during the build, so archive names can change only if a runner changes.

Linux uses Debian with system Python and PySide6 installed through `apt`. Windows and macOS use uv. The actual Linux container is `node:22-bookworm`, not Alpine.

Each build job performs:

1. `actions/checkout@v4`
2. Linux: `apt-get` installs Python, pip, the shared Python runtime required by PyInstaller, and the OpenGL, D-Bus, font, GLib, X11/XCB, and xkbcommon runtime libraries listed in the workflow, followed by `python3 -m pip install --break-system-packages pyside6-essentials`; Windows/macOS: `actions/setup-python@v5` and `astral-sh/setup-uv@v5`
3. FFmpeg installation through Homebrew only on macOS ARM
4. All `unittest` tests with Qt `offscreen`
5. `deployment/build.py`
6. ZIP upload through `actions/upload-artifact@v4` with a 3-day retention

On Windows and Linux, `deployment/prepare_ffprobe.py` downloads static FFmpeg builds. On macOS it runs `brew install ffmpeg || brew upgrade ffmpeg`.

## Do I need dedicated machines?

Not necessarily physical machines, but PyInstaller is not a cross-compiler: each artifact must be built on its own operating system. Compatible environments are required:

- **Linux:** the existing Docker runner is sufficient, using `node:22-bookworm` and `apt`.
- **Windows:** a Windows host or VM with label `windows-latest` to produce `Fotogramma.exe`.
- **macOS ARM:** an Apple Silicon host with label `macos-15` to produce `Fotogramma.app`; Intel is not used.

Self-hosted VMs and GitHub-hosted runners are both valid. If the pool has no compatible Windows/macOS runners, use GitHub-hosted runners with those labels or provision two VMs dedicated to running actions.

## Free GitHub-hosted runners

For public repositories, standard GitHub-hosted runners are free, including Windows and macOS. This avoids maintaining dedicated Windows or Apple Silicon VMs for this project.

For private repositories, GitHub provides limited monthly compute and artifact storage. Windows and macOS usage costs more than Linux, and artifact storage can accumulate quickly with large application bundles. A public mirror for CI is therefore the simplest option for this project.

## Automatic releases

The `release` job runs only for `refs/tags/` after all build jobs succeed. It downloads all artifacts, configures Python with `actions/setup-python@v5`, and invokes `deployment/create_release.py`; no privileged package installation is required.

The script uses the GitHub API directly:

- `POST /repos/{owner}/{repo}/releases` to create a release;
- `GET /repos/{owner}/{repo}/releases/tags/{tag}` when it already exists;
- deletion of an existing asset with the same name before re-uploading, making reruns idempotent;
- upload of every ZIP with `Content-Type: application/zip` through the release's dedicated `upload_url` endpoint.

There are no release actions tied to another hosting provider: `GITHUB_API_URL` comes from the GitHub Actions context (`github.api_url`).

## Token and permissions

The release job has `contents: write` and uses the automatically provided `secrets.GITHUB_TOKEN`. `create_release.py` also accepts `FORGEJO_TOKEN` as a fallback for other integrations. Tokens are never printed in logs.

If automatic tokens are unavailable, create a token with `write:repository` permission and add it as the `GITHUB_TOKEN` secret in the repository settings.

## Runner requirements

Labels `ubuntu-latest`, `windows-latest`, and `macos-15` must exist in the runner pool. If self-hosted runners use different labels, update the workflow. Runners must be able to reach:

- GitHub or the action mirror for action downloads;
- PyPI for dependencies;
- Homebrew for macOS;
- gyan.dev and johnvansickle.com for static binaries;
- the GitHub API of the instance.

## Publishing a release

```bash
git tag -a v1.0.0 -m "Fotogramma Desktop 1.0.0"
git push origin main
git push origin v1.0.0
```

The tag starts all builds and then the release job. To rerun without creating a tag, use `workflow_dispatch` or rerun the failed job. The same release and assets are updated without duplicates.

## Release client tests

`tests/test_create_release.py` uses a local HTTP server to verify:

- release creation;
- retrieval of an existing release;
- asset replacement on a second upload;
- no token in the logs.

Run it with:

```bash
uv run --no-project --with pyside6-essentials python -m unittest tests.test_create_release -v
```
