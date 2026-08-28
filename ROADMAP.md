# Forge ML Roadmap

This is the living delivery tracker for Forge ML. Update it whenever a feature
lands, a milestone changes, or a design assumption is invalidated. Detailed
product and architecture decisions live in [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md).

Legend:

- `[x]` implemented and verified
- `[~]` partially implemented or prototype quality
- `[ ]` not implemented

## Current status

Current application version: `0.40.0`

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
- [~] Table rendering is suitable for prototypes, not million-row datasets.
- [~] Arrow-backed datasets and streamed record batches (Arrow-backed storage landed; streaming ingestion remains).
- [x] Virtualized rows and columns.
- [x] Sorting, column controls, selection, editing, and linked plots.
- [x] CSV/Parquet/Arrow/JSON file browser and importer UI.
- [x] Database connections and SQL workbench.
- [x] Object-storage connections.
- [x] General MIME and structured plot output.

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
- [x] Add UI-level tests for docking and divider persistence where practical.
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
- [x] Add a native Millwright `Table` adapter (always embedded since 0.31).
- [x] Add dataset profiles, missingness, correlations, alerts, and lineage.
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
- Millwright was optional in this milestone; 0.31 made the published crate a core dependency.

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
- Millwright/Polars was feature-gated in this milestone; 0.31 intentionally embeds it in every build.
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

- Generated projects target published Burn `0.22.0-pre.3`; 0.31 also links Burn into the Forge binary.
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

- [x] Enable Millwright 2.2.1's native ONNX API (always embedded since 0.31).
- [x] Generate editable pipeline export cells that call `ExportOnnx` and verify a load/predict round trip.
- [x] Resolve registry aliases to immutable version metadata before service generation.
- [x] Copy the selected registered artifact into the generated service instead of retaining a workspace path.
- [x] Generate health, readiness, metadata, and prediction endpoints with bounded request counters.
- [x] Generate Docker, Compose, and Kubernetes deployment/readiness templates.
- [x] Parse framework-neutral `forge_service:` and `forge_drift:` JSON records into the Deploy pane.
- [x] Show request count, error rate, optional p95 latency, and recent per-feature drift status.

Implementation notes:

- Forge consumes the published `millwright = 2.2.1` crate only; no local repository or path override is used.
- ONNX support was optional in this milestone; 0.31 accepts the larger dependency graph and embeds it.
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

- Generated services link their own serving runtime; since 0.31 the IDE also embeds Millwright's ONNX APIs.
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

## 0.26 non-blocking local dataset imports — implemented

- [x] Add a first-class file importer to the Data inspector as well as the Tools menu.
- [x] Move CSV, TSV, JSON Lines, Parquet, and Arrow IPC parsing off the UI thread.
- [x] Deliver dataset names, tables, sources, and original paths through a typed worker result.
- [x] Build Arrow-backed workspace datasets on the UI thread after successful parsing.
- [x] Open successful imports directly in the docked data viewer.
- [x] Show an active-operation spinner and disable conflicting data operations while importing.
- [x] Reject non-files and files larger than 512 MiB before allocating parser state.
- [x] Add a bounded asynchronous CSV import test.

Implementation notes:

- The 512 MiB limit applies to the compressed/on-disk file size; expanded datasets can require substantially more memory and streaming record batches remain future work.
- Imports share the sequential integration worker with databases and object storage, preventing concurrent large allocations and external transfers.
- File selection remains a native modal dialog, while all parsing and conversion work runs in the background.

## 0.27 cryptographic experiment provenance — implemented

- [x] Replace implementation-dependent 64-bit hashes with stable SHA-256 fingerprints.
- [x] Apply SHA-256 to dataset content, Cargo lockfiles, Python runtime packages, and project-bundle entries.
- [x] Record the fingerprint algorithm explicitly in every new experiment run.
- [x] Record separate hashed dataset-source identities without exposing source paths or connection labels.
- [x] Preserve loading of historical runs through serde defaults and label their fingerprints as legacy in the UI.
- [x] Show dataset count and fingerprint algorithm in the experiment comparison pane.
- [x] Upgrade reproducible project-bundle manifests to schema 2 with an explicit digest algorithm.
- [x] Reconcile the high-level roadmap checklist with capabilities already delivered by milestones 0.3–0.9.
- [x] Verify the SHA-256 implementation against a known digest vector.

