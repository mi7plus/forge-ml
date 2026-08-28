# Forge ML Development Plan

Forge ML is intended to become a Rust-first scientific and machine-learning IDE:
an environment comparable to Spyder and JupyterLab, but designed around Cargo,
Rust-native data tooling, reproducible experiments, and deployable models.

The product should support five connected workflows:

1. Explore data from files, databases, and object storage.
2. Develop interactively in Rust notebooks or regular Cargo projects.
3. Build and compare classical ML pipelines through Millwright.
4. Train and monitor deep-learning models through Burn.
5. Export notebooks, reports, datasets, experiments, packages, and models.

Rust remains the primary environment. Python support stops at a managed runtime:
Forge can execute Python and manage its environment, but does not bundle or
reimplement a Python scientific stack. Jupyter, databases, registries, and
remote services are integrations rather than foundations the application
requires in order to run.

## Product principles

- Rust-first, with excellent interoperability rather than hidden language boundaries.
- Project-local reproducibility through Cargo manifests, lockfiles, toolchains, and `.forge/` metadata.
- Process isolation for kernels and long-running training jobs so failures do not crash the UI.
- Arrow-compatible columnar data at subsystem boundaries instead of copying tables through JSON.
- Versioned events for datasets, plots, metrics, artifacts, and training state.
- Code and visual tools should reinforce one another; visual workflows should generate readable Rust.
- Credentials belong in operating-system credential stores, never in projects or notebook output.
- Potentially irreversible operations such as publishing and destructive Git actions require explicit confirmation.

## Target architecture

The prototype currently concentrates most UI behavior in `src/main.rs`. Before
adding large integrations, split the application into workspace crates or
equivalent strongly separated modules:

```text
forge-ml/
├── forge-app          Desktop UI, docking, commands, and window state
├── forge-core         Projects, settings, IDs, and shared domain models
├── forge-protocol     Dataset, plot, metric, training, and artifact events
├── forge-kernel       Rust/Evcxr and Jupyter kernel management
├── forge-data         Arrow tables, previews, queries, and conversions
├── forge-connectors   Files, databases, and object storage
├── forge-millwright   Always-embedded native Millwright integration
├── forge-training     Runs, monitoring, checkpoints, and experiment storage
├── forge-python       Python runtime, environments, and kernels only
├── forge-packages     Cargo/crates.io and Python/PyPI integration
├── forge-github       Git and GitHub integration
├── forge-export       Notebook, report, plot, data, package, and model exports
└── forge-cli          Headless execution, automation, and CI entry points
```

Introduce a versioned event protocol:

```rust
enum ForgeEvent {
    DatasetRegistered { id: DatasetId, schema: DatasetSchema },
    DatasetBatch { id: DatasetId, batch: ArrowBatch },
    PlotCreated(PlotSpec),
    MetricLogged(MetricPoint),
    TrainingProgress(TrainingEvent),
    ArtifactCreated(Artifact),
}
```

The existing `forge_metric`, `forge_vector`, and `forge_table` stdout formats
remain supported as a compatibility layer.

## Phase 0: foundation and reliability

Estimated effort: 4–6 weeks.

- Split UI, runtime, data, plots, experiments, and persistence into modules.
- Introduce stable IDs for datasets, plots, runs, kernels, and artifacts.
- Store workspace metadata in project-local `.forge/` directories.
- Use SQLite for experiment metadata and filesystem storage for large artifacts.
- Move execution results to the versioned event protocol.
- Isolate kernels and training jobs from the GUI process.
- Add integration tests for reset, cancellation, persistence, and large tables.
- Align application, package, and installer versions.
- Upgrade Evcxr independently and cautiously.

Exit criteria:

- No major subsystem exists exclusively inside `main.rs`.
- Reopening a workspace restores layout, datasets, runs, and artifacts.
- Runtime failures cannot crash the GUI.

## Phase 1: complete data workbench

Estimated effort: 6–8 weeks.

### Dataset explorer

- Virtualized tables capable of browsing millions of rows.
- Sorting, filtering, column resizing, pinning, hiding, and reordering.
- Column types, missing values, cardinality, distributions, and statistics.
- Copy selections and export selected rows or columns.
- Multiple independently dockable dataset viewers.
- Dataset refresh, lineage, source provenance, and fingerprints.
- Read-only defaults with an explicit editable mode.
- Linked selection between tables and plots.

