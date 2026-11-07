# Fotogramma Desktop

Fotogramma Desktop è la versione **nativa** di Fotogramma: una vera applicazione desktop realizzata con **PySide6/Qt Widgets**. Non usa browser, WebView, server HTTP o launcher esterni.

![Fotogramma Desktop nativo](docs/assets/fotogramma-native-results.png)

## Funzionalità

- Finestra nativa per macOS, Windows e Linux.
- Ricerca ricorsiva di `.mp4`, `.mkv` e `.webm`.
- Classificazione chiara in **da convertire**, **già compatibili** (HEVC/AV1) e **non leggibili**.
- Statistiche con valori in evidenza, filtri, ricerca e ordinamento tramite header.
- Azioni sulla selezione: **Cerca H.265 online** e **Converti selezionati**.
- Scansione in background con avanzamento e interruzione.
- Conversione diretta tramite `ffmpeg`, con profili leggibili e modalità personalizzata.
- Icona originale con lettera F e numero 265, inclusa nella finestra e nel bundle.
- Avvisi per cartelle non accessibili e risultati parziali.
- Esportazione dell’elenco tramite `QFileDialog`.
- Tema Cinema Pop con font Sora e IBM Plex Mono inclusi.

**La modalità predefinita non sovrascrive gli originali.** Le conversioni creano nuovi file `.h265.mkv` o `.h265.mp4`; se il nome esiste già, viene aggiunto un numero progressivo. La sostituzione dell’originale è opzionale e avviene solo dopo una verifica completa del risultato.

## Conversione diretta

Seleziona uno o più video da convertire e scegli **Converti selezionati**. La finestra mostra il profilo, la destinazione e l’anteprima del comando FFmpeg prima di iniziare.

![Profili di conversione](docs/assets/fotogramma-conversion-dialog.png)

### Profili disponibili

| Profilo | Uso consigliato | Video | Audio predefinito |
| --- | --- | --- | --- |
| Massima qualità e compressione | Miglior rapporto qualità/dimensione, anche se lento | CRF 20, preset veryslow, AQ mode 3, 10-bit | Originale |
| Intermedio · buona qualità | Equilibrio tra qualità, spazio e tempo | CRF 22, preset medium, 10-bit | Originale |
| Veloce · transcode rapido | Massima velocità disponibile | CRF 26, preset ultrafast, 8-bit | Originale |
| Personalizzato | Controllo manuale | CRF, preset, pixel format, tune e AQ mode | Originale o AAC |

L’audio può essere mantenuto originale oppure convertito in **AAC 128/160/256 kbps**. Tutti gli stream vengono mappati con `-map 0`, mantenendo tracce audio, sottotitoli, capitoli e metadati compatibili con il contenitore scelto.

Il contenitore **MKV** è consigliato per la massima compatibilità degli stream; **MP4** aggiunge `+faststart` per la riproduzione progressiva.

## Sostituzione dell’originale dopo verifica

L’opzione **Sostituisci gli originali solo dopo verifica completa** è disattivata di default. Se viene attivata, Fotogramma lavora su file `.partial` nella stessa directory degli originali e sostituisce ciascun file soltanto dopo questi controlli:

1. Codec di output realmente HEVC.
2. Durata coerente con l’originale entro l’1% o 1 secondo.
3. Stesso numero e tipo di stream audio, video, sottotitoli e dati.
4. Decodifica completa del risultato tramite `ffmpeg -v error -i output -f null -` senza errori.
5. Controllo finale del file sostituito prima di eliminare il backup temporaneo.

Se un controllo fallisce, l’originale non viene modificato. La sostituzione usa rinomina atomica sulla stessa filesystem e un backup `.fotogramma-backup` che viene eliminato solo dopo esito positivo. Per MKV e MP4 il contenitore resta coerente con l’estensione originale; la sostituzione non è disponibile per WebM.

## Ricerca online

Seleziona un singolo video da convertire e scegli **Cerca H.265 online**. Fotogramma apre nel browser predefinito una ricerca su **1337x.to** con titolo e `H.265`.

Non viene scaricato alcun contenuto e non esiste integrazione torrent o WebView: viene soltanto aperto un URL esterno. Il nome del file viene ripulito dai tag di release più comuni per migliorare la ricerca. La query lascia il PC soltanto quando scegli esplicitamente questa azione.

## Avvio dalla build portabile

La build contiene **Python, Qt, font, ffmpeg e ffprobe**, quindi sul computer di destinazione non serve installare nulla.

Per macOS Apple Silicon:

```text
dist/Fotogramma-Darwin-arm64.zip
```

