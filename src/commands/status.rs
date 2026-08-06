use serde_json::Value;

use crate::client::{authed_state, get_submission, latest_submission};
use crate::ui::{ebold, fail, show_verdict};

pub fn run(submission: Option<i64>, problem: Option<&str>) {
    let state = authed_state();
    let sub: Value = match (submission, problem) {
        (None, None) => fail(&format!(
            "status needs a submission id or {}",
            ebold("-p PROBLEM")
        )),
        (Some(id), _) => get_submission(&state, id),
        (None, Some(problem)) => latest_submission(&state, problem),
    };
    std::process::exit(if show_verdict(&sub, true) { 0 } else { 1 });
}
