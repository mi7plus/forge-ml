# Homepage screenshots

The homepage gallery (the **Screenshots** section of [`site/index.html`](../index.html))
loads PNGs from this folder. Until a file is present, its tile shows a
placeholder, so the page always looks intentional — dropping in a real capture is
all it takes to replace one.

## Capture these files

| File | Pane / view to capture |
| --- | --- |
| `workspace.png` | The default dockable layout — file navigator, editor with `//# %%` notebook cells, an inspector on the right, and the console docked below. |
| `classification.png` | Deep-learning pane after **Train classifier**, showing accuracy / per-class F1, with the confusion-matrix heatmap in the Charts tab. |
| `terminals-kernels.png` | Two or three embedded terminals and a Rust kernel open as separate docked panes on one row. |
| `data-viewer.png` | The data viewer on an imported dataset — filter box, a sorted column, and a pinned column visible. |
| `playground.png` | The **Inference playground** with the per-feature form and the live per-class probability bars. |
| `plots.png` | A live training-loss line chart plus the Experiments/Runs board comparing a couple of runs. |

## Recommended capture settings

- **Window size:** ~1600×1000 (16:10). The gallery renders each shot at a 16:10
  aspect ratio, so this avoids cropping. Any consistent 16:10 size is fine.
- **Theme:** dark mode (the site's default look), high-contrast off.
- **Format:** PNG. Keep each file under ~500 KB — resize to ~1600 px wide and,
  if needed, run through an optimizer (`oxipng`, `pngquant`, or squoosh.app).
- **Content:** use the bundled [example notebooks](../../examples) and a sample
  dataset so the panes show real output, not empty state.

## How to capture on each OS

- **Windows:** `Win + Shift + S` (Snipping Tool), or `Alt + PrtScn` for the
  active window.
- **macOS:** `Cmd + Shift + 4`, then press <kbd>Space</kbd> and click the window.
- **Linux:** GNOME Screenshot / Spectacle / Flameshot in window mode.

Save each file here with the exact name from the table, then reload the homepage
— the placeholder is replaced automatically. No HTML or CSS changes are needed.
