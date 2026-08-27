# Forge ML Roadmap

This is the living delivery tracker for Forge ML. Update it whenever a feature
lands, a milestone changes, or a design assumption is invalidated. Detailed
product and architecture decisions live in [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md).

Legend:

- `[x]` implemented and verified
- `[~]` partially implemented or prototype quality
- `[ ]` not implemented

## Current status

Current application version: `0.25.0`

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
- [~] Arrow-backed datasets and streamed record batches (Arrow-backed storage landed; streaming ingestion remains).
- [x] Virtualized rows and columns.
- [x] Sorting, column controls, selection, editing, and linked plots.
- [ ] CSV/Parquet/Arrow/JSON file browser and importer UI.
- [x] Database connections and SQL workbench.
- [x] Object-storage connections.
- [x] General MIME and structured plot output.

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
- [~] Automatic application update channel (attested stable/beta discovery landed; installation remains intentionally manual).
- [x] Crash reporting and opt-in diagnostics.

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

## 0.3 data, Git, and Rust packages — implemented

- [x] Introduce Arrow-backed datasets.
- [x] Add virtualized table rendering.
- [x] Add sorting, column resizing, visibility, pinning, and selection.
- [x] Add CSV, TSV, Parquet, Arrow IPC, and JSON Lines ingestion.
- [x] Add an optional native Millwright `Table` adapter (`millwright` feature).
- [~] Add dataset profiles, missingness, correlations, alerts, and lineage (column statistics and source lineage landed).
- [x] Add local Git status and project-tree decorations.
- [x] Add diff, staging, commits, branch switching/creation, pull, and push.
- [x] Add crates.io search and crate detail views through Cargo.
- [x] Add/remove/update Cargo dependencies and feature flags through `cargo add` specifications.
- [x] Add dependency tree, duplicate version, audit, and license views.

Implementation notes:

- Imported data is normalized into an Arrow `RecordBatch` while retaining a display-oriented table projection.
- Large filtered datasets render only visible rows; column headers support numeric-aware sorting.
- The Git workbench uses the installed Git CLI and never stores credentials itself.
- The Crates workbench uses Cargo for registry discovery and manifest/lockfile changes.
- Millwright remains optional so the base IDE binary does not pull Polars into every build.

## 0.4 notebooks and GitHub — implemented

- [x] Introduce a shared notebook document model.
- [x] Add Markdown cells and a MIME-typed output model.
- [x] Read and write `.ipynb`.
- [x] Discover Jupyter kernelspecs and install the Evcxr kernelspec.
- [~] Run Evcxr through the Jupyter protocol where appropriate (kernelspec integration is ready; native execution remains the low-latency default).
- [x] Add GitHub authentication through `gh` with secure credential-store delegation.
- [x] Clone, fork, and publish repositories.
- [x] Add pull request, issue, and Actions views.
- [x] Record Git commit and dirty state in notebook executions.

Implementation notes:

- Markdown cells use `//# %% [markdown]` and remain valid Rust comments on disk.
- Notebook interchange preserves cell kind, source, nbformat metadata, and kernelspec name.
- GitHub operations use the installed `gh` CLI, so tokens are never persisted by Forge ML.
- Every executed code cell records the short Git commit and whether the working tree was dirty.

## 0.5 Millwright Studio — implemented

- [x] Add Forge's `TrainingObserver`/`TrainingEvent` bridge for published Millwright.
- [x] Add Forge's native adapter for published `millwright` 2.2.1.
- [x] Load Millwright CSV/Parquet tables directly into the data viewer.
- [x] Build the first visual pipeline editor.
- [x] Generate Rust pipeline code.
- [x] Display evaluation reports, confusion matrices, ROC, and residuals.
- [x] Show cross-validation, trial, fold, and epoch progress.
- [x] Add AutoML leaderboard and feature-importance views.
- [x] Add Python runtime/package discovery and environment compatibility checks.

Implementation notes:

- Forge integrates only the published crates.io package; local Millwright checkouts are ignored.
- Millwright/Polars remains behind the optional `millwright` feature to protect default binary size.
- Runtime cells can stream `forge_training:<json>` and `forge_evaluation:<json>` records into Studio.
- Generated pipelines use Millwright's Rust API and open as editable notebook cells.
- Python inspection is read-only runtime discovery; packages and ML frameworks remain user-managed.

## 0.6 experiments and provenance — implemented

