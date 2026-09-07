//! System presence diagnostics for `forge doctor`.
//!
//! These complement the provider probes: the providers report whether Forge can
//! *activate* an environment for a manifest; these report what is actually
//! installed on the machine — rustc, cargo, rustup, a C compiler/linker, CUDA,
//! and Python — so `forge doctor` can explain *why* something is missing. Every
//! probe is cheap, read-only, and best-effort; they run only for `doctor`, never
//! at app startup.

use std::process::Command;

/// The state of one diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Installed and runnable.
    Present,
    /// Not found, and its absence will bite (e.g. no C linker).
    Absent,
    /// Not found or not applicable, but that is fine here (optional tooling).
    Note,
}

/// One host tooling check.
#[derive(Debug, Clone)]
pub struct Check {
    pub label: &'static str,
    pub status: Status,
    pub detail: String,
}

impl Check {
    fn present(label: &'static str, detail: impl Into<String>) -> Self {
        Check {
            label,
            status: Status::Present,
            detail: detail.into(),
        }
    }
    fn absent(label: &'static str, detail: impl Into<String>) -> Self {
        Check {
            label,
            status: Status::Absent,
            detail: detail.into(),
        }
    }
    fn note(label: &'static str, detail: impl Into<String>) -> Self {
        Check {
            label,
            status: Status::Note,
            detail: detail.into(),
        }
    }
}

/// Run every diagnostic (shells out; bounded and best-effort).
pub fn run() -> Vec<Check> {
    vec![
        match tool_version("rustc", &["--version"]) {
            Some(version) => Check::present("rustc", version),
            None => Check::absent("rustc", "not on PATH — install from https://rustup.rs"),
        },
        match tool_version("cargo", &["--version"]) {
            Some(version) => Check::present("cargo", version),
            None => Check::absent("cargo", "not on PATH — install from https://rustup.rs"),
        },
        match tool_version("rustup", &["--version"]) {
            Some(version) => Check::present("rustup", version),
            None => Check::note("rustup", "not found (optional toolchain manager)"),
        },
        c_toolchain_check(),
        cuda_check(),
        python_check(),
        Check::note(
            "BLAS/LAPACK",
            "not required — Forge's linfa/smartcore/Millwright stack is pure Rust",
        ),
    ]
}

/// Format the checks as a `doctor` section.
pub fn format(checks: &[Check]) -> String {
    let mut out = String::from("diagnostics:\n");
    for check in checks {
        let marker = match check.status {
            Status::Present => "ok  ",
            Status::Absent => "MISS",
            Status::Note => "note",
        };
        out.push_str(&format!("  [{marker}] {:<12} {}\n", check.label, check.detail));
    }
    out
}

/// A C compiler / linker, needed to build native crates. Reports the first of
/// cc/clang/gcc/cl found; on Windows a miss is a note (MSVC is usually present
/// via Visual Studio Build Tools but off PATH outside a developer prompt).
fn c_toolchain_check() -> Check {
    for program in ["cc", "clang", "gcc"] {
        if let Some(version) = tool_version(program, &["--version"]) {
            return Check::present("C toolchain", format!("{program}: {version}"));
        }
    }
    // MSVC `cl` prints its banner to stderr and exits non-zero with no inputs, so
    // detect it by whether it runs at all rather than by a clean `--version`.
    if cfg!(windows) && Command::new("cl").output().is_ok() {
        return Check::present("C toolchain", "cl (MSVC)");
    }
    if cfg!(windows) {
        Check::note(
            "C toolchain",
            "no cc/clang/cl on PATH — install VS Build Tools, or run from a Developer prompt for native builds",
        )
    } else {
        Check::absent(
            "C toolchain",
            "no cc/clang/gcc on PATH — needed to link native crates",
        )
    }
}

/// NVIDIA CUDA presence. GPU acceleration ships in every installer, but Linux GPU
/// ONNX inference needs the CUDA toolkit; DirectML (Windows) and CoreML (macOS)
/// need nothing extra, so a miss is a note rather than a failure.
fn cuda_check() -> Check {
    if let Some(version) = tool_version("nvcc", &["--version"]).and_then(|out| {
        // nvcc prints several lines; keep the "release" line if present.
        out.lines()
            .find(|line| line.contains("release"))
            .map(str::to_owned)
            .or(Some(out))
    }) {
        return Check::present("CUDA", format!("nvcc: {version}"));
    }
    if Command::new("nvidia-smi").arg("-L").output().is_ok_and(|o| o.status.success()) {
        return Check::note(
            "CUDA",
            "NVIDIA driver present but no nvcc toolkit — needed for Linux GPU ONNX inference",
        );
    }
    Check::note(
        "CUDA",
        "no NVIDIA CUDA detected — GPU still works via DirectML (Windows) / CoreML (macOS); Linux GPU ONNX needs CUDA",
    )
}

/// Python, used only for the optional notebook Python bridge.
fn python_check() -> Check {
    for program in ["python3", "python"] {
        if let Some(version) = tool_version(program, &["--version"]) {
            return Check::present("Python", version);
        }
    }
    Check::note(
        "Python",
        "not found (optional — only the notebook Python bridge uses it)",
    )
}

/// The first non-empty line a tool prints (stdout, then stderr) when run with
/// `args`, or `None` if it is not runnable. Shared with the system-toolchain
/// provider. Only treats a zero exit as success.
pub(super) fn tool_version(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(line) = stdout.lines().map(str::trim).find(|line| !line.is_empty()) {
        return Some(line.to_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_renders_a_marker_per_status() {
        let checks = vec![
            Check::present("rustc", "rustc 1.98.0"),
            Check::absent("cargo", "missing"),
            Check::note("Python", "optional"),
        ];
        let text = format(&checks);
        assert!(text.starts_with("diagnostics:\n"));
        assert!(text.contains("[ok  ] rustc"));
        assert!(text.contains("[MISS] cargo"));
        assert!(text.contains("[note] Python"));
    }

    #[test]
    fn run_always_reports_rustc_cargo_and_blas_note() {
        let checks = run();
        let labels: Vec<&str> = checks.iter().map(|check| check.label).collect();
        assert!(labels.contains(&"rustc"));
        assert!(labels.contains(&"cargo"));
        // The BLAS line is a fixed note regardless of the host.
        let blas = checks.iter().find(|check| check.label == "BLAS/LAPACK").unwrap();
        assert_eq!(blas.status, Status::Note);
    }
}
