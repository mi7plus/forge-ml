# Changelog

All notable changes to Forge ML are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to
follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Development to date has moved through four themed phases — a dockable workspace,
embedded terminals and kernels, an editor/LSP tooling sweep, and an ML modelling
sweep — followed by an internal restructuring of the application code. Those are
grouped under the **0.98.0** release below.

## [Unreleased]

### Added
- **Git pane: interactive branch list.** Refresh/Branches now render a selectable
  list (current branch and remotes flagged with icons). Left-click selects a
  branch; right-click opens a context menu — **Checkout**, **Merge into current
  branch**, **Delete**, **Force delete** — each disabled where it doesn't apply
  (the checked-out branch, or a remote). A conflicting merge flows straight into
  the conflict-mediation section.
- **Plots pane: Clear plots.** A dedicated button that removes all structured
  plots while leaving the metric/vector datasets in place (distinct from *Clear
  current*, which wipes both).

### Changed
- **Cells explorer navigation.** Clicking a cell in the Notebook Cells rail now
  jumps the editor to that cell — moving the caret to its start and scrolling it
  into view — instead of only marking it selected.
- **Responsive pane toolbars.** The Data viewer's filter/dataset rows and the
  Plots pane's run/export and experiment-metadata rows now wrap onto the next
  line on a narrow pane instead of overflowing off the right edge.
