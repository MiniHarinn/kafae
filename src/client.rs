use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ui::{ebold, edim, err_tag, fail};

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

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap().join("kafae")
}

pub fn problems_cache() -> PathBuf {
    cache_dir().join("problems.json")
}

pub fn statements_dir() -> PathBuf {
    cache_dir().join("statements")
}

pub fn tests_dir(name: &str) -> PathBuf {
    cache_dir().join("tests").join(name)
}

fn tree_size(path: &Path) -> u64 {
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

pub fn clear_cache() -> u64 {
    discard(&cache_dir())
}

pub fn clear_problems_cache() -> u64 {
    discard(&problems_cache())
}

pub fn clear_problem_cache(name: &str) -> u64 {
    discard(&tests_dir(name))
        + discard(&statements_dir().join(format!("{name}.pdf")))
        + discard(&statements_dir().join(format!("{name}.json")))
}

pub fn clear_state() -> u64 {
    discard(&state_file())
}

pub fn load_state() -> State {
    fs::read_to_string(state_file())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
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
    if state.token.is_none() {
        fail(&format!("not logged in, run {}", ebold("kafae login")));
    }
    state
}

fn request(
    state: &State,
    method: minreq::Method,
    route: &str,
    body: Option<&Value>,
) -> minreq::Response {
    let base = state.url.clone().unwrap_or_default();
    let mut req = minreq::Request::new(method, format!("{base}/api/v1/{route}")).with_timeout(30);
    if let Some(token) = &state.token {
        req = req.with_header("Authorization", format!("Bearer {token}"));
    }
    if let Some(body) = body {
        req = req
            .with_json(body)
            .unwrap_or_else(|error| fail(&error.to_string()));
    }
    req.send()
        .unwrap_or_else(|error| fail(&format!("cannot reach {base}: {error}")))
}

fn fail_from(state: &State, resp: &minreq::Response) -> ! {
    let mut detail = resp
        .json::<Value>()
        .ok()
        .and_then(|body| body.get("error").and_then(|e| e.as_str()).map(String::from))
        .unwrap_or_else(|| resp.reason_phrase.clone());
    if resp.status_code == 401 && state.token.is_some() {
        detail.push_str(&format!(", run {}", ebold("kafae login")));
    }
    fail(&detail);
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

pub fn title_of(problem: &Value) -> String {
    problem["full_name"].as_str().unwrap_or("").to_string()
}

pub fn get_problems(state: &State) -> Vec<Value> {
    let problems = api(state, minreq::Method::Get, "problems", None)
        .as_array()
        .cloned()
        .unwrap_or_default();
    let listing: Vec<Value> = problems
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p["id"].as_i64(),
                "name": p["name"].as_str().unwrap_or(""),
                "title": title_of(p),
                "tags": p["tags"].as_array().cloned().unwrap_or_default(),
            })
        })
        .collect();
    if fs::create_dir_all(cache_dir()).is_ok() {
        let _ = fs::write(problems_cache(), serde_json::to_string(&listing).unwrap());
    }
    problems
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

pub fn cached_problems() -> Vec<(String, String)> {
    fs::read_to_string(problems_cache())
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
        .map(|entries| {
            entries
                .iter()
                .map(|p| {
                    (
                        p["name"].as_str().unwrap_or("").to_string(),
                        p["title"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn cached_tags() -> Vec<String> {
    let mut tags: Vec<String> = fs::read_to_string(problems_cache())
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
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

pub fn cached_problem_name(reference: &str) -> Option<String> {
    let entries: Vec<Value> =
        serde_json::from_str(&fs::read_to_string(problems_cache()).ok()?).ok()?;
    entries
        .iter()
        .find(|p| p["name"].as_str() == Some(reference))
        .or_else(|| {
            let id = reference.parse::<i64>().ok()?;
            entries.iter().find(|p| p["id"].as_i64() == Some(id))
        })
        .and_then(|p| Some(p["name"].as_str()?.to_string()))
}

pub fn resolve_problem(state: &State, reference: &str) -> Value {
    let problems = get_problems(state);
    if let Some(hit) = problems
        .iter()
        .find(|p| p["name"].as_str() == Some(reference))
    {
        return api(
            state,
            minreq::Method::Get,
            &format!("problems/{}", hit["id"]),
            None,
        );
    }

    if !reference.is_empty()
        && reference.chars().all(|c| c.is_ascii_digit())
        && problems
            .iter()
            .any(|p| p["id"].as_i64() == reference.parse().ok())
    {
        return api(
            state,
            minreq::Method::Get,
            &format!("problems/{reference}"),
            None,
        );
    }

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
    std::process::exit(1);
}
