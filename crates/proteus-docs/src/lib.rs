//! The Rust snippets in `docs/`, run as doctests by `cargo test`, so that
//! every page's code compiles, and runs unless it is marked `no_run`.
//!
//! Each page is included as the doc comment of an empty module below. A
//! code block with no language is tested as Rust too, so a page labels every
//! other block, such as `bash`, `text` or `ts`. A test checks that every page
//! in `docs/` is included.

/// Includes each page, and lists them in [`PAGES`].
macro_rules! pages {
    ($($module:ident => $path:literal),* $(,)?) => {
        $(
            #[doc = include_str!(concat!("../../../docs/", $path))]
            pub mod $module {}
        )*

        /// The pages included, relative to `docs/`.
        pub const PAGES: &[&str] = &[$($path),*];
    };
}

pages! {
    index => "README.md",
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::PAGES;

    fn markdown_pages(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                markdown_pages(&path, root, out);
            } else if path.extension().is_some_and(|e| e == "md") {
                let relative = path.strip_prefix(root).unwrap();
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    // A page left out of `pages!` would have its snippets go unchecked.
    #[test]
    fn every_page_in_docs_is_included() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let mut found = Vec::new();
        markdown_pages(&root, &root, &mut found);
        found.sort();
        let mut included: Vec<String> = PAGES.iter().map(|p| p.to_string()).collect();
        included.sort();
        assert_eq!(
            found, included,
            "add the missing pages to `pages!` in lib.rs"
        );
    }
}
