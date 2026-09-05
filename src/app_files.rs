//! Project, file, and editor-tab lifecycle: opening projects and files,
//! creating/deleting/saving files, running Cargo/clippy/format, tab close and
//! save-guards, and jump-to navigation. Methods on the shared
//! [`crate::ForgeApp`], split out of `main.rs` to keep the app struct's methods
//! grouped by concern.

use crate::*;
use eframe::egui;

impl crate::ForgeApp {
    pub(crate) fn open_project(&mut self) {
        if self.tabs.iter().any(|tab| tab.dirty) {
            self.pending_unsaved_action = Some(PendingUnsavedAction::OpenProject(None));
            return;
        }
        self.open_project_dialog();
    }

    pub(crate) fn open_project_dialog(&mut self) {
        let Some(root) = rfd::FileDialog::new()
            .set_title("Open Forge ML project")
            .pick_folder()
        else {
            return;
        };
        self.open_project_path(root);
    }

    pub(crate) fn request_open_project_path(&mut self, root: PathBuf) {
        if self.tabs.iter().any(|tab| tab.dirty) {
            self.pending_unsaved_action = Some(PendingUnsavedAction::OpenProject(Some(root)));
        } else {
            self.open_project_path(root);
        }
    }

    pub(crate) fn open_project_path(&mut self, root: PathBuf) {
        match Project::open(root.clone()) {
            Ok(project) => {
                self.console = format!("Opened {}", project.root.display());
                self.project = Some(project);
                self.workspace_store = WorkspaceStore::open(&root).ok();
                privacy_diagnostics::configure(self.diagnostics_opt_in, Some(&root));
                if let Some(store) = &self.workspace_store {
                    self.database_profiles = store.load_connections().unwrap_or_default();
                    self.sql_history = database::bounded_query_history(
                        store.load_query_history().unwrap_or_default(),
                    );
                    self.remote_profiles = store.load_remote_profiles().unwrap_or_default();
                    self.object_profiles = store.load_object_profiles().unwrap_or_default();
                    self.database_selected = 0;
                    if let Ok(runs) = store.load_experiments::<ExperimentRun>() {
                        self.saved_runs = runs;
                    }
                    if let Ok(recovery) = store.load_recovery() {
                        if let Some(height) = recovery.explorer_height {
                            self.explorer_height = height;
                        }
                        if let Some(height) = recovery.dataset_pane_height {
                            self.dataset_pane_height = height;
                        }
                        if let Some(docked) = recovery.dataset_viewer_docked {
                            self.dataset_viewer_docked = docked;
                        }
                        let mut restored_tabs = recovery
                            .open_files
                            .into_iter()
                            .filter_map(|path| {
                                std::fs::read_to_string(&path)
                                    .ok()
                                    .map(|content| EditorTab {
                                        title: file_title(&path),
                                        path: Some(path),
                                        disk_hash: Some(content_hash(&content)),
                                        content,
                                        dirty: false,
                                        external_change_pending: false,
                                    })
                            })
                            .collect::<Vec<_>>();
                        if !restored_tabs.is_empty() {
                            self.active_tab = recovery
                                .active_file
                                .and_then(|active| {
                                    restored_tabs
                                        .iter()
                                        .position(|tab| tab.path.as_ref() == Some(&active))
                                })
                                .unwrap_or(0);
                            self.tabs.clear();
                            self.tabs.append(&mut restored_tabs);
                        }
                    }
                }
                self.recent_projects.retain(|path| path != &root);
                self.recent_projects.insert(0, root);
                self.recent_projects.truncate(10);
                self.last_lsp_hash = 0;
            }
            Err(error) => self.console = format!("Could not open project: {error}"),
        }
    }

    pub(crate) fn create_new_file(&mut self, directory: Option<PathBuf>) {
        let Some(project_root) = self.project.as_ref().map(|project| project.root.clone()) else {
            self.console = "Open a project before creating a file.".to_owned();
            return;
        };
        let directory = directory.unwrap_or_else(|| project_root.clone());
        let Some(path) = rfd::FileDialog::new()
            .set_title("Create file in Forge ML project")
            .set_directory(directory)
            .save_file()
        else {
            return;
        };
        let Some(parent) = path.parent() else {
            self.console = "Choose a valid file location.".to_owned();
            return;
        };
        let inside_project = project_root
            .canonicalize()
            .ok()
            .zip(parent.canonicalize().ok())
            .is_some_and(|(root, parent)| parent.starts_with(root));
        if !inside_project {
            self.console = "New files must be created inside the open project.".to_owned();
            return;
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => {
                if let Some(project) = &mut self.project {
                    let _ = project.refresh();
                }
                self.console = format!("Created {}", path.display());
                self.open_file(path);
                self.editor_needs_initial_focus = true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.console = format!(
                    "{} already exists; no file was overwritten.",
                    path.display()
                );
            }
            Err(error) => {
                self.console = format!("Could not create {}: {error}", path.display());
            }
        }
    }

