use std::path::Path;
use std::process::Command;

use crate::compile::{compile_file, interpreter, CompileError};
use crate::language::{self, Build};
use crate::ui::fail;

fn exit_with(mut command: Command) -> ! {
    let status = command
        .status()
        .unwrap_or_else(|error| fail(&error.to_string()));
    std::process::exit(status.code().unwrap_or(1));
}

pub fn run(file: &Path) {
    match language::build_of(file) {
        Some(Build::Compiler { .. }) => {
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
                Err(CompileError::Failed(_)) => {
                    let _ = tmp.close();
                    std::process::exit(1);
                }
            }
        }
        Some(Build::Interpreter { candidates }) => {
            let mut command = Command::new(interpreter(candidates));
            command.arg(file);
            exit_with(command);
        }
        Some(Build::ServerOnly { .. }) | None => fail(&language::no_runner(file)),
    }
}
