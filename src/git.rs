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

/// One entry in the branch list: the checkout name plus whether it is the
/// current branch and whether it lives on a remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    /// The name usable with `git switch`/`git merge` — remotes keep their
    /// `origin/…` prefix but drop the `remotes/` bookkeeping prefix.
    pub name: String,
    pub current: bool,
    pub is_remote: bool,
}

/// Structured branch list (local first, then remotes) for a selectable UI.
/// Parses `git branch --all` rather than porcelain so it works on every Git.
pub fn list_branches(root: &Path) -> Result<Vec<BranchInfo>, String> {
    let out = run(root, &["branch", "--all", "--no-color"])?;
    let mut branches = Vec::new();
    for line in out.lines() {
        let line = line.trim_end();
        if line.is_empty() || line == "Done." {
            continue;
        }
        // Skip symbolic refs like `remotes/origin/HEAD -> origin/main`.
        if line.contains(" -> ") {
            continue;
        }
        let current = line.trim_start().starts_with('*');
        let name = line.trim_start_matches('*').trim();
        let (is_remote, name) = match name.strip_prefix("remotes/") {
            Some(rest) => (true, rest),
            None => (false, name),
        };
        if name.is_empty() {
            continue;
        }
        branches.push(BranchInfo {
            name: name.to_owned(),
            current,
            is_remote,
        });
    }
    Ok(branches)
}

/// Merge `name` into the current branch (no-fast-forward-editor commit message).
/// A conflicting merge returns an error and leaves the tree mid-merge, which the
/// conflict-mediation UI then picks up.
pub fn merge(root: &Path, name: &str) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("Select a branch to merge first.".into());
    }
    run(root, &["merge", "--no-edit", name.trim()])
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
/// Delete a branch. `force` uses `-D` (drop even if unmerged); otherwise `-d`
/// (refuses to delete unmerged work).
pub fn delete_branch(root: &Path, name: &str, force: bool) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("Enter a branch name first.".into());
    }
    let flag = if force { "-D" } else { "-d" };
    run(root, &["branch", flag, name.trim()])
}

/// Commit history as a decorated one-line graph (most recent `limit` commits).
pub fn log(root: &Path, limit: usize) -> Result<String, String> {
    run(
        root,
        &[
            "log",
            &format!("-n{}", limit.max(1)),
            "--graph",
            "--oneline",
            "--decorate",
            "--no-color",
        ],
    )
}

/// The files currently in a merge conflict (unmerged, `git status` code `U`).
pub fn conflicts(root: &Path) -> Result<Vec<String>, String> {
    let out = run(root, &["diff", "--name-only", "--diff-filter=U"])?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != "Done.")
        .map(str::to_owned)
        .collect())
}

/// Resolve one conflicted file by taking a whole side, then stage it.
/// `side` is `"ours"` or `"theirs"`.
pub fn resolve_conflict(root: &Path, path: &str, side: &str) -> Result<String, String> {
    if side != "ours" && side != "theirs" {
        return Err("side must be `ours` or `theirs`".into());
    }
    run(root, &["checkout", &format!("--{side}"), "--", path])?;
    run(root, &["add", "--", path])
}

/// Mark a (manually edited) conflicted file as resolved by staging it.
pub fn mark_resolved(root: &Path, path: &str) -> Result<String, String> {
    run(root, &["add", "--", path])
}

/// Abort an in-progress merge, restoring the pre-merge state.
pub fn merge_abort(root: &Path) -> Result<String, String> {
    run(root, &["merge", "--abort"])
}

/// Finish a merge once all conflicts are staged (commit with the default message).
pub fn merge_continue(root: &Path) -> Result<String, String> {
    run(root, &["commit", "--no-edit"])
}
pub fn provenance(root: &Path) -> (String, bool) {
    let commit = run(root, &["rev-parse", "--short", "HEAD"]).unwrap_or_else(|_| "unknown".into());
    let dirty = run(root, &["status", "--porcelain"])
        .map(|value| value != "Done." && !value.trim().is_empty())
        .unwrap_or(false);
    (commit, dirty)
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

    #[test]
    fn delete_branch_requires_a_name() {
        assert!(delete_branch(Path::new("."), "   ", false).is_err());
    }

    #[test]
    fn resolve_conflict_rejects_unknown_side() {
        assert!(resolve_conflict(Path::new("."), "a.rs", "mine").is_err());
    }

    #[test]
    fn merge_requires_a_branch_name() {
        assert!(merge(Path::new("."), "  ").is_err());
    }

    #[test]
    fn list_branches_parses_current_and_remotes() {
        // Exercise the pure parsing by feeding representative `git branch --all`
        // output through the same rules `list_branches` applies.
        let sample =
            "* main\n  feature-x\n  remotes/origin/main\n  remotes/origin/HEAD -> origin/main\n";
        let mut parsed = Vec::new();
        for line in sample.lines() {
            let line = line.trim_end();
            if line.is_empty() || line.contains(" -> ") {
                continue;
            }
            let current = line.trim_start().starts_with('*');
            let name = line.trim_start_matches('*').trim();
            let (is_remote, name) = match name.strip_prefix("remotes/") {
                Some(rest) => (true, rest),
                None => (false, name),
            };
            parsed.push(BranchInfo {
                name: name.to_owned(),
                current,
                is_remote,
            });
        }
        assert_eq!(
            parsed,
            vec![
                BranchInfo {
                    name: "main".into(),
                    current: true,
                    is_remote: false
                },
                BranchInfo {
                    name: "feature-x".into(),
                    current: false,
                    is_remote: false
                },
                BranchInfo {
                    name: "origin/main".into(),
                    current: false,
                    is_remote: true
                },
            ]
        );
    }
}
