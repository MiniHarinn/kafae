fn main() {
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
