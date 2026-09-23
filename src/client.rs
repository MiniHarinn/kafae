use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::json;
use crate::offline;
use crate::ui::{ebold, edim, err_tag, fail, fail_as};

#[derive(Default, Serialize, Deserialize)]
pub struct State {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

pub fn state_file() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_dir)
        .unwrap()
        .join("kafae")
        .join("state.json")
}

fn cache_root() -> PathBuf {
    dirs::cache_dir().unwrap().join("kafae")
}

// two graders must not share a cache: problem names collide and a score is per account
fn host_slug(url: Option<&str>) -> String {
    let Some(url) = url else {
        return "nohost".to_string();
    };
    let host = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .trim_matches('/');
    let slug: String = host
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // a url of nothing but dots would name the parent directory, not a grader
    if one_segment(&slug) {
        slug
    } else {
        "nohost".to_string()
    }
}

pub fn cache_dir() -> PathBuf {
    cache_root().join(host_slug(load_state().url.as_deref()))
}

pub fn problems_cache() -> PathBuf {
    cache_dir().join("problems.json")
}

pub fn statements_dir() -> PathBuf {
    cache_dir().join("statements")
}

// so new --last-view in the editor window can pick up what view showed in the other
fn last_view_file() -> PathBuf {
    cache_dir().join("last_view")
}

pub fn remember_view(name: &str) {
    let path = last_view_file();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, name);
}

pub fn last_viewed() -> Option<String> {
    fs::read_to_string(last_view_file())
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

// this becomes a path and then remove_dir_all, so it must not climb out
fn one_segment(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none()
}

fn cache_name(name: &str) -> &str {
    if !one_segment(name) {
        fail(&format!("{} is not a problem name", ebold(name)));
    }
    name
}

pub fn tests_dir(name: &str) -> PathBuf {
    cache_dir().join("tests").join(cache_name(name))
}

pub fn statement_file(name: &str, extension: &str) -> PathBuf {
    statements_dir().join(format!("{}.{extension}", cache_name(name)))
}

pub fn attachments_dir() -> PathBuf {
    cache_dir().join("attachments")
}

// the grader names the attachment, so its extension is the only thing saying what the
// file is; it becomes a path here, so only a plain ascii word gets through
fn sane_ext(ext: &str) -> Option<String> {
    let ext = ext.trim().to_lowercase();
    let plain =
        !ext.is_empty() && ext.len() <= 16 && ext.chars().all(|c| c.is_ascii_alphanumeric());
    plain.then_some(ext)
}

// content-disposition: attachment; filename="Exam1_Template.dig"; filename*=UTF-8''Exam1...
fn disposition_name(disposition: &str) -> Option<String> {
    let mut plain = None;
    for part in disposition.split(';').map(str::trim) {
        if let Some(value) = part.strip_prefix("filename*=") {
            // RFC 5987: charset'language'name, and the name is the half worth keeping
            let name = value.rsplit_once('\'').map_or(value, |(_, name)| name);
            return Some(name.trim_matches('"').to_string());
        }
        if let Some(value) = part.strip_prefix("filename=") {
            plain = Some(value.trim_matches('"').to_string());
        }
    }
    plain
}

// what the grader called it, else the one language it accepts, else no claim at all
pub fn attachment_ext(disposition: Option<&str>, prob: &Value) -> String {
    disposition
        .and_then(disposition_name)
        .and_then(|name| sane_ext(Path::new(&name).extension()?.to_str()?))
        .or_else(|| match permitted_exts(prob).as_slice() {
            [only] => sane_ext(only),
            _ => None,
        })
        .unwrap_or_else(|| "bin".to_string())
}

pub fn attachment_file(name: &str, extension: &str) -> PathBuf {
    attachments_dir().join(format!("{}.{extension}", cache_name(name)))
}

// the extension is the grader's, so the cached file is found by its stem
pub fn cached_attachment(name: &str) -> Option<PathBuf> {
    let name = cache_name(name);
    fs::read_dir(attachments_dir())
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.file_stem().and_then(|stem| stem.to_str()) == Some(name))
}

pub fn clear_attachment(name: &str) -> u64 {
    cached_attachment(name).map_or(0, |path| discard(&path))
}

