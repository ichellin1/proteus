//! The texts every engine lays out: each shows one thing, and the workloads
//! match the native benchmark's `text` mode.

/// Which font a span asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Face {
    /// The static Inter Bold Proteus embeds today.
    InterBold,
    /// Variable Inter, at the span's weight; italic when the span is.
    Inter,
}

/// Where lines sit in the text's width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// Part of a text with one style.
#[derive(Clone, Debug)]
pub struct Span {
    pub text: &'static str,
    pub face: Face,
    pub weight: u16,
    pub italic: bool,
}

impl Span {
    pub const fn bold(text: &'static str) -> Self {
        Self {
            text,
            face: Face::InterBold,
            weight: 700,
            italic: false,
        }
    }

    pub const fn inter(text: &'static str, weight: u16) -> Self {
        Self {
            text,
            face: Face::Inter,
            weight,
            italic: false,
        }
    }

    pub const fn italic(text: &'static str, weight: u16) -> Self {
        Self {
            text,
            face: Face::Inter,
            weight,
            italic: true,
        }
    }
}

/// A text to lay out.
#[derive(Clone, Debug)]
pub struct Case {
    pub name: &'static str,
    /// What it shows, for the report.
    pub about: &'static str,
    pub spans: Vec<Span>,
    /// Size in logical pixels.
    pub size: f32,
    /// The width lines wrap at, in logical pixels; `None` for one line.
    pub width: Option<f32>,
    pub align: Align,
    /// An inline box this many logical pixels square, in place of the
    /// text's U+FFFC, where the engine supports it.
    pub inline_box: Option<f32>,
}

impl Case {
    fn one(name: &'static str, about: &'static str, span: Span, size: f32) -> Self {
        Self {
            name,
            about,
            spans: vec![span],
            size,
            width: None,
            align: Align::Start,
            inline_box: None,
        }
    }

    /// The whole text, spans joined.
    pub fn text(&self) -> String {
        self.spans.iter().map(|s| s.text).collect()
    }
}

const PARAGRAPH: &str = "Proteus draws every component as a textured, rounded rectangle, \
    in one call. A split turns one component into several, and a merge turns several into one, \
    while text moves with its component like any other texture.";

/// The cases saved as images, each showing one thing.
pub fn visual() -> Vec<Case> {
    let mut cases = vec![
        Case::one(
            "kerning",
            "Pairs a font moves closer: A and V, T and y, W and o",
            Span::bold("AVATAR Wolf Ty. To"),
            48.0,
        ),
        Case::one(
            "ligatures",
            "fi and ffl joined, and Inter's -> arrow",
            Span::bold("office waffle -> go"),
            48.0,
        ),
        Case::one(
            "accents",
            "é typed as one character, then as e plus a combining accent",
            Span::bold("café cafe\u{301}"),
            48.0,
        ),
        Case::one(
            "small",
            "Small text, where spacing errors show most",
            Span::bold("The quick brown fox jumps over the lazy dog"),
            12.0,
        ),
    ];
    for (name, align) in [
        ("wrap-start", Align::Start),
        ("wrap-center", Align::Center),
        ("wrap-end", Align::End),
    ] {
        cases.push(Case {
            name,
            about: "A paragraph wrapped at 360 pixels",
            spans: vec![Span::inter(PARAGRAPH, 400)],
            size: 16.0,
            width: Some(360.0),
            align,
            inline_box: None,
        });
    }
    cases.push(Case {
        name: "weights",
        about: "One variable font at 100, 400, 700 and 900, then italic: mixed in one text",
        spans: vec![
            Span::inter("Thin ", 100),
            Span::inter("Regular ", 400),
            Span::inter("Bold ", 700),
            Span::inter("Black ", 900),
            Span::italic("Italic", 400),
        ],
        size: 40.0,
        width: None,
        align: Align::Start,
        inline_box: None,
    });
    cases.push(Case::one(
        "fallback",
        "Hindi, which Inter lacks: a fallback font, and letters that join",
        Span::inter("Hello नमस्ते क्षत्रिय", 400),
        40.0,
    ));
    cases.push(Case {
        name: "inline-box",
        about: "A 32-pixel inline box in a line, where a component would sit",
        spans: vec![Span::inter("Press \u{FFFC} to play", 400)],
        size: 32.0,
        width: None,
        align: Align::Start,
        inline_box: Some(32.0),
    });
    cases
}

/// What the labels say, in turn, as in the native benchmark.
const LABEL_WORDS: [&str; 16] = [
    "Settings",
    "Play",
    "Watch later",
    "Search",
    "Home",
    "Library",
    "Continue watching",
    "Profile",
    "Sign out",
    "New episodes",
    "Top picks for you",
    "Add to list",
    "Back",
    "Downloads",
    "Recently added",
    "Help",
];

/// What the lines say, in turn, as in the native benchmark.
const LINE_TEXTS: [&str; 4] = [
    "Proteus draws every component as a textured, rounded rectangle in one call.",
    "A split turns one component into several, and a merge turns several into one.",
    "Text is rasterized on the CPU, then added to the atlas and drawn like an image.",
    "Kerning pulls pairs such as AV, To and Wa closer; office and fluffy have ligatures.",
];

/// The native benchmark's labels: 500, from 14 to 24 pixels, in Inter Bold.
pub fn labels() -> Vec<Case> {
    (0..500)
        .map(|i| {
            Case::one(
                "label",
                "",
                Span::bold(LABEL_WORDS[i % LABEL_WORDS.len()]),
                14.0 + (i % 6) as f32 * 2.0,
            )
        })
        .collect()
}

/// The native benchmark's lines: 20, at 16 pixels, in Inter Bold.
pub fn lines() -> Vec<Case> {
    (0..20)
        .map(|i| {
            Case::one(
                "line",
                "",
                Span::bold(LINE_TEXTS[i % LINE_TEXTS.len()]),
                16.0,
            )
        })
        .collect()
}

/// Paragraphs wrapped at 360 pixels, which today's text can't do: 20, at 16
/// pixels, in Inter Bold.
pub fn paragraphs() -> Vec<Case> {
    (0..20)
        .map(|_| Case {
            name: "paragraph",
            about: "",
            spans: vec![Span::bold(PARAGRAPH)],
            size: 16.0,
            width: Some(360.0),
            align: Align::Start,
            inline_box: None,
        })
        .collect()
}