### Local data sources

- CSV and TSV.
- Parquet.
- Arrow IPC and Feather.
- JSON and JSON Lines.
- Optional Excel import.
- Millwright `Table`, `Frame`, and `Dataset`.
- Polars, ndarray, and Arrow data.

Millwright's typed ingestion and profiling should be exposed directly through
Forge instead of duplicated. This includes profiles, correlations, missingness,
alerts, and suggested preprocessing pipelines.

### Database and remote sources

Start with SQLite, DuckDB, and PostgreSQL, then add MySQL, SQL Server, cloud
warehouses, and private data services. Prefer Apache ADBC where practical so
query results arrive as Arrow record batches.

- Connection profiles and connection testing.
- Schema, table, view, and column browser.
- SQL editor with parameterized queries and history.
- Limited previews by default to prevent accidental huge downloads.
- Import query results as named datasets.
- Query cancellation and execution timing.
- S3, Azure Blob, and Google Cloud Storage connectors in a later iteration.
- Credentials stored through the OS keychain.

## Phase 2: notebooks and rich output

Estimated effort: 8–10 weeks.

Use one internal notebook model with two representations:

- Git-friendly Rust scripts using `//# %%` cells.
- Standard `.ipynb` files for ecosystem interoperability.

### Notebook features

- Code and Markdown cells.
- Run cell, above, below, selection, and all.
- Per-cell output, timing, status, and execution count.
- Cell insertion, deletion, movement, collapse, and tagging.
- Checkpoints, autosave, recovery, and trusted-output controls.
- Notebook outline and search.
- Kernel selection and restart/interrupt controls.
- Import and export between Rust scripts and `.ipynb` where possible.

### MIME output

- `text/plain`, `text/markdown`, and sanitized `text/html`.
- PNG, JPEG, and SVG.
- Forge dataset, plot, training, and artifact MIME types.
- Plotly/Plotters-compatible rich output.
- Output size limits and lazy loading for large artifacts.

### Jupyter compatibility

Forge should act as a Jupyter frontend rather than embedding JupyterLab:

- Discover kernelspecs.
- Start, stop, interrupt, and restart kernels.
- Execute through the Jupyter messaging protocol.
- Handle display data, streams, errors, completion, hover, and inspection.
- Connect to local kernels first and remote kernels later.
- Use Evcxr as the default Rust kernel.

## Phase 3: native Millwright Studio

Estimated effort: 8–12 weeks.

Millwright should be a first-party integration and the native foundation for
classical ML in Forge.

### Visual pipeline builder

- Source, preprocessing, balancing, feature selection, estimator, and evaluation nodes.
- Drag-and-drop composition with validation.
- Parameter editors generated from Millwright metadata.
- Feature and target selection.
- Train/test splitting and cross-validation configuration.
- Generated readable Rust shown beside the visual pipeline.
- Serialized pipeline configurations stored with experiments.

### Classical ML dashboard

- Classification and regression reports.
- Confusion matrices, ROC, precision-recall, and calibration curves.
- Residuals, predicted-versus-actual plots, and regression diagnostics.
- Cross-validation fold results.
- Grid, random, and Bayesian search progress.
- AutoML leaderboard and candidate comparison.
- Feature importance, permutation importance, and SHAP summaries.
- Outlier, class imbalance, and sampling reports.
- ONNX portability checks.

### Millwright observer API

Millwright needs a UI-independent observer contract used by search, AutoML,
ensembles, fitting, evaluation, and export:

```rust
trait TrainingObserver {
    fn on_event(&mut self, event: TrainingEvent);
}

enum TrainingEvent {
    Started { run_id: String },
    FoldStarted { fold: usize },
    TrialCompleted { params: Params, score: f64 },
    Metric { name: String, step: u64, value: f64 },
    Artifact { kind: ArtifactKind, path: PathBuf },
    Completed { summary: RunSummary },
    Failed { message: String },
}
```

Millwright must not depend on egui or Forge.

## Phase 4: experiments and training monitoring

Estimated effort: 6–8 weeks.

Persist the following for every run:

- Name, tags, notes, status, timestamps, and duration.
- Dataset source, schema, fingerprint, and preprocessing.
- Cargo manifest, lockfile, feature set, and Rust toolchain.
- Git commit, branch, dirty-worktree state, and pull request.
- Hyperparameters, metrics, plots, reports, and artifacts.
- Hardware, device, and environment information.
- Parent/child relationships for trials and folds.
- Checkpoints and best-model selection.

