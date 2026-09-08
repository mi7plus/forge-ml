//! `forge` — one entry point for Rust ML projects, built *around* Cargo.
//!
//! It never replaces `cargo`/`rustup`; it wraps them with data-science defaults
//! and hands the environment commands to the `forge_ide` binary, which already
//! resolves the offline runtime and the `forge.toml`/`forge.lock` environment.
//! Deliberately dependency-free.

pub mod profiles;
pub mod scaffold;

use profiles::Profile;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// The `forge` command entry point. Called by the `forge` binary (shipped in the
/// forge_ide package so it installs alongside the app) and by the standalone
/// dev bin.
pub fn run() -> ExitCode {
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
        "gpu" => cmd_gpu(rest),
        "native" => cmd_native(rest),
        "python" => cmd_python(rest),
        "manage" => cmd_manage(rest),
        "reproduce" => cmd_reproduce(rest),
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
    let (name, profile_name) = parse_new_args(args)?;
    let profile = Profile::find(profile_name).ok_or_else(|| {
        format!(
            "unknown profile `{profile_name}` ({})",
            profiles::profile_names()
        )
    })?;

    status(cargo().args(["new", name]))?;
    let root = PathBuf::from(name);
    // cargo names the package after the last path component; match it for the
    // project's own identity (forge.toml, README) when `name` is a path.
    let display = root
        .file_name()
        .and_then(|component| component.to_str())
        .unwrap_or(name);
    std::fs::write(root.join("forge.toml"), forge_toml(display, profile.name))
        .map_err(|e| format!("writing forge.toml: {e}"))?;

    // Add the profile's crates with data-science feature defaults.
    for krate in profile.crates {
        status(
            cargo()
                .current_dir(&root)
                .args(["add"])
                .args(add_args(krate)),
        )?;
    }

    // Replace cargo's stub main.rs with a Forge starter, and write a forge-only
    // README, so `forge run` does something meaningful and the reader never has
    // to reach for cargo.
    std::fs::write(root.join("src").join("main.rs"), scaffold::starter_main(profile))
        .map_err(|e| format!("writing src/main.rs: {e}"))?;
    std::fs::write(root.join("README.md"), scaffold::project_readme(display, profile))
        .map_err(|e| format!("writing README.md: {e}"))?;

    println!("Created {name} (profile: {}).", profile.name);
    println!("  cd {name} && forge run     # build and run the starter");
    println!("  forge ide                  # open the studio to train and ship a model");
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

/// `forge gpu detect` — report the GPU backends detected on this machine.
/// Delegates to forge_ide, which owns the detection.
fn cmd_gpu(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("detect") => run_forge_ide(&["--gpu-detect".to_owned()]),
        Some(other) => Err(format!("unknown gpu subcommand `{other}` (detect)")),
        None => Err("usage: forge gpu detect".into()),
    }
}

/// `forge python check [dir]` — report the referenced Python bridge environment
/// (interpreter, version, manager). Forge references the env; it never manages
/// it. Delegates to forge_ide.
fn cmd_python(args: &[String]) -> Result<(), String> {
    match args.split_first() {
        Some((sub, rest)) if sub == "check" => {
            let mut forwarded = vec!["--python-check".to_owned()];
            forwarded.extend(rest.iter().cloned());
            run_forge_ide(&forwarded)
        }
        Some((other, _)) => Err(format!("unknown python subcommand `{other}` (check)")),
        None => Err("usage: forge python check [dir]".into()),
    }
}

/// `forge native <check|provide|pin>` — inspect, provide, or pin native
/// prerequisites. `check` reports status; `provide` downloads+verifies+exposes
/// the project's pinned prebuilts; `pin` prints a verified catalog entry. All
/// delegate to forge_ide.
fn cmd_native(args: &[String]) -> Result<(), String> {
    let forward = |flag: &str, rest: &[String]| {
        let mut forwarded = vec![flag.to_owned()];
        forwarded.extend(rest.iter().cloned());
        run_forge_ide(&forwarded)
    };
    match args.split_first() {
        Some((sub, rest)) if sub == "check" => forward("--native-check", rest),
        Some((sub, rest)) if sub == "provide" => forward("--native-provide", rest),
        Some((sub, rest)) if sub == "pin" => forward("--native-pin", rest),
        Some((other, _)) => Err(format!(
            "unknown native subcommand `{other}` (check | provide | pin)"
        )),
        None => Err("usage: forge native <check|provide|pin> …".into()),
    }
}

/// `forge reproduce <run-id> [dir]` — check the current environment against a
/// recorded run's provenance. Delegates to forge_ide and mirrors its exit code
/// (non-zero when the run is not reproducible here), so it composes in scripts
/// and CI without the generic "exited with N" wrapper.
fn cmd_reproduce(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: forge reproduce <run-id> [dir]".into());
    }
    let mut forwarded = vec!["--reproduce".to_owned()];
    forwarded.extend(args.iter().cloned());
    let status = Command::new(forge_ide_path()?)
        .args(&forwarded)
        .status()
        .map_err(|error| format!("launching forge_ide: {error}"))?;
    std::process::exit(status.code().unwrap_or(1));
}

/// `forge manage [dir]` — open the Forge Manager GUI on a project (the
/// Navigator-style environment/package manager). Launches detached.
fn cmd_manage(args: &[String]) -> Result<(), String> {
    let mut command = Command::new(sibling_binary("forge_manager"));
    if let Some(dir) = args.first() {
        command.arg(dir);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("launching forge_manager: {error}"))
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
    let mut command = cargo();
    command.arg(sub).args(args);
    apply_native_env(&mut command);
    status(&mut command)
}

