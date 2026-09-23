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
    // one template cannot serve both: "a .py file" reads right, "a extension-less file"
    // does not
    match suffix(file) {
        Some(ext) => format!("don't know how to run a .{ext} file"),
        None => "don't know how to run a file with no extension".to_string(),
    }
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

// what to call each permitted language in a refusal. The grader's own ext is the useful
// half when it carries one, but an entry with only a name is still a language this
// problem takes, and a list that quietly drops it claims more than the code can know.
fn permitted_names(prob: &Value) -> Vec<String> {
    let Some(permitted) = prob["permitted_languages"].as_array() else {
        return Vec::new();
    };
    permitted
        .iter()
        .filter_map(|lang| {
            let named = |key| lang[key].as_str().filter(|value| !value.is_empty());
            match named("ext") {
                Some(ext) => Some(format!(".{ext}")),
                None => named("pretty_name")
                    .or_else(|| named("name"))
                    .map(String::from),
            }
        })
        .collect()
}

// the grader lists the languages it will take when it has an opinion, and a file in any
// other language is a submission spent on a refusal. The list has to be built from the
// same fields accepts_ext honours, or the sentence argues with the decision above it
pub fn unusable(prob: &Value, ext: &str) -> Option<String> {
    if accepts_ext(prob, ext) {
        return None;
    }
    let names = permitted_names(prob).join(" or ");
    // "not ." reads as a typo; and nothing nameable to list is better said without a list
    Some(match (names.is_empty(), ext.is_empty()) {
        (false, false) => format!("takes only {names}, not .{ext}"),
        (false, true) => format!("takes only {names}; this file has no extension"),
        (true, false) => format!("does not take .{ext}"),
        (true, true) => "does not take a file with no extension".to_string(),
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
        assert!(for_ext("xyz").is_none());
        assert!(for_ext("").is_none());
    }

    // .xyz is the stand-in for a language kafae has never heard of, so it must stay one
    // no row can ever claim: .sql played the part until postgres was announced, and a
    // table row that is meant to be the whole change would have turned this red instead
    #[test]
    fn the_unknown_language_fixture_belongs_to_no_row() {
        for local in LOCALS {
            assert!(
                !local.exts.contains(&"xyz"),
                "{} claims the unknown-language fixture",
                local.name
            );
        }
    }

    // the grader grades a circuit; kafae has simply never met a .xyz. Both refuse to run,
    // and a reader of the message should be able to tell which is which
    #[test]
    fn says_which_kind_of_cannot_run_this_is() {
        let circuit = no_runner(Path::new("01.dig"));
        assert!(circuit.contains("graded on the server"), "{circuit}");
        let query = no_runner(Path::new("01.xyz"));
        assert!(query.contains("don't know how to run"), "{query}");
        assert!(no_runner(Path::new("README")).contains("a file with no extension"));
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
        let unknown = json!({"permitted_languages": [{"id": 99, "name": "xyz", "ext": "xyz"}]});
        assert!(accepts_ext(&unknown, "xyz"));
        assert!(!accepts_ext(&unknown, "cpp"));
    }

    #[test]
    fn refuses_a_language_the_problem_does_not_take() {
        let prob = json!({"permitted_languages": [{"ext": "dig", "name": "digital"}]});
        assert_eq!(
            unusable(&prob, "cpp"),
            Some("takes only .dig, not .cpp".to_string())
        );
        assert_eq!(unusable(&prob, "dig"), None);
    }

    #[test]
    fn lists_every_language_the_problem_does_take() {
        let prob = json!({
            "permitted_languages": [{"ext": "c"}, {"ext": "cpp"}, {"ext": "py"}]
        });
        assert_eq!(
            unusable(&prob, "dig"),
            Some("takes only .c or .cpp or .py, not .dig".to_string())
        );
    }

    // the grader's spelling is its own, and .DIG is the same language as .dig
    #[test]
    fn matches_the_extension_whatever_its_case() {
        let prob = json!({"permitted_languages": [{"ext": "DIG"}]});
        assert_eq!(unusable(&prob, "dig"), None);
    }

    // no listed language is the grader having no opinion, not it refusing everything
    #[test]
    fn allows_anything_when_the_problem_lists_nothing() {
        assert_eq!(unusable(&json!({}), "cpp"), None);
        assert_eq!(unusable(&json!({"permitted_languages": []}), "cpp"), None);
    }

    // the grader need not spell out an ext, and a language named but not listed would
    // leave "takes only , not .rs" behind
    #[test]
    fn names_a_permitted_language_the_grader_gave_no_extension_for() {
        let prob = json!({"permitted_languages": [
            {"id": 4, "name": "digital", "pretty_name": "Digital"},
            {"id": 2, "name": "cpp", "ext": "cpp"},
        ]});
        assert_eq!(
            unusable(&prob, "rs"),
            Some("takes only Digital or .cpp, not .rs".to_string())
        );
        assert_eq!(unusable(&prob, "dig"), None);
    }

    // nothing to name is not the same as an empty list; say what is refused instead
    #[test]
    fn drops_the_list_when_no_permitted_language_can_be_named() {
        let prob = json!({"permitted_languages": [{"id": 4}]});
        assert_eq!(unusable(&prob, "rs"), Some("does not take .rs".to_string()));
        assert_eq!(
            unusable(&prob, ""),
            Some("does not take a file with no extension".to_string())
        );
    }

    // a file with no suffix has no extension to quote back, the way no_runner already says
    #[test]
    fn says_a_file_has_no_extension_rather_than_quoting_an_empty_one() {
        let prob = json!({"permitted_languages": [{"ext": "dig", "name": "digital"}]});
        assert_eq!(
            unusable(&prob, ""),
            Some("takes only .dig; this file has no extension".to_string())
        );
    }
}