Implementation notes:

- Source fingerprints hash the recorded source identity; they support equality checks without serializing local paths into run metadata.
- Existing run and bundle fingerprints are not rewritten. Empty algorithm metadata identifies legacy experiment records.
- SHA-256 strengthens reproducibility and integrity comparison but does not by itself provide authenticity; signed release attestations remain the trust mechanism for distributed artifacts.

## 0.28 bounded dataset materialization — implemented

- [x] Enforce a common one-million-row limit across standard local dataset formats.
- [x] Enforce a common 10,000-column limit before building the workspace table.
- [x] Cap decoded header and cell text at 512 MiB in addition to the on-disk file limit.
- [x] Reject individual cells and JSON lines larger than 16 MiB.
- [x] Stream CSV and TSV records through the shared budget checker instead of collecting unchecked rows.
- [x] Discover JSON Lines schemas through a bounded first pass and materialize rows through a second streaming pass.
- [x] Decode Parquet with 8,192-row record batches to reduce compressed-data expansion peaks.
- [x] Reject incompatible Arrow record-batch schemas and propagate value-formatting errors.
- [x] Add tests for row/decoded-size rejection and late-column JSON Lines materialization.

Implementation notes:

- Limits cover the display-oriented string projection used by the current viewer. Arrow buffers, vectors, and Rust collection overhead mean process memory can exceed the decoded-text count.
- Arrow IPC files retain their encoded record-batch boundaries, so a single unusually large batch can still create a transient allocation before Forge validates its projected values.
- The published-Millwright import remains a separate native path; these limits apply to Forge's standard CSV/TSV/JSON Lines/Parquet/Arrow importer.

## 0.29 non-blocking dataset exports — implemented

- [x] Move full-table CSV, TSV, JSON Lines, Parquet, and Arrow IPC exports off the UI thread.
- [x] Hand a shallow-cloned Arrow record batch to the worker instead of cloning display strings.
- [x] Generate JSON Lines directly from Arrow values in the worker.
- [x] Write exports to unique sibling temporary files rather than directly truncating destinations.
- [x] Sync completed temporary output before publishing it.
- [x] Replace existing destinations through a backup-and-rollback sequence that works on Windows.
- [x] Remove partial temporary files after write, sync, or publication failures.
- [x] Disable conflicting Data-pane operations and show the shared activity indicator while exporting.
- [x] Add an asynchronous overwrite test that verifies output and temporary-file cleanup.

Implementation notes:

- Full-dataset exports use the sequential integration worker shared by imports, databases, and object storage, bounding concurrent I/O and memory pressure.
- Exporting only the current grid selection remains synchronous because its projected table is already bounded by explicit user selection; it still uses crash-safe destination publication.
- Destination replacement retains the previous file until the new file is complete. If final publication fails, Forge attempts to restore the backup and reports the error.

## 0.30 non-blocking native Millwright imports — implemented

- [x] Move Millwright CSV and Parquet loading off the UI thread.
- [x] Continue using only the published `millwright = 2.2.1` crates.io dependency.
- [x] Convert native Polars rows into typed Forge table results inside the worker.
- [x] Apply Forge's one-million-row, 10,000-column, 512 MiB decoded-text, and 16 MiB cell limits to the projected result.
- [x] Apply the existing 512 MiB regular-file preflight before invoking Millwright.
- [x] Insert the completed result into the Arrow-backed workspace only on the UI thread.
- [x] Open successful native imports directly in the docked dataset viewer.
- [x] Disable the Millwright import action while another integration operation is active.
- [x] Add a worker test that executes the published Millwright CSV loader.

