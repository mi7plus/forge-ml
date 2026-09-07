//! # forge_ml
//!
//! An umbrella crate that re-exports the curated Forge ML Rust data-science
//! stack behind one dependency and one prelude, so a scientific-Rust project can
//! start from `use forge_ml::prelude::*;` instead of assembling and version-
//! matching the pieces by hand. Cargo stays underneath — this crate only
//! re-exports; it resolves nothing itself.
//!
//! ## Features
//! - `classical` (default): [`millwright`] classical models (smartcore + linfa backends).
//! - `deep-learning`: the [`burn`] tensor/autodiff framework.
//! - `data`: the [`polars`] dataframe library (csv + parquet).
//!
//! These mirror the CLI's curated profiles (`forge new --profile …`). The
//! profiles scaffold crates into a *user's* project and so can pull in crates
//! Forge itself doesn't depend on (e.g. `plotters`, `statrs`); this umbrella only
//! re-exports crates already in Forge's own tree, so its `data` feature is
//! polars rather than the full data profile.
//!
//! ```no_run
//! use forge_ml::prelude::*;
//! let x = Array2::<f64>::zeros((8, 4)); // ndarray, always available
//! let _ = x;
//! ```

pub use ndarray;

#[cfg(feature = "classical")]
pub use millwright;

#[cfg(feature = "deep-learning")]
pub use burn;

#[cfg(feature = "data")]
pub use polars;

/// The one glob-import that brings the curated stack into scope.
pub mod prelude {
    pub use ndarray::prelude::*;

    #[cfg(feature = "classical")]
    pub use millwright::prelude::*;

    #[cfg(feature = "deep-learning")]
    pub use burn::tensor::{Device, Tensor};

    // Specific items rather than a glob: polars' prelude overlaps names with
    // ndarray/millwright, and glob-re-exporting all three would shadow items
    // silently. The full prelude is a qualified import away: `forge_ml::polars`.
    #[cfg(feature = "data")]
    pub use polars::prelude::{DataFrame, PolarsError, PolarsResult, Series};
}

/// The crate versions this umbrella pins, for `forge doctor` / diagnostics.
pub const STACK: &[(&str, &str)] = &[
    ("ndarray", "0.16.1"),
    #[cfg(feature = "classical")]
    ("millwright", "2.3.1"),
    #[cfg(feature = "data")]
    ("polars", "0.55.2"),
    #[cfg(feature = "deep-learning")]
    ("burn", "0.22.0-pre.3"),
];

#[cfg(test)]
mod tests {
    #[test]
    fn stack_lists_ndarray_and_default_classical() {
        let names: Vec<&str> = super::STACK.iter().map(|(name, _)| *name).collect();
        assert!(names.contains(&"ndarray"));
        #[cfg(feature = "classical")]
        assert!(names.contains(&"millwright"));
    }
}
