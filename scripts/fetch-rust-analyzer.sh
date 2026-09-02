#!/usr/bin/env bash
# Download the official rust-analyzer sidecar for the current release platform
# into packaging/resources/, where Packager.toml picks it up as a bundled
# resource. Run once per platform on the release runner (see release.yml).
set -euo pipefail

os="${RUNNER_OS:-$(uname -s)}"
arch="${RUNNER_ARCH:-$(uname -m)}"

# `ext` differs by platform: rust-analyzer ships the Windows builds as a .zip
# (containing rust-analyzer.exe + a .pdb) and every other platform as a bare
# gzipped binary.
case "$os/$arch" in
  Windows/X64|MINGW*/x86_64)   target="x86_64-pc-windows-msvc";   binary="rust-analyzer.exe"; ext="zip" ;;
  Windows/ARM64)               target="aarch64-pc-windows-msvc";  binary="rust-analyzer.exe"; ext="zip" ;;
  macOS/ARM64|Darwin/arm64)    target="aarch64-apple-darwin";     binary="rust-analyzer";     ext="gz" ;;
  macOS/X64|Darwin/x86_64)     target="x86_64-apple-darwin";      binary="rust-analyzer";     ext="gz" ;;
  Linux/ARM64|Linux/aarch64)   target="aarch64-unknown-linux-gnu"; binary="rust-analyzer";    ext="gz" ;;
  Linux/X64|Linux/x86_64)      target="x86_64-unknown-linux-gnu"; binary="rust-analyzer";     ext="gz" ;;
  *) echo "Unsupported release platform: $os/$arch" >&2; exit 1 ;;
esac

mkdir -p packaging/resources
dest="packaging/resources/${binary}"
url="https://github.com/rust-lang/rust-analyzer/releases/latest/download/rust-analyzer-${target}.${ext}"
echo ">> Downloading $url"

if [ "$ext" = "zip" ]; then
  tmp="$(mktemp -d)"
  curl --fail --location --retry 3 --output "$tmp/ra.zip" "$url"
  # The archive holds rust-analyzer.exe at its root (alongside a .pdb we drop).
  unzip -o -j "$tmp/ra.zip" "rust-analyzer.exe" -d packaging/resources >/dev/null
  rm -rf "$tmp"
else
  curl --fail --location --retry 3 "$url" | gzip -dc > "$dest"
fi

chmod +x "$dest"
"$dest" --version
