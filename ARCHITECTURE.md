# Forge ML architecture

A map of the codebase for contributors. Forge ML is a single `egui`/`eframe`
desktop binary (`forge_ide`) plus a set of workspace crates (a thin CLI, a
companion GUI, shared libs, and out-of-process helpers). Long-running work
(compilation, LSP, IO, training) runs off the UI thread and reports back through
channels; the UI thread only paints. The IDE also spawns sibling helper
processes for things it deliberately keeps out of its own address space (a
system WebView, an embedded Chromium) and talks to them over pipes/shared memory.

## Workspace layout

```
forge-ml/
├── src/                     the forge_ide binary (app shell + all subsystems)
│   ├── ui/                  egui view code (panes, editor, grids, menus, theme)
│   └── environment/         forge.toml / forge.lock / provider system
├── crates/
│   ├── forge-protocol/      the forge_* stdout event types + TableData (shared)
│   ├── forge-storage/       workspace persistence & recovery (SQLite-backed)
│   ├── forge-cli/           the `forge` CLI (scaffold/add/run/env/doctor/ide)
│   ├── forge-ml/            umbrella crate re-exporting the curated ML stack
│   ├── forge-manager/       standalone env/package GUI (reads --env-status-json)
│   ├── forge-webview/       wry/tao WebView window helper (macOS/Linux preview)
│   ├── forge-cef-ipc/       shared-memory + command protocol for the CEF helper
│   └── forge-cef/           out-of-process Chromium offscreen web-preview helper
├── examples/                runnable examples + //# %% notebooks
├── packaging/               offline-runtime bundle scripts, icons, rasterizer
├── site/                    the GitHub Pages marketing/guide site
└── docs/                    guides, protocol, environment design (FORGE_ENV.md)
```

## Layers

### App shell & state
- **`main.rs`** — the `ForgeApp` struct and its core methods, the frame loop,
  splash/icon, and the Windows console handling. *(Large; extraction into
  `app_*`/`ui`/`cli` modules is ongoing — the `eframe::App` and
  `egui_tiles::Behavior` impls live in `ui/app_impl.rs`.)*
- **`cli.rs`** — the early-`main()` `--env-*` / `--cargo-*` / `--python-*` /
  `--native-*` / `--reproduce` / `--notebook-selftest` flag dispatch, run before
  the egui viewport starts (the `forge` CLI shells out to these).
- **`app_files.rs`** — `ForgeApp` methods for the project/file/editor-tab
  lifecycle (open/create/delete/save, Cargo/clippy/format, tab close-guards,
  jump-to navigation), split out of `main.rs`.
- **`app_exec.rs`** — `ForgeApp` execution orchestration: enqueueing/running
  cells, the Rust and Python consoles, variable inspection, Python-runtime
  discovery, and background Cargo diagnostics.
- **`app_lsp.rs`** — `ForgeApp` language-server integration: document sync,
  on-demand LSP requests, rename, workspace edits, and the definition probe.
- **`session.rs`** — the serialized `SessionState` (theme, layout, recents,
  settings) persisted by eframe.
- **`workspace.rs`** — workspace snapshots for the dockable pane layout tree
  (`egui_tiles`).
- **`keymap.rs`** — customizable keyboard shortcuts.

### Execution & runtime
- **`runtime.rs`** — the notebook/console runtime: an Evcxr `CommandContext` on a
  worker thread, with concurrent stdout/stderr draining and `forge_*` event
  parsing. Activates the offline environment before spawning anything.
- **`rust_kernel.rs` / `python_kernel.rs` / `python_runtime.rs`** — additional
  REPL kernels and the optional managed Python runtime.
- **`terminal.rs`** — embedded PTY terminals.
- **`jobs.rs` / `integration_worker.rs` / `performance.rs`** — the background job
  queue, the DB/object-storage worker thread, and perf instrumentation.

### Language tooling
- **`lsp.rs`** — the rust-analyzer client (document sync, diagnostics, hover,
  completion, go-to-def, rename, code actions) with an enable/disable toggle.
- **`diagnostics.rs`** — background `cargo` diagnostics for the Problems pane.
- **`notebook.rs`** — `//# %%` cell parsing and manipulation.

### Machine learning
- **`classification.rs`** — native softmax (multinomial logistic) regression +
  metrics/confusion matrix (pure, deterministic, fully tested).
- **`deep_learning.rs`** — Burn training paths.
- **`millwright_studio.rs`** — the Millwright pipeline designer + in-process
  training (compiled-in `smartcore`/`linfa` backends) and training telemetry.
- **`model_registry.rs` / `service_monitor.rs`** — versioned model registry with
  SHA-256 provenance, generated inference services, and drift/latency monitoring.
- **`prep.rs`** — dataset preparation (encoding, imputation, scaling).

### Data & IO
- **`data.rs`** — dataset import (CSV/TSV/JSONL/Parquet/Arrow) and the `Dataset`
  type.
- **`database.rs` / `object_storage.rs`** — read-only SQL workbench and
  S3/rclone object storage (credential-safe, via the integration worker).
- **`export.rs`** — CSV/HTML/PDF/ZIP exports for datasets, reports, and bundles.
- **`plot.rs`** — the versioned `forge_plot:` spec and plot data model.
- **`jupyter.rs` / `remote.rs`** — remote Jupyter kernel integration.

