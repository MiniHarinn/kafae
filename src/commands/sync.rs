use bytesize::ByteSize;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::Value;

use crate::client::{
    api, api_bytes, authed_state, cache_write, detail_file, get_problems, statement_file,
    tree_size, State,
};
use crate::commands::problems::{select, Filter};
use crate::commands::test::{case_names, fetch_testcases};
use crate::offline;
use crate::ui::{bold, dim, edim, fail};

// what one problem cost, so the tally can tell a download from a skip
#[derive(Default)]
struct Got {
    bytes: u64,
    failed: Option<String>,
}

impl Got {
    fn fetched(&self) -> bool {
        self.bytes > 0
    }
}

fn plural(count: usize) -> String {
    format!("{count} problem{}", if count == 1 { "" } else { "s" })
}

fn sync_one(state: &State, prob: &Value, force: bool) -> Got {
    let mut got = Got::default();
    let name = prob["name"].as_str().unwrap_or_default();
    let id = &prob["id"];

    let mut save = |path: std::path::PathBuf, bytes: Vec<u8>| {
        got.bytes += bytes.len() as u64;
        cache_write(&path, bytes);
    };

    let detail = detail_file(name);
    if force || !detail.is_file() {
        let body = api(state, minreq::Method::Get, &format!("problems/{id}"), None);
        save(detail, serde_json::to_string(&body).unwrap().into_bytes());
    }

    let description = statement_file(name, "json");
    if force || !description.is_file() {
        let body = api(
            state,
            minreq::Method::Get,
            &format!("problems/{id}/description"),
            None,
        );
        save(
            description,
            serde_json::to_string(&body).unwrap().into_bytes(),
        );
    }

    // a problem with no PDF costs one request per sync to find that out again
    let pdf = statement_file(name, "pdf");
    if force || !pdf.is_file() {
        if let Some(bytes) = api_bytes(state, &format!("problems/{id}/files/pdf")) {
            save(pdf, bytes);
        }
    }

    if prob["has_testcase"].as_bool() == Some(true) && (force || case_names(name).is_empty()) {
        match fetch_testcases(state, prob, false) {
            Ok(dir) => got.bytes += tree_size(&dir),
            Err(reason) => got.failed = Some(format!("{name}: {reason}")),
        }
    }
    got
}

pub fn run(filter: Filter, force: bool) {
    if offline::on() {
        offline::refuse("sync");
    }
    let state = authed_state();
    let problems = select(get_problems(&state), &filter);
    if problems.is_empty() {
        println!("{}", dim("no problems match"));
        return;
    }

    eprintln!(
        "{}",
        edim(format!(
            "syncing {} from {}",
            plural(problems.len()),
            state.url.clone().unwrap_or_default()
        ))
    );
    let bar = ProgressBar::new(problems.len() as u64);
    bar.set_style(
        ProgressStyle::with_template("{bar:24} {pos}/{len}  {msg}")
            .unwrap()
            .progress_chars("=> "),
    );

    let (mut fetched, mut bytes) = (0, 0);
    let mut failed = Vec::new();
    for prob in &problems {
        bar.set_message(prob["name"].as_str().unwrap_or_default().to_string());
        let got = sync_one(&state, prob, force);
        if got.fetched() {
            fetched += 1;
        }
        bytes += got.bytes;
        failed.extend(got.failed);
        bar.inc(1);
    }
    bar.finish_and_clear();

    let mut tally = format!(
        "synced {} · skipped {}",
        bold(fetched),
        bold(problems.len() - fetched)
    );
    // a problem can land its statement and still lose its testcases, so this counts apart
    if !failed.is_empty() {
        tally.push_str(&format!(" · failed {}", bold(failed.len())));
    }
    println!(
        "{tally} · {}",
        bold(ByteSize::b(bytes).display().iec().to_string())
    );
    if !failed.is_empty() {
        for note in &failed {
            eprintln!("  {}", dim(note));
        }
        fail(&format!("{} could not be synced", plural(failed.len())));
    }
}
