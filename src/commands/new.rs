use std::fs;
use std::path::PathBuf;

use glob::Pattern;
use serde_json::Value;

use crate::client::{
    from_env, get_problems, last_viewed, resolve_problem, state_for_reads, title_of,
};
use crate::language;
use crate::opener;
use crate::templates;
use crate::ui::{bold, dim, ebold, fail};
pub fn run(
    problem: Option<&str>,
    last_view: bool,
    template: Option<&str>,
    force: bool,
    edit: bool,
) {
    let remembered;
    let problem = match problem {
        Some(problem) => problem,
        None => {
            debug_assert!(last_view);
            remembered = last_viewed().unwrap_or_else(|| {
                fail(&format!(
                    "nothing viewed yet, run {} first",
                    ebold("kafae view")
                ))
            });
            println!("{} {}", dim("last viewed:"), bold(&remembered));
            &remembered
        }
    };
    // -t wins, then the environment, then the builtin every course can use
    let wanted = template
        .map(String::from)
        .or_else(|| from_env("KAFAE_TEMPLATE"))
        .unwrap_or_else(|| templates::DEFAULT.to_string());
    let start = templates::pick(&wanted).unwrap_or_else(|reason| fail(&reason));
    let state = state_for_reads();

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

    // --edit opens the file even when this call only found it already there
    let mut solutions = Vec::new();
    let mut refused = Vec::new();
    for prob in &probs {
        let name = prob["name"].as_str().unwrap_or("");
        let title = title_of(prob);
        let (ext, bytes) = match templates::open(&start, &state, prob, name, &title) {
            Ok(opened) => opened,
            Err(reason) => {
                refused.push(format!("{name} {reason}"));
                continue;
            }
        };
        if let Some(reason) = language::unusable(prob, &ext) {
            refused.push(format!("{name} {reason}"));
            continue;
        }
        let path = PathBuf::from(format!("{name}.{ext}"));
        solutions.push(path.clone());
        if path.exists() && !force {
            println!(
                "{}",
                dim(format!(
                    "{}  exists, skipped (--force to overwrite)",
                    path.display()
                ))
            );
            continue;
        }
        fs::write(&path, bytes).unwrap_or_else(|error| fail(&error.to_string()));
        println!("{}  {}", bold(path.display()), dim(&title));
    }

    // a glob can turn up problems this template cannot serve; every one of them refusing
    // is the command failing, not a note under files that got written
    if solutions.is_empty() {
        let mut only = refused.first().cloned().unwrap_or_default();
        if refused.len() > 1 {
            only.push_str(&format!(" (and {} more)", refused.len() - 1));
        }
        fail(&only);
    }
    for note in &refused {
        println!("{}", dim(format!("{note}, skipped")));
    }
    if let (1, Some(path)) = (probs.len(), solutions.first()) {
        let name = probs[0]["name"].as_str().unwrap_or("");
        println!(
            "{} kafae view {name}   kafae submit {}",
            dim("next:"),
            path.display()
        );
    }
    if edit {
        opener::edit(&solutions);
    }
}
