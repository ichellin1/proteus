//! Collects the Rust snippets in `docs/` that use the web host into one file,
//! which `lib.rs` compiles only for wasm32: the web host only builds there,
//! so those snippets can't run as native doctests. CI checks them with
//! `cargo clippy --target wasm32-unknown-unknown -p proteus-docs`.

use std::path::{Path, PathBuf};
use std::{env, fs};

fn pages(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            pages(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

fn main() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    println!("cargo:rerun-if-changed={}", docs.display());

    let mut found = Vec::new();
    pages(&docs, &mut found);
    found.sort();

    let mut out = String::new();
    let mut n = 0;
    for page in found {
        let text = fs::read_to_string(&page).unwrap();
        let mut block: Option<String> = None;
        for line in text.lines() {
            match block.as_mut() {
                None if line.starts_with("```rust") => block = Some(String::new()),
                None => {}
                Some(code) if line == "```" => {
                    // A comment that names the web host doesn't make a
                    // snippet one that uses it.
                    let uses_web_host = code.lines().any(|line| {
                        !line.trim_start().starts_with("//") && line.contains("proteus_host_web")
                    });
                    if uses_web_host {
                        n += 1;
                        let name = page.strip_prefix(&docs).unwrap().display();
                        out.push_str(&format!("// {name}\npub mod snippet_{n} {{\n{code}}}\n"));
                    }
                    block = None;
                }
                Some(code) => {
                    // A hidden line, `# code`, is part of the snippet.
                    let line =
                        line.strip_prefix("# ")
                            .unwrap_or(if line == "#" { "" } else { line });
                    code.push_str(line);
                    code.push('\n');
                }
            }
        }
    }
    let path = Path::new(&env::var("OUT_DIR").unwrap()).join("web_snippets.rs");
    fs::write(path, out).unwrap();
}
