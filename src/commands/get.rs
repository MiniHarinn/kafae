use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::client::{authed_state, get_submission, latest_submission};
use crate::ui::{ago, bold, dim, ebold, fail};

fn provenance(sub: &Value) -> String {
    let mut parts = vec![format!(
        "{} #{}",
        sub["problem_name"].as_str().unwrap_or(""),
        sub["number"]
    )];
    if let Some(when) = sub["submitted_at"].as_str().and_then(ago) {
        parts.push(when);
    }
    parts.join(" · ")
}

pub fn run(submission: Option<i64>, problem: Option<&str>, output: Option<&Path>, force: bool) {
    let state = authed_state();
    let sub = match (submission, problem) {
        (None, None) => fail(&format!(
            "get needs a submission id or {}",
            ebold("-p PROBLEM")
        )),
        (Some(id), _) => get_submission(&state, id),
        (None, Some(problem)) => latest_submission(&state, problem),
    };

    // binaries and other people's submissions come back without one
    let Some(source) = sub["source"].as_str() else {
        fail(&format!(
            "the grader did not send the source of {}",
            ebold(format!("#{}", sub["id"]))
        ));
    };

    let Some(path) = output else {
        print!("{source}");
        return;
    };
    if path.exists() && !force {
        fail(&format!(
            "{} exists, pass {} to overwrite it",
            ebold(path.display()),
            ebold("--force")
        ));
    }
    fs::write(path, source).unwrap_or_else(|error| fail(&error.to_string()));
    println!("{}  {}", bold(path.display()), dim(provenance(&sub)));
}
