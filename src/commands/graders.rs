use std::io::IsTerminal;

use console::style;
use serde_json::{json, Value};

use crate::client::{self, Account, Session};
use crate::config;
use crate::json;
use crate::ui::{bold, dim, ebold, fail, table, time_left};

// one line of `kafae graders`: a configured table, or a session no table names
struct Row {
    name: Option<String>,
    url: Option<String>,
    login: Option<String>,
    current: bool,
    session: Option<Session>,
}

impl Row {
    fn account(&self) -> Option<Account> {
        Some(Account::new(self.url.as_deref()?, self.login.as_deref()?))
    }
}

// the selected grader is shown as it will actually be used, so a KAFAE_URL export shows
// up here and not only in the dim line announce() prints once. The account chain has one
// rung the config does not — the single stored session — and a table naming only a url,
// which is all the file requires, gets its login from exactly there.
fn effective(
    config: &config::Config,
    name: &str,
    table: &config::Grader,
) -> (Option<String>, Option<String>) {
    if config.selected().map(|grader| grader.value).as_deref() != Some(name) {
        return (table.url.clone(), table.login.clone());
    }
    let account = client::current_account();
    (
        config
            .url(None)
            .map(|url| url.value)
            .or_else(|| account.map(|account| account.url.clone())),
        config
            .login(None)
            .map(|login| login.value)
            .or_else(|| account.map(|account| account.login.clone())),
    )
}

fn rows() -> Vec<Row> {
    let config = config::load();
    let current = client::current_account();
    let mut rows: Vec<Row> = config
        .graders()
        .iter()
        .map(|(name, grader)| {
            let (url, login) = effective(config, name, grader);
            let mut row = Row {
                name: Some(name.clone()),
                url,
                login,
                current: false,
                session: None,
            };
            let account = row.account();
            row.current = current.is_some() && current == account.as_ref();
            row.session = account.as_ref().and_then(client::saved_session);
            row
        })
        .collect();

    // a session the config does not name is still an account you are logged into: the
    // user who only ever exported KAFAE_URL has nothing but these
    for session in client::sessions() {
        let account = session.account();
        if rows
            .iter()
            .any(|row| row.account().as_ref() == Some(&account))
        {
            continue;
        }
        rows.push(Row {
            name: None,
            url: Some(account.url.clone()),
            login: Some(account.login.clone()),
            current: current == Some(&account),
            session: Some(session),
        });
    }
    rows
}

// no network here: what is left of a session is whatever its stored expiry says, and a
// v1 record carries none — the token is tried anyway and a 401 drives the renew
fn session_cell(session: Option<&Session>) -> String {
    let Some(session) = session.filter(|session| session.token.is_some()) else {
        return dim("logged out").to_string();
    };
    let Some(stamp) = &session.expires else {
        return dim("logged in").to_string();
    };
    match time_left(stamp) {
        Some(left) => format!("{left} left"),
        None => style("expired").red().to_string(),
    }
}

pub fn run() {
    let rows = rows();

    if json::on() {
        let entries: Vec<Value> = rows
            .iter()
            .map(|row| {
                let expires = row.session.as_ref().and_then(|s| s.expires.clone());
                json!({
                    "name": row.name,
                    "url": row.url,
                    "login": row.login,
                    "current": row.current,
                    "logged_in": row.session.as_ref().is_some_and(|s| s.token.is_some()),
                    "expires": expires,
                    "expired": expires.as_deref().is_some_and(|stamp| time_left(stamp).is_none()),
                })
            })
            .collect();
        json::emit(&json!({ "graders": entries }));
        return;
    }

    if rows.is_empty() {
        println!(
            "{}",
            dim(format!(
                "no graders yet, run {}",
                ebold("kafae login --url <url>")
            ))
        );
        return;
    }

    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            // a session with no table of its own has no name to print, only an account
            let name = match &row.name {
                Some(name) => bold(name).to_string(),
                None => dim("(unnamed)").to_string(),
            };
            let shown =
                |value: &Option<String>| value.clone().unwrap_or_else(|| dim("-").to_string());
            vec![
                if row.current {
                    "*".to_string()
                } else {
                    String::new()
                },
                name,
                shown(&row.url),
                shown(&row.login),
                session_cell(row.session.as_ref()),
            ]
        })
        .collect();

    table(
        &[
            ("", false),
            ("name", false),
            ("url", false),
            ("login", false),
            ("session", false),
        ],
        &cells,
    );
}