- [x] Persist full run metadata and per-run JSON artifacts.
- [x] Add tags, notes, archive, clone, compare, and parent/child runs.
- [x] Capture dataset, Git, Cargo lockfile, toolchain, environment, and hardware fingerprints.
- [x] Add a serialized background training job queue.
- [x] Add active trial/fold progress, elapsed time, ETA, and worker monitoring.
- [x] Associate runs with GitHub issues, pull requests, and Actions.

Implementation notes:

- Existing 0.2–0.5 experiment JSON receives backward-compatible defaults on load.
- Every saved run writes `.forge/artifacts/runs/<run-id>/run.json` atomically.
- Cloning a run assigns a new stable ID and records the source run as its parent.
- Background commands execute one at a time from the project root and preserve captured output.
- Dataset fingerprints and Cargo lockfile fingerprints make comparisons reproducible without copying source datasets.

## 0.7 Python runtime and publishing previews — implemented

- [x] Discover Python runtimes and explicitly create project `.venv` environments.
- [x] Add Python Jupyter kernel selection and persistent console sessions.
- [x] Pass MIME-typed output from Python runtime sessions.
- [x] Persist the selected interpreter and Jupyter kernelspec.
- [x] Add PyPI JSON discovery and version compatibility views.
- [x] Detect uv, pip, Poetry, and Conda without taking ownership of user packages.
- [x] Add crates.io package validation and publish dry run.
- [~] Add Python build and smoke-test previews (uploads intentionally remain external).

Implementation notes:

- The Python console uses a persistent isolated interpreter namespace and captures stdout, stderr, and tracebacks.
- PyPI metadata is read through Python's standard-library HTTPS client; Forge stores no registry credentials.
- Creating `.venv` is explicit, and Forge never installs packages or bundles an ML framework.
- Cargo publishing remains a local dry run; Python publishing stops at build and smoke-test validation.

Scope boundary:

- [x] Do not bundle NumPy, pandas, SciPy, scikit-learn, PyTorch, TensorFlow, or CUDA.
- [x] Do not build Python-specific ML pipeline or training framework integrations.
- [x] Treat all packages beyond the runtime as user-managed project dependencies.

## 0.8 databases and coordinated releases — implemented

- [x] Add an `adbc_core` connector foundation.
- [x] Add embedded SQLite plus DuckDB and PostgreSQL CLI adapters.
- [x] Add project connection profiles, schema browser, SQL editor, and query history.
- [x] Load query results into Arrow-backed datasets.
- [x] Store database passwords in native OS credential managers.
- [x] Report Forge, published Millwright, and Python wheel versions together.
- [x] Add Python/PyPI, crates.io, checksums, provenance, and GitHub Release workflow stages.
- [x] Support PyPI Trusted Publishing through GitHub OIDC and build provenance attestations.

Implementation notes:

- Connection profile JSON never includes passwords; it stores only an opaque credential key.
- SQLite is embedded. DuckDB and PostgreSQL use their official CLIs, keeping their native libraries out of the default binary.
- Query results are normalized to the same Arrow-backed dataset path as file and Millwright imports.
- ADBC core is linked as the common API foundation; concrete driver-manager installation remains user-controlled.
- The coordinated release workflow requires explicit dispatch inputs before any registry publishing occurs.

## 0.9 deep learning and remote execution — implemented

- [x] Add published Burn project templates without linking Burn into the IDE.
- [x] Add backend/device selection and framework-neutral model summaries.
- [x] Add epoch/batch metrics, checkpoints, early stopping, and resume controls.
- [x] Monitor CPU, RAM, NVIDIA GPU, throughput, and ETA.
- [x] Add tensor, image, embedding, and prediction viewers.
- [x] Add secured remote Jupyter/training-agent profiles.
- [x] Add GitHub Actions remote training dispatch, workflows, and artifact retrieval.

Implementation notes:

- Generated projects target published Burn `0.22.0-pre.3`; Burn is not linked into the Forge binary.
- Deep-learning outputs use framework-neutral `forge_model`, `forge_tensor`, `forge_image`, `forge_embedding`, `forge_predictions`, and `forge_checkpoint` records.
- This does not restore the removed Burn-specific telemetry implementation.
- Remote tokens use the OS credential manager and never enter project profile JSON.
- GitHub Actions training is explicit, command-driven, time-bounded, and always attempts artifact upload.

## 1.0 production readiness