Implementation notes:

- Millwright/Polars necessarily materializes its native table before Forge projects display strings, so Forge's decoded-data budget limits the workspace result rather than the native loader's peak allocation.
- Millwright was still optional in 0.30; 0.31 intentionally moved its Polars and ONNX graph into every desktop build.
- The standard Forge importer remains preferable when native Millwright semantics are not specifically required.

## 0.31 embedded native ML runtimes — implemented

- [x] Make published Millwright 2.2.1 a non-optional dependency of every Forge build.
- [x] Keep the historical `millwright` Cargo feature as a no-op compatibility alias.
- [x] Remove compile-time guards from native Millwright imports and their worker verification.
- [x] Embed published Burn 0.22.0-pre.3 with training and system metrics.
- [x] Embed Burn's portable Flex CPU runtime and cross-platform WGPU runtime.
- [x] Add a native Burn tensor self-test to the Deep Learning pane.
- [x] Align Forge and forge-storage on rusqlite 0.40 for Burn runtime compatibility.
- [x] Keep generated CUDA and ROCm projects available without requiring their platform SDKs in every installer.
- [x] Update extension and user documentation to make the new binary boundary explicit.
- [x] Measure and report the resulting Windows release executable size.

Implementation notes:

- Millwright and Burn are now part of the standard IDE executable; binary size and first-build time are accepted product costs.
- The optimized Windows x86-64 `forge_ide.exe` built for 0.31.0 is 118,527,488 bytes (113.04 MiB). Installer size will vary with packaging and compression.
- Burn GPU support embedded in the cross-platform package is WGPU. CUDA and ROCm require vendor SDK/runtime compatibility and remain generated-project or remote-training targets.
- Python remains runtime-only and user-managed; embedding Rust ML frameworks does not expand Forge into Python framework distribution.
- The `millwright` feature name no longer changes dependency selection, preserving existing build scripts while ensuring all builds contain Millwright.

## 0.32 docked data-viewer layout hardening — implemented

- [x] Extract the right-pane split and divider-drag calculations into a focused layout component.
- [x] Test upward/downward divider dragging and both minimum-height clamps.
- [x] Keep narrow right panes within their actual available height instead of over-allocating two minimum-size children.
- [x] Make brand-new session defaults match legacy-session restoration: docked with a 280 px dataset pane.
- [x] Test docking defaults across both fresh construction and JSON persistence round trips.

Implementation notes:

- The visual drag target remains 12 px high and keeps the vertical-resize cursor and hover treatment.
- When less than 252 px is available, the two panes share the usable height evenly; at normal sizes each pane retains a 120 px minimum.
- These deterministic layout tests cover the state and geometry boundaries practical without a platform window; cross-platform visual smoke testing remains a release activity.

## 0.33 dataset quality and correlations — implemented

- [x] Report missing percentages, numeric coverage, standard deviation, uniqueness, range, and mean per column.
- [x] Detect high missingness, constant columns, and mixed numeric/text values.
- [x] Compute Pearson correlations for usable numeric column pairs and sort by absolute strength.
- [x] Bound correlation work to 10,000 rows and 24 numeric columns.
- [x] Cache profiles and quality results once per immutable dataset revision.
- [x] Show alerts and strongest correlations in the Data inspector.
- [x] Include quality alerts and correlations in EDA HTML reports.
- [x] Preserve existing source lineage and cryptographic source fingerprints.

Implementation notes:

- Profiles are evaluated lazily, keeping dataset insertion off the profiling path, then reused on subsequent UI frames and exports.
- Dataset edits rebuild the Arrow batch with a new revision, naturally invalidating the cached profile and quality report.
- Correlations use complete finite numeric pairs from the bounded row window; constant and mixed-type columns are excluded.

## 0.34 non-blocking dataset preparation — implemented

