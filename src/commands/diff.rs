use std::fs;
use std::path::Path;

use console::style;
use similar::{ChangeTag, TextDiff};

use crate::client::{authed_state, latest_submission};
use crate::ui::{ago, dim, ebold, fail};

const CONTEXT: usize = 3;

// windows editors rewrite every line ending; that is not a code change
fn unify(text: &str) -> String {
    text.replace("\r\n", "\n")
}

pub fn run(file: &Path, problem: Option<&str>) {
    let state = authed_state();
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let sub = latest_submission(&state, problem.unwrap_or(stem));
    let Some(theirs) = sub["source"].as_str() else {
        fail(&format!(
            "the grader did not send the source of {}",
            ebold(format!("#{}", sub["id"]))
        ));
    };
    let ours = fs::read_to_string(file).unwrap_or_else(|error| fail(&error.to_string()));

    let mut sent = format!(
        "{} #{}",
        sub["problem_name"].as_str().unwrap_or(""),
        sub["number"]
    );
    if let Some(when) = sub["submitted_at"].as_str().and_then(ago) {
        sent.push_str(&format!(", {when}"));
    }

    let theirs = unify(theirs);
    let ours = unify(&ours);
    if theirs == ours {
        println!("{}", dim(format!("same as {sent}")));
        return;
    }

    let diff = TextDiff::from_lines(theirs.as_str(), ours.as_str());
    println!("{}", style(format!("--- {sent}")).red().bold());
    println!(
        "{}",
        style(format!("+++ {}", file.display())).green().bold()
    );
    for group in diff.grouped_ops(CONTEXT) {
        if let Some(op) = group.first() {
            println!(
                "{}",
                style(format!(
                    "@@ -{} +{} @@",
                    op.old_range().start + 1,
                    op.new_range().start + 1
                ))
                .cyan()
            );
        }
        for op in &group {
            for change in diff.iter_changes(op) {
                let text = change.value();
                let text = text.strip_suffix('\n').unwrap_or(text);
                println!(
                    "{}",
                    match change.tag() {
                        ChangeTag::Delete => style(format!("-{text}")).red(),
                        ChangeTag::Insert => style(format!("+{text}")).green(),
                        ChangeTag::Equal => style(format!(" {text}")).dim(),
                    }
                );
            }
        }
    }
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crlf_file_reads_as_the_same_code() {
        assert_eq!(unify("int main() {\r\n}\r\n"), "int main() {\n}\n");
        assert_eq!(unify("already\nunix\n"), "already\nunix\n");
    }

    // a lone carriage return is data, not a line ending we put there
    #[test]
    fn leaves_a_bare_carriage_return_alone() {
        assert_eq!(unify("spin\rtext"), "spin\rtext");
    }
}
