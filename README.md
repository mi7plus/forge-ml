# Forge ML

Forge ML is an experimental desktop compute studio for interactive Rust machine-learning work.

Its workspace follows the scientific-IDE model popularized by Spyder: an editor-centered layout surrounded by project, outline, variable, plot, help, diagnostics, console, and history panes.

See the [user guide](docs/USER_GUIDE.md), [privacy guide](docs/PRIVACY.md), [development plan](DEVELOPMENT_PLAN.md), [roadmap](ROADMAP.md), [event protocol](docs/PROTOCOL.md), [extension guide](docs/EXTENSIONS.md), and [contributor guide](CONTRIBUTING.md).

## Current prototype

- Persistent Evcxr session running away from the UI thread
- Notebook cells separated with `//# %% <name>`
- Selectable cell navigator and active execution state
- Captured standard output, expression results, compiler errors, and execution time
- Runtime reset without restarting the application
- Variable and training-metric inspector surfaces
- Live Evcxr variable names and Rust types after every successful cell
- Multiple editor tabs with independent unsaved-state protection
- Run Cell, Run Above, and Run All execution flows with per-cell status/output
- Restored project, open files, active file, window, and panel layout across launches
- Recent-project history with safe unsaved-change handling
- Persistent appearance settings for theme, editor font size, and caret blinking
- Background Cargo diagnostics in the Problems inspector
- Telemetry-driven line charts and vector bar visualizations
- Spyder-style dataset viewer in an adjustable bottom-right pane, with undocking, row filtering, and two-dimensional tables
- Deletable live datasets and plots, with experiment snapshots and comparison settings persisted across launches
- SHA-256 experiment provenance for datasets, source identities, Cargo lockfiles, environments, and project bundles
- Project and Outline navigation tabs
- Project-wide search with clickable line and column results
- Clickable source outline for functions, structs, enums, traits, implementations, and modules
- Spyder-style Variable Explorer table and Plots/Help/Problems tool panes
- Interactive Rust console with persistent Evcxr state and command history
- `rust-analyzer` document synchronization, diagnostics, completion, hover, and definition requests
- Inline rust-analyzer diagnostic underlines with hover messages
- Caret-anchored, clickable rust-analyzer completion popup
- Dataset export to CSV, TSV, JSON Lines, Parquet, and Arrow IPC
- Notebook export to `.ipynb`, Markdown, and self-contained HTML
- Compressed experiment bundles containing the run manifest and referenced artifacts
- Project-local versioned model registry with promotion/rollback aliases
- SHA-256 model integrity, immutable version enforcement, and tamper-aware deployment readiness
- Native Millwright 2.2 ONNX export/round-trip cells using the published crates.io API
- Non-blocking native Millwright 2.2.1 CSV/Parquet imports normalized through Forge's bounded data pipeline
- Always-embedded Millwright 2.2.1 plus Burn 0.22 training, metrics, Flex CPU, and WGPU runtimes
- Regression-tested right-pane dataset docking, divider clamping, and persisted layout defaults
- Cached dataset-quality profiles with missingness, type coverage, standard deviation, bounded correlations, and alerts
- Background Arrow construction and quality preparation for file, SQL, and native Millwright imports
- Validated remote Jupyter profiles with credential-safe, bounded background kernelspec probes
- Authenticated background start/stop lifecycle for Forge-managed remote Jupyter kernels
- Self-contained Rust/Axum inference services with real Millwright ONNX inference, health, readiness, metadata, Docker, Compose, and Kubernetes templates
- Framework-neutral request/error/latency and feature-drift monitoring records in the deployment pane
- S3-compatible and rclone object-storage profiles with bounded browsing, reachability probes, and atomic prefix-aware project caching
- Hardened SQLite, DuckDB, and PostgreSQL profiles with test probes, command timeouts, output caps, and credential-safe errors
- Non-blocking database and object-storage operations with typed background result delivery
- Non-blocking CSV, TSV, JSON Lines, Parquet, and Arrow IPC imports with a 512 MiB interactive safety limit
- Bounded dataset materialization with row, column, decoded-text, cell, and Parquet batch limits
- Non-blocking full-dataset exports with shallow Arrow handoff and crash-safe destination replacement
- Private Cargo and PyPI-compatible registry discovery plus GitHub Enterprise authentication validation
- Attested stable/beta update discovery that validates release manifests without silently installing binaries
- Packaging preflight and executable notebook/table/plot performance budgets
- Searchable keyboard command palette, inspector-pane cycling, high contrast, and reduced motion
- Off-by-default local diagnostics and reviewable crash-report ZIP export with no automatic upload
- Framework-neutral structured plots with 11 plot families, series visibility, log transforms, and JSON/SVG export
- Two-axis virtualized data grids with selection, hidden/pinned/resizable columns, explicit draft editing, selection export, and linked plots
- Revision-aware filter/sort indexes and zero-copy steady-state table viewing
- Reproducible project bundles plus standalone EDA and experiment-comparison HTML reports

## Notebook controls

- `Shift+Enter`: run the selected cell
- `Ctrl+Shift+Enter`: run all cells
- `Ctrl+S`: save the active file
- `Ctrl+left-click`: open the definition of the clicked Rust symbol, including definitions in other files
- `Ctrl+F`: find and replace in the active file
- `Ctrl+Shift+F`: search all editable files in the open project
- `Ctrl+Space`: request and open rust-analyzer completions at the caret
- `Ctrl+Shift+P`: open the command palette
- `F6`: cycle through inspector panes
- `Ctrl+1`: open the Variables pane

Cells can publish visualization data through stdout:

```rust
println!("forge_metric:loss={}", loss);
println!("forge_vector:weights=0.2,0.7,1.1,1.8");
println!(r#"forge_table:samples={{"columns":["x","label"],"rows":[[0.2,"cat"],[0.7,"dog"]]}}"#);
```

Metrics appear as live line charts, while vectors appear as bar plots in the Charts inspector. Vectors and tables appear in Data; click a dataset name to open the full table viewer.

## Run on Windows

Use a Visual Studio Native Tools command prompt so Cargo can find the MSVC linker:

```powershell
cargo run
```

Release installers bundle the correct `rust-analyzer` binary for Windows, macOS, or Linux. Development builds can install or repair the component from the Help pane, or manually with:

```powershell
rustup component add rust-analyzer
```

## Packaging

Tagged releases and manual release workflow runs build an NSIS installer for Windows, a DMG for macOS, and DEB/AppImage packages for Linux. The release workflow downloads the matching official `rust-analyzer` sidecar and includes it in each package.

Tagged release artifacts and their update-channel manifests receive GitHub build-provenance attestations. Forge can verify and report an available update from the Crates pane, but installation remains an explicit user action. Operating-system code signing and notarization require the release environment's own signing identities.

Select a cell in the left notebook rail, then choose **Run cell**. Cells share one Evcxr session, so setup and dataset cells can define values used by later cells.
