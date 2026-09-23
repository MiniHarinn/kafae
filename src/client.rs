use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config;
use crate::json;
use crate::offline;
use crate::ui::{ebold, edim, err_tag, fail, fail_as};

// the shape of one session record. An older kafae reading it finds no top-level token and
// asks for a password once; that is the one-way door one file per account costs.
const SESSION_VERSION: u32 = 2;

// one grader account's session, and the only thing kafae stores that is worth stealing.
// It is never written to config.toml.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub version: u32,
    pub url: String,
    pub login: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    // absent means unknown: the token is tried anyway and a 401 drives the renew that
    // has always been there
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
}

impl Session {
    pub fn new(url: &str, login: &str) -> Session {
        let account = Account::new(url, login);
        Session {
            version: SESSION_VERSION,
            url: account.url,
            login: account.login,
            token: None,
            expires: None,
        }
    }

    pub fn account(&self) -> Account {
        Account::new(&self.url, &self.login)
    }
}

// who a session, a cache directory and a session filename all belong to. A grader table's
// name is not part of it, so renaming a grader costs nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub url: String,
    pub login: String,
}

impl Account {
    pub fn new(url: &str, login: &str) -> Account {
        Account {
            url: url.trim().trim_end_matches('/').to_string(),
            login: login.trim().to_string(),
        }
    }

    // one filesystem-safe segment: it names a session file and nests a cache directory, so
    // it must not be able to name anything but itself. Neither slug can contain a ~, so
    // the two halves cannot be read as one another either.
    pub fn key(&self) -> String {
        format!("{}~{}", host_slug(Some(&self.url)), login_slug(&self.login))
    }

    pub fn cache_dir(&self) -> PathBuf {
        cache_root()
            .join(host_slug(Some(&self.url)))
            .join(login_slug(&self.login))
    }
}

// dirs returns None on a machine with no HOME, and this is reached by the completers
// before cli::run, so it degrades instead of panicking
fn cache_root() -> PathBuf {
    config::cache_root().unwrap_or_else(|| std::env::temp_dir().join("kafae"))
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
    let slug: String = host.chars().map(plain).collect();
    // a url of nothing but dots would name the parent directory, not a grader
    if one_segment(&slug) {
        slug
    } else {
        "nohost".to_string()
    }
}

// the v1 cache put these beside the host directory, so a login spelt like one of them
// would land on top of it
const RESERVED: &[&str] = &["problems", "tests", "attachments", "statements"];

// two logins on one grader must not share a cache either: the per-problem files carry
// best_score and last_submission_id, which belong to the account and not to the grader
fn login_slug(login: &str) -> String {
    // long enough for any student id, short enough that the key still fits a filename
    let slug: String = login.trim().chars().take(64).map(plain).collect();
    if !one_segment(&slug) {
        return "nologin".to_string();
    }
    if RESERVED.contains(&slug.as_str()) {
        return format!("{slug}_");
    }
    slug
}

fn plain(c: char) -> char {
    if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
        c
    } else {
        '_'
    }
}

// one command talks to one grader, so the account is settled once and every cache path
// agrees with every other for the life of the process
pub fn current_account() -> Option<&'static Account> {
    static CURRENT: OnceLock<Option<Account>> = OnceLock::new();
    CURRENT
        .get_or_init(|| choose(config::load(), sole_session().as_ref()))
        .as_ref()
}

// url:   --url > KAFAE_URL > [grader.<sel>].url > the url of the single stored session,
//        if the config names no grader at all
// login: --user > KAFAE_USER > [grader.<sel>].login > the login of that same session
//
// KAFAE_URL never destroys a token here: it selects a different account, and unexporting
// it brings the previous session back alive.
fn choose(config: &config::Config, sole: Option<&Session>) -> Option<Account> {
    // With tables around, a selection matching none of them is a typo, and a url taken
    // from whichever session happens to be on this machine submits to the wrong course.
    // The login is not gated: a table is allowed to name only a url.
    let stored_url = sole.filter(|_| config.graders().is_empty());
    let url = config
        .url(None)
        .map(|url| url.value)
        .or_else(|| stored_url.map(|session| session.url.clone()))?;
    let login = config
        .login(None)
        .map(|login| login.value)
        .or_else(|| sole.map(|session| session.login.clone()))?;
    Some(Account::new(&url, &login))
}

