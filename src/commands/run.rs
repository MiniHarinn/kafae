use std::path::Path;
use std::process::Command;

use crate::compile::{compile_file, compiler_for, CompileError};
use crate::ui::fail;

fn exit_with(mut command: Command) -> ! {
    let status = command
        .status()
        .unwrap_or_else(|error| fail(&error.to_string()));
    std::process::exit(status.code().unwrap_or(1));
}

pub fn run(file: &Path) {
    if compiler_for(file).is_some() {
        let tmp = tempfile::tempdir().unwrap_or_else(|error| fail(&error.to_string()));
        match compile_file(file, tmp.path(), "does not compile") {
            Ok(binary) => {
                let status = Command::new(binary)
                    .status()
                    .unwrap_or_else(|error| fail(&error.to_string()));
                let code = status.code().unwrap_or(1);
                let _ = tmp.close();
                std::process::exit(code);
            }
            Err(CompileError::MissingCompiler(compiler)) => {
                let _ = tmp.close();
                fail(&format!("{compiler} not on PATH"));
            }
            Err(CompileError::Failed) => {
                let _ = tmp.close();
                std::process::exit(1);
            }
        }
    } else if file.extension().and_then(|ext| ext.to_str()) == Some("py") {
        let python = which::which("python3").unwrap_or_else(|_| fail("python3 not on PATH"));
        let mut command = Command::new(python);
        command.arg(file);
        exit_with(command);
    } else {
        let suffix = file
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!(".{ext}"))
            .unwrap_or_else(|| "extension-less".to_string());
        fail(&format!("don't know how to run a {suffix} file"));
    }
}
