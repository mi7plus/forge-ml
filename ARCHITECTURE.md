# Forge ML architecture

A map of the codebase for contributors. Forge ML is a single `egui`/`eframe`
desktop binary (`forge_ide`) plus two workspace crates. Long-running work
(compilation, LSP, IO, training) runs off the UI thread and reports back through
channels; the UI thread only paints.

## Workspace layout

```
forge-ml/
├── src/                     the forge_ide binary (app shell + all subsystems)
│   ├── ui/                  egui view code (panes, editor, grids, menus, theme)
│   └── environment/         forge.toml / forge.lock / provider system
├── crates/
│   ├── forge-protocol/      the forge_* stdout event types + TableData (shared)
│   └── forge-storage/       workspace persistence & recovery (SQLite-backed)
├── examples/                runnable examples + //# %% notebooks
├── packaging/               offline-runtime bundle scripts, icons, rasterizer
├── site/                    the GitHub Pages marketing/guide site
└── docs/                    guides, protocol, environment design (FORGE_ENV.md)
```

## Layers

### App shell & state
- **`main.rs`** — `ForgeApp` (the `eframe::App`), the frame loop, splash/icon,
  the Windows console handling, and the `--env-*` / `--notebook-selftest` CLI
  flags. *(Large; a target for extraction — see ROADMAP.)*
- **`session.rs`** — the serialized `SessionState` (theme, layout, recents,
  settings) persisted by eframe.
- **`workspace.rs` / `pane_layout.rs`** — workspace snapshots and the dockable
  pane layout tree (`egui_tiles`).
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
  (`provider.rs`, `mod.rs`), and the one shipped provider (`bundled.rs`). See
  [docs/FORGE_ENV.md](docs/FORGE_ENV.md).
- **`offline.rs`** — locating and describing the bundled offline Rust runtime
  (toolchain + vendored crate cache) that the bundled provider activates.

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
dockable layout; **`editor.rs`/`editor_pane.rs`/`editing.rs`** the code editor;
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

## Conventions
- The UI thread never blocks: heavy work goes to a worker and returns via a
  channel; panes poll results each frame.
- Data reaches the viewer/plots only through the `forge_*` stdout protocol
  (`forge-protocol`), never by direct coupling to the runtime.
- Tests live beside their code (`#[cfg(test)] mod tests`); pure modules
  (classification, prep, protocol, environment) are exhaustively unit-tested.
- CI gates `cargo fmt --all --check` and `cargo clippy --workspace --all-targets
  -- -D warnings`; keep both clean.
