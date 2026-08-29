# Forge ML examples

Open the Forge ML repository as a project, then use the files in this folder to exercise IDE features.

## Notebook examples

Files under `notebooks/` contain Forge `//# %%` cells. Open one in the editor and place the caret in a cell before pressing `Shift+Enter`.

- `notebooks/basics.rs` tests shared Evcxr state, cell insertion, movement, deletion, and output clearing.
- `notebooks/training_metrics.rs` publishes loss, accuracy, weights, and sample data to the Plots inspector.
- `notebooks/experiment_comparison.rs` provides baseline and tuned cells for testing named run snapshots, overlays, and CSV export.
- `notebooks/stop_execution.rs` runs long enough to test the Stop button safely.
- `notebooks/diagnostics.rs` contains intentional errors for the Problems pane.
- `notebooks/native_regression.rs` fits a univariate model inline and emits a loss curve, an original-unit equation, MAE/RMSE/R2, and actual-vs-predicted vectors, mirroring the native-regression workflow.
- `notebooks/structured_plots.rs` emits versioned `forge_plot:` JSON across several plot families (line, scatter, bars, histogram) plus legacy metric/vector markers, to exercise the Plots pane and its exports.

These notebook files are nested deliberately. Cargo does not treat them as standalone example binaries, because interactive cell bodies are not ordinary `main` functions.

To test experiment comparison, run the baseline cell in `experiment_comparison.rs`, enter `baseline` in the Plots run-name field, and choose **Save run**. Clear or restart the runtime, run the tuned cell, save it as `tuned`, then select `loss` or `accuracy` in Runs.

## Navigation example

Open `navigation_demo.rs`, hold Left Ctrl, and hover or click `LinearModel`, `predict`, or `mean_squared_error`. Rust-analyzer should underline navigable symbols and open their definitions in `support/model.rs`.

Run the file in Forge to populate Variables with the model, inputs, targets, predictions, and loss. Its vectors appear in Data and Plots, and loss appears as a metric plot.

Run `cargo check --example navigation_demo` to verify the conventional Rust example independently.
