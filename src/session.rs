use crate::experiment::ExperimentRun;
use crate::plot::PlotSpec;
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

const MAX_SESSION_PLOTS: usize = 128;
const MAX_SESSION_PLOT_BYTES: usize = 16 * 1024 * 1024;

pub fn bounded_plots(plots: &[PlotSpec]) -> Vec<PlotSpec> {
    let mut result = Vec::new();
    let mut bytes = 0usize;
    for plot in plots.iter().take(MAX_SESSION_PLOTS) {
        if plot.validate().is_err() {
            continue;
        }
        let Ok(size) = serde_json::to_vec(plot).map(|value| value.len()) else {
            continue;
        };
        if bytes.saturating_add(size) > MAX_SESSION_PLOT_BYTES {
            break;
        }
        bytes += size;
        result.push(plot.clone());
    }
    result
}

#[derive(Serialize, Deserialize)]
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
    #[serde(default)]
    pub structured_plots: Vec<PlotSpec>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            project_root: None,
            open_files: Vec::new(),
            active_file: None,
            dark_mode: false,
            explorer_height: default_explorer_height(),
            recent_projects: Vec::new(),
            editor_font_size: default_editor_font_size(),
            caret_blink: true,
            high_contrast: false,
            reduced_motion: false,
            diagnostics_opt_in: false,
            saved_runs: Vec::new(),
            experiment_name: default_experiment_name(),
            comparison_metric: default_comparison_metric(),
            dataset_viewer_docked: true,
            dataset_pane_height: default_dataset_pane_height(),
            selected_python: None,
            selected_jupyter_kernel: String::new(),
            python_environment_fingerprint: String::new(),
            structured_plots: Vec::new(),
        }
    }
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

    #[test]
    fn new_sessions_match_persisted_docking_defaults() {
        let mut state = SessionState::default();
        assert!(state.dataset_viewer_docked);
        assert_eq!(state.dataset_pane_height, 280.0);

        state.dataset_viewer_docked = false;
        state.dataset_pane_height = 412.0;
        let restored: SessionState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert!(!restored.dataset_viewer_docked);
        assert_eq!(restored.dataset_pane_height, 412.0);
    }

    #[test]
    fn persisted_plots_are_valid_and_bounded() {
        let plot = PlotSpec {
            version: crate::plot::PLOT_SPEC_VERSION,
            name: "loss".into(),
            kind: crate::plot::PlotKind::Line,
            x_label: String::new(),
            y_label: String::new(),
            series: Vec::new(),
            matrix: Vec::new(),
            x_log: false,
            y_log: false,
        };
        let plots = vec![plot; MAX_SESSION_PLOTS + 10];
        assert_eq!(bounded_plots(&plots).len(), MAX_SESSION_PLOTS);
        let restored: SessionState = serde_json::from_str(
            &serde_json::to_string(&SessionState {
                structured_plots: bounded_plots(&plots),
                ..SessionState::default()
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            bounded_plots(&restored.structured_plots).len(),
            MAX_SESSION_PLOTS
        );
    }
}
