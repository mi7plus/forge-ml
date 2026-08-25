#!/usr/bin/env bash
set -euo pipefail

os="${RUNNER_OS:-$(uname -s)}"
arch="${RUNNER_ARCH:-$(uname -m)}"

case "$os/$arch" in
  Windows/X64|MINGW*/x86_64) target="x86_64-pc-windows-msvc"; binary="rust-analyzer.exe" ;;
  macOS/ARM64|Darwin/arm64) target="aarch64-apple-darwin"; binary="rust-analyzer" ;;
  macOS/X64|Darwin/x86_64) target="x86_64-apple-darwin"; binary="rust-analyzer" ;;
  Linux/ARM64|Linux/aarch64) target="aarch64-unknown-linux-gnu"; binary="rust-analyzer" ;;
  Linux/X64|Linux/x86_64) target="x86_64-unknown-linux-gnu"; binary="rust-analyzer" ;;
  *) echo "Unsupported release platform: $os/$arch" >&2; exit 1 ;;
esac

mkdir -p packaging/resources
curl --fail --location --retry 3 \
  "https://github.com/rust-lang/rust-analyzer/releases/latest/download/rust-analyzer-${target}.gz" \
  | gzip -dc > "packaging/resources/${binary}"
chmod +x "packaging/resources/${binary}"
"packaging/resources/${binary}" --version
