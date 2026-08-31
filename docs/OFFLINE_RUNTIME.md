# Offline runtime bundle

Forge ML aims to work **fully offline, out of the box** — install it and every
Millwright and Burn capability works with no extra downloads and no
user-installed Rust toolchain.

Two very different execution paths make that possible, and it helps to keep them
straight.

## 1. In-process features (already offline)

The ML Lab classifier, Burn training panel, Millwright Studio (in-process
training), ONNX inference, and data tooling all call libraries **compiled
directly into `forge_ide`**. They need no compiler and no network — they are
just function calls. Millwright ships with `eda`, `onnx`, `smartcore-backend`,
and `linfa-backend`; Burn ships with `std`, `train`, `flex`, `wgpu`, and
`metrics`. See `Cargo.toml`.

Nothing here depends on the offline runtime bundle below.

## 2. Notebook cells / generated projects (need a compiler)

Notebook cells run through **evcxr**, which *compiles the Rust you type* with
`rustc` before running it — true for every cell, not just `:dep` lines. So the
notebook fundamentally needs a Rust toolchain, and `:dep` lines additionally
need the dependency sources.

To make this work with no network and no user toolchain, the installer ships a
**self-contained offline runtime bundle**, and the app points evcxr at it.

### What the bundle contains

Produced by `packaging/build-offline-bundle.{sh,ps1}`:

```
packaging/forge-runtime/
  bin/            cargo, rustc, rustdoc, ...   (the pinned toolchain)
  lib/            rustlib sysroot for the above
  vendor/         every crate in packaging/offline-deps' locked closure
  VERSION         `rustc --version`, shown in the app
```

The vendored set is the locked closure of `packaging/offline-deps/Cargo.toml` — a
dedicated manifest that mirrors the Millwright and Burn features the app is built
with (not the whole GUI workspace). So a notebook `:dep millwright = "…"` resolves
to a version that is present and known to compile. (A bare `:dep` with no lockfile
fails offline on yanked transitives; vendoring the committed `Cargo.lock` avoids
that.)

Note there is **no `config.toml` in the bundle**: cargo requires an *absolute*
`directory` for a vendored source, and the install path isn't known until install
time, so the app generates the config at runtime (below).

Approximate size per platform: ~670 MB toolchain + ~900 MB vendored sources ≈
**1.6 GB uncompressed** (smaller in the compressed installer).

### How the app uses it

`src/offline.rs`:

- `detect()` looks for `forge-runtime/` next to the executable (and in the macOS
  `Resources/` dir), validating that it has a `bin/cargo` and a `vendor/` dir.
  Cached.
- `activate()` (called once by the notebook runtime before evcxr starts) prepends
  `bin/` to `PATH`; creates a **writable** per-user `CARGO_HOME` (the install
  location may be read-only) and writes a `config.toml` there that forces offline
  mode and replaces crates.io with the bundle's **absolute** `vendor/` path; sets
  `CARGO_NET_OFFLINE=true`; and points `EVCXR_TMPDIR` at a writable scratch dir.
- The notebook runtime (`src/runtime.rs`) additionally turns on evcxr's
  `offline_mode` and grants a large persistent compile **cache** so the prebuilt
  dependency artifacts are reused instead of recompiled on first use.

When no bundle is present (development builds), all of this is an inert no-op and
the app falls back to the system toolchain, exactly as before. The current state
is shown in the ML Lab (“Offline Rust runtime bundled (…)” vs. “System Rust
toolchain …”).

## Building a release with the offline runtime

On each release platform (Windows, macOS x64 + arm64, Linux), with rustup and the
pinned toolchain from `rust-toolchain.toml`:

1. Stage the bundle (needs network once, to populate `vendor/`):

   - Unix: `packaging/build-offline-bundle.sh`
   - Windows: `packaging/build-offline-bundle.ps1`

2. Build the installer. `Packager.toml` already lists
   `packaging/forge-runtime/**/*` as a bundled resource, so the runtime is copied
   in alongside the app.

3. Smoke-test offline: disconnect the network, launch the packaged app, and run a
   notebook cell such as:

   ```rust
   :dep millwright = { version = "2.2.1", features = ["smartcore-backend"] }
   use millwright::prelude::*;
   // …train something…
   ```

   It should compile and run with no network access.

### Size and platform notes

- The toolchain + vendored sources add roughly a few hundred MB per platform to
  the installer. This is the cost of "no downloads, ever".
- Only crates in the vendored closure are available offline. A notebook `:dep` on
  an arbitrary *other* crate still needs network + toolchain; that is expected.
- The bundled toolchain, the vendored sources, and any prebuilt compile cache
  must all come from the **same** rustc (`rust-toolchain.toml`) or cargo will
  recompile — keep them in lockstep.
