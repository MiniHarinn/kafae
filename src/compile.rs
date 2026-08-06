use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use console::style;

use crate::ui::{dim, fail, panel};

pub enum CompileError {
    MissingCompiler(String),
    Failed,
}

fn suffix(file: &Path) -> Option<&str> {
    file.extension().and_then(|ext| ext.to_str())
}

pub fn compiler_for(file: &Path) -> Option<&'static str> {
    match suffix(file) {
        Some("cpp" | "cc" | "cxx") => Some("g++"),
        Some("c") => Some("gcc"),
        _ => None,
    }
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
        .unwrap_or_else(|| fail(&format!("{} not on PATH", PYTHONS[0])))
}

fn flags_for(file: &Path) -> Vec<String> {
    // -DLOCAL is not on the grader, so debug output can strip itself on submit
    let (var, default) = if suffix(file) == Some("c") {
        ("KAFAE_CFLAGS", "-O2 -std=c99 -DCONTEST -DLOCAL -lm -Wall")
    } else {
        ("KAFAE_CXXFLAGS", "-O2 -std=c++17 -DCONTEST -DLOCAL -lm -Wall")
    };
    env::var(var)
        .unwrap_or_else(|_| default.to_string())
        .split_whitespace()
        .map(String::from)
        .collect()
}

pub fn compile_file(file: &Path, tmp: &Path, title: &str) -> Result<PathBuf, CompileError> {
    let compiler = compiler_for(file).unwrap();
    let Ok(cc) = which::which(compiler) else {
        return Err(CompileError::MissingCompiler(compiler.to_string()));
    };
    // Windows won't execute an extensionless binary
    let binary = tmp.join(if cfg!(windows) { "a.exe" } else { "a.out" });
    let output = Command::new(cc)
        .arg(file)
        .arg("-o")
        .arg(&binary)
        .args(flags_for(file))
        .output()
        .unwrap_or_else(|error| fail(&error.to_string()));
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        panel(true, &format!("{compiler}: {title}"), stderr.trim_end());
        return Err(CompileError::Failed);
    }
    if !stderr.trim().is_empty() {
        eprintln!("{}", stderr.trim_end());
    }
    Ok(binary)
}

pub fn compile_check(file: &Path) {
    let Some(compiler) = compiler_for(file) else {
        return;
    };
    if which::which(compiler).is_err() {
        eprintln!(
            "{}",
            style(format!("compile check skipped: {compiler} not on PATH"))
                .for_stderr()
                .yellow()
        );
        return;
    }
    let tmp = tempfile::tempdir().unwrap_or_else(|error| fail(&error.to_string()));
    let result = compile_file(file, tmp.path(), "does not compile, not submitted");
    let _ = tmp.close();
    match result {
        Ok(_) => println!("{}", dim("compile check ok")),
        Err(CompileError::MissingCompiler(compiler)) => fail(&format!("{compiler} not on PATH")),
        Err(CompileError::Failed) => std::process::exit(1),
    }
}
