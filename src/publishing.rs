use std::path::Path;
use std::process::Command;

fn run(program: &Path, args: &[&str], root: &Path) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        Ok(text.trim().into())
    } else {
        Err(text.trim().into())
    }
}

pub fn cargo_package(root: &Path) -> Result<String, String> {
    run(Path::new("cargo"), &["package", "--allow-dirty"], root)
}
pub fn cargo_publish_dry_run(root: &Path) -> Result<String, String> {
    run(
        Path::new("cargo"),
        &["publish", "--dry-run", "--allow-dirty"],
        root,
    )
}
pub fn python_build(root: &Path, python: &Path) -> Result<String, String> {
    run(python, &["-m", "build", "--outdir", ".forge/dist"], root)
}
pub fn python_smoke_test(root: &Path, python: &Path) -> Result<String, String> {
    run(python, &["-m", "compileall", "-q", "."], root)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_command_reports_error() {
        assert!(run(Path::new("definitely-not-a-command"), &[], Path::new(".")).is_err());
    }
}
