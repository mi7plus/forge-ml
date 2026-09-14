# Forge ML task runner. `just <recipe>` — run `just` alone to list recipes.
#
# These recipes encode the exact invocations CI runs (see .github/workflows/),
# including the `forge-webview`/`forge-cef` workspace exclusion (those need
# system WebKitGTK / the CEF SDK and can't build on the CI runners). Run `just
# ci` before pushing to reproduce the gate locally.

# The workspace minus the two helper crates that need external SDKs.
workspace := "--workspace --exclude forge-webview --exclude forge-cef"

# List available recipes (default).
default:
    @just --list

# Build the whole (buildable) workspace plus examples, locked to Cargo.lock.
build:
    cargo build {{workspace}} --examples --locked

# Run the IDE (the package ships two bins, so name it).
run:
    cargo run --bin forge_ide

# Run the unit tests.
test:
    cargo test {{workspace}} --locked

# rustfmt + clippy exactly as the CI `lint` job does (clippy is -D warnings).
lint:
    cargo fmt --all --check
    cargo clippy {{workspace}} --all-targets --all-features -- -D warnings

# Auto-format the tree.
fmt:
    cargo fmt --all

# The full local gate: format check, clippy, build, test. Run before pushing.
ci: lint build test

# Clippy the helper crates that CI only lints on Windows (needs their SDKs).
lint-helpers:
    cargo clippy -p forge-webview --all-targets -- -D warnings
    cargo clippy -p forge-cef --all-targets -- -D warnings

# Security + license/dependency audit (needs cargo-audit and cargo-deny).
audit:
    cargo audit
    cargo deny check
