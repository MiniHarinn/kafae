use serde_json::{json, Value};

use crate::client::{authed_state, get_submission, latest_submission};
use crate::json;
use crate::ui::{accepted, ebold, fail, show_verdict};

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
    // the exit code is the verdict either way; --json only changes how it is spelled out
    let ok = if json::on() {
        json::emit(&json!({ "submission": json::submission(&sub) }));
        accepted(&sub)
    } else {
        show_verdict(&sub, true)
    };
    std::process::exit(if ok { 0 } else { 1 });
}
