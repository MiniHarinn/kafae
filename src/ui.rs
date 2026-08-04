use std::fmt::Display;
use std::process;

use console::{style, Style, StyledObject, Term};
use serde_json::Value;

pub fn bold<D: Display>(text: D) -> StyledObject<D> {
    style(text).bold()
}

pub fn dim<D: Display>(text: D) -> StyledObject<D> {
    style(text).dim()
}

pub fn ebold<D: Display>(text: D) -> StyledObject<D> {
    style(text).for_stderr().bold()
}

pub fn edim<D: Display>(text: D) -> StyledObject<D> {
    style(text).for_stderr().dim()
}

pub fn err_tag() -> StyledObject<&'static str> {
    style("kafae:").for_stderr().red().bold()
}

pub fn fail(message: &str) -> ! {
    eprintln!("{} {message}", err_tag());
    process::exit(1);
}

pub fn fmt_num(value: f64) -> String {
    format!("{value}")
}

pub fn score_text(best: Option<f64>, full: Option<f64>) -> String {
    match (best, full) {
        (None, _) => dim("-").to_string(),
        (Some(best), None) => fmt_num(best),
        (Some(best), Some(full)) => {
            let text = format!("{}/{}", fmt_num(best), fmt_num(full));
            let color = if best == full {
                Style::new().green()
            } else {
                Style::new().yellow()
            };
            color.apply_to(text).to_string()
        }
    }
}

pub fn show_verdict(sub: &Value, full_score: Option<f64>) -> bool {
    let status = sub["status"].as_str().unwrap_or("");
    let points = sub["points"].as_f64();
    let ok = status == "done" && points == full_score;

    let mark = if ok {
        style("✓ ").green().bold()
    } else {
        style("✗ ").red().bold()
    };
    let mut line = format!("{mark}{}  {}", bold(status), score_text(points, full_score));
    if let Some(runtime) = sub["max_runtime"].as_f64() {
        line.push_str(&format!("  {}", dim(format!("{}s", fmt_num(runtime)))));
    }
    println!("{line}");

    if let Some(comment) = sub["grader_comment"].as_str() {
        if !comment.is_empty() {
            let marks: String = comment
                .chars()
                .map(|c| {
                    let color = if c == 'P' || c == 'p' {
                        Style::new().green()
                    } else {
                        Style::new().red()
                    };
                    color.apply_to(c).to_string()
                })
                .collect();
            println!("{marks}");
        }
    }
    if status == "compilation_error" {
        if let Some(message) = sub["compiler_message"].as_str() {
            if !message.is_empty() {
                panel(false, "compiler", message.trim_end());
            }
        }
    }
    ok
}

pub fn panel(to_stderr: bool, title: &str, content: &str) {
    let term = if to_stderr {
        Term::stderr()
    } else {
        Term::stdout()
    };
    let width = term
        .size_checked()
        .map(|(_, cols)| cols as usize)
        .unwrap_or(80)
        .max(8);
    let inner = width - 2;
    let body = inner - 2;
    let mut red = Style::new().red();
    if to_stderr {
        red = red.for_stderr();
    }

    let head = format!(" {title} ");
    let head_len = head.chars().count().min(inner);
    let left = (inner - head_len) / 2;
    let right = inner - head_len - left;
    let mut lines = vec![format!(
        "{}{}{head}{}{}",
        red.apply_to("╭"),
        red.apply_to("─".repeat(left)),
        red.apply_to("─".repeat(right)),
        red.apply_to("╮")
    )];
    for raw in content.lines() {
        let chars: Vec<char> = raw.chars().collect();
        let mut start = 0;
        loop {
            let end = (start + body).min(chars.len());
            let chunk: String = chars[start..end].iter().collect();
            let pad = " ".repeat(body - (end - start));
            lines.push(format!(
                "{} {chunk}{pad} {}",
                red.apply_to("│"),
                red.apply_to("│")
            ));
            start = end;
            if start >= chars.len() {
                break;
            }
        }
    }
    lines.push(format!(
        "{}{}{}",
        red.apply_to("╰"),
        red.apply_to("─".repeat(inner)),
        red.apply_to("╯")
    ));
    for line in lines {
        if to_stderr {
            eprintln!("{line}");
        } else {
            println!("{line}");
        }
    }
}
