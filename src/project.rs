use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    pub children: Option<Vec<FileNode>>,
}

pub struct Project {
    pub root: PathBuf,
    pub files: Vec<FileNode>,
}

impl Project {
    pub fn open(root: PathBuf) -> io::Result<Self> {
        let files = read_directory(&root)?;
        Ok(Self { root, files })
    }

    pub fn refresh(&mut self) -> io::Result<()> {
        self.files = read_directory(&self.root)?;
        Ok(())
    }
}

pub fn is_editable(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("rs" | "toml" | "md" | "txt" | "json" | "yaml" | "yml")
    )
}

fn read_directory(directory: &Path) -> io::Result<Vec<FileNode>> {
    let mut entries = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter(|entry| !matches!(entry.file_name().to_str(), Some(".git" | "target")))
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
            }
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|node| (node.children.is_none(), node.name.to_lowercase()));
    Ok(entries)
}