// the languages the grader will take for this problem; none listed means it has no opinion
pub fn permitted_exts(prob: &Value) -> Vec<String> {
    prob["permitted_languages"]
        .as_array()
        .map(|langs| {
            langs
                .iter()
                .filter_map(|lang| lang["ext"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

// the problem as the grader describes it, which is what offline resolves against
pub fn detail_file(name: &str) -> PathBuf {
    cache_dir()
        .join("problems")
        .join(format!("{}.json", cache_name(name)))
}

pub fn cached_detail(name: &str) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(detail_file(name)).ok()?).ok()
}

// caching a command was asked for: a half-written cache is a lie, so say so
pub fn cache_write(path: &Path, bytes: impl AsRef<[u8]>) {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).unwrap_or_else(|error| fail(&error.to_string()));
    }
    fs::write(path, bytes).unwrap_or_else(|error| fail(&error.to_string()));
}

// caching picked up on the way past, so it must never fail the command it rode in on
fn try_cache(path: &Path, bytes: impl AsRef<[u8]>) {
    if let Some(dir) = path.parent() {
        if fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let _ = fs::write(path, bytes);
}

pub fn tree_size(path: &Path) -> u64 {
    fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| {
                    let child = entry.path();
                    if child.is_dir() {
                        tree_size(&child)
                    } else {
                        child.metadata().map(|meta| meta.len()).unwrap_or(0)
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

fn discard(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    if path.is_dir() {
        let size = tree_size(path);
        let _ = fs::remove_dir_all(path);
        size
    } else {
        let size = path.metadata().map(|meta| meta.len()).unwrap_or(0);
        let _ = fs::remove_file(path);
        size
    }
}

// every grader's cache, not just the one the state names
pub fn clear_cache() -> u64 {
    discard(&cache_root())
}

pub fn clear_problems_cache() -> u64 {
    discard(&problems_cache())
}

pub fn clear_problem_cache(name: &str) -> u64 {
    clear_attachment(name)
        + discard(&tests_dir(name))
        + discard(&statement_file(name, "pdf"))
        + discard(&statement_file(name, "json"))
        + discard(&detail_file(name))
}

pub fn clear_state() -> u64 {
    discard(&state_file())
}

pub fn saved_state() -> State {
    fs::read_to_string(state_file())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn from_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn overlay(mut state: State, url: Option<String>, login: Option<String>) -> State {
    let url = url.map(|url| url.trim_end_matches('/').to_string());
    if url
        .as_ref()
        .is_some_and(|url| state.url.as_ref() != Some(url))
        || login
            .as_ref()
            .is_some_and(|login| state.login.as_ref() != Some(login))
    {
        state.token = None;
    }
    state.url = url.or(state.url);
    state.login = login.or(state.login);
    state
}

pub fn load_state() -> State {
    overlay(saved_state(), from_env("KAFAE_URL"), from_env("KAFAE_USER"))
}

static RENEWED: Mutex<Option<String>> = Mutex::new(None);

pub fn remember_token(token: Option<&str>) {
    *RENEWED.lock().unwrap() = token.map(String::from);
}

fn token_of(state: &State) -> Option<String> {
    RENEWED
        .lock()
        .unwrap()
        .clone()
        .or_else(|| state.token.clone())
}

pub fn save_state(state: &State) {
    let path = state_file();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, serde_json::to_string(state).unwrap())
        .unwrap_or_else(|error| fail(&error.to_string()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    // parity with the 0600 above: drop inherited ACEs, keep only the owner
    #[cfg(windows)]
    if let Ok(user) = std::env::var("USERNAME") {
        use std::process::{Command, Stdio};
        let _ = Command::new("icacls")
            .arg(&path)
            .args(["/inheritance:r", "/grant:r", &format!("{user}:F")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

pub fn authed_state() -> State {
    let state = load_state();
    if state.token.is_some() {
        return state;
    }
    crate::commands::login::renew(&state).unwrap_or_else(|| {
        fail_as(
            "auth",
            &format!("not logged in, run {}", ebold("kafae login")),
            None,
        )
    })
}

// a cache is readable with a dead token, so offline must not go asking for a password
pub fn state_for_reads() -> State {
    if offline::on() {
        load_state()
    } else {
        authed_state()
    }
}

fn send(
    state: &State,
    method: minreq::Method,
    route: &str,
    body: Option<&Value>,
) -> minreq::Response {
    // every route to the network comes through here, so one refusal covers them all
    if offline::on() {
        offline::refuse(&format!("/{route}"));
    }
    let base = state.url.clone().unwrap_or_default();
    let mut req = minreq::Request::new(method, format!("{base}/api/v1/{route}")).with_timeout(30);
    if let Some(token) = token_of(state) {
        req = req.with_header("Authorization", format!("Bearer {token}"));
    }
    if let Some(body) = body {
        req = req
            .with_json(body)
            .unwrap_or_else(|error| fail(&error.to_string()));
    }
    req.send()
        .unwrap_or_else(|error| fail_as("network", &format!("cannot reach {base}: {error}"), None))
}

fn request(
    state: &State,
    method: minreq::Method,
    route: &str,
    body: Option<&Value>,
) -> minreq::Response {
    let resp = send(state, method.clone(), route, body);
    if resp.status_code != 401 || state.token.is_none() {
        return resp;
    }
    match crate::commands::login::renew(state) {
        Some(fresh) => send(&fresh, method, route, body),
        None => resp,
    }
}

fn fail_from(state: &State, resp: &minreq::Response) -> ! {
    let mut message = resp
        .json::<Value>()
        .ok()
        .and_then(|body| body.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| resp.reason_phrase.clone());
    let expired = resp.status_code == 401 && state.token.is_some();
    if expired {
        message.push_str(&format!(", run {}", ebold("kafae login")));
    }
    fail_as(
        if expired { "auth" } else { "http" },
        &message,
        Some(json!({ "status": resp.status_code })),
    );
}

pub fn api(state: &State, method: minreq::Method, route: &str, body: Option<&Value>) -> Value {
    let resp = request(state, method, route, body);
    if !(200..300).contains(&resp.status_code) {
        fail_from(state, &resp);
    }
    resp.json().unwrap_or_else(|error| fail(&error.to_string()))
}

pub fn api_bytes(state: &State, route: &str) -> Option<Vec<u8>> {
    let resp = request(state, minreq::Method::Get, route, None);
    if resp.status_code == 404 {
        return None;
    }
    if !(200..300).contains(&resp.status_code) {
        fail_from(state, &resp);
    }
    Some(resp.as_bytes().to_vec())
}

// like api_bytes, but hands back the name the grader gave the file: for an attachment
// that name is the only thing saying what the bytes are
pub fn api_download(state: &State, route: &str) -> Option<(Vec<u8>, Option<String>)> {
    let resp = request(state, minreq::Method::Get, route, None);
    if resp.status_code == 404 {
        return None;
    }
    if !(200..300).contains(&resp.status_code) {
        fail_from(state, &resp);
    }
    let disposition = resp.headers.get("content-disposition").cloned();
    Some((resp.as_bytes().to_vec(), disposition))
}

// the file the grader ships with a problem, fetched on the way past like the pdf; offline
// the cache is all there is to offer
pub fn attachment_of(state: &State, prob: &Value) -> Option<PathBuf> {
    let name = prob["name"].as_str()?;
    if offline::on() {
        return cached_attachment(name);
    }
    if prob["has_attachment"].as_bool() != Some(true) {
        return None;
    }
    let (bytes, disposition) =
        api_download(state, &format!("problems/{}/files/attachment", prob["id"]))?;
    let path = attachment_file(name, &attachment_ext(disposition.as_deref(), prob));
    cache_write(&path, bytes);
    Some(path)
}

pub fn title_of(problem: &Value) -> String {
    problem["full_name"].as_str().unwrap_or("").to_string()
}

pub fn get_problems(state: &State) -> Vec<Value> {
    if offline::on() {
        return cached_listing().unwrap_or_else(|| no_listing());
    }
    let problems = api(state, minreq::Method::Get, "problems", None)
        .as_array()
        .cloned()
        .unwrap_or_default();
    // whole, so an offline listing can say everything an online one does
    try_cache(&problems_cache(), serde_json::to_string(&problems).unwrap());
    problems
}

fn no_listing() -> ! {
    fail(&format!(
        "no cached problem list, run {} online first",
        ebold("kafae sync")
    ))
}

pub fn get_submission(state: &State, id: i64) -> Value {
    api(
        state,
        minreq::Method::Get,
        &format!("submissions/{id}"),
        None,
    )
}

// the problem carries the newest submission's id, so listing them is wasted
pub fn latest_submission(state: &State, reference: &str) -> Value {
    let problem = resolve_problem(state, reference);
    let Some(id) = problem["last_submission_id"].as_i64() else {
        fail(&format!(
            "no submissions yet for {}",
            ebold(problem["name"].as_str().unwrap_or(reference))
        ));
    };
    get_submission(state, id)
}

// the grader hands the listing back newest first, so sort it to read like kafae problems
fn name_and_title(entries: &[Value]) -> Vec<(String, String)> {
    let mut problems: Vec<(String, String)> = entries
        .iter()
        .map(|p| (p["name"].as_str().unwrap_or("").to_string(), title_of(p)))
        .collect();
    problems.sort();
    problems
}

fn cached_listing() -> Option<Vec<Value>> {
    serde_json::from_str(&fs::read_to_string(problems_cache()).ok()?).ok()
}

pub fn cached_problems() -> Vec<(String, String)> {
    cached_listing()
        .map(|entries| name_and_title(&entries))
        .unwrap_or_default()
}

pub fn cached_tags() -> Vec<String> {
    let mut tags: Vec<String> = cached_listing()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|p| p["tags"].as_array())
                .flatten()
                .filter_map(|tag| tag.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    tags.sort();
    tags.dedup();
    tags
}

// name before id, and the same rule online and offline or the two drift apart
fn find_problem<'a>(problems: &'a [Value], reference: &str) -> Option<&'a Value> {
    problems
        .iter()
        .find(|p| p["name"].as_str() == Some(reference))
        .or_else(|| {
            let id = reference.parse::<i64>().ok()?;
            problems.iter().find(|p| p["id"].as_i64() == Some(id))
        })
}

fn find_name(entries: &[Value], reference: &str) -> Option<String> {
    Some(
        find_problem(entries, reference)?["name"]
            .as_str()?
            .to_string(),
    )
}

pub fn cached_problem_name(reference: &str) -> Option<String> {
    find_name(&cached_listing()?, reference)
}

pub fn resolve_problem(state: &State, reference: &str) -> Value {
    let problems = get_problems(state);
    if let Some(hit) = find_problem(&problems, reference) {
        let name = hit["name"].as_str().unwrap_or("").to_string();
        if offline::on() {
            return cached_detail(&name).unwrap_or_else(|| {
                fail(&format!(
                    "{} is not synced, run {} online first",
                    ebold(&name),
                    ebold(format!("kafae sync {name}"))
                ))
            });
        }
        let detail = api(
            state,
            minreq::Method::Get,
            &format!("problems/{}", hit["id"]),
            None,
        );
        try_cache(&detail_file(&name), serde_json::to_string(&detail).unwrap());
        return detail;
    }
    not_found(&problems, reference)
}

// online and offline miss the same way, so the message lives here once
fn not_found(problems: &[Value], reference: &str) -> ! {
    let needle = reference.to_lowercase();
    let suggestions: Vec<&Value> = problems
        .iter()
        .filter(|p| {
            p["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains(&needle)
                || title_of(p).to_lowercase().contains(&needle)
        })
        .collect();
    // the near misses are the useful half of this message, so a script gets them too
    if json::on() {
        let detail = json!({
            "suggestions": suggestions
                .iter()
                .map(|p| json!({
                    "id": p["id"].as_i64(),
                    "name": p["name"].as_str(),
                    "title": title_of(p),
                }))
                .collect::<Vec<Value>>(),
        });
        json::fail(
            "not_found",
            &format!("no problem named {reference}"),
            Some(detail),
        );
    }
    eprintln!(
        "{} no problem named {}{}",
        err_tag(),
        ebold(reference),
        if suggestions.is_empty() {
            format!(", see {}", ebold("kafae problems"))
        } else {
            ", did you mean:".to_string()
        }
    );
    for p in suggestions {
        eprintln!(
            "  {}  {}  {}",
            edim(format!("{:>5}", p["id"].to_string())),
            p["name"].as_str().unwrap_or(""),
            edim(title_of(p))
        );
    }
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn listing() -> Vec<Value> {
        vec![
            json!({"id": 7, "name": "01_Expr_11", "full_name": "Expressions"}),
            json!({"id": 42, "name": "02_Loop_3", "full_name": "Loops"}),
        ]
    }

    fn state(url: &str, login: &str) -> State {
        State {
            url: Some(url.to_string()),
            login: Some(login.to_string()),
            token: Some("t".to_string()),
        }
    }

    #[test]
    fn the_environment_names_the_grader_and_keeps_a_token_that_still_fits() {
        let same = overlay(
            state("https://g.example", "6xxx21"),
            Some("https://g.example/".to_string()),
            Some("6xxx21".to_string()),
        );
        assert_eq!(same.url.as_deref(), Some("https://g.example"));
        assert_eq!(same.token.as_deref(), Some("t"));

        let named = overlay(state("https://g.example", "6xxx21"), None, None);
        assert_eq!(named.url.as_deref(), Some("https://g.example"));
        assert_eq!(named.token.as_deref(), Some("t"));

        let wiped = overlay(
            State::default(),
            Some("https://g.example".to_string()),
            Some("6xxx21".to_string()),
        );
        assert_eq!(wiped.url.as_deref(), Some("https://g.example"));
        assert_eq!(wiped.login.as_deref(), Some("6xxx21"));
        assert_eq!(wiped.token, None);
    }

    #[test]
    fn a_token_does_not_follow_you_to_another_grader_or_another_account() {
        let moved = overlay(
            state("https://old.example", "6xxx21"),
            Some("https://new.example".to_string()),
            None,
        );
        assert_eq!(moved.url.as_deref(), Some("https://new.example"));
        assert_eq!(moved.login.as_deref(), Some("6xxx21"));
        assert_eq!(moved.token, None);

        let someone_else = overlay(
            state("https://g.example", "6xxx21"),
            None,
            Some("6xxx22".to_string()),
        );
        assert_eq!(someone_else.login.as_deref(), Some("6xxx22"));
        assert_eq!(someone_else.token, None);
    }

    #[test]
    fn a_grader_gets_its_own_cache_directory() {
        assert_eq!(host_slug(Some("https://g.example")), "g.example");
        assert_eq!(host_slug(Some("http://g.example/")), "g.example");
        assert_eq!(host_slug(Some("https://g.example:3000")), "g.example_3000");
        assert_eq!(
            host_slug(Some("https://g.example/cs101")),
            "g.example_cs101"
        );
        assert_eq!(host_slug(None), "nohost");
        assert_eq!(host_slug(Some("")), "nohost");
        assert_eq!(host_slug(Some("https://")), "nohost");
        // two graders must not land in the same directory
        assert_ne!(
            host_slug(Some("https://a.example")),
            host_slug(Some("https://b.example"))
        );
    }

    // the slug becomes a directory, so it must not climb out either
    #[test]
    fn a_hostile_url_cannot_escape_the_cache_root() {
        for climb in [
            "https://../../etc",
            "https://a/../b",
            "https://.",
            "https://..",
        ] {
            let slug = host_slug(Some(climb));
            assert!(one_segment(&slug), "{climb} -> {slug}");
        }
        assert_eq!(host_slug(Some("https://..")), "nohost");
    }

    #[test]
    fn a_problem_name_is_one_path_segment() {
        assert!(one_segment("01_Expr_11"));
        assert!(one_segment("a b"));
        for climb in ["..", ".", "../../..", "a/b", "", "/etc", "/"] {
            assert!(!one_segment(climb), "{climb} should be rejected");
        }
        #[cfg(windows)]
        for climb in [r"a\b", r"C:\Windows", r"\\server\share"] {
            assert!(!one_segment(climb), "{climb} should be rejected");
        }
    }

    #[test]
    fn finds_a_cached_problem_by_name_or_id() {
        assert_eq!(
            find_name(&listing(), "02_Loop_3").as_deref(),
            Some("02_Loop_3")
        );
        assert_eq!(find_name(&listing(), "42").as_deref(), Some("02_Loop_3"));
        assert_eq!(find_name(&listing(), "02_loop_3"), None);
        assert_eq!(find_name(&listing(), "999"), None);
        assert_eq!(find_name(&[], "01_Expr_11"), None);
    }

    #[test]
    fn a_padded_id_is_the_same_problem() {
        let entries = listing();
        let padded = find_problem(&entries, "007").unwrap();
        assert_eq!(padded["id"].as_i64(), Some(7));
        assert_eq!(find_problem(&entries, "7").unwrap()["id"], padded["id"]);
        assert!(find_problem(&entries, "7x").is_none());
    }

    #[test]
    fn completion_candidates_come_out_in_name_order() {
        let entries = vec![
            json!({"id": 9, "name": "02_Loop_3", "full_name": "Loops"}),
            json!({"id": 7, "name": "01_Expr_11", "full_name": "Expressions"}),
            json!({"id": 8, "name": "01_Expr_12"}),
        ];
        assert_eq!(
            name_and_title(&entries),
            vec![
                ("01_Expr_11".to_string(), "Expressions".to_string()),
                ("01_Expr_12".to_string(), String::new()),
                ("02_Loop_3".to_string(), "Loops".to_string()),
            ]
        );
    }

    #[test]
    fn prefers_a_name_over_an_id() {
        let entries = vec![
            json!({"id": 1, "name": "puzzle"}),
            json!({"id": 2, "name": "1"}),
        ];
        assert_eq!(find_name(&entries, "1").as_deref(), Some("1"));
    }

    #[test]
    fn the_grader_names_the_attachment_and_the_name_carries_the_extension() {
        let dig = json!({"permitted_languages": [{"ext": "dig"}]});
        let star = "attachment; filename=\"Exam1_1_Template.dig\"; filename*=UTF-8''Exam1_1.dig";
        assert_eq!(attachment_ext(Some(star), &dig), "dig");
        assert_eq!(
            attachment_ext(Some("attachment; filename=\"template_01.DIG\""), &dig),
            "dig"
        );
        assert_eq!(
            attachment_ext(Some("attachment; filename=notes.zip"), &dig),
            "zip"
        );
    }

    // no name from the grader: the one language it takes is the next best claim, and
    // guessing past that would be inventing what the bytes are
    #[test]
    fn falls_back_to_the_only_permitted_language_and_then_to_nothing() {
        let dig = json!({"permitted_languages": [{"ext": "dig"}]});
        assert_eq!(attachment_ext(None, &dig), "dig");
        assert_eq!(attachment_ext(Some("attachment"), &dig), "dig");
        let two = json!({"permitted_languages": [{"ext": "cpp"}, {"ext": "py"}]});
        assert_eq!(attachment_ext(None, &two), "bin");
        assert_eq!(attachment_ext(None, &json!({})), "bin");
    }

    // the extension becomes a filename, so the grader does not get to steer it
    #[test]
    fn a_hostile_attachment_name_cannot_pick_the_path() {
        let plain = json!({});
        for hostile in [
            "attachment; filename=\"../../../etc/passwd\"",
            "attachment; filename=\"x.../..\"",
            "attachment; filename=\"x.a/b\"",
            "attachment; filename=\"x.\"",
            "attachment; filename=\"x.cpp exe\"",
        ] {
            assert_eq!(attachment_ext(Some(hostile), &plain), "bin", "{hostile}");
        }
    }

    #[test]
    fn a_problem_with_no_permitted_languages_has_no_opinion() {
        assert!(permitted_exts(&json!({})).is_empty());
        assert!(permitted_exts(&json!({"permitted_languages": null})).is_empty());
        assert_eq!(
            permitted_exts(&json!({"permitted_languages": [{"ext": "dig", "name": "digital"}]})),
            vec!["dig".to_string()]
        );
    }
}
