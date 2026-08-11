use std::fs;
use std::path::{Path, PathBuf};

use crate::ui::{ebold, fail};

// build.rs embeds every file in ./templates
include!(concat!(env!("OUT_DIR"), "/builtins.rs"));

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
                // .DS_Store and editor droppings are not templates
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| !name.starts_with('.'))
                })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn first_on_path(names: &[&str]) -> Option<PathBuf> {
        names.iter().find_map(|name| which::which(name).ok())
    }

    fn check(compiler: &Path, source: &Path, std_flag: &str, binary: &Path) {
        let output = Command::new(compiler)
            .arg(source)
            .arg("-o")
            .arg(binary)
            .args(["-O2", std_flag, "-DCONTEST", "-Wall"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}: {}",
            source.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // default.* and c.* must build with the host toolchain; the rest may
    // assume real gcc and are skipped where only clang exists
    #[test]
    fn builtins_compile() {
        let tmp = tempfile::tempdir().unwrap();
        let universal = first_on_path(&["c++", "g++", "clang++"]);
        let gnu = first_on_path(&["g++"]).filter(|path| {
            Command::new(path)
                .arg("--version")
                .output()
                .is_ok_and(|out| !String::from_utf8_lossy(&out.stdout).contains("clang"))
        });
        let cc = first_on_path(&["cc", "gcc", "clang"]);
        // without one this checks nothing, and a green tick would say otherwise
        assert!(
            universal.is_some(),
            "no c++ compiler on PATH, so no template was built"
        );
        for (file, content) in BUILTINS {
            let template = Template {
                filename: file.to_string(),
                content: content.to_string(),
            };
            let source = tmp.path().join(file);
            fs::write(&source, template.render("00_Test_1", "A Title")).unwrap();
            let binary = tmp.path().join("a.out");
            match template.extension() {
                "cpp" => {
                    let required = if file.starts_with("default.") {
                        &universal
                    } else {
                        &gnu
                    };
                    if let Some(compiler) = required {
                        check(compiler, &source, "-std=c++17", &binary);
                    }
                }
                "c" => {
                    if let Some(compiler) = &cc {
                        check(compiler, &source, "-std=c99", &binary);
                    }
                }
                _ => {}
            }
        }
    }
}
