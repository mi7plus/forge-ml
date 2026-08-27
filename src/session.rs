use crate::experiment::ExperimentRun;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_explorer_height() -> f32 {
    280.0
}

fn default_editor_font_size() -> f32 {
    14.0
}

fn default_dataset_pane_height() -> f32 {
    280.0
}

fn default_true() -> bool {
    true
}

fn default_experiment_name() -> String {
    "run_1".to_owned()
}

fn default_comparison_metric() -> String {
    "loss".to_owned()
}

#[derive(Default, Serialize, Deserialize)]
pub struct SessionState {
    pub project_root: Option<PathBuf>,
    pub open_files: Vec<PathBuf>,
    pub active_file: Option<PathBuf>,
    #[serde(default)]
    pub dark_mode: bool,
    #[serde(default = "default_explorer_height")]
    pub explorer_height: f32,
    #[serde(default)]
    pub recent_projects: Vec<PathBuf>,
    #[serde(default = "default_editor_font_size")]
    pub editor_font_size: f32,
    #[serde(default = "default_true")]
    pub caret_blink: bool,
    #[serde(default)]
    pub high_contrast: bool,
    #[serde(default)]
    pub reduced_motion: bool,
    #[serde(default)]
    pub diagnostics_opt_in: bool,
    #[serde(default)]
    pub saved_runs: Vec<ExperimentRun>,
    #[serde(default = "default_experiment_name")]
    pub experiment_name: String,
    #[serde(default = "default_comparison_metric")]
    pub comparison_metric: String,
    #[serde(default = "default_true")]
    pub dataset_viewer_docked: bool,
    #[serde(default = "default_dataset_pane_height")]
    pub dataset_pane_height: f32,
    #[serde(default)]
    pub selected_python: Option<PathBuf>,
    #[serde(default)]
    pub selected_jupyter_kernel: String,
    #[serde(default)]
    pub python_environment_fingerprint: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_sessions_receive_new_defaults() {
        let state: SessionState =
            serde_json::from_str(r#"{"project_root":null,"open_files":[],"active_file":null}"#)
                .unwrap();
        assert_eq!(state.experiment_name, "run_1");
        assert_eq!(state.comparison_metric, "loss");
        assert!(state.dataset_viewer_docked);
        assert_eq!(state.dataset_pane_height, 280.0);
        assert!(!state.high_contrast);
        assert!(!state.reduced_motion);
        assert!(!state.diagnostics_opt_in);
    }
}
