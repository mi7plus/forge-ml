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
- `notebooks/plotting_showcase.rs` is a dependency-free tour of **every** plot kind (scatter, line, feature-importance bars, histogram, area, box, heatmap, residual). The scatter includes a deliberate outlier to demonstrate the per-plot **Hide outliers** toggle, and the line pair spans scales to demonstrate **log Y**; try scroll-to-zoom, drag-to-pan, and double-click-to-reset on each.

### ML notebooks (Millwright + Burn)

Runnable notebook versions of the six ML examples, each self-contained — the
dataset is embedded, so they compile and run inside the editor with no file
paths. The first cell is a `:dep` line that pulls in only the features it needs
(so the first compile is as small as possible); run cells top-to-bottom with
`Shift+Enter`.

- `notebooks/millwright_regression.rs` — `LinearRegression` on tips (`smartcore-backend`).
- `notebooks/millwright_classification.rs` — `RandomForest` on iris (`smartcore-backend`).
- `notebooks/millwright_clustering.rs` — `KMeans` on iris (`linfa-backend`).
- `notebooks/burn_regression.rs` — single-layer linear model, SGD (`std,train,flex`).
- `notebooks/burn_classification.rs` — MLP 4→16→3, cross-entropy (`std,train,flex`).
- `notebooks/burn_clustering.rs` — k-means from Burn tensor ops (`std,flex`).
- `notebooks/millwright_timeseries.rs` — univariate forecasting as lag regression
  (`LinearRegression` on lags 1/2/3/12) with a recursive 12-month forecast
  (`smartcore-backend`).
- `notebooks/burn_timeseries.rs` — a linear autoregressive forecaster over a
  sliding window of the last 12 months, SGD-trained (`std,train,flex`).

Each ends with an `//# %% explore` cell that emits `forge_table:` and
`forge_plot:` markers, so running it populates the **Data viewer** (the dataset)
and **Plots** (e.g. actual-vs-predicted, or a feature scatter coloured by class /
cluster). You can also click the 🔍 button next to any variable in the
**Variables** tab to send that value to the Data viewer / Plots on demand.

The first `:dep` compile of a crate takes a while (Burn's is the largest); it is
cached afterwards, and instant once the offline runtime bundle ships. These
mirror the `cargo run --example …` versions below, which read the CSVs from
`data/` instead of embedding them.

These notebook files are nested deliberately. Cargo does not treat them as standalone example binaries, because interactive cell bodies are not ordinary `main` functions.

To test experiment comparison, run the baseline cell in `experiment_comparison.rs`, enter `baseline` in the Plots run-name field, and choose **Save run**. Clear or restart the runtime, run the tuned cell, save it as `tuned`, then select `loss` or `accuracy` in Runs.

## Navigation example

Open `navigation_demo.rs`, hold Left Ctrl, and hover or click `LinearModel`, `predict`, or `mean_squared_error`. Rust-analyzer should underline navigable symbols and open their definitions in `support/model.rs`.

Run the file in Forge to populate Variables with the model, inputs, targets, predictions, and loss. Its vectors appear in Data and Plots, and loss appears as a metric plot.

Run `cargo check --example navigation_demo` to verify the conventional Rust example independently.

## Machine-learning examples

Six self-contained programs cover regression, classification, and clustering with
each of the two bundled ML stacks — [Millwright](https://crates.io/crates/millwright)
(classical models) and [Burn](https://burn.dev) (tensors + autodiff). They read
the small public datasets in `data/`:

- `data/iris.csv` — Fisher's iris measurements, 150 rows, 3 species.
- `data/tips.csv` — restaurant tips, 244 rows.

| Task | Millwright | Burn |
| --- | --- | --- |
| Regression | `millwright_regression` — `LinearRegression`, tip ~ total_bill + size, MAE/RMSE/R² | `burn_regression` — single-layer linear model trained with SGD |
| Classification | `millwright_classification` — `RandomForest` on iris, accuracy/precision/recall/F1 | `burn_classification` — MLP (4→16→3) trained with cross-entropy |
| Clustering | `millwright_clustering` — `KMeans(k=3)` + species cross-tab | `burn_clustering` — k-means built from Burn tensor ops (broadcast distances, argmin, matmul update) |

Run any of them with, for example:

```
cargo run --example millwright_classification
cargo run --example burn_clustering
```

The Millwright examples use its `smartcore-backend` and `linfa-backend` classical
models, which are compiled into Forge ML itself (see the root `Cargo.toml`), so
they need no extra setup. Splits are deterministic (every 5th row is held out)
and, because iris is grouped by species, the split is interleaved to keep all
three classes in both the train and test sets.

Shared CSV helpers live in `support/data.rs`.
