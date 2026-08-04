use std::fs;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::client::{api, api_bytes, authed_state, resolve_problem, statements_dir};
use crate::ui::{dim, ebold, fail};

fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        _ => false,
    }
}

pub fn run(problem: &str, text: bool, pdf_tui: bool, no_open: bool) {
    let state = authed_state();
    let prob = resolve_problem(&state, problem);
    let name = prob["name"].as_str().unwrap_or("");
    let mut shown = false;

    if !text {
        if let Some(pdf) = api_bytes(&state, &format!("problems/{}/files/pdf", prob["id"])) {
            let dir = statements_dir();
            fs::create_dir_all(&dir).unwrap_or_else(|error| fail(&error.to_string()));
            let path = dir.join(format!("{name}.pdf"));
            fs::write(&path, pdf).unwrap_or_else(|error| fail(&error.to_string()));
            if pdf_tui {
                let tdf = which::which("tdf").unwrap_or_else(|_| fail("tdf not on PATH"));
                let status = Command::new(tdf)
                    .arg(&path)
                    .status()
                    .unwrap_or_else(|error| fail(&error.to_string()));
                std::process::exit(status.code().unwrap_or(1));
            }
            let opener = if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };
            if !no_open {
                if let Ok(opener) = which::which(opener) {
                    let mut viewer = Command::new(opener);
                    viewer
                        .arg(&path)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());
                    #[cfg(unix)]
                    {
                        use std::os::unix::process::CommandExt;
                        viewer.process_group(0);
                    }
                    let _ = viewer.spawn();
                }
            }
            println!("{} {}", dim("pdf:"), path.display());
            shown = true;
        } else if pdf_tui {
            fail(&format!("problem {} has no PDF statement", ebold(name)));
        }
    }

    let desc = api(
        &state,
        minreq::Method::Get,
        &format!("problems/{}/description", prob["id"]),
        None,
    );
    if let Some(body) = desc["description"].as_str().filter(|s| !s.is_empty()) {
        if truthy(&desc["markdown"]) {
            termimad::print_text(body);
        } else {
            println!("{body}");
        }
        shown = true;
    }

    if !shown {
        fail(&format!("problem {} has no statement", ebold(name)));
    }
}
