fn main() {
    // pm's `.rhai` recipe grammar has no Rust bindings upstream, so its
    // generated parser is vendored and compiled here; the other grammars come
    // from their own crates. Only grammar-loading builds link it.
    if std::env::var_os("CARGO_FEATURE_LOAD_GRAMMARS").is_none() {
        return;
    }
    let source = std::path::Path::new("vendor/tree-sitter-rhai/src");
    cc::Build::new()
        .include(source)
        .file(source.join("parser.c"))
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs")
        .compile("tree-sitter-rhai");
    println!("cargo:rerun-if-changed=vendor/tree-sitter-rhai/src");
}
