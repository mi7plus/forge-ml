//! Language-server integration: document synchronization, on-demand LSP
//! requests (completion/hover/signature/definition), rename, workspace edits,
//! and the deferred definition probe. Methods on the shared [`crate::ForgeApp`],
//! split out of `main.rs`.
use crate::*;

impl crate::ForgeApp {
    pub(crate) fn sync_lsp(&mut self) {
        let (Some(root), Some(path)) = (
            self.project.as_ref().map(|project| project.root.clone()),
            self.active().path.clone(),
        ) else {
            return;
        };
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            return;
        }
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        self.active().content.hash(&mut hasher);
        let hash = hasher.finish();
        if hash == self.last_lsp_hash {
            return;
        }
        self.last_lsp_hash = hash;
        self.document_version += 1;
        let (text, _) = lsp_document(&self.active().content);
        self.lsp.send(LspCommand::Sync {
            root,
            path,
            text,
            version: self.document_version,
        });
    }

    pub(crate) fn request_lsp(&mut self, action: &str) {
        let Some(path) = self.active().path.clone() else {
            self.lsp_status = "Save this buffer before requesting language features.".to_owned();
            return;
        };
        let (text, prefix_chars) = lsp_document(&self.active().content);
        let char_offset = self.cursor_offset + prefix_chars;
        let command = match action {
            "complete" => LspCommand::Complete {
                path,
                text,
                char_offset,
            },
            "hover" => LspCommand::Hover {
                path,
                text,
                char_offset,
            },
            "references" => LspCommand::References {
                path,
                text,
                char_offset,
            },
            "signature" => LspCommand::SignatureHelp {
                path,
                text,
                char_offset,
            },
            "codeactions" => LspCommand::CodeActions {
                path,
                text,
                char_offset,
            },
            _ => LspCommand::Definition {
                path,
                text,
                char_offset,
            },
        };
        self.lsp.send(command);
    }

    /// Whether the active buffer is a plain (non-notebook) Rust file, which is
    /// required for rename / code actions to map edits back correctly.
    pub(crate) fn active_plain_rust(&self) -> bool {
        let is_rs = self
            .active()
            .path
            .as_ref()
            .is_some_and(|p| p.extension().is_some_and(|e| e == "rs"));
        is_rs && !is_notebook_document(&self.active().content)
    }

    pub(crate) fn send_rename(&mut self, new_name: String) {
        let Some(path) = self.active().path.clone() else {
            self.lsp_status = "Save this buffer before renaming.".to_owned();
            return;
        };
        if !self.active_plain_rust() {
            self.lsp_status = "Rename is only available in plain Rust files.".to_owned();
            return;
        }
        let (text, _) = lsp_document(&self.active().content);
        self.lsp.send(LspCommand::Rename {
            path,
            text,
            char_offset: self.cursor_offset,
            new_name,
        });
    }

    /// Apply a workspace edit (from rename or a code action) to open buffers and
    /// closed project files.
    pub(crate) fn apply_file_edits(&mut self, files: Vec<lsp::FileEdit>) {
        let mut touched = 0usize;
        for file in files {
            if file.edits.is_empty() {
                continue;
            }
            if let Some(index) = self
                .tabs
                .iter()
                .position(|tab| tab.path.as_deref() == Some(file.path.as_path()))
            {
                let mut content = self.tabs[index].content.clone();
                apply_edits_to(&mut content, &file.edits);
                self.tabs[index].content = content;
                self.tabs[index].dirty = true;
                touched += 1;
            } else if let Ok(mut content) = std::fs::read_to_string(&file.path) {
                apply_edits_to(&mut content, &file.edits);
                if std::fs::write(&file.path, content).is_ok() {
                    touched += 1;
                }
            }
        }
        self.last_lsp_hash = 0;
        self.cell_records.clear();
        self.console = format!("Applied edits to {touched} file(s).");
    }

    pub(crate) fn probe_definition(&mut self, char_offset: usize) {
        let Some(path) = self.active().path.clone() else {
            return;
        };
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            return;
        }
        let (text, prefix_chars) = lsp_document(&self.active().content);
        self.lsp.send(LspCommand::ProbeDefinition {
            path,
            text,
            char_offset: char_offset + prefix_chars,
        });
    }
}
