use crate::client::{authed_session, resolve_problem};
use crate::offline;
use crate::opener;
use crate::ui::dim;

pub fn run(problem: Option<&str>, submission: Option<i64>) {
    if offline::on() {
        offline::refuse("open");
    }
    // a session always names the account it belongs to, so there is a url to open
    let session = authed_session();
    let base = session.url.clone();

    // the grader has no per-problem page for students; this is the closest thing
    let url = match (submission, problem) {
        (Some(id), _) => format!("{base}/submissions/{id}"),
        (None, Some(reference)) => format!(
            "{base}/submissions/prob/{}",
            resolve_problem(&session, reference)["id"]
        ),
        (None, None) => format!("{base}/main/list"),
    };

    println!("{}", dim(&url));
    opener::detached(opener::desktop(), url.as_ref());
}
