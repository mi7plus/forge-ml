//! Dock plumbing: terminal and Rust-kernel pane bodies, tile id allocation,
//! terminal/kernel creation, per-tile dock body dispatch, floating pane and
//! dataset windows, and applying dataset-view results. Methods on the shared
//! [`crate::ForgeApp`].

use crate::ui::theme::*;
use crate::*;
use eframe::egui;

impl crate::ForgeApp {
    /// Body of one terminal pane. Spawns its shell the first time the pane is
    /// shown, then renders and drives it each frame.
    fn terminal_pane(&mut self, id: u32, ui: &mut egui::Ui) {
        if !self.terminals.contains_key(&id) {
            let cwd = self.project_root();
            let font = self.editor_font_size.clamp(10.0, 20.0);
            match terminal::Terminal::spawn(cwd, font) {
                Ok(term) => {
                    self.terminals.insert(id, term);
                }
                Err(error) => {
                    ui.colored_label(RED, format!("Terminal unavailable: {error}"));
                    return;
                }
            }
        }
        let dark = self.dark_mode;
        if let Some(term) = self.terminals.get_mut(&id) {
            term.ui(ui, dark);
        }
    }

    /// Body of one Rust kernel pane. Spawns its Evcxr session on first show.
    fn rust_console_pane(&mut self, id: u32, ui: &mut egui::Ui) {
        let kernel = self
            .kernels
            .entry(id)
            .or_insert_with(rust_kernel::RustKernel::spawn);
        kernel.ui(id, ui);
    }

