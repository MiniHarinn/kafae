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
    let rows: Vec<[String; 4]> = get_problems(&state)
        .iter()
        .map(|p| {
            [
                p["id"].to_string(),
                p["name"].as_str().unwrap_or("").to_string(),
                score_text(p["best_score"].as_f64()),
                title_of(p),
            ]
        })
        .collect();

    let headers = ["id", "name", "score", "title"];
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
            "{}  {}  {}  {}",
            head(headers[0], widths[0], true),
            head(headers[1], widths[1], false),
            head(headers[2], widths[2], false),
            head(headers[3], widths[3], false),
        )
        .trim_end()
    );
    for [id, name, score, title] in &rows {
        println!(
            "{}",
            format!(
                "{}  {}  {}  {}",
                pad(&dim(id).to_string(), widths[0], true),
                pad(&bold(name).to_string(), widths[1], false),
                pad(score, widths[2], false),
                if title.is_empty() {
                    String::new()
                } else {
                    dim(title).to_string()
                },
            )
            .trim_end()
        );
    }
}
