use crate::templates;
use crate::ui::{bold, dim, table};

pub fn run() {
    // every source is in the catalogue, including the grader's own file, so nothing here
    // has to invent a row for what the listing cannot describe
    let rows: Vec<Vec<String>> = templates::catalogue()
        .into_iter()
        .map(|start| {
            vec![
                bold(start.name).to_string(),
                dim(&start.file).to_string(),
                dim(start.source.label()).to_string(),
            ]
        })
        .collect();

    table(&[("name", false), ("file", false), ("from", false)], &rows);

    println!(
        "\n{}",
        dim(format!("yours go in {}", templates::user_dir().display()))
    );
}
