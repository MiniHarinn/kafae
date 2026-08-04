use std::fs;
use std::path::Path;

use glob::Pattern;
use serde_json::Value;

use crate::client::{authed_state, get_problems, resolve_problem, title_of};
use crate::ui::{bold, dim, ebold, fail};

const TEMPLATE: &str = "// {name}  {title}
#include <iostream>
using namespace std;

int main() {

    return 0;
}
";

pub fn run(problem: &str, force: bool) {
    let state = authed_state();

    let probs: Vec<Value> = if problem.contains(['*', '?', '[']) {
        // name only: the glob picks the files this writes, and those are named after it
        let pattern = Pattern::new(problem)
            .unwrap_or_else(|error| fail(&format!("bad pattern {}: {error}", ebold(problem))));
        let mut hits: Vec<Value> = get_problems(&state)
            .into_iter()
            .filter(|p| pattern.matches(p["name"].as_str().unwrap_or("")))
            .collect();
        if hits.is_empty() {
            fail(&format!(
                "no problems matching {}, see {}",
                ebold(problem),
                ebold("kafae problems")
            ));
        }
        hits.sort_by_key(|p| p["name"].as_str().unwrap_or("").to_string());
        hits
    } else {
        vec![resolve_problem(&state, problem)]
    };

    for prob in &probs {
        let name = prob["name"].as_str().unwrap_or("");
        let title = title_of(prob);
        let path = format!("{name}.cpp");
        if Path::new(&path).exists() && !force {
            println!("{}", dim(format!("{path}  exists, skipped")));
            continue;
        }
        fs::write(
            &path,
            TEMPLATE.replace("{name}", name).replace("{title}", &title),
        )
        .unwrap_or_else(|error| fail(&error.to_string()));
        println!("{}  {}", bold(&path), dim(&title));
    }
    if probs.len() == 1 {
        let name = probs[0]["name"].as_str().unwrap_or("");
        println!(
            "{} kafae view {name}   kafae submit {name}.cpp",
            dim("next:")
        );
    }
}
