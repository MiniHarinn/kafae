use std::path::Path;

use crate::templates;
use crate::ui::{bold, dim, table};

pub fn run() {
    let rows: Vec<Vec<String>> = templates::listing()
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

    table(&[("name", false), ("file", false), ("from", false)], &rows);

    println!(
        "\n{}",
        dim(format!("yours go in {}", templates::user_dir().display()))
    );
}
