<p align="center">
  <img src="assets/logo-wordmark.svg" alt="Forge ML" width="440">
</p>

# Forge ML

Forge ML is a desktop compute studio for interactive Rust machine-learning work, shipping as signed Windows, macOS, and Linux installers.

Its workspace follows the scientific-IDE model popularized by Spyder — an editor surrounded by project, outline, variable, plot, help, diagnostics, console, and history panes — but every surface is a fully dockable pane. Split, drag between regions, tab together, reorder, or hide any pane; the arrangement is remembered across restarts.

A browsable feature site lives under [`site/`](site/) (homepage plus a detailed guide), deployed to GitHub Pages by [`.github/workflows/pages.yml`](.github/workflows/pages.yml).

See the [user guide](docs/USER_GUIDE.md), [architecture](ARCHITECTURE.md), [roadmap](ROADMAP.md), [environment design](docs/FORGE_ENV.md), [privacy guide](docs/PRIVACY.md), [event protocol](docs/PROTOCOL.md), [extension guide](docs/EXTENSIONS.md), and [contributor guide](CONTRIBUTING.md).

## What it does

Forge ML has grown past a prototype — it ships signed Windows, macOS, and Linux
installers, each carrying an offline Rust runtime so notebooks and generated
projects build with no user-installed toolchain and no network. The highlights
below are grouped by workflow; the [feature site](site/) and
[user guide](docs/USER_GUIDE.md) carry the exhaustive list.

### Workspace & editor
- Fully dockable `egui_tiles` workspace — split, drag between regions, tab,
  reorder, or hide any pane, with the layout persisted across restarts and a
  **View → Panes** menu to toggle visibility
- Multiple editor tabs with independent unsaved-state protection, drag-to-reorder
  and middle-click-close, plus a bottom status bar (runtime state, active file,
  background tasks, cursor position, language-server status)
- Session restore for project, open files, active file, window, layout, and
  appearance (theme, editor font size, caret blink); recent-project history with
  safe unsaved-change handling
- Command palette, `Ctrl+1`–`Ctrl+9` inspector jumps, `F6` pane cycling, high
  contrast, and reduced-motion modes

### Notebooks & the Rust runtime
- Persistent Evcxr session off the UI thread; cells separated with `//# %% <name>`
- Run Cell / Run Above / Run All with per-cell status, captured stdout,
  expression results, compiler errors, and timing; runtime reset without restart
- Live variable names and Rust types after every successful cell, a selectable
  cell rail, and an interactive Rust console with persistent state and history
- `rust-analyzer` synchronization, diagnostics with inline underlines, a
  caret-anchored completion popup, hover, and go-to-definition across files;
  project-wide search and a clickable symbol outline

### Data
- Dataset viewer as a dockable pane or floating window: two-axis virtualized
  grids with selection, hidden/pinned/resizable columns, draft editing, filter
  and sort, selection export, and linked plots
- Non-blocking CSV/TSV/JSON Lines/Parquet/Arrow IPC import and export with a
  512 MiB interactive safety limit, bounded Arrow-batch materialization, and live
  decoded-row progress
- Cached dataset-quality profiles (missingness, type coverage, standard
  deviation, bounded correlations, alerts) built in the background
- SQLite, DuckDB, PostgreSQL, and verified-TLS MySQL connection profiles with
  a read-only SQL workbench and project-scoped query history; S3-compatible and
  rclone object-storage profiles with bounded browsing

### Machine learning
- Classical ML with **Millwright 2.2** compiled in (smartcore + linfa backends,
  pure Rust): design a pipeline and train it in-process on the selected table —
  no toolchain, no network — with ONNX export/round-trip
- Deep learning with **Burn 0.22** compiled in: one-click Flex/autodiff and
  WGPU-backend training with native SGD, validated epoch/learning-rate controls,
  deterministic validation holdouts, patience-based early stopping, and
  leakage-safe standardization
- Framework-neutral training telemetry: live loss/metric/throughput plots,
  concurrent run contexts, and self-contained HTML/PDF reports and bundles

### Models & deployment
- Project-local versioned model registry with promotion/rollback aliases,
  SHA-256 integrity, immutable-version enforcement, and integrity-verified loading
- Attested regression artifacts with atomic JSON import/export and in-IDE
  inference; background batch inference into Arrow-backed prediction datasets with
  feature-drift monitoring
- Self-contained Rust/Axum inference services (real Millwright ONNX inference)
  with health/readiness/metadata endpoints and Docker/Compose/Kubernetes
  templates, plus a deployment-monitoring pane (requests, errors, p95 latency,
  drift) with HTML/PDF reports

### Experiments, plots & provenance
- Durable, reorderable plot history across 11 plot families — including
  statistical box/violin/ECDF — with SVG/PNG/PDF/interactive-HTML export and a
  **Hide outliers** / zoom / pan toolkit
- Telemetry-driven line charts and vector bar plots; experiment snapshots and
  comparison settings persisted across launches
- SHA-256 provenance for datasets, source identities, Cargo lockfiles,
  environments, and project bundles; reproducible project bundles and standalone
  EDA / experiment-comparison HTML and PDF reports

### Distribution & packaging
- A thin `forge` CLI (scaffold projects with `forge.toml`, pass through to Cargo,
  `env sync|doctor`) and a `forge_ml` umbrella crate re-exporting the curated
  stack — both shipped in the installer
- A declarative `forge.toml` / `forge.lock` environment system with reserved
  `[native]`/`[gpu]`/`[python]` seams for a future environment manager
- Tagged releases build an NSIS installer, a macOS DMG, and Linux DEB/AppImage
  packages with build-provenance attestations; updates are discovered and
  verified but never installed silently
- Off-by-default local diagnostics with reviewable crash-report export — no
  automatic upload (see the [privacy guide](docs/PRIVACY.md))

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
- `Ctrl+1`–`Ctrl+9`: jump to the first nine inspector panes (`Ctrl+1` is Variables)
- Drag an editor tab to reorder it; middle-click a tab to close it

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
