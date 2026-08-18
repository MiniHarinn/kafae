use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::json;
use crate::ui::{fail_as, panel};

pub enum CompileError {
    MissingCompiler(String),
    // what the compiler said, so --json can carry it instead of drawing a panel
    Failed(String),
}

// what a local compile check came to; --no-check and a script both skip it
pub enum Check {
    Skipped(Option<String>),
    Ok,
    Failed(String),
}

fn suffix(file: &Path) -> Option<&str> {
    file.extension().and_then(|ext| ext.to_str())
}

pub fn compiler_for(file: &Path) -> Option<String> {
    let (var, default) = match suffix(file) {
        Some("cpp" | "cc" | "cxx") => ("KAFAE_CXX", "g++"),
        Some("c") => ("KAFAE_CC", "gcc"),
        _ => return None,
    };
    Some(env::var(var).unwrap_or_else(|_| default.to_string()))
}

const PYTHONS: &[&str] = if cfg!(windows) {
    &["py", "python", "python3"]
} else {
    &["python3", "python"]
};

pub fn python() -> PathBuf {
    PYTHONS
        .iter()
        .filter_map(|name| which::which(name).ok())
        // skip the Store stub, a zero-byte python3.exe that just opens a shop window
        .find(|path| path.metadata().is_ok_and(|meta| meta.len() > 0))
        .unwrap_or_else(|| fail_as("missing_tool", &format!("{} not on PATH", PYTHONS[0]), None))
}

fn flags_for(file: &Path) -> Vec<String> {
    // -DLOCAL is not on the grader, so debug output can strip itself on submit
    let (var, default) = if suffix(file) == Some("c") {
        ("KAFAE_CFLAGS", "-O2 -std=c99 -DCONTEST -DLOCAL -lm -Wall")
    } else {
        (
            "KAFAE_CXXFLAGS",
            "-O2 -std=c++17 -DCONTEST -DLOCAL -lm -Wall",
        )
    };
    env::var(var)
        .unwrap_or_else(|_| default.to_string())
        .split_whitespace()
        .map(String::from)
        .collect()
}

pub fn compile_file(file: &Path, tmp: &Path, title: &str) -> Result<PathBuf, CompileError> {
    let compiler = compiler_for(file).unwrap();
    let Ok(cc) = which::which(&compiler) else {
        return Err(CompileError::MissingCompiler(compiler));
    };
    // Windows won't execute an extensionless binary
    let binary = tmp.join(if cfg!(windows) { "a.exe" } else { "a.out" });
    let output = Command::new(cc)
        .arg(file)
        .arg("-o")
        .arg(&binary)
        .args(flags_for(file))
        .output()
        .unwrap_or_else(|error| fail_as("io", &error.to_string(), None));
    let stderr = String::from_utf8_lossy(&output.stderr)
        .trim_end()
        .to_string();
    if !output.status.success() {
        if !json::on() {
            panel(true, &format!("{compiler}: {title}"), &stderr);
        }
        return Err(CompileError::Failed(stderr));
    }
    if !stderr.trim().is_empty() && !json::on() {
        eprintln!("{stderr}");
    }
    Ok(binary)
}

pub fn compile_check(file: &Path) -> Check {
    let Some(compiler) = compiler_for(file) else {
        return Check::Skipped(None);
    };
    if which::which(&compiler).is_err() {
        return Check::Skipped(Some(format!("{compiler} not on PATH")));
    }
    let tmp = tempfile::tempdir().unwrap_or_else(|error| fail_as("io", &error.to_string(), None));
    let result = compile_file(file, tmp.path(), "does not compile, not submitted");
    let _ = tmp.close();
    match result {
        Ok(_) => Check::Ok,
        Err(CompileError::MissingCompiler(compiler)) => {
            fail_as("missing_tool", &format!("{compiler} not on PATH"), None)
        }
        Err(CompileError::Failed(message)) => Check::Failed(message),
    }
}
