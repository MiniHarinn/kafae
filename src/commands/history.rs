use console::style;
use serde_json::{json, Value};

use crate::client::{api, authed_state, resolve_problem, title_of};
use crate::json;
use crate::ui::{bold, dim, informative, marks, score_text, since, table, Column};

fn result_text(sub: &Value) -> String {
    match sub["status"].as_str().unwrap_or("") {
        "compilation_error" => style("compile error").red().to_string(),
        "grader_error" => style("grader error").red().to_string(),
        "done" => match sub["grader_comment"].as_str().filter(|c| !c.is_empty()) {
            Some(comment) => marks(comment),
            None => dim("-").to_string(),
        },
        other => dim(if other.is_empty() { "grading" } else { other }).to_string(),
    }
}

pub fn run(problem: &str) {
    let state = authed_state();
    let prob = resolve_problem(&state, problem);
    let subs = api(
        &state,
        minreq::Method::Get,
        &format!("problems/{}/submissions", prob["id"]),
        None,
    );
    // the api sends newest first; oldest first reads as the story it is
    let mut subs = subs.as_array().cloned().unwrap_or_default();
    subs.reverse();

    let name = prob["name"].as_str().unwrap_or(problem);

    // nothing scored yet is not the same as a scored zero
    let best = subs
        .iter()
        .filter_map(|s| s["points"].as_f64())
        .fold(None, |best: Option<f64>, points| {
            Some(best.map_or(points, |best| best.max(points)))
        });

    if json::on() {
        json::emit(&json!({
            "problem": {
                "id": prob["id"].as_i64(),
                "name": name,
                "title": json::text(&prob["full_name"]),
            },
            "submissions": subs.iter().map(json::submission).collect::<Vec<Value>>(),
            "summary": {
                "attempts": subs.len(),
                "best_score": best,
                "latest_id": subs.last().and_then(|s| s["id"].as_i64()),
            },
        }));
        return;
    }

    if subs.is_empty() {
        println!("{}", dim(format!("no submissions yet for {name}")));
        return;
    }
    println!("{}  {}", bold(name), dim(title_of(&prob)));

    let column =
        |render: &dyn Fn(&Value) -> String| -> Vec<String> { subs.iter().map(render).collect() };
    let mut columns: Vec<Column> = vec![("#", true), ("id", true), ("score", true)];
    let mut cells = vec![
        column(&|s| bold(format!("#{}", s["number"])).to_string()),
        column(&|s| dim(s["id"].to_string()).to_string()),
        column(&|s| score_text(s["points"].as_f64())),
    ];
    for (header, values) in [
        (("result", false), column(&|s| result_text(s))),
        (
            ("lang", false),
            column(&|s| dim(s["language"].as_str().unwrap_or("")).to_string()),
        ),
        (
            ("when", false),
            column(&|s| {
                s["submitted_at"]
                    .as_str()
                    .and_then(since)
                    .map(|when| dim(when).to_string())
                    .unwrap_or_default()
            }),
        ),
    ] {
        if informative(&values) {
            columns.push(header);
            cells.push(values);
        }
    }

    let rows: Vec<Vec<String>> = (0..subs.len())
        .map(|row| cells.iter().map(|column| column[row].clone()).collect())
        .collect();
    table(&columns, &rows);
    println!(
        "\n{} {} {}",
        dim(format!("{} attempts · best", subs.len())),
        score_text(best),
        dim(format!("· kafae status {}", subs.last().unwrap()["id"])),
    );
}
