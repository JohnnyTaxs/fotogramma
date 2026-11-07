# AGENTS.md

Regole per agenti che modificano **Fotogramma Desktop**, il Progetto 2 nativo. La webapp in `h265-check` è un progetto separato: non trasformare questa app in un launcher o in una WebView.

## Invarianti

- **Solo widget Qt nativi** tramite PySide6 Essentials. Niente server HTTP, Electron, Tauri o Qt WebEngine. Il browser esterno è ammesso solo per aprire la ricerca 1337x.to richiesta dall’utente.
- **Gli originali non vengono mai sovrascritti.** La conversione crea nuovi file `.h265` con output progressivo e `ffmpeg -n`.
- `hevc` e `av1` sono `compatible`; gli altri codec validi sono `convert`; i problemi di lettura sono `error`.
- Analisi del solo primo stream video (`v:0`) di `.mp4`, `.mkv` e `.webm`.
- Nessuna dipendenza CDN o servizio online a runtime.
- `QSettings` conserva solo directory e geometria, non i risultati della scansione.
- Build **onedir** specifica per OS/architettura, non onefile e non cross-compilata.

## Comandi

```bash
# Avvio sorgente
uv run --no-project --with pyside6-essentials python -m fotogramma

# Test completi
QT_QPA_PLATFORM=offscreen \
  uv run --no-project --with pyside6-essentials \
  python -m unittest discover -s tests -v

# Build
uv run --no-project --with pyside6-essentials --with pyinstaller==6.22.3 \
  python deployment/build.py

# Controlli statici
python3 -m compileall -q fotogramma runtime.py deployment
git diff --check
```

Non installare il pacchetto completo `PySide6`: include WebEngine e altre dipendenze non necessarie. Usare sempre **PySide6-Essentials**.

## Architettura

| File | Regole |
| --- | --- |
| `fotogramma/app.py` | Entry point, font e timer smoke test. Non aggiungere server. |
| `fotogramma/core/scanner.py` | Discovery, Popen, classificazione e cancellazione. Resta indipendente da Qt. |
| `fotogramma/ui/workers.py` | `ScanWorker` e `ConversionWorker`. Nessuna operazione UI nei thread. |
| `fotogramma/ui/main_window.py` | Tutte le interazioni avvengono nel GUI thread, inclusa la selezione multipla. |
| `fotogramma/core/conversion.py` | Profili e argomenti FFmpeg. Usare `-n`, `-map 0` e output senza sovrascrittura. |
| `fotogramma/core/media_tools.py` | Risoluzione di ffmpeg in sviluppo e frozen. |
| `fotogramma/core/search.py` | Solo URL esterno 1337x.to; nessuna richiesta di rete integrata. |
| `fotogramma/ui/conversion_dialog.py` | Profili, anteprima comando e avanzamento. Mantieni controlli nel GUI thread. |
| `fotogramma/ui/models.py` | Modello/proxy per grandi librerie, ruoli custom e badge. |
| `fotogramma/ui/theme.py` | QSS Cinema Pop; non introdurre HTML/CSS o un secondo tema. |
| `runtime.py` | `resource_path` per asset in sviluppo e frozen. |
| `deployment/` | Build, download e vendor. Non rinominarla `packaging`: entrerebbe in conflitto con il pacchetto Python omonimo. |

Il flusso è: directory → `discover_videos` → `probe_videos` → segnali Qt → modello → proxy → `QTableView`.

## Thread e chiusura

- Non aggiornare widget da `ScanWorker.run()`: usa `progress`, `scanCompleted` e `failed`.
- Conserva sempre un riferimento al worker per evitare distruzione durante l’esecuzione.
- La cancellazione usa `threading.Event`; `probe_video` termina il processo con `Popen.kill()`.
- In `closeEvent`, se il job è attivo, chiedi conferma e attendi la terminazione del worker prima di accettare la chiusura.
- `ConversionWorker` usa `-progress pipe:1` e `stderr=STDOUT` per evitare deadlock; chiudi sempre `process.stdout` dopo `wait()`.
- Non lanciare più conversioni parallele: la coda sequenziale riduce saturazione di disco e CPU. `fileStarted`, `progress`, `fileCompleted` e `conversionCompleted` aggiornano la UI solo via segnali.
- Non chiamare metodi Qt GUI da test worker eseguiti senza `QApplication` quando usi widget o `QPixmap`; i test GUI usano `QT_QPA_PLATFORM=offscreen`.

## Contratto del modello

`INFO_ROLE`, `SORT_ROLE` e `CATEGORY_ROLE` sono dipendenze interne del modello. Se li cambi, aggiorna proxy e test. `QSortFilterProxyModel.set_category` e `set_query` devono chiamare `invalidate()`; non riutilizzare il deprecato `invalidateFilter()`.

La tabella usa ordinamento nativo degli header, non paginazione HTML. Per nuove colonne aggiorna `HEADERS`, `data`, `_sort_value`, test e layout.

## Build e asset

- PyInstaller include `static/`, `ffmpeg` e `ffprobe` tramite `deployment/Fotogramma.spec`.
- Su macOS, `ffprobe_environment()` usa `lib` tramite `DYLD_LIBRARY_PATH`.
- `deployment/prepare_ffprobe.py` verifica con `DYLD_PRINT_LIBRARIES` che nessuna libreria Homebrew venga caricata da `ffmpeg` o `ffprobe`.
- Windows/Linux scaricano entrambi i binari dagli archivi statici senza estrarre percorsi arbitrari.
- Non committare `build/`, `dist/`, `deployment/vendor/`, `.venv/` o screenshot con dati reali.

## Checklist

1. Nessuna dipendenza browser/WebView introdotta.
2. Test unittest aggiornati e verdi.
3. GUI verificata con screenshot Qt offscreen, non soltanto con test logici.
4. Scansione di almeno cinque fixture verificata: due convert, due compatible, un error; conversione di un breve H.264 verificata come `hevc` senza modificare l’originale.
5. Build spostata e avviata con `PATH=/usr/bin:/bin`, scansione e conversione tramite l’`ffmpeg` incluso verificate.
6. `git diff --check` e `compileall` superati.
7. README, `docs/` e questa guida aggiornati se architettura o comandi cambiano.
