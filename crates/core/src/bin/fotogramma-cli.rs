//! Fotogramma on the command line: find the videos that are not H.265 yet, in
//! a library of any size, without opening the app.
//!
//!     fotogramma-cli scan /path/to/library
//!
//! It shares the scan cache with the app, so a scan from a NAS shell makes the
//! next scan in the app fast.

use fotogramma_core::cache::ScanCache;
use fotogramma_core::export::{self, Format};
use fotogramma_core::formatting::{format_bytes, format_int};
use fotogramma_core::i18n::{set_lang, tr_str, Lang};
use fotogramma_core::media_tools::ffprobe_path;
use fotogramma_core::scanner::{
    discover_videos, probe_videos_cached, reclassify, summarize, Category, VideoInfo,
};
use fotogramma_core::tr;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, PartialEq)]
struct ScanArgs {
    directory: PathBuf,
    format: Format,
    only: Option<Category>,
    codecs: Vec<String>,
    cache: CacheChoice,
    output: Option<PathBuf>,
    quiet: bool,
    fail_if_convert: bool,
}

#[derive(Debug, PartialEq)]
enum CacheChoice {
    /// The cache shared with the app.
    Shared,
    Off,
    File(PathBuf),
}

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Version,
    Scan(Box<ScanArgs>),
}

fn usage_error(message: String) -> String {
    tr!(
        "{message}. Prova «fotogramma-cli --help».",
        "{message}. Try “fotogramma-cli --help”."
    )
}

fn parse(arguments: &[String]) -> Result<Command, String> {
    let mut iterator = arguments.iter();
    match iterator.next().map(String::as_str) {
        None | Some("--help") | Some("-h") | Some("help") => return Ok(Command::Help),
        Some("--version") | Some("-V") => return Ok(Command::Version),
        Some("scan") => {}
        Some(other) => {
            return Err(usage_error(tr!(
                "comando sconosciuto «{other}»",
                "unknown command “{other}”"
            )))
        }
    }
    let mut args = ScanArgs {
        directory: PathBuf::new(),
        format: Format::Paths,
        only: Some(Category::Convert),
        codecs: vec!["hevc".to_string(), "av1".to_string()],
        cache: CacheChoice::Shared,
        output: None,
        quiet: false,
        fail_if_convert: false,
    };
    let mut directory = None;
    while let Some(argument) = iterator.next() {
        let mut value = |name: &str| {
            iterator.next().cloned().ok_or_else(|| {
                usage_error(tr!("{name} richiede un valore", "{name} needs a value"))
            })
        };
        match argument.as_str() {
            "--format" | "-f" => {
                args.format = match value("--format")?.as_str() {
                    "txt" | "paths" => Format::Paths,
                    "csv" => Format::Csv,
                    "json" => Format::Json,
                    other => {
                        return Err(usage_error(tr!(
                            "formato sconosciuto «{other}» (txt, csv o json)",
                            "unknown format “{other}” (txt, csv or json)"
                        )))
                    }
                }
            }
            "--only" => {
                args.only = match value("--only")?.as_str() {
                    "convert" => Some(Category::Convert),
                    "compatible" => Some(Category::Compatible),
                    "error" => Some(Category::Error),
                    "all" => None,
                    other => {
                        return Err(usage_error(tr!(
                            "--only «{other}» non valido (convert, compatible, error o all)",
                            "invalid --only “{other}” (convert, compatible, error or all)"
                        )))
                    }
                }
            }
            "--codecs" => {
                args.codecs = value("--codecs")?
                    .split(',')
                    .map(|codec| codec.trim().to_lowercase())
                    .filter(|codec| !codec.is_empty())
                    .collect();
            }
            "--cache" => args.cache = CacheChoice::File(PathBuf::from(value("--cache")?)),
            "--no-cache" => args.cache = CacheChoice::Off,
            "--output" | "-o" => args.output = Some(PathBuf::from(value("--output")?)),
            "--quiet" | "-q" => args.quiet = true,
            "--fail-if-convert" => args.fail_if_convert = true,
            flag if flag.starts_with('-') => {
                return Err(usage_error(tr!(
                    "opzione sconosciuta «{flag}»",
                    "unknown option “{flag}”"
                )))
            }
            path => {
                if directory.replace(PathBuf::from(path)).is_some() {
                    return Err(usage_error(
                        tr_str("indica una sola cartella", "give a single folder").to_string(),
                    ));
                }
            }
        }
    }
    args.directory = directory.ok_or_else(|| {
        usage_error(
            tr_str(
                "manca la cartella da analizzare",
                "missing the folder to scan",
            )
            .to_string(),
        )
    })?;
    Ok(Command::Scan(Box::new(args)))
}

