The generated parser of [tree-sitter-rhai](https://github.com/elkowar/tree-sitter-rhai)
at commit `4ac7384d487ffcb54e746ef1569585a749370c5b`, the grammar pm's Zed
extension (`editors/zed` in dasmatus/pm) names for `.rhai` recipes. The
repository ships no Rust bindings, so `crates/grammars/build.rs` compiles
`src/parser.c` itself. MIT licensed, see `LICENSE-MIT`.
