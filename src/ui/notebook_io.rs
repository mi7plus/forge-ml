//! Notebook and dataset import/export: .ipynb round-trips, notebook document
//! and project-bundle export, Jupyter discovery, and dataset imports. Methods
//! on the shared [`crate::ForgeApp`].

use crate::result_ext::ResultText;
use crate::*;

impl crate::ForgeApp {
    pub(crate) fn import_ipynb(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Jupyter notebook", &["ipynb"])
            .pick_file()
        else {
            return;
        };
        let result = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| NotebookDocument::from_ipynb(&text));
        match result {
            Ok(notebook) => {
                self.tabs.push(EditorTab {
                    title: format!("{}.rs", file_title(&path)),
                    path: None,
                    content: notebook.to_rust(),
                    dirty: true,
                    disk_hash: None,
                    external_change_pending: false,
                });
                self.active_tab = self.tabs.len() - 1;
                self.selected_cell = 0;
                self.console = format!(
                    "Imported {} using kernel `{}`.",
                    path.display(),
                    notebook.kernel
                );
            }
            Err(error) => self.console = format!("Could not import notebook: {error}"),
        }
    }

    pub(crate) fn export_ipynb(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Jupyter notebook", &["ipynb"])
            .set_file_name("notebook.ipynb")
            .save_file()
        else {
            return;
        };
        let notebook = NotebookDocument::parse_rust(&self.active().content);
        match notebook
            .to_ipynb()
            .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()))
        {
            Ok(()) => self.console = format!("Exported {}.", path.display()),
            Err(error) => self.console = format!("Could not export notebook: {error}"),
        }
    }

    pub(crate) fn export_notebook_document(&mut self, extension: &str) {
        let document = NotebookDocument::parse_rust(&self.active().content);
        let output = if extension == "html" {
            export::notebook_html(&document)
        } else {
            export::notebook_markdown(&document)
        };
        let stem = self
            .active()
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|v| v.to_str())
            .unwrap_or("notebook");
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.{extension}"))
            .save_file()
        {
            self.console = std::fs::write(&path, output)
                .map(|()| format!("Exported {}", path.display()))
                .unwrap_or_else(|e| format!("Notebook export failed: {e}"));
        }
    }

    pub(crate) fn export_project_bundle(&mut self) {
        let Some(root) = self.project_root() else {
            self.console = "Open a project before exporting a project bundle.".into();
            return;
        };
        let name = root
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("forge-project");
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{name}-bundle.zip"))
            .save_file()
        {
            self.console = export::project_bundle(&root, &path)
                .map(|()| format!("Exported reproducible project bundle to {}", path.display()))
                .unwrap_or_else(|e| format!("Project bundle failed: {e}"));
        }
    }

    pub(crate) fn discover_jupyter(&mut self) {
        self.jupyter_output = jupyter::discover()
            .map(|kernels| {
                kernels
                    .into_iter()
                    .map(|kernel| {
                        format!(
                            "{} ({}) [{}]\n  {}",
                            kernel.display_name,
                            kernel.name,
                            kernel.language,
                            kernel.resource_dir.display()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .text();
        self.inspector_tab = InspectorTab::Help;
        self.hover_text = format!("Jupyter kernels\n\n{}", self.jupyter_output);
    }

    pub(crate) fn import_dataset(&mut self) {
        if self.integration_pending > 0 {
            self.console = "Wait for the current data operation to finish.".into();
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import dataset")
            .add_filter(
                "Datasets",
                &["csv", "tsv", "jsonl", "ndjson", "parquet", "arrow", "ipc"],
            )
            .pick_file()
        else {
            return;
        };
        match self
            .integration_worker
            .submit(IntegrationRequest::DataImport(path.clone()))
        {
            Ok(()) => {
                self.integration_pending += 1;
                self.console = format!("Importing {} in the background…", path.display());
            }
            Err(error) => self.console = format!("Could not start dataset import: {error}"),
        }
    }

    pub(crate) fn import_millwright_dataset(&mut self) {
        if self.integration_pending > 0 {
            self.console = "Wait for the current data operation to finish.".into();
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import Millwright table")
            .add_filter("Millwright tables", &["csv", "parquet"])
            .pick_file()
        else {
            return;
        };
        match self
            .integration_worker
            .submit(IntegrationRequest::MillwrightImport(path.clone()))
        {
            Ok(()) => {
                self.integration_pending += 1;
                self.console = format!(
                    "Importing {} through published Millwright 2.2.1 in the background…",
                    path.display()
                );
            }
            Err(error) => self.console = format!("Could not start Millwright import: {error}"),
        }
    }
}
