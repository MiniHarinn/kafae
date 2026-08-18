use std::cmp::Ordering;

use clap::ValueEnum;
use glob::{MatchOptions, Pattern};
use jiff::Timestamp;
use serde_json::{json, Value};

use crate::client::{authed_state, get_problems, title_of};
use crate::json;
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

// the table hides a column that says nothing; JSON never does, or the shape would
// depend on the data in it
fn entry(problem: &Value) -> Value {
    json!({
        "id": problem["id"].as_i64(),
        "name": name_of(problem),
        "title": json::text(&problem["full_name"]),
        "best_score": problem["best_score"].as_f64(),
        "submission_count": problem["submission_count"].as_i64().unwrap_or(0),
        "last_submission_time": json::text(&problem["last_submission_time"]),
        "difficulty": problem["difficulty"].as_f64(),
        "tags": problem["tags"].as_array().cloned().unwrap_or_default(),
        "has_testcase": problem["has_testcase"].as_bool(),
        "solved": solved(problem),
        "tried": tried(problem),
    })
}

pub fn run(filter: Filter, sort: Sort, reverse: bool) {
    let state = authed_state();
    let pattern = filter.pattern.as_deref().map(glob);
    // the api hands problems back in reverse course order
    let mut problems = get_problems(&state);
    problems.retain(|p| keep(p, &filter, pattern.as_ref()));
    arrange(&mut problems, sort, reverse);

    if json::on() {
        json::emit(&json!({
            "problems": problems.iter().map(entry).collect::<Vec<Value>>(),
            "summary": {
                "total": problems.len(),
                "solved": problems.iter().filter(|p| solved(p)).count(),
                "attempted": problems.iter().filter(|p| tried(p)).count(),
            },
        }));
        return;
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn problem(name: &str, title: &str, score: Option<f64>, tries: i64) -> Value {
        json!({
            "name": name,
            "full_name": title,
            "best_score": score,
            "submission_count": tries,
            "tags": ["ComProg", "Week1"],
        })
    }

    fn at(name: &str, stamp: Option<&str>) -> Value {
        let mut problem = problem(name, "A Title", Some(50.0), 1);
        problem["last_submission_time"] = json!(stamp);
        problem
    }

    fn nothing() -> Filter {
        Filter {
            pattern: None,
            solved: false,
            unsolved: false,
            untried: false,
            partial: false,
            tag: None,
        }
    }

    fn kept(problems: &[Value], filter: &Filter) -> Vec<String> {
        let pattern = filter.pattern.as_deref().map(glob);
        problems
            .iter()
            .filter(|p| keep(p, filter, pattern.as_ref()))
            .map(|p| name_of(p).to_string())
            .collect()
    }

    fn sample() -> Vec<Value> {
        vec![
            problem("01_Expr_11", "Arithmetic Expressions", Some(100.0), 3),
            problem("02_Loop_3", "Nested Loops", Some(40.0), 5),
            problem("03_Str_7", "String Handling", None, 0),
        ]
    }

    #[test]
    fn filters_by_state() {
        let solved = Filter {
            solved: true,
            ..nothing()
        };
        assert_eq!(kept(&sample(), &solved), ["01_Expr_11"]);
        let unsolved = Filter {
            unsolved: true,
            ..nothing()
        };
        assert_eq!(kept(&sample(), &unsolved), ["02_Loop_3", "03_Str_7"]);
        let untried = Filter {
            untried: true,
            ..nothing()
        };
        assert_eq!(kept(&sample(), &untried), ["03_Str_7"]);
        let partial = Filter {
            partial: true,
            ..nothing()
        };
        assert_eq!(kept(&sample(), &partial), ["02_Loop_3"]);
    }

    #[test]
    fn matches_a_tag_whatever_its_case() {
        let tagged = Filter {
            tag: Some("comprog".to_string()),
            ..nothing()
        };
        assert_eq!(kept(&sample(), &tagged).len(), 3);
        let missing = Filter {
            tag: Some("Week2".to_string()),
            ..nothing()
        };
        assert!(kept(&sample(), &missing).is_empty());
    }

    #[test]
    fn a_plain_word_matches_anywhere_in_the_name_or_title() {
        let word = |text: &str| Filter {
            pattern: Some(text.to_string()),
            ..nothing()
        };
        assert_eq!(kept(&sample(), &word("expr")), ["01_Expr_11"]);
        assert_eq!(kept(&sample(), &word("nested")), ["02_Loop_3"]);
        assert!(kept(&sample(), &word("nothing")).is_empty());
    }

    #[test]
    fn a_wildcard_is_taken_as_written() {
        let anchored = Filter {
            pattern: Some("01_*".to_string()),
            ..nothing()
        };
        assert_eq!(kept(&sample(), &anchored), ["01_Expr_11"]);
        let unanchored = Filter {
            pattern: Some("*Loop*".to_string()),
            ..nothing()
        };
        assert_eq!(kept(&sample(), &unanchored), ["02_Loop_3"]);
    }

    #[test]
    fn a_missing_key_sinks_either_way() {
        assert_eq!(rank(None::<f64>, Some(1.0), false), Ordering::Greater);
        assert_eq!(rank(None::<f64>, Some(1.0), true), Ordering::Greater);
        assert_eq!(rank(Some(1.0), None::<f64>, true), Ordering::Less);
        assert_eq!(rank(Some(1.0), Some(2.0), false), Ordering::Less);
        assert_eq!(rank(Some(1.0), Some(2.0), true), Ordering::Greater);
    }

    fn ordered(sort: Sort, reverse: bool) -> Vec<String> {
        let mut problems = sample();
        arrange(&mut problems, sort, reverse);
        problems.iter().map(|p| name_of(p).to_string()).collect()
    }

    #[test]
    fn sorts_and_leaves_the_unscored_last() {
        assert_eq!(
            ordered(Sort::Score, false),
            ["02_Loop_3", "01_Expr_11", "03_Str_7"]
        );
        assert_eq!(
            ordered(Sort::Score, true),
            ["01_Expr_11", "02_Loop_3", "03_Str_7"]
        );
        assert_eq!(
            ordered(Sort::Tries, false),
            ["02_Loop_3", "01_Expr_11", "03_Str_7"]
        );
        assert_eq!(
            ordered(Sort::Name, false),
            ["01_Expr_11", "02_Loop_3", "03_Str_7"]
        );
        assert_eq!(
            ordered(Sort::Name, true),
            ["03_Str_7", "02_Loop_3", "01_Expr_11"]
        );
    }

    #[test]
    fn an_untried_problem_never_leads_the_tries_order() {
        assert_eq!(
            ordered(Sort::Tries, true),
            ["01_Expr_11", "02_Loop_3", "03_Str_7"]
        );
        assert_eq!(
            ordered(Sort::Tries, false),
            ["02_Loop_3", "01_Expr_11", "03_Str_7"]
        );
    }

    #[test]
    fn sorts_recent_newest_first_and_parses_the_graders_stamps() {
        assert_eq!(
            submitted_at(&at("x", Some("2026-08-01T09:12:33Z"))),
            Some(1785575553)
        );
        assert_eq!(
            submitted_at(&at("x", Some("2026-08-01T16:12:33+07:00"))),
            Some(1785575553)
        );
        assert_eq!(submitted_at(&at("x", None)), None);

        let mut problems = vec![
            at("older", Some("2026-08-01T09:12:33Z")),
            at("never", None),
            at("newer", Some("2026-08-05T09:12:33Z")),
        ];
        arrange(&mut problems, Sort::Recent, false);
        let names: Vec<&str> = problems.iter().map(name_of).collect();
        assert_eq!(names, ["newer", "older", "never"]);
        arrange(&mut problems, Sort::Recent, true);
        let names: Vec<&str> = problems.iter().map(name_of).collect();
        assert_eq!(names, ["older", "newer", "never"]);
    }

    // the table drops a column that says nothing; a script would read that as a missing field
    #[test]
    fn the_json_entry_keeps_every_field_whatever_the_grader_sent() {
        let keys =
            |value: &Value| -> Vec<String> { value.as_object().unwrap().keys().cloned().collect() };
        let sparse = entry(&json!({ "name": "03_Str_7" }));
        let full = entry(&problem("01_Expr_11", "Expressions", Some(100.0), 3));
        assert_eq!(keys(&sparse), keys(&full));
        assert_eq!(sparse["best_score"], Value::Null);
        assert_eq!(sparse["difficulty"], Value::Null);
        assert_eq!(sparse["last_submission_time"], Value::Null);
        assert_eq!(sparse["submission_count"], json!(0));
        assert_eq!(sparse["tags"], json!([]));
    }

    #[test]
    fn the_json_entry_says_solved_and_tried_so_a_script_need_not_guess() {
        let solved = entry(&problem("01_Expr_11", "Expressions", Some(100.0), 3));
        assert_eq!(solved["solved"], json!(true));
        assert_eq!(solved["tried"], json!(true));
        assert_eq!(solved["title"], json!("Expressions"));
        let untried = entry(&problem("03_Str_7", "String Handling", None, 0));
        assert_eq!(untried["solved"], json!(false));
        assert_eq!(untried["tried"], json!(false));
        let partial = entry(&problem("02_Loop_3", "Nested Loops", Some(40.0), 5));
        assert_eq!(partial["solved"], json!(false));
        assert_eq!(partial["tried"], json!(true));
    }

    #[test]
    fn a_tie_is_broken_by_name() {
        let mut problems = vec![
            problem("02_Loop_3", "b", Some(50.0), 1),
            problem("01_Expr_11", "a", Some(50.0), 1),
        ];
        arrange(&mut problems, Sort::Score, false);
        let names: Vec<&str> = problems.iter().map(name_of).collect();
        assert_eq!(names, ["01_Expr_11", "02_Loop_3"]);
    }

    #[test]
    fn falls_back_to_the_name_when_the_key_ties() {
        assert_eq!(
            ordered(Sort::Difficulty, false),
            ["01_Expr_11", "02_Loop_3", "03_Str_7"]
        );
        assert_eq!(
            ordered(Sort::Difficulty, true),
            ["01_Expr_11", "02_Loop_3", "03_Str_7"]
        );
    }
}
