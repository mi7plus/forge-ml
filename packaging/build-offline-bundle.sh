#!/usr/bin/env bash
# Assemble the self-contained offline Rust runtime that Forge ML ships so that
# notebook `:dep` cells and generated projects using Millwright and Burn compile
# and run with NO network and NO user-installed toolchain.
#
# Output layout (consumed by src/offline.rs):
#
#   packaging/forge-runtime/
#     bin/            cargo, rustc, rustdoc, ...   (the pinned toolchain)
#     lib/            rustlib sysroot for the above
#     vendor/         every crate in packaging/offline-deps' locked closure
#     VERSION         `rustc --version`, shown in the app
#
# The app writes the offline cargo config itself at runtime (into a writable
# per-user CARGO_HOME) because cargo needs an ABSOLUTE vendor path, which isn't
# known until install time.
#
# Run on each release platform (Linux/macOS); see build-offline-bundle.ps1 for
# Windows. Requires rustup + the pinned toolchain (rust-toolchain.toml) and one
# network-enabled run to populate the vendor directory.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
out="$repo_root/packaging/forge-runtime"
deps_manifest="$repo_root/packaging/offline-deps/Cargo.toml"
sysroot="$(rustc --print sysroot)"

echo ">> Toolchain sysroot: $sysroot"
rm -rf "$out"
mkdir -p "$out"

# 1. Stage the toolchain (bin + lib/rustlib) so rustc finds its sysroot relative
#    to bin/, exactly as in a normal install.
echo ">> Staging toolchain into $out"
cp -a "$sysroot/bin" "$out/bin"
cp -a "$sysroot/lib" "$out/lib"

# 2. Vendor the blessed Millwright + Burn dependency closure at locked versions.
echo ">> Vendoring dependencies (this needs network once)"
cargo vendor --locked --manifest-path "$deps_manifest" "$out/vendor" > /dev/null

# 3. Record the toolchain version for display in the app.
rustc --version > "$out/VERSION"

echo ">> Done. Offline runtime staged at $out"
du -sh "$out" 2>/dev/null || true