/// Apply `.forge/native-env` (written by `forge native provide`) to a child
/// command: prepend `path` entries to `PATH` and set `env` KEY=VALUE vars, so
/// provisioned tools and libraries are visible to the build. Dependency-free
/// line parser; a no-op when the file is absent.
fn apply_native_env(command: &mut Command) {
    let Ok(text) = std::fs::read_to_string(PathBuf::from(".forge").join("native-env")) else {
        return;
    };
    let (prepend, vars) = parse_native_env(&text);
    for (key, value) in vars {
        command.env(key, value);
    }
    if !prepend.is_empty() {
        let sep = if cfg!(windows) { ";" } else { ":" };
        let existing = std::env::var("PATH").unwrap_or_default();
        let joined = if existing.is_empty() {
            prepend.join(sep)
        } else {
            format!("{}{sep}{existing}", prepend.join(sep))
        };
        command.env("PATH", joined);
    }
}

/// Parse a `.forge/native-env` file into `PATH` dirs to prepend and env vars to
/// set. Lines are `path\t<dir>` or `env\t<KEY>=<VALUE>`; anything else is ignored.
fn parse_native_env(text: &str) -> (Vec<String>, Vec<(String, String)>) {
    let mut paths = Vec::new();
    let mut vars = Vec::new();
    for line in text.lines() {
        if let Some(dir) = line.strip_prefix("path\t") {
            paths.push(dir.to_owned());
        } else if let Some(kv) = line.strip_prefix("env\t") {
            if let Some((key, value)) = kv.split_once('=') {
                vars.push((key.to_owned(), value.to_owned()));
            }
        }
    }
    (paths, vars)
}

fn run_forge_ide(args: &[String]) -> Result<(), String> {
    status(Command::new(forge_ide_path()?).args(args))
}

// ── pure helpers (unit-tested) ───────────────────────────────────────────────

/// `cargo add` arguments for one crate, applying the profile module's shared
/// data-science feature defaults.
fn add_args(krate: &str) -> Vec<String> {
    let mut args = vec![krate.to_owned()];
    let features = profiles::default_features(krate);
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
    let mut profile = profiles::DEFAULT_PROFILE;
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
    let name = name
        .ok_or_else(|| format!("usage: forge new <name> [--profile {}]", profiles::profile_names()))?;
    Ok((name, profile))
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// Locate the `forge_ide` binary. See [`sibling_binary`].
fn forge_ide_path() -> Result<PathBuf, String> {
    Ok(sibling_binary("forge_ide"))
}

/// Locate a Forge binary by stem: next to this executable first (installed
/// layout, where all Forge binaries live together), then on `PATH` (dev: `cargo`
/// puts them in target/<profile>/). `.exe` is appended on Windows.
fn sibling_binary(stem: &str) -> PathBuf {
    let exe_name = if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_owned()
    };
    if let Ok(here) = std::env::current_exe() {
        if let Some(dir) = here.parent() {
            let sibling = dir.join(&exe_name);
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from(exe_name)
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
         \x20 forge new <name> [--profile P]   scaffold a project + forge.toml (P below; default {})\n\
         \x20 forge add <crate>...             cargo add with data-science feature defaults\n\
         \x20 forge run|build|test [args]      cargo passthrough\n\
         \x20 forge env sync|doctor [dir]      write forge.lock / report the environment\n\
         \x20 forge doctor                     diagnose the current environment\n\
         \x20 forge gpu detect                 report detected GPU backends\n\
         \x20 forge native check|provide|pin  check / download+provide / pin native prerequisites\n\
         \x20 forge python check [dir]        report the referenced Python bridge environment\n\
         \x20 forge reproduce <id> [dir]       verify the environment against a recorded run\n\
         \x20 forge manage [dir]               open the Forge Manager (environment/package GUI)\n\
         \x20 forge ide [dir]                  open the Forge ML desktop app\n\
         \x20 forge version                    print the version",
        env!("CARGO_PKG_VERSION"),
        profiles::DEFAULT_PROFILE
    );
    println!("\nPROFILES:");
    for profile in profiles::PROFILES {
        println!("  {:<14} {}", profile.name, profile.summary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_known_and_deep_learning_is_curated() {
        assert!(Profile::find("data").is_some());
        assert!(Profile::find("classical-ml")
            .unwrap()
            .crates
            .contains(&"millwright"));
        assert!(Profile::find("deep-learning")
            .unwrap()
            .crates
            .contains(&"burn"));
        assert!(Profile::find("bogus").is_none());
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
    fn parse_native_env_reads_paths_and_vars() {
        let text = "path\t/opt/tools/bin\nenv\tOPENSSL_DIR=/opt/openssl\njunk line\npath\tC:\\p";
        let (paths, vars) = parse_native_env(text);
        assert_eq!(paths, ["/opt/tools/bin", "C:\\p"]);
        assert_eq!(vars, [("OPENSSL_DIR".to_owned(), "/opt/openssl".to_owned())]);
    }

    #[test]
    fn forge_ide_binary_name_is_platform_correct() {
        let path = forge_ide_path().unwrap();
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("forge_ide"));
        assert_eq!(name.ends_with(".exe"), cfg!(windows));
    }
}
