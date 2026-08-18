use std::fs;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::{json, Value};

use crate::client::{api, authed_state, resolve_problem, State};
use crate::compile::{compile_check, Check};
use crate::json;
use crate::ui::{accepted, bold, dim, ebold, edim, fail, show_verdict};

const POLL_SECS: u64 = 2;
const POLL_TIMEOUT: u64 = 300;
const TERMINAL: [&str; 3] = ["done", "compilation_error", "grader_error"];

// submit always answers with the same shape, so a compile that never got sent still parses
fn report(compile: &Value, sub: Option<&Value>) {
    json::emit(&json!({
        "compile": compile,
        "submission": sub.map(json::submission),
    }));
}

// the grader need not name the problem back, and we already know which one it was
fn stamped(sub: &Value, name: &str) -> Value {
    let mut sub = sub.clone();
    if json::text(&sub["problem_name"]) == Value::Null {
        sub["problem_name"] = json!(name);
    }
    sub
}

// a script has no terminal to watch, so poll quietly and answer once
fn wait_json(state: &State, compile: &Value, name: &str, sub_id: &Value) -> ! {
    let deadline = Instant::now() + Duration::from_secs(POLL_TIMEOUT);
    while Instant::now() < deadline {
        let sub = api(
            state,
            minreq::Method::Get,
            &format!("submissions/{sub_id}"),
            None,
        );
        if TERMINAL.contains(&sub["status"].as_str().unwrap_or("")) {
            report(compile, Some(&stamped(&sub, name)));
            std::process::exit(if accepted(&sub) { 0 } else { 1 });
        }
        sleep(Duration::from_secs(POLL_SECS));
    }
    fail(&format!(
        "no verdict after {POLL_TIMEOUT}s; check with {}",
        ebold(format!("kafae status {sub_id}"))
    ));
}

fn wait_verdict(state: &State, sub_id: &Value) -> ! {
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
            std::process::exit(if show_verdict(&sub, false) { 0 } else { 1 });
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
    let compile = check(file, no_check);
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let prob = resolve_problem(&state, problem.unwrap_or(stem));
    let name = prob["name"].as_str().unwrap_or("").to_string();

    let source = fs::read_to_string(file).unwrap_or_else(|error| fail(&error.to_string()));
    let filename = file.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let resp = api(
        &state,
        minreq::Method::Post,
        &format!("problems/{}/submissions", prob["id"]),
        Some(&json!({ "source": source, "filename": filename })),
    );
    if json::on() {
        if no_wait {
            report(&compile, Some(&stamped(&resp, &name)));
            return;
        }
        wait_json(&state, &compile, &name, &resp["id"]);
    }
    println!(
        "submitted {} to {} {}",
        bold(format!("#{}", resp["number"])),
        bold(&name),
        dim(format!("(id {})", resp["id"]))
    );
    if !no_wait {
        wait_verdict(&state, &resp["id"]);
    }
}

// the local check is the one failure that spends no submission, so it reports for itself
fn check(file: &Path, no_check: bool) -> Value {
    let outcome = if no_check {
        Check::Skipped(Some("skipped by --no-check".to_string()))
    } else {
        compile_check(file)
    };
    let (checked, ok, message) = match &outcome {
        Check::Skipped(reason) => (false, Value::Null, reason.clone()),
        Check::Ok => (true, json!(true), None),
        Check::Failed(message) => (true, json!(false), Some(message.clone())),
    };
    let compile = json!({ "checked": checked, "ok": ok, "message": message });

    match outcome {
        Check::Skipped(Some(reason)) if !no_check && !json::on() => eprintln!(
            "{}",
            style(format!("compile check skipped: {reason}"))
                .for_stderr()
                .yellow()
        ),
        Check::Ok if !json::on() => println!("{}", dim("compile check ok")),
        Check::Failed(_) => {
            if json::on() {
                report(&compile, None);
            } else {
                eprintln!(
                    "{} {}",
                    edim("if only your local toolchain is at fault, submit anyway with"),
                    ebold("--no-check")
                );
            }
            // your code, not the tool: the same exit 1 a wrong answer gets
            std::process::exit(1);
        }
        _ => {}
    }
    compile
}

#[cfg(test)]
mod tests {
    use super::*;

    // --no-wait answers from the POST reply alone, which need not name the problem back
    #[test]
    fn fills_in_the_problem_the_grader_left_out() {
        let bare = stamped(&json!({ "id": 12 }), "01_Expr_11");
        assert_eq!(bare["problem_name"], json!("01_Expr_11"));
        let empty = stamped(&json!({ "problem_name": "" }), "01_Expr_11");
        assert_eq!(empty["problem_name"], json!("01_Expr_11"));
    }

    #[test]
    fn takes_the_graders_word_over_ours_when_it_gave_one() {
        let named = stamped(&json!({ "problem_name": "02_Loop_3" }), "01_Expr_11");
        assert_eq!(named["problem_name"], json!("02_Loop_3"));
    }
}
