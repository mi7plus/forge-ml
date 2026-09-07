//! Project scaffolding templates for `forge new`.
//!
//! `forge new` writes a starter `src/main.rs` and a `README.md` into the fresh
//! project so a newcomer goes from `forge new` to a running project — and on to
//! `forge ide` — without ever invoking cargo directly (the Phase 3 onboarding
//! goal). Dependency-free static templating, kept beside the profile definitions
//! it draws from.

use crate::profiles::Profile;

/// A minimal, always-compiling starter `src/main.rs`. It uses only `ndarray`,
/// which is present in every profile with a stable API, so the generated project
/// builds and runs immediately; real modelling happens interactively in `forge
/// ide`, where the rest of the profile's stack is put to work.
pub fn starter_main(profile: &Profile) -> String {
    format!(
        "//! A Forge ML project (profile: {name}).\n\
         //!\n\
         //! Run it with `forge run`. Open the interactive studio with `forge ide`\n\
         //! to load data, train a model, and ship it — no cargo required.\n\
         \n\
         use ndarray::Array2;\n\
         \n\
         fn main() {{\n\
         \x20   // ndarray ships in every Forge profile; the rest of your `{name}`\n\
         \x20   // stack ({crates}) is in Cargo.toml and ready to `use`.\n\
         \x20   let features = Array2::<f64>::zeros((3, 2));\n\
         \x20   println!(\"Forge ML project ready (profile: {name}).\");\n\
         \x20   println!(\"feature matrix shape: {{:?}}\", features.dim());\n\
         \x20   println!(\"Next: run `forge ide` to load data, train, and ship a model.\");\n\
         }}\n",
        name = profile.name,
        crates = profile.crates.join(", "),
    )
}

/// The project `README.md`: the forge-only workflow, so the reader never needs
/// to reach for raw cargo.
pub fn project_readme(project: &str, profile: &Profile) -> String {
    format!(
        "# {project}\n\
         \n\
         A Forge ML project — {summary}.\n\
         \n\
         Built on the Forge curated Rust stack ({crates}). Cargo and rustup are\n\
         underneath; the `forge` commands below are all you need.\n\
         \n\
         ## First steps\n\
         \n\
         ```\n\
         forge run          # build and run src/main.rs\n\
         forge ide          # open the Forge ML studio on this project\n\
         ```\n\
         \n\
         `forge ide` is where you load data, train (classical or deep), inspect runs\n\
         and plots, and export or serve a model — without writing build files.\n\
         \n\
         ## Environment & reproducibility\n\
         \n\
         ```\n\
         forge env sync            # write forge.lock (pins the environment)\n\
         forge doctor              # check the toolchain, linker, CUDA, Python\n\
         forge reproduce <run-id>  # verify this machine can reproduce a saved run\n\
         ```\n\
         \n\
         ## Add a crate\n\
         \n\
         ```\n\
         forge add <crate>   # cargo add with data-science feature defaults\n\
         ```\n",
        project = project,
        summary = profile.summary,
        crates = profile.crates.join(", "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::PROFILES;

    #[test]
    fn starter_compiles_shape_for_every_profile() {
        for profile in PROFILES {
            let src = starter_main(profile);
            assert!(src.contains("fn main()"));
            assert!(src.contains("use ndarray::Array2;"));
            assert!(src.contains(profile.name));
            // Points the newcomer to the studio, not to build files.
            assert!(src.contains("forge ide"));
        }
    }

    #[test]
    fn readme_is_forge_only_and_named() {
        let profile = Profile::find("classical-ml").unwrap();
        let readme = project_readme("house-prices", profile);
        assert!(readme.starts_with("# house-prices"));
        assert!(readme.contains("forge run"));
        assert!(readme.contains("forge ide"));
        assert!(readme.contains("forge reproduce"));
        assert!(readme.contains(profile.summary));
    }
}
