use std::path::Path;

use serde_json::Value;

// The grader names every submission type it accepts: GET /api/v1/languages lists them as
// {id, name, pretty_name, ext}, and a problem's permitted_languages is a subset of that
// list. That name is the join key, so what kafae stores here is only the other half: what
// it can do with a language on this machine. A new submission type is a row, not a branch
// in every harness.
pub struct Local {
    // the grader's own name for it, e.g. "cpp", "digital", "postgres"
    pub name: &'static str,
    // extensions that mean this language here; the first is the one kafae writes
    pub exts: &'static [&'static str],
    pub build: Build,
}

// how a file of this language becomes something runnable here, if it can at all
#[derive(Clone, Copy)]
pub enum Build {
    // a gcc-flavoured driver turns it into a binary
    Compiler {
        env: &'static str,
        default: &'static str,
        flags_env: &'static str,
        flags: &'static str,
    },
    // an interpreter off PATH runs it as it stands; first candidate that exists wins
    Interpreter {
        candidates: &'static [&'static str],
    },
    // only the grader can run it, and saying so is not the same as kafae never having
    // heard of the language
    ServerOnly {
        why: &'static str,
    },
}

const PYTHONS: &[&str] = if cfg!(windows) {
    &["py", "python", "python3"]
} else {
    &["python3", "python"]
};

// -DLOCAL is not on the grader, so debug output can strip itself on submit
const LOCALS: &[Local] = &[
    Local {
        name: "c",
        exts: &["c"],
        build: Build::Compiler {
            env: "KAFAE_CC",
            default: "gcc",
            flags_env: "KAFAE_CFLAGS",
            flags: "-O2 -std=c99 -DCONTEST -DLOCAL -lm -Wall",
        },
    },
    Local {
        name: "cpp",
        exts: &["cpp", "cc", "cxx"],
        build: Build::Compiler {
            env: "KAFAE_CXX",
            default: "g++",
            flags_env: "KAFAE_CXXFLAGS",
            flags: "-O2 -std=c++17 -DCONTEST -DLOCAL -lm -Wall",
        },
    },
    Local {
        name: "python",
        exts: &["py"],
        build: Build::Interpreter {
            candidates: PYTHONS,
        },
    },
    Local {
        name: "digital",
        exts: &["dig"],
        build: Build::ServerOnly {
            why: "digital circuits are graded on the server, not compiled locally",
        },
    },
];

pub fn suffix(file: &Path) -> Option<&str> {
    file.extension().and_then(|ext| ext.to_str())
}

pub fn for_ext(ext: &str) -> Option<&'static Local> {
    let ext = ext.to_lowercase();
    LOCALS
        .iter()
        .find(|local| local.exts.iter().any(|known| *known == ext))
}

pub fn for_file(file: &Path) -> Option<&'static Local> {
    for_ext(suffix(file)?)
}

pub fn build_of(file: &Path) -> Option<Build> {
    Some(for_file(file)?.build)
}

// a language kafae cannot run here is either one the grader keeps to itself or one kafae
// was never taught; the difference is worth saying out loud
pub fn no_runner(file: &Path) -> String {
    if let Some(Build::ServerOnly { why }) = build_of(file) {
        return why.to_string();
    }
    let suffix = suffix(file)
        .map(|ext| format!(".{ext}"))
        .unwrap_or_else(|| "extension-less".to_string());
    format!("don't know how to run a {suffix} file")
}

// The grader lists what a problem takes as {id, name, ext}. A file fits if the grader
// spells its extension that way, or if kafae knows the two spellings to be one language:
// a .cc is the grader's "cpp" even though the grader writes that extension "cpp". Nothing
// listed means the grader has no opinion and everything is allowed.
pub fn accepts_ext(prob: &Value, ext: &str) -> bool {
    let Some(permitted) = prob["permitted_languages"].as_array() else {
        return true;
    };
    if permitted.is_empty() {
        return true;
    }
    let mine = for_ext(ext).map(|local| local.name);
    permitted.iter().any(|lang| {
        lang["ext"]
            .as_str()
            .is_some_and(|known| known.eq_ignore_ascii_case(ext))
            || (mine.is_some() && lang["name"].as_str() == mine)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_extension_finds_the_language_the_grader_would_name() {
        assert_eq!(for_ext("cpp").unwrap().name, "cpp");
        assert_eq!(for_ext("cc").unwrap().name, "cpp");
        assert_eq!(for_ext("CPP").unwrap().name, "cpp");
        assert_eq!(for_ext("c").unwrap().name, "c");
        assert_eq!(for_ext("py").unwrap().name, "python");
        assert_eq!(for_ext("dig").unwrap().name, "digital");
        assert!(for_ext("sql").is_none());
        assert!(for_ext("").is_none());
    }

    // the grader grades a circuit; kafae has simply never met a .sql. Both refuse to run,
    // and a reader of the message should be able to tell which is which
    #[test]
    fn says_which_kind_of_cannot_run_this_is() {
        let circuit = no_runner(Path::new("01.dig"));
        assert!(circuit.contains("graded on the server"), "{circuit}");
        let query = no_runner(Path::new("01.sql"));
        assert!(query.contains("don't know how to run"), "{query}");
        assert!(no_runner(Path::new("README")).contains("extension-less"));
    }

    // every language kafae claims locally must be one the grader can name, or the join
    // key is a lie; these are the names from GET /api/v1/languages
    #[test]
    fn every_local_language_is_one_the_grader_knows() {
        let grader = [
            "c", "cpp", "python", "digital", "postgres", "text", "archive",
        ];
        for local in LOCALS {
            assert!(
                grader.contains(&local.name),
                "{} is not a grader language",
                local.name
            );
            assert!(!local.exts.is_empty(), "{} claims no extension", local.name);
        }
    }

    // the grader writes C++ as ext "cpp", so a .cc would be refused by a plain string
    // compare even though the grader and kafae agree it is the same language
    #[test]
    fn another_spelling_of_the_same_language_is_still_that_language() {
        let cpp = json!({"permitted_languages": [{"id": 2, "name": "cpp", "ext": "cpp"}]});
        for spelling in ["cpp", "cc", "cxx", "CC"] {
            assert!(accepts_ext(&cpp, spelling), "{spelling} is c++");
        }
        assert!(!accepts_ext(&cpp, "py"));
        assert!(!accepts_ext(&cpp, "dig"));
    }

    // a problem that lists nothing takes anything; that is the common case on this grader
    #[test]
    fn a_problem_with_no_opinion_takes_anything() {
        for quiet in [
            json!({}),
            json!({"permitted_languages": null}),
            json!({"permitted_languages": []}),
        ] {
            assert!(accepts_ext(&quiet, "rs"), "{quiet}");
            assert!(accepts_ext(&quiet, "dig"));
        }
    }

    // a language kafae has never heard of still matches on the grader's own spelling
    #[test]
    fn an_unknown_language_matches_on_the_extension_the_grader_gave() {
        let sql = json!({"permitted_languages": [{"id": 12, "name": "postgres", "ext": "sql"}]});
        assert!(accepts_ext(&sql, "sql"));
        assert!(!accepts_ext(&sql, "cpp"));
    }
}
