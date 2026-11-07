# Architettura nativa

Fotogramma Desktop non ha alcun componente HTTP. L’eseguibile avvia direttamente `QApplication` e una `MainWindow` Qt.

```text
fotogramma.__main__
        |
        v
QApplication + MainWindow
        |
        +--> QFileDialog / QSettings
        |
        +--> ScanWorker (QThread)
                |
                +--> discover_videos
                +--> probe_videos (max 4 processi ffprobe)
                |
                v
        segnali Qt nel GUI thread
                |
                v
VideoTableModel --> VideoFilterProxy --> QTableView
        |
        +--> selezione video --> ricerca esterna oppure ConversionWorker
                                      |
                                      +--> ffmpeg (nuovi file .h265)
```

## Moduli

### `fotogramma/app.py`

- Crea `QApplication` e carica i font tramite `resource_path`.
- Applica il QSS Cinema Pop.
- Espone `--directory`, `--scan-on-start` e `--smoke-test` per verifiche automatizzate.
- Chiude la finestra dopo una scansione smoke test soltanto quando il worker è terminato.

### `core/scanner.py`

Contiene il motore condiviso concettualmente con la webapp ma incluso come sorgente nel progetto. Non dipende da Qt.

- `discover_videos`: ricerca deterministica e warning `os.walk`.
- `probe_video`: `Popen` con timeout 45 secondi, JSON e cancellazione.
- `probe_videos`: massimo quattro processi per batch, senza una seconda ondata in coda.
- `get_ffprobe_path`: in frozen preferisce il binario incluso.
- `ffprobe_environment`: in macOS frozen aggiunge `lib` a `DYLD_LIBRARY_PATH`.

### `ui/workers.py`

`ScanWorker` esegue la scansione fuori dal GUI thread. Segnali:

- `progress(processed, total, current_file)` con `total = -1` durante discovery;
- `scanCompleted(videos, warnings, cancelled)`;
- `failed(message)` per errori non recuperabili.

Il worker non tocca mai widget o modelli direttamente.

### `core/media_tools.py`

`get_ffmpeg_path()` risolve `ffmpeg` in sviluppo e frozen, preferendo il binario nel bundle. `ffprobe_environment()` viene condiviso anche dai processi di conversione per trovare le dylib incluse.

### `core/conversion.py`

`ConversionProfile` definisce CRF, preset, pixel format, tune, AQ mode e comportamento audio. I profili disponibili sono `balanced`, `quality`, `anime` e `light`, più `custom_profile` per la modalità manuale.

`build_ffmpeg_args()` genera:

- `-n` per non sovrascrivere mai;
- `-map 0` per tutti gli stream;
- `libx265` con CRF/preset/pixel format scelti;
- audio `copy` oppure AAC con bitrate;
- sottotitoli e dati `copy`;
- metadati e capitoli originali;
- `+faststart` solo per MP4;
- `-progress pipe:1` per il progresso nativo.

`allocate_output_path()` aggiunge `.h265` al nome e un contatore progressivo se il file esiste già. L’originale resta intatto nella modalità predefinita.

### `core/verification.py`

La sostituzione opzionale usa due fasi separate:

1. `validate_conversion_output()` verifica codec HEVC, durata entro max(1 secondo, 1%), stesso numero/tipo di stream e decodifica completa tramite `ffmpeg -v error -i output -f null -`.
2. `replace_verified_output()` rinomina l’originale in `.fotogramma-backup`, sposta il `.partial` sul percorso originale, ricontrolla il codec e solo dopo elimina il backup.

Se un test fallisce non avviene alcuna sostituzione. Se il controllo finale fallisce dopo la rinomina, il backup ripristina l’originale. `allocate_partial_path()` mantiene lo stesso contenitore di MKV/MP4 e rifiuta WebM.

### `core/search.py`

Ripulisce il titolo dai tag di release e costruisce un URL `https://1337x.to/search/<titolo+H.265>/1/`, includendo la prima pagina richiesta dal sito. `QDesktopServices.openUrl()` apre il browser esterno solo dopo un’azione esplicita; nessuna richiesta di rete passa dall’app.

### `ui/workers.py`

`ConversionWorker` elabora una coda sequenziale per evitare saturazione di CPU e disco. Unisce `stderr=STDOUT`, legge `out_time_ms` per calcolare la percentuale sulla durata del video e chiude sempre lo stream dopo `wait()`.

### `ui/models.py`

`VideoTableModel` espone sei colonne native: Video, Codec, Durata, Risoluzione, Dimensione, Stato.

Ruoli personalizzati:

- `INFO_ROLE`: oggetto `VideoInfo` completo;
- `SORT_ROLE`: valore tipizzato per l’ordinamento;
- `CATEGORY_ROLE`: categoria usata dal proxy.

`VideoFilterProxy` applica categoria e ricerca case-insensitive su nome, percorso e codec. L’ordinamento numerico usa il ruolo custom, quindi dimensione e durata non vengono ordinate come stringhe.

### `ui/main_window.py`

Gestisce:

- selezione directory e validazione;
- statistiche cliccabili e filtri;
- avanzamento e cancellazione;
- warning per risultati parziali;
- dettagli con doppio clic;
- export tramite `QFileDialog`;
- chiusura con conferma se un job è attivo.

`QSettings` salva solo percorso e geometria. I video restano in memoria finché l’app è aperta.

## Stati dell’interfaccia

La `QStackedWidget` contiene:

1. pagina iniziale con matrice codec;
2. pagina risultati;
3. pagina errore con Riprova.

Durante la scansione i controlli vengono disabilitati e il progresso appare nella command bar. L’interruzione non classifica i file bloccati come errori.

## Build frozen

`runtime.py` usa `sys._MEIPASS` per trovare asset e binari. La build onedir contiene:

- interprete Python;
- PySide6 Essentials e plugin Qt richiesti;
- `static/` con font e licenze;
- `ffmpeg`, `ffprobe` e dipendenze native;
- codice `fotogramma` e `runtime.py`.

Il pacchetto può essere spostato. La verifica esegue la build estratta con `PATH=/usr/bin:/bin`, evitando Python e Homebrew di sistema.

## Testing

- Test motore con mock di `Popen`, `os.walk` e concorrenza.
- Test modello/proxy con `QApplication` offscreen.
- Test `MainWindow` per statistiche, filtri e warning.
- Test worker su cinque video reali opzionali tramite `FOTOGRAMMA_DEMO_DIR`.
- Test di conversione reale di un breve H.264 con verifica `hevc`, originale invariato e nessuna sovrascrittura.
- Test profili, argomenti AAC/faststart, selezione azioni, anteprima comando e URL di ricerca.
- Test del client di release Forgejo con server HTTP locale per creazione, recupero e sostituzione asset, senza token nei log.
- Screenshot nativi ottenuti con `QWidget.grab()`, senza browser o screenshot HTML.
