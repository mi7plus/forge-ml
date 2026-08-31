# Assemble the self-contained offline Rust runtime that Forge ML ships so that
# notebook `:dep` cells and generated projects using Millwright and Burn compile
# and run with NO network and NO user-installed toolchain. Windows counterpart of
# build-offline-bundle.sh; see that file for the output layout consumed by
# src/offline.rs. The app writes the offline cargo config at runtime (it needs an
# absolute vendor path, unknown until install time).
#
# Run on a Windows release host with rustup + the pinned toolchain
# (rust-toolchain.toml) and one network-enabled run to populate the vendor dir.
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$out = Join-Path $repoRoot "packaging\forge-runtime"
$depsManifest = Join-Path $repoRoot "packaging\offline-deps\Cargo.toml"
$sysroot = (& rustc --print sysroot).Trim()

Write-Host ">> Toolchain sysroot: $sysroot"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force -Path $out | Out-Null

# 1. Stage the toolchain (bin + lib) so rustc finds its sysroot next to bin/.
Write-Host ">> Staging toolchain into $out"
Copy-Item -Recurse -Force (Join-Path $sysroot "bin") (Join-Path $out "bin")
Copy-Item -Recurse -Force (Join-Path $sysroot "lib") (Join-Path $out "lib")

# 2. Vendor the blessed Millwright + Burn dependency closure at locked versions.
Write-Host ">> Vendoring dependencies (this needs network once)"
& cargo vendor --locked --manifest-path $depsManifest (Join-Path $out "vendor") | Out-Null

# 3. Record the toolchain version for display in the app.
(& rustc --version) | Out-File -Encoding ascii (Join-Path $out "VERSION")

Write-Host ">> Done. Offline runtime staged at $out"
