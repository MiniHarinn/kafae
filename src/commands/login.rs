use std::io::{self, BufRead, IsTerminal, Write};

use serde_json::{json, Value};

use crate::client::{api, current_account, remember_token, save_session, Session};
use crate::config::{self, Source};
use crate::offline;
use crate::ui::{bold, dim, ebold, edim, err_tag, fail, time_left};

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

// one account's session, written under that account's own name. No cache is touched: an
// account-keyed tree is nobody else's to read, and the v1 tree is already scoped to the
// account that wrote it, so leftovers are only disk and kafae clean ages them out.
fn authenticate(url: String, user: String, password: &str) -> (Session, Value) {
    let mut session = Session::new(&url, &user);
    remember_token(None);
    let resp = api(
        &session,
        minreq::Method::Post,
        "auth/login",
        Some(&json!({ "login": user, "password": password })),
    );
    session.token = resp["token"].as_str().map(String::from);
    // what the grader says the token is good for, so kafae graders can say it without
    // asking; absent means unknown and a 401 drives the renew below
    session.expires = resp["expires_at"]
        .as_str()
        .map(str::trim)
        .filter(|expires| !expires.is_empty())
        .map(String::from);
    save_session(&session);
    remember_token(session.token.as_deref());
    (session, resp)
}

pub fn renew(session: &Session) -> Option<Session> {
    if session.url.is_empty() || session.login.is_empty() {
        return None;
    }
    if crate::json::on() || !io::stdin().is_terminal() {
        return None;
    }
    let (url, user) = (session.url.clone(), session.login.clone());
    let gone = if session.token.is_some() {
        "token expired for"
    } else {
        "no token for"
    };
    eprintln!("{}", edim(format!("{gone} {user} at {url}")));
    let password = ask_password();
    // a renew rides in on some other command, and a config file is never that command's
    // side effect: only run() below writes one
    Some(authenticate(url, user, &password).0)
}

// the host is what a second grader is second in: a scheme or a trailing slash changes
// neither the account nor the cache it is filed under
fn host_of(url: &str) -> &str {
    url.split_once("://")
        .map_or(url, |(_, rest)| rest)
        .trim_end_matches('/')
        .split('/')
        .next()
        .unwrap_or_default()
}

// --url pointed at another grader is a second table, not a new url under the first one.
// Repointing the grader another terminal is mid-course on is not something to guess at.
fn guard(config: &config::Config, url: Option<&str>) {
    let (Some(url), Some(selected)) = (url, config.selected()) else {
        return;
    };
    if selected.source == Source::Flag {
        return;
    }
    let Some(configured) = config
        .graders()
        .get(&selected.value)
        .and_then(|grader| grader.url.as_deref())
    else {
        return;
    };
    if host_of(configured) == host_of(url) {
        return;
    }
    fail(&format!(
        "grader {} is {configured}, pass {} to add a second grader",
        ebold(&selected.value),
        ebold("--grader <name>")
    ));
}

// the name of the table this login belongs to. A user who has never had a config gets
// "default", which is the name the file's own comments tell them how to change.
fn grader_name(config: &config::Config) -> String {
    config
        .selected()
        .map_or_else(|| "default".to_string(), |selected| selected.value)
}

// KAFAE_URL and KAFAE_USER are the ad-hoc layer: stamping one onto the table would make a
// single shell's export permanent, and unexporting it would no longer bring the other
// grader back. A table with no value of its own is the exception — it has nothing to keep,
// and a first login has to fill it in from somewhere.
fn durable<'a>(
    source: Option<&Source>,
    value: &'a str,
    configured: Option<&str>,
) -> Option<&'a str> {
    let held = configured.is_some_and(|value| !value.trim().is_empty());
    (!(matches!(source, Some(Source::Env(_))) && held)).then_some(value)
}

// a read-only or home-manager-managed config directory is a real machine: it must not be
// able to fail a login that already succeeded
fn remember(
    config: &config::Config,
    session: &Session,
    url_from: Option<&Source>,
    login_from: Option<&Source>,
) {
    let fresh = !config.exists();
    let name = grader_name(config);
    // the template's current names "default", which nothing is configured under: the
    // first grader in the file is the one current is allowed to point at
    let first = config.graders().is_empty();
    let table = config.graders().get(&name);
    let url = durable(url_from, &session.url, table.and_then(|t| t.url.as_deref()));
    let login = durable(
        login_from,
        &session.login,
        table.and_then(|t| t.login.as_deref()),
    );
    let wrote =
        config::ensure_file().and_then(|_| config::remember_grader(&name, url, login, first));
    match wrote {
        Ok(path) if fresh => eprintln!(
            "{}",
            edim(format!(
                "wrote {} — your grader is named {name}, rename it with kafae config edit",
                path.display()
            ))
        ),
        Ok(_) => {}
        Err(reason) => eprintln!("{} cannot write the config file: {reason}", err_tag()),
    }
}

pub fn run(url: Option<String>, user: Option<String>) {
    if offline::on() {
        offline::refuse("login");
    }
    let config = config::load();
    guard(config, url.as_deref());
    // --url > KAFAE_URL > the selected grader's table > the account already stored here.
    // The stored account only answers for a config that names no grader at all: with
    // tables around, `login --grader algo` asking for algo's url is the point.
    let known = config.graders().is_empty().then(current_account).flatten();
    // the provenance is kept, not flattened away: what the table gets back depends on
    // whether the value came from a flag, the file or one shell's export
    let picked_url = config.url(url.as_deref());
    let picked_user = config.login(user.as_deref());
    let url = picked_url
        .as_ref()
        .map(|url| url.value.clone())
        .or_else(|| known.map(|account| account.url.clone()))
        .unwrap_or_else(|| prompt("grader url: "));
    let user = picked_user
        .as_ref()
        .map(|login| login.value.clone())
        .or_else(|| known.map(|account| account.login.clone()))
        .unwrap_or_else(|| prompt("login: "));
    let password = ask_password();

    let (session, resp) = authenticate(url, user, &password);
    // --grader names the table this went into; current is left where it is, so logging
    // into one course does not move another terminal onto it
    remember(
        config,
        &session,
        picked_url.as_ref().map(|url| &url.source),
        picked_user.as_ref().map(|login| &login.source),
    );

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exported_url_holds_for_one_command_and_never_reaches_the_table() {
        let env = Some(Source::Env("KAFAE_URL"));
        // the table already says something, and one shell's export does not get to
        // outlive that shell by overwriting it
        assert_eq!(durable(env.as_ref(), "https://b", Some("https://a")), None);
        // nothing to preserve: a first login has to fill the table in from somewhere
        assert_eq!(durable(env.as_ref(), "https://b", None), Some("https://b"));
        assert_eq!(
            durable(env.as_ref(), "https://b", Some("  ")),
            Some("https://b")
        );
        // a flag, the file itself and an answered prompt are all the student saying so
        for source in [Some(Source::Flag), Some(Source::Grader("ce".into())), None] {
            assert_eq!(
                durable(source.as_ref(), "https://b", Some("https://a")),
                Some("https://b")
            );
        }
    }
}