- [x] Build Arrow datasets for local file imports on the background integration worker.
- [x] Prepare cached profiles, alerts, and bounded correlations before imported datasets reach the UI.
- [x] Apply the same background preparation to published-Millwright CSV/Parquet imports.
- [x] Apply the same background preparation to database schema and query results.
- [x] Transfer ready `Dataset` values to the UI instead of rebuilding them from display tables.
- [x] Verify standard CSV, Millwright, and SQLite worker paths return usable prepared datasets.

Implementation notes:

- Imported datasets now pay Arrow conversion and initial profiling costs on the sequential worker; inserting the completed result into the workspace is a map update on the UI thread.
- Runtime telemetry and user-committed cell edits still create revisions in-process and retain lazy cached analysis, because they do not pass through the integration worker.
- The integration worker remains sequential, bounding concurrent profiling memory while preserving deterministic result ordering.

## 0.35 secure remote Jupyter discovery — implemented

- [x] Validate remote profile names, credential keys, and Jupyter base URLs before persistence.
- [x] Require HTTPS except for explicit localhost development endpoints.
- [x] Reject usernames, passwords, query credentials, and fragments in persisted URLs.
- [x] Preserve JupyterHub user-prefix paths when resolving the kernelspec API.
- [x] Load bearer tokens only from the OS credential manager.
- [x] Keep tokens out of child-process arguments by supplying the authorization header over stdin.
- [x] Probe kernelspecs on the background integration worker with a 10-second timeout and 1 MiB response cap.
- [x] Report sorted remote kernelspec names in the Deep Learning pane.
- [x] Redact credential values from command errors and test the security boundaries.

Implementation notes:

- Forge currently uses the installed curl executable for the small Jupyter REST probe, avoiding another embedded HTTP/TLS stack while keeping output and time bounded.
- Remote session creation, Jupyter messaging authentication, WebSocket channels, interrupts, and shutdown remain necessary before remote kernels can execute notebook cells.
- Python support remains runtime-only: discovering a remote Python kernelspec does not install or distribute Python or its packages.

## 0.36 remote Jupyter kernel lifecycle — implemented

- [x] Start a named kernelspec through Jupyter's authenticated kernel REST endpoint.
- [x] Parse and validate the server-issued kernel ID and effective kernel name.
- [x] Keep the originating validated remote profile attached to the in-memory session.
- [x] Show the active remote, kernelspec, and session ID in the Deep Learning pane.
- [x] Stop the managed kernel through Jupyter's authenticated DELETE endpoint.
- [x] Disable additional Start actions while Forge owns an active remote kernel.
- [x] Run creation and shutdown on the background integration worker.
- [x] Reuse the 10-second timeout, 1 MiB response cap, HTTPS policy, OS credential store, stdin authorization header, and error redaction.

Implementation notes:

- The active session is deliberately not persisted: restarting Forge cannot safely assume a server-side kernel remains live or owned by the same client.
- Kernelspec names and server IDs are limited to 128 ASCII letters, numbers, dots, underscores, or hyphens before use in API routes.
- At the end of 0.36, lifecycle support alone was not marked as remote execution; 0.37 added authenticated WebSocket channels and correlated text execution.

## 0.37 remote Jupyter WebSocket execution — implemented

- [x] Open authenticated `ws`/`wss` Jupyter kernel channels while preserving JupyterHub base paths.
- [x] Use native-root rustls for cross-platform TLS certificate validation.
- [x] Generate Jupyter 5.3 `execute_request` messages with unique client session and message IDs.
- [x] Correlate incoming messages through the request's parent message ID.
- [x] Collect stream text, plain-text display data/results, and remote tracebacks.
- [x] Wait for both `execute_reply` and idle status before completing a run.
- [x] Enforce 1 MiB code, 2 MiB message/output, and 30-second read limits.
- [x] Execute the blocking channel workflow on the background integration worker.
- [x] Add a Deep Learning pane editor and **Run on remote kernel** action.
- [x] Test request envelopes, WebSocket endpoint construction, output handling, reply metadata, and idle completion.
- [x] Disable debug symbols and incremental caches in development/test profiles after the embedded stack's cache reached 99 GiB.

