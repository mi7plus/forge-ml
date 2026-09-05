//! Source-control and dependency inspectors: Git status/commits, the Crates
//! (packages) pane, and the GitHub pane. Methods on the shared [`crate::ForgeApp`].

use crate::result_ext::ResultText;
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
                    .text();
                self.git_conflicts = git::conflicts(&root).unwrap_or_default();
                self.git_branches = git::list_branches(&root).unwrap_or_default();
                if let Some(p) = &mut self.project {
                    p.refresh_git_status();
                }
            }
            if ui.button("History").clicked() {
                self.git_output = git::log(&root, 50).text();
            }
            if ui.button("Diff").clicked() {
                self.git_output = git::diff(&root, false).text();
            }
            if ui.button("Staged diff").clicked() {
                self.git_output = git::diff(&root, true).text();
            }
            if ui.button("Stage all").clicked() {
                self.git_output = git::stage_all(&root).text();
            }
            if ui.button("Unstage all").clicked() {
                self.git_output = git::unstage_all(&root).text();
            }
            if ui.button("Branches").clicked() {
                self.git_branches = git::list_branches(&root).unwrap_or_default();
                self.git_output = git::branches(&root).text();
            }
            if ui.button("Pull").clicked() {
                self.git_output = git::pull(&root).text();
            }
            if ui.button("Push").clicked() {
                self.git_output = git::push(&root).text();
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.git_commit_message)
                    .hint_text("Commit message"),
            );
            if ui.button("Commit").clicked() {
                self.git_output = git::commit(&root, &self.git_commit_message).text();
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.git_branch_name).hint_text("Branch name"));
            if ui.button("Switch").clicked() {
                self.git_output = git::switch(&root, &self.git_branch_name, false).text();
            }
            if ui.button("Create branch").clicked() {
                self.git_output = git::switch(&root, &self.git_branch_name, true).text();
            }
            if ui.button("Delete branch").clicked() {
                self.git_output = git::delete_branch(&root, &self.git_branch_name, false).text();
            }
            if ui
                .button("Force delete")
                .on_hover_text("Delete even if the branch has unmerged commits (-D)")
                .clicked()
            {
                self.git_output = git::delete_branch(&root, &self.git_branch_name, true).text();
            }
        });

        // Selectable branch list with a right-click context menu. Populated by
        // "Refresh"/"Branches"; a left-click selects and mirrors the name into
        // the text field, a right-click offers checkout/merge/delete actions.
        if !self.git_branches.is_empty() {
            /// One deferred action, applied after the borrow of `git_branches` ends.
            enum BranchAction {
                Checkout(String),
                Merge(String),
                Delete(String),
                ForceDelete(String),
            }
            let mut action: Option<BranchAction> = None;
            ui.add_space(2.0);
            ui.label(
                RichText::new("Branches — click to select, right-click for actions")
                    .size(10.0)
                    .color(MUTED),
            );
            egui::ScrollArea::vertical()
                .id_salt("branch_list")
                .max_height(140.0)
                .show(ui, |ui| {
                    for branch in &self.git_branches {
                        let selected =
                            self.git_selected_branch.as_deref() == Some(branch.name.as_str());
                        let icon = if branch.current {
                            egui_phosphor_icons::icons::CHECK_CIRCLE
                        } else if branch.is_remote {
                            egui_phosphor_icons::icons::CLOUD
                        } else {
                            egui_phosphor_icons::icons::GIT_BRANCH
                        };
                        let mut label =
                            RichText::new(format!("{}  {}", icon.as_str(), branch.name));
                        if branch.current {
                            label = label.color(GREEN).strong();
                        } else if branch.is_remote {
                            label = label.color(MUTED);
                        }
                        let response = ui.selectable_label(selected, label);
                        if response.clicked() {
                            self.git_selected_branch = Some(branch.name.clone());
                            self.git_branch_name = branch.name.clone();
                        }
                        response.context_menu(|ui| {
                            ui.label(RichText::new(&branch.name).strong());
                            ui.separator();
                            if branch.current {
                                ui.add_enabled(
                                    false,
                                    egui::Button::new("Checkout (current branch)"),
                                );
                            } else if ui.button("Checkout").clicked() {
                                action = Some(BranchAction::Checkout(branch.name.clone()));
                                ui.close();
                            }
                            if branch.current {
                                ui.add_enabled(false, egui::Button::new("Merge into current"));
                            } else if ui
                                .button("Merge into current branch")
                                .on_hover_text("git merge --no-edit")
                                .clicked()
                            {
                                action = Some(BranchAction::Merge(branch.name.clone()));
                                ui.close();
                            }
                            ui.separator();
                            let deletable = !branch.current && !branch.is_remote;
                            if ui
                                .add_enabled(deletable, egui::Button::new("Delete"))
                                .on_hover_text(if deletable {
                                    "git branch -d (refuses unmerged work)"
                                } else if branch.current {
                                    "Can't delete the checked-out branch"
                                } else {
                                    "Remote branches can't be deleted here"
                                })
                                .clicked()
                            {
                                action = Some(BranchAction::Delete(branch.name.clone()));
                                ui.close();
                            }
                            if ui
                                .add_enabled(deletable, egui::Button::new("Force delete"))
                                .on_hover_text("git branch -D (drops unmerged commits)")
                                .clicked()
                            {
                                action = Some(BranchAction::ForceDelete(branch.name.clone()));
                                ui.close();
                            }
                        });
                    }
                });
            if let Some(action) = action {
                self.git_output = match action {
                    BranchAction::Checkout(name) => git::switch(&root, &name, false),
                    BranchAction::Merge(name) => git::merge(&root, &name),
                    BranchAction::Delete(name) => git::delete_branch(&root, &name, false),
                    BranchAction::ForceDelete(name) => git::delete_branch(&root, &name, true),
                }
                .text();
                // The action may have changed the current branch, the branch set,
                // or (a conflicting merge) left the tree mid-merge.
                self.git_branches = git::list_branches(&root).unwrap_or_default();
                self.git_conflicts = git::conflicts(&root).unwrap_or_default();
                if let Some(p) = &mut self.project {
                    p.refresh_git_status();
                }
            }
        }

        // Merge-conflict mediation: shown only while a merge is in progress.
        if !self.git_conflicts.is_empty() {
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "{} file(s) in conflict — resolve each, then continue the merge",
                    self.git_conflicts.len()
                ))
                .color(EMBER)
                .strong(),
            );
            let mut action: Option<(String, &'static str)> = None;
            for path in &self.git_conflicts {
                ui.horizontal_wrapped(|ui| {
                    ui.code(path);
                    if ui
                        .button("Keep ours")
                        .on_hover_text("git checkout --ours")
                        .clicked()
                    {
                        action = Some((path.clone(), "ours"));
                    }
                    if ui
                        .button("Keep theirs")
                        .on_hover_text("git checkout --theirs")
                        .clicked()
                    {
                        action = Some((path.clone(), "theirs"));
                    }
                    if ui
                        .button("Mark resolved")
                        .on_hover_text("Stage the file as-is after editing it")
                        .clicked()
                    {
                        action = Some((path.clone(), "resolved"));
                    }
                });
            }
            if let Some((path, side)) = action {
                self.git_output = match side {
                    "resolved" => git::mark_resolved(&root, &path),
                    other => git::resolve_conflict(&root, &path, other),
                }
                .text();
                self.git_conflicts = git::conflicts(&root).unwrap_or_default();
            }
            ui.horizontal(|ui| {
                if ui
                    .button("Continue merge")
                    .on_hover_text("Commit the resolved merge (--no-edit)")
                    .clicked()
                {
                    self.git_output = git::merge_continue(&root).text();
                    self.git_conflicts = git::conflicts(&root).unwrap_or_default();
                }
                if ui.button("Abort merge").clicked() {
                    self.git_output = git::merge_abort(&root).text();
                    self.git_conflicts = git::conflicts(&root).unwrap_or_default();
                }
            });
        }

        ui.separator();
        egui::ScrollArea::both().show(ui, |ui| {
            ui.code(if self.git_output.is_empty() {
                "Refresh to inspect repository status, history, and diffs."
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
                        .text();
            }
            if ui.button("Info").clicked() {
                self.package_output = packages::info(&root, &self.package_query).text();
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
                self.package_output = packages::add(&root, &self.package_query).text();
            }
            if ui.button("Remove").clicked() {
                self.package_output = packages::remove(&root, &self.package_query).text();
            }
            if ui.button("Update lockfile").clicked() {
                self.package_output = packages::update(&root).text();
            }
            if ui.button("Dependency tree").clicked() {
                self.package_output = packages::tree(&root, false).text();
            }
            if ui.button("Duplicate versions").clicked() {
                self.package_output = packages::tree(&root, true).text();
            }
            if ui.button("Audit").clicked() {
                self.package_output = packages::audit(&root).text();
            }
            if ui.button("Licenses/metadata").clicked() {
                self.package_output = packages::licenses(&root).text();
            }
            if ui.button("Cargo package check").clicked() {
                self.package_output = publishing::cargo_package(&root).text();
            }
            if ui.button("crates.io dry run").clicked() {
                self.package_output = publishing::cargo_publish_dry_run(&root).text();
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
                        .text()
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
                    .map(|python| publishing::python_build(&root, python).text())
                    .unwrap_or_else(|| "Select a Python runtime first.".into());
            }
            if ui.button("Python smoke test").clicked() {
                self.package_output = self
                    .selected_python
                    .as_ref()
                    .map(|python| publishing::python_smoke_test(&root, python).text())
                    .unwrap_or_else(|| "Select a Python runtime first.".into());
            }
            if ui.button("Release versions").clicked() {
                self.package_output = release::version_report(&root);
            }
            if ui.button("Release provenance preview").clicked() {
                self.package_output = release::checksums(&root).text();
            }
            if ui.button("Generate release workflow").clicked() {
                self.package_output = release::install_workflow(&root).text();
            }
            if ui.button("Packaging preflight").clicked() {
                self.package_output = release::validate_packaging(&root).text();
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
                    updater::check(&root, &self.update_repository, self.update_channel).text();
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
                self.github_output = github::auth_status().text();
            }
            if ui.button("Clone...").clicked() {
                if let Some(destination) = rfd::FileDialog::new().pick_folder() {
                    self.github_output = github::clone(&self.github_input, &destination).text();
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.github_enterprise_host)
                    .hint_text("GitHub Enterprise hostname"),
            );
            if ui.button("Enterprise auth status").clicked() {
                self.github_output =
                    github::enterprise_auth_status(&self.github_enterprise_host).text();
            }
        });
        if let Some(root) = root {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Repository").clicked() {
                    self.github_output = github::repos(&root).text();
                }
                if ui.button("Fork").clicked() {
                    self.github_output = github::fork(&root).text();
                }
                if ui.button("Publish").clicked() {
                    self.github_output = github::publish(&root, &self.github_input).text();
                }
                if ui.button("Pull requests").clicked() {
                    self.github_output = github::prs(&root).text();
                }
                if ui.button("Create PR").clicked() {
                    self.github_output = github::create_pr(&root, &self.github_input).text();
                }
                if ui.button("Issues").clicked() {
                    self.github_output = github::issues(&root).text();
                }
                if ui.button("Create issue").clicked() {
                    self.github_output = github::create_issue(&root, &self.github_input).text();
                }
                if ui.button("Actions").clicked() {
                    self.github_output = github::actions(&root).text();
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