pub fn cache_dir() -> PathBuf {
    match current_account() {
        Some(account) => account.cache_dir(),
        None => cache_root().join(host_slug(None)).join(login_slug("")),
    }
}

// Nothing is moved on upgrade: a read falls back to the v1 tree until the next sync fills
// the new path, and kafae clean ages it out.
fn legacy_cache_dir() -> Option<PathBuf> {
    static LEGACY: OnceLock<Option<PathBuf>> = OnceLock::new();
    LEGACY
        .get_or_init(|| legacy_dir_of(legacy_session().as_ref(), current_account()))
        .clone()
}

// The v1 tree belongs to one account and only that account may read it. Keyed on the host
// alone, it is where a second login on the same grader would otherwise find the last
// student's best_score, submission_count and last_submission_id.
fn legacy_dir_of(old: Option<&Session>, current: Option<&Account>) -> Option<PathBuf> {
    let account = old?.account();
    (current == Some(&account)).then(|| cache_root().join(host_slug(Some(&account.url))))
}

fn cache_read(relative: &Path) -> PathBuf {
    let fresh = cache_dir().join(relative);
    if fresh.exists() {
        return fresh;
    }
    match legacy_cache_dir().map(|dir| dir.join(relative)) {
        Some(old) if old.exists() => old,
        _ => fresh,
    }
}

pub fn problems_cache() -> PathBuf {
    cache_dir().join("problems.json")
}

