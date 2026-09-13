# CLAUDE.md

Guidance for AI coding agents (and humans) working in this repo. Forge ML is a
native Rust desktop ML IDE (egui/eframe) that ships as offline, self-contained
Windows/macOS/Linux installers. See [ARCHITECTURE.md](ARCHITECTURE.md),
[docs/FORGE_ENV.md](docs/FORGE_ENV.md), and the feature site under `site/`.

## Build, test, lint

```bash
cargo build                      # the default forge_ide binary (+ the `forge` CLI)
cargo run --bin forge_ide        # run the IDE (the package has two bins; name it)
cargo test -p forge_ide          # unit tests (267+); most modules carry #[cfg(test)]
cargo clippy --workspace --exclude forge-webview --exclude forge-cef --all-targets --all-features -- -D warnings
```

CI (`.github/workflows/ci.yml`) runs build/test/clippy `--workspace` **excluding
`forge-webview` and `forge-cef`** — those need system WebKitGTK / the CEF SDK and
can't build on the CI runners. Match that exclusion locally.

- `forge_ide` is the GUI, but it also handles a set of `--…` CLI flags early in
  `main()` before the egui viewport starts (e.g. `--env-doctor`, `--cargo-provide`).
  The thin `forge` CLI (`crates/forge-cli`) shells out to those.
- **Clippy is `-D warnings` and the tree keeps exactly one `#[allow]`** — don't add
  suppressions; fix the lint. There are ~0 TODO/FIXME markers; keep it that way.

## Workspace map

| Crate | What |
| --- | --- |
| `.` (`forge_ide`) | the IDE app; `src/` has ~78 modules. `src/main.rs` holds the app struct, CLI dispatch, and the `egui_tiles::Behavior` impl |
| `crates/forge-cli` (`forge`) | thin CLI wrapper; shells out to `forge_ide --…` |
| `crates/forge-manager` (`forge_manager`) | standalone env/package GUI; reads `forge_ide --env-status-json` |
| `crates/forge-cef` + `forge-cef-ipc` | out-of-process Chromium OSR web-preview helper + its shared-memory/command protocol |
| `crates/forge-webview` | wry/tao WebView window helper |
| `crates/forge-ml`, `forge-protocol`, `forge-storage` | umbrella crate + protocol/storage libs |

## Key subsystems

- **Environment system** (`src/environment/`): `forge.toml` manifest → providers
  (`EnvironmentProvider` trait) → `forge.lock`. Sections `[gpu]`/`[native]`/
  `[python]`/`[cargo]` are all active; `forge doctor` reports coverage,
  `forge env provide` provisions (native tools, `cargo add`, pip/pixi install),
  `forge reproduce` verifies. Providers are root-less (`&Manifest`); root-aware
  reports/provisioning are separate free functions wired through the CLI. See
  [docs/FORGE_ENV.md](docs/FORGE_ENV.md).
- **Web preview** (`src/ui/cef_preview.rs` + `crates/forge-cef*`): the IDE stays
  CEF-free — it maps a shared-memory frame buffer and spawns `forge_cef.exe`. CEF
  **cannot be built in a sandbox** (needs the ~150MB SDK + a display); write the
  code, then a human builds/runs it and reports.
- **Offline runtime** (`src/offline.rs`): bundled toolchain + vendored deps so
  notebook `:dep` cells build with no network/toolchain. Staged into the installer.

## Conventions

- **Commit and push straight to `main`** — no branches/PRs for this repo.
- End commit messages with:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`
- **`glyph_guard` test** (`src/glyph_guard.rs`) scans string *literals* for glyphs
  outside the app's bundled fonts. Use `egui_phosphor_icons::icons::*.as_str()`
  constants (not flagged) rather than pasting Unicode glyphs into literals.
- **egui/eframe are pinned to 0.36** (a future-dated API): `App::ui(&mut self, ui,
  frame)` not `update`; no `SidePanel`/`TopBottomPanel`; `smooth_scroll_delta` not
  `raw_scroll_delta`. Check the pinned source under `~/.cargo/registry` before
  guessing an API.
- `gh` is **not** installed — use the GitHub REST API via `curl` with a
  `User-Agent` header. The unauthenticated limit is 60/hr; don't tight-loop poll.

## Releases (`.github/workflows/release.yml`)

Tagging `v*` builds NSIS/DMG/DEB+AppImage via cargo-packager (~65–90 min) and
publishes a GitHub Release with `SHA256SUMS` + a signed update manifest. Bump the
7 workspace `Cargo.toml` versions + the `site/` download links first. Hard-won
release gotchas (all fixed in the workflow, but know them) live in the
`release-installers` project memory: the `release` job needs **all three** matrix
jobs; `action-gh-release` never deletes old assets, so to re-release cleanly the
human must **delete the GitHub Release before re-tagging**; the asset upload is an
explicit allowlist; the update manifest selects `*-setup.exe` for Windows and
maps the dmg's space→dot rename; and the Unix packager step retries the transient
`hdiutil: Resource busy` dmg flake.