- [~] Complete data, notebook, experiment, and model exports (portable data, notebooks, run/project bundles, EDA, and comparison reports landed; PDF and direct model conversion remain).
- [x] Add Millwright ONNX, registry, rollback, and service generation UI.
- [~] Add database and object-storage connector hardening (bounded database previews and secured CLI-backed object profiles landed; broader driver validation remains).
- [x] Add private Cargo/Python registries and GitHub Enterprise validation.
- [~] Add signed releases, update channels, provenance, and attestations (artifact and channel-manifest attestations landed; OS code-signing identities remain release-environment work).
- [~] Complete accessibility and keyboard navigation review (keyboard shell, contrast, motion, and guidance landed; formal assistive-technology testing remains).
- [~] Complete cross-platform packaging and upgrade tests (matrix preflight landed; clean-machine upgrade runs remain).
- [x] Establish performance budgets for startup, tables, notebooks, and plots.
- [x] Publish user, extension, protocol, and contributor documentation.

## 0.10 export, registry, and deployment foundations — implemented

- [x] Export complete Arrow-backed datasets as CSV, TSV, JSON Lines, Parquet, and Arrow IPC.
- [x] Export Rust notebooks as `.ipynb`, Rust source, Markdown, and self-contained HTML.
- [x] Export experiment manifests and referenced artifacts as compressed ZIP bundles.
- [x] Add a project-local, versioned model registry with safe names and atomic metadata updates.
- [x] Add aliases for promotion and rollback without deleting model versions.
- [x] Register ONNX and other model artifacts independently of their producing framework.
- [x] Generate editable Axum inference-service projects, Dockerfiles, and Compose manifests.

Implementation notes:

- Registry artifacts live under `.forge/models`; source artifacts are copied so registry versions remain immutable.
- Millwright remains the published crates.io dependency. Forge accepts its exported artifacts but does not depend on a local checkout.
- Generated prediction endpoints are intentionally pass-through scaffolds until the user selects an inference runtime compatible with the registered format.
- PDF reports, reproducible whole-project bundles, selected-row exports, and direct Millwright ONNX API invocation remain part of 1.0 hardening.

## 0.11 storage and enterprise connections — implemented

- [x] Add project-local S3-compatible and rclone object-storage profiles.
- [x] Delegate S3/rclone authentication to their established credential chains.
- [x] Add bounded object listings and explicit downloads into `.forge/object-cache`.
- [x] Require HTTPS for remote object endpoints and private Python registries, with localhost development exceptions.
- [x] Reject traversing object keys and redact common secret assignments from CLI errors.
- [x] Add timeouts to object-storage CLI operations and cap listings at 1,000 entries.
- [x] Cap database query previews at 10,000 rows and reject empty, NUL-containing, or oversized statements.
- [x] Add named private Cargo-registry search through project Cargo configuration.
- [x] Add private PyPI-compatible JSON discovery without installing packages.
- [x] Add validated GitHub Enterprise authentication checks through `gh`.

Implementation notes:

- Forge stores no object-storage, registry, or GitHub tokens in project files.
- Cargo, Python, AWS CLI, rclone, and GitHub CLI remain the owners of authentication and trust configuration.
- Object downloads require an explicit key and are confined to the project cache.

## 0.12 trusted updates and performance budgets — implemented

- [x] Generate stable or beta update manifests from tagged release artifacts.
- [x] Include platform, HTTPS URL, byte size, and SHA-256 for every update artifact.
- [x] Attach GitHub build-provenance attestations to installers and update manifests.
- [x] Verify downloaded update manifests with `gh attestation verify` before parsing them.
- [x] Validate manifest schema, channel, version, platform, HTTPS URL, and checksum shape.
- [x] Keep update installation manual; update checks never replace a running binary.
- [x] Restrict release artifact upload patterns and fail packaging when installers are absent.
- [x] Add a packaging preflight for version alignment, platform matrix, provenance, and update assets.
- [x] Add repeatable notebook parsing, 100k-row table, and million-point plot-preparation budgets.

Implementation notes:

- GitHub attestations provide identity-bound signatures through the existing OIDC release trust chain.
- Windows Authenticode, Apple Developer ID/notarization, and Linux package signing still require project-owned signing identities.
- Current interactive budgets are 250 ms for 10k notebook cells, 350 ms for filtering/sorting 100k rows, and 500 ms for preparing one million plot points.
- Startup remains governed by the existing three-second product target; precise cold-start automation requires packaged clean-machine runners.

## 0.13 accessibility and documentation — implemented

