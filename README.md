# Forge ML

Forge ML is an experimental desktop compute studio for interactive Rust machine-learning work.

Its workspace follows the scientific-IDE model popularized by Spyder: an editor-centered layout surrounded by project, outline, variable, plot, help, diagnostics, console, and history panes.

See the [development plan](DEVELOPMENT_PLAN.md) for the target architecture and full product scope, and the [roadmap](ROADMAP.md) for implementation status and upcoming milestones.

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
- Generated Rust/Axum inference services with Docker and Compose templates
- S3-compatible and rclone object-storage profiles with bounded browsing and project-local downloads
- Private Cargo and PyPI-compatible registry discovery plus GitHub Enterprise authentication validation

## Notebook controls

- `Shift+Enter`: run the selected cell
- `Ctrl+Shift+Enter`: run all cells
- `Ctrl+S`: save the active file
- `Ctrl+left-click`: open the definition of the clicked Rust symbol, including definitions in other files
- `Ctrl+F`: find and replace in the active file
- `Ctrl+Shift+F`: search all editable files in the open project
- `Ctrl+Space`: request and open rust-analyzer completions at the caret

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

Select a cell in the left notebook rail, then choose **Run cell**. Cells share one Evcxr session, so setup and dataset cells can define values used by later cells.
