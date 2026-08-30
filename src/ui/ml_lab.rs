//! The Deep-learning / ML-lab inspector: native Burn training, softmax
//! classification, dataset preparation, hyperparameter sweeps, the inference
//! playground, and ONNX import — all methods on the shared [`crate::ForgeApp`].

use crate::ui::theme::*;
use crate::*;
use eframe::egui;
use egui::RichText;

impl crate::ForgeApp {
    pub(crate) fn deep_learning_inspector(&mut self, ui: &mut egui::Ui) {
        let root = self.project_root();
        ui.heading("Deep learning");
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("deep_backend")
                .selected_text(self.deep_backend.label())
                .show_ui(ui, |ui| {
                    for backend in [
                        DeepBackend::Cpu,
                        DeepBackend::Wgpu,
                        DeepBackend::Cuda,
                        DeepBackend::Rocm,
                    ] {
                        ui.selectable_value(&mut self.deep_backend, backend, backend.label());
                    }
                });
            if ui.button("Generate Burn project").clicked() {
                self.sql_output = root
                    .as_ref()
                    .map(|root| {
                        deep_learning::generate_burn_project(root, self.deep_backend)
                            .unwrap_or_else(|e| e)
                    })
                    .unwrap_or_else(|| "Open a project first.".into());
            }
            if ui.button("Test embedded Burn").clicked() {
                self.sql_output = deep_learning::native_burn_self_test();
            }
            if self.burn_training_cancel.is_none()
                && ui.button("Run native Burn training").clicked()
            {
                let data = if self.burn_training_use_dataset {
                    self.selected_native_training_data().map(Some)
                } else {
                    Ok(None)
                };
                let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
                match data.and_then(|data| {
                    self.integration_worker
                        .submit(IntegrationRequest::BurnTraining {
                            backend: self.deep_backend,
                            config: NativeTrainingConfig {
                                epochs: self.burn_training_epochs,
                                learning_rate: self.burn_training_learning_rate,
                                validation_fraction: self.burn_training_validation_fraction,
                                early_stopping_patience: self.early_stopping_patience,
                            },
                            data,
                            cancelled: Arc::clone(&cancelled),
                        })
                }) {
                    Ok(()) => {
                        self.burn_training_cancel = Some(cancelled);
                        self.integration_pending += 1;
                        self.sql_output = format!(
                            "Running embedded Burn training on {} in the background…",
                            self.deep_backend.label()
                        );
                    }
                    Err(error) => {
                        self.sql_output = format!("Could not start Burn training: {error}")
                    }
                }
            }
            if let Some(cancelled) = &self.burn_training_cancel {
                if ui.button("Cancel native training").clicked() {
                    cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                    self.sql_output = "Cancelling embedded Burn training…".into();
                }
            }
            ui.add(
                egui::DragValue::new(&mut self.burn_training_epochs)
                    .range(1..=10_000)
                    .prefix("epochs "),
            );
            ui.add(
                egui::DragValue::new(&mut self.burn_training_learning_rate)
                    .range(0.000_001..=1.0)
                    .speed(0.001)
                    .prefix("lr "),
            );
            ui.add(
                egui::DragValue::new(&mut self.burn_training_validation_fraction)
                    .range(0.0..=0.5)
                    .speed(0.01)
                    .prefix("validation "),
            );
            ui.checkbox(&mut self.burn_training_use_dataset, "use selected dataset");
            if self.burn_training_use_dataset {
                ui.add(
                    egui::TextEdit::singleline(&mut self.burn_training_feature)
                        .desired_width(110.0)
                        .hint_text("feature column"),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.burn_training_target)
                        .desired_width(110.0)
                        .hint_text("target column"),
                );
            }
            ui.add(
                egui::DragValue::new(&mut self.early_stopping_patience)
                    .range(0..=10_000)
                    .prefix("patience "),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.resume_checkpoint)
                    .hint_text("checkpoint to resume"),
            );
        });
        ui.label(format!(
            "Burn {} embedded · Flex CPU, WGPU, training, and metrics compiled into Forge",
            deep_learning::BURN_VERSION
        ));
        ui.label(&self.sql_output);

        ui.separator();
        ui.strong("Dataset preparation");
        ui.label(
            RichText::new("Encode categorical columns, fill missing values, and scale features into a new numeric dataset (uses the feature/target columns below).")
                .size(10.0)
                .color(MUTED),
        );
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.prep_categorical)
                    .desired_width(200.0)
                    .hint_text("categorical columns (comma-separated)"),
            );
            egui::ComboBox::from_id_salt("prep_encoding")
                .selected_text(match self.prep_encoding {
                    prep::Encoding::OneHot => "one-hot",
                    prep::Encoding::Ordinal => "ordinal",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.prep_encoding, prep::Encoding::OneHot, "one-hot");
                    ui.selectable_value(&mut self.prep_encoding, prep::Encoding::Ordinal, "ordinal");
                });
            egui::ComboBox::from_id_salt("prep_missing")
                .selected_text(match self.prep_missing {
                    prep::Missing::DropRows => "drop rows",
                    prep::Missing::Mean => "impute mean",
                    prep::Missing::Zero => "impute zero",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.prep_missing, prep::Missing::DropRows, "drop rows");
                    ui.selectable_value(&mut self.prep_missing, prep::Missing::Mean, "impute mean");
                    ui.selectable_value(&mut self.prep_missing, prep::Missing::Zero, "impute zero");
                });
            egui::ComboBox::from_id_salt("prep_scaling")
                .selected_text(match self.prep_scaling {
                    prep::Scaling::None => "no scaling",
                    prep::Scaling::Standardize => "standardize",
                    prep::Scaling::MinMax => "min-max",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.prep_scaling, prep::Scaling::None, "no scaling");
                    ui.selectable_value(
                        &mut self.prep_scaling,
                        prep::Scaling::Standardize,
                        "standardize",
                    );
                    ui.selectable_value(&mut self.prep_scaling, prep::Scaling::MinMax, "min-max");
                });
            if ui
                .button("Prepare dataset")
                .on_hover_text("Write a new numeric dataset from the feature columns; the target becomes a passthrough column")
                .clicked()
            {
                self.run_data_prep();
            }
        });
        if !self.prep_result.is_empty() {
            ui.label(RichText::new(&self.prep_result).monospace().size(11.0));
        }

        ui.separator();
        ui.strong("Native classification (softmax)");
        ui.label(
            RichText::new("Train a multiclass classifier on the open dataset; see accuracy, per-class F1, and a confusion matrix.")
                .size(10.0)
                .color(MUTED),
        );
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.class_features)
                    .desired_width(200.0)
                    .hint_text("feature columns (comma-separated)"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.class_target)
                    .desired_width(110.0)
                    .hint_text("target column"),
            );
            ui.add(
                egui::DragValue::new(&mut self.class_epochs)
                    .range(1..=100_000)
                    .prefix("epochs "),
            );
            ui.add(
                egui::DragValue::new(&mut self.class_lr)
                    .speed(0.05)
                    .range(0.001..=10.0)
                    .prefix("lr "),
            );
            ui.add(
                egui::DragValue::new(&mut self.class_test_fraction)
                    .speed(0.01)
                    .range(0.0..=0.9)
                    .prefix("test ")
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            );
            if ui.button("Train classifier").clicked() {
                self.train_classifier();
            }
            if ui
                .button("Sweep")
                .on_hover_text("Grid-search lr × epochs and adopt the best by held-out macro-F1")
                .clicked()
            {
                self.run_classifier_sweep();
            }
        });
        if !self.class_result.is_empty() {
            ui.label(RichText::new(&self.class_result).monospace().size(11.0));
        }
        self.classifier_playground(ui);

        ui.separator();
        ui.strong("ONNX inference");
        ui.label(
            RichText::new("Load an external ONNX model and run it inside the IDE (Millwright / tract).")
                .size(10.0)
                .color(MUTED),
        );
        ui.horizontal_wrapped(|ui| {
            if ui.button("Load ONNX model…").clicked() {
                self.load_onnx_model();
            }
            if !self.onnx_model_name.is_empty() {
                ui.label(
                    RichText::new(format!("model: {}", self.onnx_model_name))
                        .size(11.0)
                        .color(CYAN),
                );
            }
        });
        if self.onnx_model.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.onnx_input)
                        .desired_width(220.0)
                        .hint_text("feature values (comma-separated)"),
                );
                if ui.button("Predict row").clicked() {
                    self.predict_onnx_row();
                }
                if ui
                    .button("Predict dataset")
                    .on_hover_text("Score the open dataset using the classification feature columns")
                    .clicked()
                {
                    self.predict_onnx_dataset();
                }
            });
        }
        if !self.onnx_result.is_empty() {
            ui.label(RichText::new(&self.onnx_result).monospace().size(11.0));
        }
        if let Some(artifact) = self.native_burn_artifact.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label("Drift policy");
                ui.add(
                    egui::DragValue::new(&mut self.drift_mean_shift_threshold)
                        .speed(0.05)
                        .prefix("mean σ "),
                );
                ui.add(
                    egui::DragValue::new(&mut self.drift_scale_ratio_lower)
                        .speed(0.05)
                        .prefix("scale min "),
                );
                ui.add(
                    egui::DragValue::new(&mut self.drift_scale_ratio_upper)
                        .speed(0.05)
                        .prefix("scale max "),
                );
            });
            ui.label(format!(
                "Fitted {} = {:.6} × {} + {:.6} · best score {:.6} · {} epoch(s)",
                artifact.target,
                artifact.slope,
                artifact.feature,
                artifact.intercept,
                artifact.best_score,
                artifact.epochs_completed
            ));
            ui.label(format!(
                "Schema {} · backend {} · rows {} (train {}, validation {}) · data SHA {}",
                artifact.schema,
                if artifact.backend.is_empty() {
                    "legacy"
                } else {
                    &artifact.backend
                },
                artifact.rows,
                artifact.training_rows,
                artifact.validation_rows,
                if artifact.data_sha256.is_empty() {
                    "unavailable"
                } else {
                    artifact
                        .data_sha256
                        .get(..12)
                        .unwrap_or(&artifact.data_sha256)
                }
            ));
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut self.native_burn_inference_feature)
                        .speed(0.1)
                        .prefix(format!("{} ", artifact.feature)),
                );
                if ui.button("Predict").clicked() {
                    self.sql_output = artifact
                        .predict(self.native_burn_inference_feature)
                        .map(|prediction| format!("{} = {prediction:.8}", artifact.target))
                        .unwrap_or_else(|error| error);
                }
                if ui.button("Export model JSON…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("native-burn-regression.json")
                        .add_filter("JSON", &["json"])
                        .save_file()
                    {
                        self.sql_output = export::native_regression_artifact(&artifact, &path)
                            .map(|()| format!("Exported native model to {}", path.display()))
                            .unwrap_or_else(|error| format!("Model export failed: {error}"));
                    }
                }
                if ui.button("Export model card…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("native-burn-model-card.html")
                        .add_filter("HTML", &["html"])
                        .save_file()
                    {
                        let policy = deep_learning::DriftPolicy {
                            mean_shift_threshold: self.drift_mean_shift_threshold,
                            scale_ratio_lower: self.drift_scale_ratio_lower,
                            scale_ratio_upper: self.drift_scale_ratio_upper,
                        };
                        self.sql_output =
                            export::native_regression_model_card(&artifact, policy, &path)
                                .map(|()| {
                                    format!("Exported native model card to {}", path.display())
                                })
                                .unwrap_or_else(|error| {
                                    format!("Model-card export failed: {error}")
                                });
                    }
                }
                if ui.button("Predict selected dataset").clicked() {
                    let selected = self
                        .selected_dataset_info()
                        .ok_or_else(|| "Select a table dataset in the Data viewer first".to_owned())
                        .and_then(|(name, is_table)| {
                            if !is_table {
                                return Err(
                                    "Native batch inference requires a table dataset".into()
                                );
                            }
                            self.data
                                .tables
                                .get(&name)
                                .map(|dataset| (name, Arc::clone(&dataset.table)))
                                .ok_or_else(|| "The selected dataset no longer exists".into())
                        });
                    match selected.and_then(|(dataset_name, table)| {
                        let drift_policy = deep_learning::DriftPolicy {
                            mean_shift_threshold: self.drift_mean_shift_threshold,
                            scale_ratio_lower: self.drift_scale_ratio_lower,
                            scale_ratio_upper: self.drift_scale_ratio_upper,
                        }
                        .validate()?;
                        self.integration_worker.submit(
                            IntegrationRequest::NativeRegressionPredict {
                                artifact: artifact.clone(),
                                dataset_name,
                                table,
                                drift_policy,
                            },
                        )
                    }) {
                        Ok(()) => {
                            self.integration_pending += 1;
                            self.sql_output = "Running native batch inference…".into();
                        }
                        Err(error) => self.sql_output = error,
                    }
                }
            });
        }
        if ui.button("Import native model JSON…").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("JSON", &["json"])
                .pick_file()
            {
                match export::import_native_regression_artifact(&path) {
                    Ok(artifact) => {
                        self.sql_output = format!(
                            "Imported native regression model for {} → {}.",
                            artifact.feature, artifact.target
                        );
                        self.native_burn_artifact = Some(artifact);
                    }
                    Err(error) => self.sql_output = format!("Model import failed: {error}"),
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Registry");
            ui.add(
                egui::TextEdit::singleline(&mut self.registry_model)
                    .desired_width(100.0)
                    .hint_text("model"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.registry_version)
                    .desired_width(80.0)
                    .hint_text("version"),
            );
            if ui
                .add_enabled(
                    self.native_burn_artifact.is_some(),
                    egui::Button::new("Register native model"),
                )
                .clicked()
            {
                self.sql_output = root
                    .as_ref()
                    .ok_or_else(|| "Open a project first".to_owned())
                    .and_then(|root| model_registry::ModelRegistry::open(root))
                    .and_then(|registry| {
                        registry.register_native_regression(
                            &self.registry_model,
                            &self.registry_version,
                            self.native_burn_artifact.as_ref().expect("button enabled"),
                            vec!["native-burn".into(), "regression".into()],
                        )
                    })
                    .map(|version| {
                        self.registry_format = version.format.clone();
                        format!(
                            "Registered native model {} {} · {} bytes · SHA-256 {}",
                            version.model, version.version, version.size_bytes, version.sha256
                        )
                    })
                    .unwrap_or_else(|error| format!("Native model registration failed: {error}"));
            }
            if ui.button("Load registry version").clicked() {
                match root
                    .as_ref()
                    .ok_or_else(|| "Open a project first".to_owned())
                    .and_then(|root| model_registry::ModelRegistry::open(root))
                    .and_then(|registry| {
                        registry
                            .load_native_regression(&self.registry_model, &self.registry_version)
                    }) {
                    Ok(artifact) => {
                        self.sql_output = format!(
                            "Loaded integrity-verified native model {} {}.",
                            self.registry_model, self.registry_version
                        );
                        self.native_burn_artifact = Some(artifact);
                    }
                    Err(error) => self.sql_output = format!("Native registry load failed: {error}"),
                }
            }
        });
        let memory = if self.resource_snapshot.total_memory == 0 {
            0.0
        } else {
            self.resource_snapshot.used_memory as f64 / self.resource_snapshot.total_memory as f64
                * 100.0
        };
        ui.horizontal(|ui| {
            ui.label(format!("CPU {:.1}%", self.resource_snapshot.cpu_percent));
            ui.label(format!("RAM {:.1}%", memory));
            ui.label(&self.resource_snapshot.gpu);
        });
        if let Some(event) = self.training_events.iter().rev().find_map(|event| {
            if let TrainingEvent::Epoch {
                epoch,
                total,
                loss,
                metric,
            } = event
            {
                Some((*epoch, *total, *loss, *metric))
            } else {
                None
            }
        }) {
            ui.add(
                egui::ProgressBar::new(event.0 as f32 / event.1.max(1) as f32).text(format!(
                    "epoch {}/{} · loss {:.5}{}",
                    event.0,
                    event.1,
                    event.2,
                    event
                        .3
                        .map(|value| format!(" · metric {value:.5}"))
                        .unwrap_or_default()
                )),
            );
        }
        if let Some((batch, total, loss, throughput)) =
            self.training_events.iter().rev().find_map(|event| {
                if let TrainingEvent::Batch {
                    batch,
                    total,
                    loss,
                    samples_per_second,
                    ..
                } = event
                {
                    Some((*batch, *total, *loss, *samples_per_second))
                } else {
                    None
                }
            })
        {
            ui.add(
                egui::ProgressBar::new(batch as f32 / total.max(1) as f32).text(format!(
                    "batch {batch}/{total} · loss {loss:.5} · {throughput:.1} samples/s"
                )),
            );
        }
        for checkpoint in &self.deep_outputs.checkpoints {
            ui.label(format!("Checkpoint: {checkpoint}"));
        }
        if let Some(model) = &self.deep_outputs.model {
            ui.collapsing(
                format!(
                    "Model summary · {} · {} parameters",
                    model.name, model.parameters
                ),
                |ui| {
                    egui::Grid::new("model_summary")
                        .striped(true)
                        .show(ui, |ui| {
                            for (name, shape, parameters) in &model.layers {
                                ui.label(name);
                                ui.label(shape);
                                ui.label(parameters.to_string());
                                ui.end_row();
                            }
                        });
                },
            );
        }
        ui.collapsing("Tensors", |ui| {
            for tensor in &self.deep_outputs.tensors {
                ui.label(format!(
                    "{} {:?} · {} values",
                    tensor.name,
                    tensor.shape,
                    tensor.values.len()
                ));
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.monospace(
                        tensor
                            .values
                            .iter()
                            .take(256)
                            .map(|value| format!("{value:.4}"))
                            .collect::<Vec<_>>()
                            .join("  "),
                    );
                });
            }
        });
        ui.collapsing("Images", |ui| {
            for image in &self.deep_outputs.images {
                ui.label(format!("{} · {}×{}", image.name, image.width, image.height));
                if image.rgba.len() == image.width * image.height * 4 {
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width, image.height],
                        &image.rgba,
                    );
                    let texture = ui.ctx().load_texture(
                        format!("deep_image_{}", image.name),
                        color,
                        Default::default(),
                    );
                    let size = texture.size_vec2();
                    ui.image((texture.id(), size));
                }
            }
        });
        ui.collapsing("Embeddings", |ui| {
            for embedding in &self.deep_outputs.embeddings {
                Plot::new(format!("embedding_{}", embedding.name))
                    .height(180.0)
                    .show(ui, |plot_ui| {
                        plot_ui.points(
                            Points::new(
                                &embedding.name,
                                PlotPoints::from(embedding.points.clone()),
                            )
                            .radius(3.0),
                        );
                    });
            }
        });
        ui.collapsing("Predictions", |ui| {
            for prediction in &self.deep_outputs.predictions {
                ui.label(&prediction.name);
                for (label, probability) in prediction.labels.iter().zip(&prediction.probabilities)
                {
                    ui.add(
                        egui::ProgressBar::new(*probability as f32)
                            .text(format!("{label} · {probability:.3}")),
                    );
                }
            }
        });
        ui.separator();
        ui.strong("Remote execution");
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.remote_name).hint_text("profile name"));
            ui.add(egui::TextEdit::singleline(&mut self.remote_url).hint_text("Jupyter URL"));
            ui.add(
                egui::TextEdit::singleline(&mut self.remote_token)
                    .password(true)
                    .hint_text("token"),
            );
            if ui.button("Save remote").clicked() {
                if let Some(root) = &root {
                    let profile = remote::RemoteProfile {
                        name: self.remote_name.clone(),
                        jupyter_url: self.remote_url.clone(),
                        agent_command: self.remote_command.clone(),
                        credential_key: format!("remote:{}:{}", root.display(), self.remote_name),
                    };
                    match remote::validate_profile(&profile) {
                        Ok(()) => {
                            let token_result = if self.remote_token.is_empty() {
                                Ok(())
                            } else {
                                remote::store_token(&profile, &self.remote_token)
                            };
                            match token_result {
                                Ok(()) => {
                                    self.remote_token.clear();
                                    self.remote_profiles
                                        .retain(|existing| existing.name != profile.name);
                                    self.remote_profiles.push(profile);
                                    if let Some(store) = &self.workspace_store {
                                        let _ = store.save_remote_profiles(&self.remote_profiles);
                                    }
                                    self.sql_output = "Saved validated remote profile.".into();
                                }
                                Err(error) => {
                                    self.sql_output =
                                        format!("Could not store remote credential: {error}");
                                }
                            }
                        }
                        Err(error) => self.sql_output = error,
                    }
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.remote_command)
                    .hint_text("remote training command"),
            );
            if ui.button("Generate Actions training").clicked() {
                self.sql_output = root
                    .as_ref()
                    .map(|root| remote::generate_actions_workflow(root).unwrap_or_else(|e| e))
                    .unwrap_or_else(|| "Open a project first.".into());
            }
            if ui.button("Dispatch Actions training").clicked() {
                self.sql_output = root
                    .as_ref()
                    .map(|root| {
                        github::dispatch_training(root, &self.remote_command).unwrap_or_else(|e| e)
                    })
                    .unwrap_or_else(|| "Open a project first.".into());
            }
            if ui.button("Retrieve artifacts").clicked() {
                self.sql_output = root
                    .as_ref()
                    .map(|root| github::download_artifacts(root).unwrap_or_else(|e| e))
                    .unwrap_or_else(|| "Open a project first.".into());
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Remote kernelspec");
            ui.add(
                egui::TextEdit::singleline(&mut self.remote_kernel_name)
                    .hint_text("for example: python3 or rust"),
            );
            if let Some(session) = &self.remote_kernel_session {
                ui.label(format!(
                    "Active: {} · {} · {}",
                    session.profile.name, session.name, session.id
                ));
                if ui
                    .add_enabled(
                        self.integration_pending == 0,
                        egui::Button::new("Stop remote kernel"),
                    )
                    .clicked()
                {
                    match self
                        .integration_worker
                        .submit(IntegrationRequest::RemoteKernelStop(session.clone()))
                    {
                        Ok(()) => {
                            self.integration_pending += 1;
                            self.sql_output = "Stopping remote kernel…".into();
                        }
                        Err(error) => self.sql_output = error,
                    }
                }
                let can_interrupt = self.remote_execution_pending && !self.remote_interrupt_pending;
                if ui
                    .add_enabled(can_interrupt, egui::Button::new("Interrupt execution"))
                    .clicked()
                {
                    match self
                        .integration_worker
                        .submit(IntegrationRequest::RemoteKernelInterrupt(session.clone()))
                    {
                        Ok(()) => {
                            self.integration_pending += 1;
                            self.remote_interrupt_pending = true;
                            self.remote_input_sender = None;
                            self.remote_input_prompt = None;
                            self.remote_input_response.clear();
                            self.sql_output = "Interrupting remote execution…".into();
                        }
                        Err(error) => self.sql_output = error,
                    }
                }
            }
        });
        ui.add_enabled_ui(self.remote_kernel_session.is_some(), |ui| {
            ui.checkbox(
                &mut self.remote_notebook_execution,
                "Run notebook cells on active remote kernel",
            )
            .on_hover_text(
                "Routes Run Cell, Run Above, and Run All through the managed Jupyter kernel",
            );
        });
        ui.add(
            egui::TextEdit::multiline(&mut self.remote_code)
                .desired_rows(4)
                .hint_text("Code for the active remote kernel"),
        );
        if ui
            .add_enabled(
                self.integration_pending == 0 && self.remote_kernel_session.is_some(),
                egui::Button::new("Run on remote kernel"),
            )
            .clicked()
        {
            if let Some(session) = self.remote_kernel_session.clone() {
                let (input_tx, input_rx) = mpsc::channel();
                match self
                    .integration_worker
                    .submit(IntegrationRequest::RemoteExecute {
                        session,
                        code: self.remote_code.clone(),
                        cell_id: None,
                        input: input_rx,
                    }) {
                    Ok(()) => {
                        self.remote_input_sender = Some(input_tx);
                        self.integration_pending += 1;
                        self.remote_execution_pending = true;
                        self.remote_mime_outputs.clear();
                        self.sql_output = "Running code on remote kernel…".into();
                    }
                    Err(error) => self.sql_output = error,
                }
            }
        }
        for profile in self.remote_profiles.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} · {} · {}",
                    profile.name, profile.jupyter_url, profile.agent_command
                ));
                if ui
                    .add_enabled(
                        self.integration_pending == 0,
                        egui::Button::new("Test Jupyter"),
                    )
                    .clicked()
                {
                    match self
                        .integration_worker
                        .submit(IntegrationRequest::RemoteTest(profile.clone()))
                    {
                        Ok(()) => {
                            self.integration_pending += 1;
                            self.sql_output = format!("Testing remote `{}`…", profile.name);
                        }
                        Err(error) => self.sql_output = error,
                    }
                }
                if ui
                    .add_enabled(
                        self.integration_pending == 0 && self.remote_kernel_session.is_none(),
                        egui::Button::new("Start kernel"),
                    )
                    .clicked()
                {
                    match self
                        .integration_worker
                        .submit(IntegrationRequest::RemoteKernelStart {
                            profile: profile.clone(),
                            kernel_name: self.remote_kernel_name.trim().to_owned(),
                        }) {
                        Ok(()) => {
                            self.integration_pending += 1;
                            self.sql_output = format!(
                                "Starting `{}` on remote `{}`…",
                                self.remote_kernel_name.trim(),
                                profile.name
                            );
                        }
                        Err(error) => self.sql_output = error,
                    }
                }
            });
        }
        if !self.remote_mime_outputs.is_empty() {
            ui.collapsing(
                format!("Remote rich output ({})", self.remote_mime_outputs.len()),
                |ui| {
                    for output in &self.remote_mime_outputs {
                        ui.label(RichText::new(&output.mime).strong().color(CYAN));
                        egui::ScrollArea::horizontal()
                            .max_height(160.0)
                            .show(ui, |ui| {
                                ui.label(RichText::new(&output.data).monospace());
                            });
                    }
                },
            );
        }
    }

    /// Train a native softmax classifier on the selected dataset and open its
    /// confusion matrix. Feature columns are comma-separated.
    fn train_classifier(&mut self) {
        let result = (|| -> Result<(String, plot::PlotSpec, classification::Classifier), String> {
            let (name, is_table) = self
                .selected_dataset_info()
                .ok_or("Select a table dataset in the Data viewer first")?;
            if !is_table {
                return Err("Classification requires a table dataset".into());
            }
            let table = &self
                .data
                .tables
                .get(&name)
                .ok_or("The selected dataset no longer exists")?
                .table;
            let features: Vec<String> = self
                .class_features
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            let data = classification::prepare(table, &features, self.class_target.trim())?;
            let rows = data.features.len();
            let classes = data.class_names.len();
            let (train, test) = data.split(self.class_test_fraction);
            let train = if train.features.len() >= 2 { train } else { data.clone() };
            let model = classification::Classifier::train(&train, self.class_epochs, self.class_lr)?;
            let (eval_set, eval_label) = if !test.features.is_empty() {
                (&test, format!("held-out test ({} rows)", test.features.len()))
            } else {
                (&train, "training set".to_owned())
            };
            let metrics = model.evaluate(eval_set);
            let per_class = model
                .classes
                .iter()
                .zip(&metrics.per_class)
                .map(|(name, m)| {
                    format!(
                        "  {name}: precision {:.3}, recall {:.3}, F1 {:.3} (n={})",
                        m.precision, m.recall, m.f1, m.support
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let summary = format!(
                "Softmax classifier on `{name}` — {rows} rows, {classes} classes, {} features.\nEvaluated on {eval_label}: accuracy {:.3} · macro-F1 {:.3}\n{per_class}",
                data.feature_names.len(),
                metrics.accuracy,
                metrics.macro_f1
            );
            let plot = classification::confusion_plot(&metrics, &format!("{name} confusion"));
            plot.validate()?;
            Ok((summary, plot, model))
        })();
        match result {
            Ok((summary, plot, model)) => {
                self.class_result = summary;
                self.class_playground = model.baseline();
                self.class_model = Some(model);
                if let Some(existing) = self
                    .structured_plots
                    .iter_mut()
                    .find(|p| p.name == plot.name)
                {
                    *existing = plot;
                } else {
                    self.structured_plots.push(plot);
                }
                self.inspector_tab = InspectorTab::Charts;
            }
            Err(error) => self.class_result = error,
        }
    }

    /// Interactive single-example prediction: one input per feature, with live
    /// per-class probabilities from the last-trained classifier.
    fn classifier_playground(&mut self, ui: &mut egui::Ui) {
        let Some(model) = self.class_model.as_ref() else {
            return;
        };
        let features = model.feature_names.clone();
        if self.class_playground.len() != features.len() {
            self.class_playground = model.baseline();
            if self.class_playground.len() != features.len() {
                self.class_playground = vec![0.0; features.len()];
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.strong("Inference playground");
            if ui
                .small_button("Reset to average")
                .on_hover_text("Restore each feature to its training mean")
                .clicked()
            {
                self.class_playground = model.baseline();
            }
        });
        ui.label(
            RichText::new("Adjust the feature values to see live class probabilities from the trained classifier.")
                .size(10.0)
                .color(MUTED),
        );
        egui::Grid::new("forge_class_playground_inputs")
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for (i, name) in features.iter().enumerate() {
                    ui.label(RichText::new(name).size(11.0));
                    ui.add(egui::DragValue::new(&mut self.class_playground[i]).speed(0.1));
                    ui.end_row();
                }
            });
        let proba = model.predict_proba(&self.class_playground);
        let best = proba
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);
        if let Some(class) = model.classes.get(best) {
            ui.label(
                RichText::new(format!("Predicted: {class} ({:.1}%)", proba[best] * 100.0))
                    .strong()
                    .color(CYAN),
            );
        }
        for (class, &p) in model.classes.iter().zip(&proba) {
            ui.add(
                egui::ProgressBar::new(p as f32)
                    .desired_width(260.0)
                    .text(RichText::new(format!("{class}  {:.1}%", p * 100.0)).size(10.0)),
            );
        }
    }

    /// Encode, impute, and scale the selected dataset's feature columns into a
    /// new fully numeric table, inserted alongside the source for training.
    fn run_data_prep(&mut self) {
        let result = (|| -> Result<(String, prep::PrepReport), String> {
            let (name, is_table) = self
                .selected_dataset_info()
                .ok_or("Select a table dataset in the Data viewer first")?;
            if !is_table {
                return Err("Dataset preparation requires a table dataset".into());
            }
            let table = self
                .data
                .tables
                .get(&name)
                .ok_or("The selected dataset no longer exists")?
                .table
                .as_ref()
                .clone();
            let split = |text: &str| {
                text.split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            };
            let passthrough = split(&self.class_target);
            let config = prep::PrepConfig {
                feature_columns: split(&self.class_features),
                categorical_columns: split(&self.prep_categorical),
                encoding: self.prep_encoding,
                missing: self.prep_missing,
                scaling: self.prep_scaling,
                passthrough,
            };
            let (prepared, report) = prep::transform(&table, &config)?;
            let new_name = format!("{name} · prepared");
            let dataset = data::Dataset::from_table(prepared, Some(format!("prep::{name}")))
                .map_err(|e| format!("Could not build prepared dataset: {e}"))?;
            self.data.tables.insert(new_name.clone(), dataset);
            Ok((new_name, report))
        })();
        match result {
            Ok((new_name, report)) => {
                self.prep_result = format!("Saved `{new_name}`.\n{}", report.summary());
                self.console = format!("Prepared dataset saved as `{new_name}`.");
            }
            Err(error) => self.prep_result = error,
        }
    }

    /// Load an ONNX model from disk for in-IDE inference (via Millwright/tract).
    fn load_onnx_model(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("ONNX model", &["onnx"])
            .set_title("Load ONNX model")
            .pick_file()
        else {
            return;
        };
        match millwright::onnx::InferenceModel::load(&path) {
            Ok(model) => {
                self.onnx_model = Some(model);
                self.onnx_model_name = file_title(&path);
                self.onnx_result = format!("Loaded `{}`. Ready for inference.", self.onnx_model_name);
            }
            Err(error) => {
                self.onnx_model = None;
                self.onnx_result = format!("Could not load ONNX model: {error}");
            }
        }
    }

    /// Run the loaded ONNX model on one comma-separated feature row.
    fn predict_onnx_row(&mut self) {
        let Some(model) = self.onnx_model.as_ref() else {
            self.onnx_result = "Load an ONNX model first.".to_owned();
            return;
        };
        let values: Vec<f64> = self
            .onnx_input
            .split(',')
            .filter_map(|token| token.trim().parse::<f64>().ok())
            .collect();
        if values.is_empty() {
            self.onnx_result = "Enter comma-separated numeric feature values.".to_owned();
            return;
        }
        let columns: Vec<String> = (0..values.len()).map(|i| format!("f{i}")).collect();
        self.onnx_result = match millwright::frame::Frame::from_rows(vec![values], columns)
            .and_then(|frame| model.predict(&frame))
        {
            Ok(prediction) => format!("ONNX prediction: {prediction:?}"),
            Err(error) => format!("Inference failed: {error}"),
        };
    }

    /// Run the loaded ONNX model over the selected dataset's feature columns
    /// (reusing the classifier's feature-column list) and summarize predictions.
    fn predict_onnx_dataset(&mut self) {
        let Some(model) = self.onnx_model.as_ref() else {
            self.onnx_result = "Load an ONNX model first.".to_owned();
            return;
        };
        let result = (|| -> Result<String, String> {
            let (name, is_table) = self
                .selected_dataset_info()
                .ok_or("Select a table dataset in the Data viewer first")?;
            if !is_table {
                return Err("ONNX batch inference requires a table dataset".into());
            }
            let table = &self
                .data
                .tables
                .get(&name)
                .ok_or("The selected dataset no longer exists")?
                .table;
            let features: Vec<String> = self
                .class_features
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            if features.is_empty() {
                return Err("Set the feature columns (in the classification section) first".into());
            }
            let indices: Vec<usize> = features
                .iter()
                .map(|column| {
                    table
                        .columns
                        .iter()
                        .position(|c| c == column)
                        .ok_or_else(|| format!("Column `{column}` was not found"))
                })
                .collect::<Result<_, _>>()?;
            let mut rows: Vec<Vec<f64>> = Vec::new();
            for row in &table.rows {
                let mut values = Vec::with_capacity(indices.len());
                let mut ok = true;
                for &index in &indices {
                    match row.get(index).and_then(|c| c.parse::<f64>().ok()) {
                        Some(v) if v.is_finite() => values.push(v),
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    rows.push(values);
                }
            }
            if rows.len() < 1 {
                return Err("No complete numeric rows to score".into());
            }
            let n = rows.len();
            let frame = millwright::frame::Frame::from_rows(rows, features.clone())
                .map_err(|e| e.to_string())?;
            let predictions = model.predict(&frame).map_err(|e| e.to_string())?;
            let preview: Vec<String> = predictions
                .iter()
                .take(10)
                .map(|v| format!("{v:.4}"))
                .collect();
            Ok(format!(
                "Scored {n} rows of `{name}` with `{}`.\nFirst {}: [{}]",
                self.onnx_model_name,
                preview.len(),
                preview.join(", ")
            ))
        })();
        self.onnx_result = result.unwrap_or_else(|error| error);
    }

    /// Grid-search learning rate × epochs for the classifier on the selected
    /// dataset, rank by held-out macro-F1, and adopt the best configuration.
    fn run_classifier_sweep(&mut self) {
        let result = (|| -> Result<String, String> {
            let (name, is_table) = self
                .selected_dataset_info()
                .ok_or("Select a table dataset in the Data viewer first")?;
            if !is_table {
                return Err("Classification requires a table dataset".into());
            }
            let table = &self
                .data
                .tables
                .get(&name)
                .ok_or("The selected dataset no longer exists")?
                .table;
            let features: Vec<String> = self
                .class_features
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            let data = classification::prepare(table, &features, self.class_target.trim())?;
            let (train, test) = data.split(self.class_test_fraction);
            let train = if train.features.len() >= 2 { train } else { data.clone() };
            let learning_rates = [0.05, 0.1, 0.3, 0.5, 1.0];
            let epoch_grid = [100usize, 300, 600];
            let results = classification::sweep(&train, &test, &learning_rates, &epoch_grid);
            let best = results
                .first()
                .cloned()
                .ok_or("The sweep produced no results")?;
            self.class_lr = best.learning_rate;
            self.class_epochs = best.epochs;
            let table_rows = results
                .iter()
                .take(8)
                .map(|r| {
                    format!(
                        "  lr {:<5} epochs {:<4} → acc {:.3}, macro-F1 {:.3}",
                        r.learning_rate, r.epochs, r.accuracy, r.macro_f1
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(format!(
                "Sweep on `{name}` ({} configs) — best: lr {}, epochs {} (macro-F1 {:.3}). Adopted best config.\n{table_rows}",
                results.len(),
                best.learning_rate,
                best.epochs,
                best.macro_f1
            ))
        })();
        self.class_result = result.unwrap_or_else(|error| error);
    }

    fn selected_native_training_data(&self) -> Result<deep_learning::NativeTrainingData, String> {
        let (name, is_table) = self
            .selected_dataset_info()
            .ok_or("Select a table dataset in the Data viewer first")?;
        if !is_table {
            return Err("Native Burn training requires a table dataset".into());
        }
        let dataset = self
            .data
            .tables
            .get(&name)
            .ok_or("The selected dataset no longer exists")?;
        deep_learning::native_training_data(
            &name,
            &dataset.table,
            self.burn_training_feature.trim(),
            self.burn_training_target.trim(),
        )
    }
}
