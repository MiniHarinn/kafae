use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::{
    api, api_bytes, authed_state, cached_problem_name, resolve_problem, statements_dir, title_of,
};
use crate::opener;
use crate::ui::{bold, dim, ebold, fail, fmt_num};

// what the last online view saw, so --cached can say the same things
#[derive(Default, Serialize, Deserialize)]
struct Statement {
    #[serde(default)]
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    full_score: Option<f64>,
    #[serde(default)]
    notes: Vec<String>,
    #[serde(default)]
    markdown: bool,
    #[serde(default)]
    description: String,
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        _ => false,
    }
}

fn pdf_path(name: &str) -> PathBuf {
    statements_dir().join(format!("{name}.pdf"))
}

fn statement_path(name: &str) -> PathBuf {
    statements_dir().join(format!("{name}.json"))
}

// --cached later reads back whatever we write here, so a half-written cache is a lie
fn cache(path: &PathBuf, bytes: impl AsRef<[u8]>) {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).unwrap_or_else(|error| fail(&error.to_string()));
    }
    fs::write(path, bytes).unwrap_or_else(|error| fail(&error.to_string()));
}

fn from_grader(problem: &str, text: bool) -> (String, Statement, Option<PathBuf>) {
    let state = authed_state();
    let prob = resolve_problem(&state, problem);
    let name = prob["name"].as_str().unwrap_or("").to_string();

    let mut notes = Vec::new();
    if let Some(langs) = prob["permitted_languages"].as_array() {
        let names: Vec<&str> = langs.iter().filter_map(|l| l["name"].as_str()).collect();
        if !names.is_empty() {
            notes.push(format!("{} only", names.join(", ")));
        }
    }
    if prob["has_attachment"].as_bool() == Some(true) {
        notes.push("attachment available".to_string());
    }

    let desc = api(
        &state,
        minreq::Method::Get,
        &format!("problems/{}/description", prob["id"]),
        None,
    );
    let statement = Statement {
        title: title_of(&prob),
        full_score: prob["full_score"].as_f64(),
        notes,
        markdown: truthy(&desc["markdown"]),
        description: desc["description"].as_str().unwrap_or("").to_string(),
    };

    let mut pdf = None;
    if !text {
        if let Some(bytes) = api_bytes(&state, &format!("problems/{}/files/pdf", prob["id"])) {
            let path = pdf_path(&name);
            cache(&path, bytes);
            pdf = Some(path);
        }
    }

    cache(
        &statement_path(&name),
        serde_json::to_string(&statement).unwrap(),
    );
    (name, statement, pdf)
}

fn from_cache(problem: &str) -> (String, Statement, Option<PathBuf>) {
    let name = cached_problem_name(problem).unwrap_or_else(|| {
        fail(&format!(
            "{} is not in the cached problem list, run {} online first",
            ebold(problem),
            ebold("kafae problems")
        ))
    });

    let statement: Option<Statement> = fs::read_to_string(statement_path(&name))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let pdf = Some(pdf_path(&name)).filter(|path| path.is_file());
    if statement.is_none() && pdf.is_none() {
        fail(&format!(
            "nothing cached for {}, run {} online first",
            ebold(&name),
            ebold(format!("kafae view {name}"))
        ));
    }
    (name, statement.unwrap_or_default(), pdf)
}

pub fn run(
    problem: &str,
    text: bool,
    open: bool,
    open_with: Option<&str>,
    detach: bool,
    cached: bool,
) {
    let (name, statement, pdf) = if cached {
        from_cache(problem)
    } else {
        from_grader(problem, text)
    };

    let mut head = bold(&name).to_string();
    if !statement.title.is_empty() {
        head.push_str(&format!("  {}", dim(&statement.title)));
    }
    if let Some(worth) = statement.full_score {
        head.push_str(&format!("  {}", dim(format!("· {} pts", fmt_num(worth)))));
    }
    println!("{head}");
    if !statement.notes.is_empty() {
        println!("{}", dim(statement.notes.join(" · ")));
    }
    println!();

    let mut shown = false;
    if !text {
        if let Some(path) = &pdf {
            println!("{} {}", dim("pdf:"), path.display());
            shown = true;
            if open {
                opener::detached(opener::desktop(), path.as_os_str());
            } else if let Some(command) = open_with {
                if detach {
                    opener::detached(command, path.as_os_str());
                } else {
                    opener::foreground(command, path.as_os_str());
                }
            }
        } else if open || open_with.is_some() {
            fail(&format!("problem {} has no PDF statement", ebold(&name)));
        }
    }

    if !statement.description.is_empty() {
        if statement.markdown {
            termimad::print_text(&statement.description);
        } else {
            println!("{}", statement.description);
        }
        shown = true;
    }

    if !shown {
        if text {
            fail(&format!(
                "problem {} has no text description; drop {} for the PDF",
                ebold(&name),
                ebold("--text")
            ));
        }
        fail(&format!("problem {} has no statement", ebold(&name)));
    }
}
