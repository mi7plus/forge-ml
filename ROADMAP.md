# Forge ML Roadmap

The **forward-looking** delivery tracker for Forge ML — vision, current status, a
capability snapshot, the path to `1.0`, and known risks. Update it when a
milestone changes or a design assumption is invalidated.

- The milestone-by-milestone build log (every `0.x — implemented` section that
  used to live here) now lives in [docs/HISTORY.md](docs/HISTORY.md).
- The release-by-release history is in [CHANGELOG.md](CHANGELOG.md).
- The code map is in [ARCHITECTURE.md](ARCHITECTURE.md); the
  environment/distribution design is in [docs/FORGE_ENV.md](docs/FORGE_ENV.md).

Legend:

- `[x]` implemented and verified
- `[~]` partially implemented or prototype quality
- `[ ]` not implemented

## Vision

Forge ML is a Rust-first scientific and machine-learning IDE — a desktop
environment comparable to Spyder/JupyterLab, but built **around Cargo** rather
than replacing it. It supports five connected workflows: explore data (files,
databases, object storage); develop interactively in Rust notebooks or Cargo
projects; build classical ML pipelines with Millwright; train and monitor
deep-learning models with Burn; and export notebooks, reports, datasets,
experiments, and deployable models. Python support stops at a bridge (execute +
managed runtime, ONNX/Arrow interchange) — Forge never reimplements a Python
scientific stack. The longer-term "Forge distribution" direction (a curated,
offline, reproducible environment with a thin `forge` CLI) is designed in
[docs/FORGE_ENV.md](docs/FORGE_ENV.md).

## Current status

Current application version: `1.15.0`

Forge ML is a shipping desktop IDE with interactive Rust execution, editor and
language tooling, project navigation, telemetry plots, experiment snapshots, a
virtualized tabular data viewer, embedded Millwright/Burn training, native model
deployment, and a self-contained offline runtime bundle. It publishes native
installers for Windows, macOS, and Linux. Foundation modules isolate protocol,
storage, notebook, data, plot, experiment, environment, and persisted UI
concerns; tables, plots, and metrics enter through the `forge_*` stdout adapter.

Since the 1.x line it has grown a small **ecosystem**: a `forge.toml`/`forge.lock`
environment system whose `[gpu]`/`[native]`/`[cargo]`/`[python]` providers are all
active and provisioned by `forge env provide` (`forge doctor`/`reproduce` verify);
a companion **Forge Manager** GUI over that system; and a dockable in-app **web
preview** (real Chromium, offscreen). See the [environment design](docs/FORGE_ENV.md).

## Implemented prototype capabilities

### Workspace and editor

- [x] Desktop egui/eframe application.
- [x] Cargo project opening and recent-project history.
- [x] Project file tree and Rust source outline.
- [x] Multiple editor tabs with dirty-state protection.
- [x] Save, new file, close, find/replace, undo, and redo workflows.
- [x] External file-change detection.
- [x] Project-wide search with navigation.
- [x] Persistent theme, editor font size, caret, window, and pane settings.
- [x] Adjustable left project/notebook split.
- [x] Responsive wrapped inspector tabs for narrow right panes.

### Rust execution and notebooks

- [x] Isolated persistent Evcxr runtime.
- [x] `//# %%` notebook cell parsing and navigation.
- [x] Run cell, run above, run all, restart-and-run-all, and stop execution.
- [x] Per-cell state, output, errors, and execution time.
- [x] Shared state between notebook cells.
- [x] Runtime reset without restarting the application.
- [x] Interactive Rust console with command history.
- [~] Runtime cancellation currently restarts the runtime and loses live state.
- [x] Markdown cells.
- [x] Standard `.ipynb` import/export.
- [~] Jupyter kernel protocol support (kernelspec integration is available; native Evcxr remains the default).
- [x] Remote kernel MVP (secured discovery, lifecycle, bounded rich execution, responsive interrupts, notebook routing, and stdin prompts).

### Rust language intelligence

- [x] rust-analyzer lifecycle and document synchronization.
- [x] Diagnostics and inline diagnostic underlines.
- [x] Completion popup and insertion.
- [x] Hover documentation.
- [x] Ctrl-click definition navigation across files.
- [x] Background Cargo diagnostics in Problems.
- [~] Notebook source wrapping for rust-analyzer.

### Data and plots

- [x] Numeric metric telemetry through `forge_metric`.
- [x] Numeric vector telemetry through `forge_vector`.
- [x] Rectangular table telemetry through `forge_table` JSON.
- [x] Dataset previews in the Data inspector.
- [x] Filterable two-dimensional data viewer.
- [x] Dataset viewer as a separate adjustable bottom-right pane.
- [x] Dataset viewer dock/undock workflow and persisted pane height.
- [x] Vector line/bar visualizations.
- [x] Dataset and plot deletion.
- [x] CSV telemetry export.
- [~] Million-row table workflows (two-axis virtualization and background filter/sort indexing landed; row-oriented compatibility storage still limits memory efficiency).
- [~] Arrow-backed datasets and streamed record batches (bounded chunked storage and streaming exports landed; incremental UI ingestion remains).
- [x] Virtualized rows and columns.
- [x] Sorting, column controls, selection, editing, and linked plots.
- [x] CSV/Parquet/Arrow/JSON file browser and importer UI.
- [x] Database connections and SQL workbench.
- [x] Object-storage connections.
- [x] General MIME and structured plot output.
- [x] Portable native PNG export for structured plots.
- [x] Self-contained interactive HTML export for structured plots.
- [x] Standalone vector PDF export for structured plots.