- [x] Add a searchable global command palette with keyboard selection and execution.
- [x] Add `F6` inspector-pane cycling and a direct Variables-pane shortcut.
- [x] Persist high-contrast and reduced-motion preferences.
- [x] Disable custom caret animation when reduced motion is active.
- [x] Add visible textual status announcements for keyboard navigation and palette commands.
- [x] Document the complete keyboard workflow and accessibility settings.
- [x] Publish user, Forge protocol, extension/integration, and contributor guides.
- [x] Document credential, optional-dependency, Millwright, Burn, and Python scope boundaries.

Implementation notes:

- State continues to use labels, symbols, or counts in addition to color.
- The command catalog is centralized and tested so menu discovery can evolve without duplicating search logic.
- Formal NVDA, Narrator, VoiceOver, keyboard-only, and contrast audits remain required on packaged builds before 1.0.

## 0.14 private diagnostics and crash reports — implemented

- [x] Keep diagnostics disabled by default and persist explicit user consent.
- [x] Scope local diagnostic recording to the currently open project.
- [x] Record bounded event categories without source, data, SQL, environment, or command output.
- [x] Install a panic hook that preserves the standard hook and writes sanitized crash summaries only when opted in.
- [x] Redact home paths and common inline token, password, secret, and key assignments.
- [x] Rotate event logs at 1 MB and cap exported crash summaries.
- [x] Export a reviewable ZIP with a machine-readable privacy manifest.
- [x] Never upload diagnostics or crash reports automatically.

Implementation notes:

- Diagnostic data lives under `.forge/diagnostics` and remains entirely user-controlled.
- Disabling consent stops new recording but does not silently delete existing files.
- The export manifest lists excluded sensitive categories so a bundle can be inspected before sharing.

## 0.15 structured plots and visual analytics — implemented

- [x] Add a versioned, framework-neutral `PlotSpec` and `forge_plot:` output record.
- [x] Render line, scatter, bar, filled-area, histogram, box, and heatmap plots.
- [x] Render ML-specific ROC, precision–recall, residual, and feature-importance views.
- [x] Add per-series visibility controls and X/Y log10 transforms.
- [x] Add plot-definition JSON and standalone SVG export.
- [x] Validate finite values, series counts, total values, matrix shape, and heatmap size.
- [x] Replace a same-named runtime plot so iterative notebook execution updates rather than duplicates it.

Implementation notes:

- Structured plots remain independent of Millwright, Burn, and Python-specific packages.
- Plot payloads are limited to one million values, 128 series, and 512×512 heatmaps before entering UI state.
- Existing `forge_metric` and `forge_vector` output remains supported.
- SVG is a dependency-free portable export foundation; richer typography and multi-panel layout remain future work.

## 0.16 advanced data grid and linked plots — implemented

- [x] Add row selection and select-all across the current filtered view.
- [x] Preserve selection by source-row identity while filtering and sorting.
- [x] Add column visibility, pinning, and width controls.
- [x] Render pinned visible columns before regular visible columns.
- [x] Add explicit draft cell editing with Save and Cancel actions.
- [x] Rebuild the Arrow `RecordBatch` only after committed table edits.
- [x] Export selected rows and visible columns to CSV.
- [x] Create linked scatter plots from selected numeric X/Y columns.
- [x] Keep vector datasets read-only while allowing selection, export, and linked views.

Implementation notes:

- Cell edits affect the in-memory dataset; Forge never overwrites an imported source file implicitly.
- Hidden columns remain in the dataset and reappear without data loss when made visible.
- An empty row selection means all rows for export and linked plotting.

## 0.17 reproducible bundles and reports — implemented

- [x] Export a complete project as a deterministic, sorted ZIP archive.
- [x] Include a machine-readable manifest with file sizes and stable content digests.
- [x] Exclude Git/Forge/build/environment state, symlinks, and credential-like files.
- [x] Enforce 20,000-file, 100 MB per-file, and 500 MB total archive limits.
- [x] Generate self-contained dataset EDA reports with profiles and bounded previews.
- [x] Generate self-contained experiment-comparison reports with metrics and provenance.
- [x] Escape project-controlled HTML content and embed no remote scripts or styles.

Implementation notes:

- Project bundles intentionally omit `.forge`; export experiment bundles separately when run artifacts are required.
- Reports are offline HTML and can be printed to PDF through the operating system or browser.
- Bundle manifests make it possible to verify content without extracting or executing project code.

## 0.18 Millwright portability and model operations — implemented

