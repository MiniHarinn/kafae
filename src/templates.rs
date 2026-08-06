use std::fs;
use std::path::{Path, PathBuf};

use crate::ui::{ebold, fail};

// new files in ./templates must be registered here
const BUILTINS: &[(&str, &str)] = &[
    ("default.cpp", include_str!("../templates/default.cpp")),
    ("c.c", include_str!("../templates/c.c")),
    ("py.py", include_str!("../templates/py.py")),
];

pub struct Template {
    pub filename: String,
    pub content: String,
}

impl Template {
    pub fn extension(&self) -> &str {
        Path::new(&self.filename)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
    }

    pub fn render(&self, name: &str, title: &str) -> String {
        self.content
            .replace("{name}", name)
            .replace("{title}", title)
    }
}

pub fn user_dir() -> PathBuf {
    dirs::config_dir().unwrap().join("kafae").join("templates")
}

fn user_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(user_dir())
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

pub fn listing() -> Vec<(String, bool)> {
    user_files()
        .iter()
        .filter_map(|path| Some((path.file_name()?.to_str()?.to_string(), false)))
        .chain(BUILTINS.iter().map(|(file, _)| (file.to_string(), true)))
        .collect()
}

pub fn names() -> Vec<String> {
    let mut names = Vec::new();
    for (file, _) in listing() {
        if let Some(stem) = Path::new(&file).file_stem().and_then(|s| s.to_str()) {
            if !names.contains(&stem.to_string()) {
                names.push(stem.to_string());
            }
        }
    }
    names
}

fn matches(file: &str, name: &str) -> bool {
    file == name || Path::new(file).file_stem().and_then(|stem| stem.to_str()) == Some(name)
}

pub fn resolve(name: &str) -> Template {
    let hits: Vec<PathBuf> = user_files()
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|file| file.to_str())
                .is_some_and(|file| matches(file, name))
        })
        .collect();
    if hits.len() > 1 {
        fail(&format!(
            "template {} is ambiguous: {}",
            ebold(name),
            hits.iter()
                .filter_map(|path| path.file_name()?.to_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(path) = hits.first() {
        return Template {
            filename: path.file_name().unwrap().to_string_lossy().into_owned(),
            content: fs::read_to_string(path).unwrap_or_else(|error| fail(&error.to_string())),
        };
    }
    if let Some((file, content)) = BUILTINS.iter().find(|(file, _)| matches(file, name)) {
        return Template {
            filename: file.to_string(),
            content: content.to_string(),
        };
    }
    fail(&format!(
        "no template named {}, see {}",
        ebold(name),
        ebold("kafae templates")
    ));
}
