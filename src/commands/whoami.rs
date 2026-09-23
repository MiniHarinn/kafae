use serde_json::{json, Value};

use crate::client::{api, authed_session, Session};
use crate::config::{self, Source};
use crate::json;
use crate::offline;
use crate::ui::{bold, dim};

fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.trim().to_string()).filter(|t| !t.is_empty()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

// what picked this grader, named the way a student can go and change it: "config" says
// nothing about which line, and a stale export is what the answer usually is
fn origin(source: &Source) -> String {
    match source {
        Source::Flag => "--grader".to_string(),
        Source::Global => "current in the config file".to_string(),
        Source::Default => "the only grader configured".to_string(),
        Source::Session => "the stored session".to_string(),
        named => named.label(),
    }
}

// the grader the next submit would go to, and why it is that one. Without a table naming
// it there is nothing to call it but its url, which is still the honest answer.
fn grader_of(config: &config::Config, session: &Session) -> (String, String) {
    match config.selected() {
        Some(selected) => (selected.value, origin(&selected.source)),
        None => (
            session.url.clone(),
            config
                .url(None)
                .map_or_else(|| origin(&Source::Session), |url| origin(&url.source)),
        ),
    }
}

pub fn run() {
    if offline::on() {
        offline::refuse("whoami");
    }
    let session = authed_session();
    let (grader, from) = grader_of(config::load(), &session);
    let me = api(&session, minreq::Method::Get, "me", None);

    if json::on() {
        // the grader sends section as a number on some courses and a string on others
        let field = |key: &str| text(&me[key]).map(Value::from).unwrap_or(Value::Null);
        json::emit(&json!({
            "login": field("login"),
            "full_name": field("full_name"),
            "email": field("email"),
            "section": field("section"),
            "admin": me["admin"].as_bool().unwrap_or(false),
            "url": session.url,
            "grader": grader,
            "grader_from": from,
        }));
        return;
    }

    let login = me["login"].as_str().unwrap_or("");
    let name = text(&me["full_name"]).unwrap_or_else(|| login.to_string());
    println!("{}  {}", bold(name), dim(format!("({login})")));
    // the line this command exists for: which course is about to be submitted to
    println!(
        "{} {} {}",
        dim("grader:"),
        bold(&grader),
        dim(format!("(from {from})"))
    );

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
    // a named grader has not said where it points yet; an unnamed one already is its url
    if session.url != grader {
        notes.push(session.url.clone());
    }
    if !notes.is_empty() {
        println!("{}", dim(notes.join(" · ")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Env};

    fn config(text: &str, env: &[(&str, &str)]) -> Config {
        let (file, _) = config::parse(text);
        Config::new(file, Env::of(env), None)
    }

    const TWO: &str = r#"
current = "ce"

[grader.ce]
url = "https://ce.example"
login = "6xxxxxxx21"

[grader.algo]
url = "https://algo.example"
login = "6xxxxxxx21"
"#;

    // the whole point of the line: a student who exported KAFAE_GRADER in another
    // terminal last week must be able to see that that is what is deciding
    #[test]
    fn says_which_grader_is_active_and_what_chose_it() {
        let session = Session::new("https://ce.example", "6xxxxxxx21");
        let (grader, from) = grader_of(&config(TWO, &[]), &session);
        assert_eq!(
            (grader.as_str(), from.as_str()),
            ("ce", "current in the config file")
        );

        let env = config(TWO, &[("KAFAE_GRADER", "algo")]);
        let (grader, from) = grader_of(&env, &session);
        assert_eq!((grader.as_str(), from.as_str()), ("algo", "KAFAE_GRADER"));
    }

    // a user who only ever exported KAFAE_URL has no table to name, and "config" would be
    // a lie about where that url came from
    #[test]
    fn an_unnamed_grader_is_its_own_url() {
        let session = Session::new("https://ce.example", "6xxxxxxx21");
        let env = config("", &[("KAFAE_URL", "https://ce.example")]);
        let (grader, from) = grader_of(&env, &session);
        assert_eq!(
            (grader.as_str(), from.as_str()),
            ("https://ce.example", "KAFAE_URL")
        );

        let (grader, from) = grader_of(&config("", &[]), &session);
        assert_eq!(
            (grader.as_str(), from.as_str()),
            ("https://ce.example", "the stored session")
        );
    }
}
