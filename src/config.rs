use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;
use toml_edit::{value, DocumentMut, Item, Table};

use crate::json;
use crate::language::{self, Build};
use crate::templates;
use crate::ui::{edim, err_tag, fail};

// the schema this binary writes. A file that says more than this is read anyway: a config
// from a newer kafae must not stop an older one from working
const VERSION: i64 = 1;

// the whole key set, twice: once for what serde fills in and once for the walk that tells
// a user about a key nothing will ever read
const GLOBAL_KEYS: &[&str] = &[
    "version", "current", "template", "cc", "cxx", "cflags", "cxxflags", "grader",
];
// no cc/cxx: a compiler path is a property of this machine, never of a grader
const GRADER_KEYS: &[&str] = &["url", "login", "template", "cflags", "cxxflags"];

// a file kafae cannot parse at all is the one thing here that stops a command
const UNPARSEABLE: &str = "the config file is not valid toml";

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct File {
    pub version: Option<i64>,
    pub current: Option<String>,
    // the machine-wide settings, flattened by hand: #[serde(flatten)] cannot be told to
    // reject unknown keys and it degrades every error message it touches
    pub template: Option<String>,
    pub cc: Option<String>,
    pub cxx: Option<String>,
    pub cflags: Option<String>,
    pub cxxflags: Option<String>,
    pub grader: BTreeMap<String, Grader>,
}

// every leaf is an Option: an unset grader key falls through to the global key and an
// unset global key falls through to the compiled-in default, and a String cannot say
// "keep falling"
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Grader {
    pub url: Option<String>,
    pub login: Option<String>,
    pub template: Option<String>,
    pub cflags: Option<String>,
    pub cxxflags: Option<String>,
}

impl File {
    fn global(&self, key: &str) -> Option<&str> {
        match key {
            "current" => self.current.as_deref(),
            "template" => self.template.as_deref(),
            "cc" => self.cc.as_deref(),
            "cxx" => self.cxx.as_deref(),
            "cflags" => self.cflags.as_deref(),
            "cxxflags" => self.cxxflags.as_deref(),
            _ => None,
        }
    }
}

impl Grader {
    fn get(&self, key: &str) -> Option<&str> {
        match key {
            "url" => self.url.as_deref(),
            "login" => self.login.as_deref(),
            "template" => self.template.as_deref(),
            "cflags" => self.cflags.as_deref(),
            "cxxflags" => self.cxxflags.as_deref(),
            _ => None,
        }
    }
}

// where a value came from, so "why is it compiling with g++-11" is answerable without
// reading source; the answer is almost always a stale export in a shell rc
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Flag,
    Env(&'static str),
    Grader(String),
    Global,
    Default,
    Session,
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Source::Flag => "flag".to_string(),
            Source::Env(name) => (*name).to_string(),
            Source::Grader(name) => format!("[grader.{name}]"),
            Source::Global => "config".to_string(),
            Source::Default => "default".to_string(),
            Source::Session => "session".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub value: String,
    pub source: Source,
}

impl Resolved {
    fn new(value: impl Into<String>, source: Source) -> Resolved {
        Resolved {
            value: value.into(),
            source,
        }
    }
}

// today's rule, kept: a value is trimmed and an empty string counts as unset, so
// KAFAE_URL="" still means "not set" and never means "the grader is the empty string"
pub fn from_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(String::from)
}

// the environment a resolution reads. Tests hand in a fixed set so they do not depend on
// the developer's shell; the real one reads the process each time, because a command may
// set a variable for a child and the suite already mutates the process environment
#[derive(Clone, Default)]
pub struct Env {
    fixed: Option<BTreeMap<String, String>>,
}

impl Env {
    pub fn process() -> Env {
        Env { fixed: None }
    }

    // tests only: a command always reads the process, and a fixed set is what keeps the
    // suite off the developer's shell
    #[cfg(test)]
    pub fn of(pairs: &[(&str, &str)]) -> Env {
        Env {
            fixed: Some(
                pairs
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.to_string()))
                    .collect(),
            ),
        }
    }

    fn get(&self, name: &str) -> Option<String> {
        match &self.fixed {
            Some(fixed) => trimmed(fixed.get(name).map(String::as_str)),
            None => from_env(name),
        }
    }
}

// every setting key `k` has the environment variable KAFAE_K, so the language table stays
// the single source of truth for the default, the variable and the config key alike
pub fn key_of(env: &str) -> String {
    env.strip_prefix("KAFAE_").unwrap_or(env).to_lowercase()
}

fn normalize_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

pub struct Config {
    file: File,
    env: Env,
    grader_flag: Option<String>,
    path: Option<PathBuf>,
    exists: bool,
    warnings: Vec<String>,
}

