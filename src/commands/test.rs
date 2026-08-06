use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use console::style;

use crate::client::{api, api_bytes, authed_state, cache_dir, resolve_problem};
use crate::compile::{compile_file, compiler_for, python, CompileError};
use crate::ui::{dim, ebold, edim, fail, fmt_runtime, mark_style};

// a hang guard, not the grader's limit
const TIME_LIMIT: Duration = Duration::from_secs(10);

struct Case {
    name: String,
    input: PathBuf,
    answer: PathBuf,
}

enum Outcome {
    Pass,
    Wrong {
        line: usize,
        expected: String,
        got: String,
    },
    Timeout,
    Crash {
        code: Option<i32>,
        stderr: String,
    },
}

impl Outcome {
    fn code(&self) -> char {
        match self {
            Outcome::Pass => 'P',
            Outcome::Wrong { .. } => '-',
            Outcome::Timeout => 'T',
            Outcome::Crash { .. } => 'x',
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Outcome::Pass => "correct",
            Outcome::Wrong { .. } => "wrong answer",
            Outcome::Timeout => "time limit",
            Outcome::Crash { .. } => "runtime error",
        }
    }
}

fn cases_in(dir: &Path) -> Vec<Case> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut cases: Vec<Case> = entries
        .flatten()
        .filter_map(|entry| {
            let input = entry.path();
            if input.extension().and_then(|ext| ext.to_str()) != Some("in") {
                return None;
            }
            let name = input.file_stem()?.to_str()?.to_string();
            let answer = input.with_extension("sol");
            answer.is_file().then_some(Case {
                name,
                input,
                answer,
            })
        })
        .collect();
    // numeric order where names allow, so 10 comes after 2
    cases.sort_by_key(|c| (c.name.parse::<u64>().unwrap_or(u64::MAX), c.name.clone()));
    cases
}

fn fetch_cases(reference: &str) -> PathBuf {
    let state = authed_state();
    let prob = resolve_problem(&state, reference);
    let name = prob["name"].as_str().unwrap_or(reference);
    let dir = cache_dir().join("tests").join(name);
    // the reference may have been an id for a name we already cached
    if !cases_in(&dir).is_empty() {
        return dir;
    }
    if prob["has_testcase"].as_bool() != Some(true) {
        fail(&format!(
            "the grader does not share testcases for {}",
            ebold(name)
        ));
    }
    let list = api(
        &state,
        minreq::Method::Get,
        &format!("problems/{}/testcases", prob["id"]),
        None,
    );
    let list = list.as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        fail(&format!("the grader has no testcases for {}", ebold(name)));
    }
    eprintln!(
        "{}",
        edim(format!(
            "fetching {} testcases from the grader…",
            list.len()
        ))
    );
    fs::create_dir_all(&dir).unwrap_or_else(|error| fail(&error.to_string()));
    for tc in &list {
        let (Some(id), Some(num)) = (tc["id"].as_i64(), tc["num"].as_i64()) else {
            continue;
        };
        let Some(input) = api_bytes(&state, &format!("testcases/{id}/input")) else {
            continue;
        };
        let Some(sol) = api_bytes(&state, &format!("testcases/{id}/sol")) else {
            continue;
        };
        fs::write(dir.join(format!("{num}.in")), input)
            .unwrap_or_else(|error| fail(&error.to_string()));
        fs::write(dir.join(format!("{num}.sol")), sol)
            .unwrap_or_else(|error| fail(&error.to_string()));
    }
    dir
}

enum Runner {
    Binary(PathBuf),
    Script(PathBuf, PathBuf),
}

impl Runner {
    fn command(&self) -> Command {
        match self {
            Runner::Binary(binary) => Command::new(binary),
            Runner::Script(interpreter, file) => {
                let mut command = Command::new(interpreter);
                command.arg(file);
                command
            }
        }
    }
}

fn prepare(file: &Path, tmp: &Path) -> Runner {
    if compiler_for(file).is_some() {
        match compile_file(file, tmp, "does not compile") {
            Ok(binary) => Runner::Binary(binary),
            Err(CompileError::MissingCompiler(compiler)) => {
                fail(&format!("{compiler} not on PATH"))
            }
            Err(CompileError::Failed) => std::process::exit(1),
        }
    } else if file.extension().and_then(|ext| ext.to_str()) == Some("py") {
        Runner::Script(python(), file.to_path_buf())
    } else {
        let suffix = file
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!(".{ext}"))
            .unwrap_or_else(|| "extension-less".to_string());
        fail(&format!("don't know how to run a {suffix} file"));
    }
}

fn wait_within(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(error) => fail(&error.to_string()),
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(Duration::from_millis(2));
    }
}

// the grader compares whole lines; ignore trailing whitespace and a blank tail
fn normalize(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| line.trim_end().to_string())
        .collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

fn diff(got: &str, want: &str) -> Outcome {
    let got = normalize(got);
    let want = normalize(want);
    for index in 0..got.len().max(want.len()) {
        if got.get(index) != want.get(index) {
            let missing = || "<nothing>".to_string();
            return Outcome::Wrong {
                line: index + 1,
                expected: want.get(index).cloned().unwrap_or_else(missing),
                got: got.get(index).cloned().unwrap_or_else(missing),
            };
        }
    }
    Outcome::Pass
}