Estrai lo ZIP, fai clic destro su `Fotogramma.app` e scegli **Apri**. La build è firmata ad hoc ma non notarizzata; Gatekeeper potrebbe chiedere conferma.

Su Windows esegui `Fotogramma.exe` dentro la cartella estratta. Su Linux esegui il binario `Fotogramma`. Ogni build va compilata sul rispettivo sistema operativo; PyInstaller non è un cross-compiler.

## Avvio dal codice sorgente

Requisiti: Python 3.10+, `ffmpeg` e `ffprobe` su `PATH` o nella cartella del progetto, PySide6 Essentials.

Con `uv`, senza creare un ambiente nel progetto:

```bash
uv run --no-project --with pyside6-essentials python -m fotogramma
```

Opzioni:

```bash
# Precompila la cartella
uv run --no-project --with pyside6-essentials python -m fotogramma --directory /percorso/video

# Avvia subito la scansione
uv run --no-project --with pyside6-essentials python -m fotogramma \
  --directory /percorso/video --scan-on-start
```

## Utilizzo

1. Scegli una cartella con **Sfoglia** o inserisci il percorso.
2. Seleziona **Analizza libreria**.
3. Usa statistiche e filtri per isolare i candidati.
4. Seleziona uno o più video per vedere le azioni disponibili.
5. Ordina cliccando gli header e fai doppio clic su un video per i dettagli.
6. Converti direttamente oppure esporta l’elenco da usare altrove.

Cartella e geometria della finestra vengono ricordate tramite `QSettings`; i risultati della scansione non vengono salvati su disco.

## Test

```bash
QT_QPA_PLATFORM=offscreen \
  uv run --no-project --with pyside6-essentials \
  python -m unittest discover -s tests -v
```

Per worker e conversione reale con i video dimostrativi:

```bash
FOTOGRAMMA_DEMO_DIR=/percorso/fixture QT_QPA_PLATFORM=offscreen \
  uv run --no-project --with pyside6-essentials \
  python -m unittest discover -s tests -v
```

La suite attuale contiene **35 test** per motore, modelli Qt, finestra, worker, profili, ricerca online, conversione HEVC reale, validazione completa e ripristino del backup.

## CI/CD e release

![Build & Release](https://git.local.tassoni.net/JohnnyTax/fotogramma-desktop/actions/workflows/build-release.yml/badge.svg)

La pipeline Forgejo esegue test e build su **Linux, Windows e macOS ARM**. macOS Intel è escluso. Su ogni tag `v*` pubblica o aggiorna automaticamente una release con tutti gli ZIP portabili.

Dettagli e requisiti dei runner: [CI/CD su Forgejo](docs/CICD.md).

## Build

```bash
uv run --no-project --with pyside6-essentials --with pyinstaller==6.22.3 \
  python deployment/build.py
```

La guida completa è in [`deployment/README.md`](deployment/README.md).

## Struttura

```text
fotogramma/
  app.py                 Entry point, icona, font e opzioni CLI
  core/scanner.py        Discovery, ffprobe, classificazione e cancellazione
  core/media_tools.py    Percorso di ffmpeg in sviluppo e frozen
  core/conversion.py     Profili, argomenti FFmpeg e output senza sovrascrittura
  core/verification.py   Validazione completa e sostituzione atomica con backup
  core/search.py         Pulizia titolo e URL esterno 1337x.to
  ui/main_window.py      Finestra e azioni di selezione
  ui/conversion_dialog.py Profili e avanzamento conversione
  ui/models.py           QAbstractTableModel e QSortFilterProxyModel
  ui/workers.py          ScanWorker e ConversionWorker
  ui/theme.py            QSS Cinema Pop
  ui/widgets.py          CompositionBar nativa
runtime.py               Percorsi risorse in sviluppo e frozen
static/                  Font, licenze e icona originale
deployment/              Build PyInstaller e vendor FFmpeg
tests/                   Suite unittest, anche Qt offscreen
docs/                    Architettura e asset dimostrativi
```

## Documentazione

- [Architettura](docs/ARCHITECTURE.md)
- [Build e distribuzione](deployment/README.md)
- [CI/CD su Forgejo](docs/CICD.md)
- [Guida per agenti](AGENTS.md)

## Privacy e licenze

L’app non apre alcun server e non invia dati online durante analisi o conversione. Solo la ricerca esplicita su 1337x.to apre un URL esterno contenente il titolo del video.

Sora e IBM Plex Mono sono distribuiti con le rispettive licenze SIL Open Font License incluse in `static/fonts/`. L’icona originale è inclusa in `static/icons/`.