    pub(crate) fn delete_file(&mut self, path: PathBuf) {
        let Some(project_root) = self.project.as_ref().map(|project| project.root.clone()) else {
            return;
        };
        if self
            .tabs
            .iter()
            .any(|tab| tab.path.as_ref() == Some(&path) && tab.dirty)
        {
            self.console = format!(
                "Save or discard changes in {} before deleting it.",
                path.display()
            );
            return;
        }
        let inside_project = project_root
            .canonicalize()
            .ok()
            .zip(path.canonicalize().ok())
            .is_some_and(|(root, target)| target.starts_with(root) && target.is_file());
        if !inside_project {
            self.console = "Forge only deletes files inside the open project.".to_owned();
            return;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.tabs.retain(|tab| tab.path.as_ref() != Some(&path));
                if self.tabs.is_empty() {
                    self.tabs.push(blank_tab());
                }
                self.active_tab = self.active_tab.min(self.tabs.len() - 1);
                self.lsp_diagnostics.remove(&path);
                if let Some(project) = &mut self.project {
                    let _ = project.refresh();
                }
                self.console = format!("Deleted {}. This action cannot be undone.", path.display());
            }
            Err(error) => self.console = format!("Could not delete {}: {error}", path.display()),
        }
    }

    pub(crate) fn delete_confirmation(&mut self, ctx: &egui::Context) {
        let Some(path) = self.pending_delete.clone() else {
            return;
        };
        egui::Window::new("Delete file?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("Delete {}?", path.display()));
                ui.label(RichText::new("This action cannot be undone.").color(RED));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.pending_delete = None;
                    }
                    if ui.button(RichText::new("Delete file").color(RED)).clicked() {
                        self.pending_delete = None;
                        self.delete_file(path.clone());
                    }
                });
            });
    }

    pub(crate) fn open_file(&mut self, path: PathBuf) {
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.path.as_ref() == Some(&path))
        {
            self.active_tab = index;
            self.selected_cell = 0;
            return;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                self.tabs.push(EditorTab {
                    title: file_title(&path),
                    path: Some(path),
                    disk_hash: Some(content_hash(&content)),
                    content,
                    dirty: false,
                    external_change_pending: false,
                });
                self.active_tab = self.tabs.len() - 1;
                self.selected_cell = 0;
                self.cell_records.clear();
            }
            Err(error) => self.console = format!("Could not open {}: {error}", path.display()),
        }
    }

    pub(crate) fn save_active(&mut self) {
        if self.format_on_save && self.active_is_rust() {
            self.format_document();
        }
        let _ = self.save_tab(self.active_tab);
    }

    pub(crate) fn active_is_rust(&self) -> bool {
        self.active()
            .path
            .as_ref()
            .map(|p| p.extension().is_some_and(|e| e == "rs"))
            .unwrap_or(true)
    }

    /// Format the active buffer with `rustfmt`, replacing its contents on success.
    pub(crate) fn format_document(&mut self) {
        if !self.active_is_rust() {
            self.console = "Format document only applies to Rust files.".to_owned();
            return;
        }
        match run_rustfmt(&self.active().content) {
            Ok(formatted) => {
                if formatted != self.active().content {
                    self.active_mut().content = formatted;
                    self.active_mut().dirty = true;
                    self.cell_records.clear();
                    self.last_lsp_hash = 0;
                }
                self.console = "Formatted with rustfmt.".to_owned();
            }
            Err(error) => self.console = format!("rustfmt failed: {error}"),
        }
    }

    /// Queue a cargo subcommand as a background job and surface it in Studio.
    pub(crate) fn run_cargo_task(&mut self, args: &str) {
        let Some(root) = self.project_root() else {
            self.console = "Open a Cargo project first.".to_owned();
            return;
        };
        match self.job_queue.enqueue(format!("cargo {args}"), root) {
            Ok(id) => {
                self.console = format!("Queued `cargo {args}` as job {id}. See Studio.");
                self.inspector_tab = InspectorTab::Studio;
            }
            Err(error) => self.console = error,
        }
    }

    /// Run clippy and show its findings in the Problems pane.
    pub(crate) fn run_clippy(&mut self) {
        if let Some(project) = &self.project {
            self.diagnostics
                .check(project.root.clone(), diagnostics::Tool::Clippy);
            self.diagnostics_running = true;
            self.diagnostic_lines = vec!["Running cargo clippy...".to_owned()];
            self.inspector_tab = InspectorTab::Problems;
        } else {
            self.diagnostic_lines = vec!["Open a Cargo project to run clippy.".to_owned()];
        }
    }

    pub(crate) fn save_tab(&mut self, index: usize) -> bool {
        let path = self.tabs[index].path.clone().or_else(|| {
            let mut dialog = rfd::FileDialog::new().set_title("Save file");
            if let Some(project) = &self.project {
                dialog = dialog.set_directory(&project.root);
            }
            dialog.save_file()
        });
        let Some(path) = path else {
            return false;
        };
        let content = self.tabs[index].content.clone();
        match std::fs::write(&path, content) {
            Ok(()) => {
                let tab = &mut self.tabs[index];
                tab.path = Some(path.clone());
                tab.title = file_title(&path);
                tab.dirty = false;
                tab.disk_hash = std::fs::read_to_string(&path)
                    .ok()
                    .map(|content| content_hash(&content));
                tab.external_change_pending = false;
                self.console = format!("Saved {}", path.display());
                if let Some(project) = &mut self.project {
                    let _ = project.refresh();
                }
                self.run_diagnostics();
                true
            }
            Err(error) => {
                self.console = format!("Could not save {}: {error}", path.display());
                false
            }
        }
    }

    pub(crate) fn close_tab(&mut self, index: usize) {
        if self.tabs[index].dirty {
            self.pending_unsaved_action = Some(PendingUnsavedAction::CloseTab(index));
            return;
        }
        self.close_tab_now(index);
    }

    pub(crate) fn close_tab_now(&mut self, index: usize) {
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.tabs.push(blank_tab());
        }
        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
        self.selected_cell = 0;
        self.cell_records.clear();
    }

    pub(crate) fn unsaved_confirmation(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_unsaved_action.clone() else {
            return;
        };
        let dirty_names = self
            .tabs
            .iter()
            .filter(|tab| tab.dirty)
            .map(|tab| tab.title.clone())
            .collect::<Vec<_>>();
        egui::Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Save your changes before continuing?");
                ui.label(RichText::new(dirty_names.join(", ")).color(EMBER));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.pending_unsaved_action = None;
                    }
                    if ui.button("Discard").clicked() {
                        self.pending_unsaved_action = None;
                        match action.clone() {
                            PendingUnsavedAction::CloseTab(index) => self.close_tab_now(index),
                            PendingUnsavedAction::OpenProject(Some(path)) => {
                                self.open_project_path(path)
                            }
                            PendingUnsavedAction::OpenProject(None) => self.open_project_dialog(),
                        }
                    }
                    if ui.button("Save").clicked() {
                        let indices = match &action {
                            PendingUnsavedAction::CloseTab(index) => vec![*index],
                            PendingUnsavedAction::OpenProject(_) => self
                                .tabs
                                .iter()
                                .enumerate()
                                .filter_map(|(index, tab)| tab.dirty.then_some(index))
                                .collect(),
                        };
                        if indices.into_iter().all(|index| self.save_tab(index)) {
                            self.pending_unsaved_action = None;
                            match action.clone() {
                                PendingUnsavedAction::CloseTab(index) => self.close_tab_now(index),
                                PendingUnsavedAction::OpenProject(Some(path)) => {
                                    self.open_project_path(path)
                                }
                                PendingUnsavedAction::OpenProject(None) => {
                                    self.open_project_dialog()
                                }
                            }
                        }
                    }
                });
            });
    }

    pub(crate) fn navigate_to(&mut self, path: PathBuf, line: usize, column: usize) {
        self.open_file(path);
        let content = &self.active().content;
        let line_start = content
            .split_inclusive('\n')
            .take(line)
            .map(str::chars)
            .map(Iterator::count)
            .sum::<usize>();
        let line_length = content
            .lines()
            .nth(line)
            .map(str::chars)
            .map(Iterator::count)
            .unwrap_or(0);
        let offset = line_start + column.min(line_length);
        self.pending_editor_selection = Some((offset, offset));
        self.cursor_offset = offset;
        self.editor_needs_initial_focus = true;
    }
}
