use std::ffi::OsStr;
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

fn parts(command: &str) -> (std::path::PathBuf, Vec<&str>) {
    let mut words = command.split_whitespace();
    let program = words
        .next()
        .unwrap_or_else(|| fail(&format!("{} needs a command", ebold("--open-with"))));
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
