use std::fmt::Display;
use std::process;
use std::time::Duration;

use bytesize::ByteSize;
use console::{measure_text_width, style, Style, StyledObject, Term};
use jiff::Timestamp;
use serde_json::Value;

const ROW_LIMIT: usize = 8;

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

pub fn pad(text: &str, width: usize, right: bool) -> String {
    let fill = " ".repeat(width.saturating_sub(measure_text_width(text)));
    if right {
        format!("{fill}{text}")
    } else {
        format!("{text}{fill}")
    }
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

// points are normalised to 0-100; full_score is the course weight, not a maximum
pub fn score_text(points: Option<f64>) -> String {
    let Some(points) = points else {
        return dim("-").to_string();
    };
    let color = if points >= 100.0 {
        Style::new().green()
    } else if points <= 0.0 {
        Style::new().red()
    } else {
        Style::new().yellow()
    };
    color.apply_to(format!("{}%", fmt_num(points))).to_string()
}

pub fn fmt_runtime(millis: f64) -> String {
    if millis >= 1000.0 {
        format!("{:.2}s", millis / 1000.0)
    } else {
        format!("{}ms", fmt_num(millis))
    }
}

// the api reports memory in kibibytes
pub fn fmt_memory(kib: f64) -> String {
    ByteSize::kib(kib.max(0.0) as u64)
        .display()
        .iec()
        .to_string()
}

// Upstream Evaluation::RESULT_CODE, one char per testcase, groups bracketed:
//   ? waiting   P correct   - wrong   s partial      T time limit
//   M memory    x crash     E error   ! grader error
pub fn mark_style(code: char) -> Style {
    match code {
        'P' => Style::new().green(),
        's' => Style::new().cyan(),
        'T' | 'M' => Style::new().yellow(),
        '?' | '[' | ']' => Style::new().dim(),
        _ => Style::new().red(),
    }
}

struct Eval {
    result: String,
    score: f64,
    time: f64,
    memory: f64,
}

fn evaluations(sub: &Value) -> Vec<Eval> {
    sub["evaluations"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|e| Eval {
                    result: e["result"].as_str().unwrap_or("").to_string(),
                    score: e["score"].as_f64().unwrap_or(0.0),
                    time: e["time"].as_f64().unwrap_or(0.0),
                    memory: e["memory"].as_f64().unwrap_or(0.0),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn label(result: &str) -> &str {
    match result {
        "time_limit" => "time limit",
        "memory_limit" => "memory limit",
        other => other,
    }
}

fn verdict_word(result: &str) -> &str {
    match result {
        "wrong" => "wrong answer",
        "crash" => "runtime error",
        "waiting" => "still grading",
        other => label(other),
    }
}

// None when everything ties; a pointer only means something if one stands out
fn standout(values: impl Iterator<Item = f64>) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    let mut low = f64::MAX;
    for (i, v) in values.enumerate() {
        low = low.min(v);
        if best.is_none_or(|(_, b)| v > b) {
            best = Some((i, v));
        }
    }
    best.filter(|(_, b)| *b > low).map(|(i, _)| i)
}

// ask the testcases rather than points
fn full_marks(sub: &Value, evals: &[Eval]) -> bool {
    if !evals.is_empty() {
        return evals.iter().all(|e| e.result == "correct");
    }
    let comment = sub["grader_comment"].as_str().unwrap_or("");
    if !comment.is_empty() {
        return comment
            .chars()
            .filter(|c| !matches!(c, '[' | ']'))
            .all(|c| c == 'P');
    }
    sub["points"].as_f64().is_some_and(|points| points >= 100.0)
}

// grader runs every testcase, so naming one is honest only when it's the only one
fn headline(sub: &Value, evals: &[Eval], ok: bool) -> String {
    match sub["status"].as_str().unwrap_or("") {
        "compilation_error" => return "✗ compile error".to_string(),
        "grader_error" => return "✗ grader error".to_string(),
        "done" => {}
        other => return format!("⋯ {}", if other.is_empty() { "grading" } else { other }),
    }

    let total = evals.len();
    let bad: Vec<(usize, &Eval)> = evals
        .iter()
        .enumerate()
        .filter(|(_, e)| e.result != "correct")
        .collect();

    if total == 0 {
        return if ok {
            "✓ accepted"
        } else {
            "✗ not accepted"
        }
        .to_string();
    }
    if bad.is_empty() {
        return if total == 1 {
            "✓ accepted".to_string()
        } else {
            format!("✓ accepted · {total} tests")
        };
    }
    if let [(index, eval)] = bad[..] {
        let word = verdict_word(&eval.result);
        return if total == 1 {
            format!("✗ {word}")
        } else {
            format!("✗ {word} on test {} of {total}", index + 1)
        };
    }
    let first = &bad[0].1.result;
    if bad.iter().all(|(_, e)| &e.result == first) {
        return format!(
            "✗ {} on {} of {total} tests",
            verdict_word(first),
            bad.len()
        );
    }
    format!("✗ {} of {total} tests failed", bad.len())
}

fn resources(sub: &Value, evals: &[Eval]) -> String {
    let mut parts = Vec::new();
    let tag = |text: String, index: Option<usize>| match index {
        Some(i) => format!("{text} #{}", i + 1),
        None => text,
    };
    if let Some(runtime) = sub["max_runtime"].as_f64() {
        let at = standout(evals.iter().map(|e| e.time));
        parts.push(tag(fmt_runtime(runtime), at));
    }
    if let Some(memory) = sub["peak_memory"].as_f64() {
        let at = standout(evals.iter().map(|e| e.memory));
        parts.push(tag(fmt_memory(memory), at));
    }
    parts.join(" · ")
}

// at n=1 the row would only repeat the headline and the runtime
fn failure_rows(evals: &[Eval]) -> Vec<String> {
    if evals.len() < 2 {
        return Vec::new();
    }
    let bad: Vec<(usize, &Eval)> = evals
        .iter()
        .enumerate()
        .filter(|(_, e)| e.result != "correct")
        .collect();
    if bad.is_empty() {
        return Vec::new();
    }
    let name = bad
        .iter()
        .map(|(_, e)| label(&e.result).len())
        .max()
        .unwrap();
    let digits = bad.last().unwrap().0.to_string().len() + 1;

    let mut lines = Vec::new();
    for (index, eval) in bad.iter().take(ROW_LIMIT) {
        let mut line = format!(
            "  {}  {}  {}",
            dim(format!("#{:<width$}", index + 1, width = digits)),
            mark_style(code_of(&eval.result)).apply_to(format!("{:<name$}", label(&eval.result))),
            dim(format!("{:>7}", fmt_runtime(eval.time))),
        );
        if eval.result == "partial" {
            line.push_str(&format!("  {}", dim(fmt_num(eval.score))));
        } else if eval.result == "memory_limit" {
            line.push_str(&format!("  {}", dim(fmt_memory(eval.memory))));
        }
        lines.push(line);
    }
    if bad.len() > ROW_LIMIT {
        lines.push(dim(format!("  …and {} more", bad.len() - ROW_LIMIT)).to_string());
    }
    lines
}

fn code_of(result: &str) -> char {
    match result {
        "correct" => 'P',
        "partial" => 's',
        "time_limit" => 'T',
        "memory_limit" => 'M',
        "waiting" => '?',
        _ => '-',
    }
}

fn ago(stamp: &str) -> Option<String> {
    let then: Timestamp = stamp.parse().ok()?;
    let elapsed = Timestamp::now().as_second() - then.as_second();
    let elapsed = u64::try_from(elapsed).ok()?;
    Some(timeago::Formatter::new().convert(Duration::from_secs(elapsed)))
}

fn provenance(sub: &Value) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(name) = sub["problem_name"].as_str() {
        match sub["number"].as_i64() {
            Some(number) => parts.push(format!("{name} #{number}")),
            None => parts.push(name.to_string()),
        }
    }
    for key in ["source_filename", "language"] {
        if let Some(text) = sub[key].as_str().filter(|t| !t.is_empty()) {
            parts.push(text.to_string());
        }
    }
    if let Some(when) = sub["submitted_at"].as_str().and_then(ago) {
        parts.push(when);
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

pub fn show_verdict(sub: &Value, with_provenance: bool) -> bool {
    let evals = evaluations(sub);
    let status = sub["status"].as_str().unwrap_or("");
    let ok = status == "done" && full_marks(sub, &evals);

    let head = headline(sub, &evals, ok);
    let mut line = if ok {
        style(head).green().bold().to_string()
    } else if status == "done" || status.ends_with("_error") {
        style(head).red().bold().to_string()
    } else {
        dim(head).to_string()
    };
    if status == "done" {
        line.push_str(&format!("  {}", score_text(sub["points"].as_f64())));
    }
    let resources = resources(sub, &evals);
    if !resources.is_empty() {
        line.push_str(&format!("  {}", dim(resources)));
    }
    println!("{line}");

    if let Some(comment) = sub["grader_comment"].as_str().filter(|c| !c.is_empty()) {
        let marks: String = comment
            .chars()
            .map(|c| mark_style(c).apply_to(c).to_string())
            .collect();
        println!("{marks}");
    }
    for row in failure_rows(&evals) {
        println!("{row}");
    }
    if status == "compilation_error" {
        if let Some(message) = sub["compiler_message"].as_str().filter(|m| !m.is_empty()) {
            panel(false, "compiler", message.trim_end());
        }
    }
    if status == "grader_error" {
        println!(
            "{}",
            dim("the grader failed to run this submission; try submitting again")
        );
    }
    if with_provenance {
        if let Some(text) = provenance(sub) {
            println!("{}", dim(text));
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
