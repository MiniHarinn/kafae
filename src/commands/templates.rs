use std::path::Path;

use crate::templates;
use crate::ui::{bold, dim, table};

pub fn run() {
    let mut rows: Vec<Vec<String>> = templates::listing()
        .into_iter()
        .map(|(file, builtin)| {
            let name = Path::new(&file)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("")
                .to_string();
            vec![
                bold(name).to_string(),
                dim(&file).to_string(),
                dim(if builtin { "builtin" } else { "yours" }).to_string(),
            ]
        })
        .collect();

    // it has no file of its own: the grader ships one per problem, named its own way
    rows.push(vec![
        bold(templates::ATTACHMENT).to_string(),
        dim("<problem>.<ext>").to_string(),
        dim("problem").to_string(),
    ]);

    table(&[("name", false), ("file", false), ("from", false)], &rows);

    println!(
        "\n{}",
        dim(format!("yours go in {}", templates::user_dir().display()))
    );
}