impl Config {
    // pure: no globals, no disk. resolve() being a function of (file, env, selection) is
    // what keeps the tests off a process-global cache and a process-global environment
    pub fn new(file: File, env: Env, grader_flag: Option<String>) -> Config {
        Config {
            file,
            env,
            grader_flag,
            path: None,
            exists: false,
            warnings: Vec::new(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn exists(&self) -> bool {
        self.exists
    }

    // derived rather than stored, so a Config built in a test says exactly what one read
    // off disk says
    pub fn error(&self) -> Option<&str> {
        self.warnings
            .iter()
            .find(|warning| warning.starts_with(UNPARSEABLE))
            .map(String::as_str)
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn graders(&self) -> &BTreeMap<String, Grader> {
        &self.file.grader
    }

    pub fn grader_names(&self) -> Vec<String> {
        self.file.grader.keys().cloned().collect()
    }

    // --grader > KAFAE_GRADER > current > the only table there is > nothing, which is the
    // implicit grader a user who only ever exported KAFAE_URL has
    pub fn selected(&self) -> Option<Resolved> {
        if let Some(name) = trimmed(self.grader_flag.as_deref()) {
            return Some(Resolved::new(name, Source::Flag));
        }
        if let Some(name) = self.env.get("KAFAE_GRADER") {
            return Some(Resolved::new(name, Source::Env("KAFAE_GRADER")));
        }
        if let Some(name) = trimmed(self.file.current.as_deref()) {
            return Some(Resolved::new(name, Source::Global));
        }
        match self.file.grader.keys().collect::<Vec<_>>().as_slice() {
            [only] => Some(Resolved::new(only.as_str(), Source::Default)),
            _ => None,
        }
    }

    fn grader_entry(&self) -> Option<(&str, &Grader)> {
        let name = self.selected()?.value;
        let (name, grader) = self.file.grader.get_key_value(&name)?;
        Some((name.as_str(), grader))
    }

    // a selection naming no table is a typo worth an error; with no tables at all it is
    // simply a user who has not written a config yet
    pub fn unknown_selection(&self) -> Option<String> {
        let selected = self.selected()?;
        if self.file.grader.is_empty() || self.file.grader.contains_key(&selected.value) {
            return None;
        }
        Some(selected.value)
    }

    pub fn url(&self, flag: Option<&str>) -> Option<Resolved> {
        if let Some(url) = trimmed(flag) {
            return Some(Resolved::new(normalize_url(&url), Source::Flag));
        }
        if let Some(url) = self.env.get("KAFAE_URL") {
            return Some(Resolved::new(normalize_url(&url), Source::Env("KAFAE_URL")));
        }
        let (name, grader) = self.grader_entry()?;
        let url = trimmed(grader.url.as_deref())?;
        Some(Resolved::new(
            normalize_url(&url),
            Source::Grader(name.to_string()),
        ))
    }

    pub fn login(&self, flag: Option<&str>) -> Option<Resolved> {
        if let Some(login) = trimmed(flag) {
            return Some(Resolved::new(login, Source::Flag));
        }
        if let Some(login) = self.env.get("KAFAE_USER") {
            return Some(Resolved::new(login, Source::Env("KAFAE_USER")));
        }
        let (name, grader) = self.grader_entry()?;
        let login = trimmed(grader.login.as_deref())?;
        Some(Resolved::new(login, Source::Grader(name.to_string())))
    }

    // flag > KAFAE_<KEY> > [grader.<sel>].<key> > top-level <key> > compiled-in default
    fn layered(
        &self,
        key: &str,
        env: &'static str,
        flag: Option<&str>,
        fallback: &str,
        per_grader: bool,
    ) -> Resolved {
        if let Some(value) = trimmed(flag) {
            return Resolved::new(value, Source::Flag);
        }
        if let Some(value) = self.env.get(env) {
            return Resolved::new(value, Source::Env(env));
        }
        if per_grader {
            if let Some((name, grader)) = self.grader_entry() {
                if let Some(value) = trimmed(grader.get(key)) {
                    return Resolved::new(value, Source::Grader(name.to_string()));
                }
            }
        }
        if let Some(value) = trimmed(self.file.global(key)) {
            return Resolved::new(value, Source::Global);
        }
        Resolved::new(fallback, Source::Default)
    }

    pub fn template(&self, flag: Option<&str>) -> Resolved {
        self.layered("template", "KAFAE_TEMPLATE", flag, templates::DEFAULT, true)
    }

    // None for a language nothing compiles here; a script has no compiler to name
    pub fn compiler(&self, local: &language::Local) -> Option<Resolved> {
        let Build::Compiler { env, default, .. } = local.build else {
            return None;
        };
        Some(self.layered(&key_of(env), env, None, default, false))
    }

    pub fn flags(&self, local: &language::Local) -> Option<Resolved> {
        let Build::Compiler {
            flags_env, flags, ..
        } = local.build
        else {
            return None;
        };
        Some(self.layered(&key_of(flags_env), flags_env, None, flags, true))
    }

    // an override that moves the account announces itself before anything reaches the
    // network; a compiler flag doing the same on every compile would just be noise
    pub fn override_notices(&self) -> Vec<String> {
        let Some((name, grader)) = self.grader_entry() else {
            return Vec::new();
        };
        [
            ("KAFAE_URL", grader.url.as_deref(), true),
            ("KAFAE_USER", grader.login.as_deref(), false),
        ]
        .into_iter()
        .filter_map(|(env, configured, is_url)| {
            // both sides as the account is built from them: a url pasted out of a browser
            // differs by a trailing slash and moves nothing, and a line on every command
            // that says otherwise is the noise this one exists to avoid
            let shape = |value: &str| {
                if is_url {
                    normalize_url(value)
                } else {
                    value.trim().to_string()
                }
            };
            let configured = shape(&trimmed(configured)?);
            let overriding = shape(&self.env.get(env)?);
            (configured != overriding)
                .then(|| format!("{env} overrides grader {name} ({configured} -> {overriding})"))
        })
        .collect()
    }
}

// ---------------------------------------------------------------------------
// where everything lives. KAFAE_HOME moves config, templates, sessions and cache
// together: relocating one of the four is the partial isolation that surprises everyone
// who tries it, and it is what makes the suite hermetic.
// ---------------------------------------------------------------------------

// every path kafae keeps. None is a machine with no HOME, where dirs has nothing to
// offer: this sits on the completion path, so it degrades rather than panicking.
pub struct Paths {
    pub config: Option<PathBuf>,
    pub templates: Option<PathBuf>,
    pub sessions: Option<PathBuf>,
    // the v1 single-account file, read for as long as anyone still has one and never
    // written again
    pub legacy_state: Option<PathBuf>,
    pub cache: Option<PathBuf>,
}

fn home() -> Option<PathBuf> {
    from_env("KAFAE_HOME").map(PathBuf::from)
}

// pure, and the whole layout at once: kafae config path prints all five, and moving one
// of them without the others is the partial isolation this variable exists to avoid
fn paths_of(home: Option<&Path>) -> Paths {
    if let Some(home) = home {
        return Paths {
            config: Some(home.join("config.toml")),
            templates: Some(home.join("templates")),
            sessions: Some(home.join("sessions")),
            legacy_state: Some(home.join("state.json")),
            cache: Some(home.join("cache")),
        };
    }
    let config = dirs::config_dir().map(|dir| dir.join("kafae"));
    // state_dir is None on macOS and Windows, where data_dir is the next best thing
    let state = dirs::state_dir()
        .or_else(dirs::data_dir)
        .map(|dir| dir.join("kafae"));
    Paths {
        config: config.as_ref().map(|dir| dir.join("config.toml")),
        templates: config.map(|dir| dir.join("templates")),
        sessions: state.as_ref().map(|dir| dir.join("sessions")),
        legacy_state: state.map(|dir| dir.join("state.json")),
        cache: dirs::cache_dir().map(|dir| dir.join("kafae")),
    }
}

pub fn paths() -> Paths {
    paths_of(home().as_deref())
}

pub fn config_file() -> Option<PathBuf> {
    paths().config
}

pub fn templates_dir() -> Option<PathBuf> {
    paths().templates
}

pub fn sessions_dir() -> Option<PathBuf> {
    paths().sessions
}

pub fn legacy_state_file() -> Option<PathBuf> {
    paths().legacy_state
}

pub fn cache_root() -> Option<PathBuf> {
    paths().cache
}

// ---------------------------------------------------------------------------
// reading
// ---------------------------------------------------------------------------

// infallible and silent: clap_complete runs the completers before cli::run, and a panic,
// a prompt or a stderr write here breaks tab completion in every shell for every command.
// What it has to say is kept for announce() to print on the ordinary command path.
pub fn load() -> &'static Config {
    static LOADED: OnceLock<Config> = OnceLock::new();
    LOADED.get_or_init(|| {
        let path = config_file();
        let text = path.as_ref().map(fs::read_to_string);
        let mut config = match &text {
            Some(Ok(text)) => {
                let (file, warnings) = parse(text);
                Config {
                    warnings,
                    ..Config::new(file, Env::process(), grader_flag())
                }
            }
            // a missing file is not an error; anything else about it is worth saying
            Some(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                Config::new(File::default(), Env::process(), grader_flag())
            }
            Some(Err(error)) => Config {
                warnings: vec![format!("cannot read the config file: {error}")],
                ..Config::new(File::default(), Env::process(), grader_flag())
            },
            None => Config::new(File::default(), Env::process(), grader_flag()),
        };
        config.exists = matches!(&text, Some(Ok(_)));
        config.path = path;
        config
    })
}

static GRADER_FLAG: OnceLock<Option<String>> = OnceLock::new();

// --grader is only known once clap has parsed, so cli::run hands it over before anything
// resolves. Set it after the first load() and it has already missed its turn.
pub fn select(name: Option<String>) {
    let _ = GRADER_FLAG.set(name);
}

fn grader_flag() -> Option<String> {
    GRADER_FLAG.get().cloned().flatten()
}

// the ordinary command path, off the completion path: json::enable() and offline::enable()
// come first, then this, then the first cache_dir() call
pub fn announce() {
    let config = load();
    if let Some(error) = config.error() {
        fail(error);
    }
    // under --json every diagnostic on stderr is an object, and a consumer that has
    // learned to read the stream as newline-delimited json chokes on a bare warning line
    if json::on() {
        if !config.warnings().is_empty() {
            eprintln!("{}", serde_json::json!({ "warnings": config.warnings() }));
        }
        return;
    }
    for warning in config.warnings() {
        eprintln!("{} {warning}", err_tag());
    }
    for notice in config.override_notices() {
        eprintln!("{}", edim(notice));
    }
}

// unknown keys warn and are ignored, and one malformed grader never takes the rest of the
// file down with it: a file from a newer kafae must still work
pub fn parse(text: &str) -> (File, Vec<String>) {
    let mut warnings = Vec::new();
    let mut doc = match text.parse::<DocumentMut>() {
        Ok(doc) => doc,
        Err(error) => {
            warnings.push(format!("{UNPARSEABLE}: {error}"));
            return (File::default(), warnings);
        }
    };
    sanitize(&mut doc, &mut warnings);
    match toml_edit::de::from_document::<File>(doc) {
        Ok(file) => {
            if file.version.is_some_and(|version| version > VERSION) {
                warnings.push(format!(
                    "the config file says version {}, and this kafae knows {VERSION}; reading it anyway",
                    file.version.unwrap_or_default()
                ));
            }
            (file, warnings)
        }
        Err(error) => {
            warnings.push(format!("{UNPARSEABLE}: {error}"));
            (File::default(), warnings)
        }
    }
}

fn sanitize(doc: &mut DocumentMut, warnings: &mut Vec<String>) {
    let mut drop = Vec::new();
    for (key, item) in doc.iter() {
        let complaint = match key {
            "grader" if item.as_table_like().is_none() => {
                Some("grader must be a table of [grader.<name>] tables".to_string())
            }
            "grader" => None,
            "version" if item.as_integer().is_none() => {
                Some("version must be a number".to_string())
            }
            "version" => None,
            _ if !GLOBAL_KEYS.contains(&key) => Some(unknown(key, None)),
            _ if item.as_str().is_none() => Some(format!("{key} must be a string")),
            _ => None,
        };
        if let Some(complaint) = complaint {
            warnings.push(format!("{complaint}; ignoring it"));
            drop.push(key.to_string());
        }
    }
    for key in drop {
        doc.remove(&key);
    }
    sanitize_graders(doc, warnings);
}

fn sanitize_graders(doc: &mut DocumentMut, warnings: &mut Vec<String>) {
    let Some(graders) = doc.get("grader").and_then(Item::as_table_like) else {
        return;
    };
    let mut drop_graders = Vec::new();
    let mut drop_keys = Vec::new();
    for (name, item) in graders.iter() {
        // a name with a dot would silently have made [grader.a.b] a grader named b inside
        // a grader named a, so it is refused rather than half-understood
        if !valid_name(name) {
            warnings.push(format!(
                "grader name {name} is not letters, digits, - or _; ignoring it"
            ));
            drop_graders.push(name.to_string());
            continue;
        }
        let Some(table) = item.as_table_like() else {
            warnings.push(format!("[grader.{name}] must be a table; ignoring it"));
            drop_graders.push(name.to_string());
            continue;
        };
        for (key, value) in table.iter() {
            let complaint = if !GRADER_KEYS.contains(&key) {
                unknown(key, Some(name))
            } else if value.as_str().is_none() {
                format!("{key} in [grader.{name}] must be a string")
            } else {
                continue;
            };
            warnings.push(format!("{complaint}; ignoring it"));
            drop_keys.push((name.to_string(), key.to_string()));
        }
    }
    let Some(graders) = doc.get_mut("grader").and_then(Item::as_table_like_mut) else {
        return;
    };
    for (name, key) in drop_keys {
        if let Some(table) = graders.get_mut(&name).and_then(Item::as_table_like_mut) {
            table.remove(&key);
        }
    }
    for name in drop_graders {
        graders.remove(&name);
    }
}

fn unknown(key: &str, grader: Option<&str>) -> String {
    let known = if grader.is_some() {
        GRADER_KEYS
    } else {
        GLOBAL_KEYS
    };
    let where_ = match grader {
        Some(name) => format!(" in [grader.{name}]"),
        None => String::new(),
    };
    let near = known
        .iter()
        .map(|known| (distance(key, known), *known))
        .filter(|(distance, _)| *distance <= 2)
        .min_by_key(|(distance, _)| *distance);
    match near {
        Some((_, near)) => format!("unknown key {key}{where_}; did you mean {near}?"),
        None => format!("unknown key {key}{where_}"),
    }
}

// enough to catch a transposition or a dropped letter, which is every typo a seven-key
// file can have
fn distance(from: &str, to: &str) -> usize {
    let (from, to): (Vec<char>, Vec<char>) = (from.chars().collect(), to.chars().collect());
    let mut row: Vec<usize> = (0..=to.len()).collect();
    for (i, left) in from.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, right) in to.iter().enumerate() {
            let cost = usize::from(left != right);
            let next = (row[j] + 1).min(row[j + 1] + 1).min(diagonal + cost);
            diagonal = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[to.len()]
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// ---------------------------------------------------------------------------
// writing. Only login, use and config edit ever reach here: a config file is never a
// side effect of problems, test or submit.
// ---------------------------------------------------------------------------

// The file's whole value is that it is commented, so a write goes through toml_edit and
// the comments survive a kafae use. Nothing trails the last key on purpose: toml_edit
// appends a new [grader.<name>] after the root keys, and a comment block below them
// would end up under the tables it is meant to introduce.
pub const TEMPLATE: &str = r#"# kafae configuration. Created by `kafae login`; open it with `kafae config edit`.
#
# Every setting key here has an environment variable that overrides it for one
# command: a key named `cxx` is overridden by KAFAE_CXX, `cxxflags` by
# KAFAE_CXXFLAGS, and so on. Run `kafae config show` to see what is in effect
# and where each value came from. No token is ever kept here.

# Schema version. Leave it alone. A file newer than your kafae warns and is
# still read; kafae never rewrites this line on its own.
version = 1

# ---------------------------------------------------------------------------
# Settings that belong to THIS MACHINE.
# A compiler path is a property of this laptop, never of a course, so `cc` and
# `cxx` can only be set here and not inside a grader table below. Uncomment a
# line to change it; left commented, kafae's own defaults apply and keep up to
# date with it.
# ---------------------------------------------------------------------------
#
# The C and C++ compilers `kafae run`, `test` and the pre-submit check use.
# Defaults are "gcc" and "g++". On a mac with Homebrew gcc, for code that
# includes <bits/stdc++.h>:
#   cxx = "g++-15"
# cc = "gcc"
# cxx = "g++"
#
# Default compiler flags. These mirror what the grader compiles with, plus
# -DLOCAL so `#ifdef LOCAL` debug output strips itself on submit. A course that
# grades at a different -std overrides them in its own table below.
# cflags = "-O2 -std=c99 -DCONTEST -DLOCAL -lm -Wall"
# cxxflags = "-O2 -std=c++17 -DCONTEST -DLOCAL -lm -Wall"
#
# What `kafae new` starts a solution from when -t is absent. `kafae templates`
# lists the names; "attachment" means the file the problem itself ships.
# template = "default"

# ---------------------------------------------------------------------------
# One table per grader, below. The name is yours — it is what `kafae use`,
# --grader and KAFAE_GRADER take, and renaming one costs nothing (your session
# and your cache belong to the account, not to the name). Names are letters,
# digits, - and _ only.
#
# Only `url` is required. `template`, `cflags` and `cxxflags` override the
# machine-wide values above for that grader alone.
# ---------------------------------------------------------------------------

# Which grader every command talks to unless something else says otherwise.
# Change it with `kafae use <name>`; override it for one shell or one command
# with KAFAE_GRADER=<name> or --grader <name>.
current = "default"
"#;

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    // a torn config would be a syntax error on the next command, so it lands whole
    let temp = path.with_extension("toml.new");
    let write = || -> std::io::Result<()> {
        let mut file = fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, path)
    };
    write().map_err(|error| {
        let _ = fs::remove_file(&temp);
        format!("{}: {error}", path.display())
    })
}

// a missing file starts from the template, so the comments are there the first time too
fn edit_at(path: &Path, mutate: impl FnOnce(&mut DocumentMut)) -> Result<PathBuf, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => TEMPLATE.to_string(),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|error| format!("{} is not valid toml: {error}", path.display()))?;
    mutate(&mut doc);
    write_file(path, &doc.to_string())?;
    Ok(path.to_path_buf())
}

