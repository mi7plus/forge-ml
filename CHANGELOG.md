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
- **Theme builder.** A new *Theme builder* in Settings → Appearance: pick a theme
  (built-in Dark/Light or any custom one), edit the seven base colors with a live
  preview and hex readout, save / duplicate / delete named custom themes, and
  export/import them as JSON. The active theme and custom themes persist with the
  session, and every UI surface follows the active palette (status accent colors
  stay fixed for legibility).
- **Named workspaces.** *File → Save workspace as… / Open workspace…* write and
  load a portable workspace file capturing the project root, open files, dock
  layout, theme (and custom themes), key bindings, appearance settings, and
  database connection profiles. Connection profiles carry only a keychain
  *reference*, never a secret, so a workspace file is safe to share.

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
- Prioritized [improvement roadmap](docs/IMPROVEMENT_ROADMAP.md).
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
