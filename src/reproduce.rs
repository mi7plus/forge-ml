//! `forge reproduce <ID>` — verify whether the current checkout and environment
//! can reproduce a recorded experiment run.
//!
//! A run records its provenance when saved (git commit, Cargo.lock hash, rustc /
//! cargo versions, OS/arch, dataset hashes — see [`crate::experiment`]). This
//! module reloads a run from the project's workspace store, recomputes the same
//! provenance for the *current* environment, and reports, dimension by
//! dimension, what matches and what diverged. The exit status is the moat: a
//! clean reproduction (same commit, same Cargo.lock, clean tree, datasets
//! unchanged) exits 0; any reproducibility-critical divergence exits non-zero so
//! the check is usable in CI (`clone && forge reproduce <ID>`).
//!
//! It verifies rather than re-executes: recreating an arbitrary IDE/notebook run
//! headlessly is out of scope. Knowing precisely whether — and why not — a run
//! is reproducible is the valuable, buildable core.

use crate::experiment::{capture_provenance, stable_digest, ExperimentRun, RunProvenance};
use forge_storage::WorkspaceStore;
use std::collections::HashMap;
use std::path::Path;

/// One recorded dataset and the hash it has in the current environment (if its
/// source could be located and re-read).
pub struct DatasetStatus {
    pub name: String,
    pub recorded: String,
    pub current: Option<String>,
}

/// The outcome of a reproduce check: a human-readable report and whether the
/// current environment reproduces the run (drives the process exit code).
pub struct Report {
    pub text: String,
    pub reproducible: bool,
}

/// Load run `query` (a full RunId or an unambiguous prefix) from the project's
/// workspace store and check the current environment against it.
pub fn reproduce(root: &Path, query: &str) -> Report {
    let store = match WorkspaceStore::open(root) {
        Ok(store) => store,
        Err(error) => return fail(format!("cannot open the workspace store: {error}")),
    };
    let runs: Vec<ExperimentRun> = match store.load_experiments() {
        Ok(runs) => runs,
        Err(error) => return fail(format!("cannot read recorded runs: {error}")),
    };
    if runs.is_empty() {
        return fail("no recorded runs in this project (save an experiment first)".into());
    }

    let matches: Vec<&ExperimentRun> = runs
        .iter()
        .filter(|run| run.id.as_str() == query || run.id.as_str().starts_with(query))
        .collect();
    let run = match matches.as_slice() {
        [run] => *run,
        [] => return fail(unknown_run_message(query, &runs)),
        many => return fail(ambiguous_run_message(query, many)),
    };

    // Recompute the environment provenance now. Datasets are re-hashed
    // separately from their recorded sources, so pass none here.
    let current = capture_provenance(Some(root), HashMap::new(), HashMap::new());
    let datasets = dataset_statuses(root, &run.provenance);

    let (body, reproducible) = evaluate(&run.provenance, &current, &datasets);
    let mut text = format!(
        "Reproduce run {} \"{}\"\n",
        short_id(run.id.as_str()),
        run.name
    );
    text.push_str(&body);
    text.push_str(if reproducible {
        "\nresult: REPRODUCIBLE — the current environment matches this run.\n"
    } else {
        "\nresult: DIVERGED — resolve the DIFF lines above to reproduce this run.\n"
    });
    Report { text, reproducible }
}

/// Re-hash each recorded dataset from its recorded source path when that source
/// is a readable local file; otherwise its current hash is unknown.
fn dataset_statuses(root: &Path, provenance: &RunProvenance) -> Vec<DatasetStatus> {
    let mut statuses: Vec<DatasetStatus> = provenance
        .datasets
        .iter()
        .map(|(name, recorded)| {
            let current = provenance
                .dataset_sources
                .get(name)
                .map(|source| resolve_source(root, source))
                .and_then(|path| std::fs::read(path).ok())
                .map(|bytes| stable_digest(&bytes));
            DatasetStatus {
                name: name.clone(),
                recorded: recorded.clone(),
                current,
            }
        })
        .collect();
    statuses.sort_by(|a, b| a.name.cmp(&b.name));
    statuses
}

/// Resolve a recorded dataset source against the project root (absolute paths and
/// URLs are returned unchanged; relative paths join the root).
fn resolve_source(root: &Path, source: &str) -> std::path::PathBuf {
    let path = Path::new(source);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(source)
    }
}

