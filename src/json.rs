use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{json, Value};

use crate::ui;

static ON: AtomicBool = AtomicBool::new(false);

// under --json stdout is a document, so styling and progress have to get out of it
pub fn enable() {
    ON.store(true, Ordering::Relaxed);
    console::set_colors_enabled(false);
    console::set_colors_enabled_stderr(false);
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

// one compact object per line, so test --watch is a stream and everything else is one line
pub fn emit(value: &Value) {
    println!("{value}");
}

pub fn fail(kind: &str, message: &str, detail: Option<Value>) -> ! {
    let mut error = json!({ "kind": kind, "message": message });
    if let Some(detail) = detail {
        error["detail"] = detail;
    }
    eprintln!("{}", json!({ "error": error }));
    std::process::exit(2);
}

// a field the grader sends empty is a field it did not send
pub fn text(value: &Value) -> Value {
    match value.as_str().map(str::trim) {
        Some(text) if !text.is_empty() => json!(text),
        _ => Value::Null,
    }
}

// the grader reports runtimes in milliseconds and memory in kibibytes; say so in the name
fn evaluations(sub: &Value) -> Vec<Value> {
    sub["evaluations"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|e| {
                    json!({
                        "result": text(&e["result"]),
                        "score": e["score"].as_f64(),
                        "time_ms": e["time"].as_f64(),
                        "memory_kib": e["memory"].as_f64(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

// the one submission shape, so status, history, submit and get all read alike
pub fn submission(sub: &Value) -> Value {
    json!({
        "id": sub["id"].as_i64(),
        "number": sub["number"].as_i64(),
        "problem": text(&sub["problem_name"]),
        "status": text(&sub["status"]),
        "points": sub["points"].as_f64(),
        "accepted": ui::accepted(sub),
        "grader_comment": text(&sub["grader_comment"]),
        "max_runtime_ms": sub["max_runtime"].as_f64(),
        "peak_memory_kib": sub["peak_memory"].as_f64(),
        "language": text(&sub["language"]),
        "source_filename": text(&sub["source_filename"]),
        "submitted_at": text(&sub["submitted_at"]),
        "compiler_message": text(&sub["compiler_message"]),
        "evaluations": evaluations(sub),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_string_is_no_value_at_all() {
        assert_eq!(text(&json!("done")), json!("done"));
        assert_eq!(text(&json!("  padded  ")), json!("padded"));
        assert_eq!(text(&json!("")), Value::Null);
        assert_eq!(text(&json!("   ")), Value::Null);
        assert_eq!(text(&json!(7)), Value::Null);
        assert_eq!(text(&Value::Null), Value::Null);
    }

    // a listing sends a fraction of what one submission does; the shape must not move
    #[test]
    fn every_field_is_there_even_when_the_grader_sends_none_of_them() {
        let sparse = submission(&json!({ "id": 12 }));
        let full = submission(&json!({
            "id": 12,
            "number": 3,
            "problem_name": "01_Expr_11",
            "status": "done",
            "points": 100.0,
            "grader_comment": "PPP",
            "max_runtime": 12.0,
            "peak_memory": 2048.0,
            "language": "cpp",
            "source_filename": "01_Expr_11.cpp",
            "submitted_at": "2026-08-01T09:12:33Z",
            "evaluations": [{"result": "correct", "score": 10.0, "time": 12.0, "memory": 2048.0}],
        }));
        let keys =
            |value: &Value| -> Vec<String> { value.as_object().unwrap().keys().cloned().collect() };
        assert_eq!(keys(&sparse), keys(&full));
        assert_eq!(sparse["status"], Value::Null);
        assert_eq!(sparse["evaluations"], json!([]));
        assert_eq!(sparse["accepted"], json!(false));
    }

    #[test]
    fn renames_the_grader_fields_and_keeps_its_units() {
        let sub = submission(&json!({
            "problem_name": "01_Expr_11",
            "max_runtime": 12.5,
            "peak_memory": 2048.0,
            "evaluations": [{"result": "correct", "time": 1.5, "memory": 64.0}],
        }));
        assert_eq!(sub["problem"], json!("01_Expr_11"));
        assert_eq!(sub["max_runtime_ms"], json!(12.5));
        assert_eq!(sub["peak_memory_kib"], json!(2048.0));
        assert_eq!(sub["evaluations"][0]["time_ms"], json!(1.5));
        assert_eq!(sub["evaluations"][0]["memory_kib"], json!(64.0));
    }

    #[test]
    fn accepted_only_when_the_grader_is_done_and_every_test_passed() {
        let done = json!({
            "status": "done",
            "evaluations": [{"result": "correct"}, {"result": "correct"}],
        });
        assert_eq!(submission(&done)["accepted"], json!(true));
        let grading = json!({
            "status": "received",
            "evaluations": [{"result": "correct"}],
        });
        assert_eq!(submission(&grading)["accepted"], json!(false));
        let failed = json!({
            "status": "done",
            "evaluations": [{"result": "correct"}, {"result": "wrong"}],
        });
        assert_eq!(submission(&failed)["accepted"], json!(false));
    }
}
