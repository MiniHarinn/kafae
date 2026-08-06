use glob::{MatchOptions, Pattern};
use serde_json::Value;

use crate::client::{authed_state, get_problems, title_of};
use crate::ui::{bold, dim, ebold, fail, score_text, table};

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

pub fn run(filter: Filter) {
    let state = authed_state();
    let pattern = filter.pattern.as_deref().map(glob);
    // the api hands problems back in reverse course order
    let mut problems = get_problems(&state);
    problems.retain(|p| keep(p, &filter, pattern.as_ref()));
    problems.sort_by_key(|p| p["name"].as_str().unwrap_or("").to_string());

    if problems.is_empty() {
        println!("{}", dim("no problems match"));
        return;
    }

    let solved_count = problems.iter().filter(|p| solved(p)).count();
    let tried_count = problems.iter().filter(|p| tried(p)).count();

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
    let scope = if filter.is_set() { " shown" } else { "" };
    println!(
        "\n{}",
        dim(format!(
            "{solved_count} of {}{scope} solved · {tried_count} attempted",
            rows.len()
        ))
    );
}
