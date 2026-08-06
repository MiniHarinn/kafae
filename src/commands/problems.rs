use crate::client::{authed_state, get_problems, title_of};
use crate::ui::{bold, dim, score_text, table};

pub fn run() {
    let state = authed_state();
    // the api hands problems back in reverse course order
    let mut problems = get_problems(&state);
    problems.sort_by_key(|p| p["name"].as_str().unwrap_or("").to_string());

    let solved = problems
        .iter()
        .filter(|p| p["best_score"].as_f64().is_some_and(|s| s >= 100.0))
        .count();
    let attempted = problems
        .iter()
        .filter(|p| p["submission_count"].as_i64().unwrap_or(0) > 0)
        .count();

    let rows: Vec<Vec<String>> = problems
        .iter()
        .map(|p| {
            let tries = match p["submission_count"].as_i64().unwrap_or(0) {
                0 => dim("-").to_string(),
                count => count.to_string(),
            };
            let title = title_of(p);
            vec![
                dim(p["id"].to_string()).to_string(),
                bold(p["name"].as_str().unwrap_or("")).to_string(),
                score_text(p["best_score"].as_f64()),
                tries,
                if title.is_empty() {
                    String::new()
                } else {
                    dim(title).to_string()
                },
            ]
        })
        .collect();

    table(
        &[
            ("id", true),
            ("name", false),
            ("score", true),
            ("tries", true),
            ("title", false),
        ],
        &rows,
    );
    println!(
        "\n{}",
        dim(format!(
            "{solved} of {} solved · {attempted} attempted",
            rows.len()
        ))
    );
}
