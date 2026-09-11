#!/usr/bin/env bash
# Stage Forge's sibling helper binaries into packaging/helpers so `cargo
# packager` bundles them. Run before packaging, like build-offline-bundle.sh.
#
# CEF (the web preview's forge_cef helper) is Windows-only for now and handled
# by stage-helpers.ps1. On Unix this stages the environment-manager GUI, and
# the WebView preview window where the platform can build it: macOS uses
# WKWebView (always available); Linux uses WebKitGTK (only if its dev packages
# are installed on the build host).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
helpers="$here/helpers"

rm -rf "$helpers"
mkdir -p "$helpers"

crates="forge-manager"
case "$(uname -s)" in
    Darwin)
        crates="$crates forge-webview"
        ;;
    *)
        if pkg-config --exists webkit2gtk-4.1 2>/dev/null || pkg-config --exists webkit2gtk-4.0 2>/dev/null; then
            crates="$crates forge-webview"
        else
            echo "WebKitGTK dev packages not found; skipping forge_webview on this host."
        fi
        ;;
esac

pkg_args=""
for c in $crates; do
    pkg_args="$pkg_args -p $c"
done

# shellcheck disable=SC2086
(cd "$root" && cargo build --release $pkg_args)

for c in $crates; do
    bin="${c/forge-/forge_}" # crate name -> binary name (forge-manager -> forge_manager)
    cp "$root/target/release/$bin" "$helpers/$bin"
done

echo "Staged helpers:"
ls -la "$helpers"
