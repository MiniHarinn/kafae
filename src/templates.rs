use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::client::{attachment_of, Session};
use crate::config;
use crate::offline;

// build.rs embeds every file in ./templates
include!(concat!(env!("OUT_DIR"), "/builtins.rs"));

// where a solution starts from. A builtin and a file of your own are the same for every
// problem, so one of them is picked once and used for all of them; the grader ships its
// own file per problem, which is why that source has no file here of its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Builtin,
    User,
    Problem,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Builtin => "builtin",
            Source::User => "yours",
            Source::Problem => "problem",
        }
    }
}

// one thing a solution can start from: what -t names and what kafae templates lists
#[derive(Debug)]
pub struct Start {
    pub name: String,
    pub source: Source,
    // what the listing shows; the grader names its own, so for that one this is a shape
    pub file: String,
    path: Option<PathBuf>,
}

pub const ATTACHMENT: &str = "attachment";
pub const DEFAULT: &str = "default";

// None on a machine with no HOME, where dirs has nothing to offer: this is reached by the
// completers before cli::run, so it degrades rather than panicking
pub fn user_dir() -> Option<PathBuf> {
    config::templates_dir()
}

fn user_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = user_dir()
        .and_then(|dir| fs::read_dir(dir).ok())
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

// the filename is the whole declaration: py.py is named py and writes a .py. That is
// enough here because kafae writes exactly one file, so nothing needs a manifest to
// describe it; two files sharing a stem are told apart by naming one in full.
fn stem_of(file: &str) -> String {
    Path::new(file)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(file)
        .to_string()
}

fn ext_of(file: &str) -> String {
    Path::new(file)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_string()
}

// yours first, so a file of your own shadows a builtin of the same name
pub fn catalogue() -> Vec<Start> {
    let mine = user_files().into_iter().filter_map(|path| {
        let file = path.file_name()?.to_str()?.to_string();
        Some(Start {
            name: stem_of(&file),
            source: Source::User,
            file,
            path: Some(path),
        })
    });
    let builtin = BUILTINS.iter().map(|(file, _)| Start {
        name: stem_of(file),
        source: Source::Builtin,
        file: file.to_string(),
        path: None,
    });
    let grader = Start {
        name: ATTACHMENT.to_string(),
        source: Source::Problem,
        file: "<problem>.<ext>".to_string(),
        path: None,
    };
    mine.chain(builtin).chain([grader]).collect()
}

pub fn names() -> Vec<String> {
    let mut names = Vec::new();
    for start in catalogue() {
        if !names.contains(&start.name) {
            names.push(start.name);
        }
    }
    names
}

// Err rather than ending the process: new asks this before a glob it may still serve, and
// the caller decides whether one problem failing is the whole command failing
pub fn pick(wanted: &str) -> Result<Start, String> {
    let hits: Vec<Start> = catalogue()
        .into_iter()
        .filter(|start| start.name == wanted || start.file == wanted)
        .collect();
    // two files of your own under one stem: only you can say which, by naming the file
    if hits.len() > 1 && hits.iter().all(|start| start.source == Source::User) {
        return Err(format!(
            "template {wanted} is ambiguous: {}",
            hits.iter()
                .map(|start| start.file.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    hits.into_iter()
        .next()
        .ok_or_else(|| format!("no template named {wanted}, see kafae templates"))
}

fn render(content: &str, name: &str, title: &str) -> String {
    content.replace("{name}", name).replace("{title}", title)
}

// the extension the solution will take and the bytes to put in it. Err says why this one
// problem cannot start here, which the caller turns into a skip.
pub fn open(
    start: &Start,
    session: &Session,
    prob: &Value,
    name: &str,
    title: &str,
) -> Result<(String, Vec<u8>), String> {
    match start.source {
        Source::User => {
            let path = start.path.as_ref().ok_or("template has no file")?;
            let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
            Ok((
                ext_of(&start.file),
                render(&content, name, title).into_bytes(),
            ))
        }
        Source::Builtin => {
            let (_, content) = BUILTINS
                .iter()
                .find(|(file, _)| *file == start.file)
                .ok_or("builtin template went missing")?;
            Ok((
                ext_of(&start.file),
                render(content, name, title).into_bytes(),
            ))
        }
        // byte for byte: the grader's file is not ours to substitute into, and it may not
        // be text at all
        Source::Problem => {
            let Some(path) = attachment_of(session, prob) else {
                return Err(if offline::on() {
                    "has no attachment cached, run kafae sync online first".to_string()
                } else {
                    "ships no attachment".to_string()
                });
            };
            let ext = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("bin")
                .to_string();
            Ok((ext, fs::read(&path).map_err(|error| error.to_string())?))
        }
    }
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
            let source = tmp.path().join(file);
            fs::write(&source, render(content, "00_Test_1", "A Title")).unwrap();
            let binary = tmp.path().join("a.out");
            match ext_of(file).as_str() {
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

    // the grader's file is a source like the others, so it comes out of the catalogue
    // rather than being appended by whatever happens to be printing
    #[test]
    fn the_graders_file_is_one_of_the_sources() {
        let sources = catalogue();
        let grader = sources
            .iter()
            .find(|start| start.name == ATTACHMENT)
            .expect("the grader's file is a source");
        assert_eq!(grader.source, Source::Problem);
        assert_eq!(grader.source.label(), "problem");
        assert!(names().contains(&ATTACHMENT.to_string()));
        assert!(sources.iter().any(|start| start.source == Source::Builtin));
    }

    // the filename is the declaration, so both halves of it name the same thing
    #[test]
    fn a_name_or_the_whole_filename_pick_the_same_thing() {
        assert_eq!(pick(DEFAULT).unwrap().file, "default.cpp");
        assert_eq!(pick("default.cpp").unwrap().name, DEFAULT);
        assert_eq!(pick(ATTACHMENT).unwrap().source, Source::Problem);
        assert_eq!(ext_of("default.cpp"), "cpp");
        assert_eq!(ext_of("Makefile"), "");
    }

    // picking must not end the process: a glob may still have problems it can serve
    #[test]
    fn an_unknown_name_is_an_error_and_not_an_exit() {
        let missing = pick("nosuchtemplate").unwrap_err();
        assert!(missing.contains("no template named"), "{missing}");
    }
}