// ---------------------------------------------------------------------------
// kafae use
// ---------------------------------------------------------------------------

pub fn switch(name: Option<String>) {
    let config = config::load();
    let names = config.grader_names();
    // asked before the write, because the write is what makes the file exist
    let fresh = !config.exists();
    match name {
        Some(name) => set(&names, &name, fresh),
        None => pick(config, &names, fresh),
    }
}

fn set(names: &[String], name: &str, fresh: bool) {
    // a name no table carries is a typo far more often than a table about to be written
    if !names.is_empty() && !names.iter().any(|known| known == name) {
        fail(&format!(
            "no grader named {}, configured: {}",
            ebold(name),
            names.join(", ")
        ));
    }
    let path = config::set_current(name).unwrap_or_else(|error| fail(&error));
    println!("now using {}", bold(name));
    if fresh {
        println!("{}", dim(format!("wrote {}", path.display())));
    }
}

// with exactly two configured, the one you are not on is the only one you can have meant;
// any other shape is a question, and a question needs a terminal to ask it in
fn other_than<'a>(names: &'a [String], current: Option<&str>) -> Option<&'a String> {
    let current = current?;
    let [first, second] = names else {
        return None;
    };
    match (first == current, second == current) {
        (true, false) => Some(second),
        (false, true) => Some(first),
        _ => None,
    }
}

fn pick(config: &config::Config, names: &[String], fresh: bool) {
    if names.is_empty() {
        fail(&format!(
            "no graders configured, run {}",
            ebold("kafae login --url <url>")
        ));
    }
    let current = config.selected().map(|grader| grader.value);

    if let Some(other) = other_than(names, current.as_deref()) {
        set(names, other, fresh);
        return;
    }

    // nothing is reading a list in a pipe, so being ambiguous there has to be an error
    if !std::io::stdout().is_terminal() {
        fail(&format!(
            "name the grader to use, one of: {}",
            names.join(", ")
        ));
    }
    for name in names {
        let mine = Some(name.as_str()) == current.as_deref();
        let line = format!("{} {name}", if mine { "*" } else { " " });
        println!("{}", if mine { bold(line).to_string() } else { line });
    }
    println!("{}", dim("kafae use <name> to switch"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn with_two_graders_the_one_you_are_not_on_needs_no_naming() {
        let two = names(&["algo", "ce"]);
        assert_eq!(
            other_than(&two, Some("ce")).map(String::as_str),
            Some("algo")
        );
        assert_eq!(
            other_than(&two, Some("algo")).map(String::as_str),
            Some("ce")
        );
        // a current naming neither of them, and a third grader, are both questions
        assert_eq!(other_than(&two, Some("db")), None);
        assert_eq!(other_than(&two, None), None);
        assert_eq!(other_than(&names(&["algo", "ce", "db"]), Some("ce")), None);
        assert_eq!(other_than(&names(&["ce"]), Some("ce")), None);
    }

    #[test]
    fn what_is_left_of_a_session_is_read_off_the_record_and_never_asked_for() {
        // dim()/style() colour a real terminal, and whether the harness running this
        // test counts as one is not this test's business either way
        let mut session = Session::new("https://grader.example", "6xxxxxxx21");
        assert!(session_cell(None).contains("logged out"));
        assert!(session_cell(Some(&session)).contains("logged out"));
        session.token = Some("eyJhbGciOi".to_string());
        // a v1 record carries no expiry, and unknown is not the same as expired
        assert!(session_cell(Some(&session)).contains("logged in"));
        session.expires = Some("2000-01-01T00:00:00Z".to_string());
        assert!(session_cell(Some(&session)).contains("expired"));
        session.expires = Some("2999-01-01T00:00:00Z".to_string());
        assert!(session_cell(Some(&session)).ends_with(" left"));
    }
}
