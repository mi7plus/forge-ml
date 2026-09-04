//! `forge` — one entry point for Rust ML projects, built *around* Cargo.
//!
//! It never replaces `cargo`/`rustup`; it wraps them with data-science defaults
//! and hands the environment commands to the `forge_ide` binary, which already
//! resolves the offline runtime and the `forge.toml`/`forge.lock` environment.
//! Deliberately dependency-free.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = match args.split_first() {
        Some((cmd, rest)) => (cmd.as_str(), rest),
        None => {
            print_help();
            return ExitCode::SUCCESS;
        }
    };

    let result = match cmd {
        "new" => cmd_new(rest),
        "add" => cmd_add(rest),
        "run" => passthrough_cargo("run", rest),
        "build" => passthrough_cargo("build", rest),
        "test" => passthrough_cargo("test", rest),
        "env" => cmd_env(rest),
        "doctor" => run_forge_ide(&["--env-doctor".to_owned()]),
        "ide" => cmd_ide(rest),
        "version" | "--version" | "-V" => {
            println!("forge {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command `{other}` (try `forge help`)")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("forge: {error}");
            ExitCode::FAILURE
        }
    }
}

/// `forge new NAME [--profile P]` — scaffold a Cargo project, write a forge.toml,
/// and add the profile's curated crate set.
fn cmd_new(args: &[String]) -> Result<(), String> {
    let (name, profile) = parse_new_args(args)?;
    if profile_crates(profile).is_none() {
        return Err(format!(
            "unknown profile `{profile}` (data | classical-ml | deep-learning)"
        ));
    }

    status(cargo().args(["new", name]))?;
    let root = PathBuf::from(name);
    std::fs::write(root.join("forge.toml"), forge_toml(name, profile))
        .map_err(|e| format!("writing forge.toml: {e}"))?;

    // Add the profile's crates with data-science feature defaults.
    for krate in profile_crates(profile).unwrap() {
        status(
            cargo()
                .current_dir(&root)
                .args(["add"])
                .args(add_args(krate)),
        )?;
    }
    println!("Created {name} (profile: {profile}). Try: cd {name} && forge ide");
    Ok(())
}

/// `forge add CRATE...` — `cargo add` with sensible ML feature defaults per crate.
fn cmd_add(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: forge add <crate> [crate...]".into());
    }
    for krate in args {
        status(cargo().args(["add"]).args(add_args(krate)))?;
    }
    Ok(())
}

/// `forge env sync|doctor [dir]` — delegate to forge_ide's environment entry points.
fn cmd_env(args: &[String]) -> Result<(), String> {
    let (sub, rest) = args
        .split_first()
        .ok_or("usage: forge env <sync|doctor> [dir]")?;
    let mut forwarded = match sub.as_str() {
        "sync" => vec!["--env-sync".to_owned()],
        "doctor" => vec!["--env-doctor".to_owned()],
        other => return Err(format!("unknown env subcommand `{other}` (sync | doctor)")),
    };
    forwarded.extend(rest.iter().cloned());
    run_forge_ide(&forwarded)
}

/// `forge ide [dir]` — open the Forge ML desktop app on a project.
fn cmd_ide(args: &[String]) -> Result<(), String> {
    // Launch detached so the terminal returns; the GUI owns its lifetime.
    let exe = forge_ide_path()?;
    let mut command = Command::new(exe);
    if let Some(dir) = args.first() {
        command.arg(dir);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("launching forge_ide: {e}"))
}

fn passthrough_cargo(sub: &str, args: &[String]) -> Result<(), String> {
    status(cargo().arg(sub).args(args))
}

fn run_forge_ide(args: &[String]) -> Result<(), String> {
    status(Command::new(forge_ide_path()?).args(args))
}

// ── pure helpers (unit-tested) ───────────────────────────────────────────────

/// The curated crate set for a profile, or `None` if the profile is unknown.
fn profile_crates(profile: &str) -> Option<&'static [&'static str]> {
    match profile {
        "data" => Some(&["polars", "ndarray", "plotters", "statrs"]),
        "classical-ml" => Some(&["polars", "ndarray", "linfa", "smartcore", "millwright"]),
        "deep-learning" => Some(&["burn", "ndarray"]),
        _ => None,
    }
}

/// `cargo add` arguments for one crate, applying data-science feature defaults.
fn add_args(krate: &str) -> Vec<String> {
    let mut args = vec![krate.to_owned()];
    let features: &[&str] = match krate {
        "polars" => &["lazy", "csv", "parquet"],
        "burn" => &["train"],
        _ => &[],
    };
    if !features.is_empty() {
        args.push("--features".to_owned());
        args.push(features.join(","));
    }
    args
}

