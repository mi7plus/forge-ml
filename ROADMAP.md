# Forge ML Roadmap

This is the living delivery tracker for Forge ML. Update it whenever a feature
lands, a milestone changes, or a design assumption is invalidated. Detailed
product and architecture decisions live in [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md).

Legend:

- `[x]` implemented and verified
- `[~]` partially implemented or prototype quality
- `[ ]` not implemented

## Current status

Current application version: `0.2.0`

Forge ML is a functional desktop prototype with interactive Rust execution,
editor and language tooling, project navigation, telemetry plots, experiment
snapshots, and an initial tabular data viewer. Foundation modules now isolate
protocol, storage, notebook, data, plot, experiment, and persisted UI concerns;
tables, plots, and metrics still enter through a compatibility stdout adapter.

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
- [ ] Markdown cells.
- [ ] Standard `.ipynb` import/export.
- [ ] Jupyter kernel protocol support.
- [ ] Remote kernels.

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
- [~] Table rendering is suitable for prototypes, not million-row datasets.
- [ ] Arrow-backed datasets and streamed record batches.
- [ ] Virtualized rows and columns.
- [ ] Sorting, column controls, selection, editing, and linked plots.
- [ ] CSV/Parquet/Arrow/JSON file browser and importer UI.
- [ ] Database connections and SQL workbench.
- [ ] Object-storage connections.
- [ ] General MIME and structured plot output.

### Experiments and ML

- [x] Live metric and vector collection.
- [x] Named experiment snapshots.
- [x] Experiment snapshot persistence across launches.
- [x] Metric comparison across saved runs.
- [x] Experiment CSV export.
- [ ] Project-local experiment database.
- [ ] Dataset and source fingerprints.
- [ ] Git/Cargo/environment provenance.
- [ ] Millwright integration.
- [ ] Visual classical ML pipeline builder.
- [ ] Search, cross-validation, and AutoML progress.
- [ ] Evaluation, explainability, and diagnostics dashboards.
- [ ] Burn deep-learning integration.
- [ ] Checkpoints, resource monitoring, and remote training.

### Packaging and delivery

- [x] Cross-platform release workflow scaffold.
- [x] rust-analyzer sidecar packaging design.
- [~] Windows/macOS/Linux package definitions exist but require ongoing release validation.
- [ ] Automatic application update channel.
- [ ] Crash reporting and opt-in diagnostics.

### Integrations

- [ ] Local Git status, diff, staging, commits, branches, and remotes.
- [ ] GitHub authentication, repositories, pull requests, issues, and Actions.
- [ ] crates.io discovery and Cargo dependency management UI.
- [ ] crates.io publishing assistant.
- [ ] Python environment discovery and management.
- [ ] PyPI discovery, installation, and publishing.
- [ ] Coordinated Millwright crates.io/PyPI releases.

## Now: stabilize the 0.1 prototype

- [x] Persist experiment snapshots and comparison settings.
- [x] Add rectangular table telemetry parsing and validation.
- [x] Add a filterable table viewer.
- [x] Add a separate adjustable docked dataset pane.
- [x] Fix narrow inspector tab overflow.
- [x] Fix the dataset divider interaction target and clipping.
- [x] Add a runnable dataset-viewer notebook example.
- [x] Add the complete development plan and living roadmap.
- [ ] Exercise the dataset viewer manually on Windows, macOS, and Linux.
- [ ] Add UI-level tests for docking and divider persistence where practical.
- [x] Package the dataset-viewer and planning milestone as release `0.1.5`.

## 0.2 foundation — implemented

- [x] Create a Cargo workspace with protocol and storage subsystem crates.
- [x] Extract runtime, notebook, data, plot, experiment, and persisted UI state modules.
- [x] Define stable dataset, run, plot, kernel, and artifact IDs.
- [x] Define versioned `ForgeEvent` envelopes.
- [x] Adapt existing stdout telemetry into `ForgeEvent`.
- [x] Add project-local `.forge/` storage.
- [x] Add SQLite experiment metadata and artifact directories.
- [x] Add crash-safe writes and workspace recovery.
- [x] Align Cargo, installer, displayed, and LSP client versions.
- [x] Upgrade Evcxr from 0.18 to 0.22 in isolation.
- [x] Add protocol, storage, recovery, artifact safety, and subsystem tests.

Exit criteria:

- The application restores a project without losing layout or experiment state.
- Kernel crashes do not crash the GUI.
- Major subsystems no longer depend directly on the `ForgeApp` structure.

Implementation notes:

- `.forge/workspace.sqlite3` stores project experiment metadata in WAL mode.
- `.forge/recovery.json` uses replace-with-backup writes for crash recovery.
- Project recovery includes open files, the active file, and adjustable pane layout.
- `.forge/artifacts/` rejects absolute and traversing paths.
- Runtime startup failures are automatically retried twice without terminating the GUI.
- Legacy `forge_metric`, `forge_vector`, and `forge_table` output remains compatible.

