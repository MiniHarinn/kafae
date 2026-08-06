use std::cmp::Ordering;

use clap::ValueEnum;
use glob::{MatchOptions, Pattern};
use jiff::Timestamp;
use serde_json::Value;

use crate::client::{authed_state, get_problems, title_of};
use crate::ui::{bold, dim, ebold, fail, fmt_num, informative, score_text, since, table, Column};

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Sort {
    Name,
    Score,
    Tries,
    Recent,
    Difficulty,
}

impl Sort {
    fn descending(self) -> bool {
        matches!(self, Sort::Tries | Sort::Recent)
    }
}

pub struct Filter {
    pub pattern: Option<String>,
    pub solved: bool,
    pub unsolved: bool,
    pub untried: bool,
    pub partial: bool,
    pub tag: Option<String>,
}

impl Filter {
    fn is_set(&self) -> bool {
        self.pattern.is_some()
            || self.solved
            || self.unsolved
            || self.untried
            || self.partial
            || self.tag.is_some()
    }
}

fn solved(problem: &Value) -> bool {
    problem["best_score"].as_f64().is_some_and(|s| s >= 100.0)
}

fn tried(problem: &Value) -> bool {
    problem["submission_count"].as_i64().unwrap_or(0) > 0
}

fn tagged(problem: &Value, tag: &str) -> bool {
    problem["tags"].as_array().is_some_and(|tags| {
        tags.iter()
            .any(|t| t.as_str().is_some_and(|t| t.eq_ignore_ascii_case(tag)))
    })
}

fn glob(pattern: &str) -> Pattern {
    let pattern = if pattern.contains(['*', '?', '[']) {
        pattern.to_string()
    } else {
        format!("*{pattern}*")
    };
    Pattern::new(&pattern)
        .unwrap_or_else(|error| fail(&format!("bad pattern {}: {error}", ebold(&pattern))))
}

fn keep(problem: &Value, filter: &Filter, pattern: Option<&Pattern>) -> bool {
    if filter.solved && !solved(problem) {
        return false;
    }
    if filter.unsolved && solved(problem) {
        return false;
    }
    if filter.untried && tried(problem) {
        return false;
    }
    if filter.partial && (!tried(problem) || solved(problem)) {
        return false;
    }
    if let Some(tag) = &filter.tag {
        if !tagged(problem, tag) {
            return false;
        }
    }
    if let Some(pattern) = pattern {
        let options = MatchOptions {
            case_sensitive: false,
            ..MatchOptions::new()
        };
        let name = problem["name"].as_str().unwrap_or("");
        if !pattern.matches_with(name, options)
            && !pattern.matches_with(&title_of(problem), options)
        {
            return false;
        }
    }
    true
}

fn name_of(problem: &Value) -> &str {
    problem["name"].as_str().unwrap_or("")
}

// a problem the key says nothing about sits at the bottom either way
fn rank<T: PartialOrd>(a: Option<T>, b: Option<T>, descending: bool) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => {
            let order = a.partial_cmp(&b).unwrap_or(Ordering::Equal);
            if descending {
                order.reverse()
            } else {
                order
            }
        }
    }
}

fn submitted_at(problem: &Value) -> Option<i64> {
    let stamp: Timestamp = problem["last_submission_time"].as_str()?.parse().ok()?;
    Some(stamp.as_second())
}

fn arrange(problems: &mut [Value], sort: Sort, reverse: bool) {
    let descending = sort.descending() != reverse;
    problems.sort_by(|a, b| {
        let order = match sort {
            Sort::Name => rank(Some(name_of(a)), Some(name_of(b)), descending),
            Sort::Score => rank(
                a["best_score"].as_f64(),
                b["best_score"].as_f64(),
                descending,
            ),
            Sort::Tries => rank(
                a["submission_count"].as_i64().filter(|count| *count > 0),
                b["submission_count"].as_i64().filter(|count| *count > 0),
                descending,
            ),
            Sort::Recent => rank(submitted_at(a), submitted_at(b), descending),
            Sort::Difficulty => rank(
                a["difficulty"].as_f64(),
                b["difficulty"].as_f64(),
                descending,
            ),
        };
        order.then_with(|| name_of(a).cmp(name_of(b)))
    });
}

pub fn run(filter: Filter, sort: Sort, reverse: bool) {
    let state = authed_state();
    let pattern = filter.pattern.as_deref().map(glob);
    // the api hands problems back in reverse course order
    let mut problems = get_problems(&state);
    problems.retain(|p| keep(p, &filter, pattern.as_ref()));
    arrange(&mut problems, sort, reverse);

    if problems.is_empty() {
        println!("{}", dim("no problems match"));
        return;
    }

    let solved_count = problems.iter().filter(|p| solved(p)).count();
    let tried_count = problems.iter().filter(|p| tried(p)).count();

    let column = |render: &dyn Fn(&Value) -> String| -> Vec<String> {
        problems.iter().map(render).collect()
    };
    let missing = || dim("-").to_string();
    let mut columns: Vec<Column> = vec![("id", true), ("name", false), ("score", true)];
    let mut cells = vec![
        column(&|p| dim(p["id"].to_string()).to_string()),
        column(&|p| bold(name_of(p)).to_string()),
        column(&|p| score_text(p["best_score"].as_f64())),
    ];

    let optional = [
        (
            ("tries", true),
            column(&|p| match p["submission_count"].as_i64().unwrap_or(0) {
                0 => missing(),
                count => count.to_string(),
            }),
        ),
        (
            ("last", false),
            column(&|p| {
                p["last_submission_time"]
                    .as_str()
                    .and_then(since)
                    .map(|when| dim(when).to_string())
                    .unwrap_or_else(missing)
            }),
        ),
        (
            ("difficulty", true),
            column(&|p| match p["difficulty"].as_f64() {
                Some(level) => fmt_num(level),
                None => missing(),
            }),
        ),
        (
            ("tags", false),
            column(&|p| {
                let tags: Vec<&str> = p["tags"]
                    .as_array()
                    .map(|tags| tags.iter().filter_map(|t| t.as_str()).collect())
                    .unwrap_or_default();
                if tags.is_empty() {
                    missing()
                } else {
                    dim(tags.join(",")).to_string()
                }
            }),
        ),
    ];
    for (header, values) in optional {
        if informative(&values) {
            columns.push(header);
            cells.push(values);
        }
    }
    columns.push(("title", false));
    cells.push(column(&|p| dim(title_of(p)).to_string()));

    let rows: Vec<Vec<String>> = (0..problems.len())
        .map(|row| cells.iter().map(|column| column[row].clone()).collect())
        .collect();
    table(&columns, &rows);
    let scope = if filter.is_set() { " shown" } else { "" };
    println!(
        "\n{}",
        dim(format!(
            "{solved_count} of {}{scope} solved · {tried_count} attempted",
            rows.len()
        ))
    );
}
