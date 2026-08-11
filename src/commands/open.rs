use crate::client::{authed_state, resolve_problem};
use crate::opener;
use crate::ui::{dim, ebold, fail};

pub fn run(problem: Option<&str>, submission: Option<i64>) {
    let state = authed_state();
    let base = state.url.clone().unwrap_or_default();
    if base.is_empty() {
        fail(&format!("no grader url, run {}", ebold("kafae login")));
    }

    // the grader has no per-problem page for students; this is the closest thing
    let url = match (submission, problem) {
        (Some(id), _) => format!("{base}/submissions/{id}"),
        (None, Some(reference)) => format!(
            "{base}/submissions/prob/{}",
            resolve_problem(&state, reference)["id"]
        ),
        (None, None) => format!("{base}/main/list"),
    };

    println!("{}", dim(&url));
    opener::detached(opener::desktop(), url.as_ref());
}
