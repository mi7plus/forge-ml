use serde::{Deserialize, Serialize};
use std::{
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

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
                run_bounded("aws", &args, Duration::from_secs(30))
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
            )
            .map(|text| text.lines().take(limit).collect::<Vec<_>>().join("\n")),
        }
    }
    pub fn download(&self, key: &str, project: &Path) -> Result<PathBuf, String> {
        self.validate()?;
        safe_key(key)?;
        let file_name = Path::new(key)
            .file_name()
            .ok_or("Object key has no file name")?;
        let destination_dir = project.join(".forge/object-cache");
        std::fs::create_dir_all(&destination_dir).map_err(|e| e.to_string())?;
        let destination = destination_dir.join(file_name);
        match self.provider {
            Provider::S3 => {
                let uri = format!("s3://{}/{key}", self.bucket);
                let destination_text = destination.to_string_lossy().into_owned();
                let mut args = vec!["s3", "cp", &uri, &destination_text];
                if !self.endpoint.is_empty() {
                    args.extend(["--endpoint-url", &self.endpoint]);
                }
                run_bounded("aws", &args, Duration::from_secs(120))?;
            }
            Provider::Rclone => {
                let source = format!("{}:{key}", self.bucket);
                let destination_text = destination.to_string_lossy().into_owned();
                run_bounded(
                    "rclone",
                    &["copyto", &source, &destination_text],
                    Duration::from_secs(120),
                )?;
            }
        }
        Ok(destination)
    }
}

fn safe_key(key: &str) -> Result<(), String> {
    if Path::new(key).components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        Err("Object keys must be relative and may not traverse directories".into())
    } else {
        Ok(())
    }
}
fn is_local(url: &url::Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

fn run_bounded(program: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program} is not available: {e}"))?;
    let started = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            let output = child.wait_with_output().map_err(|e| e.to_string())?;
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return if output.status.success() {
                Ok(stdout)
            } else {
                Err(redact(&stderr))
            };
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "{program} timed out after {} seconds",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn redact(text: &str) -> String {
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
        assert!(!redact("failed token=abc password=xyz").contains("abc"));
    }
}
