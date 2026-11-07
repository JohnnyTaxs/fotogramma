# CI/CD su Forgejo

Il workflow `.forgejo/workflows/build-release.yml` esegue test e build su ogni push a `main`, pull request, tag `v*` e avvio manuale.

## Piattaforme

| Job | Runner | Output |
| --- | --- | --- |
| Linux | `docker` | `Fotogramma-Linux-x86_64.zip` |
| Windows | `windows-latest` | `Fotogramma-Windows-amd64.zip` |
| macOS ARM | `macos-15` | `Fotogramma-Darwin-arm64.zip` |

**macOS Intel è escluso intenzionalmente.** Le architetture derivano da `platform.machine()` durante la build, quindi i nomi degli archivi possono variare solo se cambia il runner. Per Linux si usa il label `docker`, coerente con gli altri workflow self-hosted della infrastruttura; `actions/setup-python` installa Python 3.13 nel container del job.

Il container Linux effettivo del runner è `node:22-bookworm`, quindi si usa Debian con Python e PySide6 installati tramite `apt`. Non viene invocato `uv run`; PyInstaller viene installato tramite pip nel Python di sistema. Windows e macOS continuano a usare uv.

Ogni job esegue:

1. `actions/checkout@v4`
2. Su Linux (`node:22-bookworm`): `apt-get install python3 python3-pip python3-pyside6`; su Windows/macOS: `actions/setup-python@v5` e `astral-sh/setup-uv@v5`
3. Installazione FFmpeg con Homebrew solo su macOS ARM
4. Tutti i test `unittest` con Qt `offscreen`
5. Build `deployment/build.py`
6. Upload dello ZIP tramite `actions/upload-artifact@v4`

Su Windows e Linux, `deployment/prepare_ffprobe.py` scarica automaticamente le build statiche di FFmpeg. Su macOS viene usato `brew install ffmpeg || brew upgrade ffmpeg`.

## Release automatica

Il job `release` parte soltanto per ref in `refs/tags/`, dopo il completamento di tutti i build job. Scarica tutti gli artifact e invoca `deployment/create_release.py`.

Lo script usa direttamente l’API Forgejo:

- `POST /repos/{owner}/{repo}/releases` per creare la release;
- `GET /repos/{owner}/{repo}/releases/tags/{tag}` se esiste già;
- elimina un asset con lo stesso nome prima di ricaricarlo, rendendo il rilancio idempotente;
- carica tutti gli ZIP con `Content-Type: application/zip`.

Non vengono usate action di release specifiche per GitHub: `GITHUB_API_URL` viene impostato automaticamente dal contesto Forgejo (`github.api_url`).

## Token e permessi

Il job usa `secrets.GITHUB_TOKEN` quando disponibile. `create_release.py` accetta anche `FORGEJO_TOKEN` come fallback.

Alcune versioni di Forgejo ignorano il campo `permissions` del workflow: per questo il file non lo include più. Abilitare la pubblicazione tramite **Authorized Integrations**, assegnando al job/release il permesso di scrittura sulla repository. Se l’istanza non fornisce token automatici, creare un token Forgejo con permesso `write:repository` e aggiungerlo come secret `GITHUB_TOKEN` nelle impostazioni della repository. Il token non viene mai stampato nei log.

## Requisiti dei runner

I label `docker`, `windows-latest` e `macos-15` devono esistere nel pool Forgejo Actions. Se i runner self-hosted usano label diverse, aggiornare `matrix.os` nel workflow. I runner devono poter accedere a:

- GitHub o al mirror Forgejo per scaricare le action;
- PyPI per le dipendenze;
- Homebrew per macOS;
- gyan.dev e johnvansickle.com per i binari statici;
- l’API Forgejo dell’istanza.

## Pubblicare una release

```bash
git tag -a v1.0.0 -m "Fotogramma Desktop 1.0.0"
git push origin main
git push origin v1.0.0
```

Il push del tag avvia tutte le build e poi il job di release. Per rilanciare senza creare un nuovo tag, usare `workflow_dispatch` oppure riavviare il job fallito. La stessa release e gli stessi asset verranno aggiornati senza duplicati.

## Test del client di release

`tests/test_create_release.py` usa un server HTTP locale per verificare:

- creazione della release;
- recupero di una release già esistente;
- sostituzione degli asset al secondo upload;
- assenza del token nei log.

Eseguirlo con:

```bash
uv run --no-project --with pyside6-essentials python -m unittest tests.test_create_release -v
```
