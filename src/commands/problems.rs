use console::measure_text_width;

use crate::client::{authed_state, get_problems, title_of};
use crate::ui::{bold, dim, score_text};

fn pad(text: &str, width: usize, right: bool) -> String {
    let fill = " ".repeat(width.saturating_sub(measure_text_width(text)));
    if right {
        format!("{fill}{text}")
    } else {
        format!("{text}{fill}")
    }
}

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

    let rows: Vec<[String; 5]> = problems
        .iter()
        .map(|p| {
            let tries = match p["submission_count"].as_i64().unwrap_or(0) {
                0 => dim("-").to_string(),
                count => count.to_string(),
            };
            [
                p["id"].to_string(),
                p["name"].as_str().unwrap_or("").to_string(),
                score_text(p["best_score"].as_f64()),
                tries,
                title_of(p),
            ]
        })
        .collect();

    let headers = ["id", "name", "score", "tries", "title"];
    let mut widths = headers.map(measure_text_width);
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(measure_text_width(cell));
        }
    }

    let head =
        |text: &str, width, right| bold(dim(pad(text, width, right)).to_string()).to_string();
    println!(
        "{}",
        format!(
            "{}  {}  {}  {}  {}",
            head(headers[0], widths[0], true),
            head(headers[1], widths[1], false),
            head(headers[2], widths[2], true),
            head(headers[3], widths[3], true),
            head(headers[4], widths[4], false),
        )
        .trim_end()
    );
    for [id, name, score, tries, title] in &rows {
        println!(
            "{}",
            format!(
                "{}  {}  {}  {}  {}",
                pad(&dim(id).to_string(), widths[0], true),
                pad(&bold(name).to_string(), widths[1], false),
                pad(score, widths[2], true),
                pad(tries, widths[3], true),
                if title.is_empty() {
                    String::new()
                } else {
                    dim(title).to_string()
                },
            )
            .trim_end()
        );
    }
    println!(
        "\n{}",
        dim(format!(
            "{solved} of {} solved · {attempted} attempted",
            rows.len()
        ))
    );
}
