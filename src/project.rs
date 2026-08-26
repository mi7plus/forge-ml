use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    pub children: Option<Vec<FileNode>>,
    pub git_status: Option<String>,
}

pub struct Project {
    pub root: PathBuf,
    pub files: Vec<FileNode>,
}

impl Project {
    pub fn open(root: PathBuf) -> io::Result<Self> {
        let files = read_directory(&root)?;
        let mut project = Self { root, files };
        project.refresh_git_status();
        Ok(project)
    }

    pub fn refresh(&mut self) -> io::Result<()> {
        self.files = read_directory(&self.root)?;
        self.refresh_git_status();
        Ok(())
    }

    pub fn refresh_git_status(&mut self) {
        let Ok(snapshot) = crate::git::snapshot(&self.root) else {
            return;
        };
        apply_git_status(&self.root, &mut self.files, &snapshot.files);
    }
}

pub fn is_editable(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("rs" | "toml" | "md" | "txt" | "json" | "ipynb" | "yaml" | "yml")
    )
}

fn read_directory(directory: &Path) -> io::Result<Vec<FileNode>> {
    let mut entries = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some(".forge" | ".git" | ".venv" | "target")
            )
        })
        .map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let children = path
                .is_dir()
                .then(|| read_directory(&path).unwrap_or_default());
            FileNode {
                path,
                name,
                children,
                git_status: None,
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|node| (node.children.is_none(), node.name.to_lowercase()));
    Ok(entries)
}

fn apply_git_status(
    root: &Path,
    nodes: &mut [FileNode],
    statuses: &std::collections::HashMap<PathBuf, String>,
) {
    for node in nodes {
        let relative = node.path.strip_prefix(root).unwrap_or(&node.path);
        node.git_status = statuses.get(relative).cloned();
        if let Some(children) = &mut node.children {
            apply_git_status(root, children, statuses);
        }
    }
}
