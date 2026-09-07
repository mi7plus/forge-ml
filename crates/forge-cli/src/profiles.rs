//! The single source of truth for Forge's curated crate **profiles**.
//!
//! A profile is a blessed, compatible set of crates for a kind of work, plus the
//! data-science feature defaults Forge enables when adding each crate. `forge
//! new --profile P` scaffolds a project from a profile; `forge add CRATE` reuses
//! the same per-crate feature defaults. Kept as dependency-free static data so
//! both the CLI and (via `forge_cli::profiles`) `forge_ide` can reason about the
//! same definitions rather than hardcoding crate lists in several places.
//!
//! Note the deliberate split from the `forge_ml` umbrella crate: profiles here
//! list crates to add to a **user's** project (any crates.io crate is fair
//! game), whereas the umbrella only re-exports crates already in Forge's own
//! dependency tree. The two share names, not a mechanism.

/// A curated profile: a named, blessed crate set for one kind of work.
pub struct Profile {
    /// The name used in `forge.toml` and `forge new --profile <name>`.
    pub name: &'static str,
    /// One-line description for help output.
    pub summary: &'static str,
    /// The crates `forge new` adds, in order. Feature defaults come from
    /// [`default_features`], so a crate's features are defined once even when it
    /// appears in several profiles.
    pub crates: &'static [&'static str],
}

/// Every curated profile. The first is the default for `forge new`.
pub const PROFILES: &[Profile] = &[
    Profile {
        name: "classical-ml",
        summary: "Classical machine learning with Millwright (smartcore + linfa backends built in)",
        // Millwright is the classical-ML interface and bundles the smartcore and
        // linfa backends at pinned versions, so the profile does not add those
        // separately — doing so conflicts on smartcore's version.
        crates: &["polars", "ndarray", "millwright"],
    },
    Profile {
        name: "data",
        summary: "Data wrangling and analysis: dataframes, arrays, plotting, and statistics",
        crates: &["polars", "ndarray", "plotters", "statrs"],
    },
    Profile {
        name: "deep-learning",
        summary: "Deep learning with the Burn tensor/autodiff framework",
        crates: &["burn", "ndarray"],
    },
];

/// The default profile for `forge new` when `--profile` is omitted.
pub const DEFAULT_PROFILE: &str = "classical-ml";

impl Profile {
    /// Look up a profile by name.
    pub fn find(name: &str) -> Option<&'static Profile> {
        PROFILES.iter().find(|profile| profile.name == name)
    }
}

/// The profile names, for help text and error messages (e.g.
/// `data | classical-ml | deep-learning`).
pub fn profile_names() -> String {
    let mut names: Vec<&str> = PROFILES.iter().map(|profile| profile.name).collect();
    names.sort_unstable();
    names.join(" | ")
}

/// The data-science feature defaults Forge enables for a crate, whether it is
/// added via a profile (`forge new`) or on its own (`forge add`). Defined once
/// here so every entry point agrees.
pub fn default_features(krate: &str) -> &'static [&'static str] {
    match krate {
        "polars" => &["lazy", "csv", "parquet"],
        "burn" => &["train"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_exists_and_is_curated() {
        let profile = Profile::find(DEFAULT_PROFILE).expect("default profile is defined");
        assert!(profile.crates.contains(&"millwright"));
    }

    #[test]
    fn every_profile_is_findable_and_nonempty() {
        for profile in PROFILES {
            assert!(!profile.crates.is_empty(), "{} has no crates", profile.name);
            assert!(!profile.summary.is_empty(), "{} has no summary", profile.name);
            assert!(Profile::find(profile.name).is_some());
        }
        assert!(Profile::find("bogus").is_none());
    }

    #[test]
    fn feature_defaults_are_single_sourced() {
        assert_eq!(default_features("polars"), ["lazy", "csv", "parquet"]);
        assert_eq!(default_features("burn"), ["train"]);
        assert!(default_features("ndarray").is_empty());
    }

    #[test]
    fn profile_names_lists_all() {
        let names = profile_names();
        for profile in PROFILES {
            assert!(names.contains(profile.name), "{} missing from list", profile.name);
        }
    }
}
