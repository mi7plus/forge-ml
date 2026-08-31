//! Services & cloud inspectors: object storage, deployment/drift monitoring,
//! the SQL database workbench, and the Millwright Studio pane. Methods on the
//! shared [`crate::ForgeApp`].

use crate::ui::theme::*;
use crate::*;
use eframe::egui;
use egui::RichText;

impl crate::ForgeApp {
    pub(crate) fn object_storage_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(root) = self.project_root() else {
            ui.label("Open a project to configure object-storage profiles.");
            return;
        };
        ui.heading("Object storage");
        ui.label(RichText::new("AWS and rclone own authentication. Forge stores profile metadata only; commands and cache downloads are bounded.").color(MUTED));
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.object_name).hint_text("profile name"));
            egui::ComboBox::from_id_salt("object_provider")
                .selected_text(self.object_provider.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.object_provider,
                        object_storage::Provider::S3,
                        "S3 / compatible",
                    );
                    ui.selectable_value(
                        &mut self.object_provider,
                        object_storage::Provider::Rclone,
                        "rclone remote",
                    );
                });
            ui.add(
                egui::TextEdit::singleline(&mut self.object_bucket).hint_text("bucket or remote"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.object_prefix).hint_text("prefix"));
            ui.add(
                egui::TextEdit::singleline(&mut self.object_endpoint)
                    .hint_text("optional HTTPS endpoint"),
            );
            if ui.button("Save profile").clicked() {
                let profile = object_storage::ObjectProfile {
                    name: self.object_name.clone(),
                    provider: self.object_provider,
                    bucket: self.object_bucket.clone(),
                    prefix: self.object_prefix.clone(),
                    endpoint: self.object_endpoint.clone(),
                    credential_hint: if self.object_provider == object_storage::Provider::S3 {
                        "AWS CLI credential chain".into()
                    } else {
                        "rclone configuration".into()
                    },
                };
                self.object_output = profile
                    .validate()
                    .and_then(|()| {
                        self.object_profiles.retain(|p| p.name != profile.name);
                        self.object_profiles.push(profile);
                        self.workspace_store
                            .as_ref()
                            .ok_or_else(|| "Workspace storage unavailable".to_owned())?
                            .save_object_profiles(&self.object_profiles)
                    })
                    .map(|()| "Saved object-storage profile without credentials.".into())
                    .unwrap_or_else(|e| e);
            }
        });
        let mut selected = None;
        for (index, profile) in self.object_profiles.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} · {} · {} / {}",
                    profile.name,
                    profile.provider.label(),
                    profile.bucket,
                    profile.prefix
                ));
                if ui.small_button("Use").clicked() {
                    selected = Some(index);
                }
            });
        }
        if let Some(index) = selected {
            let profile = self.object_profiles[index].clone();
            self.object_name = profile.name;
            self.object_provider = profile.provider;
            self.object_bucket = profile.bucket;
            self.object_prefix = profile.prefix;
            self.object_endpoint = profile.endpoint;
        }
        ui.horizontal_wrapped(|ui| {
            let available = self.integration_pending == 0;
            if ui
                .add_enabled(available, egui::Button::new("Test"))
                .clicked()
            {
                let profile = object_storage::ObjectProfile {
                    name: self.object_name.clone(),
                    provider: self.object_provider,
                    bucket: self.object_bucket.clone(),
                    prefix: self.object_prefix.clone(),
                    endpoint: self.object_endpoint.clone(),
                    credential_hint: String::new(),
                };
                self.object_output = match self
                    .integration_worker
                    .submit(IntegrationRequest::ObjectTest(profile))
                {
                    Ok(()) => {
                        self.integration_pending += 1;
                        "Testing object-storage profile…".into()
                    }
                    Err(error) => error,
                };
            }
            if ui
                .add_enabled(available, egui::Button::new("List objects"))
                .clicked()
            {
                let profile = object_storage::ObjectProfile {
                    name: self.object_name.clone(),
                    provider: self.object_provider,
                    bucket: self.object_bucket.clone(),
                    prefix: self.object_prefix.clone(),
                    endpoint: self.object_endpoint.clone(),
                    credential_hint: String::new(),
                };
                self.object_output =
                    match self
                        .integration_worker
                        .submit(IntegrationRequest::ObjectList {
                            profile,
                            limit: 200,
                        }) {
                        Ok(()) => {
                            self.integration_pending += 1;
                            "Listing objects…".into()
                        }
                        Err(error) => error,
                    };
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.object_key)
                    .hint_text("key relative to prefix"),
            );
            if ui
                .add_enabled(available, egui::Button::new("Download to project cache"))
                .clicked()
            {
                let profile = object_storage::ObjectProfile {
                    name: self.object_name.clone(),
                    provider: self.object_provider,
                    bucket: self.object_bucket.clone(),
                    prefix: self.object_prefix.clone(),
                    endpoint: self.object_endpoint.clone(),
                    credential_hint: String::new(),
                };
                self.object_output =
                    match self
                        .integration_worker
                        .submit(IntegrationRequest::ObjectDownload {
                            profile,
                            key: self.object_key.clone(),
                            root: root.clone(),
                        }) {
                        Ok(()) => {
                            self.integration_pending += 1;
                            "Downloading object to project cache…".into()
                        }
                        Err(error) => error,
                    };
            }
        });
        ui.separator();
        egui::ScrollArea::both().show(ui, |ui| {
            ui.code(&self.object_output);
        });
    }

    pub(crate) fn deployment_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(root) = self.project_root() else {
            ui.label("Open a project to use its local model registry.");
            return;
        };
        ui.heading("Model registry & deployment");
        ui.label(RichText::new("Artifacts remain project-local under .forge/models. Promoting an older version provides rollback without deleting newer versions.").color(MUTED));
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.registry_model).hint_text("model"));
            ui.add(egui::TextEdit::singleline(&mut self.registry_version).hint_text("version"));
            ui.add(egui::TextEdit::singleline(&mut self.registry_format).hint_text("format"));
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.registry_artifact).hint_text("artifact path"),
            );
            if ui.button("Browse").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_file() {
                    self.registry_artifact = path.display().to_string();
                }
            }
            if ui.button("Register").clicked() {
                self.registry_output = model_registry::ModelRegistry::open(&root)
                    .and_then(|registry| {
                        registry.register(
                            &self.registry_model,
                            &self.registry_version,
                            &self.registry_format,
                            Path::new(&self.registry_artifact),
                            Vec::new(),
                        )
                    })
                    .map(|item| {
                        format!(
                            "Registered {} {} ({})",
                            item.model, item.version, item.format
                        )
                    })
                    .unwrap_or_else(|e| e);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.registry_alias).hint_text("alias"));
            if ui.button("Promote / rollback").clicked() {
                self.registry_output = model_registry::ModelRegistry::open(&root)
                    .and_then(|registry| {
                        registry.promote(
                            &self.registry_model,
                            &self.registry_alias,
                            &self.registry_version,
                        )
                    })
                    .map(|()| {
                        format!(
                            "{}:{} now resolves to {}",
                            self.registry_model, self.registry_alias, self.registry_version
                        )
                    })
                    .unwrap_or_else(|e| e);
            }
            if ui.button("Generate Rust service").clicked() {
                self.registry_output = model_registry::ModelRegistry::open(&root)
                    .and_then(|registry| {
                        let version =
                            registry.resolve_version(&self.registry_model, &self.registry_alias)?;
                        let artifact =
                            registry.resolve(&self.registry_model, &self.registry_alias)?;
                        Ok((version, artifact))
                    })
                    .and_then(|(version, artifact)| {
                        model_registry::generate_inference_service(
                            &root,
                            &self.registry_model,
                            &version,
                            &artifact,
                        )
                    })
                    .map(|path| format!("Generated {}", path.display()))
                    .unwrap_or_else(|e| e);
            }
        });
        if let Ok(registry) = model_registry::ModelRegistry::open(&root) {
            if let Ok(versions) = registry.versions(&self.registry_model) {
                egui::Grid::new("model_versions")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Version");
                        ui.strong("Format");
                        ui.strong("Size");
                        ui.strong("SHA-256");
                        ui.strong("Artifact");
                        ui.end_row();
                        for version in versions {
                            ui.label(version.version);
                            ui.label(version.format);
                            ui.label(format!("{} B", version.size_bytes));
                            ui.label(if version.sha256.is_empty() {
                                "legacy".into()
                            } else {
                                version.sha256.chars().take(12).collect::<String>()
                            });
                            ui.label(version.artifact);
                            ui.end_row();
                        }
                    });
            }
        }
        ui.separator();
        ui.strong("Service monitoring");
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "{} service · {} drift events",
                self.service_events.len(),
                self.drift_events.len()
            ));
            if (!self.service_events.is_empty() || !self.drift_events.is_empty())
                && ui.button("Export snapshot").clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("forge-monitoring-snapshot.json")
                    .save_file()
                {
                    self.registry_output =
                        service_monitor::snapshot_json(&self.service_events, &self.drift_events)
                            .and_then(|bytes| {
                                std::fs::write(&path, bytes).map_err(|error| error.to_string())
                            })
                            .map(|()| format!("Exported monitoring snapshot to {}", path.display()))
                            .unwrap_or_else(|error| format!("Monitoring export failed: {error}"));
                }
            }
            if (!self.service_events.is_empty() || !self.drift_events.is_empty())
                && ui.button("Export CSV").clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("forge-deployment-monitoring.csv")
                    .save_file()
                {
                    self.registry_output =
                        export::monitoring_csv(&self.service_events, &self.drift_events, &path)
                            .map(|()| {
                                format!("Exported monitoring CSV to {}", path.display())
                            })
                            .unwrap_or_else(|error| {
                                format!("Monitoring CSV export failed: {error}")
                            });
                }
            }
            if ui.button("Import snapshot").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Forge monitoring JSON", &["json"])
                    .pick_file()
                {
                    self.registry_output = std::fs::read(&path)
                        .map_err(|error| error.to_string())
                        .and_then(|bytes| service_monitor::parse_snapshot(&bytes))
                        .map(|snapshot| {
                            self.service_events = snapshot.service_events;
                            self.drift_events = snapshot.drift_events;
                            format!("Imported monitoring snapshot from {}", path.display())
                        })
                        .unwrap_or_else(|error| format!("Monitoring import failed: {error}"));
                }
            }
            if (!self.service_events.is_empty() || !self.drift_events.is_empty())
                && ui.button("Open monitoring plots").clicked()
            {
                let plots =
                    service_monitor::monitoring_plots(&self.service_events, &self.drift_events);
                let count = plots.len();
                for spec in plots {
                    if let Some(existing) = self
                        .structured_plots
                        .iter_mut()
                        .find(|existing| existing.name == spec.name)
                    {
                        *existing = spec;
                    } else {
                        self.structured_plots.push(spec);
                    }
                }
                self.inspector_tab = InspectorTab::Charts;
                self.console = format!("Opened {count} native monitoring plot(s).");
            }
            if (!self.service_events.is_empty() || !self.drift_events.is_empty())
                && ui.button("HTML report").clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("forge-deployment-monitoring.html")
                    .save_file()
                {
                    self.registry_output = service_monitor::monitoring_report(
                        &self.service_events,
                        &self.drift_events,
                    )
                    .and_then(|report| {
                        std::fs::write(&path, report).map_err(|error| error.to_string())
                    })
                    .map(|()| format!("Exported monitoring report to {}", path.display()))
                    .unwrap_or_else(|error| format!("Monitoring report failed: {error}"));
                }
            }
            if (!self.service_events.is_empty() || !self.drift_events.is_empty())
                && ui.button("PDF report").clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("forge-deployment-monitoring.pdf")
                    .save_file()
                {
                    self.registry_output = service_monitor::monitoring_pdf_lines(
                        &self.service_events,
                        &self.drift_events,
                    )
                    .and_then(|lines| export::write_text_pdf(&path, &lines))
                    .map(|()| format!("Exported monitoring PDF report to {}", path.display()))
                    .unwrap_or_else(|error| format!("Monitoring PDF report failed: {error}"));
                }
            }
            if (!self.service_events.is_empty() || !self.drift_events.is_empty())
                && ui.button("Monitoring bundle").clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("forge-deployment-monitoring.zip")
                    .save_file()
                {
                    self.registry_output =
                        export::monitoring_bundle(&self.service_events, &self.drift_events, &path)
                            .map(|()| format!("Exported monitoring bundle to {}", path.display()))
                            .unwrap_or_else(|error| format!("Monitoring bundle failed: {error}"));
                }
            }
            if ui.button("Import bundle").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Forge monitoring bundle", &["zip"])
                    .pick_file()
                {
                    self.registry_output = export::import_monitoring_bundle(&path)
                        .map(|bundle| {
                            let services = bundle.snapshot.service_events.len();
                            let drift = bundle.snapshot.drift_events.len();
                            let plots = bundle.plots.len();
                            self.service_events = bundle.snapshot.service_events;
                            self.drift_events = bundle.snapshot.drift_events;
                            for spec in bundle.plots {
                                if let Some(existing) = self
                                    .structured_plots
                                    .iter_mut()
                                    .find(|existing| existing.name == spec.name)
                                {
                                    *existing = spec;
                                } else {
                                    self.structured_plots.push(spec);
                                }
                            }
                            format!(
                                "Imported {services} service event(s), {drift} drift event(s), and {plots} plot(s) from {}",
                                path.display()
                            )
                        })
                        .unwrap_or_else(|error| {
                            format!("Monitoring bundle import failed: {error}")
                        });
                }
            }
            if (!self.service_events.is_empty() || !self.drift_events.is_empty())
                && ui.button("Clear monitoring").clicked()
            {
                self.service_events.clear();
                self.drift_events.clear();
                self.registry_output = "Cleared live monitoring events.".into();
            }
        });
        let overview =
            service_monitor::deployment_overview(&self.service_events, &self.drift_events);
        if overview.is_empty() {
            ui.label(
                RichText::new("Run or stream forge_service JSON lines to populate request health.")
                    .color(MUTED),
            );
        } else {
            ui.label(format!("Latest model health ({} shown)", overview.len()));
            egui::ScrollArea::horizontal().show(ui, |ui| {
                egui::Grid::new("deployment_health_overview")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Model");
                        ui.strong("Version");
                        ui.strong("Requests");
                        ui.strong("Errors");
                        ui.strong("p95 ms");
                        ui.strong("Drift");
                        ui.strong("Latest feature");
                        ui.strong("Observed");
                        ui.strong("Mean shift");
                        ui.strong("Scale ratio");
                        ui.end_row();
                        for health in overview {
                            ui.label(health.model);
                            ui.label(health.version);
                            ui.label(
                                health
                                    .requests
                                    .map_or_else(|| "-".into(), |value| value.to_string()),
                            );
                            ui.label(health.error_rate.map_or_else(
                                || "-".into(),
                                |value| format!("{value:.2}% ({})", health.errors.unwrap_or(0)),
                            ));
                            ui.label(
                                health
                                    .p95_ms
                                    .map_or_else(|| "-".into(), |value| format!("{value:.1}")),
                            );
                            ui.colored_label(
                                if health.drift_breaches > 0 {
                                    Color32::from_rgb(214, 126, 44)
                                } else {
                                    GREEN
                                },
                                format!("{} / {}", health.drift_breaches, health.drift_features),
                            );
                            ui.label(health.latest_drift_feature.as_deref().unwrap_or("-"));
                            ui.label(
                                health
                                    .drift_observed
                                    .map_or_else(|| "-".into(), |value| value.to_string()),
                            );
                            ui.label(
                                health
                                    .drift_mean_shift
                                    .map_or_else(|| "-".into(), |value| format!("{value:.3}σ")),
                            );
                            ui.label(
                                health
                                    .drift_scale_ratio
                                    .map_or_else(|| "-".into(), |value| format!("{value:.3}")),
                            );
                            ui.end_row();
                        }
                    });
            });
        }
        ui.separator();
        ui.code(&self.registry_output);
    }

    pub(crate) fn database_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(root) = self.project_root() else {
            ui.label("Open a project to use project-scoped connection profiles.");
            return;
        };
        ui.heading("SQL workbench");
        ui.horizontal_wrapped(|ui| {
            ui.text_edit_singleline(&mut self.database_name);
            egui::ComboBox::from_id_salt("database_kind")
                .selected_text(self.database_kind.label())
                .show_ui(ui, |ui| {
                    for kind in [
                        ConnectionKind::SQLite,
                        ConnectionKind::DuckDb,
                        ConnectionKind::PostgreSql,
                        ConnectionKind::MySql,
                        ConnectionKind::Adbc,
                    ] {
                        ui.selectable_value(&mut self.database_kind, kind, kind.label());
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut self.database_location)
                    .hint_text("file path, DSN, or ADBC driver"),
            );
            ui.add(egui::TextEdit::singleline(&mut self.database_username).hint_text("username"));
            ui.add(
                egui::TextEdit::singleline(&mut self.database_secret)
                    .password(true)
                    .hint_text("password (never saved in project)"),
            );
            if ui.button("Save profile").clicked() {
                let credential_key = format!("{}:{}", root.display(), self.database_name.trim());
                let profile = ConnectionProfile {
                    name: self.database_name.trim().to_owned(),
                    kind: self.database_kind,
                    location: self.database_location.trim().to_owned(),
                    username: self.database_username.trim().to_owned(),
                    credential_key,
                };
                if let Err(error) = database::validate_profile(&profile) {
                    self.sql_output = error;
                    return;
                }
                if !self.database_secret.is_empty() {
                    if let Err(error) =
                        database::store_secret(&profile.credential_key, &self.database_secret)
                    {
                        self.sql_output = format!("Credential store failed: {error}");
                        return;
                    }
                    self.database_secret.clear();
                }
                if let Some(existing) = self
                    .database_profiles
                    .iter_mut()
                    .find(|existing| existing.name == profile.name)
                {
                    *existing = profile;
                } else {
                    self.database_profiles.push(profile);
                }
                if let Some(store) = &self.workspace_store {
                    self.sql_output = store
                        .save_connections(&self.database_profiles)
                        .map(|_| "Connection profile saved without plaintext credentials.".into())
                        .unwrap_or_else(|e| e);
                }
            }
        });
        let mut remove_profile = false;
        ui.horizontal(|ui| {
            let available = self.integration_pending == 0;
            egui::ComboBox::from_id_salt("database_profile")
                .selected_text(
                    self.database_profiles
                        .get(self.database_selected)
                        .map(|profile| profile.name.as_str())
                        .unwrap_or("No profile"),
                )
                .show_ui(ui, |ui| {
                    for (index, profile) in self.database_profiles.iter().enumerate() {
                        ui.selectable_value(
                            &mut self.database_selected,
                            index,
                            format!("{} · {}", profile.name, profile.kind.label()),
                        );
                    }
                });
            if ui
                .add_enabled(available, egui::Button::new("Test"))
                .clicked()
            {
                if let Some(profile) = self.database_profiles.get(self.database_selected) {
                    self.sql_output =
                        match self
                            .integration_worker
                            .submit(IntegrationRequest::DatabaseTest {
                                profile: profile.clone(),
                                root: root.clone(),
                            }) {
                            Ok(()) => {
                                self.integration_pending += 1;
                                "Testing database connection…".into()
                            }
                            Err(error) => error,
                        };
                }
            }
            if ui
                .add_enabled(available, egui::Button::new("Schema"))
                .clicked()
            {
                if let Some(profile) = self.database_profiles.get(self.database_selected) {
                    self.sql_output =
                        match self
                            .integration_worker
                            .submit(IntegrationRequest::DatabaseSchema {
                                profile: profile.clone(),
                                root: root.clone(),
                                dataset_name: format!("{}_schema", profile.name),
                            }) {
                            Ok(()) => {
                                self.integration_pending += 1;
                                "Loading database schema…".into()
                            }
                            Err(error) => error,
                        };
                }
            }
            if ui
                .add_enabled(
                    available && !self.database_profiles.is_empty(),
                    egui::Button::new("Remove profile"),
                )
                .on_hover_text("Remove this project profile and its stored OS credential")
                .clicked()
            {
                remove_profile = true;
            }
            ui.label(format!("ADBC core: {}", database::adbc_marker()));
        });
        if remove_profile {
            if let Some(profile) =
                database::remove_profile(&mut self.database_profiles, &mut self.database_selected)
            {
                self.sql_output = match self
                    .workspace_store
                    .as_ref()
                    .ok_or_else(|| "Workspace storage unavailable".to_owned())
                    .and_then(|store| store.save_connections(&self.database_profiles))
                {
                    Ok(()) => match database::delete_secret(&profile.credential_key) {
                        Ok(()) => format!(
                            "Removed connection profile `{}` and its stored credential.",
                            profile.name
                        ),
                        Err(error) => format!(
                            "Removed profile `{}`, but OS credential cleanup failed: {error}",
                            profile.name
                        ),
                    },
                    Err(error) => {
                        self.database_profiles.push(profile);
                        self.database_selected = self.database_profiles.len() - 1;
                        format!("Could not remove connection profile: {error}")
                    }
                };
            }
        }
        ui.add(
            egui::TextEdit::multiline(&mut self.sql_editor)
                .font(egui::TextStyle::Monospace)
                .desired_rows(6)
                .hint_text("SQL query"),
        );
        if ui
            .add_enabled(
                self.integration_pending == 0,
                egui::Button::new("Run query into data viewer"),
            )
            .clicked()
        {
            if let Some(profile) = self.database_profiles.get(self.database_selected) {
                if let Err(error) = database::validate_query(&self.sql_editor) {
                    self.sql_output = error;
                    return;
                }
                let name = format!("{}_query_{}", profile.name, self.sql_history.len() + 1);
                self.sql_output =
                    match self
                        .integration_worker
                        .submit(IntegrationRequest::DatabaseQuery {
                            profile: profile.clone(),
                            root: root.clone(),
                            dataset_name: name,
                            sql: self.sql_editor.clone(),
                        }) {
                        Ok(()) => {
                            self.integration_pending += 1;
                            "Running query in background…".into()
                        }
                        Err(error) => error,
                    };
            }
        }
        ui.label(&self.sql_output);
        ui.horizontal(|ui| {
            ui.strong(format!("Query history ({})", self.sql_history.len()));
            if !self.sql_history.is_empty() && ui.button("Export JSON").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("forge-query-history.json")
                    .save_file()
                {
                    self.sql_output = serde_json::to_vec_pretty(&self.sql_history)
                        .map_err(|error| error.to_string())
                        .and_then(|bytes| {
                            std::fs::write(&path, bytes).map_err(|error| error.to_string())
                        })
                        .map(|()| format!("Exported query history to {}", path.display()))
                        .unwrap_or_else(|error| format!("Query history export failed: {error}"));
                }
            }
            if !self.sql_history.is_empty() && ui.button("Clear").clicked() {
                self.sql_history.clear();
                self.sql_output = self
                    .workspace_store
                    .as_ref()
                    .map(|store| store.save_query_history(&self.sql_history))
                    .transpose()
                    .map(|_| "Cleared project query history.".to_owned())
                    .unwrap_or_else(|error| format!("Could not clear query history: {error}"));
            }
        });
        ui.collapsing("Recall successful queries", |ui| {
            for query in self.sql_history.iter().rev() {
                if ui
                    .button(RichText::new(query).monospace().size(9.0))
                    .clicked()
                {
                    self.sql_editor = query.clone();
                }
            }
        });
    }

    pub(crate) fn millwright_studio(&mut self, ui: &mut egui::Ui) {
        ui.heading("Millwright Studio");
        ui.horizontal(|ui| {
            ui.label("Pipeline");
            ui.text_edit_singleline(&mut self.pipeline_design.name);
            ui.label("Target");
            ui.text_edit_singleline(&mut self.pipeline_design.target);
        });
        ui.horizontal_wrapped(|ui| {
            for step in [
                PipelineStep::Impute,
                PipelineStep::Standardize,
                PipelineStep::OneHotEncode,
                PipelineStep::RandomForest,
                PipelineStep::LogisticRegression,
                PipelineStep::LinearRegression,
            ] {
                if ui.small_button(format!("+ {}", step.label())).clicked() {
                    self.pipeline_design.steps.push(step);
                }
            }
            if ui.small_button("Remove last").clicked() {
                self.pipeline_design.steps.pop();
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (index, step) in self.pipeline_design.steps.iter().enumerate() {
                ui.label(RichText::new(format!("{}  {}", index + 1, step.label())).color(accent()));
                if index + 1 < self.pipeline_design.steps.len() {
                    ui.label("→");
                }
            }
        });
        if ui.button("Generate Rust notebook cell").clicked() {
            let code = self.pipeline_design.rust_code();
            self.tabs.push(EditorTab {
                title: format!("{}.rs", self.pipeline_design.name),
                path: None,
                content: format!("//# %% generated Millwright pipeline\n{code}"),
                dirty: true,
                disk_hash: None,
                external_change_pending: false,
            });
            self.active_tab = self.tabs.len() - 1;
            self.selected_cell = 0;
        }
        if ui
            .button("Generate native Millwright ONNX export cell")
            .clicked()
        {
            let artifact = format!("models/{}.onnx", self.pipeline_design.name);
            let code = self.pipeline_design.onnx_export_code(&artifact);
            self.tabs.push(EditorTab {
                title: format!("{}_onnx.rs", self.pipeline_design.name),
                path: None,
                content: format!("//# %% generated Millwright ONNX export\n{code}"),
                dirty: true,
                disk_hash: None,
                external_change_pending: false,
            });
            self.active_tab = self.tabs.len() - 1;
            self.selected_cell = 0;
        }
        ui.separator();
        ui.strong("Training progress");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.job_command)
                    .hint_text("background training command"),
            );
            if ui.button("Queue job").clicked() {
                if let Some(root) = self.project_root() {
                    match self.job_queue.enqueue(self.job_command.clone(), root) {
                        Ok(id) => self.console = format!("Queued training job {id}."),
                        Err(error) => self.console = error,
                    }
                }
            }
        });
        let active = self
            .job_queue
            .jobs
            .iter()
            .filter(|job| job.state == JobState::Running)
            .count();
        let queued = self
            .job_queue
            .jobs
            .iter()
            .filter(|job| job.state == JobState::Queued)
            .count();
        ui.label(format!(
            "Workers: {active}/1 active · {queued} queued · ETA {}",
            self.job_queue
                .eta()
                .map(|eta| format!("{:.1}s", eta.as_secs_f64()))
                .unwrap_or_else(|| "calculating".into())
        ));
        for job in self.job_queue.jobs.iter().rev().take(8) {
            ui.collapsing(
                format!(
                    "Job {} · {:?} · {:.1}s · queued {:.1}s ago",
                    job.id,
                    job.state,
                    job.elapsed.as_secs_f64(),
                    job.queued_at.elapsed().as_secs_f64()
                ),
                |ui| {
                    ui.code(&job.command);
                    if !job.output.is_empty() {
                        ui.code(&job.output);
                    }
                },
            );
        }
        if self.training_events.is_empty() {
            ui.label("Emit `forge_training:<json>` from a Rust cell to stream trials, folds, epochs, and scores.");
        }
        ui.horizontal(|ui| {
            ui.label(format!(
                "Training events: {}/{}",
                self.training_events.len(),
                millwright_studio::MAX_TRAINING_EVENTS
            ));
            for (label, extension) in [("Export JSON", "json"), ("Export CSV", "csv")] {
                if !self.training_events.is_empty() && ui.button(label).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name(format!("forge-training-events.{extension}"))
                        .save_file()
                    {
                        let output = if extension == "json" {
                            millwright_studio::training_json(&self.training_events)
                        } else {
                            millwright_studio::training_csv(&self.training_events)
                        };
                        self.console = output
                            .and_then(|bytes| {
                                std::fs::write(&path, bytes).map_err(|error| error.to_string())
                            })
                            .map(|()| format!("Exported training events to {}", path.display()))
                            .unwrap_or_else(|error| format!("Training export failed: {error}"));
                    }
                }
            }
            if !self.training_events.is_empty() && ui.button("Run summary CSV").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("forge-training-runs.csv")
                    .save_file()
                {
                    self.console = export::training_run_csv(&self.training_events, &path)
                        .map(|()| format!("Exported training run summary to {}", path.display()))
                        .unwrap_or_else(|error| {
                            format!("Training run-summary export failed: {error}")
                        });
                }
            }
            if ui.button("Import JSON").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Forge training JSON", &["json"])
                    .pick_file()
                {
                    self.console = std::fs::read(&path)
                        .map_err(|error| error.to_string())
                        .and_then(|bytes| millwright_studio::parse_training_json(&bytes))
                        .map(|events| {
                            let count = events.len();
                            self.training_events = events;
                            format!("Imported {count} training event(s) from {}", path.display())
                        })
                        .unwrap_or_else(|error| format!("Training import failed: {error}"));
                }
            }
            if !self.training_events.is_empty() && ui.button("HTML report").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("forge-training-report.html")
                    .save_file()
                {
                    self.console = millwright_studio::training_report(&self.training_events)
                        .and_then(|report| {
                            std::fs::write(&path, report).map_err(|error| error.to_string())
                        })
                        .map(|()| format!("Exported training report to {}", path.display()))
                        .unwrap_or_else(|error| format!("Training report failed: {error}"));
                }
            }
            if !self.training_events.is_empty() && ui.button("PDF report").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("forge-training-report.pdf")
                    .save_file()
                {
                    self.console = millwright_studio::training_pdf_lines(&self.training_events)
                        .and_then(|lines| export::write_text_pdf(&path, &lines))
                        .map(|()| format!("Exported training PDF to {}", path.display()))
                        .unwrap_or_else(|error| format!("Training PDF failed: {error}"));
                }
            }
            if !self.training_events.is_empty() && ui.button("Training bundle").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("forge-training-bundle.zip")
                    .save_file()
                {
                    self.console = export::training_bundle(&self.training_events, &path)
                        .map(|()| format!("Exported training bundle to {}", path.display()))
                        .unwrap_or_else(|error| format!("Training bundle failed: {error}"));
                }
            }
            if ui.button("Import bundle").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Forge training bundle", &["zip"])
                    .pick_file()
                {
                    self.console = export::import_training_bundle(&path)
                        .map(|bundle| {
                            let events = bundle.events.len();
                            let plots = bundle.plots.len();
                            self.training_events = bundle.events;
                            for spec in bundle.plots {
                                if let Some(existing) = self
                                    .structured_plots
                                    .iter_mut()
                                    .find(|existing| existing.name == spec.name)
                                {
                                    *existing = spec;
                                } else {
                                    self.structured_plots.push(spec);
                                }
                            }
                            format!(
                                "Imported {events} training event(s) and {plots} plot(s) from {}",
                                path.display()
                            )
                        })
                        .unwrap_or_else(|error| format!("Training bundle import failed: {error}"));
                }
            }
            if !self.training_events.is_empty() && ui.button("Open metric plots").clicked() {
                let plots = millwright_studio::training_plots(&self.training_events);
                let count = plots.len();
                for spec in plots {
                    if let Some(existing) = self
                        .structured_plots
                        .iter_mut()
                        .find(|existing| existing.name == spec.name)
                    {
                        *existing = spec;
                    } else {
                        self.structured_plots.push(spec);
                    }
                }
                if count > 0 {
                    self.inspector_tab = InspectorTab::Charts;
                    self.console = format!("Opened {count} native training metric plot(s).");
                } else {
                    self.console = "No plottable training metrics have arrived yet.".into();
                }
            }
            if !self.training_events.is_empty() && ui.button("Clear events").clicked() {
                self.training_events.clear();
                self.console = "Cleared live training events.".into();
            }
        });
        let run_overview = millwright_studio::training_run_overview(&self.training_events);
        if !run_overview.is_empty() {
            ui.label(format!(
                "Latest training runs ({} shown)",
                run_overview.len()
            ));
            egui::ScrollArea::horizontal().show(ui, |ui| {
                egui::Grid::new("training_run_overview")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Job");
                        ui.strong("Run ID");
                        ui.strong("Status");
                        ui.strong("Trials");
                        ui.strong("Epoch");
                        ui.strong("Loss");
                        ui.strong("Metric");
                        ui.strong("Best");
                        ui.end_row();
                        for run in run_overview {
                            ui.label(run.job);
                            ui.label(run.run_id.unwrap_or_else(|| "-".into()));
                            ui.label(run.status);
                            ui.label(format!("{} / {}", run.completed_trials, run.total_trials));
                            ui.label(match (run.epoch, run.total_epochs) {
                                (Some(epoch), Some(total)) => format!("{epoch} / {total}"),
                                _ => "-".into(),
                            });
                            for value in [run.latest_loss, run.latest_metric, run.best_score] {
                                ui.label(
                                    value.map_or_else(|| "-".into(), |value| format!("{value:.6}")),
                                );
                            }
                            ui.end_row();
                        }
                    });
            });
        }
        egui::ScrollArea::vertical()
            .max_height(130.0)
            .show(ui, |ui| {
                for event in self.training_events.iter().rev().take(100) {
                    ui.label(RichText::new(format!("{event:?}")).monospace().size(9.0));
                }
            });
        if !self.leaderboard.is_empty() {
            self.leaderboard.sort_by(|a, b| b.score.total_cmp(&a.score));
            ui.collapsing("AutoML leaderboard", |ui| {
                egui::Grid::new("automl_leaderboard")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("Model");
                        ui.strong("Score");
                        ui.strong("Time");
                        ui.strong("Parameters");
                        ui.end_row();
                        for entry in &self.leaderboard {
                            ui.label(&entry.model);
                            ui.label(format!("{:.5}", entry.score));
                            ui.label(format!("{} ms", entry.duration_ms));
                            ui.label(&entry.parameters);
                            ui.end_row();
                        }
                    });
            });
        }
        ui.collapsing("Evaluation and explainability", |ui| {
            ui.label(format!(
                "Accuracy: {}   RMSE: {}",
                self.evaluation_report
                    .accuracy
                    .map(|v| format!("{v:.4}"))
                    .unwrap_or_else(|| "—".into()),
                self.evaluation_report
                    .rmse
                    .map(|v| format!("{v:.4}"))
                    .unwrap_or_else(|| "—".into())
            ));
            if !self.evaluation_report.confusion.is_empty() {
                egui::Grid::new("confusion_matrix")
                    .striped(true)
                    .show(ui, |ui| {
                        for row in &self.evaluation_report.confusion {
                            for value in row {
                                ui.label(value.to_string());
                            }
                            ui.end_row();
                        }
                    });
            }
            if !self.evaluation_report.roc.is_empty() {
                Plot::new("roc_curve").height(150.0).show(ui, |plot_ui| {
                    plot_ui.line(Line::new(
                        "ROC",
                        PlotPoints::from(self.evaluation_report.roc.clone()),
                    ));
                });
            }
            if !self.evaluation_report.residuals.is_empty() {
                Plot::new("residuals").height(150.0).show(ui, |plot_ui| {
                    let points = self
                        .evaluation_report
                        .residuals
                        .iter()
                        .enumerate()
                        .map(|(i, v)| [i as f64, *v])
                        .collect::<Vec<_>>();
                    plot_ui.line(Line::new("Residuals", PlotPoints::from(points)));
                });
            }
            for (feature, importance) in &self.evaluation_report.feature_importance {
                ui.add(
                    egui::ProgressBar::new(*importance as f32)
                        .text(format!("{feature}  {importance:.3}")),
                );
            }
        });
        if ui.button("Discover Python runtimes").clicked() {
            let runtimes = python_runtime::discover();
            self.python_runtime_output = if runtimes.is_empty() {
                "No Python runtime found.".into()
            } else {
                runtimes
                    .iter()
                    .flat_map(python_runtime::compatibility)
                    .collect::<Vec<_>>()
                    .join("\n")
            };
        }
        if !self.python_runtime_output.is_empty() {
            ui.collapsing("Python runtime compatibility", |ui| {
                ui.code(&self.python_runtime_output);
                ui.label(
                    "Packages remain user-managed; Forge does not bundle Python ML frameworks.",
                );
            });
        }
    }
}
