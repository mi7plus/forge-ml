//! The native-artifact provisioning engine (Forge Distribution, Phase 5).
//!
//! When a project's `[native]` prerequisite is missing from the system and Forge
//! curates a prebuilt for it, Forge *provides* it: download the pinned artifact,
//! **verify its SHA-256**, extract it into a per-user cache, and expose it to the
//! build — a tool on `PATH`, or a library via env vars like `OPENSSL_DIR` /
//! `PKG_CONFIG_PATH`. Downloads happen only at `forge native provide` /
//! `forge env sync`, never at app startup; the network path is isolated behind a
//! [`Fetcher`] so the engine is unit-tested offline.
//!
//! This is a *curated, hash-pinned* prebuilt-binary channel, not a general
//! resolver: every artifact is an entry in the catalog verified by hash before it
//! is used. Nothing downloaded is ever executed by Forge.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Archive format of a downloaded artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Archive {
    Zip,
    TarGz,
}

/// What an artifact is, for reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Tool,
    Library,
}

/// One artifact for one host target (`<arch>-<os>`, e.g. `x86_64-windows`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct TargetArtifact {
    pub url: String,
    pub sha256: String,
    pub archive: Archive,
    /// Directory within the extracted tree to add to `PATH` (tools).
    #[serde(default)]
    pub bin_dir: Option<String>,
    /// Env vars to set to paths within the extracted tree (libraries), e.g.
    /// `OPENSSL_DIR = "."`. Each value is joined onto the extracted root.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// A named artifact with per-target prebuilts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Artifact {
    pub name: String,
    pub kind: Kind,
    #[serde(default)]
    pub version: String,
    /// Keyed by `<arch>-<os>` (from `std::env::consts`).
    pub targets: BTreeMap<String, TargetArtifact>,
}

impl Artifact {
    /// The entry for the current platform, if this artifact ships one.
    pub fn for_host(&self) -> Option<&TargetArtifact> {
        self.targets.get(&host_target())
    }
}

/// The curated catalog of providable native artifacts.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct Catalog {
    #[serde(default, rename = "artifact")]
    pub artifacts: Vec<Artifact>,
}

impl Catalog {
    pub fn parse(text: &str) -> Result<Catalog, String> {
        toml::from_str(text).map_err(|error| error.to_string())
    }

    pub fn find(&self, name: &str) -> Option<&Artifact> {
        self.artifacts.iter().find(|artifact| artifact.name == name)
    }
}

/// The current host target key, `<arch>-<os>`.
pub fn host_target() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

/// Fetches the bytes at a URL. Isolated so the engine is testable without a
/// network (tests supply a fixture fetcher).
pub trait Fetcher {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// A real HTTPS fetcher (ureq + rustls). Only `https://` URLs are accepted.
pub struct HttpFetcher;

impl Fetcher for HttpFetcher {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        if !url.starts_with("https://") {
            return Err(format!("refusing non-https artifact URL: {url}"));
        }
        let response = ureq::get(url).call().map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(1024 * 1024 * 1024) // 1 GiB ceiling
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        Ok(bytes)
    }
}

/// What a provisioned artifact contributes to the environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exposure {
    pub path_dirs: Vec<PathBuf>,
    pub env: Vec<(String, String)>,
}

/// Ensure `entry` is present under `cache_root` and return how to expose it.
/// Reuses an already-extracted copy (keyed by hash); otherwise fetches, verifies
/// the SHA-256, and extracts atomically. It never runs anything it downloads.
pub fn provision(
    name: &str,
    entry: &TargetArtifact,
    cache_root: &Path,
    fetcher: &dyn Fetcher,
) -> Result<Exposure, String> {
    let stamp = &entry.sha256[..entry.sha256.len().min(16)];
    let dir = cache_root.join(format!("{name}-{stamp}"));
    if !dir.join(".forge-provisioned").is_file() {
        let bytes = fetcher.fetch(&entry.url)?;
        let digest = crate::experiment::stable_digest(&bytes);
        if !digest.eq_ignore_ascii_case(&entry.sha256) {
            return Err(format!(
                "{name}: SHA-256 mismatch — expected {}, got {digest}",
                entry.sha256
            ));
        }
        extract(&bytes, entry.archive, &dir)?;
        std::fs::write(dir.join(".forge-provisioned"), &entry.sha256)
            .map_err(|error| error.to_string())?;
    }
    Ok(expose(entry, &dir))
}

/// The `PATH` dirs and env vars an already-extracted artifact contributes.
fn expose(entry: &TargetArtifact, dir: &Path) -> Exposure {
    let mut exposure = Exposure::default();
    if let Some(bin) = &entry.bin_dir {
        exposure.path_dirs.push(dir.join(bin));
    }
    for (key, relative) in &entry.env {
        exposure
            .env
            .push((key.clone(), dir.join(relative).to_string_lossy().into_owned()));
    }
    exposure
}

