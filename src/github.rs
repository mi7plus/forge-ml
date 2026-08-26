use std::path::Path;
use std::process::Command;

pub fn gh(root: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("gh");
    command.args(args);
    if let Some(root) = root {
        command.current_dir(root);
    }
    let output = command
        .output()
        .map_err(|e| format!("GitHub CLI is not available: {e}"))?;
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

pub fn auth_status() -> Result<String, String> {
    gh(None, &["auth", "status"])
}
pub fn repos(root: &Path) -> Result<String, String> {
    gh(Some(root), &["repo", "view"])
}
pub fn fork(root: &Path) -> Result<String, String> {
    gh(Some(root), &["repo", "fork", "--remote"])
}
pub fn publish(root: &Path, name: &str) -> Result<String, String> {
    let mut args = vec!["repo", "create"];
    if !name.trim().is_empty() {
        args.push(name.trim());
    }
    args.extend(["--source", ".", "--push"]);
    gh(Some(root), &args)
}
pub fn clone(repo: &str, destination: &Path) -> Result<String, String> {
    if repo.trim().is_empty() {
        return Err("Enter owner/repository first.".into());
    }
    gh(Some(destination), &["repo", "clone", repo.trim()])
}
pub fn prs(root: &Path) -> Result<String, String> {
    gh(Some(root), &["pr", "list"])
}
pub fn create_pr(root: &Path, title: &str) -> Result<String, String> {
    gh(Some(root), &["pr", "create", "--title", title, "--fill"])
}
pub fn issues(root: &Path) -> Result<String, String> {
    gh(Some(root), &["issue", "list"])
}
pub fn create_issue(root: &Path, title: &str) -> Result<String, String> {
    gh(
        Some(root),
        &[
            "issue",
            "create",
            "--title",
            title,
            "--body",
            "Created from Forge ML",
        ],
    )
}
pub fn actions(root: &Path) -> Result<String, String> {
    gh(Some(root), &["run", "list", "--limit", "20"])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clone_requires_repo() {
        assert!(clone("", Path::new(".")).is_err());
    }
}
