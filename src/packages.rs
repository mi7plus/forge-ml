use std::path::Path;
use std::process::Command;

pub fn cargo(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = format!("{}{}", stdout, stderr);
    if output.status.success() {
        Ok(text.trim().into())
    } else {
        Err(text.trim().into())
    }
}

pub fn search(root: &Path, query: &str) -> Result<String, String> {
    if query.trim().is_empty() {
        return Err("Enter a crate name first.".into());
    }
    cargo(root, &["search", query.trim(), "--limit", "20"])
}
pub fn search_registry(root: &Path, query: &str, registry: &str) -> Result<String, String> {
    if registry.trim().is_empty() {
        search(root, query)
    } else {
        if query.trim().is_empty() {
            return Err("Enter a crate name first.".into());
        }
        validate_registry(registry)?;
        cargo(
            root,
            &[
                "search",
                query.trim(),
                "--limit",
                "20",
                "--registry",
                registry.trim(),
            ],
        )
    }
}
fn validate_registry(name: &str) -> Result<(), String> {
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        Ok(())
    } else {
        Err(
            "Cargo registry names may contain only letters, numbers, dashes, and underscores"
                .into(),
        )
    }
}
pub fn info(root: &Path, name: &str) -> Result<String, String> {
    cargo(root, &["info", name.trim()])
}
pub fn add(root: &Path, specification: &str) -> Result<String, String> {
    if specification.trim().is_empty() {
        return Err("Enter a crate specification first.".into());
    }
    let parts = specification.split_whitespace().collect::<Vec<_>>();
    let mut arguments = vec!["add"];
    arguments.extend(parts);
    cargo(root, &arguments)
}
pub fn remove(root: &Path, name: &str) -> Result<String, String> {
    cargo(root, &["remove", name.trim()])
}
pub fn update(root: &Path) -> Result<String, String> {
    cargo(root, &["update"])
}
pub fn tree(root: &Path, duplicates_only: bool) -> Result<String, String> {
    if duplicates_only {
        cargo(root, &["tree", "--duplicates"])
    } else {
        cargo(root, &["tree"])
    }
}
pub fn licenses(root: &Path) -> Result<String, String> {
    let raw = cargo(root, &["metadata", "--format-version", "1"])?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let mut rows = value["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|package| {
            format!(
                "{} {}  {}",
                package["name"].as_str().unwrap_or("?"),
                package["version"].as_str().unwrap_or("?"),
                package["license"].as_str().unwrap_or("license unknown")
            )
        })
        .collect::<Vec<_>>();
    rows.sort();
    rows.dedup();
    Ok(rows.join("\n"))
}
pub fn audit(root: &Path) -> Result<String, String> {
    cargo(root, &["audit"])
        .map_err(|error| format!("{error}\nInstall cargo-audit with: cargo install cargo-audit"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_search_is_rejected() {
        assert!(search(Path::new("."), "").is_err());
    }
    #[test]
    fn rejects_unsafe_registry_names() {
        assert!(search_registry(Path::new("."), "serde", "bad name").is_err());
    }
}