### Experiments and ML

- [x] Live metric and vector collection.
- [x] Named experiment snapshots.
- [x] Experiment snapshot persistence across launches.
- [x] Metric comparison across saved runs.
- [x] Experiment CSV export.
- [x] Project-local experiment database.
- [x] Dataset and source fingerprints.
- [x] Git/Cargo/environment provenance.
- [x] Millwright integration.
- [x] Visual classical ML pipeline builder.
- [x] Search, cross-validation, and AutoML progress.
- [x] Evaluation, explainability, and diagnostics dashboards.
- [x] Burn deep-learning project integration.
- [x] Checkpoints, resource monitoring, and remote training.

### Packaging and delivery

- [x] Cross-platform release workflow scaffold.
- [x] rust-analyzer sidecar packaging design.
- [~] Windows/macOS/Linux package definitions exist but require ongoing release validation.
- [~] Automatic application update channel (attested stable/beta discovery landed; installation remains intentionally manual).
- [x] Crash reporting and opt-in diagnostics.

### Integrations

- [x] Local Git status, diff, staging, commits, branches, and remotes.
- [x] GitHub authentication, repositories, pull requests, issues, and Actions.
- [x] crates.io discovery and Cargo dependency management UI.
- [x] crates.io publishing assistant with explicit dry runs.
- [x] Python runtime/environment discovery and explicit `.venv` creation.
- [~] PyPI discovery, installation, and publishing (discovery/build validation landed; packages remain user-managed and uploads stay external).
- [x] Coordinated Millwright crates.io/PyPI release workflow.


## Path to 1.0

Remaining work to call the product `1.0`. Items marked `[~]` are partially
landed; the note records what is done and what remains.


- [~] Complete data, notebook, experiment, and model exports (portable data, notebooks, run/project bundles, HTML/PDF EDA and comparison reports landed; direct model conversion remains).
- [x] Add Millwright ONNX, registry, rollback, and service generation UI.
- [~] Add database and object-storage connector hardening (bounded SQLite, DuckDB, PostgreSQL, and MySQL paths plus secured CLI-backed object profiles landed; broader driver validation remains).
- [x] Add private Cargo/Python registries and GitHub Enterprise validation.
- [~] Add signed releases, update channels, provenance, and attestations (artifact and channel-manifest attestations landed; OS code-signing identities remain release-environment work).
- [~] Complete accessibility and keyboard navigation review (keyboard shell, contrast, motion, and guidance landed; formal assistive-technology testing remains).
- [~] Complete cross-platform packaging and upgrade tests (matrix preflight landed; clean-machine upgrade runs remain).
- [x] Establish performance budgets for startup, tables, notebooks, and plots.
- [x] Publish user, extension, protocol, and contributor documentation.


## Known technical risks

- **Transitive `quick-xml` DoS advisory (tracked).** RUSTSEC-2026-0194/0195
  (quadratic attribute scan / unbounded namespace allocation) affect
  `quick-xml < 0.41`, pinned four levels deep: `millwright 2.2.1 → polars =0.55.2
  → polars-io → object_store 0.13.2` (requires `quick-xml ^0.39`, which excludes
  0.41). Not driven by the app (polars cloud IO; Forge's own object storage is
  `src/object_storage.rs`). Acknowledged in `.cargo/audit.toml`; **re-check and
  drop the ignore when Millwright ships a polars with a newer `object_store`.**
  Same for the unmaintained `bincode`/`paste`/`ttf-parser` warnings (via
  egui/burn/wgpu), which clear when those upstreams move off them.
- `src/main.rs` is large; extraction into `app_*`/`ui`/`cli` modules is ongoing
  (`app_files.rs`, `app_exec.rs`, `app_lsp.rs`, `cli.rs`, and the `eframe::App` /
  `egui_tiles::Behavior` impls in `ui/app_impl.rs` are done; the `ForgeApp`
  struct still carries many fields that could group into sub-state structs).
- Evcxr compilation latency and cancellation semantics require careful UX.
- Arrow/Polars version alignment will affect Forge, Millwright, ADBC, and Python interchange.
- Large egui tables require virtualization rather than regular grids.
- Python distribution and native wheels vary substantially by platform.
- Deep-learning backends can add large build and packaging footprints.
- Database drivers and cloud credentials expand the security surface.
- Package publishing is irreversible and must never be triggered implicitly.
- Git operations must preserve user work and avoid destructive defaults.

## Roadmap maintenance rules

When completing a roadmap item:

1. Mark it `[x]` only after implementation and proportionate verification.
2. Use `[~]` when only the prototype or one platform/backend is supported.
3. Add important implementation notes beneath the item when future work depends on them.
4. Update `README.md` when the capability is user-facing.
5. Add or update tests before moving a milestone to complete.
6. Record deferred work explicitly instead of silently narrowing the requirement.
