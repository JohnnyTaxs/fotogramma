# Build nativa e distribuzione

La directory si chiama `deployment/` e non `packaging/` per evitare il conflitto con il pacchetto Python `packaging` richiesto da PyInstaller.

## Requisiti

Per compilare:

- Python 3.13 consigliato;
- `uv`;
- PySide6 Essentials e PyInstaller scaricati dall’ambiente temporaneo;
- su macOS, `ffprobe` installato con Homebrew e gli strumenti Xcode per `otool`;
- su Windows/Linux, accesso di rete per scaricare la build statica di `ffprobe`.

Sul computer di destinazione non serve nulla di tutto questo.

## Compilare

Dalla radice del progetto, sul sistema di destinazione:

```bash
uv run --no-project --with pyside6-essentials --with pyinstaller==6.22.3 \
  python deployment/build.py
```

Il comando prepara `ffprobe`, esegue PyInstaller e genera:

- `Fotogramma.app` su macOS;
- `dist/Fotogramma/` su Windows e Linux;
- `dist/Fotogramma-<OS>-<arch>.zip` per il trasporto.

`build/`, `dist/` e `deployment/vendor/` sono ignorati da Git.

## Contenuto

La build **onedir** include Python, PySide6 Essentials, plugin Qt, font, licenze, `ffmpeg` e `ffprobe`. Non include browser o Qt WebEngine.

`runtime.py` usa `sys._MEIPASS` per risolvere gli asset nel pacchetto spostabile. In modalità frozen, la ricerca di `ffprobe` preferisce sempre il binario incluso.

## ffmpeg e ffprobe per piattaforma

### macOS

`deployment/prepare_ffprobe.py`:

1. copia `ffmpeg` e `ffprobe` Homebrew usati per la build;
2. legge le dipendenze con `otool -L`;
3. copia ricorsivamente le dylib non di sistema in `lib/`;
4. esegue entrambi i binari con `DYLD_LIBRARY_PATH` locale;
5. verifica con `DYLD_PRINT_LIBRARIES` che nessuna libreria venga caricata da Homebrew.

Il file `deployment/vendor/<OS>-<arch>/manifest.json` include SHA-256 e dimensione dei file raccolti.

### Windows e Linux

Il downloader estrae `ffmpeg` e `ffprobe` (o i rispettivi `.exe`) dagli archivi statici gyan.dev e johnvansickle.com. Non estrae directory complete e non usa percorsi dell’archivio per scrivere fuori dalla cartella vendor.

## Verifica della build

```bash
# Avvio della GUI
Fotogramma --directory /percorso/fixture --scan-on-start --smoke-test

# macOS, percorso esplicito
Fotogramma.app/Contents/MacOS/Fotogramma \
  --directory /percorso/fixture --scan-on-start --smoke-test
```

La modalità smoke test si chiude da sola dopo l’avvio o dopo il completamento della scansione.

Prima del rilascio:

1. estrarre lo ZIP in una nuova cartella;
2. avviare con `PATH=/usr/bin:/bin`;
3. verificare apertura, font e scansione completa;
4. estrarre nuovamente lo ZIP altrove e ripetere la prova;
5. verificare che nessuna libreria nativa esterna venga caricata.

La build `Fotogramma-Darwin-arm64.zip` è stata verificata con questo flusso. Windows e Linux richiedono prove sui rispettivi sistemi.

## Verifica della conversione frozen

Oltre all’avvio della GUI, verificare i binari inclusi:

```bash
# macOS
Fotogramma.app/Contents/Frameworks/ffmpeg -version
Fotogramma.app/Contents/Frameworks/ffprobe -version
```

Eseguire una breve conversione con l’`ffmpeg` incluso e controllare con il `ffprobe` incluso che il codec risultante sia `hevc`. Il file originale deve restare invariato e l’output deve usare il suffisso `.h265`.

La build `Fotogramma-Darwin-arm64.zip` è stata verificata con questo flusso. Windows e Linux richiedono prove sui rispettivi sistemi.

## Firma

La build macOS è firmata ad hoc ma non notarizzata. Al primo avvio usare tasto destro → Apri. Per una distribuzione pubblica senza avvisi servono certificato Developer ID e notarizzazione. Anche Windows SmartScreen può richiedere una strategia di code signing.
