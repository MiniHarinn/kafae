use std::io::{self, BufRead, IsTerminal, Write};

use serde_json::{json, Value};

use crate::client::{
    api, clear_problems_cache, load_state, remember_token, save_state, saved_state, State,
};
use crate::offline;
use crate::ui::{bold, dim, edim, fail, time_left};

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

fn ask_password() -> String {
    if io::stdin().is_terminal() {
        rpassword::prompt_password("password: ").unwrap_or_else(|error| fail(&error.to_string()))
    } else {
        eprint!("password: ");
        let mut line = String::new();
        io::stdin()
            .lock()
            .read_line(&mut line)
            .unwrap_or_else(|error| fail(&error.to_string()));
        line.trim_end_matches(['\r', '\n']).to_string()
    }
}

fn authenticate(url: String, user: String, password: &str) -> (State, Value) {
    let old = saved_state();
    let moved = old.url.as_deref() != Some(url.as_str()) || old.login.as_deref() != Some(&user);
    let mut state = State {
        url: Some(url),
        login: Some(user.clone()),
        token: None,
    };
    remember_token(None);
    let resp = api(
        &state,
        minreq::Method::Post,
        "auth/login",
        Some(&json!({ "login": user, "password": password })),
    );
    state.token = resp["token"].as_str().map(String::from);
    save_state(&state);
    remember_token(state.token.as_deref());
    if moved {
        clear_problems_cache();
    }
    (state, resp)
}

pub fn renew(state: &State) -> Option<State> {
    let (url, user) = (state.url.clone()?, state.login.clone()?);
    if crate::json::on() || !io::stdin().is_terminal() {
        return None;
    }
    let gone = if state.token.is_some() {
        "token expired for"
    } else {
        "no token for"
    };
    eprintln!("{}", edim(format!("{gone} {user} at {url}")));
    let password = ask_password();
    Some(authenticate(url, user, &password).0)
}

pub fn run(url: Option<String>, user: Option<String>) {
    if offline::on() {
        offline::refuse("login");
    }
    let old = load_state();
    let url = url
        .or_else(|| old.url.clone())
        .unwrap_or_else(|| prompt("grader url: "));
    let user = user
        .or_else(|| old.login.clone())
        .unwrap_or_else(|| prompt("login: "));
    let password = ask_password();

    let url = url.trim_end_matches('/').to_string();
    let (_, resp) = authenticate(url, user, &password);
    let expires = resp["expires_at"].as_str().unwrap_or_default();
    let note = match time_left(expires) {
        Some(left) => format!("({left} left)"),
        None => format!("(until {expires})"),
    };
    println!(
        "logged in as {} {}",
        bold(resp["user"]["full_name"].as_str().unwrap_or("")),
        dim(note)
    );
}
