use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_LIST_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_ERROR_BYTES: usize = 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Provider {
    S3,
    Rclone,
}
impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Self::S3 => "S3 / compatible",
            Self::Rclone => "rclone remote",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectProfile {
    pub name: String,
    pub provider: Provider,
    pub bucket: String,
    pub prefix: String,
    pub endpoint: String,
    pub credential_hint: String,
}

impl ObjectProfile {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || !self
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(
                "Profile names may contain letters, numbers, dots, dashes, and underscores".into(),
            );
        }
        if self.bucket.trim().is_empty() || self.bucket.contains(char::is_whitespace) {
            return Err("Enter a bucket or rclone remote without whitespace".into());
        }
        safe_key(&self.prefix)?;
        if !self.endpoint.is_empty() {
            let url =
                url::Url::parse(&self.endpoint).map_err(|e| format!("Invalid endpoint: {e}"))?;
            if url.scheme() != "https" && !is_local(&url) {
                return Err(
                    "Object-storage endpoints must use HTTPS unless they target localhost".into(),
                );
            }
        }
        Ok(())
    }
    pub fn list(&self, limit: usize) -> Result<String, String> {
        self.validate()?;
        let limit = limit.clamp(1, 1000);
        match self.provider {
            Provider::S3 => {
                let uri = format!("s3://{}/{}", self.bucket, self.prefix);
                let mut args = vec!["s3", "ls", &uri];
                if !self.endpoint.is_empty() {
                    args.extend(["--endpoint-url", &self.endpoint]);
                }
                run_bounded("aws", &args, Duration::from_secs(30), &self.endpoint)
                    .map(|text| text.lines().take(limit).collect::<Vec<_>>().join("\n"))
            }
            Provider::Rclone => run_bounded(
                "rclone",
                &[
                    "lsf",
                    &format!("{}:{}", self.bucket, self.prefix),
                    "--max-depth",
                    "1",
                ],
                Duration::from_secs(30),
                &self.endpoint,
            )
            .map(|text| text.lines().take(limit).collect::<Vec<_>>().join("\n")),
        }
    }
    pub fn download(&self, key: &str, project: &Path) -> Result<PathBuf, String> {
        self.validate()?;
        safe_key(key)?;
        if key.trim_matches('/').is_empty() {
            return Err("Enter an object key relative to the configured prefix".into());
        }
        let remote_key = join_key(&self.prefix, key);
        safe_key(&remote_key)?;
        let destination = project
            .join(".forge/object-cache")
            .join(&self.name)
            .join(Path::new(&remote_key));
        let parent = destination
            .parent()
            .ok_or("Object key has no destination directory")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = destination.with_extension(format!(
            "{}.forge-download",
            destination
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("part")
        ));
        if temporary.exists() {
            fs::remove_file(&temporary).map_err(|e| e.to_string())?;
        }
        let transfer = match self.provider {
            Provider::S3 => {
                let uri = format!("s3://{}/{remote_key}", self.bucket);
                let destination_text = temporary.to_string_lossy().into_owned();
                let mut args = vec!["s3", "cp", &uri, &destination_text];
                if !self.endpoint.is_empty() {
                    args.extend(["--endpoint-url", &self.endpoint]);
                }
                run_bounded("aws", &args, Duration::from_secs(120), &self.endpoint)
            }
            Provider::Rclone => {
                let source = format!("{}:{remote_key}", self.bucket);
                let destination_text = temporary.to_string_lossy().into_owned();
                run_bounded(
                    "rclone",
                    &["copyto", &source, &destination_text],
                    Duration::from_secs(120),
                    &self.endpoint,
                )
            }
        };
        if let Err(error) = transfer {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        let size = fs::metadata(&temporary).map_err(|e| e.to_string())?.len();
        if size > MAX_DOWNLOAD_BYTES {
            let _ = fs::remove_file(&temporary);
            return Err(format!(
                "Object is larger than the {} GiB project-cache limit",
                MAX_DOWNLOAD_BYTES / (1024 * 1024 * 1024)
            ));
        }
        publish_download(&temporary, &destination)?;
        Ok(destination)
    }

    pub fn test(&self) -> Result<String, String> {
        self.list(1).map(|_| {
            format!(
                "{} profile `{}` is reachable",
                self.provider.label(),
                self.name
            )
        })
    }
}

fn safe_key(key: &str) -> Result<(), String> {
    if key.contains('\0')
        || Path::new(key).components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        Err("Object keys must be relative and may not traverse directories".into())
    } else {
        Ok(())
    }
}

