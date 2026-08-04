use serde_json::Value;

use crate::client::{api, authed_state, resolve_problem};
use crate::ui::{fail, show_verdict};

pub fn run(submission: Option<i64>, problem: Option<&str>) {
    let state = authed_state();
    let sub: Value = match (submission, problem) {
        (None, None) => fail("status needs a submission id or -p PROBLEM"),
        (Some(id), _) => api(
            &state,
            minreq::Method::Get,
            &format!("submissions/{id}"),
            None,
        ),
        (None, Some(problem)) => {
            let prob = resolve_problem(&state, problem);
            let subs = api(
                &state,
                minreq::Method::Get,
                &format!("problems/{}/submissions", prob["id"]),
                None,
            );
            let latest = subs
                .as_array()
                .and_then(|subs| subs.first())
                .unwrap_or_else(|| fail("no submissions yet"))
                .clone();
            api(
                &state,
                minreq::Method::Get,
                &format!("submissions/{}", latest["id"]),
                None,
            )
        }
    };
    std::process::exit(if show_verdict(&sub, true) { 0 } else { 1 });
}
