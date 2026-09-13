//! Command-line seams handled by the `forge_ide` binary before the GUI starts.
//!
//! The thin `forge` CLI (`crates/forge-cli`) shells out to these `--…` flags, and
//! the Forge Manager consumes `--env-status-json`. [`dispatch`] runs the matching
//! handler and returns `true` when it handled the arguments, so `main` can return
//! early without opening a window. See `docs/FORGE_ENV.md`.

use crate::{environment, reproduce};
use std::path::PathBuf;

/// The `[dir]` argument that follows an environment flag, defaulting to the
/// current directory. Ignores a following `--flag`.
fn env_cli_dir(args: &[String], flag_pos: usize) -> PathBuf {
    args.get(flag_pos + 1)
        .filter(|value| !value.starts_with("--"))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Load a project's manifest, defaulting to an empty one on absence or error.
fn manifest_at(root: &std::path::Path) -> environment::Manifest {
    environment::Manifest::load(root)
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Handle an environment/reproduce CLI flag if present. Returns `true` when the
/// arguments were handled (caller should exit), `false` to continue into the GUI.
pub fn dispatch(cli: &[String]) -> bool {
    if let Some(pos) = cli.iter().position(|a| a == "--env-doctor") {
        print!("{}", environment::doctor(&env_cli_dir(cli, pos)));
        return true;
    }
    if let Some(pos) = cli.iter().position(|a| a == "--env-sync") {
        let root = env_cli_dir(cli, pos);
        match environment::sync_project(&root) {
            Ok(path) => println!("Wrote {}", path.display()),
            Err(error) => eprintln!("forge env sync failed: {error}"),
        }
        return true;
    }
    // Machine-readable snapshot for the Forge Manager GUI.
    if let Some(pos) = cli.iter().position(|a| a == "--env-status-json") {
        println!("{}", environment::status_json(&env_cli_dir(cli, pos)));
        return true;
    }
    // `--gpu-detect` reports the GPU backends detected on this machine.
    if cli.iter().any(|a| a == "--gpu-detect") {
        print!("{}", environment::gpu_report());
        return true;
    }
    // `--native-check [dir]` checks a project's [native] prerequisites (no install).
    if let Some(pos) = cli.iter().position(|a| a == "--native-check") {
        let manifest = manifest_at(&env_cli_dir(cli, pos));
        print!("{}", environment::native_report(&manifest.native_request()));
        return true;
    }
    // `--native-provide [dir] [--tools a,b]` downloads+verifies+extracts prebuilts.
    if let Some(pos) = cli.iter().position(|a| a == "--native-provide") {
        let root = env_cli_dir(cli, pos);
        let tools: Vec<String> = cli
            .iter()
            .position(|a| a == "--tools")
            .and_then(|i| cli.get(i + 1))
            .map(|value| {
                value
                    .split(',')
                    .map(|tool| tool.trim().to_owned())
                    .filter(|tool| !tool.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        print!("{}", environment::native_provide(&root, &tools));
        return true;
    }
    // `--native-pin <url> --archive <zip|tar-gz> [--name <n>]`.
    if let Some(pos) = cli.iter().position(|a| a == "--native-pin") {
        let url = cli.get(pos + 1).filter(|value| !value.starts_with("--"));
        let flag = |name: &str| {
            cli.iter()
                .position(|a| a == name)
                .and_then(|i| cli.get(i + 1))
                .map(String::as_str)
        };
        match url {
            Some(url) => print!(
                "{}",
                environment::native_pin(url, flag("--archive").unwrap_or("zip"), flag("--name"))
            ),
            None => eprintln!("forge native pin: usage: forge native pin <https-url> --archive <zip|tar-gz> [--name <n>]"),
        }
        return true;
    }
    // `--python-check [dir]` reports the referenced Python bridge env for a project.
    if let Some(pos) = cli.iter().position(|a| a == "--python-check") {
        let root = env_cli_dir(cli, pos);
        print!(
            "{}",
            environment::python_report(&manifest_at(&root).python_request(), &root)
        );
        return true;
    }
    // `--cargo-check [dir]` reports which `[cargo].crates` are already in Cargo.toml.
    if let Some(pos) = cli.iter().position(|a| a == "--cargo-check") {
        let root = env_cli_dir(cli, pos);
        print!(
            "{}",
            environment::cargo_report(&manifest_at(&root).cargo_request(), &root)
        );
        return true;
    }
    // `--cargo-provide [dir]` runs `cargo add` for each declared crate + `cargo fetch`.
    if let Some(pos) = cli.iter().position(|a| a == "--cargo-provide") {
        let root = env_cli_dir(cli, pos);
        print!(
            "{}",
            environment::cargo_provide(&root, &manifest_at(&root).cargo_request())
        );
        return true;
    }
    // `--python-provide [dir]` installs the declared [python].packages into the env.
    if let Some(pos) = cli.iter().position(|a| a == "--python-provide") {
        let root = env_cli_dir(cli, pos);
        print!(
            "{}",
            environment::python_provide(&manifest_at(&root).python_request(), &root)
        );
        return true;
    }
    // `--env-provide [dir]` provisions the whole manifest: native tools, crates, packages.
    if let Some(pos) = cli.iter().position(|a| a == "--env-provide") {
        let root = env_cli_dir(cli, pos);
        let manifest = manifest_at(&root);
        print!("{}", environment::native_provide(&root, &[]));
        print!(
            "{}",
            environment::cargo_provide(&root, &manifest.cargo_request())
        );
        print!(
            "{}",
            environment::python_provide(&manifest.python_request(), &root)
        );
        return true;
    }
    // `--reproduce <ID> [dir]` verifies the environment against a recorded run's
    // provenance; exits non-zero on a reproducibility-critical divergence.
    if let Some(pos) = cli.iter().position(|a| a == "--reproduce") {
        let Some(id) = cli.get(pos + 1).filter(|value| !value.starts_with("--")) else {
            eprintln!("forge reproduce: usage: forge reproduce <run-id> [dir]");
            return true;
        };
        let report = reproduce::reproduce(&env_cli_dir(cli, pos + 1), id);
        print!("{}", report.text);
        if !report.reproducible {
            std::process::exit(1);
        }
        return true;
    }
    false
}
