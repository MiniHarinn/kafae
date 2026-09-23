use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{self, Config};
use crate::json;
use crate::language::{self, Build};
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

// both settings come from the same chain — KAFAE_CXX > the config file > the table's own
// default — so they are resolved against a Config rather than the process environment. A
// test hands in its own Config; only these two wrappers ever reach the loaded one.
fn compiler_in(config: &Config, file: &Path) -> Option<String> {
    Some(config.compiler(language::for_file(file)?)?.value)
}

fn flags_in(config: &Config, file: &Path) -> Vec<String> {
    config
        .flags(match language::for_file(file) {
            Some(local) => local,
            None => return Vec::new(),
        })
        .map(|flags| flags.value.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

// the compiler this file would go through, which doubles as "is this compiled at all"
pub fn compiler_for(file: &Path) -> Option<String> {
    compiler_in(config::load(), file)
}

// first candidate that is really there; the Windows Store ships a zero-byte python3.exe
// that only opens a shop window
pub fn interpreter(candidates: &[&str]) -> PathBuf {
    candidates
        .iter()
        .filter_map(|name| which::which(name).ok())
        .find(|path| path.metadata().is_ok_and(|meta| meta.len() > 0))
        .unwrap_or_else(|| {
            fail_as(
                "missing_tool",
                &format!(
                    "{} not on PATH",
                    candidates.first().unwrap_or(&"interpreter")
                ),
                None,
            )
        })
}

fn flags_for(file: &Path) -> Vec<String> {
    flags_in(config::load(), file)
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
    // unlike a script, silently skipping would read as "nothing to check" rather than
    // "not something this machine compiles"
    if let Some(Build::ServerOnly { why }) = language::build_of(file) {
        return Check::Skipped(Some(why.to_string()));
    }
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

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;
    use crate::config::Env;

    // hermetic: a Config built here reads neither the developer's config file nor their
    // shell, and the process environment stays out of the way of the other tests
    fn config_of(text: &str, env: &[(&str, &str)]) -> Config {
        let (file, warnings) = config::parse(text);
        assert!(warnings.is_empty(), "{warnings:?}");
        Config::new(file, Env::of(env), None)
    }

    // a .dig circuit is graded server-side; the local check must say so, not just go quiet
    #[test]
    fn a_server_only_language_skips_with_an_explicit_reason() {
        let outcome = compile_check(Path::new("01.dig"));
        assert!(matches!(outcome, Check::Skipped(Some(_))));
    }

    // a language kafae was never taught skips quietly: there is nothing to report
    #[test]
    fn an_unknown_language_skips_without_a_reason() {
        assert!(matches!(
            compile_check(Path::new("01.xyz")),
            Check::Skipped(None)
        ));
        assert!(compiler_for(Path::new("01.xyz")).is_none());
    }

    // through compiler_for/flags_for this would read KAFAE_CXX and friends and go red for
    // anyone whose shell sets them, and it would never see the env names the row carries:
    // swapping KAFAE_CC for KAFAE_CFLAGS would still pass. ask the table instead
    #[test]
    fn a_compiled_language_names_its_compiler_and_flags() {
        assert!(matches!(
            language::build_of(Path::new("a.cpp")),
            Some(Build::Compiler {
                env: "KAFAE_CXX",
                default: "g++",
                flags_env: "KAFAE_CXXFLAGS",
                flags,
            }) if flags.contains("-std=c++17")
        ));
        assert!(matches!(
            language::build_of(Path::new("a.c")),
            Some(Build::Compiler {
                env: "KAFAE_CC",
                default: "gcc",
                flags_env: "KAFAE_CFLAGS",
                flags,
            }) if flags.contains("-std=c99")
        ));
        // a script has no compiler flags to give
        assert!(flags_for(Path::new("a.py")).is_empty());
    }

    // the table alone cannot catch compiler_for reading flags_env, or flags_for reading
    // env: both would still compile and the defaults would still look right. Only the
    // override says which field each one reads, and an override is also immune to
    // whatever the contributor's shell already exports. One test, because the env is
    // process-global and the test threads run side by side
    #[test]
    fn each_compiler_setting_is_read_from_its_own_env_var() {
        env::set_var("KAFAE_CXX", "kafae-test-cxx");
        env::set_var("KAFAE_CXXFLAGS", "-std=kafae");
        env::set_var("KAFAE_CC", "kafae-test-cc");
        env::set_var("KAFAE_CFLAGS", "-std=kafae-c");
        assert_eq!(
            compiler_for(Path::new("a.cpp")).as_deref(),
            Some("kafae-test-cxx")
        );
        assert_eq!(
            flags_for(Path::new("a.cpp")),
            vec!["-std=kafae".to_string()]
        );
        assert_eq!(
            compiler_for(Path::new("a.c")).as_deref(),
            Some("kafae-test-cc")
        );
        assert_eq!(
            flags_for(Path::new("a.c")),
            vec!["-std=kafae-c".to_string()]
        );
        env::remove_var("KAFAE_CXX");
        env::remove_var("KAFAE_CXXFLAGS");
        env::remove_var("KAFAE_CC");
        env::remove_var("KAFAE_CFLAGS");
    }

    // the test above passes just as well against a compile.rs that calls env::var itself
    // and never asks config anything: it only ever sets variables. This one sets none, so
    // a bypassed config layer answers with the table's "g++" and goes red
    #[test]
    fn the_config_file_names_the_compiler_and_its_flags() {
        let config = config_of("cxx = \"config-cxx\"\ncxxflags = \"-std=config\"\n", &[]);
        assert_eq!(
            compiler_in(&config, Path::new("a.cpp")).as_deref(),
            Some("config-cxx")
        );
        assert_eq!(
            flags_in(&config, Path::new("a.cpp")),
            vec!["-std=config".to_string()]
        );
        // a key the file says nothing about keeps the table's default
        assert_eq!(
            compiler_in(&config, Path::new("a.c")).as_deref(),
            Some("gcc")
        );
        assert!(flags_in(&config, Path::new("a.c")).contains(&"-std=c99".to_string()));
        // and a script has neither a compiler nor compiler flags
        assert!(compiler_in(&config, Path::new("a.py")).is_none());
        assert!(flags_in(&config, Path::new("a.py")).is_empty());
    }

    // the file is the layer underneath the environment, never over it: an export is the
    // ad-hoc override for one command and it keeps outranking what is on disk
    #[test]
    fn a_variable_still_outranks_the_config_file() {
        let config = config_of(
            "cc = \"config-cc\"\ncflags = \"-std=config\"\n",
            &[("KAFAE_CC", "env-cc"), ("KAFAE_CFLAGS", "-std=env")],
        );
        assert_eq!(
            compiler_in(&config, Path::new("a.c")).as_deref(),
            Some("env-cc")
        );
        assert_eq!(
            flags_in(&config, Path::new("a.c")),
            vec!["-std=env".to_string()]
        );
    }

    // a course that grades at another -std says so in its own table; a compiler path is a
    // property of this machine and has no grader level to be overridden from
    #[test]
    fn the_selected_graders_flags_beat_the_machine_wide_ones() {
        let config = config_of(
            r#"
current = "algo"
cxx = "machine-cxx"
cxxflags = "-std=machine"

[grader.algo]
url = "https://algo.example"
cxxflags = "-std=course"
"#,
            &[],
        );
        assert_eq!(
            flags_in(&config, Path::new("a.cpp")),
            vec!["-std=course".to_string()]
        );
        assert_eq!(
            compiler_in(&config, Path::new("a.cpp")).as_deref(),
            Some("machine-cxx")
        );
    }
}
