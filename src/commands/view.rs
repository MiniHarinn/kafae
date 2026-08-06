use std::fs;

use serde_json::Value;

use crate::client::{api, api_bytes, authed_state, resolve_problem, statements_dir, title_of};
use crate::opener;
use crate::ui::{bold, dim, ebold, fail, fmt_num};

fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        _ => false,
    }
}

pub fn run(problem: &str, text: bool, open: bool, open_with: Option<&str>, detach: bool) {
    let state = authed_state();
    let prob = resolve_problem(&state, problem);
    let name = prob["name"].as_str().unwrap_or("");
    let mut shown = false;

    let mut head = format!("{}  {}", bold(name), dim(title_of(&prob)));
    if let Some(worth) = prob["full_score"].as_f64() {
        head.push_str(&format!("  {}", dim(format!("· {} pts", fmt_num(worth)))));
    }
    println!("{head}");

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
    if !notes.is_empty() {
        println!("{}", dim(notes.join(" · ")));
    }
    println!();

    if !text {
        if let Some(pdf) = api_bytes(&state, &format!("problems/{}/files/pdf", prob["id"])) {
            let dir = statements_dir();
            fs::create_dir_all(&dir).unwrap_or_else(|error| fail(&error.to_string()));
            let path = dir.join(format!("{name}.pdf"));
            fs::write(&path, pdf).unwrap_or_else(|error| fail(&error.to_string()));
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
            fail(&format!("problem {} has no PDF statement", ebold(name)));
        }
    }

    let desc = api(
        &state,
        minreq::Method::Get,
        &format!("problems/{}/description", prob["id"]),
        None,
    );
    if let Some(body) = desc["description"].as_str().filter(|s| !s.is_empty()) {
        if truthy(&desc["markdown"]) {
            termimad::print_text(body);
        } else {
            println!("{body}");
        }
        shown = true;
    }

    if !shown {
        if text {
            fail(&format!(
                "problem {} has no text description; drop {} for the PDF",
                ebold(name),
                ebold("--text")
            ));
        }
        fail(&format!("problem {} has no statement", ebold(name)));
    }
}
