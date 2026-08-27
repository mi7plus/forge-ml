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

The data grid renders only the rows and columns intersecting the viewport, with a small overscan buffer for smooth scrolling. This keeps very wide tables responsive without changing sorting, filtering, selection, editing, column visibility/widths, or linked plots. The footer reports how many visible columns are currently rendered. Filtering and sorting still scan the in-memory table, so extremely large datasets should use a narrowed database query or pre-filtered import.

Inside the full data viewer, select individual or all filtered rows, hide or pin columns, and adjust each visible width. **Edit cells** creates a draft; **Save edits** replaces the in-memory table and rebuilds its Arrow batch, while **Cancel edits** discards every draft change. Selection export includes selected rows and visible columns. Choose numeric X/Y columns to create a linked scatter plot from the current selection.

Each table can produce a self-contained EDA HTML report with column profiles and a bounded preview. Runs can produce an offline comparison report containing final metrics, step counts, tags, Git commits, and escaped run manifests.

File → **Export reproducible project bundle** creates a deterministic ZIP with a manifest, file sizes, and content digests. It is limited to 20,000 files, 100 MB per file, and 500 MB total. Build directories, virtual environments, Forge state, Git state, symlinks, and credential-like files are excluded. Review the manifest before sharing.

The Plots pane accepts legacy metrics/vectors and versioned `forge_plot:` JSON. Structured plots support lines, scatter, bars, filled areas, histograms, box summaries, heatmaps, ROC, precision–recall, residuals, and feature importance. Each series can be hidden, axes can be transformed to log10, and definitions can be exported as JSON or standalone SVG.

## External tools and credentials

Forge invokes Cargo, Git, `gh`, Jupyter, Python, AWS CLI, rclone, DuckDB, or PostgreSQL only for the feature that needs them. Those tools own their credentials. Forge project JSON contains profile metadata and opaque credential keys, never tokens or passwords.

## Database connections

The SQL inspector supports embedded SQLite plus the official DuckDB and PostgreSQL command-line clients. Save a project-scoped profile, then use **Test** before schema discovery or running a query. External database commands stop after 30 seconds, previews are limited to 10,000 rows and 64 MiB of CSV output, and stderr is scrubbed of the connection location and stored password before display.

Enter PostgreSQL passwords only in Forge's password field. Forge rejects passwords embedded in PostgreSQL URLs or keyword connection strings, stores accepted credentials through the operating-system credential manager, and never writes them to project profile JSON. The optional ADBC integration exposes the common API boundary, while concrete ADBC driver-manager installation remains user-controlled.

## Object storage

The Storage inspector supports AWS S3-compatible buckets and configured rclone remotes without storing their credentials. Save a profile and use **Test** to verify that its bucket, prefix, endpoint, and external credential chain are reachable. Listings stop after 30 seconds and retain at most 4 MiB; the UI displays at most the requested bounded number of entries.

Download keys are relative to the profile prefix. Forge mirrors the full prefix and key beneath `.forge/object-cache/<profile>/`, so objects with the same basename do not collide. Transfers write to a temporary file, enforce a 2 GiB cache-object limit, and replace an existing cached object atomically. Failed, timed-out, or oversized transfers remove their partial files. Remote endpoints and common secret assignments are redacted from displayed command errors.

## Updates

Use Crates → Check signed updates. Forge downloads the small channel manifest, verifies its GitHub provenance attestation, and reports the matching platform artifact. It never installs or replaces the executable automatically.

## Diagnostics and privacy

Diagnostics are off by default. Settings can enable bounded local events and sanitized crash summaries for the open project. Forge never uploads them automatically. Use the explicit ZIP export to review the exact bundle before sharing it. See [Diagnostics and privacy](PRIVACY.md).
## Portable Millwright models and deployment

In Millwright Studio, build a pipeline and choose **Generate native Millwright ONNX export cell**. The generated Rust cell uses the published Millwright 2.2.1 crate, exports the fitted pipeline through `ExportOnnx`, reloads it with `InferenceModel`, and checks the prediction count. Add Millwright with its `onnx` feature to the notebook project before running the cell.

Use the Deploy inspector to register the resulting artifact, assign an alias such as `production`, and generate a Rust service. Forge resolves the alias first and copies that exact model version into the service. The generated service includes health, readiness, metadata, and prediction routes plus Docker, Compose, and Kubernetes templates. ONNX services perform real inference through Millwright 2.2.1 and accept `{"rows":[[...], ...]}`. Other registered formats receive a clearly labeled editable adapter.

Newly registered model versions are content-addressed with SHA-256 and cannot be overwritten with different bytes. Re-registering identical bytes is safe and idempotent. Forge verifies the artifact whenever it resolves a version or alias, and generated services repeat that verification during readiness and container health checks. The Deploy table shows artifact size and a short digest prefix; full hashes are retained in registry and service metadata.

Runtime output prefixed with `forge_service:` or `forge_drift:` and followed by JSON appears in the Deploy monitoring view. Service records can report `model`, `version`, `requests`, `errors`, and optional `p95_ms`; drift records report `model`, `version`, `feature`, `score`, and `threshold`.
