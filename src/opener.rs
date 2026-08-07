use std::env;
use std::ffi::OsStr;
use std::path::{self, PathBuf};
use std::process::{Command, Stdio};

use crate::ui::{ebold, fail};

pub fn desktop() -> &'static str {
    if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        // not start: that's a cmd builtin, and parts() needs a real program on PATH
        "explorer"
    } else {
        "xdg-open"
    }
}

fn parts(command: &str) -> (PathBuf, Vec<&str>) {
    let mut words = command.split_whitespace();
    let program = words.next().unwrap_or_else(|| fail("no command to run"));
    let found =
        which::which(program).unwrap_or_else(|_| fail(&format!("{} not on PATH", ebold(program))));
    (found, words.collect())
}

// nothing waits for it, so it has to outlive us and keep off our stdio
pub fn detached(command: &str, target: &OsStr) {
    let (program, args) = parts(command);
    let mut viewer = Command::new(program);
    viewer
        .args(args)
        .arg(target)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        viewer.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        viewer.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let _ = viewer.spawn();
}

// in a nvim :terminal $NVIM is the nvim we are inside, so open there not nested
pub fn edit(paths: &[PathBuf]) {
    if let Some(server) = env::var_os("NVIM").filter(|server| !server.is_empty()) {
        let nvim = which::which("nvim")
            .unwrap_or_else(|_| fail(&format!("{} is set but nvim is not on PATH", ebold("NVIM"))));
        // that nvim resolves a relative path against its own cwd, which is not ours
        let files: Vec<PathBuf> = paths
            .iter()
            .map(|path| path::absolute(path).unwrap_or_else(|_| path.clone()))
            .collect();
        let status = Command::new(nvim)
            .arg("--server")
            .arg(&server)
            .arg("--remote")
            .args(&files)
            .status()
            .unwrap_or_else(|error| fail(&error.to_string()));
        if !status.success() {
            fail(&format!(
                "nvim would not open the file in {}",
                ebold(server.to_string_lossy())
            ));
        }
        return;
    }

    let editor = ["VISUAL", "EDITOR"]
        .iter()
        .find_map(|name| env::var(name).ok().filter(|value| !value.is_empty()))
        .unwrap_or_else(|| {
            fail(&format!(
                "no editor to open it with, set {} or {}",
                ebold("VISUAL"),
                ebold("EDITOR")
            ))
        });
    let (program, args) = parts(&editor);
    let status = Command::new(program)
        .args(args)
        .args(paths)
        .status()
        .unwrap_or_else(|error| fail(&error.to_string()));
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

// this one owns the terminal while it runs, so its exit status becomes ours
pub fn foreground(command: &str, target: &OsStr) -> ! {
    let (program, args) = parts(command);
    let status = Command::new(program)
        .args(args)
        .arg(target)
        .status()
        .unwrap_or_else(|error| fail(&error.to_string()));
    std::process::exit(status.code().unwrap_or(1));
}
