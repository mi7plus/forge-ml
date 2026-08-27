# Forge ML user guide

Forge ML is a desktop scientific IDE for Rust machine-learning projects. Open a Cargo project, edit Rust scripts or `//# %%` notebooks, and run cells in the persistent Evcxr console.

## Keyboard workflow

- `Ctrl+Shift+P`: open the searchable command palette.
- `F6`: move to the next right-side inspector pane.
- `Ctrl+1`: return to Variables.
- `Shift+Enter`: run the current cell.
- `Ctrl+Shift+Enter`: run all cells.
- `Ctrl+S`: save; `Ctrl+N`: new file.
- `Ctrl+F`: find in the active file; `Ctrl+Shift+F`: search the project.
- `Ctrl+Space`: request rust-analyzer completion.

The command palette accepts words from a command name or its shortcut. Use Up/Down, Enter, and Escape without reaching for the mouse.

## Accessibility

Settings offers light/dark themes, a 10–24 px editor font, high contrast, reduced motion, and caret blinking. Reduced motion disables the custom blinking caret. Execution, diagnostics, Git state, and cells use text or symbols in addition to color.

## Data and experiments

Import CSV, TSV, JSON Lines, Parquet, or Arrow IPC from Tools. Click a dataset in Data to open the adjustable lower-right viewer. Export tables from their Data menu. Runs can be compared, cloned, archived, and exported as ZIP bundles.

Inside the full data viewer, select individual or all filtered rows, hide or pin columns, and adjust each visible width. **Edit cells** creates a draft; **Save edits** replaces the in-memory table and rebuilds its Arrow batch, while **Cancel edits** discards every draft change. Selection export includes selected rows and visible columns. Choose numeric X/Y columns to create a linked scatter plot from the current selection.

The Plots pane accepts legacy metrics/vectors and versioned `forge_plot:` JSON. Structured plots support lines, scatter, bars, filled areas, histograms, box summaries, heatmaps, ROC, precision–recall, residuals, and feature importance. Each series can be hidden, axes can be transformed to log10, and definitions can be exported as JSON or standalone SVG.

## External tools and credentials

Forge invokes Cargo, Git, `gh`, Jupyter, Python, AWS CLI, rclone, DuckDB, or PostgreSQL only for the feature that needs them. Those tools own their credentials. Forge project JSON contains profile metadata and opaque credential keys, never tokens or passwords.

## Updates

Use Crates → Check signed updates. Forge downloads the small channel manifest, verifies its GitHub provenance attestation, and reports the matching platform artifact. It never installs or replaces the executable automatically.

## Diagnostics and privacy

Diagnostics are off by default. Settings can enable bounded local events and sanitized crash summaries for the open project. Forge never uploads them automatically. Use the explicit ZIP export to review the exact bundle before sharing it. See [Diagnostics and privacy](PRIVACY.md).