fn help() -> String {
    tr_str(
        "fotogramma-cli · trova i video non ancora H.265 in una libreria

USO
  fotogramma-cli scan <cartella> [opzioni]

OPZIONI
  -f, --format <txt|csv|json>   formato dell'elenco (predefinito: txt, un percorso per riga)
      --only <convert|compatible|error|all>
                                cosa elencare (predefinito: convert)
      --codecs <elenco>         codec già accettati, separati da virgola (predefinito: hevc,av1)
      --cache <file>            usa questo file di cache
      --no-cache                rilegge tutto con ffprobe
  -o, --output <file>           scrive qui invece che su stdout
  -q, --quiet                   niente avanzamento né riepilogo su stderr
      --fail-if-convert         esce con codice 1 se resta qualcosa da convertire
  -V, --version                 mostra la versione
  -h, --help                    mostra questo aiuto

La cache è condivisa con l'app: una scansione da qui velocizza la prossima nell'app.
Codici di uscita: 0 ok, 1 ci sono video da convertire (con --fail-if-convert), 2 errore.
",
        "fotogramma-cli · find the videos that are not H.265 yet, in a library

USAGE
  fotogramma-cli scan <folder> [options]

OPTIONS
  -f, --format <txt|csv|json>   list format (default: txt, one path per line)
      --only <convert|compatible|error|all>
                                what to list (default: convert)
      --codecs <list>           codecs already accepted, comma separated (default: hevc,av1)
      --cache <file>            use this cache file
      --no-cache                read everything again with ffprobe
  -o, --output <file>           write here instead of stdout
  -q, --quiet                   no progress or summary on stderr
      --fail-if-convert         exit with code 1 if anything is left to convert
  -V, --version                 show the version
  -h, --help                    show this help

The cache is shared with the app: a scan from here makes the next one in the app fast.
Exit codes: 0 ok, 1 there are videos to convert (with --fail-if-convert), 2 error.
",
    )
    .to_string()
}

fn run_scan(args: &ScanArgs) -> Result<ExitCode, String> {
    if !args.directory.is_dir() {
        return Err(tr!(
            "«{}» non è una cartella",
            "“{}” is not a folder",
            args.directory.display()
        ));
    }
    let ffprobe = ffprobe_path()?;
    let show = !args.quiet && std::io::stderr().is_terminal();
    let candidates = discover_videos(
        &args.directory,
        None,
        |_, _| {},
        |path, message| {
            if !args.quiet {
                eprintln!("{}: {message}", path.display());
            }
        },
    )?;

    let cache_file = match &args.cache {
        CacheChoice::Off => None,
        CacheChoice::File(file) => Some(file.clone()),
        CacheChoice::Shared => fotogramma_core::paths::scan_cache_file(),
    };
    let cache = cache_file.as_deref().map(ScanCache::load);
    let total = candidates.len();
    let outcome = probe_videos_cached(
        &candidates,
        &args.directory,
        &ffprobe,
        None,
        cache.as_ref(),
        |processed, _, _, _| {
            if show && (processed % 25 == 0 || processed == total) {
                eprint!("\r{processed} / {total}");
            }
        },
    );
    if show {
        eprintln!();
    }
    if let (Some(cache), Some(file)) = (&cache, &cache_file) {
        cache.finish(&args.directory, !outcome.cancelled);
        // A cache that cannot be written only costs speed next time.
        let _ = cache.save(file);
    }

    let mut videos = outcome.videos;
    reclassify(&mut videos, &args.codecs);
    let summary = summarize(&videos, None);
    let listed: Vec<VideoInfo> = videos
        .iter()
        .filter(|video| args.only.is_none_or(|only| video.category == only))
        .cloned()
        .collect();
    let text = export::render(&listed, args.format);
    match &args.output {
        Some(file) => std::fs::write(file, text).map_err(|error| error.to_string())?,
        None => {
            // A closed pipe (`| head`) is not an error.
            let _ = std::io::stdout().write_all(text.as_bytes());
        }
    }
    if !args.quiet {
        eprintln!(
            "{}",
            tr!(
                "{} video · {} da convertire ({}) · {} compatibili · {} non leggibili · dalla cache: {}",
                "{} videos · {} to convert ({}) · {} compatible · {} unreadable · from the cache: {}",
                format_int(summary.total),
                format_int(summary.to_convert),
                format_bytes(Some(summary.to_convert_bytes)),
                format_int(summary.compatible),
                format_int(summary.errors),
                format_int(outcome.from_cache)
            )
        );
    }
    Ok(ExitCode::from(u8::from(
        args.fail_if_convert && summary.to_convert > 0,
    )))
}