    /// The lowest unused id among panes of a given kind, so new instances never
    /// collide with existing panes (including ones restored from a saved layout).
    fn pane_next_id(tree: &Tree<PaneKind>, is_kind: impl Fn(&PaneKind) -> Option<u32>) -> u32 {
        tree.tiles
            .iter()
            .filter_map(|(_, tile)| match tile {
                Tile::Pane(pane) => is_kind(pane),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            + 1
    }

    /// Create a new Rust kernel tile, grouped with an existing kernel, else the
    /// primary console's container, else the tab sibling of `anchor`, else root.
    pub(crate) fn create_kernel(tree: &mut Tree<PaneKind>, anchor: Option<TileId>) -> PaneKind {
        let id = Self::pane_next_id(tree, |p| match p {
            PaneKind::RustConsole(n) => Some(*n),
            _ => None,
        });
        let kind = PaneKind::RustConsole(id);
        let new_tile = tree.tiles.insert_pane(kind);
        let find_pane = |pred: fn(&PaneKind) -> bool| {
            tree.tiles.iter().find_map(|(tid, tile)| match tile {
                Tile::Pane(p) if pred(p) => Some(*tid),
                _ => None,
            })
        };
        let anchor_tile = anchor
            .or_else(|| find_pane(|p| matches!(p, PaneKind::RustConsole(_))))
            .or_else(|| find_pane(|p| matches!(p, PaneKind::Console)));
        let parent = anchor_tile.and_then(|tile| tree.tiles.parent_of(tile));
        if let Some(parent) = parent.or_else(|| tree.root()) {
            tree.move_tile_to_container(new_tile, parent, usize::MAX, false);
        }
        kind
    }

    /// The lowest unused terminal id, so a new terminal never collides with an
    /// existing pane (including ones restored from a saved layout).
    fn terminal_next_id(tree: &Tree<PaneKind>) -> u32 {
        tree.tiles
            .iter()
            .filter_map(|(_, tile)| match tile {
                Tile::Pane(PaneKind::Terminal(id)) => Some(*id),
                _ => None,
            })
            .max()
            .unwrap_or(0)
            + 1
    }

    /// Create a new terminal tile, placing it as a tab sibling of `anchor` (its
    /// container) when given, otherwise beside an existing terminal or at the root.
    pub(crate) fn create_terminal(tree: &mut Tree<PaneKind>, anchor: Option<TileId>) -> PaneKind {
        let id = Self::terminal_next_id(tree);
        let kind = PaneKind::Terminal(id);
        let new_tile = tree.tiles.insert_pane(kind);
        let anchor = anchor
            .or_else(|| {
                // Group with an existing terminal if there is one.
                tree.tiles.iter().find_map(|(tid, tile)| match tile {
                    Tile::Pane(PaneKind::Terminal(_)) => Some(*tid),
                    _ => None,
                })
            })
            .and_then(|tile| tree.tiles.parent_of(tile));
        let destination = anchor.or_else(|| tree.root());
        if let Some(parent) = destination {
            tree.move_tile_to_container(new_tile, parent, usize::MAX, false);
        }
        kind
    }

    /// Render one pane's contents. Shared by the docked tiles and the floating
    /// pane windows so both paths stay identical.
    pub(crate) fn dock_pane_body(&mut self, kind: PaneKind, ui: &mut egui::Ui) {
        match kind {
            PaneKind::Editor => self.editor_pane(ui),
            PaneKind::Files => self.file_explorer(ui),
            PaneKind::Outline => self.outline(ui),
            PaneKind::Cells => self.cell_rail(ui),
            PaneKind::Console => self.console_pane(ConsoleTab::Console, ui),
            PaneKind::History => self.console_pane(ConsoleTab::History, ui),
            PaneKind::Python => self.console_pane(ConsoleTab::Python, ui),
            PaneKind::Terminal(id) => self.terminal_pane(id, ui),
            PaneKind::RustConsole(id) => self.rust_console_pane(id, ui),
            PaneKind::DataViewer => self.dock_data_viewer(ui),
            // Inspector panes scroll as a whole; the stable id keeps each pane's
            // scroll position remembered across focus changes and restarts.
            PaneKind::Inspector(tab) => {
                egui::ScrollArea::vertical()
                    .id_salt(("dock_inspector_scroll", tab))
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.inspector_body(tab, ui));
            }
        }
    }

    /// Render every pane that has been popped out into a floating window. Each
    /// floating pane's tree tile stays hidden; closing the window or choosing
    /// Dock returns it to its docked position.
    pub(crate) fn dock_floating_windows(&mut self, ctx: &egui::Context) {
        if self.floating_panes.is_empty() {
            return;
        }
        // A pane re-shown from View → Panes is no longer floating.
        if let Some(tree) = self.dock_tree.as_ref() {
            let visible: Vec<PaneKind> = self
                .floating_panes
                .iter()
                .copied()
                .filter(|kind| {
                    Self::dock_tile_of(tree, *kind).is_some_and(|id| tree.tiles.is_visible(id))
                })
                .collect();
            self.floating_panes.retain(|k| !visible.contains(k));
        }

        let mut dock_back: Vec<PaneKind> = Vec::new();
        for kind in self.floating_panes.clone() {
            let mut open = true;
            let mut dock = false;
            let window_title = match kind {
                PaneKind::Terminal(id) => {
                    let live = self.terminals.get(&id).map(|t| t.title()).unwrap_or("Terminal");
                    if live == "Terminal" {
                        format!("Terminal {id}")
                    } else {
                        live.to_owned()
                    }
                }
                PaneKind::RustConsole(id) => format!("Rust {id}"),
                other => other.title().to_owned(),
            };
            egui::Window::new(window_title)
                .id(egui::Id::new(("forge_float_pane", kind)))
                .default_size([540.0, 420.0])
                .min_size([300.0, 160.0])
                .resizable(true)
                .open(&mut open)
                .show(ctx, |ui| {
                    if ui
                        .small_button("Dock")
                        .on_hover_text("Return this pane to the workspace")
                        .clicked()
                    {
                        dock = true;
                    }
                    ui.separator();
                    self.dock_pane_body(kind, ui);
                });
            if dock || !open {
                dock_back.push(kind);
            }
        }
        for kind in dock_back {
            self.floating_panes.retain(|k| *k != kind);
            if let Some(tree) = self.dock_tree.as_mut() {
                if let Some(id) = Self::dock_tile_of(tree, kind) {
                    tree.tiles.set_visible(id, true);
                }
            }
        }
    }

    pub(crate) fn dataset_window(&mut self, ctx: &egui::Context) {
        if self.dataset_viewer_docked {
            return;
        }
        let Some((name, editable)) = self.selected_dataset_info() else {
            self.open_dataset = None;
            return;
        };
        let mut open = true;
        let mut dock = false;
        egui::Window::new(format!("Data viewer — {name}"))
            .id(egui::Id::new("forge_dataset_viewer"))
            .open(&mut open)
            .default_size([760.0, 520.0])
            .min_size([420.0, 260.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .small_button("Dock right")
                        .on_hover_text("Return this viewer to the Data panel")
                        .clicked()
                    {
                        dock = true;
                    }
                });
                if let Some(result) = self.draw_selected_dataset(ui, &name, editable, "floating") {
                    self.apply_dataset_view_result(&name, result);
                }
            });
        if dock {
            self.dataset_viewer_docked = true;
        }
        if !open {
            self.open_dataset = None;
        }
    }

    pub(crate) fn apply_dataset_view_result(&mut self, name: &str, result: DatasetViewResult) {
        if let Some(table) = result.committed {
            if let Some(existing) = self.data.tables.get(name) {
                let source = existing.source.clone();
                match data::Dataset::from_table(table, source) {
                    Ok(dataset) => {
                        self.data.tables.insert(name.to_owned(), dataset);
                        self.console =
                            format!("Saved edits to `{name}` and rebuilt its Arrow batch.");
                    }
                    Err(error) => self.console = format!("Could not save dataset edits: {error}"),
                }
            }
        }
        if let Some(spec) = result.linked_plot {
            self.structured_plots
                .retain(|existing| existing.name != spec.name);
            self.structured_plots.push(spec);
            self.inspector_tab = InspectorTab::Charts;
        }
        if let Some(message) = result.message {
            self.console = message;
        }
    }
}
