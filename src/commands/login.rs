use std::io::{self, BufRead, IsTerminal, Write};

use serde_json::json;

use crate::client::{api, clear_problems_cache, load_state, save_state, State};
use crate::ui::{bold, dim, fail};

fn prompt(label: &str) -> String {
    print!("{label}");
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .unwrap_or_else(|error| fail(&error.to_string()));
    line.trim_end_matches(['\r', '\n']).to_string()
}

pub fn run(url: Option<String>, user: Option<String>) {
    let old = load_state();
    let url = url
        .or_else(|| old.url.clone())
        .unwrap_or_else(|| prompt("grader url: "));
    let user = user
        .or_else(|| old.login.clone())
        .unwrap_or_else(|| prompt("login: "));
    let password = if io::stdin().is_terminal() {
        rpassword::prompt_password("password: ").unwrap_or_else(|error| fail(&error.to_string()))
    } else {
        eprint!("password: ");
        let mut line = String::new();
        io::stdin()
            .lock()
            .read_line(&mut line)
            .unwrap_or_else(|error| fail(&error.to_string()));
        line.trim_end_matches(['\r', '\n']).to_string()
    };

    let url = url.trim_end_matches('/').to_string();
    let moved = old.url.as_deref() != Some(&url) || old.login.as_deref() != Some(&user);
    let mut state = State {
        url: Some(url),
        login: Some(user.clone()),
        token: None,
    };
    let resp = api(
        &state,
        minreq::Method::Post,
        "auth/login",
        Some(&json!({ "login": user, "password": password })),
    );
    state.token = resp["token"].as_str().map(String::from);
    save_state(&state);
    if moved {
        clear_problems_cache();
    }
    let expires = resp["expires_at"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| resp["expires_at"].to_string());
    println!(
        "logged in as {} {}",
        bold(resp["user"]["full_name"].as_str().unwrap_or("")),
        dim(format!("(until {expires})"))
    );
}
