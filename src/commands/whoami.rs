use serde_json::Value;

use crate::client::{api, authed_state};
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
