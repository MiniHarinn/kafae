use serde_json::{json, Value};

use crate::client::{api, authed_state};
use crate::json;
use crate::ui::{bold, dim};

fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.trim().to_string()).filter(|t| !t.is_empty()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

pub fn run() {
    let state = authed_state();
    let me = api(&state, minreq::Method::Get, "me", None);

    if json::on() {
        // the grader sends section as a number on some courses and a string on others
        let field = |key: &str| text(&me[key]).map(Value::from).unwrap_or(Value::Null);
        json::emit(&json!({
            "login": field("login"),
            "full_name": field("full_name"),
            "email": field("email"),
            "section": field("section"),
            "admin": me["admin"].as_bool().unwrap_or(false),
            "url": state.url,
        }));
        return;
    }

    let login = me["login"].as_str().unwrap_or("");
    let name = text(&me["full_name"]).unwrap_or_else(|| login.to_string());
    println!("{}  {}", bold(name), dim(format!("({login})")));

    let mut notes = Vec::new();
    if let Some(section) = text(&me["section"]) {
        notes.push(format!("section {section}"));
    }
    if let Some(email) = text(&me["email"]) {
        notes.push(email);
    }
    if me["admin"].as_bool() == Some(true) {
        notes.push("admin".to_string());
    }
    if let Some(url) = &state.url {
        notes.push(url.clone());
    }
    if !notes.is_empty() {
        println!("{}", dim(notes.join(" · ")));
    }
}
