use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressStyle};
use serde_json::{json, Value};

use crate::client::{api, authed_state, resolve_problem, State};
use crate::compile::compile_check;
use crate::ui::{bold, dim, ebold, edim, fail, show_verdict};

const POLL_SECS: u64 = 2;
const POLL_TIMEOUT: u64 = 300;
const TERMINAL: [&str; 3] = ["done", "compilation_error", "grader_error"];

fn wait_verdict(state: &State, sub_id: &Value, full_score: Option<f64>) -> ! {
    let deadline = Instant::now() + Duration::from_secs(POLL_TIMEOUT);
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
    );
    spinner.enable_steady_tick(Duration::from_millis(80));
    spinner.set_message("waiting for verdict...");
    while Instant::now() < deadline {
        let sub = spinner.suspend(|| {
            api(
                state,
                minreq::Method::Get,
                &format!("submissions/{sub_id}"),
                None,
            )
        });
        let status = sub["status"].as_str().unwrap_or("").to_string();
        if TERMINAL.contains(&status.as_str()) {
            spinner.finish_and_clear();
            std::process::exit(if show_verdict(&sub, full_score) { 0 } else { 1 });
        }
        spinner.set_message(edim(format!("{status}...")).to_string());
        sleep(Duration::from_secs(POLL_SECS));
    }
    spinner.finish_and_clear();
    fail(&format!(
        "no verdict after {POLL_TIMEOUT}s; check with {}",
        ebold(format!("kafae status {sub_id}"))
    ));
}

pub fn run(file: &Path, problem: Option<&str>, no_wait: bool, no_check: bool) {
    let state = authed_state();
    if !no_check {
        compile_check(file);
    }
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let prob = resolve_problem(&state, problem.unwrap_or(stem));

    let source = fs::read_to_string(file).unwrap_or_else(|error| fail(&error.to_string()));
    let filename = file.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let resp = api(
        &state,
        minreq::Method::Post,
        &format!("problems/{}/submissions", prob["id"]),
        Some(&json!({ "source": source, "filename": filename })),
    );
    println!(
        "submitted {} to {} {}",
        bold(format!("#{}", resp["number"])),
        bold(prob["name"].as_str().unwrap_or("")),
        dim(format!("(id {})", resp["id"]))
    );
    if !no_wait {
        wait_verdict(&state, &resp["id"], prob["full_score"].as_f64());
    }
}
