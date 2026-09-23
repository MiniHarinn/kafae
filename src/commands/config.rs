use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::client;
use crate::config;
use crate::json;
use crate::language::{self, Build};
use crate::opener;
use crate::ui::{bold, dim, fail, table};

// ---------------------------------------------------------------------------
// kafae config path
// ---------------------------------------------------------------------------

fn row(what: &str, path: Option<&PathBuf>) -> Vec<String> {
    let Some(path) = path else {
        // dirs has nothing to offer on a machine with no HOME, and kafae degrades
        return vec![
            what.to_string(),
            dim("nowhere on this machine").to_string(),
            String::new(),
        ];
    };
    vec![
        what.to_string(),
        path.display().to_string(),
        if path.exists() {
            String::new()
        } else {
            dim("missing").to_string()
        },
    ]
}

// three XDG directories mean "where is my stuff" has three answers, and on Windows and
// macOS none of them is anywhere a terminal user looks
pub fn path() {
    let paths = config::paths();
    let mut rows = vec![
        row("config", paths.config.as_ref()),
        row("sessions", paths.sessions.as_ref()),
        row("cache", paths.cache.as_ref()),
        row("templates", paths.templates.as_ref()),
    ];
    // the v1 state file is worth a row only while there still is one
    if let Some(state) = paths.legacy_state.as_ref().filter(|path| path.exists()) {
        rows.push(row("state (old)", Some(state)));
    }
    table(&[("what", false), ("path", false), ("", false)], &rows);

    if config::from_env("KAFAE_HOME").is_some() {
        println!("\n{}", dim("KAFAE_HOME is moving all of these together"));
    }
}

// ---------------------------------------------------------------------------
// kafae config show
// ---------------------------------------------------------------------------

// the config file holds no secrets, so every row of this is safe to paste into a bug
// report; provenance is the whole point, because the answer is usually a stale export
fn settings(config: &config::Config) -> Vec<(String, Option<config::Resolved>)> {
    // url and login can come from the one stored session when no table names them, and
    // that is the only thing Source::Session ever labels
    let stored = |pick: fn(&client::Account) -> String| {
        client::current_account().map(|account| config::Resolved {
            value: pick(account),
            source: config::Source::Session,
        })
    };
    let mut rows = vec![
        ("grader".to_string(), config.selected()),
        (
            "url".to_string(),
            config.url(None).or_else(|| stored(|a| a.url.clone())),
        ),
        (
            "login".to_string(),
            config.login(None).or_else(|| stored(|a| a.login.clone())),
        ),
        ("template".to_string(), Some(config.template(None))),
    ];
    // the language table is the single source of truth for the key name as well as the
    // default, so cc/cflags and cxx/cxxflags are read off it rather than written twice
    for ext in ["c", "cpp"] {
        let Some(local) = language::for_ext(ext) else {
            continue;
        };
        let Build::Compiler { env, flags_env, .. } = local.build else {
            continue;
        };
        rows.push((config::key_of(env), config.compiler(local)));
        rows.push((config::key_of(flags_env), config.flags(local)));
    }
    rows
}

pub fn show() {
    let config = config::load();
    let settings = settings(config);

    if json::on() {
        let entries: Map<String, Value> = settings
            .iter()
            .map(|(name, resolved)| {
                let value = match resolved {
                    Some(resolved) => {
                        json!({ "value": resolved.value, "source": resolved.source.label() })
                    }
                    None => Value::Null,
                };
                (name.clone(), value)
            })
            .collect();
        json::emit(&json!({
            "path": config.path().map(|path| path.display().to_string()),
            "exists": config.exists(),
            "settings": entries,
        }));
        return;
    }

    let rows: Vec<Vec<String>> = settings
        .iter()
        .map(|(name, resolved)| match resolved {
            Some(resolved) => vec![
                bold(name).to_string(),
                resolved.value.clone(),
                dim(resolved.source.label()).to_string(),
            ],
            None => vec![
                bold(name).to_string(),
                dim("not set").to_string(),
                String::new(),
            ],
        })
        .collect();
    table(
        &[("name", false), ("value", false), ("source", false)],
        &rows,
    );

    if let Some(name) = config.unknown_selection() {
        println!(
            "\n{}",
            dim(format!(
                "nothing is configured under [grader.{name}]; there is {}",
                config.grader_names().join(", ")
            ))
        );
    }
    match config.path() {
        Some(path) if config.exists() => println!("\n{}", dim(format!("read {}", path.display()))),
        Some(path) => println!(
            "\n{}",
            dim(format!(
                "no config file yet; it would go in {}",
                path.display()
            ))
        ),
        None => {}
    }
}

// ---------------------------------------------------------------------------
// kafae config edit
// ---------------------------------------------------------------------------

pub fn edit() {
    // the commented template is most of what the file is for, so an absent one is
    // written before the editor opens rather than left as an empty buffer
    let fresh = config::config_file().is_some_and(|path| !path.exists());
    let path = config::ensure_file().unwrap_or_else(|error| fail(&error));
    if fresh {
        println!("{}", dim(format!("wrote {}", path.display())));
    }
    opener::edit(&[path]);
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"
version = 1
current = "ce"
cxx = "clang++"

[grader.ce]
url = "https://grader.example/"
login = "6xxxxxxx21"
cxxflags = "-O2 -std=c++20 -DLOCAL"
"#;

    // nothing here touches the machine: a fixture file and a fixed environment, so the
    // row a developer with their own config sees is the row this asserts on
    #[test]
    fn every_setting_names_the_layer_its_value_came_from() {
        let (file, _) = config::parse(FILE);
        let env = config::Env::of(&[("KAFAE_CC", "gcc-15")]);
        let rows = settings(&config::Config::new(file, env, None));
        let row = |name: &str| {
            rows.iter()
                .find(|(key, _)| key == name)
                .and_then(|(_, resolved)| resolved.as_ref())
                .map(|resolved| (resolved.value.as_str(), resolved.source.label()))
        };
        assert_eq!(row("grader"), Some(("ce", "config".to_string())));
        // the trailing slash is stripped, so the url reads as the account key sees it
        assert_eq!(
            row("url"),
            Some(("https://grader.example", "[grader.ce]".to_string()))
        );
        assert_eq!(
            row("login"),
            Some(("6xxxxxxx21", "[grader.ce]".to_string()))
        );
        assert_eq!(row("template"), Some(("default", "default".to_string())));
        assert_eq!(row("cc"), Some(("gcc-15", "KAFAE_CC".to_string())));
        assert_eq!(row("cxx"), Some(("clang++", "config".to_string())));
        assert_eq!(
            row("cxxflags"),
            Some(("-O2 -std=c++20 -DLOCAL", "[grader.ce]".to_string()))
        );
    }
}