- [x] Enable Millwright 2.2.1's native ONNX API behind Forge's optional `millwright` feature.
- [x] Generate editable pipeline export cells that call `ExportOnnx` and verify a load/predict round trip.
- [x] Resolve registry aliases to immutable version metadata before service generation.
- [x] Copy the selected registered artifact into the generated service instead of retaining a workspace path.
- [x] Generate health, readiness, metadata, and prediction endpoints with bounded request counters.
- [x] Generate Docker, Compose, and Kubernetes deployment/readiness templates.
- [x] Parse framework-neutral `forge_service:` and `forge_drift:` JSON records into the Deploy pane.
- [x] Show request count, error rate, optional p95 latency, and recent per-feature drift status.

Implementation notes:

- Forge consumes the published `millwright = 2.2.1` crate only; no local repository or path override is used.
- ONNX support remains optional because Millwright's ONNX/runtime dependency graph materially increases compile and binary size.
- Generated prediction handlers deliberately remain editable integration scaffolds: model formats require format-specific tensor adapters before production inference.
- The service copies the exact alias-resolved artifact, so later alias promotion or rollback cannot silently change an already generated deployment.

## 0.19 executable ONNX inference services — implemented

- [x] Generate actual ONNX prediction endpoints through the published Millwright 2.2.1 serving runtime.
- [x] Accept rectangular `f64` row batches and return model predictions instead of pass-through values.
- [x] Apply Millwright's 10,000-row/column limits, 8 MB body limit, 64-request concurrency limit, and 30-second inference timeout.
- [x] Keep health, readiness, immutable metadata, container, Compose, and Kubernetes operations around the inference router.
- [x] Identify the Millwright runtime in service metadata and generated documentation.
- [x] Keep non-ONNX formats explicit as editable adapters rather than pretending to execute an unsupported runtime.
- [x] Add a standalone generated-project compile verification in addition to template unit tests.

Implementation notes:

- The inference runtime is linked only into generated ONNX service projects, not Forge's default desktop binary.
- Generated projects depend on `millwright = 2.2.1` from crates.io with its `serve` feature; local Millwright repositories are not consulted.
- Millwright's tract-backed loader handles ordinary ONNX graphs. ONNX-ML tree operators may require a different runtime adapter, which remains explicit rather than silently falling back.

## 0.20 immutable model integrity — implemented

- [x] Record SHA-256 and byte size for every newly registered model artifact.
- [x] Make repeated registration of identical model/version bytes idempotent.
- [x] Reject attempts to overwrite an existing model version with different bytes.
- [x] Copy new artifacts through a temporary file before atomically publishing the registry path.
- [x] Verify registered hashes and sizes whenever a model version or alias is resolved.
- [x] Reject service generation when a registered artifact has been modified after registration.
- [x] Embed the copied artifact's full identity in generated service metadata.
- [x] Make generated readiness and container health checks hash the bundled model, not merely test for file existence.
- [x] Show artifact sizes and digest prefixes in the Deploy registry table.

Implementation notes:

- SHA-256 is streamed in 64 KiB chunks, so validation does not load large model artifacts into memory.
- Registry records created before 0.20 remain readable. Their historical bytes cannot be authenticated retroactively, but any newly generated service computes and pins the exact copied artifact identity.
- Immutability applies to the `(model, version)` pair; aliases remain movable so promotion and rollback continue to work.

## 0.21 database connection hardening — implemented

- [x] Validate profile names, locations, usernames, and NUL safety before storing credentials or running commands.
- [x] Reject plaintext PostgreSQL passwords embedded in URLs and keyword connection strings.
- [x] Add explicit SQLite, DuckDB, and PostgreSQL connection/version probes to the SQL inspector.
- [x] Stop DuckDB and PostgreSQL commands after a 30-second timeout.
- [x] Drain subprocess pipes concurrently to avoid blocking on full stdout or stderr buffers.
- [x] Cap retained query output at 64 MiB while still draining excess child output safely.
- [x] Cap retained database error output at 1 MiB.
- [x] Redact connection locations, OS-stored passwords, and common password assignments from CLI errors.
- [x] Preserve the existing 10,000-row preview and 1 MB SQL statement limits.

Implementation notes:

- DuckDB and PostgreSQL remain CLI integrations so their client libraries do not increase the default desktop binary.
- Passwords are passed to PostgreSQL through the child-process environment and are never placed in command arguments or saved profile JSON.
- Queries remain explicit user actions and may contain writes; Forge bounds execution but does not silently rewrite SQL semantics.
- Broader ADBC driver-manager validation remains dependent on each user-installed driver and is still tracked for 1.0 hardening.