/// Compare recorded vs current provenance and produce the report body plus the
/// reproducibility verdict. Pure: no IO, so it is unit-tested directly.
fn evaluate(
    recorded: &RunProvenance,
    current: &RunProvenance,
    datasets: &[DatasetStatus],
) -> (String, bool) {
    let mut lines = String::new();
    let mut critical_ok = true;

    // Git commit — reproducibility-critical when the run recorded one.
    if recorded.git_commit.is_empty() {
        line(&mut lines, "warn", "commit", "the run recorded no git commit");
    } else if recorded.git_commit == current.git_commit {
        line(&mut lines, "ok", "commit", &short_id(&recorded.git_commit));
    } else {
        critical_ok = false;
        line(
            &mut lines,
            "DIFF",
            "commit",
            &format!(
                "recorded {}, current {} — `git checkout {}`",
                short_id(&recorded.git_commit),
                short_id(&current.git_commit),
                short_id(&recorded.git_commit),
            ),
        );
    }

    // Working tree state.
    if recorded.git_dirty {
        line(
            &mut lines,
            "warn",
            "tree",
            "the run was saved from a dirty working tree (not fully reproducible)",
        );
    }
    if current.git_dirty {
        critical_ok = false;
        line(
            &mut lines,
            "DIFF",
            "tree",
            "uncommitted changes present — commit or stash them",
        );
    }

    // Cargo.lock — reproducibility-critical when the run recorded a hash.
    if recorded.cargo_lock_hash.is_empty() {
        line(&mut lines, "warn", "Cargo.lock", "the run recorded no lock hash");
    } else if recorded.cargo_lock_hash == current.cargo_lock_hash {
        line(&mut lines, "ok", "Cargo.lock", &short_id(&recorded.cargo_lock_hash));
    } else {
        critical_ok = false;
        line(
            &mut lines,
            "DIFF",
            "Cargo.lock",
            "the crate graph changed — restore the run's Cargo.lock and `cargo build --locked`",
        );
    }

    // Toolchain and platform — informational drift, not a hard failure.
    drift(&mut lines, "rustc", &recorded.rustc_version, &current.rustc_version);
    drift(&mut lines, "cargo", &recorded.cargo_version, &current.cargo_version);
    let recorded_platform = format!("{}/{}", recorded.os, recorded.architecture);
    let current_platform = format!("{}/{}", current.os, current.architecture);
    drift(&mut lines, "platform", &recorded_platform, &current_platform);

    // Datasets — a changed input is reproducibility-critical.
    for dataset in datasets {
        match &dataset.current {
            Some(current) if *current == dataset.recorded => {
                line(&mut lines, "ok", "dataset", &format!("{} unchanged", dataset.name))
            }
            Some(_) => {
                critical_ok = false;
                line(
                    &mut lines,
                    "DIFF",
                    "dataset",
                    &format!("{} has changed since the run", dataset.name),
                )
            }
            None => line(
                &mut lines,
                "warn",
                "dataset",
                &format!("{} could not be re-read to verify", dataset.name),
            ),
        }
    }

    (lines, critical_ok)
}

/// Emit an informational line when a non-critical dimension differs.
fn drift(lines: &mut String, label: &str, recorded: &str, current: &str) {
    if recorded.is_empty() || recorded == current {
        return;
    }
    line(
        lines,
        "warn",
        label,
        &format!("recorded \"{recorded}\", current \"{current}\""),
    );
}

fn line(lines: &mut String, status: &str, label: &str, detail: &str) {
    lines.push_str(&format!("  [{status:<4}] {label:<11} {detail}\n"));
}

fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}

fn fail(message: String) -> Report {
    Report {
        text: format!("forge reproduce: {message}\n"),
        reproducible: false,
    }
}

fn unknown_run_message(query: &str, runs: &[ExperimentRun]) -> String {
    let mut message = format!("no run matches `{query}`. Recorded runs:\n");
    for run in runs.iter().rev().take(10) {
        message.push_str(&format!("  {}  {}\n", short_id(run.id.as_str()), run.name));
    }
    message
}

fn ambiguous_run_message(query: &str, matches: &[&ExperimentRun]) -> String {
    let mut message = format!("`{query}` is ambiguous; it matches:\n");
    for run in matches {
        message.push_str(&format!("  {}  {}\n", short_id(run.id.as_str()), run.name));
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> RunProvenance {
        RunProvenance {
            fingerprint_algorithm: "sha256".into(),
            git_commit: "abc123def456".into(),
            git_dirty: false,
            cargo_lock_hash: "lockhashaaaa".into(),
            rustc_version: "rustc 1.98.0".into(),
            cargo_version: "cargo 1.98.0".into(),
            os: "windows".into(),
            architecture: "x86_64".into(),
            cpu_count: 8,
            datasets: HashMap::new(),
            dataset_sources: HashMap::new(),
        }
    }

    #[test]
    fn identical_environment_reproduces() {
        let (_text, ok) = evaluate(&base(), &base(), &[]);
        assert!(ok);
    }

    #[test]
    fn diverged_commit_fails() {
        let mut current = base();
        current.git_commit = "999999999999".into();
        let (text, ok) = evaluate(&base(), &current, &[]);
        assert!(!ok);
        assert!(text.contains("[DIFF] commit"));
        assert!(text.contains("git checkout abc123def456"));
    }

    #[test]
    fn changed_lock_or_dirty_tree_fails() {
        let mut current = base();
        current.cargo_lock_hash = "different".into();
        assert!(!evaluate(&base(), &current, &[]).1);

        let mut dirty = base();
        dirty.git_dirty = true;
        assert!(!evaluate(&base(), &dirty, &[]).1);
    }

    #[test]
    fn toolchain_drift_warns_but_still_reproduces() {
        let mut current = base();
        current.rustc_version = "rustc 1.99.0".into();
        let (text, ok) = evaluate(&base(), &current, &[]);
        assert!(ok, "toolchain drift is a warning, not a failure");
        assert!(text.contains("[warn] rustc"));
    }

    #[test]
    fn changed_dataset_fails_and_missing_dataset_warns() {
        let changed = [DatasetStatus {
            name: "train.csv".into(),
            recorded: "h1".into(),
            current: Some("h2".into()),
        }];
        assert!(!evaluate(&base(), &base(), &changed).1);

        let missing = [DatasetStatus {
            name: "train.csv".into(),
            recorded: "h1".into(),
            current: None,
        }];
        let (text, ok) = evaluate(&base(), &base(), &missing);
        assert!(ok, "an unverifiable dataset warns, it does not fail");
        assert!(text.contains("could not be re-read"));
    }

    #[test]
    fn missing_provenance_warns_not_fails() {
        // A legacy run with no recorded commit or lock hash should not hard-fail
        // on those dimensions — there is nothing to compare against.
        let mut recorded = base();
        recorded.git_commit = String::new();
        recorded.cargo_lock_hash = String::new();
        let (text, ok) = evaluate(&recorded, &base(), &[]);
        assert!(ok);
        assert!(text.contains("[warn] commit"));
        assert!(text.contains("[warn] Cargo.lock"));
    }
}
