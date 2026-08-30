#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    NewFile,
    Save,
    RunCell,
    RunAll,
    Stop,
    Find,
    FindProject,
    FindReferences,
    RenameSymbol,
    CodeActions,
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
    (Command::FindReferences, "Source: Find references", ""),
    (Command::RenameSymbol, "Source: Rename symbol", ""),
    (Command::CodeActions, "Source: Code actions / quick fixes", ""),
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

/// Fuzzy subsequence score of `needle` within `haystack` (case-insensitive).
/// Rewards consecutive and early matches; `None` if not a subsequence.
fn fuzzy_score(needle: &str, haystack: &str) -> Option<i32> {
    let hay: Vec<char> = haystack.to_ascii_lowercase().chars().collect();
    let mut hi = 0usize;
    let mut score = 0i32;
    let mut streak = 0i32;
    for nc in needle.to_ascii_lowercase().chars() {
        if nc.is_whitespace() {
            continue;
        }
        let mut found = false;
        while hi < hay.len() {
            if hay[hi] == nc {
                found = true;
                break;
            }
            hi += 1;
            streak = 0;
        }
        if !found {
            return None;
        }
        score += 12 + streak * 6 - (hi as i32).min(24);
        streak += 1;
        hi += 1;
    }
    Some(score)
}

/// Fuzzy-match commands against `query`, ranked best first. An empty query
/// returns every command in its declared order.
pub fn matches(query: &str) -> Vec<(Command, &'static str, &'static str)> {
    if query.trim().is_empty() {
        return ALL.to_vec();
    }
    let mut scored: Vec<(i32, (Command, &'static str, &'static str))> = ALL
        .iter()
        .filter_map(|entry| {
            let (_, label, shortcut) = entry;
            let haystack = format!("{label} {shortcut}");
            fuzzy_score(query, &haystack).map(|score| (score, *entry))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1 .1.cmp(b.1 .1)));
    scored.into_iter().map(|(_, entry)| entry).collect()
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

    #[test]
    fn fuzzy_matches_and_ranks() {
        // Empty query lists everything.
        assert_eq!(matches("").len(), ALL.len());
        // An exact prefix ranks its command first.
        assert_eq!(matches("save").first().map(|v| v.0), Some(Command::Save));
        // Non-contiguous subsequence still matches.
        assert!(matches("frmt").iter().any(|v| v.0 == Command::FormatDocument));
        // Nonsense yields nothing.
        assert!(matches("zzqx").is_empty());
    }
}
