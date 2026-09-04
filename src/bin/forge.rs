//! The `forge` CLI binary. It lives in the forge_ide package (rather than only in
//! crates/forge-cli) so cargo-packager auto-detects it and installs it alongside
//! `forge_ide`, letting `forge ide`/`forge doctor` find the app as a sibling. The
//! logic lives in the dependency-free `forge_cli` library.

fn main() -> std::process::ExitCode {
    forge_cli::run()
}
