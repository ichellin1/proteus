//! The Rust snippets in `docs/`, run as doctests by `cargo test`, so that
//! every page's code compiles, and runs unless it is marked `no_run`.
//!
//! Each page is included as the doc comment of an empty module below. A
//! code block with no language is tested as Rust too, so a page labels every
//! other block, such as `bash`, `text` or `ts`. A test checks that every page
//! in `docs/` is included.
//!
//! The snippets that use the web host only build for wasm32, so natively they
//! compile to nothing. `build.rs` collects them into `web_snippets`, which is
//! compiled for wasm32 only.

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
    getting_started_rust => "getting-started/rust.md",
    getting_started_typescript => "getting-started/typescript.md",
    guides_components => "guides/components.md",
    guides_transitions => "guides/transitions.md",
    guides_splits_and_merges => "guides/splits-and-merges.md",
    guides_interaction => "guides/interaction.md",
    guides_content => "guides/content.md",
    guides_configuration => "guides/configuration.md",
    guides_hosts => "guides/hosts.md",
    how_to_chaining_transitions => "how-to/chaining-transitions.md",
    how_to_staggering => "how-to/staggering.md",
    how_to_loading_images => "how-to/loading-images.md",
    how_to_custom_easing => "how-to/custom-easing.md",
    how_to_platform_features => "how-to/platform-features.md",
    how_to_tricks => "how-to/tricks.md",
}

/// The snippets that use the web host; see `build.rs`.
#[cfg(target_arch = "wasm32")]
#[allow(dead_code, clippy::all)]
mod web_snippets {
    include!(concat!(env!("OUT_DIR"), "/web_snippets.rs"));
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

    /// The page's lines outside code blocks.
    fn prose(page: &str) -> Vec<&str> {
        let mut in_code = false;
        page.lines()
            .filter(|line| {
                if line.trim_start().starts_with("```") {
                    in_code = !in_code;
                    return false;
                }
                !in_code
            })
            .collect()
    }

    /// The anchor GitHub gives a heading: lowercase, punctuation dropped,
    /// spaces as hyphens.
    fn anchor(heading: &str) -> String {
        heading
            .trim()
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
            .map(|c| if c == ' ' { '-' } else { c })
            .collect()
    }

    fn anchors(page: &str) -> Vec<String> {
        prose(page)
            .into_iter()
            .filter_map(|line| line.strip_prefix('#'))
            .map(|heading| anchor(heading.trim_start_matches('#')))
            .collect()
    }

    /// Every `](target)` in the page's prose.
    fn links(page: &str) -> Vec<String> {
        let mut found = Vec::new();
        for line in prose(page) {
            let mut rest = line;
            while let Some(start) = rest.find("](") {
                rest = &rest[start + 2..];
                if let Some(end) = rest.find(')') {
                    found.push(rest[..end].to_string());
                    rest = &rest[end..];
                }
            }
        }
        found
    }

    // A link to a page that moved, or to a heading that was renamed, would
    // otherwise only be found by a reader.
    #[test]
    fn every_link_in_docs_resolves() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let mut broken = Vec::new();
        for page in PAGES {
            let path = root.join(page);
            let text = std::fs::read_to_string(&path).unwrap();
            for link in links(&text) {
                if link.starts_with("http://") || link.starts_with("https://") {
                    continue;
                }
                if link.contains("PLANNING") {
                    broken.push(format!("{page}: {link} links to PLANNING"));
                    continue;
                }
                let (file, heading) = link.split_once('#').unwrap_or((&link, ""));
                let target = if file.is_empty() {
                    path.clone()
                } else {
                    path.parent().unwrap().join(file)
                };
                if !target.exists() {
                    broken.push(format!("{page}: {link} has no file"));
                    continue;
                }
                if !heading.is_empty() {
                    let target_text = std::fs::read_to_string(&target).unwrap();
                    if !anchors(&target_text).iter().any(|a| a == heading) {
                        broken.push(format!("{page}: {link} has no such heading"));
                    }
                }
            }
        }
        assert!(broken.is_empty(), "broken links:\n{}", broken.join("\n"));
    }

    #[test]
    fn anchors_follow_github() {
        assert_eq!(anchor("Where a component rests"), "where-a-component-rests");
        assert_eq!(anchor("1→1 transitions"), "11-transitions");
        assert_eq!(anchor("`on_drag` and friends"), "on_drag-and-friends");
    }
}