/// The generated `forge.toml` for a new project.
fn forge_toml(name: &str, profile: &str) -> String {
    format!(
        "# Forge environment manifest — see docs/FORGE_ENV.md\n\
         schema = 1\n\n\
         [environment]\n\
         name = \"{name}\"\n\
         profile = \"{profile}\"\n\
         channel = \"stable\"\n"
    )
}

/// Parse `forge new` args into `(name, profile)`, accepting `--profile X` and
/// `--profile=X`. Defaults the profile to `classical-ml`.
fn parse_new_args(args: &[String]) -> Result<(&str, &str), String> {
    let mut name = None;
    let mut profile = "classical-ml";
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if let Some(value) = arg.strip_prefix("--profile=") {
            profile = value;
            i += 1;
        } else if arg == "--profile" {
            profile = args
                .get(i + 1)
                .map(String::as_str)
                .ok_or("--profile needs a value")?;
            i += 2;
        } else if !arg.starts_with("--") && name.is_none() {
            name = Some(arg);
            i += 1;
        } else {
            i += 1;
        }
    }
    let name = name.ok_or("usage: forge new <name> [--profile data|classical-ml|deep-learning]")?;
    Ok((name, profile))
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// Locate the `forge_ide` binary: next to this executable first (installed
/// layout), then on `PATH`.
fn forge_ide_path() -> Result<PathBuf, String> {
    let exe_name = if cfg!(windows) {
        "forge_ide.exe"
    } else {
        "forge_ide"
    };
    if let Ok(here) = std::env::current_exe() {
        if let Some(dir) = here.parent() {
            let sibling = dir.join(exe_name);
            if sibling.is_file() {
                return Ok(sibling);
            }
        }
    }
    // Fall back to PATH (dev: `cargo run` puts both in target/<profile>/).
    Ok(PathBuf::from(exe_name))
}

fn status(command: &mut Command) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|e| format!("running {:?}: {e}", command.get_program()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{:?} exited with {}",
            command.get_program(),
            status
                .code()
                .map_or_else(|| "signal".to_owned(), |c| c.to_string())
        ))
    }
}

fn print_help() {
    println!(
        "forge {} — a Rust ML workflow around Cargo\n\n\
         USAGE:\n\
         \x20 forge new <name> [--profile P]   scaffold a project + forge.toml (P: data|classical-ml|deep-learning)\n\
         \x20 forge add <crate>...             cargo add with data-science feature defaults\n\
         \x20 forge run|build|test [args]      cargo passthrough\n\
         \x20 forge env sync|doctor [dir]      write forge.lock / report the environment\n\
         \x20 forge doctor                     diagnose the current environment\n\
         \x20 forge ide [dir]                  open the Forge ML desktop app\n\
         \x20 forge version                    print the version",
        env!("CARGO_PKG_VERSION")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_known_and_deep_learning_is_curated() {
        assert!(profile_crates("data").is_some());
        assert!(profile_crates("classical-ml")
            .unwrap()
            .contains(&"millwright"));
        assert!(profile_crates("deep-learning").unwrap().contains(&"burn"));
        assert!(profile_crates("bogus").is_none());
    }

    #[test]
    fn add_args_apply_feature_defaults() {
        assert_eq!(
            add_args("polars"),
            ["polars", "--features", "lazy,csv,parquet"]
        );
        assert_eq!(add_args("burn"), ["burn", "--features", "train"]);
        assert_eq!(add_args("ndarray"), ["ndarray"]);
    }

    #[test]
    fn parse_new_args_handles_both_profile_forms() {
        let space = vec!["myproj".into(), "--profile".into(), "data".into()];
        assert_eq!(parse_new_args(&space).unwrap(), ("myproj", "data"));
        let eq = vec!["--profile=deep-learning".into(), "myproj".into()];
        assert_eq!(parse_new_args(&eq).unwrap(), ("myproj", "deep-learning"));
        let default = vec!["myproj".into()];
        assert_eq!(
            parse_new_args(&default).unwrap(),
            ("myproj", "classical-ml")
        );
        assert!(parse_new_args(&[]).is_err());
    }

    #[test]
    fn forge_toml_carries_name_and_profile() {
        let toml = forge_toml("house-prices", "classical-ml");
        assert!(toml.contains("name = \"house-prices\""));
        assert!(toml.contains("profile = \"classical-ml\""));
        assert!(toml.contains("schema = 1"));
    }

    #[test]
    fn forge_ide_binary_name_is_platform_correct() {
        let path = forge_ide_path().unwrap();
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("forge_ide"));
        assert_eq!(name.ends_with(".exe"), cfg!(windows));
    }
}