Classical training views should show active trials, folds, leaderboards,
parallel workers, best parameters, score distributions, and ETA.

Deep-learning views should show train/validation metrics, learning rate, epoch,
batch, throughput, ETA, CPU/RAM/GPU utilization, gradient statistics,
checkpoints, early stopping, and the best checkpoint.

## Phase 5: Git and GitHub integration

Estimated effort: 6–8 weeks.

### Local Git

- Initialize and clone repositories.
- Project-tree status decorations.
- Diff viewer and inline staging.
- Stage, unstage, commit, amend, and safe discard.
- Branch, checkout, merge, rebase, fetch, pull, and push.
- Conflict resolution, history, blame, tags, and Git LFS awareness.
- Rust ML `.gitignore` templates.

### GitHub

- Sign in, sign out, account switching, and GitHub Enterprise hosts.
- Clone, fork, publish, and configure upstream repositories.
- Browse, create, review, and merge pull requests.
- Browse and edit issues, labels, milestones, and assignees.
- GitHub Actions status, logs, workflow dispatch, and artifacts.
- Releases and model/report artifact uploads.
- Open files, commits, issues, and runs on GitHub.

Initially use the `gh` CLI when available because it already provides secure
browser authentication and credential-store integration. A later Forge GitHub
App should use fine-grained permissions and short-lived tokens. Tokens must
never be written to `.forge/`, logs, session files, or notebook output.

### ML provenance

Every experiment records its commit and dirty state. Forge should warn when
users compare or publish runs created from different uncommitted source states.
Large datasets and checkpoints should be referenced by fingerprint or artifact
URI rather than committed accidentally.

## Phase 6: package ecosystems

Estimated effort: 7–10 weeks.

Provide one Packages pane with separate Rust and Python environments.

### Cargo and crates.io

- Search by crate name, keyword, category, and capability.
- Show descriptions, versions, dates, licenses, docs, repositories, and features.
- Identify yanked, outdated, incompatible, duplicate, or vulnerable dependencies.
- Add, remove, and update dependencies through Cargo.
- Configure default, optional, target-specific, Git, path, and private-registry dependencies.
- Explain enabled features and dependency trees.
- Run Cargo audit, license, update, and future-incompatibility checks.
- Synchronize notebook `:dep` declarations with generated kernel projects.
- Cache compiled notebook environments using dependency hashes.

### crates.io publishing

- Validate package metadata and SemVer changes.
- Preview packaged files.
- Run tests, formatting, Clippy, docs, and `cargo publish --dry-run`.
- Generate release notes, changelog entries, and tags.
- Publish only after explicit confirmation.
- Support owners, yank/unyank, and private registries.
- Use Cargo's OS credential providers rather than storing tokens in Forge.

### Python and PyPI

- Discover and inspect Python packages and versions through PyPI's JSON APIs.
- Show Python requirements, wheels, licenses, release files, hashes, and vulnerabilities.
- Compare installed and available versions.
- Install, remove, and update through uv, pip, Poetry, or Conda.
- Preview dependency-resolution changes before mutating environments.
- Detect when a package requires local Rust or native compilation.
- Freeze and restore environments from supported lockfiles.
- Associate a Python environment with each project and kernel.

### Coordinated Millwright releases

Forge should coordinate the Rust crate and Python package from one commit:

```text
Version and metadata checks
    → Rust tests and feature matrix
    → Python wheel builds
    → Cargo package dry run
    → TestPyPI publish and smoke test
    → Git tag and GitHub release
    → crates.io and PyPI production publish
```

Prefer PyPI Trusted Publishing through GitHub Actions rather than long-lived
PyPI tokens. Generate workflows for Maturin wheels, TestPyPI, production PyPI,
crates.io, checksums, provenance, and GitHub Release artifacts.

## Phase 7: Python runtime support

Estimated effort: 8–10 weeks.

Use managed Python processes and Jupyter kernels rather than embedding Python
inside the GUI process. This phase ends at reliable runtime execution and
environment management; Forge remains a Rust ML IDE and does not gain
Python-specific ML designers, training dashboards, or framework integrations.

