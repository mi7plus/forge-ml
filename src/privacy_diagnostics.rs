use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, Once, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

static ENABLED: AtomicBool = AtomicBool::new(false);
static ROOT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static HOOK: Once = Once::new();
const MAX_LOG_BYTES: u64 = 1_000_000;

#[derive(Serialize)]
struct Event<'a> {
    timestamp_unix: u64,
    kind: &'a str,
    app_version: &'a str,
    os: &'a str,
    architecture: &'a str,
}
#[derive(Serialize)]
struct Crash {
    timestamp_unix: u64,
    app_version: &'static str,
    message: String,
    location: String,
    thread: String,
}

pub fn configure(enabled: bool, root: Option<&Path>) {
    ENABLED.store(enabled && root.is_some(), Ordering::Relaxed);
    *ROOT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = root.map(Path::to_owned);
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if ENABLED.load(Ordering::Relaxed) {
                let message = info
                    .payload()
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("non-string panic");
                let location = info
                    .location()
                    .map(|v| format!("{}:{}:{}", v.file(), v.line(), v.column()))
                    .unwrap_or_default();
                let crash = Crash {
                    timestamp_unix: now(),
                    app_version: env!("CARGO_PKG_VERSION"),
                    message: sanitize(message),
                    location: sanitize(&location),
                    thread: std::thread::current()
                        .name()
                        .unwrap_or("unnamed")
                        .to_owned(),
                };
                let _ = write_crash(&crash);
            }
            previous(info);
        }));
    });
}

pub fn record(kind: &'static str) -> Result<(), String> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Ok(());
    }
    let root = current_root().ok_or("Diagnostics have no project root")?;
    let directory = root.join(".forge/diagnostics");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let path = directory.join("events.jsonl");
    if path.metadata().map(|m| m.len()).unwrap_or(0) >= MAX_LOG_BYTES {
        let rotated = directory.join("events.previous.jsonl");
        if rotated.exists() {
            fs::remove_file(&rotated).map_err(|e| e.to_string())?;
        }
        fs::rename(&path, rotated).map_err(|e| e.to_string())?;
    }
    let event = Event {
        timestamp_unix: now(),
        kind,
        app_version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
    };
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    writeln!(
        file,
        "{}",
        serde_json::to_string(&event).map_err(|e| e.to_string())?
    )
    .map_err(|e| e.to_string())
}

pub fn export_bundle(root: &Path, destination: &Path) -> Result<(), String> {
    let file = fs::File::create(destination).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let manifest = serde_json::json!({"schema":1,"app_version":env!("CARGO_PKG_VERSION"),"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"included":"bounded diagnostic events and crash summaries","excluded":["source files","datasets","environment variables","credentials","tokens"]});
    zip.start_file("diagnostics-manifest.json", options)
        .map_err(|e| e.to_string())?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let directory = root.join(".forge/diagnostics");
    for name in ["events.jsonl", "events.previous.jsonl"] {
        let path = directory.join(name);
        if path.is_file() {
            add_file(&mut zip, &path, name, options)?;
        }
    }
    let crashes = directory.join("crashes");
    if let Ok(entries) = fs::read_dir(crashes) {
        for entry in entries.flatten().take(20) {
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) == Some("json") {
                let name = format!("crashes/{}", entry.file_name().to_string_lossy());
                add_file(&mut zip, &path, &name, options)?;
            }
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn add_file(
    zip: &mut zip::ZipWriter<fs::File>,
    path: &Path,
    name: &str,
    options: zip::write::SimpleFileOptions,
) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    zip.start_file(name, options).map_err(|e| e.to_string())?;
    zip.write_all(&bytes).map_err(|e| e.to_string())
}
fn write_crash(crash: &Crash) -> Result<(), String> {
    let root = current_root().ok_or("No diagnostics root")?;
    let directory = root.join(".forge/diagnostics/crashes");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let path = directory.join(format!(
        "crash-{}-{}.json",
        crash.timestamp_unix,
        std::process::id()
    ));
    fs::write(
        path,
        serde_json::to_vec_pretty(crash).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn current_root() -> Option<PathBuf> {
    ROOT.get()?
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn sanitize(value: &str) -> String {
    let mut text = value.chars().take(4096).collect::<String>();
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        text = text.replace(&home.to_string_lossy().to_string(), "[HOME]");
    }
    text.split_whitespace()
        .map(|word| {
            let lower = word.to_ascii_lowercase();
            if ["token=", "password=", "secret=", "key="]
                .iter()
                .any(|marker| lower.contains(marker))
            {
                "[REDACTED]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sanitizes_secrets_and_limits_payloads() {
        let output = sanitize(&format!("token=abc {}", "x".repeat(5000)));
        assert!(!output.contains("abc"));
        assert!(output.len() < 4200);
    }
    #[test]
    fn bundle_excludes_project_files() {
        let root = std::env::temp_dir().join(format!("forge-diag-{}", std::process::id()));
        fs::create_dir_all(root.join(".forge/diagnostics")).unwrap();
        fs::write(root.join("secret.rs"), "private").unwrap();
        fs::write(root.join(".forge/diagnostics/events.jsonl"), "{}\n").unwrap();
        let bundle = root.join("bundle.zip");
        export_bundle(&root, &bundle).unwrap();
        let mut archive = zip::ZipArchive::new(fs::File::open(&bundle).unwrap()).unwrap();
        assert!(archive.by_name("secret.rs").is_err());
        assert!(archive.by_name("diagnostics-manifest.json").is_ok());
        let _ = fs::remove_dir_all(root);
    }
}
