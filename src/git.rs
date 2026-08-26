use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct GitSnapshot {
    pub branch: String,
    pub files: HashMap<PathBuf, String>,
    pub summary: String,
}

pub fn snapshot(root: &Path) -> Result<GitSnapshot, String> {
    let branch = run(root, &["branch", "--show-current"])?;
    let status = run(root, &["status", "--short"])?;
    let mut files = HashMap::new();
    for line in status.lines().filter(|line| line.len() >= 3) {
        let state = line[..2].trim().to_owned();
        let raw_path = line[3..].split(" -> ").last().unwrap_or(&line[3..]);
        files.insert(PathBuf::from(raw_path), state);
    }
    Ok(GitSnapshot {
        branch: branch.trim().to_owned(),
        summary: status,
        files,
    })
}

pub fn diff(root: &Path, staged: bool) -> Result<String, String> {
    if staged {
        run(root, &["diff", "--cached"])
    } else {
        run(root, &["diff"])
    }
}

pub fn stage_all(root: &Path) -> Result<String, String> {
    run(root, &["add", "--all"])
}
pub fn unstage_all(root: &Path) -> Result<String, String> {
    run(root, &["restore", "--staged", "."])
}
pub fn commit(root: &Path, message: &str) -> Result<String, String> {
    if message.trim().is_empty() {
        return Err("Enter a commit message first.".into());
    }
    run(root, &["commit", "-m", message.trim()])
}
pub fn pull(root: &Path) -> Result<String, String> {
    run(root, &["pull", "--ff-only"])
}
pub fn push(root: &Path) -> Result<String, String> {
    run(root, &["push"])
}
pub fn branches(root: &Path) -> Result<String, String> {
    run(root, &["branch", "--all", "--no-color"])
}
pub fn switch(root: &Path, name: &str, create: bool) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("Enter a branch name first.".into());
    }
    if create {
        run(root, &["switch", "-c", name.trim()])
    } else {
        run(root, &["switch", name.trim()])
    }
}

fn run(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = if stdout.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };
    if output.status.success() {
        Ok(if text.is_empty() {
            "Done.".into()
        } else {
            text.into()
        })
    } else {
        Err(text.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_commit_message_is_rejected_before_git_runs() {
        assert!(commit(Path::new("."), "  ").is_err());
    }
}
