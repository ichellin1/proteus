//! Parsing the HLS manifests the web shell's player reads: the playlist that
//! lists a stream's init segment and media segments.
//!
//! It's here, rather than in the web shell, because it uses no browser API,
//! so its tests run with `cargo test` on every platform. The web shell only
//! builds for WebAssembly, where CI runs no tests.

/// A parsed HLS manifest: the init segment, the media segments, and their
/// total duration.
pub struct HlsManifest {
    /// The init segment's URI, relative to the manifest.
    pub init_uri: String,
    /// The media segments' URIs, relative to the manifest, in order.
    pub segment_uris: Vec<String>,
    /// The total duration, in seconds.
    pub duration: f64,
}

/// Parses a single-stream HLS manifest: one `#EXT-X-MAP` init segment and a
/// list of `#EXTINF` segments. Streams with several bitrates aren't
/// supported.
///
/// # Errors
///
/// If the manifest has no init segment or no media segments.
pub fn parse(text: &str) -> Result<HlsManifest, String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let mut init_uri = None;
    let mut duration = 0.0;
    let mut segment_uris = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        if let Some(rest) = line.strip_prefix("#EXT-X-MAP:") {
            if let Some(start) = rest.find("URI=\"") {
                let after = &rest[start + "URI=\"".len()..];
                if let Some(end) = after.find('"') {
                    init_uri = Some(after[..end].to_string());
                }
            }
        } else if let Some(rest) = line.strip_prefix("#EXTINF:") {
            duration += parse_leading_f64(rest);
            if let Some(next) = lines.get(i + 1) {
                if !next.starts_with('#') {
                    segment_uris.push(next.to_string());
                }
            }
        }
    }

    match init_uri {
        Some(init_uri) if !segment_uris.is_empty() => Ok(HlsManifest {
            init_uri,
            segment_uris,
            duration,
        }),
        _ => Err("HLS manifest: missing init segment or media segments".to_string()),
    }
}

/// Parses the number at the start of `s` and ignores the rest, like
/// JavaScript's `parseFloat`: `#EXTINF:6.006,` has a trailing comma.
fn parse_leading_f64(s: &str) -> f64 {
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(s.len());
    s[..end].parse().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_init_uri_and_segments_in_order() {
        let manifest = "#EXTM3U\n#EXT-X-MAP:URI=\"init.m4s\"\n#EXTINF:6.006,\nseg_0.m4s\n#EXTINF:2.002,\nseg_1.m4s\n#EXT-X-ENDLIST\n";
        let parsed = parse(manifest).unwrap();
        assert_eq!(parsed.init_uri, "init.m4s");
        assert_eq!(parsed.segment_uris, vec!["seg_0.m4s", "seg_1.m4s"]);
        assert!((parsed.duration - 8.008).abs() < 1e-9);
    }

    #[test]
    fn missing_init_or_segments_is_an_error() {
        assert!(parse("#EXTM3U\n#EXTINF:6.006,\nseg_0.m4s\n").is_err());
        assert!(parse("#EXTM3U\n#EXT-X-MAP:URI=\"init.m4s\"\n").is_err());
    }

    #[test]
    fn parse_leading_f64_stops_at_trailing_comma() {
        assert!((parse_leading_f64("6.006,") - 6.006).abs() < 1e-9);
    }
}
