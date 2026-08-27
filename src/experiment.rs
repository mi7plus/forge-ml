use forge_protocol::RunId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct RunProvenance {
    #[serde(default)]
    pub fingerprint_algorithm: String,
    pub git_commit: String,
    pub git_dirty: bool,
    pub cargo_lock_hash: String,
    pub rustc_version: String,
    pub cargo_version: String,
    pub os: String,
    pub architecture: String,
    pub cpu_count: usize,
    pub datasets: HashMap<String, String>,
    #[serde(default)]
    pub dataset_sources: HashMap<String, String>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct GitHubLinks {
    pub issue: Option<String>,
    pub pull_request: Option<String>,
    pub action_run: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ExperimentRun {
    #[serde(default)]
    pub id: RunId,
    pub name: String,
    pub metrics: HashMap<String, Vec<[f64; 2]>>,
    pub vectors: HashMap<String, Vec<f64>>,
    pub execution_count: usize,
    #[serde(default)]
    pub created_at_unix: u64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub parent_id: Option<RunId>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub provenance: RunProvenance,
    #[serde(default)]
    pub github: GitHubLinks,
}

impl ExperimentRun {
    pub fn snapshot(
        name: String,
        metrics: &HashMap<String, Vec<[f64; 2]>>,
        vectors: &HashMap<String, Vec<f64>>,
        execution_count: usize,
    ) -> Self {
        Self {
            id: RunId::new(),
            name,
            metrics: metrics.clone(),
            vectors: vectors.clone(),
            execution_count,
            created_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            tags: Vec::new(),
            notes: String::new(),
            archived: false,
            parent_id: None,
            artifacts: Vec::new(),
            provenance: RunProvenance::default(),
            github: GitHubLinks::default(),
        }
    }

    pub fn clone_as_child(&self, name: String) -> Self {
        let mut child = self.clone();
        child.id = RunId::new();
        child.name = name;
        child.parent_id = Some(self.id.clone());
        child.archived = false;
        child.created_at_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        child
    }
}

pub fn capture_provenance(
    root: Option<&Path>,
    datasets: HashMap<String, String>,
    dataset_sources: HashMap<String, String>,
) -> RunProvenance {
    let command = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .current_dir(root.unwrap_or(Path::new(".")))
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default()
    };
    let git_commit = command("git", &["rev-parse", "HEAD"]);
    let git_dirty = !command("git", &["status", "--porcelain"]).is_empty();
    let cargo_lock_hash = root
        .and_then(|root| std::fs::read(root.join("Cargo.lock")).ok())
        .map(|bytes| stable_digest(&bytes))
        .unwrap_or_default();
    RunProvenance {
        fingerprint_algorithm: "sha256".into(),
        git_commit,
        git_dirty,
        cargo_lock_hash,
        rustc_version: command("rustc", &["--version"]),
        cargo_version: command("cargo", &["--version"]),
        os: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        cpu_count: std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1),
        datasets,
        dataset_sources,
    }
}

pub fn stable_digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_snapshots_receive_defaults() {
        let run: ExperimentRun = serde_json::from_str(
            r#"{"name":"baseline","metrics":{},"vectors":{},"execution_count":1}"#,
        )
        .unwrap();
        assert!(!run.id.as_str().is_empty());
        assert!(run.tags.is_empty());
        assert!(!run.archived);
    }
    #[test]
    fn cloned_run_records_parent() {
        let run = ExperimentRun::snapshot("parent".into(), &HashMap::new(), &HashMap::new(), 1);
        let child = run.clone_as_child("child".into());
        assert_eq!(child.parent_id, Some(run.id));
    }

    #[test]
    fn fingerprints_use_stable_sha256() {
        assert_eq!(
            stable_digest(b"forge"),
            "71b41d6dd48dc58eba8f5cf9edf30fef6597fdf285a521bb8fcbad4b3d50887d"
        );
    }
}
