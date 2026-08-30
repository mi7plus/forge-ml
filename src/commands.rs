#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    NewFile,
    Save,
    RunCell,
    RunAll,
    Stop,
    Find,
    FindProject,
    FormatDocument,
    Clippy,
    CargoBuild,
    CargoTest,
    CargoRun,
    NewTerminal,
    NewKernel,
    ImportData,
    ToggleTheme,
    Settings,
    Variables,
    Data,
    Plots,
    Runs,
    Problems,
    Git,
    Packages,
    GitHub,
    Studio,
    Sql,
    Deep,
    Deploy,
    Storage,
}

pub const ALL: &[(Command, &str, &str)] = &[
    (Command::NewFile, "File: New file", "Ctrl+N"),
    (Command::Save, "File: Save", "Ctrl+S"),
    (Command::RunCell, "Run: Run current cell", "Shift+Enter"),
    (Command::RunAll, "Run: Run all cells", "Ctrl+Shift+Enter"),
    (Command::Stop, "Run: Stop execution", ""),
    (Command::Find, "Search: Find and replace", "Ctrl+F"),
    (
        Command::FindProject,
        "Search: Find in project",
        "Ctrl+Shift+F",
    ),
    (Command::FormatDocument, "Edit: Format document (rustfmt)", ""),
    (Command::Clippy, "Source: Run clippy", ""),
    (Command::CargoBuild, "Cargo: Build", ""),
    (Command::CargoTest, "Cargo: Test", ""),
    (Command::CargoRun, "Cargo: Run", ""),
    (Command::NewTerminal, "Terminal: New terminal", ""),
    (Command::NewKernel, "Kernel: New Rust kernel", ""),
    (Command::ImportData, "Data: Import dataset", ""),
    (Command::ToggleTheme, "View: Toggle light/dark theme", ""),
    (Command::Settings, "View: Settings", ""),
    (Command::Variables, "Pane: Variables", "Ctrl+1"),
    (Command::Data, "Pane: Data", ""),
    (Command::Plots, "Pane: Plots", ""),
    (Command::Runs, "Pane: Runs", ""),
    (Command::Problems, "Pane: Problems", ""),
    (Command::Git, "Pane: Git", ""),
    (Command::Packages, "Pane: Crates", ""),
    (Command::GitHub, "Pane: GitHub", ""),
    (Command::Studio, "Pane: Millwright Studio", ""),
    (Command::Sql, "Pane: SQL", ""),
    (Command::Deep, "Pane: Deep learning", ""),
    (Command::Deploy, "Pane: Deploy", ""),
    (Command::Storage, "Pane: Object storage", ""),
];

pub fn matches(query: &str) -> Vec<(Command, &'static str, &'static str)> {
    let terms = query
        .to_ascii_lowercase()
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    ALL.iter()
        .filter(|(_, label, shortcut)| {
            let haystack = format!(
                "{} {}",
                label.to_ascii_lowercase(),
                shortcut.to_ascii_lowercase()
            );
            terms.iter().all(|term| haystack.contains(term))
        })
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn searches_labels_and_shortcuts() {
        assert!(matches("import data")
            .iter()
            .any(|v| v.0 == Command::ImportData));
        assert!(matches("ctrl shift f")
            .iter()
            .any(|v| v.0 == Command::FindProject));
    }
}
