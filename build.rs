fn main() {
    embed_templates();
    // an exe with no version resource looks like malware to Defender's heuristics
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set("ProductName", "kafae")
            .set("FileDescription", "Terminal client for Cafe Grader")
            .set("OriginalFilename", "kafae.exe")
            .set("LegalCopyright", "Copyright (c) 2026 Harinn, MIT license")
            .compile()
            .expect("failed to embed the version resource");
    }
}

// every file in ./templates becomes a builtin template, dotfiles excepted
fn embed_templates() {
    println!("cargo:rerun-if-changed=templates");
    let mut files: Vec<String> = std::fs::read_dir("templates")
        .expect("missing ./templates")
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|file| !file.starts_with('.'))
        .collect();
    files.sort();
    let rows: String = files
        .iter()
        .map(|file| {
            format!(
                "    ({file:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/templates/{file}\"))),\n"
            )
        })
        .collect();
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("builtins.rs");
    std::fs::write(
        out,
        format!("const BUILTINS: &[(&str, &str)] = &[\n{rows}];\n"),
    )
    .expect("cannot write builtins.rs");
}
