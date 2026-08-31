//! Source-control and dependency inspectors: Git status/commits, the Crates
//! (packages) pane, and the GitHub pane. Methods on the shared [`crate::ForgeApp`].

use crate::ui::theme::*;
use crate::*;
use eframe::egui;
use egui::RichText;

impl crate::ForgeApp {
    pub(crate) fn git_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(root) = self.project_root() else {
            crate::ui::theme::empty_state(
                ui,
                egui_phosphor_icons::icons::GIT_BRANCH,
                "No project open",
                "Open a project to see Git status, diffs, and commits.",
            );
            return;
        };
        ui.horizontal_wrapped(|ui| {
            if ui.button("Refresh").clicked() {
                self.git_output = git::snapshot(&root)
                    .map(|s| format!("Branch: {}\n{}", s.branch, s.summary))
                    .unwrap_or_else(|e| e);
                if let Some(p) = &mut self.project {
                    p.refresh_git_status();
                }
            }
            if ui.button("Diff").clicked() {
                self.git_output = git::diff(&root, false).unwrap_or_else(|e| e);
            }
            if ui.button("Staged diff").clicked() {
                self.git_output = git::diff(&root, true).unwrap_or_else(|e| e);
            }
            if ui.button("Stage all").clicked() {
                self.git_output = git::stage_all(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Unstage all").clicked() {
                self.git_output = git::unstage_all(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Branches").clicked() {
                self.git_output = git::branches(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Pull").clicked() {
                self.git_output = git::pull(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Push").clicked() {
                self.git_output = git::push(&root).unwrap_or_else(|e| e);
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.git_commit_message)
                    .hint_text("Commit message"),
            );
            if ui.button("Commit").clicked() {
                self.git_output =
                    git::commit(&root, &self.git_commit_message).unwrap_or_else(|e| e);
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.git_branch_name).hint_text("Branch name"));
            if ui.button("Switch").clicked() {
                self.git_output =
                    git::switch(&root, &self.git_branch_name, false).unwrap_or_else(|e| e);
            }
            if ui.button("Create branch").clicked() {
                self.git_output =
                    git::switch(&root, &self.git_branch_name, true).unwrap_or_else(|e| e);
            }
        });
        ui.separator();
        egui::ScrollArea::both().show(ui, |ui| {
            ui.code(if self.git_output.is_empty() {
                "Refresh to inspect repository status."
            } else {
                &self.git_output
            });
        });
    }

    pub(crate) fn packages_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(root) = self.project_root() else {
            crate::ui::theme::empty_state(
                ui,
                egui_phosphor_icons::icons::PACKAGE,
                "No project open",
                "Open a Cargo project to search crates and inspect dependencies.",
            );
            return;
        };
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.package_query)
                    .hint_text("crate or crate@version"),
            );
            if ui.button("Search").clicked() {
                self.package_output =
                    packages::search_registry(&root, &self.package_query, &self.cargo_registry)
                        .unwrap_or_else(|e| e);
            }
            if ui.button("Info").clicked() {
                self.package_output =
                    packages::info(&root, &self.package_query).unwrap_or_else(|e| e);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.cargo_registry)
                    .hint_text("Cargo registry name (blank = crates.io)"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.python_registry)
                    .hint_text("Python registry HTTPS base URL"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Add").clicked() {
                self.package_output =
                    packages::add(&root, &self.package_query).unwrap_or_else(|e| e);
            }
            if ui.button("Remove").clicked() {
                self.package_output =
                    packages::remove(&root, &self.package_query).unwrap_or_else(|e| e);
            }
            if ui.button("Update lockfile").clicked() {
                self.package_output = packages::update(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Dependency tree").clicked() {
                self.package_output = packages::tree(&root, false).unwrap_or_else(|e| e);
            }
            if ui.button("Duplicate versions").clicked() {
                self.package_output = packages::tree(&root, true).unwrap_or_else(|e| e);
            }
            if ui.button("Audit").clicked() {
                self.package_output = packages::audit(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Licenses/metadata").clicked() {
                self.package_output = packages::licenses(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Cargo package check").clicked() {
                self.package_output = publishing::cargo_package(&root).unwrap_or_else(|e| e);
            }
            if ui.button("crates.io dry run").clicked() {
                self.package_output =
                    publishing::cargo_publish_dry_run(&root).unwrap_or_else(|e| e);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("PyPI details").clicked() {
                if self.python_runtimes.is_empty() {
                    self.discover_python_runtimes();
                }
                self.package_output = self
                    .python_runtimes
                    .first()
                    .map(|runtime| {
                        python_runtime::pypi_index(
                            runtime,
                            &self.package_query,
                            &self.python_registry,
                        )
                        .unwrap_or_else(|e| e)
                    })
                    .unwrap_or_else(|| {
                        "No Python runtime is available for secure PyPI HTTPS discovery.".into()
                    });
            }
            if ui.button("Environment tools").clicked() {
                self.package_output = python_runtime::managers().join("\n");
            }
            if ui.button("Python build preview").clicked() {
                self.package_output = self
                    .selected_python
                    .as_ref()
                    .map(|python| publishing::python_build(&root, python).unwrap_or_else(|e| e))
                    .unwrap_or_else(|| "Select a Python runtime first.".into());
            }
            if ui.button("Python smoke test").clicked() {
                self.package_output = self
                    .selected_python
                    .as_ref()
                    .map(|python| {
                        publishing::python_smoke_test(&root, python).unwrap_or_else(|e| e)
                    })
                    .unwrap_or_else(|| "Select a Python runtime first.".into());
            }
            if ui.button("Release versions").clicked() {
                self.package_output = release::version_report(&root);
            }
            if ui.button("Release provenance preview").clicked() {
                self.package_output = release::checksums(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Generate release workflow").clicked() {
                self.package_output = release::install_workflow(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Packaging preflight").clicked() {
                self.package_output = release::validate_packaging(&root).unwrap_or_else(|e| e);
            }
            if ui.button("Performance budgets").clicked() {
                self.package_output = performance::report(&performance::run());
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.update_repository)
                    .hint_text("update repository owner/name"),
            );
            egui::ComboBox::from_id_salt("update_channel")
                .selected_text(self.update_channel.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.update_channel,
                        updater::Channel::Stable,
                        "stable",
                    );
                    ui.selectable_value(&mut self.update_channel, updater::Channel::Beta, "beta");
                });
            if ui.button("Check signed updates").clicked() {
                self.package_output =
                    updater::check(&root, &self.update_repository, self.update_channel)
                        .unwrap_or_else(|e| e);
            }
        });
        ui.separator();
        egui::ScrollArea::both().show(ui, |ui| {
            ui.code(if self.package_output.is_empty() {
                "Search crates.io or inspect this project's dependencies."
            } else {
                &self.package_output
            });
        });
    }

    pub(crate) fn github_inspector(&mut self, ui: &mut egui::Ui) {
        let root = self.project_root();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.github_input).hint_text("owner/repo or title"),
            );
            if ui.button("Auth status").clicked() {
                self.github_output = github::auth_status().unwrap_or_else(|e| e);
            }
            if ui.button("Clone...").clicked() {
                if let Some(destination) = rfd::FileDialog::new().pick_folder() {
                    self.github_output =
                        github::clone(&self.github_input, &destination).unwrap_or_else(|e| e);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.github_enterprise_host)
                    .hint_text("GitHub Enterprise hostname"),
            );
            if ui.button("Enterprise auth status").clicked() {
                self.github_output = github::enterprise_auth_status(&self.github_enterprise_host)
                    .unwrap_or_else(|e| e);
            }
        });
        if let Some(root) = root {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Repository").clicked() {
                    self.github_output = github::repos(&root).unwrap_or_else(|e| e);
                }
                if ui.button("Fork").clicked() {
                    self.github_output = github::fork(&root).unwrap_or_else(|e| e);
                }
                if ui.button("Publish").clicked() {
                    self.github_output =
                        github::publish(&root, &self.github_input).unwrap_or_else(|e| e);
                }
                if ui.button("Pull requests").clicked() {
                    self.github_output = github::prs(&root).unwrap_or_else(|e| e);
                }
                if ui.button("Create PR").clicked() {
                    self.github_output =
                        github::create_pr(&root, &self.github_input).unwrap_or_else(|e| e);
                }
                if ui.button("Issues").clicked() {
                    self.github_output = github::issues(&root).unwrap_or_else(|e| e);
                }
                if ui.button("Create issue").clicked() {
                    self.github_output =
                        github::create_issue(&root, &self.github_input).unwrap_or_else(|e| e);
                }
                if ui.button("Actions").clicked() {
                    self.github_output = github::actions(&root).unwrap_or_else(|e| e);
                }
            });
        } else {
            ui.label("Open a project for repository, PR, issue, and Actions operations.");
        }
        ui.label(RichText::new("Authentication is delegated to GitHub CLI's secure credential store (`gh auth login`).").size(9.0).color(MUTED));
        ui.separator();
        egui::ScrollArea::both().show(ui, |ui| {
            ui.code(if self.github_output.is_empty() {
                "Check authentication or open a repository."
            } else {
                &self.github_output
            });
        });
    }
}