// Err rather than a failure: an immutably-managed config directory is a real machine, and
// the session record carries url and login well enough to keep every command working
fn edit(mutate: impl FnOnce(&mut DocumentMut)) -> Result<PathBuf, String> {
    let path = config_file().ok_or("no config directory on this machine")?;
    edit_at(&path, mutate)
}

// login, use and config edit all want the file to exist before they say anything about it
pub fn ensure_file() -> Result<PathBuf, String> {
    let path = config_file().ok_or("no config directory on this machine")?;
    if path.exists() {
        return Ok(path);
    }
    write_file(&path, TEMPLATE)?;
    Ok(path)
}

fn named(name: &str) -> Result<(), String> {
    valid_name(name)
        .then_some(())
        .ok_or_else(|| format!("{name} is not a grader name: letters, digits, - and _ only"))
}

fn put_current(doc: &mut DocumentMut, name: &str) {
    doc["current"] = value(name);
}

// None for a field leaves whatever the file already says about it: an ad-hoc override is
// not the table's own value, and the student wrote that line, not kafae
fn put_grader(
    doc: &mut DocumentMut,
    name: &str,
    url: Option<&str>,
    login: Option<&str>,
    claim_current: bool,
) {
    let graders = doc
        .entry("grader")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut();
    if let Some(graders) = graders {
        // [grader] itself is never written: only [grader.<name>] is a real table
        graders.set_implicit(true);
        let fresh = !graders.contains_key(name);
        let entry = graders
            .entry(name)
            .or_insert(Item::Table(Table::new()))
            .as_table_mut();
        if let Some(entry) = entry {
            // a blank line before a table kafae is adding to a hand-written file
            if fresh {
                entry.decor_mut().set_prefix("\n");
            }
            if let Some(url) = url {
                entry["url"] = value(normalize_url(url));
            }
            if let Some(login) = login {
                entry["login"] = value(login.trim());
            }
        }
    }
    // The template ships a current nothing is configured under, so the first grader
    // written takes it over — a file naming a grader that does not exist is one kafae
    // wrote wrong. After that, current follows only when nothing has claimed it: logging
    // into a second grader must not move the first out from under another terminal.
    if claim_current || trimmed(doc.get("current").and_then(Item::as_str)).is_none() {
        put_current(doc, name);
    }
}

