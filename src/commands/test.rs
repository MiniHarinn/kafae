use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use console::{style, Term};

use crate::client::{
    api, api_bytes, authed_state, cached_problem_name, resolve_problem, tests_dir,
};
use crate::compile::{compile_file, compiler_for, python, CompileError};
use crate::ui::{dim, ebold, edim, fail, fmt_runtime, mark_style};

// a hang guard, not the grader's limit
const TIME_LIMIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(250);

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
    let dir = tests_dir(name);
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

// None means it did not build, which the next save may fix
fn prepare(file: &Path, tmp: &Path) -> Option<Runner> {
    if compiler_for(file).is_some() {
        match compile_file(file, tmp, "does not compile") {
            Ok(binary) => Some(Runner::Binary(binary)),
            Err(CompileError::MissingCompiler(compiler)) => {
                fail(&format!("{compiler} not on PATH"))
            }
            Err(CompileError::Failed) => None,
        }
    } else if file.extension().and_then(|ext| ext.to_str()) == Some("py") {
        Some(Runner::Script(python(), file.to_path_buf()))
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

const INPUT_LINES: usize = 5;

fn show_input(case: &Case) {
    let Ok(raw) = fs::read(&case.input) else {
        return;
    };
    let text = String::from_utf8_lossy(&raw);
    let mut lines = text.lines();
    for (index, line) in lines.by_ref().take(INPUT_LINES).enumerate() {
        let label = if index == 0 { "input   " } else { "        " };
        println!("      {}", dim(format!("{label}  {}", clip(line))));
    }
    if lines.next().is_some() {
        println!("      {}", dim("          …"));
    }
}

pub fn case_names(reference: &str) -> Vec<String> {
    cases_in(&tests_dir(reference))
        .into_iter()
        .map(|case| case.name)
        .collect()
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

// HFS+ mtime granularity is a second, so size catches a same-second save
fn stamp(file: &Path) -> Option<(SystemTime, u64)> {
    let meta = fs::metadata(file).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

// an editor saving by rename briefly unlinks the file, so only a readable stamp counts
fn await_change(file: &Path) {
    let before = stamp(file);
    loop {
        thread::sleep(POLL);
        let now = stamp(file);
        if now.is_some() && now != before {
            return;
        }
    }
}

pub fn run(file: &Path, problem: Option<&str>, wanted: &[String], watch: bool) {
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let reference = problem.unwrap_or(stem);
    // an id names no directory, so map it through the cached list before looking
    let name = cached_problem_name(reference).unwrap_or_else(|| reference.to_string());

    let mut cases = cases_in(&tests_dir(&name));
    if cases.is_empty() {
        cases = cases_in(&fetch_cases(reference));
    }
    if cases.is_empty() {
        fail(&format!("no testcases for {}", ebold(reference)));
    }
    only_cases(&mut cases, wanted);

    if !watch {
        std::process::exit(if attempt(file, &cases) { 0 } else { 1 });
    }
    let term = Term::stdout();
    loop {
        let _ = term.clear_screen();
        println!(
            "{}",
            dim(format!("watching {} · ctrl-c to stop", file.display()))
        );
        attempt(file, &cases);
        await_change(file);
    }
}

fn attempt(file: &Path, cases: &[Case]) -> bool {
    let tmp = tempfile::tempdir().unwrap_or_else(|error| fail(&error.to_string()));
    let Some(runner) = prepare(file, tmp.path()) else {
        return false;
    };

    let plural = |n: usize| if n == 1 { "test" } else { "tests" };
    println!(
        "{}",
        dim(format!("{} {}", cases.len(), plural(cases.len())))
    );
    let mut results = Vec::new();
    for case in cases {
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
        show_input(case);
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
        return true;
    }
    println!(
        "{}",
        style(format!("✗ {} of {total} tests failed", failures.len()))
            .red()
            .bold()
    );
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(name: &str) -> Case {
        Case {
            name: name.to_string(),
            input: PathBuf::from(format!("{name}.in")),
            answer: PathBuf::from(format!("{name}.sol")),
        }
    }

    #[test]
    fn keeps_only_the_cases_asked_for() {
        let mut cases = vec![case("1"), case("2"), case("10")];
        only_cases(&mut cases, &["10".to_string(), "1".to_string()]);
        let names: Vec<&str> = cases.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["1", "10"]);
    }

    #[test]
    fn no_case_asked_for_means_all_of_them() {
        let mut cases = vec![case("1"), case("2")];
        only_cases(&mut cases, &[]);
        assert_eq!(cases.len(), 2);
    }

    #[test]
    fn ignores_trailing_whitespace_and_blank_lines() {
        assert_eq!(normalize("a  \nb\t\n\n\n"), ["a", "b"]);
        assert_eq!(normalize(""), Vec::<String>::new());
        assert!(matches!(diff("1\n2\n\n", "1\n2"), Outcome::Pass));
    }

    #[test]
    fn names_the_first_line_that_differs() {
        let Outcome::Wrong {
            line,
            expected,
            got,
        } = diff("1\n3\n", "1\n2\n")
        else {
            panic!("expected a wrong answer");
        };
        assert_eq!((line, expected.as_str(), got.as_str()), (2, "2", "3"));
    }

    #[test]
    fn short_output_counts_as_wrong_rather_than_equal() {
        let Outcome::Wrong { line, got, .. } = diff("1\n", "1\n2\n") else {
            panic!("expected a wrong answer");
        };
        assert_eq!((line, got.as_str()), (2, "<nothing>"));
    }

    #[test]
    fn clips_a_long_line() {
        assert_eq!(clip("short"), "short");
        assert_eq!(clip(&"x".repeat(60)), "x".repeat(60));
        assert_eq!(clip(&"x".repeat(61)), format!("{}…", "x".repeat(60)));
    }
}
