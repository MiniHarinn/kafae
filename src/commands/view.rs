use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::client::{api, api_bytes, authed_state, resolve_problem, statements_dir, title_of};
use crate::ui::{bold, dim, ebold, fail, fmt_num};

fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        _ => false,
    }
}

fn desktop_opener() -> &'static str {
    if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    }
}

fn parts(command: &str) -> (std::path::PathBuf, Vec<&str>) {
    let mut words = command.split_whitespace();
    let program = words
        .next()
        .unwrap_or_else(|| fail(&format!("{} needs a command", ebold("--open-with"))));
    let found =
        which::which(program).unwrap_or_else(|_| fail(&format!("{} not on PATH", ebold(program))));
    (found, words.collect())
}

// nothing waits for it, so it has to outlive us and keep off our stdio
fn open_detached(command: &str, path: &Path) {
    let (program, args) = parts(command);
    let mut viewer = Command::new(program);
    viewer
        .args(args)
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        viewer.process_group(0);
    }
    let _ = viewer.spawn();
}

// this one owns the terminal while it runs, so its exit status becomes ours
fn open_foreground(command: &str, path: &Path) -> ! {
    let (program, args) = parts(command);
    let status = Command::new(program)
        .args(args)
        .arg(path)
        .status()
        .unwrap_or_else(|error| fail(&error.to_string()));
    std::process::exit(status.code().unwrap_or(1));
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
                open_detached(desktop_opener(), &path);
            } else if let Some(command) = open_with {
                if detach {
                    open_detached(command, &path);
                } else {
                    open_foreground(command, &path);
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