## 0.22 object-storage transfer hardening — implemented

- [x] Add explicit S3-compatible and rclone reachability probes to the Storage inspector.
- [x] Interpret download keys relative to the configured profile prefix.
- [x] Mirror profile names and complete object keys in `.forge/object-cache` to prevent basename collisions.
- [x] Write downloads to temporary files and atomically publish or replace cache entries.
- [x] Remove partial files after failed, timed-out, or oversized transfers.
- [x] Enforce a 2 GiB per-object project-cache limit.
- [x] Stop listings after 30 seconds and downloads after 120 seconds.
- [x] Drain child stdout and stderr concurrently without unbounded memory growth.
- [x] Cap retained listing output at 4 MiB and retained error output at 1 MiB.
- [x] Redact configured endpoints and common token/password/secret assignments from errors.

Implementation notes:

- AWS CLI and rclone continue to own authentication; Forge persists no object-storage credentials.
- Cache paths preserve remote hierarchy beneath a validated profile name, and traversal/root/prefix components are rejected.
- The cache limit is deliberately conservative for interactive dataset work; larger artifacts should remain in external storage or use an explicit project import workflow.

## 0.23 two-axis data-grid virtualization — implemented

- [x] Retain the existing vertical row virtualization for filtered and sorted row indexes.
- [x] Read the persistent horizontal scroll position and calculate the intersecting column window.
- [x] Render only viewport columns plus one-column overscan on each side.
- [x] Preserve the full virtual width with leading and trailing spacers for stable horizontal scrolling.
- [x] Apply the same window to headers, read-only cells, and editable draft cells.
- [x] Preserve column visibility, pinned-first ordering, custom widths, sorting, selection, export, and linked plots.
- [x] Report rendered versus visible column counts in the viewer footer.
- [x] Add wide, empty, and small-table window tests plus a 100,000-column performance budget.

Implementation notes:

- Column-window calculation is linear in the number of visible columns but widget construction is bounded by viewport width; this removes the dominant per-frame cost for very wide tables.
- Filtering and sorting still scan materialized row strings. Streamed Arrow batches and query pushdown remain the path to million-row interactive filtering.
- Pinned columns retain their existing pinned-first ordering; frozen sticky columns require a synchronized split-grid layout and remain a separate UX enhancement.

## 0.24 revision-aware data views — implemented

- [x] Stop cloning complete `TableData` values before every docked or floating viewer frame.
- [x] Borrow Arrow-backed workspace tables directly during read-only and draft-edit rendering.
- [x] Assign a monotonic revision whenever a dataset is imported, replaced, queried, or rebuilt after edits.
- [x] Cache filtered and sorted source-row indexes by dataset revision and view criteria.
- [x] Invalidate cached indexes when filter text, sort column, sort direction, or dataset revision changes.
- [x] Bypass persistent caching during draft edits so unsaved value changes are reflected immediately.
- [x] Preserve vector viewing through a lightweight transient one-column projection.
- [x] Test dataset revision changes plus numeric/text filtering and sorting behavior.

Implementation notes:

- Steady-state table viewing now avoids both the full table clone and repeated filter/sort scan; only viewport widgets are rebuilt each frame.
- The cached index stores source-row integers, not copied row data, so selections and linked plots keep stable source identity.
- A filter or sort change still performs one full in-memory scan. Streamed Arrow ingestion and query pushdown remain future work for datasets that cannot be materialized comfortably.

## 0.25 non-blocking data integrations — implemented

- [x] Add a dedicated sequential worker for database and object-storage requests.
- [x] Move database connection tests, schema discovery, and SQL queries off the UI thread.
- [x] Move object-storage tests, listings, and downloads off the UI thread.
- [x] Return typed database tables rather than reparsing worker text in the UI.
- [x] Insert successful query/schema results into Arrow-backed workspace datasets on the UI thread.
- [x] Update query history only after successful background execution.
- [x] Deliver downloaded cache paths and bounded listing output through typed result variants.
- [x] Disable conflicting integration actions while work is active and repaint status at 100 ms intervals.
- [x] Add an asynchronous SQLite query test with bounded result waiting.

Implementation notes:

- The worker is deliberately sequential: this prevents overlapping credential prompts, competing cache writes, and unbounded external process fan-out.
- Existing database and object-storage timeouts remain the terminal guarantee for commands running inside the worker.
- Workspace mutation remains on the UI thread after result delivery, keeping egui and dataset state single-threaded.

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
