use std::path::Path;

use console::measure_text_width;

use crate::templates;
use crate::ui::{bold, dim, pad};

pub fn run() {
    let rows: Vec<[String; 3]> = templates::listing()
        .into_iter()
        .map(|(file, builtin)| {
            let name = Path::new(&file)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("")
                .to_string();
            [
                name,
                file,
                if builtin { "builtin" } else { "yours" }.to_string(),
            ]
        })
        .collect();

    let headers = ["name", "file", "from"];
    let mut widths = headers.map(measure_text_width);
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(measure_text_width(cell));
        }
    }

    let head = |text: &str, width| bold(dim(pad(text, width, false)).to_string()).to_string();
    println!(
        "{}",
        format!(
            "{}  {}  {}",
            head(headers[0], widths[0]),
            head(headers[1], widths[1]),
            head(headers[2], widths[2]),
        )
        .trim_end()
    );
    for [name, file, from] in &rows {
        println!(
            "{}",
            format!(
                "{}  {}  {}",
                pad(&bold(name).to_string(), widths[0], false),
                pad(&dim(file).to_string(), widths[1], false),
                dim(from),
            )
            .trim_end()
        );
    }

    println!(
        "\n{}",
        dim(format!("yours go in {}", templates::user_dir().display()))
    );
}