## 0.3 data, Git, and Rust packages

- [ ] Introduce Arrow-backed datasets.
- [ ] Add virtualized table rendering.
- [ ] Add sorting, column resizing, visibility, pinning, and selection.
- [ ] Add CSV, TSV, Parquet, Arrow IPC, and JSON Lines ingestion.
- [ ] Integrate Millwright `Table` and `Profile` for EDA.
- [ ] Add dataset profiles, missingness, correlations, alerts, and lineage.
- [ ] Add local Git status and project-tree decorations.
- [ ] Add diff, staging, commits, branches, pull, and push.
- [ ] Add crates.io search and crate detail views.
- [ ] Add/remove/update Cargo dependencies and feature flags.
- [ ] Add dependency tree, duplicate version, audit, and license views.

## 0.4 notebooks and GitHub

- [ ] Introduce a shared notebook document model.
- [ ] Add Markdown cells and rich MIME output.
- [ ] Read and write `.ipynb`.
- [ ] Discover and manage Jupyter kernelspecs.
- [ ] Run Evcxr through the Jupyter protocol where appropriate.
- [ ] Add GitHub authentication through `gh` with secure fallback design.
- [ ] Clone, fork, and publish repositories.
- [ ] Add pull request, issue, and Actions views.
- [ ] Record Git commit and dirty state in notebook executions.

## 0.5 Millwright Studio

- [ ] Add `TrainingObserver`/`TrainingEvent` to Millwright.
- [ ] Add Forge's Millwright adapter.
- [ ] Load Millwright tables and datasets directly into the data viewer.
- [ ] Build the first visual pipeline editor.
- [ ] Generate Rust pipeline code.
- [ ] Display evaluation reports, confusion matrices, ROC, and residuals.
- [ ] Show cross-validation and search progress.
- [ ] Add AutoML leaderboard and explainability views.
- [ ] Add Python package discovery and environment compatibility checks.

## 0.6 experiments and provenance

- [ ] Persist full run metadata and artifacts.
- [ ] Add tags, notes, archive, clone, compare, and parent/child runs.
- [ ] Capture dataset, Git, Cargo, toolchain, environment, and hardware fingerprints.
- [ ] Add training job queue and background execution.
- [ ] Add active trial/fold progress, ETA, and worker monitoring.
- [ ] Associate runs with GitHub issues, pull requests, and Actions.

## 0.7 Python runtime and publishing previews

- [ ] Discover and create Python environments.
- [ ] Add Python Jupyter kernels and console sessions.
- [ ] Pass standard Jupyter MIME output from Python kernels.
- [ ] Record the selected interpreter and environment lock state.
- [ ] Add PyPI JSON discovery and version compatibility views.
- [ ] Manage packages through uv, pip, Poetry, and Conda adapters.
- [ ] Add crates.io package validation and publish dry run.
- [ ] Add Python build, TestPyPI upload, and smoke-test workflow.

Scope boundary:

- [ ] Do not bundle NumPy, pandas, SciPy, scikit-learn, PyTorch, TensorFlow, or CUDA.
- [ ] Do not build Python-specific ML pipeline or training framework integrations.
- [ ] Treat all packages beyond the runtime as user-managed project dependencies.

## 0.8 databases and coordinated releases

- [ ] Add ADBC connector foundation.
- [ ] Add SQLite, DuckDB, and PostgreSQL.
- [ ] Add connection profiles, schema browser, SQL editor, and query history.
- [ ] Stream query results into Arrow datasets.
- [ ] Store credentials in OS credential managers.
- [ ] Coordinate Millwright crate and Python wheel versions.
- [ ] Generate Maturin, TestPyPI, PyPI, crates.io, and GitHub Release workflows.
- [ ] Support PyPI Trusted Publishing and release provenance.

## 0.9 deep learning and remote execution

- [ ] Add Burn adapter and project templates.
- [ ] Add backend/device selection and model summaries.
- [ ] Add epoch/batch metrics, checkpoints, early stopping, and resume.
- [ ] Monitor CPU, RAM, GPU, throughput, and ETA.
- [ ] Add tensor, image, embedding, and prediction viewers.
- [ ] Add remote Jupyter kernels and training agents.
- [ ] Add GitHub Actions remote training workflows and artifact retrieval.

## 1.0 production readiness

- [ ] Complete data, notebook, experiment, and model exports.
- [ ] Add Millwright ONNX, registry, rollback, and service generation UI.
- [ ] Add database and object-storage connector hardening.
- [ ] Add private Cargo/Python registries and GitHub Enterprise validation.
- [ ] Add signed releases, update channels, provenance, and attestations.
- [ ] Complete accessibility and keyboard navigation review.
- [ ] Complete cross-platform packaging and upgrade tests.
- [ ] Establish performance budgets for startup, tables, notebooks, and plots.
- [ ] Publish user, extension, protocol, and contributor documentation.

## Known technical risks

- `src/main.rs` is too large for safe parallel feature development.
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