pub fn set_current(name: &str) -> Result<PathBuf, String> {
    named(name)?;
    edit(|doc| put_current(doc, name))
}

// what login writes back; a None field is one login resolved from the environment and has
// no business outliving the shell that exported it. claim_current is for the first grader
// in the file: until one is written, current names the template's "default" and no table
// answers to it.
pub fn remember_grader(
    name: &str,
    url: Option<&str>,
    login: Option<&str>,
    claim_current: bool,
) -> Result<PathBuf, String> {
    named(name)?;
    edit(|doc| put_grader(doc, name, url, login, claim_current))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str, env: &[(&str, &str)]) -> Config {
        let (file, warnings) = parse(text);
        Config {
            warnings,
            ..Config::new(file, Env::of(env), None)
        }
    }

    const TWO: &str = r#"
version = 1
current = "ce"
cxx = "g++-15"
cxxflags = "-O2 -std=c++17"

[grader.ce]
url = "https://grader.cp.eng.chula.ac.th/"
login = "6xxxxxxx21"

[grader.algo]
url = "https://algo.example"
login = "6xxxxxxx21"
cxxflags = "-O2 -std=c++20"
template = "attachment"
"#;

    fn cpp() -> &'static language::Local {
        language::for_ext("cpp").expect("c++ is a language kafae knows")
    }

    fn c() -> &'static language::Local {
        language::for_ext("c").expect("c is a language kafae knows")
    }

    #[test]
    fn a_missing_config_is_not_an_error_and_everything_still_resolves() {
        let none = config("", &[]);
        assert!(none.warnings().is_empty());
        assert_eq!(none.selected(), None);
        assert_eq!(none.url(None), None);
        assert_eq!(none.template(None).source, Source::Default);
        assert_eq!(none.template(None).value, templates::DEFAULT);
        assert_eq!(none.compiler(cpp()).unwrap().value, "g++");
        assert_eq!(none.flags(c()).unwrap().source, Source::Default);
    }

    #[test]
    fn current_names_the_grader_and_a_flag_and_the_environment_outrank_it() {
        let file = config(TWO, &[]);
        assert_eq!(file.selected().unwrap().value, "ce");
        assert_eq!(file.selected().unwrap().source, Source::Global);

        let env = config(TWO, &[("KAFAE_GRADER", "algo")]);
        assert_eq!(env.selected().unwrap().value, "algo");
        assert_eq!(env.selected().unwrap().source, Source::Env("KAFAE_GRADER"));

        let (parsed, _) = parse(TWO);
        let flag = Config::new(
            parsed,
            Env::of(&[("KAFAE_GRADER", "algo")]),
            Some("ce".into()),
        );
        assert_eq!(flag.selected().unwrap().value, "ce");
        assert_eq!(flag.selected().unwrap().source, Source::Flag);
    }

    // one grader needs no naming, and a file that names none of them is the user who only
    // ever exported KAFAE_URL
    #[test]
    fn the_only_grader_there_is_needs_no_naming() {
        let one = config("[grader.ce]\nurl = \"https://g.example\"\n", &[]);
        assert_eq!(one.selected().unwrap().value, "ce");
        assert_eq!(one.selected().unwrap().source, Source::Default);

        let two = config(
            "[grader.ce]\nurl = \"https://a\"\n[grader.algo]\nurl = \"https://b\"\n",
            &[],
        );
        assert_eq!(two.selected(), None);
        assert_eq!(two.url(None), None);
    }

    #[test]
    fn the_selected_grader_names_the_url_and_the_login() {
        let file = config(TWO, &[]);
        let url = file.url(None).unwrap();
        // the trailing slash is the grader's, not part of the address
        assert_eq!(url.value, "https://grader.cp.eng.chula.ac.th");
        assert_eq!(url.source, Source::Grader("ce".to_string()));
        assert_eq!(file.login(None).unwrap().value, "6xxxxxxx21");
    }

    #[test]
    fn the_environment_overrides_the_url_without_naming_another_grader() {
        let env = config(TWO, &[("KAFAE_URL", "https://other.example/")]);
        assert_eq!(env.selected().unwrap().value, "ce");
        assert_eq!(env.url(None).unwrap().value, "https://other.example");
        assert_eq!(env.url(None).unwrap().source, Source::Env("KAFAE_URL"));
        assert_eq!(
            env.override_notices(),
            vec!["KAFAE_URL overrides grader ce (https://grader.cp.eng.chula.ac.th -> https://other.example)".to_string()]
        );
        assert!(config(TWO, &[]).override_notices().is_empty());
    }

    // a url pasted out of a browser carries a trailing slash, resolves to the same
    // account and the same cache, and must not announce an override on every command
    #[test]
    fn an_override_that_changes_nothing_is_not_announced() {
        let same = config(
            TWO,
            &[
                ("KAFAE_URL", "https://grader.cp.eng.chula.ac.th"),
                ("KAFAE_USER", " 6xxxxxxx21 "),
            ],
        );
        assert!(
            same.override_notices().is_empty(),
            "{:?}",
            same.override_notices()
        );
        assert_eq!(
            same.url(None).unwrap().value,
            config(TWO, &[]).url(None).unwrap().value
        );
    }

    // an empty export means "not set", or KAFAE_URL="" would name the empty grader
    #[test]
    fn an_empty_export_is_not_a_value() {
        let blank = config(TWO, &[("KAFAE_URL", "  "), ("KAFAE_GRADER", "")]);
        assert_eq!(blank.selected().unwrap().value, "ce");
        assert_eq!(
            blank.url(None).unwrap().source,
            Source::Grader("ce".to_string())
        );
    }

    #[test]
    fn a_flag_outranks_the_environment_and_the_file_for_a_url() {
        let flag = config(TWO, &[("KAFAE_URL", "https://other.example")]);
        let url = flag.url(Some("https://flag.example/")).unwrap();
        assert_eq!(url.value, "https://flag.example");
        assert_eq!(url.source, Source::Flag);
    }

    #[test]
    fn a_grader_setting_outranks_the_machine_wide_one_and_both_outrank_the_default() {
        let ce = config(TWO, &[]);
        assert_eq!(ce.flags(cpp()).unwrap().value, "-O2 -std=c++17");
        assert_eq!(ce.flags(cpp()).unwrap().source, Source::Global);
        assert_eq!(ce.template(None).value, templates::DEFAULT);

        let algo = config(TWO, &[("KAFAE_GRADER", "algo")]);
        assert_eq!(algo.flags(cpp()).unwrap().value, "-O2 -std=c++20");
        assert_eq!(
            algo.flags(cpp()).unwrap().source,
            Source::Grader("algo".to_string())
        );
        assert_eq!(algo.template(None).value, "attachment");
        // c++ flags in a grader table say nothing about c
        assert_eq!(algo.flags(c()).unwrap().source, Source::Default);
    }

    // a compiler path is a property of this machine, so a grader table cannot hold one
    #[test]
    fn a_compiler_never_comes_from_a_grader_table() {
        let with_cxx = config(
            "cxx = \"g++-15\"\n[grader.ce]\nurl = \"https://a\"\ncxx = \"clang++\"\n",
            &[],
        );
        assert_eq!(with_cxx.compiler(cpp()).unwrap().value, "g++-15");
        assert!(with_cxx
            .warnings()
            .iter()
            .any(|warning| warning.contains("unknown key cxx in [grader.ce]")));
    }

    #[test]
    fn the_environment_outranks_every_layer_of_the_file() {
        let env = config(
            TWO,
            &[
                ("KAFAE_GRADER", "algo"),
                ("KAFAE_CXX", "clang++"),
                ("KAFAE_CXXFLAGS", "-std=kafae"),
                ("KAFAE_TEMPLATE", "py"),
            ],
        );
        assert_eq!(
            env.compiler(cpp()).unwrap().source,
            Source::Env("KAFAE_CXX")
        );
        assert_eq!(env.compiler(cpp()).unwrap().value, "clang++");
        assert_eq!(env.flags(cpp()).unwrap().value, "-std=kafae");
        assert_eq!(env.template(None).value, "py");
        // and -t still outranks the environment
        assert_eq!(env.template(Some("c")).value, "c");
        assert_eq!(env.template(Some("c")).source, Source::Flag);
    }

    // the language table is the single source of truth for the default, the variable and
    // the config key, so the key is derived from the variable rather than written twice
    #[test]
    fn a_setting_key_is_its_environment_variable_without_the_prefix() {
        assert_eq!(key_of("KAFAE_CXX"), "cxx");
        assert_eq!(key_of("KAFAE_CXXFLAGS"), "cxxflags");
        assert_eq!(key_of("KAFAE_CC"), "cc");
        assert_eq!(key_of("KAFAE_CFLAGS"), "cflags");
        assert_eq!(key_of("KAFAE_TEMPLATE"), "template");
        for local in [cpp(), c()] {
            let Build::Compiler { env, flags_env, .. } = local.build else {
                panic!("{} is compiled", local.name);
            };
            assert!(GLOBAL_KEYS.contains(&key_of(env).as_str()));
            assert!(GLOBAL_KEYS.contains(&key_of(flags_env).as_str()));
            assert!(GRADER_KEYS.contains(&key_of(flags_env).as_str()));
        }
    }

    // a file from a newer kafae must still work, so nothing unknown is ever fatal
    #[test]
    fn an_unknown_key_warns_and_the_rest_of_the_file_still_reads() {
        let odd = config(
            "version = 99\ncxxflag = \"-O0\"\ncolour = true\n[grader.ce]\nurl = \"https://a\"\nlogon = \"me\"\n",
            &[],
        );
        assert!(odd.error().is_none());
        assert_eq!(odd.url(None).unwrap().value, "https://a");
        let said = odd.warnings().join("\n");
        assert!(said.contains("did you mean cxxflags?"), "{said}");
        assert!(said.contains("unknown key colour"), "{said}");
        assert!(said.contains("unknown key logon in [grader.ce]"), "{said}");
        assert!(said.contains("version 99"), "{said}");
    }

    #[test]
    fn a_value_of_the_wrong_shape_is_skipped_and_never_fatal() {
        let odd = config(
            "cxx = 7\n[grader.ce]\nurl = \"https://a\"\nlogin = 6001\n",
            &[],
        );
        assert!(odd.error().is_none());
        assert_eq!(odd.compiler(cpp()).unwrap().source, Source::Default);
        assert_eq!(odd.url(None).unwrap().value, "https://a");
        assert_eq!(odd.login(None), None);
    }

    // a name with a dot would silently have been a grader inside a grader
    #[test]
    fn a_grader_name_that_is_not_a_name_is_skipped_on_read_and_refused_on_write() {
        let dotted = config("[grader.\"a b\"]\nurl = \"https://a\"\n", &[]);
        assert!(dotted.grader_names().is_empty());
        assert!(dotted.warnings().iter().any(|w| w.contains("a b")));
        for bad in ["a.b", "a b", "", "a/b", "../x"] {
            assert!(!valid_name(bad), "{bad} is not a grader name");
        }
        for good in ["ce", "algo-2", "CS_101", "2110101"] {
            assert!(valid_name(good), "{good} is a grader name");
        }
    }

    #[test]
    fn a_file_that_is_not_toml_at_all_is_one_clear_error() {
        let broken = config("current = \n[grader\n", &[]);
        assert!(broken.error().is_some());
        assert!(broken.error().unwrap().contains("not valid toml"));
    }

    #[test]
    fn a_selection_naming_no_table_is_named_back() {
        let typo = config(TWO, &[("KAFAE_GRADER", "algoo")]);
        assert_eq!(typo.unknown_selection().as_deref(), Some("algoo"));
        assert_eq!(typo.grader_names(), vec!["algo", "ce"]);
        assert_eq!(config(TWO, &[]).unknown_selection(), None);
        // with no tables at all there is nothing to have mistyped
        assert_eq!(
            config("", &[("KAFAE_GRADER", "ce")]).unknown_selection(),
            None
        );
    }

    // the whole point of toml_edit: a student who switches courses must not lose the
    // comments that tell them what the file is
    #[test]
    fn switching_graders_keeps_every_comment_and_stays_valid_toml() {
        let commented = "# mine\nversion = 1\ncurrent = \"ce\"\n\n# the hard one\n[grader.algo]\nurl = \"https://a\"\n";
        let mut doc = commented
            .parse::<DocumentMut>()
            .expect("a hand-written file parses");
        put_current(&mut doc, "algo");
        let written = doc.to_string();
        assert!(written.contains("# mine"), "{written}");
        assert!(written.contains("# the hard one"), "{written}");
        assert!(written.contains("current = \"algo\""), "{written}");
        assert!(!written.contains("current = \"ce\""), "{written}");
        let (file, warnings) = parse(&written);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(file.current.as_deref(), Some("algo"));
    }

    // the template is the file every new user gets, so it has to be a file kafae reads
    // without a word of complaint
    #[test]
    fn the_embedded_template_parses_with_nothing_to_warn_about() {
        let (file, warnings) = parse(TEMPLATE);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(file.version, Some(VERSION));
        assert_eq!(file.current.as_deref(), Some("default"));
        // the machine block is commented, so a later change to kafae's defaults still
        // reaches the user
        assert_eq!(file.cxx, None);
        assert_eq!(file.cflags, None);
        assert!(file.grader.is_empty());
    }

    // a grader table written into a hand-edited file must come out as a table, not as a
    // key that TOML then reads as part of the last one
    #[test]
    fn a_remembered_grader_reads_back_as_a_grader() {
        let mut doc = TEMPLATE
            .parse::<DocumentMut>()
            .expect("the template parses");
        put_grader(
            &mut doc,
            "ce",
            Some("https://g.example/"),
            Some("6xxxxxxx21 "),
            true,
        );
        put_grader(
            &mut doc,
            "algo",
            Some("https://algo.example"),
            Some("6xxxxxxx21"),
            false,
        );
        let written = doc.to_string();
        let (file, warnings) = parse(&written);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(file.grader["ce"].url.as_deref(), Some("https://g.example"));
        assert_eq!(file.grader["ce"].login.as_deref(), Some("6xxxxxxx21"));
        assert_eq!(
            file.grader["algo"].url.as_deref(),
            Some("https://algo.example")
        );
        // [grader] is implicit: only the named tables are ever written
        assert!(!written.contains("\n[grader]"), "{written}");
        // the first login names current after the grader it wrote, because the template
        // ships "default" and nothing is configured under it; a second must not steal it
        assert_eq!(file.current.as_deref(), Some("ce"));
        // a new table is appended after the last root key, so nothing in the template may
        // sit below it or the comments end up under what they introduce
        let first = written.find("[grader.").expect("a table was written");
        assert!(
            written.find("# One table per grader").unwrap() < first,
            "{written}"
        );
        assert!(written.find("current =").unwrap() < first, "{written}");
    }

    // login hands a field over only when the student is the one who said it; the rest of
    // the table is theirs and a write must come out the other side untouched
    #[test]
    fn a_field_login_skips_keeps_whatever_the_file_already_says() {
        let mut doc = r#"
current = "ce"

[grader.ce]
url = "https://g.example"
login = "6xxxxxxx21"
cxxflags = "-O2 -std=c++20"
"#
        .parse::<DocumentMut>()
        .expect("the fixture parses");
        put_grader(&mut doc, "ce", None, Some("7xxxxxxx11"), false);
        let (file, warnings) = parse(&doc.to_string());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(file.grader["ce"].url.as_deref(), Some("https://g.example"));
        assert_eq!(file.grader["ce"].login.as_deref(), Some("7xxxxxxx11"));
        // a key kafae never writes is a key kafae never disturbs either
        assert_eq!(
            file.grader["ce"].cxxflags.as_deref(),
            Some("-O2 -std=c++20")
        );
    }

    // the config lives where KAFAE_HOME says, and a write that cannot happen is an Err a
    // command carries on past, never a failure
    #[test]
    fn a_write_creates_the_file_from_the_template_and_then_edits_it_in_place() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let path = home.path().join("config.toml");
        edit_at(&path, |doc| {
            put_grader(
                doc,
                "ce",
                Some("https://g.example"),
                Some("6xxxxxxx21"),
                true,
            )
        })
        .expect("a fresh config is written");
        assert!(fs::read_to_string(&path)
            .expect("it is there")
            .contains("# kafae configuration."));

        edit_at(&path, |doc| put_current(doc, "algo")).expect("and edited again");
        let (file, warnings) = parse(&fs::read_to_string(&path).expect("it is still there"));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(file.current.as_deref(), Some("algo"));
        assert_eq!(file.grader["ce"].url.as_deref(), Some("https://g.example"));
        // nothing is left behind by the atomic write
        assert_eq!(fs::read_dir(home.path()).unwrap().count(), 1);

        assert!(named("a.b").is_err());
        assert!(edit_at(home.path(), |_| {}).is_err());
    }

    // `kafae login --grader algo` on a machine with no config: the file kafae writes must
    // not be left naming a grader kafae never wrote, or every command after it fails on a
    // name the template invented
    #[test]
    fn a_first_login_names_current_after_the_grader_it_wrote() {
        let home = tempfile::tempdir().expect("a temporary directory");
        let path = home.path().join("config.toml");
        edit_at(&path, |doc| {
            put_grader(
                doc,
                "algo",
                Some("https://algo.example"),
                Some("6xxxxxxx21"),
                true,
            )
        })
        .expect("a fresh config is written");

        let fresh = config(&fs::read_to_string(&path).expect("it is there"), &[]);
        assert!(fresh.warnings().is_empty(), "{:?}", fresh.warnings());
        assert_eq!(fresh.selected().unwrap().value, "algo");
        assert_eq!(fresh.unknown_selection(), None);
        assert_eq!(fresh.url(None).unwrap().value, "https://algo.example");
    }

    // one variable moves config, sessions and cache together: moving one of the three
    // while the other two stay put is the partial isolation everyone trips over
    #[test]
    fn kafae_home_moves_every_path_or_none_of_them() {
        let home = Path::new("/tmp/kafae-test-home");
        let moved = paths_of(Some(home));
        for path in [
            &moved.config,
            &moved.templates,
            &moved.sessions,
            &moved.legacy_state,
            &moved.cache,
        ] {
            let path = path.as_deref().expect("a named home always has a path");
            assert!(path.starts_with(home), "{}", path.display());
        }
        assert_eq!(
            moved.config.as_deref(),
            Some(home.join("config.toml").as_path())
        );
        assert_eq!(
            moved.sessions.as_deref(),
            Some(home.join("sessions").as_path())
        );
        assert_eq!(moved.cache.as_deref(), Some(home.join("cache").as_path()));

        // unset, every one of them is where it has always been
        let usual = paths_of(None);
        for (moved, usual) in [
            (&moved.config, &usual.config),
            (&moved.sessions, &usual.sessions),
            (&moved.cache, &usual.cache),
        ] {
            assert_ne!(moved, usual);
        }
        if let Some(sessions) = &usual.sessions {
            assert!(sessions.ends_with("kafae/sessions") || cfg!(windows));
            assert_eq!(
                sessions.parent(),
                usual.legacy_state.as_deref().and_then(Path::parent)
            );
        }
    }
}
