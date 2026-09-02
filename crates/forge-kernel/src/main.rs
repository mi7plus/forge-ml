//! The evcxr runtime child for Forge ML notebooks.
//!
//! Forge launches this binary as evcxr's runtime child (instead of re-exec'ing
//! `forge_ide`). It statically links Millwright and Burn so that, when the child
//! `dlopen`s a compiled `:dep` cell, those crates' native/DLL dependencies are
//! already resident — a minimal evcxr-only child deadlocks there on the Windows
//! loader lock, and re-exec'ing the full forge_ide deadlocks too (a conflict with
//! its GUI/system crates). Millwright+Burn-only loads cleanly.
//!
//! When evcxr invokes this binary it sets `EVCXR_IS_RUNTIME`, and `runtime_hook`
//! takes over the evaluation loop and never returns. Invoked directly it just
//! runs the (unreachable-in-practice) keep-alive references below and exits.
fn main() {
    evcxr::runtime_hook();

    // Never reached when running as the evcxr runtime child (runtime_hook does
    // not return then). Present only so Millwright and Burn are actually linked
    // into the binary — the whole point — rather than dead-code-eliminated.
    // Uses the CPU (Flex) path only, so no GPU is initialized.
    let _ = std::hint::black_box(
        millwright::frame::Frame::from_rows(vec![vec![1.0]], vec!["x".into()]).is_ok(),
    );
    let _ = std::hint::black_box(
        burn::tensor::Tensor::<1>::from_floats(&[1.0f32][..], &burn::tensor::Device::flex())
            .sum()
            .into_scalar::<f32>(),
    );
}
