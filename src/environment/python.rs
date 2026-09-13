//! The Python provider (Forge Distribution, Phase 6) — a **reference**, never a
//! manager.
//!
//! Forge's core is Rust; Python is only the optional interop bridge (PyO3 /
//! maturin, Arrow/Parquet/ONNX). The roadmap is emphatic that Forge must **not**
//! build a managed Python environment — that re-imports the exact dependency
//! problem Rust lets you escape. So this provider *references* an environment a
//! project already manages with `uv` or `pixi`: it checks that an interpreter of
//! the requested version is resolvable and reports coverage or a gap, but it
//! never creates, resolves, or installs anything. (The Forge Hub — a
//! datasets/models registry — stays deferred until the core loop is polished and
//! Hugging Face is integrated first.)

use super::diagnostics::tool_version;
use super::lock::{sha256_hex, LockEntry};
use super::manifest::{Manifest, PythonRequest};
use super::provider::{Activation, Capabilities, EnvironmentProvider, Probe};
use std::path::Path;

/// A resolved Python interpreter and how it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interpreter {
    /// The `--version` line, e.g. `Python 3.13.1`.
    pub version: String,
    /// Where it came from (a project `.venv`, `uv`, or PATH).
    pub source: String,
}

/// Find a Python interpreter for the project rooted at `root`: prefer a
/// project-local `.venv` (what uv/pixi create), else fall back to PATH.
pub fn find_interpreter(root: &Path) -> Option<Interpreter> {
    let venv = if cfg!(windows) {
        root.join(".venv").join("Scripts").join("python.exe")
    } else {
        root.join(".venv").join("bin").join("python")
    };
    if venv.is_file() {
        if let Some(version) = interpreter_version(&venv.to_string_lossy()) {
            return Some(Interpreter {
                version,
                source: ".venv".to_owned(),
            });
        }
    }
    for program in ["python3", "python"] {
        if let Some(version) = interpreter_version(program) {
            return Some(Interpreter {
                version,
                source: format!("{program} on PATH"),
            });
        }
    }
    None
}

fn interpreter_version(program: &str) -> Option<String> {
    tool_version(program, &["--version"])
}

/// Whether a version line (`Python 3.13.1`) satisfies a requested `major.minor`
/// (`3.13`). A bare match on the `major.minor.` prefix, tolerant of the `Python`
/// banner.
fn version_matches(found: &str, want: &str) -> bool {
    let digits: String = found
        .chars()
        .skip_while(|character| !character.is_ascii_digit())
        .collect();
    digits == want || digits.starts_with(&format!("{want}."))
}

/// Whether the requested manager (`uv`/`pixi`) is on PATH. `system`/unset ⇒ true.
fn manager_present(request: &PythonRequest) -> bool {
    match request.manager.as_deref() {
        Some("uv") => tool_version("uv", &["--version"]).is_some(),
        Some("pixi") => tool_version("pixi", &["--version"]).is_some(),
        _ => true,
    }
}

/// A human report for `forge python check`.
pub fn report(request: &PythonRequest, root: &Path) -> String {
    if !request.present {
        return "No [python] section in forge.toml. Forge's core is Rust; add a [python] \
                section only if your project uses the Python bridge (PyO3/Arrow/ONNX), and \
                manage the env with uv or pixi.\n"
            .to_owned();
    }
    let interpreter = find_interpreter(root);
    let mut out = String::from("Python bridge:\n");
    match &interpreter {
        Some(found) => out.push_str(&format!(
            "  [ok  ] interpreter  {} ({})\n",
            found.version, found.source
        )),
        None => out.push_str("  [MISS] interpreter  none found (.venv or PATH)\n"),
    }
    if let Some(want) = &request.version {
        let ok = interpreter
            .as_ref()
            .is_some_and(|found| version_matches(&found.version, want));
        out.push_str(&format!(
            "  [{}] version      requested {want}\n",
            if ok { "ok  " } else { "MISS" }
        ));
    }
    if let Some(manager) = &request.manager {
        let ok = manager_present(request);
        out.push_str(&format!(
            "  [{}] manager      {manager}{}\n",
            if ok { "ok  " } else { "MISS" },
            if ok { "" } else { " (not on PATH)" }
        ));
    }
    if !request.bridge.is_empty() {
        out.push_str(&format!(
            "  [note] bridge       {}\n",
            request.bridge.join(", ")
        ));
    }
    if !request.packages.is_empty() {
        let installed = interpreter
            .as_ref()
            .and_then(|found| installed_packages(&interpreter_program(root, found)));
        out.push_str("  packages:\n");
        for spec in &request.packages {
            let name = normalize(pkg_name(spec));
            let status = match &installed {
                Some(set) if set.contains(&name) => "ok  ",
                Some(_) => "MISS",
                None => "?   ", // couldn't list (no interpreter / pip)
            };
            out.push_str(&format!("    [{status}] {spec}\n"));
        }
    }
    out.push_str(
        "\nForge references the interpreter (uv/pixi own the env). With `packages` declared, \
         `forge python provide` installs them into it.\n",
    );
    out
}