Implementation notes:

- The server handles Jupyter connection-file signing behind its WebSocket gateway; Forge authenticates the HTTPS/WSS transport with the token from the OS credential store.
- At the end of 0.37, Forge consumed legacy JSON text frames; rich MIME bundles, binary buffers, stdin prompts, interrupts, and direct notebook routing were explicit follow-up work. Version 0.38 completes MIME preservation and interrupts.
- The reduced Cargo development/test profiles reclaimed 103.8 GiB of generated artifacts on the Windows verification host; release optimization and shipping symbols are configured independently.

## 0.38 remote Jupyter interrupts and rich output — implemented

- [x] Preserve HTML, Markdown, SVG, PNG, and JSON results from Jupyter MIME bundles alongside their plain-text fallback.
- [x] Accept string arrays used by Jupyter for multiline MIME payloads and serialize structured JSON payloads.
- [x] Enforce the existing 2 MiB output budget across combined text and rich payloads.
- [x] Add an **Interrupt execution** action while a remote request is running.
- [x] Send interrupts on a dedicated control worker so they are not queued behind the blocking execution channel.
- [x] Prevent duplicate interrupt requests and expose captured rich payloads in a bounded, collapsible Deep Learning view.
- [x] Test mixed text/HTML/JSON display bundles and keep request parsing isolated from the UI.

Implementation notes:

- Interrupt uses Jupyter's authenticated `POST /api/kernels/{id}/interrupt` endpoint. The execution WebSocket remains responsible for reporting the resulting reply and idle state.
- Rich payloads are preserved for inspection without executing remote HTML or SVG in the IDE, avoiding an embedded active-content surface.
- At the end of 0.38, stdin prompts and direct notebook-cell routing remained explicit follow-up work; 0.39 completes notebook routing.

## 0.39 remote notebook execution — implemented

- [x] Add an explicit **Run notebook cells on active remote kernel** execution-target toggle.
- [x] Route Run Cell, Run Above, and Run All through the normal notebook queue to the managed Jupyter session.
- [x] Correlate background results with their originating Forge cell instead of the standalone remote editor.
- [x] Preserve text and rich MIME payloads in each cell's existing output record.
- [x] Continue queued cells after successful remote replies and stop the queue on remote errors.
- [x] Route the normal Stop action to the dedicated remote interrupt lane while a notebook cell is active.
- [x] Display non-plain MIME payloads as inert, collapsible source in the cell console.
- [x] Clear the remote execution target when Forge stops its managed kernel.

Implementation notes:

- The routing choice is deliberately session-only: Forge cannot safely restore ownership of a server-side kernel after restart.
- **Restart and run all** runs all cells in the current remote session because remote kernel restart is not yet exposed.
- At the end of 0.39, Jupyter stdin prompts remained disabled as the final protocol-level item in this roadmap slice; 0.40 completes them.

## 0.40 remote Jupyter stdin — implemented

- [x] Enable stdin in remote Jupyter execute requests.
- [x] Recognize correlated `input_request` messages without completing or decrementing the active integration task.
- [x] Present a focused IDE input dialog for standalone and notebook-cell remote execution.
- [x] Mask password requests and avoid copying prompt responses into logs, project state, or cell output.
- [x] Send Jupyter 5.3 `input_reply` messages on the stdin channel with the request header as parent metadata.
- [x] Keep the reply channel alive across multiple prompts in one execution.
- [x] Limit prompts and replies to 64 KiB and time out unanswered prompts after five minutes.
- [x] Make Cancel and Stop drop the pending input channel and interrupt the remote kernel through the independent control lane.
- [x] Test stdin enablement and the structure/correlation of generated reply messages.

Implementation notes:

- This completes the planned remote-kernel MVP. Binary Jupyter buffers and comm/widget protocols remain outside the MVP and can be added when a concrete data interchange or widget requirement needs them.
- Remote input exists only in memory for the duration of the request; password values use the same bounded channel but are never displayed after submission.

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