/// Extract archive bytes into `dir` atomically (temp dir + rename).
fn extract(bytes: &[u8], archive: Archive, dir: &Path) -> Result<(), String> {
    let parent = dir.parent().ok_or("invalid cache directory")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temp = parent.join(format!(".tmp-{}-{}", std::process::id(), rand_suffix()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).map_err(|error| error.to_string())?;
    let result = match archive {
        Archive::Zip => extract_zip(bytes, &temp),
        Archive::TarGz => extract_tar_gz(bytes, &temp),
    };
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(error);
    }
    let _ = std::fs::remove_dir_all(dir);
    std::fs::rename(&temp, dir).map_err(|error| format!("finalizing {}: {error}", dir.display()))
}

fn extract_zip(bytes: &[u8], dir: &Path) -> Result<(), String> {
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|error| error.to_string())?;
    zip.extract(dir).map_err(|error| error.to_string())
}

fn extract_tar_gz(bytes: &[u8], dir: &Path) -> Result<(), String> {
    let decoder = flate2::read::GzDecoder::new(bytes);
    tar::Archive::new(decoder)
        .unpack(dir)
        .map_err(|error| error.to_string())
}

/// A short non-cryptographic suffix to keep concurrent temp dirs distinct.
fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;

    struct MapFetcher(HashMap<String, Vec<u8>>);
    impl Fetcher for MapFetcher {
        fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
            self.0.get(url).cloned().ok_or_else(|| "no such url".to_owned())
        }
    }

    fn make_zip() -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            writer
                .start_file("bin/tool.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"provided").unwrap();
            writer.finish().unwrap();
        }
        buf
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("forge-prov-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn provisions_and_exposes_a_tool_on_path() {
        let bytes = make_zip();
        let sha = crate::experiment::stable_digest(&bytes);
        let url = "https://example.test/tool.zip".to_owned();
        let entry = TargetArtifact {
            url: url.clone(),
            sha256: sha,
            archive: Archive::Zip,
            bin_dir: Some("bin".to_owned()),
            env: BTreeMap::new(),
        };
        let fetcher = MapFetcher(HashMap::from([(url, bytes)]));
        let cache = temp_dir("ok");

        let exposure = provision("tool", &entry, &cache, &fetcher).unwrap();
        assert_eq!(exposure.path_dirs.len(), 1);
        assert!(exposure.path_dirs[0].join("tool.txt").is_file());

        // A second call reuses the cache (no fetch needed): an empty fetcher works.
        let empty = MapFetcher(HashMap::new());
        assert!(provision("tool", &entry, &cache, &empty).is_ok());

        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn rejects_a_sha256_mismatch() {
        let bytes = make_zip();
        let url = "https://example.test/tool.zip".to_owned();
        let entry = TargetArtifact {
            url: url.clone(),
            sha256: "0".repeat(64),
            archive: Archive::Zip,
            bin_dir: Some("bin".to_owned()),
            env: BTreeMap::new(),
        };
        let fetcher = MapFetcher(HashMap::from([(url, bytes)]));
        let cache = temp_dir("bad");
        let result = provision("tool", &entry, &cache, &fetcher);
        assert!(result.is_err_and(|error| error.contains("SHA-256 mismatch")));
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn http_fetcher_refuses_non_https() {
        assert!(HttpFetcher.fetch("http://example.test/x").is_err());
    }

    #[test]
    fn library_exposes_env_pointing_at_extracted_root() {
        let bytes = make_zip();
        let sha = crate::experiment::stable_digest(&bytes);
        let url = "https://example.test/lib.zip".to_owned();
        let entry = TargetArtifact {
            url: url.clone(),
            sha256: sha,
            archive: Archive::Zip,
            bin_dir: None,
            env: BTreeMap::from([("OPENSSL_DIR".to_owned(), ".".to_owned())]),
        };
        let fetcher = MapFetcher(HashMap::from([(url, bytes)]));
        let cache = temp_dir("lib");
        let exposure = provision("openssl", &entry, &cache, &fetcher).unwrap();
        assert_eq!(exposure.env.len(), 1);
        assert_eq!(exposure.env[0].0, "OPENSSL_DIR");
        assert!(Path::new(&exposure.env[0].1).is_dir());
        let _ = std::fs::remove_dir_all(&cache);
    }

    #[test]
    fn catalog_parses_targets() {
        let catalog = Catalog::parse(
            r#"
[[artifact]]
name = "protoc"
kind = "tool"
version = "28.3"
[artifact.targets.x86_64-windows]
url = "https://example.test/protoc-win.zip"
sha256 = "abc"
archive = "zip"
bin_dir = "bin"
"#,
        )
        .unwrap();
        let protoc = catalog
            .artifacts
            .iter()
            .find(|artifact| artifact.name == "protoc")
            .unwrap();
        assert_eq!(protoc.kind, Kind::Tool);
        assert!(protoc.targets.contains_key("x86_64-windows"));
    }
}