- Discover system Python, uv, Conda, Poetry, and virtualenv environments.
- Create, select, repair, and remove project environments.
- Python console, terminal, scripts, and notebook kernels.
- Capture Python dependencies and interpreter details with experiments.
- Pass standard text, image, HTML, and Jupyter MIME output through unchanged.
- Allow user-installed packages to run without Forge-specific framework support.

PyO3 remains appropriate for Millwright's Python bindings, but the IDE should
not require an embedded interpreter to start.

Forge distributions must not bundle NumPy, pandas, SciPy, scikit-learn,
PyTorch, TensorFlow, or CUDA through the Python integration. Users may install
such packages into their selected environment through the PyPI package tools,
but they remain external project dependencies with their own disk footprint and
support lifecycle.

## Phase 8: deep learning

Estimated effort: 10–14 weeks.

Use Burn as the initial native deep-learning backend because it already offers
training loops, metrics, checkpointing, devices, and multiple compute backends.

- Burn project and notebook templates.
- Backend and device selection.
- Dataset, dataloader, batch, tensor, and image inspection.
- Model summaries and parameter counts.
- Training configuration editor.
- Live metrics and resource dashboard through `TrainingEvent`.
- Checkpoint browser, comparison, resume, and early stopping.
- Confusion matrices, embeddings, saliency, and sample predictions.
- Supported model export and inference templates.

Candle and `tch-rs` can be later adapters. Do not build multiple deep-learning
backends simultaneously before the Forge training protocol is stable.

## Phase 9: plots and visual analytics

Estimated effort: 6–8 weeks and suitable for partial parallel development.

Create a structured `PlotSpec` supporting:

- Line, scatter, bar, area, histogram, density, and box plots.
- Heatmaps, correlation matrices, and confusion matrices.
- ROC, precision-recall, calibration, residual, and Q-Q plots.
- Feature importance, SHAP, and hyperparameter parallel coordinates.
- Time-series plots, images, tensors, and embedding projections.
- Multi-panel figures and linked dataset selections.

Plots should provide zoom, pan, tooltips, series visibility, axis configuration,
logarithmic scales, history, docking, and export to PNG, SVG, PDF, and
interactive HTML. Plotters and Plotly outputs should be accepted, while common
plots are normalized to `PlotSpec` for native interaction.

## Phase 10: export, registry, and deployment

Estimated effort: 6–8 weeks.

### Data

- CSV/TSV, Parquet, Arrow IPC, and JSON Lines.
- Entire datasets or selected rows and columns.

### Notebooks and reports

- `.ipynb`, Rust cell scripts, Markdown, HTML, and PDF.
- Reproducible project bundles.
- Self-contained EDA, evaluation, and experiment comparison reports.

### Experiments

- Metric history as CSV.
- Run manifests as JSON.
- Comparison reports and complete artifact archives.

### Models and services

- Millwright ONNX and whole-pipeline export.
- Local registry, tags, versions, and rollback.
- Generated inference Cargo projects and prediction servers.
- Deployment manifests and container templates.
- Drift and service monitoring views.

## Release sequence

| Release | Primary user-visible outcome |
|---|---|
| 0.2 | Modular foundation, durable projects, and versioned events |
| 0.3 | Production data viewer, local ingestion, Git basics, and crates.io dependencies |
| 0.4 | `.ipynb`, MIME output, GitHub authentication, issues, and pull requests |
| 0.5 | Millwright pipeline builder, evaluation studio, and Python package discovery |
| 0.6 | Persistent experiments, training monitoring, and Git provenance |
| 0.7 | Python runtime, kernels, environments, and package publishing dry runs |
| 0.8 | Databases, SQL workbench, and coordinated Millwright releases |
| 0.9 | Burn deep-learning dashboard and remote training workflows |
| 1.0 | Full exports, registry, deployment, trusted publishing, and hardening |

## Immediate implementation order

1. Refactor Forge into app, runtime, protocol, data, and experiment modules.
2. Replace copied table strings with a virtualized Arrow-backed dataset model.
3. Add Millwright `TrainingObserver` events.
4. Run and display one end-to-end Millwright pipeline in Forge.
5. Add durable experiment storage tied to Git provenance.
6. Add local Git and Cargo dependency panes.
7. Implement the standard notebook/MIME model before adding Python kernels.

This order creates shared foundations for databases, Jupyter, Python, plots,
classical ML, deep learning, packages, and remote execution rather than building
each integration around a different ad hoc transport.
