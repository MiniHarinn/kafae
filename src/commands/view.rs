use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::client::{
    self, api, api_bytes, attachment_of, cache_write, resolve_problem, state_for_reads,
    statement_file, title_of,
};
use crate::json;
use crate::offline;
use crate::opener;
use crate::ui::{bold, dim, ebold, fail, fmt_num};

// the problem and its description, as either the grader or the cache hands them over
struct Statement {
    title: String,
    full_score: Option<f64>,
    permitted_languages: Vec<String>,
    has_attachment: bool,
    markdown: bool,
    description: String,
}

impl Statement {
    // the header line reads them out; --json hands over the fields they were made from
    fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if !self.permitted_languages.is_empty() {
            notes.push(format!("{} only", self.permitted_languages.join(", ")));
        }
        if self.has_attachment {
            notes.push("attachment available".to_string());
        }
        notes
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        _ => false,
    }
}

fn pdf_path(name: &str) -> PathBuf {
    statement_file(name, "pdf")
}

fn statement_path(name: &str) -> PathBuf {
    statement_file(name, "json")
}

fn statement_from(prob: &Value, desc: &Value) -> Statement {
    Statement {
        title: title_of(prob),
        full_score: prob["full_score"].as_f64(),
        permitted_languages: prob["permitted_languages"]
            .as_array()
            .map(|langs| {
                langs
                    .iter()
                    .filter_map(|l| l["name"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        has_attachment: prob["has_attachment"].as_bool() == Some(true),
        markdown: truthy(&desc["markdown"]),
        description: desc["description"].as_str().unwrap_or("").to_string(),
    }
}

fn load(problem: &str, text: bool) -> (String, Statement, Option<PathBuf>, Option<PathBuf>) {
    let state = state_for_reads();
    let prob = resolve_problem(&state, problem);
    let name = prob["name"].as_str().unwrap_or("").to_string();

    // a synced problem with no description is a problem that has none, so Null will do
    let desc = if offline::on() {
        fs::read_to_string(statement_path(&name))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null)
    } else {
        let desc = api(
            &state,
            minreq::Method::Get,
            &format!("problems/{}/description", prob["id"]),
            None,
        );
        cache_write(
            &statement_path(&name),
            serde_json::to_string(&desc).unwrap(),
        );
        desc
    };

    let pdf = if text {
        None
    } else if offline::on() {
        Some(pdf_path(&name)).filter(|path| path.is_file())
    } else {
        api_bytes(&state, &format!("problems/{}/files/pdf", prob["id"])).map(|bytes| {
            let path = pdf_path(&name);
            cache_write(&path, bytes);
            path
        })
    };

    // the grader ships a file with some problems; fetched like the PDF, so a plain view
    // is enough to put it on disk and the note above can say where it went
    let attachment = if text {
        None
    } else {
        attachment_of(&state, &prob)
    };

    // --text hides a cached PDF rather than proving nothing was synced
    if offline::on() && desc.is_null() && !pdf_path(&name).is_file() {
        fail(&format!(
            "nothing synced for {}, run {} online first",
            ebold(&name),
            ebold(format!("kafae sync {name}"))
        ));
    }
    (name, statement_from(&prob, &desc), pdf, attachment)
}

// no PDF and no description: the grader has a problem here, but nothing to read
fn nothing_to_show(name: &str, text: bool) -> ! {
    if text {
        fail(&format!(
            "problem {} has no text description; drop {} for the PDF",
            ebold(name),
            ebold("--text")
        ));
    }
    fail(&format!("problem {} has no statement", ebold(name)));
}

pub fn run(problem: &str, text: bool, open: bool, open_with: Option<&str>, detach: bool) {
    let (name, statement, pdf, attachment) = load(problem, text);
    client::remember_view(&name);

    if json::on() {
        if pdf.is_none() && attachment.is_none() && statement.description.is_empty() {
            nothing_to_show(&name, text);
        }
        json::emit(&json!({
            "name": name,
            "title": (!statement.title.is_empty()).then_some(statement.title),
            "full_score": statement.full_score,
            "permitted_languages": statement.permitted_languages,
            "has_attachment": statement.has_attachment,
            "markdown": statement.markdown,
            "description": statement.description,
            "pdf_path": pdf.map(|path| path.display().to_string()),
            "attachment_path": attachment.map(|path| path.display().to_string()),
        }));
        return;
    }

    let mut head = bold(&name).to_string();
    if !statement.title.is_empty() {
        head.push_str(&format!("  {}", dim(&statement.title)));
    }
    if let Some(worth) = statement.full_score {
        head.push_str(&format!("  {}", dim(format!("· {} pts", fmt_num(worth)))));
    }
    println!("{head}");
    let notes = statement.notes();
    if !notes.is_empty() {
        println!("{}", dim(notes.join(" · ")));
    }
    println!();

    let mut shown = false;
    if !text {
        if let Some(path) = &pdf {
            println!("{} {}", dim("pdf:"), path.display());
            shown = true;
            if open {
                opener::detached(opener::desktop(), path.as_os_str());
            } else if let Some(command) = open_with {
                if detach {
                    opener::detached(command, path.as_os_str());
                } else {
                    opener::foreground(command, path.as_os_str());
                }
            }
        } else if open || open_with.is_some() {
            if offline::on() {
                fail(&format!(
                    "no PDF synced for {}, run {} online first",
                    ebold(&name),
                    ebold(format!("kafae sync {name}"))
                ));
            }
            fail(&format!("problem {} has no PDF statement", ebold(&name)));
        }
        // the grader named this file, so it keeps that name and only the path is news
        if let Some(path) = &attachment {
            println!("{} {}", dim("attachment:"), path.display());
            shown = true;
        }
    }

    if !statement.description.is_empty() {
        if statement.markdown {
            termimad::print_text(&statement.description);
        } else {
            println!("{}", statement.description);
        }
        shown = true;
    }

    if !shown {
        nothing_to_show(&name, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_statement_reads_the_same_from_the_grader_and_from_the_cache() {
        let prob = json!({
            "name": "01_Expr_11",
            "full_name": "Expressions",
            "full_score": 10.0,
            "has_attachment": true,
            "permitted_languages": [{"name": "cpp"}, {"name": "py"}],
        });
        let desc = json!({ "markdown": 1, "description": "# Add two numbers" });
        let statement = statement_from(&prob, &desc);
        assert_eq!(statement.title, "Expressions");
        assert_eq!(statement.full_score, Some(10.0));
        assert_eq!(statement.permitted_languages, ["cpp", "py"]);
        assert!(statement.markdown);
        assert_eq!(statement.notes(), ["cpp, py only", "attachment available"]);
    }

    // offline a problem with nothing written beside it still has to render
    #[test]
    fn a_missing_description_is_an_empty_one() {
        let statement = statement_from(&json!({"name": "01_Expr_11"}), &Value::Null);
        assert_eq!(statement.description, "");
        assert!(!statement.markdown);
        assert_eq!(statement.full_score, None);
        assert!(statement.notes().is_empty());
    }
}
