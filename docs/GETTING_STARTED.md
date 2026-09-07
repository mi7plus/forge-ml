# Getting started — your first 10 minutes

Forge ML is a batteries-included scientific-computing distribution for Rust: one
install gives you a curated ML stack, an interactive studio, and a small `forge`
command line — with Cargo and rustup underneath, never in your way. This walk
takes you from nothing to a trained, reproducible model **without typing a single
`cargo` command**.

## 1 · Install (once)

Download the installer for your platform from the
[releases page](https://github.com/mi7plus/forge-ml/releases/latest) — Windows
(`.exe`), macOS (`.dmg`), or Linux (AppImage / `.deb`). Each bundles an offline
Rust runtime, so notebook `:dep` cells build with no toolchain and no network,
and GPU acceleration is compiled in (DirectML on Windows, CoreML on macOS, CUDA
on Linux). The installer also puts a small `forge` command on your `PATH`.

Check your machine at any time:

```
forge doctor
```

It reports the toolchain, a C linker, CUDA, and Python — with a plain note when
something optional is missing.

## 2 · Create a project

Pick a **profile** — a curated, compatible crate set for a kind of work:

| Profile | For | Adds |
| --- | --- | --- |
| `classical-ml` (default) | tabular / classical models | polars, ndarray, Millwright |
| `data` | data wrangling & analysis | polars, ndarray, plotters, statrs |
| `deep-learning` | tensors & neural nets | burn, ndarray |

```
forge new house-prices --profile classical-ml
cd house-prices
```

That scaffolds a Cargo project, a `forge.toml` environment manifest, a starter
`src/main.rs`, and a `README.md` written entirely in `forge` commands.

## 3 · Run it

```
forge run
```

You'll see the starter confirm the stack is wired. `forge run`, `forge build`,
and `forge test` are thin passthroughs to Cargo with sensible defaults — use them
instead of `cargo`.

## 4 · Open the studio and train a model

```
forge ide
```

This opens the Forge ML desktop app on your project. Inside, with no build files:

- **Import** a CSV / Parquet / Arrow dataset from the Data pane.
- **Train** a classical pipeline with Millwright, or a neural net with embedded
  Burn — choosing CPU or GPU once in **Settings → Compute**.
- **Inspect** live loss/metric plots, variable state, and run history.
- **Ship** the model: export an ONNX artifact, register a version, or generate a
  self-contained Rust inference service — all from the UI.

Every training run is captured with its provenance (git commit, `Cargo.lock`
hash, dataset hashes, toolchain).

## 5 · Pin and reproduce

Freeze the environment, then prove any machine can reproduce a saved run:

```
forge env sync                 # writes forge.lock
forge reproduce <run-id>       # ok / warn / DIFF per dimension; non-zero if it can't
```

`forge reproduce` is the payoff: clone the repo elsewhere, run it, and Forge tells
you exactly whether — and if not, why not — the run reproduces. It composes as a
CI gate.

## Where things live

- `forge.toml` — your environment manifest (profile, toolchain, reserved
  `[gpu]`/`[native]`/`[python]` sections). See [FORGE_ENV.md](FORGE_ENV.md).
- `forge.lock` — the generated, pinned environment (references `Cargo.lock` by
  hash; never hand-edited).
- `Cargo.toml` / `Cargo.lock` — still the source of truth for crates. Forge builds
  *around* them; you rarely need to touch them directly.

## The `forge` command, in one screen

```
forge new <name> [--profile P]   scaffold a project + forge.toml
forge add <crate>...             cargo add with data-science feature defaults
forge run | build | test         cargo passthrough
forge env sync | doctor          write forge.lock / report the environment
forge doctor                     diagnose toolchain, linker, CUDA, Python
forge reproduce <id>             verify the environment against a recorded run
forge ide [dir]                  open the Forge ML studio
```

That's the whole loop: **new → run → ide → reproduce**, and you never had to learn
Cargo to ship a model.