/// The program to invoke Python for `interpreter` under `root`: the project
/// `.venv` when that's the source, else a PATH interpreter.
fn interpreter_program(root: &Path, interpreter: &Interpreter) -> String {
    if interpreter.source == ".venv" {
        let venv = if cfg!(windows) {
            root.join(".venv").join("Scripts").join("python.exe")
        } else {
            root.join(".venv").join("bin").join("python")
        };
        return venv.to_string_lossy().into_owned();
    }
    if cfg!(windows) && tool_version("python", &["--version"]).is_some() {
        "python".to_owned()
    } else {
        "python3".to_owned()
    }
}

/// The distribution name a pip spec refers to (`pandas>=2` ⇒ `pandas`,
/// `scikit-learn[extra]` ⇒ `scikit-learn`).
fn pkg_name(spec: &str) -> &str {
    let end = spec
        .find(|c: char| "<>=!~[ @;".contains(c))
        .unwrap_or(spec.len());
    spec[..end].trim()
}

/// Normalize a distribution name for comparison (PyPI treats `-`/`_` and case
/// as equivalent): lowercase, underscores to hyphens.
fn normalize(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('_', "-")
}

/// The set of installed distribution names (normalized) reported by the
/// interpreter's pip, or `None` if pip could not be listed.
fn installed_packages(python: &str) -> Option<std::collections::HashSet<String>> {
    let output = std::process::Command::new(python)
        .args(["-m", "pip", "list", "--format=freeze"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(
        text.lines()
            .filter_map(|line| line.split(['=', '@', ' ']).next())
            .filter(|name| !name.is_empty())
            .map(normalize)
            .collect(),
    )
}

/// `forge python provide` — install the declared `packages` into the referenced
/// environment. Uses `pixi add` when the manager is pixi, otherwise the resolved
/// interpreter's `pip`. Real installs; nothing beyond the package manager runs.
pub fn provide(request: &PythonRequest, root: &Path) -> String {
    if !request.present || request.packages.is_empty() {
        return "Nothing to install — declare `packages = [...]` under [python] in forge.toml.\n"
            .to_owned();
    }
    // pixi manages its own environment file; add packages there.
    if request.manager.as_deref() == Some("pixi") {
        return match std::process::Command::new("pixi")
            .current_dir(root)
            .arg("add")
            .args(&request.packages)
            .output()
        {
            Ok(output) if output.status.success() => {
                format!("Installed {} package(s) with `pixi add`.\n", request.packages.len())
            }
            Ok(output) => format!(
                "forge python provide: `pixi add` failed: {}\n",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            Err(error) => format!("forge python provide: could not run pixi: {error}\n"),
        };
    }

    let Some(interpreter) = find_interpreter(root) else {
        return "forge python provide: no interpreter found (create one with `uv venv` or `python -m venv .venv`).\n"
            .to_owned();
    };
    let python = interpreter_program(root, &interpreter);
    match std::process::Command::new(&python)
        .args(["-m", "pip", "install"])
        .args(&request.packages)
        .output()
    {
        Ok(output) if output.status.success() => format!(
            "Installed {} package(s) into {} ({}).\n",
            request.packages.len(),
            interpreter.source,
            interpreter.version
        ),
        Ok(output) => format!(
            "forge python provide: pip install failed: {}\n",
            String::from_utf8_lossy(&output.stderr).trim().lines().last().unwrap_or("")
        ),
        Err(error) => format!("forge python provide: could not run {python}: {error}\n"),
    }
}

pub struct PythonProvider;

impl EnvironmentProvider for PythonProvider {
    fn id(&self) -> &'static str {
        "python"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            python: true,
            ..Capabilities::default()
        }
    }

    fn probe(&self, manifest: &Manifest) -> Probe {
        let request = manifest.python_request();
        // Cheap path: no `[python]` section (startup/default) — no probing.
        if !request.present {
            return Probe::Missing("no [python] section in forge.toml".to_owned());
        }
        // The provider is root-less; it evaluates a PATH interpreter. The
        // `.venv`-aware view is `forge python check [dir]`.
        let interpreter = interpreter_version("python3").or_else(|| interpreter_version("python"));
        evaluate(&request, interpreter.as_deref(), manager_present(&request))
    }

    fn activate(&self, _manifest: &Manifest) -> Option<Activation> {
        // Referencing only: Forge does not put a Python env on PATH or mutate the
        // environment. The bridge build uses the interpreter the project selects.
        None
    }

    fn materialize(&self, manifest: &Manifest) -> Result<LockEntry, String> {
        let request = manifest.python_request();
        let interpreter = interpreter_version("python3").or_else(|| interpreter_version("python"));
        let version = interpreter.unwrap_or_else(|| "none".to_owned());
        let mut extra = toml::Table::new();
        extra.insert("require".to_owned(), toml::Value::Boolean(request.require));
        if let Some(manager) = &request.manager {
            extra.insert("manager".to_owned(), toml::Value::String(manager.clone()));
        }
        if let Some(want) = &request.version {
            extra.insert("requested".to_owned(), toml::Value::String(want.clone()));
        }
        if !request.packages.is_empty() {
            extra.insert(
                "packages".to_owned(),
                toml::Value::Array(
                    request.packages.iter().cloned().map(toml::Value::String).collect(),
                ),
            );
        }
        Ok(LockEntry {
            id: self.id().to_owned(),
            kind: "python-ref".to_owned(),
            version: version.clone(),
            sha256: sha256_hex(version.as_bytes()),
            extra,
        })
    }
}

/// Pure probe logic: is the referenced Python satisfiable? A missing interpreter
/// or a version mismatch is only fatal when `require = true`; otherwise Forge
/// reports it but stays available (the bridge is optional).
fn evaluate(request: &PythonRequest, interpreter: Option<&str>, manager_present: bool) -> Probe {
    match interpreter {
        Some(version) => {
            if let Some(want) = &request.version {
                if !version_matches(version, want) && request.require {
                    return Probe::Incompatible(format!(
                        "[python] requires {want}, but the interpreter is `{version}`"
                    ));
                }
            }
            if !manager_present && request.require {
                return Probe::Incompatible(
                    "[python] manager is not on PATH (install uv or pixi)".to_owned(),
                );
            }
            Probe::Available
        }
        None if request.require => {
            Probe::Missing("no Python interpreter found (create one with uv/pixi)".to_owned())
        }
        None => Probe::Available,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(version: Option<&str>, require: bool) -> PythonRequest {
        PythonRequest {
            present: true,
            version: version.map(str::to_owned),
            manager: None,
            bridge: Vec::new(),
            packages: Vec::new(),
            require,
        }
    }

    #[test]
    fn pkg_name_strips_version_specifiers() {
        assert_eq!(pkg_name("numpy"), "numpy");
        assert_eq!(pkg_name("pandas>=2"), "pandas");
        assert_eq!(pkg_name("scikit-learn[extra]"), "scikit-learn");
        assert_eq!(normalize("Scikit_Learn"), "scikit-learn");
    }

    #[test]
    fn version_matching_is_major_minor() {
        assert!(version_matches("Python 3.13.1", "3.13"));
        assert!(version_matches("Python 3.13", "3.13"));
        assert!(!version_matches("Python 3.12.7", "3.13"));
        assert!(!version_matches("Python 3.1", "3.13"));
    }

    #[test]
    fn matching_interpreter_is_available() {
        assert_eq!(
            evaluate(&request(Some("3.13"), true), Some("Python 3.13.2"), true),
            Probe::Available
        );
    }

    #[test]
    fn required_version_mismatch_is_incompatible() {
        let probe = evaluate(&request(Some("3.13"), true), Some("Python 3.11.0"), true);
        assert!(matches!(probe, Probe::Incompatible(_)));
    }

    #[test]
    fn missing_interpreter_is_missing_only_when_required() {
        assert!(matches!(
            evaluate(&request(None, true), None, true),
            Probe::Missing(_)
        ));
        assert_eq!(
            evaluate(&request(None, false), None, true),
            Probe::Available
        );
    }

    #[test]
    fn required_manager_absent_is_incompatible() {
        let probe = evaluate(&request(None, true), Some("Python 3.13.0"), false);
        assert!(matches!(probe, Probe::Incompatible(_)));
    }
}