// what a read of the listing lands on, which is the v1 file until the next sync fills the
// account's own
pub fn cached_problems_file() -> PathBuf {
    cache_read(Path::new("problems.json"))
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

fn detail_relative(name: &str) -> PathBuf {
    Path::new("problems").join(format!("{}.json", cache_name(name)))
}

pub fn cached_detail(name: &str) -> Option<Value> {
    let path = cache_read(&detail_relative(name));
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
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

// every grader's cache, not just the one the config names, and the v1 tree with it
pub fn clear_cache() -> u64 {
    discard(&cache_root())
}

pub fn clear_problem_cache(name: &str) -> u64 {
    let legacy = legacy_cache_dir().map_or(0, |dir| discard(&dir.join(detail_relative(name))));
    clear_attachment(name)
        + discard(&tests_dir(name))
        + discard(&statement_file(name, "pdf"))
        + discard(&statement_file(name, "json"))
        + discard(&detail_file(name))
        + legacy
}

// ---------------------------------------------------------------------------
// sessions: one file per account, so two terminals renewing two graders cannot
// contend and a truncated file costs one account rather than all of them
// ---------------------------------------------------------------------------

fn session_file(account: &Account) -> Option<PathBuf> {
    Some(config::sessions_dir()?.join(format!("{}.json", account.key())))
}

// The v1 state.json has no version and carries one account at the top level. It is read
// by shape and never by hoping serde tolerates it: saved_state() swallowed every error
// and returned a default, so leaning on that tolerance would log every existing user out
// silently and look exactly like this change breaking their login.
fn v1_session(text: &str) -> Option<Session> {
    let value: Value = serde_json::from_str(text).ok()?;
    let object = value.as_object()?;
    if object.contains_key("version") || object.contains_key("sessions") {
        return None;
    }
    let field = |key: &str| {
        object
            .get(key)?
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    // half a v1 record names no account, and an account is what a session is filed under
    let mut session = Session::new(field("url")?, field("login")?);
    session.token = field("token").map(String::from);
    Some(session)
}

fn legacy_session() -> Option<Session> {
    let path = config::legacy_state_file()?;
    v1_session(&fs::read_to_string(path).ok()?)
}

fn read_session(path: &Path) -> Option<Session> {
    let session: Session = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    (!session.url.is_empty() && !session.login.is_empty()).then_some(session)
}

// silent on a file it cannot read: this is reached from cache_dir(), which the completers
// reach before cli::run. Not finding a session is what authed_session() reports.
pub fn saved_session(account: &Account) -> Option<Session> {
    let stored = session_file(account).as_deref().and_then(read_session);
    stored.or_else(|| legacy_session().filter(|old| old.account() == *account))
}

pub fn sessions() -> Vec<Session> {
    let mut found: Vec<Session> = config::sessions_dir()
        .and_then(|dir| fs::read_dir(dir).ok())
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
                .filter_map(|path| read_session(&path))
                .collect()
        })
        .unwrap_or_default();
    // the v1 record is a session like any other, converted in memory and never written back
    if let Some(old) = legacy_session() {
        if !found.iter().any(|kept| kept.account() == old.account()) {
            found.push(old);
        }
    }
    found.sort_by(|left, right| (&left.url, &left.login).cmp(&(&right.url, &right.login)));
    found
}

fn sole_session() -> Option<Session> {
    let mut found = sessions();
    (found.len() == 1).then(|| found.remove(0))
}

// 0600 from the moment the file exists, rather than after the write, or a token is
// world-readable for as long as it takes to chmod it
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    // a file that existed before kept whatever mode it had
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    // parity with the 0600 above: drop inherited ACEs, keep only the owner
    #[cfg(windows)]
    if let Ok(user) = std::env::var("USERNAME") {
        use std::process::{Command, Stdio};
        let _ = Command::new("icacls")
            .arg(path)
            .args(["/inheritance:r", "/grant:r", &format!("{user}:F")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    Ok(())
}

pub fn save_session(session: &Session) {
    let Some(path) = session_file(&session.account()) else {
        fail("no state directory on this machine to keep a session in");
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let stored = Session {
        version: SESSION_VERSION,
        ..session.clone()
    };
    let bytes = serde_json::to_vec(&stored).unwrap_or_default();
    write_private(&path, &bytes).unwrap_or_else(|error| fail(&error.to_string()));
}

// one account's session, for kafae logout
pub fn forget_session(account: &Account) -> u64 {
    let mut freed = session_file(account).map_or(0, |path| discard(&path));
    // the v1 record is this account's session too, until a login replaces it
    if legacy_session().is_some_and(|old| old.account() == *account) {
        freed += config::legacy_state_file().map_or(0, |path| discard(&path));
    }
    freed
}

// every account's session, for kafae clean --all
pub fn clear_sessions() -> u64 {
    config::sessions_dir().map_or(0, |dir| discard(&dir))
        + config::legacy_state_file().map_or(0, |path| discard(&path))
}

static RENEWED: Mutex<Option<String>> = Mutex::new(None);

pub fn remember_token(token: Option<&str>) {
    *RENEWED.lock().unwrap() = token.map(String::from);
}

fn token_of(session: &Session) -> Option<String> {
    RENEWED
        .lock()
        .unwrap()
        .clone()
        .or_else(|| session.token.clone())
}

// the account is known and it may or may not have a session yet; a session with no token
// still carries the url and login an interactive renew needs
pub fn current_session() -> Option<Session> {
    let account = current_account()?;
    Some(saved_session(account).unwrap_or_else(|| Session::new(&account.url, &account.login)))
}

fn no_account() -> ! {
    let config = config::load();
    if let Some(name) = config.unknown_selection() {
        fail(&format!(
            "no grader named {}, configured: {}",
            ebold(&name),
            config.grader_names().join(", ")
        ));
    }
    // several tables and nothing naming one of them: the graders are configured, so the
    // missing piece is the selection and not a login
    if config.selected().is_none() && !config.grader_names().is_empty() {
        fail(&format!(
            "no grader selected, configured: {} — pick one with {}",
            config.grader_names().join(", "),
            ebold("kafae use <name>")
        ));
    }
    // a url resolved and the account still did not, so the half that is missing is the
    // login — and --url is not the flag that fills it in
    if config.url(None).is_some() {
        let whose = config
            .selected()
            .map(|grader| format!(" for grader {}", ebold(&grader.value)))
            .unwrap_or_default();
        fail(&format!(
            "no login{whose}, run {} and it will ask for one",
            ebold("kafae login")
        ));
    }
    fail(&format!(
        "no grader url, run {}",
        ebold("kafae login --url <url>")
    ))
}

pub fn authed_session() -> Session {
    let session = current_session().unwrap_or_else(|| no_account());
    if session.token.is_some() {
        return session;
    }
    crate::commands::login::renew(&session).unwrap_or_else(|| {
        fail_as(
            "auth",
            &format!("not logged in, run {}", ebold("kafae login")),
            None,
        )
    })
}

// a cache is readable with a dead token, so offline must not go asking for a password —
// nor for an account, because reading a cache opens no socket
pub fn session_for_reads() -> Session {
    if offline::on() {
        current_session().unwrap_or_default()
    } else {
        authed_session()
    }
}

fn send(
    session: &Session,
    method: minreq::Method,
    route: &str,
    body: Option<&Value>,
) -> minreq::Response {
    // every route to the network comes through here, so one refusal covers them all
    if offline::on() {
        offline::refuse(&format!("/{route}"));
    }
    let base = session.url.clone();
    let mut req = minreq::Request::new(method, format!("{base}/api/v1/{route}")).with_timeout(30);
    if let Some(token) = token_of(session) {
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
    session: &Session,
    method: minreq::Method,
    route: &str,
    body: Option<&Value>,
) -> minreq::Response {
    let resp = send(session, method.clone(), route, body);
    if resp.status_code != 401 || session.token.is_none() {
        return resp;
    }
    match crate::commands::login::renew(session) {
        Some(fresh) => send(&fresh, method, route, body),
        None => resp,
    }
}

fn fail_from(session: &Session, resp: &minreq::Response) -> ! {
    let mut message = resp
        .json::<Value>()
        .ok()
        .and_then(|body| body.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| resp.reason_phrase.clone());
    let expired = resp.status_code == 401 && session.token.is_some();
    if expired {
        message.push_str(&format!(", run {}", ebold("kafae login")));
    }
    fail_as(
        if expired { "auth" } else { "http" },
        &message,
        Some(json!({ "status": resp.status_code })),
    );
}

pub fn api(session: &Session, method: minreq::Method, route: &str, body: Option<&Value>) -> Value {
    let resp = request(session, method, route, body);
    if !(200..300).contains(&resp.status_code) {
        fail_from(session, &resp);
    }
    resp.json().unwrap_or_else(|error| fail(&error.to_string()))
}

pub fn api_bytes(session: &Session, route: &str) -> Option<Vec<u8>> {
    let resp = request(session, minreq::Method::Get, route, None);
    if resp.status_code == 404 {
        return None;
    }
    if !(200..300).contains(&resp.status_code) {
        fail_from(session, &resp);
    }
    Some(resp.as_bytes().to_vec())
}

// like api_bytes, but hands back the name the grader gave the file: for an attachment
// that name is the only thing saying what the bytes are
pub fn api_download(session: &Session, route: &str) -> Option<(Vec<u8>, Option<String>)> {
    let resp = request(session, minreq::Method::Get, route, None);
    if resp.status_code == 404 {
        return None;
    }
    if !(200..300).contains(&resp.status_code) {
        fail_from(session, &resp);
    }
    let disposition = resp.headers.get("content-disposition").cloned();
    Some((resp.as_bytes().to_vec(), disposition))
}

// the file the grader ships with a problem, fetched on the way past like the pdf; offline
// the cache is all there is to offer
pub fn attachment_of(session: &Session, prob: &Value) -> Option<PathBuf> {
    let name = prob["name"].as_str()?;
    if offline::on() {
        return cached_attachment(name);
    }
    if prob["has_attachment"].as_bool() != Some(true) {
        return None;
    }
    let (bytes, disposition) = api_download(
        session,
        &format!("problems/{}/files/attachment", prob["id"]),
    )?;
    let path = attachment_file(name, &attachment_ext(disposition.as_deref(), prob));
    cache_write(&path, bytes);
    Some(path)
}

pub fn title_of(problem: &Value) -> String {
    problem["full_name"].as_str().unwrap_or("").to_string()
}

pub fn get_problems(session: &Session) -> Vec<Value> {
    if offline::on() {
        return cached_listing().unwrap_or_else(|| no_listing());
    }
    let problems = api(session, minreq::Method::Get, "problems", None)
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

pub fn get_submission(session: &Session, id: i64) -> Value {
    api(
        session,
        minreq::Method::Get,
        &format!("submissions/{id}"),
        None,
    )
}

// the problem carries the newest submission's id, so listing them is wasted
pub fn latest_submission(session: &Session, reference: &str) -> Value {
    let problem = resolve_problem(session, reference);
    let Some(id) = problem["last_submission_id"].as_i64() else {
        fail(&format!(
            "no submissions yet for {}",
            ebold(problem["name"].as_str().unwrap_or(reference))
        ));
    };
    get_submission(session, id)
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
    serde_json::from_str(&fs::read_to_string(cached_problems_file()).ok()?).ok()
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

pub fn resolve_problem(session: &Session, reference: &str) -> Value {
    let problems = get_problems(session);
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
            session,
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

    fn config_of(text: &str, env: &[(&str, &str)]) -> config::Config {
        let (file, _) = config::parse(text);
        config::Config::new(file, config::Env::of(env), None)
    }

    const CE: &str = r#"
current = "ce"
[grader.ce]
url = "https://g.example"
login = "6xxxxxxx21"
"#;

    // The one regression this whole change could hide: the old state.json is read by
    // shape, so a student mid-course who upgrades keeps the live token they had. Reading
    // it through serde's tolerance would hand back an empty session and look, to them,
    // exactly like being logged out by a surprise password prompt.
    #[test]
    fn the_old_state_file_is_still_a_live_session_after_upgrading() {
        let v1 = r#"{"url":"https://g.example/","login":"6xxxxxxx21","token":"eyJhbGciOi"}"#;
        let session = v1_session(v1).expect("a v1 state.json is a session");
        assert_eq!(session.token.as_deref(), Some("eyJhbGciOi"));
        assert_eq!(session.url, "https://g.example");
        assert_eq!(session.login, "6xxxxxxx21");
        // and it is filed under the same account the config names, so the token is found
        assert_eq!(
            session.account(),
            Account::new("https://g.example", "6xxxxxxx21")
        );
    }

    // a v2 record is not a v1 one, or the migration would read every session twice
    #[test]
    fn a_v2_session_is_never_read_as_a_v1_one() {
        let session = Session::new("https://g.example", "6xxxxxxx21");
        let text = serde_json::to_string(&session).expect("a session serialises");
        assert!(text.contains("\"version\":2"));
        assert_eq!(v1_session(&text).map(|old| old.url), None);
        // and neither is half a record, or an account would be filed under an empty login
        assert!(v1_session(r#"{"url":"https://g.example"}"#).is_none());
        assert!(v1_session(r#"{"login":"6xxxxxxx21"}"#).is_none());
        assert!(v1_session("").is_none());
        assert!(v1_session("not json at all").is_none());
    }

    // a token must not be thrown away because a variable disagrees with the file: it
    // names another account, whose own session is sitting there alive
    #[test]
    fn the_environment_selects_another_account_and_destroys_no_token() {
        let session = Session::new("https://g.example", "6xxxxxxx21");
        let named = choose(&config_of(CE, &[]), Some(&session)).expect("the config names one");
        assert_eq!(named, session.account());

        let moved = choose(
            &config_of(CE, &[("KAFAE_URL", "https://other.example")]),
            Some(&session),
        )
        .expect("the environment names one");
        assert_eq!(moved.url, "https://other.example");
        assert_eq!(moved.login, "6xxxxxxx21");
        assert_ne!(moved.key(), session.account().key());
        // the old session file is untouched, so unexporting brings it back
        assert_eq!(
            choose(&config_of(CE, &[]), Some(&session)).map(|account| account.key()),
            Some(session.account().key())
        );
    }

    // a user who only ever exported KAFAE_URL and never wrote a config still has one
    #[test]
    fn the_only_session_there_is_names_the_account_when_the_config_does_not() {
        let session = Session::new("https://g.example", "6xxxxxxx21");
        assert_eq!(
            choose(&config_of("", &[]), Some(&session)),
            Some(session.account())
        );
        assert_eq!(
            choose(
                &config_of("", &[("KAFAE_USER", "6xxxxxxx22")]),
                Some(&session)
            )
            .map(|account| account.login),
            Some("6xxxxxxx22".to_string())
        );
        assert_eq!(choose(&config_of("", &[]), None), None);
    }

    // a typo in --grader or KAFAE_GRADER must not resolve to whichever session happens to
    // be on this machine: `kafae --grader alg submit` would authenticate as another course
    // and submit to it. With tables around, a name none of them answers to is an error.
    #[test]
    fn a_grader_name_no_table_answers_to_names_no_account() {
        let session = Session::new("https://g.example", "6xxxxxxx21");
        let (file, _) = config::parse(CE);
        let flagged = config::Config::new(file, config::Env::of(&[]), Some("algoo".to_string()));
        assert_eq!(choose(&flagged, Some(&session)), None);
        assert_eq!(
            choose(&config_of(CE, &[("KAFAE_GRADER", "algoo")]), Some(&session)),
            None
        );
        // a config that names no grader at all is the one the stored session answers for
        assert_eq!(
            choose(&config_of("", &[("KAFAE_GRADER", "algoo")]), Some(&session)),
            Some(session.account())
        );
        // and only the url is gated: a table is allowed to name a grader and no login
        let url_only = "[grader.ce]\nurl = \"https://g.example\"\n";
        assert_eq!(
            choose(&config_of(url_only, &[]), Some(&session)),
            Some(session.account())
        );
    }

    // the v1 tree is keyed on the host alone, so reading it unscoped hands a second login
    // on that grader the first student's scores
    #[test]
    fn only_the_account_the_v1_cache_belongs_to_reads_it() {
        let old = Session::new("https://g.example", "6xxxxxxx21");
        let owner = old.account();
        let second = Account::new("https://g.example", "6xxxxxxx99");
        assert_eq!(
            legacy_dir_of(Some(&old), Some(&owner)),
            Some(cache_root().join("g.example"))
        );
        assert_eq!(legacy_dir_of(Some(&old), Some(&second)), None);
        assert_eq!(legacy_dir_of(None, Some(&owner)), None);
        assert_eq!(legacy_dir_of(Some(&old), None), None);
        // and it is not the account's own directory, which is where a sync writes
        assert_ne!(
            legacy_dir_of(Some(&old), Some(&owner)),
            Some(owner.cache_dir())
        );
    }

    // the one file with a token in it. The mode is the one it is created with, not one it
    // is chmodded to afterwards, so there is no window where it is world-readable.
    #[test]
    fn a_session_is_written_for_its_owner_alone_and_reads_back_whole() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let account = Account::new("https://g.example/", "6xxxxxxx21");
        let path = dir.path().join(format!("{}.json", account.key()));
        let mut session = Session::new(&account.url, &account.login);
        session.token = Some("eyJhbGciOi".to_string());
        session.expires = Some("2026-09-24T09:12:00+07:00".to_string());
        let bytes = serde_json::to_vec(&session).expect("a session serialises");
        write_private(&path, &bytes).expect("and lands");

        let read = read_session(&path).expect("and reads back");
        assert_eq!(read.token.as_deref(), Some("eyJhbGciOi"));
        assert_eq!(read.expires.as_deref(), Some("2026-09-24T09:12:00+07:00"));
        assert_eq!(read.account(), account);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path)
                .expect("it is there")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "{mode:o}");
        }
        // a record naming no account is not a session; it would file itself under nothing
        fs::write(&path, r#"{"version":2,"url":"","login":""}"#).expect("it is writable");
        assert!(read_session(&path).is_none());
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

    // two logins on one grader shared a cache, and the per-problem files in it carry a
    // score and a last_submission_id that belong to one of them
    #[test]
    fn two_logins_on_one_grader_get_a_directory_each() {
        let mine = Account::new("https://g.example", "6xxxxxxx21");
        let yours = Account::new("https://g.example", "6xxxxxxx22");
        assert_ne!(mine.cache_dir(), yours.cache_dir());
        assert_ne!(mine.key(), yours.key());
        assert!(mine
            .cache_dir()
            .starts_with(yours.cache_dir().parent().unwrap()));
        // and the v1 tree is the parent of both, so a login spelt like one of its own
        // directories cannot land on top of it
        assert_eq!(login_slug("problems"), "problems_");
        assert_eq!(login_slug("6xxxxxxx21"), "6xxxxxxx21");
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

    // the account key names a file holding a token, so a login is held to the same rule:
    // one segment, nothing that climbs, and never a name it was not given
    #[test]
    fn a_hostile_account_cannot_escape_the_session_directory() {
        for climb in [
            "..",
            ".",
            "../../..",
            "a/b",
            "",
            "/etc",
            "/",
            "../../id_rsa",
        ] {
            let slug = login_slug(climb);
            assert!(one_segment(&slug), "{climb} -> {slug}");
            let key = Account::new("https://g.example", climb).key();
            assert!(one_segment(&key), "{climb} -> {key}");
            assert!(one_segment(&format!("{key}.json")), "{climb} -> {key}");
        }
        for climb in ["https://../../etc", "https://..", ""] {
            let key = Account::new(climb, "6xxxxxxx21").key();
            assert!(one_segment(&key), "{climb} -> {key}");
        }
        #[cfg(windows)]
        for climb in [r"a\b", r"C:\Windows", r"\\server\share"] {
            assert!(
                one_segment(&login_slug(climb)),
                "{climb} should be rejected"
            );
        }
        // and one hostile half cannot be read as the other
        assert_ne!(
            Account::new("https://a.example", "b~c").key(),
            Account::new("https://a.example~b", "c").key()
        );
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