fn join_key(prefix: &str, key: &str) -> String {
    match (prefix.trim_matches('/'), key.trim_matches('/')) {
        ("", key) => key.to_owned(),
        (prefix, "") => prefix.to_owned(),
        (prefix, key) => format!("{prefix}/{key}"),
    }
}
fn is_local(url: &url::Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

fn run_bounded(
    program: &str,
    args: &[&str],
    timeout: Duration,
    sensitive_endpoint: &str,
) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program} is not available: {e}"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or("Could not capture object listing")?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or("Could not capture object-storage errors")?;
    let stdout_reader = thread::spawn(move || read_bounded(&mut stdout, MAX_LIST_OUTPUT_BYTES + 1));
    let stderr_reader = thread::spawn(move || read_bounded(&mut stderr, MAX_ERROR_BYTES));
    let started = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(format!(
                    "{program} timed out after {} seconds",
                    timeout.as_secs()
                ));
            }
            None => thread::sleep(Duration::from_millis(25)),
        }
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "Object listing reader failed".to_owned())?
        .map_err(|e| e.to_string())?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "Object error reader failed".to_owned())?
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(redact(
            String::from_utf8_lossy(&stderr).trim(),
            sensitive_endpoint,
        ));
    }
    if stdout.len() > MAX_LIST_OUTPUT_BYTES {
        return Err("Object-storage command returned more than 4 MiB; narrow the prefix".into());
    }
    Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
}

fn read_bounded(reader: &mut impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut stored = Vec::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(stored.len());
        stored.extend_from_slice(&chunk[..read.min(remaining)]);
    }
    Ok(stored)
}

fn redact(text: &str, sensitive_endpoint: &str) -> String {
    let text = if sensitive_endpoint.is_empty() {
        text.to_owned()
    } else {
        text.replace(sensitive_endpoint, "<endpoint>")
    };
    text.split_whitespace()
        .map(|word| {
            if word.to_ascii_lowercase().contains("token=")
                || word.to_ascii_lowercase().contains("password=")
                || word.to_ascii_lowercase().contains("secret=")
            {
                "[REDACTED]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn publish_download(temporary: &Path, destination: &Path) -> Result<(), String> {
    if !destination.exists() {
        return fs::rename(temporary, destination).map_err(|e| e.to_string());
    }
    let backup = destination.with_extension(format!(
        "{}.backup",
        destination
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("object")
    ));
    if backup.exists() {
        fs::remove_file(&backup).map_err(|e| e.to_string())?;
    }
    fs::rename(destination, &backup).map_err(|e| e.to_string())?;
    match fs::rename(temporary, destination) {
        Ok(()) => {
            fs::remove_file(backup).map_err(|e| e.to_string())?;
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(backup, destination);
            Err(error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_traversal_and_insecure_remote_endpoint() {
        let mut profile = ObjectProfile {
            name: "data".into(),
            provider: Provider::S3,
            bucket: "bucket".into(),
            prefix: "../private".into(),
            endpoint: String::new(),
            credential_hint: "AWS profile".into(),
        };
        assert!(profile.validate().is_err());
        profile.prefix = "training/".into();
        profile.endpoint = "http://example.com".into();
        assert!(profile.validate().is_err());
        profile.endpoint = "http://localhost:9000".into();
        assert!(profile.validate().is_ok());
    }
    #[test]
    fn redacts_common_secret_assignments() {
        let safe = redact(
            "failed https://storage.example token=abc password=xyz",
            "https://storage.example",
        );
        assert!(!safe.contains("abc"));
        assert!(!safe.contains("storage.example"));
        assert!(safe.contains("<endpoint>"));
    }

    #[test]
    fn prefix_aware_keys_preserve_nested_cache_identity() {
        assert_eq!(
            join_key("training/2026/", "/folds/data.parquet"),
            "training/2026/folds/data.parquet"
        );
        assert!(safe_key(&join_key("training", "../secret")).is_err());
        assert!(safe_key("bad\0key").is_err());
    }

    #[test]
    fn bounded_reader_drains_and_caps_object_output() {
        let mut source = std::io::Cursor::new(vec![1_u8; 100]);
        assert_eq!(read_bounded(&mut source, 12).unwrap().len(), 12);
        assert_eq!(source.position(), 100);
    }

    #[test]
    fn publishes_downloads_atomically_and_replaces_existing_file() {
        let root =
            std::env::temp_dir().join(format!("forge-object-publish-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let destination = root.join("model.bin");
        let temporary = root.join("model.part");
        fs::write(&destination, b"old").unwrap();
        fs::write(&temporary, b"new").unwrap();
        publish_download(&temporary, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"new");
        assert!(!temporary.exists());
        let _ = fs::remove_dir_all(root);
    }
}