### Environment & distribution
- **`environment/`** — the `forge.toml` manifest (`manifest.rs`), the generated
  `forge.lock` (`lock.rs`), the `EnvironmentProvider` trait + `Resolver`
  (`provider.rs`, `mod.rs`), and the providers themselves: `bundled.rs` (offline
  runtime), `native.rs` (system tools), `python.rs` (pip/pixi packages),
  `cargo.rs` (crate deps), and `gpu.rs` (accelerator detection). Root-aware
  reporting/provisioning (`report`/`provide`) and coverage `diagnostics.rs` /
  `status.rs` / `provision.rs` / `system.rs` wire these through the CLI. See
  [docs/FORGE_ENV.md](docs/FORGE_ENV.md).
- **`offline.rs`** — locating and describing the bundled offline Rust runtime
  (toolchain + vendored crate cache) that the bundled provider activates.
- **`helpers.rs`** — locates the sibling helper binaries (`forge_manager`,
  `forge_webview`, `forge_cef`) across the install layouts.

### Web preview
Two platform-specific backends render HTML/CSS/JS previews, both as **separate
processes** so the IDE itself stays free of a WebView/Chromium dependency:
- **`ui/cef_preview.rs` + `crates/forge-cef*`** — the dockable **in-IDE** preview
  (Windows today): the IDE maps a shared-memory framebuffer (`forge-cef-ipc`) and
  spawns `forge_cef`, an out-of-process Chromium offscreen renderer that streams
  BGRA frames back and takes input over its stdin command channel.
- **`crates/forge-webview` + `ui/preview.rs::open_in_forge_webview`** — a
  **separate WebView window** (macOS WKWebView / Linux WebKitGTK) for the
  "Rendered preview" action where CEF isn't the path. Complementary, not
  redundant: CEF is the embedded Windows surface; `forge_webview` is the
  windowed Unix surface. `ui/preview.rs` also has a dependency-free structural
  preview that always works.

### Project, VCS & release
- **`project.rs`** — the open Cargo project model and file tree.
- **`git.rs` / `github.rs`** — VCS operations and clone/PR helpers.
- **`packages.rs` / `publishing.rs`** — crate search and packaging/publishing.
- **`release.rs` / `updater.rs` / `commands.rs`** — release-workflow generation
  and packaging preflight, update-channel checks, and command dispatch.
- **`privacy_diagnostics.rs` / `experiment.rs`** — opt-in diagnostics and the
  experiment/run tracker with dataset/lockfile provenance.

### UI (`src/ui/`)
View code only, driven by the state above. **`panes.rs`/`dock.rs`** host the
dockable layout — including the **Notebook pane** (`panes.rs::notebook_pane`),
which stacks cells with their output (text, MIME, plots, and dataset previews)
inline and supports in-place editing; **`editor.rs`/`editor_pane.rs`/`editing.rs`** the code editor;
**`data_view.rs`/`grid.rs`** the virtualized data grid; **`plotting.rs`** the
plots; **`ml_lab.rs`** the ML/training surfaces; **`services.rs`/`scm.rs`** the
deployment and source-control panes; **`menus.rs`/`shortcuts.rs`** menus and the
command palette; **`notebook_io.rs`** notebook load/save; **`theme.rs`** the
theming tokens and built-in themes.

## Crates
- **`forge-protocol`** — the `forge_metric:` / `forge_vector:` / `forge_table:` /
  `forge_plot:` marker types and `TableData`. Shared so the parser and any
  producer agree on the wire format. See [docs/PROTOCOL.md](docs/PROTOCOL.md).
- **`forge-storage`** — durable workspace state and crash recovery
  (`WorkspaceStore` / `WorkspaceRecovery`), backed by bundled SQLite.
- **`forge-cli`** — the `forge` binary: a thin, dependency-free wrapper over
  `cargo`/`rustup`/`forge_ide` (`forge new/add/run/env/doctor/ide`). It delegates
  environment work to `forge_ide --env-*`, so it never duplicates the resolver.
- **`forge_ml`** — an umbrella library re-exporting the curated stack (ndarray +
  millwright by default, burn behind `deep-learning`) behind one `prelude`.
- **`forge-manager`** — a standalone egui GUI for the environment/package system;
  it reads the IDE's `forge_ide --env-status-json` snapshot rather than linking
  the resolver.
- **`forge-cef-ipc`** — the dependency-free shared-memory frame layout and stdin
  command protocol between the IDE and the `forge_cef` helper (no `cef` linkage).
- **`forge-webview` / `forge-cef`** — the two web-preview helper binaries above.
  Both need external SDKs (WebKitGTK / the CEF Chromium download), so they are
  **excluded** from the default workspace build/lint and covered separately (see
  the `helper-crates` CI job).

## Conventions
- The UI thread never blocks: heavy work goes to a worker and returns via a
  channel; panes poll results each frame.
- Data reaches the viewer/plots only through the `forge_*` stdout protocol
  (`forge-protocol`), never by direct coupling to the runtime.
- Tests live beside their code (`#[cfg(test)] mod tests`); pure modules
  (classification, prep, protocol, environment) are exhaustively unit-tested.
- CI gates `cargo fmt --all --check` and `cargo clippy --workspace --exclude
  forge-webview --exclude forge-cef --all-targets --all-features -- -D warnings`;
  keep both clean. `just ci` reproduces the gate locally.
