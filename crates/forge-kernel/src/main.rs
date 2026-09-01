//! Minimal evcxr runtime host for Forge ML notebooks.
//!
//! Forge launches this tiny binary as the evcxr runtime child (instead of
//! re-exec'ing the full `forge_ide`), so the process that dlopens each compiled
//! `:dep` cell links nothing but evcxr — no eframe/wgpu/winit/burn to clash with
//! the cell's own dependencies.
//!
//! When invoked by evcxr (with `EVCXR_IS_RUNTIME` set), `runtime_hook` takes over
//! and runs the evaluation loop, never returning. Invoked directly, it is a no-op.
fn main() {
    evcxr::runtime_hook();
}