fn main() -> ExitCode {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()));
    set_lang(Lang::resolve("auto", locale.as_deref()));
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let result = match parse(&arguments) {
        Ok(Command::Help) => {
            print!("{}", help());
            return ExitCode::SUCCESS;
        }
        Ok(Command::Version) => {
            println!("fotogramma-cli {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Ok(Command::Scan(args)) => run_scan(&args),
        Err(message) => Err(message),
    };
    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("fotogramma-cli: {message}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|value| value.to_string()).collect()
    }

    fn scan(list: &[&str]) -> ScanArgs {
        match parse(&args(list)).unwrap() {
            Command::Scan(args) => *args,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_defaults_list_what_is_left_to_convert() {
        let parsed = scan(&["scan", "/lib"]);
        assert_eq!(parsed.directory, PathBuf::from("/lib"));
        assert_eq!(parsed.format, Format::Paths);
        assert_eq!(parsed.only, Some(Category::Convert));
        assert_eq!(parsed.codecs, vec!["hevc", "av1"]);
        assert_eq!(parsed.cache, CacheChoice::Shared);
        assert!(!parsed.quiet && !parsed.fail_if_convert);
    }

    #[test]
    fn every_option_is_read() {
        let parsed = scan(&[
            "scan",
            "/lib",
            "-f",
            "csv",
            "--only",
            "all",
            "--codecs",
            "HEVC, h264",
            "--no-cache",
            "-o",
            "out.csv",
            "-q",
            "--fail-if-convert",
        ]);
        assert_eq!(parsed.format, Format::Csv);
        assert_eq!(parsed.only, None);
        assert_eq!(parsed.codecs, vec!["hevc", "h264"]);
        assert_eq!(parsed.cache, CacheChoice::Off);
        assert_eq!(parsed.output, Some(PathBuf::from("out.csv")));
        assert!(parsed.quiet && parsed.fail_if_convert);
        assert_eq!(
            scan(&["scan", "--cache", "c.json", "/lib"]).cache,
            CacheChoice::File(PathBuf::from("c.json")),
            "options may come before the folder"
        );
    }

    #[test]
    fn help_and_version_need_no_folder() {
        assert_eq!(parse(&args(&[])).unwrap(), Command::Help);
        assert_eq!(parse(&args(&["--help"])).unwrap(), Command::Help);
        assert_eq!(parse(&args(&["-V"])).unwrap(), Command::Version);
    }

    #[test]
    fn mistakes_are_reported_not_ignored() {
        for bad in [
            &["frobnicate"][..],
            &["scan"][..],
            &["scan", "/a", "/b"][..],
            &["scan", "/a", "--format"][..],
            &["scan", "/a", "--format", "xml"][..],
            &["scan", "/a", "--only", "some"][..],
            &["scan", "/a", "--wat"][..],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
    }
}