// a Windows crash comes back as an NTSTATUS exit code, which says nothing raw
fn exit_note(code: i32) -> String {
    #[cfg(windows)]
    {
        let reason = match code as u32 {
            0xC000_0005 => Some("access violation"),
            0xC000_001D => Some("illegal instruction"),
            0xC000_0094 => Some("integer divide by zero"),
            0xC000_008C => Some("array bounds exceeded"),
            0xC000_00FD => Some("stack overflow"),
            0xC000_0374 => Some("heap corruption"),
            _ => None,
        };
        if let Some(reason) = reason {
            return format!("{reason} (0x{:08X})", code as u32);
        }
    }
    format!("exit code {code}")
}

// wall time here is approximate; whole milliseconds are honest enough
fn fmt_elapsed(time: Duration) -> String {
    fmt_runtime((time.as_secs_f64() * 1000.0).round())
}

fn clip(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(60).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

fn run_case(runner: &Runner, case: &Case) -> (Outcome, Duration) {
    let input = fs::read(&case.input).unwrap_or_else(|error| fail(&error.to_string()));
    let answer = fs::read(&case.answer).unwrap_or_else(|error| fail(&error.to_string()));
    let answer = String::from_utf8_lossy(&answer).into_owned();

    let mut command = runner.command();
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let start = Instant::now();
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| fail(&error.to_string()));

    // feed and drain on threads so a chatty program can't deadlock the pipes
    let mut stdin = child.stdin.take().unwrap();
    let writer = thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut out_pipe = child.stdout.take().unwrap();
    let out_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let mut err_pipe = child.stderr.take().unwrap();
    let err_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });

    let status = wait_within(&mut child, TIME_LIMIT);
    let elapsed = start.elapsed();
    let _ = writer.join();
    let out = out_reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();

    let outcome = match status {
        None => Outcome::Timeout,
        Some(status) if !status.success() => Outcome::Crash {
            code: status.code(),
            stderr: String::from_utf8_lossy(&err).trim_end().to_string(),
        },
        Some(_) => diff(&String::from_utf8_lossy(&out), &answer),
    };
    (outcome, elapsed)
}

fn only_cases(cases: &mut Vec<Case>, wanted: &[String]) {
    if wanted.is_empty() {
        return;
    }
    for name in wanted {
        if !cases.iter().any(|case| &case.name == name) {
            fail(&format!(
                "no testcase {} here; this problem has {}",
                ebold(name),
                cases
                    .iter()
                    .map(|case| case.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    cases.retain(|case| wanted.contains(&case.name));
}

pub fn run(file: &Path, problem: Option<&str>, wanted: &[String]) {
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let reference = problem.unwrap_or(stem);

    let mut cases = cases_in(&cache_dir().join("tests").join(reference));
    if cases.is_empty() {
        cases = cases_in(&fetch_cases(reference));
    }
    if cases.is_empty() {
        fail(&format!("no testcases for {}", ebold(reference)));
    }
    only_cases(&mut cases, wanted);

    let tmp = tempfile::tempdir().unwrap_or_else(|error| fail(&error.to_string()));
    let runner = prepare(file, tmp.path());

    let plural = |n: usize| if n == 1 { "test" } else { "tests" };
    println!(
        "{}",
        dim(format!("{} {}", cases.len(), plural(cases.len())))
    );
    let mut results = Vec::new();
    for case in &cases {
        let (outcome, time) = run_case(&runner, case);
        let code = outcome.code();
        print!("{}", mark_style(code).apply_to(code));
        let _ = std::io::stdout().flush();
        results.push((outcome, time));
    }
    println!();

    let failures: Vec<(&Case, &Outcome, Duration)> = cases
        .iter()
        .zip(&results)
        .filter(|(_, (outcome, _))| !matches!(outcome, Outcome::Pass))
        .map(|(case, (outcome, time))| (case, outcome, *time))
        .collect();
    let label_width = failures
        .iter()
        .map(|(_, o, _)| o.label().len())
        .max()
        .unwrap_or(0);
    let name_width = failures
        .iter()
        .map(|(c, _, _)| c.name.len())
        .max()
        .unwrap_or(0);
    for (case, outcome, time) in &failures {
        println!(
            "  {}  {}  {}",
            dim(format!("#{:<name_width$}", case.name)),
            mark_style(outcome.code()).apply_to(format!("{:<label_width$}", outcome.label())),
            dim(format!("{:>7}", fmt_elapsed(*time))),
        );
        match outcome {
            Outcome::Wrong {
                line,
                expected,
                got,
            } => {
                let at = if *line > 1 {
                    format!(" (line {line})")
                } else {
                    String::new()
                };
                println!("      {}", dim(format!("expected  {}{at}", clip(expected))));
                println!("      {}", dim(format!("got       {}", clip(got))));
            }
            Outcome::Crash { code, stderr } => {
                for line in stderr.lines().take(3) {
                    println!("      {}", dim(line));
                }
                if stderr.is_empty() {
                    if let Some(code) = code {
                        println!("      {}", dim(exit_note(*code)));
                    }
                }
            }
            _ => {}
        }
    }

    let total = results.len();
    let passed = total - failures.len();
    if passed == total {
        let slowest = results.iter().map(|(_, t)| *t).max().unwrap_or_default();
        println!(
            "{}  {}",
            style(format!("✓ {total} {} passed", plural(total)))
                .green()
                .bold(),
            dim(fmt_elapsed(slowest)),
        );
        std::process::exit(0);
    }
    println!(
        "{}",
        style(format!("✗ {} of {total} tests failed", failures.len()))
            .red()
            .bold()
    );
    std::process::exit(1);
}