- **CI hardening.** Build & test now run on **Windows and macOS** as well as
  Linux (catching the platform-specific console/icon/rust-analyzer paths that
  previously only surfaced at release), clippy runs with `--all-features` (so
  `adbc` and `forge_ml`'s `deep-learning` code is linted, not left to rot), and
  the examples are compiled in CI.
- **Testable status/plot internals.** The status-bar run-state label is now a
  pure `run_state_chip` function, and the plotting/stats helpers gained
  edge-case coverage (empty / single / all-equal inputs) plus a headless egui
  render smoke test — the class of bug behind the console-overflow panic.

### Fixed
- **Editor caret-scroll.** Jump-to navigation (cell rail, symbol outline,
  insert-cell, find/replace) moved the caret but never scrolled the editor to
  it, because the scroll-area id didn't match the one `CodeEditor` actually
  uses. It now reveals the target line.
- **Status-bar overflow panic.** Running code from the console
  (`RunState::Running(usize::MAX)`) formatted `cell + 1` and panicked in debug;
  it now shows "Running console".

### Removed
- Dead code: the unused `pane_layout` module (superseded by `egui_tiles`), the
  unused `console_panel_frame` helper, and the no-op `millwright` feature alias.

## [1.3.0] — 2026-09-05

### Added
- **Statistical plots in the Plots pane.** Proper Tukey box-and-whisker (per-group
  boxes, median, 1.5·IQR whiskers, outliers), plus new **violin** (Gaussian-KDE
  density) and **ECDF** kinds. Every plot also gained a **Hide outliers** toggle
  (drops the extreme 1% per axis) and a discoverable zoom/pan/reset hint. The
  `plotting_showcase` notebook demonstrates all ten kinds.
- **Git pane: history, branch delete, and merge-conflict mediation.** A commit
  history (log) view, Delete / Force-delete branch, and a conflict-resolution
  section (Keep ours / Keep theirs / Mark resolved, Continue / Abort merge)
  shown while a merge is in progress.

## [1.2.0] — 2026-09-05

### Added
- **`forge` CLI + `forge_ml` umbrella crate.** A new thin, dependency-free
  `forge` command (`crates/forge-cli`) wraps Cargo/rustup with data-science
  defaults — `forge new <name> --profile …` scaffolds a project and `forge.toml`
  and adds the curated crate set; `add`/`run`/`build`/`test` pass through to
  Cargo; `env sync|doctor`, `doctor`, and `ide` delegate to `forge_ide`. And a
  new `forge_ml` umbrella crate (`crates/forge-ml`) re-exports the curated stack
  (ndarray + Millwright by default, Burn behind `deep-learning`) behind one
  `prelude`. Both pin the versions already in the workspace, so they add no new
  dependency trees. (Phase 1 of the Forge distribution — see docs/FORGE_ENV.md.)
  The `forge` binary ships inside the installer alongside `forge_ide`.
- **Time-series examples.** `millwright_timeseries` (univariate forecasting as lag
  regression — `LinearRegression` on lags 1/2/3/12 with a recursive 12-month
  forecast) and `burn_timeseries` (a linear autoregressive forecaster over a
  sliding window, SGD-trained), as both runnable `cargo` examples and `//# %%`
  notebooks under `examples/notebooks/`. Self-contained (a deterministic synthetic
  monthly series), compile-verified, and emitting `forge_table`/`forge_plot`.

### Changed
- **Repo hygiene.** Consolidated four planning docs into a single `ROADMAP.md`
  plus a new `ARCHITECTURE.md` code map, gated CI on `cargo fmt --check` and
  clippy `-D warnings`, moved the dataset-table renderer out of `main.rs` into
  `ui/data_view.rs`, and gave `forge.lock` a real structural fingerprint.

## [1.1.0] — 2026-09-02

### Added
- **Forge environment system (seams).** A declarative `forge.toml` manifest, a
  generated `forge.lock`, and an `EnvironmentProvider` interface the app resolves
  the runtime through (`src/environment/`). The manifest's `[native]`, `[gpu]`,
  and `[python]` sections are **reserved** — parsed and validated today, surfaced
  as "recognized, not yet active" — so a full environment manager is an additive
  change (new providers, filled-in sections) rather than a rewrite. The one
  provider today wraps the offline runtime bundle, so activation behavior is
  unchanged. New `--env-doctor [dir]` and `--env-sync [dir]` CLI entry points
  report the environment and write `forge.lock`. See `docs/FORGE_ENV.md`.

## [1.0.0] — 2026-09-02

First installer release: Windows/macOS/Linux packages built and published by the
release pipeline, each carrying the offline Rust runtime.

### Added
- **Brand logo.** A new Forge ML mark — a forged anvil with a neural-spark crown,
  in a Rust-hot amber→red palette — now appears as the app window/taskbar icon,
  the installer icon (Windows `.ico`, macOS `.icns`, Linux PNGs), the splash
  screen, the website favicon and nav, and the README. Master art in `assets/`
  (transparent mark, dark badge, and wordmark SVGs); the raster set is
  regenerated by `packaging/rasterize`.
- **ML example notebooks.** Notebook versions of all six ML examples under
  `examples/notebooks/` (Millwright and Burn × regression/classification/
  clustering), runnable in the editor with `Shift+Enter`. Each is self-contained:
  the dataset is embedded and the first cell is a `:dep` line requesting only the
  features it needs, so nothing has to be downloaded by hand and the first
  compile is as small as possible. All six are compile-verified.
- **Offline runtime bundle.** The packaged installer can run notebook `:dep`
  cells and generated projects for Millwright/Burn with no network and no
  user-installed Rust toolchain. The app (`src/offline.rs`) detects a bundled
  `forge-runtime/` toolchain + vendored dependency cache next to the executable
  and points evcxr at it: it writes a writable per-user `CARGO_HOME` whose config
  forces offline mode and replaces crates.io with the bundle's absolute `vendor/`
  path, and grants evcxr a large persistent compile cache. Inert no-op in
  development builds. Ships `packaging/build-offline-bundle.{sh,ps1}`, the blessed
  `packaging/offline-deps` manifest, a pinned `rust-toolchain.toml`, resource
  wiring, and `docs/OFFLINE_RUNTIME.md`; the ML Lab shows whether the offline
  runtime is active. **Verified end-to-end on Windows**: a cell using both
  Millwright and Burn resolves, compiles, and runs entirely offline against the
  vendored bundle (~1.6 GB uncompressed per platform).
- **In-process Millwright training.** The Millwright Studio can train a designed
  pipeline **inside Forge** — *Train pipeline in Forge* and *Train & export ONNX*
  fit the pipeline on the table selected in the Data viewer using the compiled-in
  classical backends, with no Rust toolchain or network. Results stream into the
  existing training telemetry (run overview, plots, report) and the console shows
  held-out metrics (accuracy for classifiers, R² for regressors) from a
  deterministic 80/20 split.

### Changed
- **Millwright is now fully native.** The `smartcore-backend` and `linfa-backend`
  classical-model backends (LinearRegression, LogisticRegression, RandomForest,
  KMeans, …) are compiled into the Forge ML binary itself, alongside the existing
  `eda` and `onnx` features — no longer scoped to example builds. Both backends
  are pure Rust (linfa uses `linfa-linalg`, not a system BLAS), so this keeps the
  Windows/macOS/Linux installer self-contained with no toolchain or network
  needed at runtime.

## [0.99.0] — 2026-08-31

### Fixed
- Editor tabs are selectable again — clicking any tab switches to it. The
  drag-to-reorder source had been occluding each tab's click sense, so only the
  last-opened file could be focused.
- rust-analyzer now sets itself up automatically: if the component is missing
  when a file is opened, the IDE installs it in the background via rustup and
  starts the server — no manual command. (The common case was a rustup *proxy*
  on PATH without the component installed, which reported "unavailable".) If
  rustup itself is absent, the footer points to https://rustup.rs.

### Changed
- The code editor grows to fill its pane down to a one-line status strip
  (Ln/Col, character count, language) at the bottom, instead of a fixed-height
  box. The Ln/Col readout moved from the IDE footer into that editor strip.
- Footer status text (e.g. the language-server message) truncates to the
  available width with an ellipsis and shows in full on hover (and copies on
  click), instead of being cut off.
- Terminal ANSI colors follow the active theme — the 16 base colors are mapped
  onto the palette (semantics preserved: 1=red, 2=green, …) so prompts, `ls`,
  and git output match the IDE; the 256-color cube keeps standard xterm values.

### Added
- **More keyboard shortcuts.** Go to definition (F12), Find references
  (Shift+F12), New terminal (Ctrl+`), Close tab (Ctrl+W), Stop execution
  (Ctrl+.), and Open settings (Ctrl+,) — all rebindable in Settings.
- **UI/UX polish.** Icon buttons in the file-explorer toolbar; new toolbar
  buttons (Format, Find, Command palette, New terminal, Settings gear,
  light/dark toggle) whose tooltips show the live shortcut; a clickable status
  bar (file name reveals the Files pane, background-task count opens Problems,
  the language-server status restarts rust-analyzer); a `project › folder ›
  file` breadcrumb under the editor tabs; and friendly empty states (no project
  open, no variables, no saved runs) with call-to-action buttons.
- **Theme builder.** A new *Theme builder* in Settings → Appearance: pick a theme,
  edit eight colors — the seven base colors plus the primary **accent** — with a
  live preview and hex readout, save / duplicate / delete named custom themes,
  and export/import them as JSON. The active theme and custom themes persist with
  the session. The whole IDE follows the palette, including the **code-editor
  background** and **syntax colors** (keywords, strings, comments, functions,
  types, numbers), the **terminal**, and **floating-window headers**; the three
  status colors (warn/ok/error) stay fixed for legibility. Ships 14 built-in
  themes alongside Dark and Light: **Nord**, **Dracula**, **Solarized
  Dark/Light**, **Gruvbox Dark**, **Rosé Pine**, **One Dark Pro**, **Tokyo
  Night**, **Monokai**, **Night Owl**, **Cobalt2**, plus **Crimson**,
  **Matrix**, and **Hacker**.
- **Interface scale.** A Settings → Appearance slider scales *all* text and
  controls across the whole IDE (not just the editor), from 80% to 160%,
  persisted with the session.
- **Named workspaces.** *File → Save workspace as… / Open workspace…* write and
  load a portable workspace file capturing the project root, open files, dock
  layout, theme (and custom themes), key bindings, appearance settings, and
  database connection profiles. Connection profiles carry only a keychain
  *reference*, never a secret, so a workspace file is safe to share.
- **Runnable ML examples.** Six bundled `cargo` examples covering regression,
  classification, and clustering with both Millwright and Burn, reading the small
  public iris and tips datasets under `examples/data/`. Run e.g. `cargo run
  --example burn_classification`. Millwright's classical backends
  (`smartcore-backend`, `linfa-backend`) are pulled in as dev-dependencies, so
  they build only for the examples and not the IDE itself. See
  `examples/README.md`.

## [0.98.0] — 2026-08-31

### Added

#### Dockable workspace
- Fully dockable, persistent `egui_tiles` workspace: every pane can be docked,
  moved, floated into a real OS-style window, hidden, or re-enabled from the
  **View** menu, and the layout is saved and restored across sessions.
- Right-click a dock tab for undock / dock / hide actions.
- Reorderable editor tabs with per-pane scrolling, tab icons, a status bar, and
  `Ctrl+1..9` jumps to the first nine inspector panes.

#### Terminals and Rust kernels
- Embedded, cross-platform system terminal pane (Windows ConPTY, Unix PTY) with
  full VT emulation via `portable-pty` + `alacritty_terminal`.
- Multiple independent terminals, each its own dockable / floatable pane.
- Multiple independent Rust kernels (Evcxr runtimes) alongside the terminals,
  each dockable / floatable.

#### Editor & language tooling (rust-analyzer)
- rustfmt: **Format Document** and optional format-on-save.
- clippy lints surfaced in the Problems pane.
- Cargo tasks (build / test / run) and a **Run clippy** action.
- rust-analyzer depth: find-references, signature help, rename, and code
  actions / quick fixes, plus snippet completions.
- Command palette upgrade: fuzzy matching, recent commands, and live key
  bindings shown per command.
- Welcome / start screen with open-recent and example launchers.
- Customizable keyboard shortcuts with a Settings **Keyboard** section
  (rebinding, conflict detection, restore-defaults) persisted with the session.
- GitHub Actions PR CI: build + test required, fmt/clippy advisory.

#### Machine learning
- Native multiclass classification — softmax (multinomial logistic) regression
  with accuracy, per-class precision / recall / F1, and a confusion-matrix
  heatmap. Pure Rust and fully unit-tested.
- Deterministic train/test split for classification.
- Classifier hyperparameter sweep (grid search over learning rate × epochs),
  ranked by held-out macro-F1.
- Dataset-preparation step: categorical one-hot / ordinal encoding, missing-value
  drop / mean / zero imputation, and standardize / min-max scaling, written to a
  new `… · prepared` numeric dataset.
- ONNX import for in-IDE inference (via Millwright / tract): score a single
  comma-separated feature row or the open dataset's feature columns.
- Inference playground: an interactive per-feature form with live per-class
  softmax probabilities from the trained classifier.

#### Site & docs
- Redesigned GitHub Pages homepage and an expanded feature guide.
- Prioritized improvement roadmap (later folded into [ROADMAP.md](ROADMAP.md)).
- This changelog.

### Changed
- **Restructured `main.rs`** (≈11k → ≈4.1k lines) into a `ui/` module tree of
  `impl crate::ForgeApp` blocks — `theme`, `plotting`, `editing`, `grid`,
  `ml_lab`, `services`, `scm`, `data_view`, `notebook_io`, `panes`, `menus`,
  `editor`, `editor_pane`, `dock`, and `shortcuts`. `main.rs` now holds the app
  struct, constructor, lifecycle/background polling, LSP wiring, and the
  `egui_tiles` / `eframe::App` trait impls. No behavioral change.
- Menu-bar dropdowns now switch on hover once a menu is already open.

### Fixed
- Windows ConPTY: keep the PTY slave handle alive so the shell stays connected.
- Removed a stray `#[derive(Clone, Copy)]` left dangling before the test module
  (would have broken `cargo test`).
- Workspace-edit and rustfmt tests made portable across Windows and Unix.

## [0.1.4] and earlier

Initial tagged releases (`v0.1.0`–`v0.1.4`) established the core IDE: the Evcxr
notebook runtime with `//# %%` cells, data import (CSV/TSV/JSON Lines/Parquet/
Arrow) with a virtualized grid, the plot families, experiments/runs with
provenance, the model registry and generated Axum inference services, embedded
Burn training and Millwright ONNX export, the SQL workbench, object storage, and
remote Jupyter. See the Git tag history for details.
