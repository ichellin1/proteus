# Text

A component can show text: a word, a label, or paragraphs that wrap, in the fonts an app loads,
with styles mixed within a line and components sitting in it. The host lays the text out and
draws it before it next draws a frame. An app can also measure it at once, before it's drawn.

## A text


A component's text is drawn over its fill, in the component's center, or wherever its anchor
puts it; see [Anchor](./components.md#anchor). It's one line, or more where it has a `\n`. It
can also wrap; see [Lines](#lines).

#### Rust

```rust
# use proteus_sdk::glam::Vec4;
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Text};
# let mut app = Proteus::new();
let title = app.component(
    ComponentSpec::new(QuadState::default()).text(
        Text::new("Proteus", 32.0)
            .with_color(Vec4::new(0.1, 0.1, 0.1, 1.0))
            .with_letter_spacing(1.5),
    ),
);
```

#### TypeScript

```ts
import { colorFrom } from "proteus-sdk";

const title = app.component({
  geometry: app.get(card)!.geometry,
  text: { content: "Proteus", sizePx: 32, color: colorFrom("#1a1a1a"), letterSpacingPx: 1.5 },
});
```

- **Size** is in pixels. The text isn't shrunk to fit the component, and isn't wrapped at the
  component's width. It wraps at a width of its own.
- **Color** defaults to white. The color's alpha is multiplied by the component's opacity.
  Changing the color costs nothing, unless the text has a colored span; see [Styles within a
  text](#styles-within-a-text).
- **Letter spacing** adds space between characters, in pixels. A negative value tightens it.
- **The font** is the app's default font, Inter, built into Proteus, bold unless the text sets
  a weight. A text can name another family the app has loaded, a weight and italic; see
  [Fonts](#fonts).
- **Text larger than an atlas page**, 2048 pixels by default, is drawn at a lower resolution on
  a high-density display, if that makes it fit. Otherwise it's cut off at the page's edge, with
  a warning.

## Lines

A text can wrap at a width, align its lines within it, space them, and stop after a number of
lines.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Text, TextAlign};
# let mut app = Proteus::new();
let summary = app.component(
    ComponentSpec::new(QuadState::default()).text(
        Text::new("Proteus draws every component as a textured, rounded rectangle.", 16.0)
            .with_wrap_width(280.0)
            .with_align(TextAlign::Center)
            .with_line_height(1.4)
            .with_max_lines(2)
            .with_ellipsis(" more"),
    ),
);
```

#### TypeScript

```ts
const summary = app.component({
  geometry: app.get(card)!.geometry,
  text: {
    content: "Proteus draws every component as a textured, rounded rectangle.",
    sizePx: 16,
    wrapWidth: 280,
    align: "center",
    lineHeight: 1.4,
    maxLines: 2,
    overflow: { ellipsis: " more" },
  },
});
```

- **The wrap width** is in logical pixels, and is also the width of the text's box. Without one,
  lines break only at `\n`, and the box is as wide as the widest line. A word wider than the
  wrap width breaks between characters. Lines break at the same words on every display.
- **Alignment** places each line at the start, center or end of the box. The anchor places the
  box on the component.
- **Line height** is the distance from one baseline to the next, as a multiple of the size, as
  CSS's unitless `line-height` is. Without a line height, the distance is the font's own line
  spacing.
- **The maximum number of lines** ends text that runs past it in an ellipsis, "…" by default.
  The text can be clipped instead, which drops the lines past the last with no mark. Clipping is
  `TextOverflow::Clip` in Rust and `overflow: "clip"` in TypeScript.

An ellipsis is laid out with the text, so it wraps as words do. The text keeps as much of
itself as fits with the ellipsis after it, cut between whole characters. An ellipsis wider than
a line, such as " read the whole article" on a narrow card, takes lines of its own, and the
text gives those lines up. An ellipsis too long to fit in the maximum number of lines on its
own is clipped, with a warning suggesting more lines, a wider wrap width or a shorter ellipsis.

The [essays example](../../examples/essays) shows each of these once, in both languages.

## Fonts

An app loads a font from the bytes of its file, and a text names the font's family to be drawn
in it. Loading takes no name, since the file is the font. `load_font` returns the family name
written inside the file. Loading a second file of the same family, such as its italic, adds that
file to the family.

#### Rust

```rust,no_run
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Text};
# let mut app = Proteus::new();
// The font is its file: here, Lora, a TTF file read at run time.
let bytes = std::fs::read("fonts/Lora-Variable.ttf")?;
// Or built into the program, which works on the web too:
// let bytes = include_bytes!("fonts/Lora-Variable.ttf");
match app.load_font(bytes) {
    // `family` is "Lora", the name inside the file.
    Ok(family) => {
        app.component(ComponentSpec::new(QuadState::default()).text(
            Text::new("Chapter one", 28.0)
                .with_family(family)
                .with_weight(600)
                .italic(),
        ));
    }
    Err(e) => eprintln!("can't read the font: {e}"),
}
# Ok::<(), std::io::Error>(())
```

#### TypeScript

```ts
const bytes = new Uint8Array(await (await fetch("fonts/Lora.ttf")).arrayBuffer());
const family = app.loadFont(bytes);
app.component({
  geometry: app.get(card)!.geometry,
  text: { content: "Chapter one", sizePx: 28, family, weight: 600, italic: true },
});
```

- **The family** is the name inside the font's file, which `load_font` returns. Without a
  family, a text is in the default font.
- **The weight** runs from 100 to 900: 400 is regular and 700 bold. A family without that weight
  uses its nearest weight, as in CSS, and a family with no bold face is thickened. Without a
  weight, a text is in the font's own weight, or bold in the built-in Inter.
- **Italic** uses the family's italic face, and slants the upright one if the family has none.

A downloaded font arrives after the app starts. Proteus waits for it if it's told the family is
coming, so texts in that family aren't drawn in one font and then another:

#### Rust

Tell Proteus the family is coming, and start the download, in `setup`. Load the font when it
arrives:

```rust
use proteus_runtime::{App, FetchId, Frame};
use proteus_sdk::WhileLoading;

struct Reader {
    lora: Option<FetchId>,
}

impl App for Reader {
    fn setup(&mut self, f: &mut Frame) {
        // The family is named here, before its file arrives, so its texts wait for it.
        f.proteus.expect_font("Lora", WhileLoading::Hide);
        self.lora = Some(f.fetch_async("https://example.com/fonts/Lora-Variable.ttf"));
    }

    fn update(&mut self, f: &mut Frame, _dt: f32) {
        for (id, bytes) in f.poll_fetches() {
            if Some(id) != self.lora {
                continue;
            }
            // `None` means the download failed; the host logs why.
            let loaded = bytes.map(|bytes| f.proteus.load_font(bytes));
            if !matches!(loaded, Some(Ok(_))) {
                // Its texts stop waiting, and are drawn in the default font.
                f.proteus.font_failed("Lora");
            }
        }
    }
}
```

#### TypeScript

```ts
app.loadFont(fetch("fonts/Lora.ttf").then((r) => r.arrayBuffer()), { family: "Lora" });
```

| The family is | Its texts are drawn |
|---|---|
| Loaded | in it |
| Coming, waiting: `WhileLoading::Hide`, `whileLoading: "hide"`, the default | not at all, until it loads or fails |
| Coming, meanwhile: `WhileLoading::DefaultFont`, `whileLoading: "defaultFont"` | in the default font, then again in their own once it loads |
| Failed, or never loaded or expected | in the default font, with a warning |

A family that's coming but never loads or fails keeps its texts hidden, as an image that never
loads is never drawn. The [essays example](../../examples/essays) loads Lora this way in
TypeScript.

## Fallback

A font has the characters its designers drew, and no more. Inter has Latin, Greek and Cyrillic,
but not Hindi's Devanagari, Chinese or emoji. A character a text's font lacks is looked for in
the app's fallback fonts, in order, then in the default font. A character none of them has is
drawn as the font's missing-glyph box, a rectangle, with a warning naming the character.

#### Rust

```rust,no_run
# use proteus_sdk::Proteus;
# let mut app = Proteus::new();
// Noto Serif Devanagari, a TTF file read at run time.
let devanagari = std::fs::read("fonts/NotoSerifDevanagari-Variable.ttf")?;
// The family name inside the file is the one to list.
if let Ok(family) = app.load_font(devanagari) {
    app.set_fallback_fonts([family]);
}
# Ok::<(), std::io::Error>(())
```

#### TypeScript

```ts
app.setFallbackFonts(["Noto Serif Devanagari"]);
```

- **There are none by default.** Fallback is the app's choice. Proteus never uses the system's
  fonts, so text is drawn the same everywhere, and nothing is fetched that the app didn't ask
  for.
- **List only what the app's text needs.** Each fallback is a font the app ships or downloads,
  and keeps in memory. A font for Chinese, Japanese or Korean is several megabytes. A long list
  makes it harder to know which font draws a given character, and a character the app didn't
  plan for can arrive in a font that doesn't match the rest. In almost every app, fonts should
  be deterministic. Choose them for the text the app shows, and know what each covers.
- **The list is the app's,** and applies to every text. Fonts are tried in the order given, and
  setting the list again replaces it.
- **A fallback that's still loading** is treated as a text's own family is. If it's
  expected with `WhileLoading::Hide`, texts with characters no loaded font has wait for it.
  Otherwise those characters are drawn as boxes, and the texts are drawn again once it loads.
- **Color emoji** isn't drawn yet. Emoji fonts store their glyphs in color, and text is drawn in
  one color. A black-and-white emoji font, such as Noto Emoji, works as a fallback.

The [essays example](../../examples/essays) draws an essay in Hindi with a fallback font.

## Styles within a text

A text can mix styles, as spans. A span is a part of the text with a weight, italic, size,
color, family or vertical alignment of its own, and takes whatever it doesn't set from the text.

#### Rust

```rust
# use proteus_sdk::glam::Vec4;
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Span, Text};
# let mut app = Proteus::new();
let ink = Vec4::new(0.14, 0.13, 0.17, 1.0);
let muted = Vec4::new(0.42, 0.4, 0.46, 1.0);
let byline = app.component(
    ComponentSpec::new(QuadState::default()).text(
        Text::rich(
            [
                Span::new("Mary Wollstonecraft").with_weight(600),
                Span::new("\n1792").with_size(12.0).with_color(muted),
            ],
            14.0,
        )
        .with_color(ink),
    ),
);
```

#### TypeScript

```ts
import { colorFrom } from "proteus-sdk";

const byline = app.component({
  geometry: app.get(card)!.geometry,
  text: {
    spans: [
      { content: "Mary Wollstonecraft", weight: 600 },
      { content: "\n1792", sizePx: 12, color: colorFrom("#6b6676") },
    ],
    sizePx: 14,
    color: colorFrom("#24212b"),
  },
});
```

- **A span can set** a weight, italic, a size, a color, a family and a vertical alignment. In
  Rust, `bold()` is a weight of 700. A span that sets nothing is in the text's own style.
- **A text is its content or its spans, not both.** In Rust, a text with both draws its spans,
  with a warning. In TypeScript, `TextSpec` takes `content` or `spans`, and an object with both
  throws.
- **Spans of different sizes share a line,** and the line is as tall as its largest span needs.
- **A span can move up or down,** as CSS's `vertical-align` does. `Middle` centers the
  span on the text's lowercase letters, which suits an icon. An icon font centers its icons
  between its ascender and descender, so they otherwise sit high beside text. `Super` and `Sub`
  are the font's superscript and subscript positions, at the span's own size. `Raise` moves the
  span by logical pixels, up, or down if negative. In TypeScript, `verticalAlign` is `"middle"`,
  `"super"`, `"sub"` or a number. A moved span makes its line taller if it needs the room.
- **A span's family counts as the text's.** While a font a span names is still loading, the
  whole text waits, or is drawn in the default font, as a text in that family would be.
- **An ellipsis can be spans:** `with_ellipsis_spans` in Rust, and `overflow: { ellipsis: [...]
  }` in TypeScript. An ellipsis given as a string is in the text's own style, not the style of
  the span it cuts.

### Colored spans

A text is drawn as one image, and the GPU tints that image with one color. A text with no
colored span is baked white, and its color is the tint, so changing the color costs nothing.
One tint can't draw two colors, so a text with a colored span is baked in its colors, with the
spans that set none in the text's color. The text's alpha and the component's opacity are
still applied as the text is drawn, so fading it costs nothing.

Changing the red, green or blue of a text with a colored span bakes it again, which costs about
as much as setting the text again. Measured natively at a display scale of 2, that's about
0.03 ms for a short label, 0.12 ms for a line of 80 characters, and 0.6 ms for a five-line
paragraph, in each frame the color changes. To animate a text's color, give it no colored span.

The [essays example](../../examples/essays) sets a byline's weight, size and color, a source's
titles in italic, a superscript footnote mark after each pull quote, and a card's ellipsis in
bold in the accent color, with an icon from an icon font centered on the text.

## Components in text

A span can hold a component in place of text, such as an icon, a tag or a button within a
sentence. The text keeps a gap for the component in its line, at the size the span gives. The
gap wraps, aligns and ends in an ellipsis as a word does. The component is placed in its gap.

#### Rust

```rust
# use proteus_sdk::glam::{Vec2, Vec4};
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Span, Text, VerticalAlign};
# let mut app = Proteus::new();
// A key cap: a small rounded component with its own text.
let key = app.component(
    ComponentSpec::new(QuadState {
        size: Vec2::new(24.0, 22.0),
        corner_radius: 5.0,
        color: Vec4::new(0.3, 0.29, 0.35, 1.0),
        ..Default::default()
    })
    .text(Text::new("K", 13.0)),
);
let hint = app.component(ComponentSpec::new(QuadState::default()).text(Text::rich(
    [
        Span::new("Press "),
        Span::component(key.id(), Vec2::new(24.0, 22.0))
            .with_vertical_align(VerticalAlign::Middle),
        Span::new(" to keep going"),
    ],
    16.0,
)));
```

#### TypeScript

```ts
const key = app.component({
  geometry: { ...app.get(card)!.geometry, size: { width: 24, height: 22 }, cornerRadius: 5 },
  text: { content: "K", sizePx: 13 },
});
const hint = app.component({
  geometry: app.get(card)!.geometry,
  text: {
    spans: [
      { content: "Press " },
      { component: key.id(), width: 24, height: 22, verticalAlign: "middle" },
      { content: " to keep going" },
    ],
    sizePx: 16,
  },
});
```

- **The text makes the component its child,** and owns the component's position. The
  component's anchor sits at the same point of its gap. By default its center is at the gap's
  center, and with an anchor of (0, 0) its top-left corner is at the gap's top-left corner. The
  text sets the position again whenever the text changes, and it overrides any position the app
  gives the component.
- **The gap's size is the span's, not the component's,** so a component that grows on hover, or
  animates its size, doesn't move the text around it. A component larger than its gap overlaps
  the text around it.
- **Vertical alignment places the gap.** By default the gap's bottom sits on the baseline, as an
  image does on a web page. `Middle` centers the gap on the text's lowercase letters, which
  suits an icon or a chip, and `Super`, `Sub` and `Raise` move the gap as they move text. A gap
  taller than its line makes the line taller.
- **A component the text doesn't show isn't drawn:** one whose gap is cut off by the maximum
  number of lines, one in an ellipsis that isn't shown, and every one in a text that waits for a
  font. The component is drawn again once the text shows it, and the visibility the app set on
  it is kept.
- **It's placed in the same frame as the text changes,** before the text is drawn, since the app
  has the fonts. `measure_text` counts each gap, and a gap at the end of a line widens the text.
- **It's an ordinary component.** It takes clicks and hover styles, so a button in a text works
  as a button. A component the text no longer holds stays its child, where it was last placed.

The [essays example](../../examples/essays) starts each open essay's body with a tag showing
its year, which moves as the toolbar changes the text, and ends the Hindi card's opening in a
small button in its ellipsis.

## Centering a label

A text's box is its line box by default. The line box runs from the top of the first line to
the bottom of the last, with the room the font reserves above and below its letters for accents
and descenders.
That room isn't the same above and below, so a label centered in a button by its line box can
sit off center. Noto Serif Devanagari, for one, reserves far more below its letters than above:

| Font, at 12 pixels | Room above the baseline | Room below | x-height | Cap height |
|---|---:|---:|---:|---:|
| Inter | 11.6 | 2.9 | 6.6 | 8.7 |
| Noto Serif Devanagari | 11.2 | 7.5 | 7.5 | 8.6 |

A text can set where its box's edges are instead, as CSS's `text-box-edge` does:

- **`TextBox::Line`, the default,** is the line box, with any line spacing.
- **`TextBox::CapHeight`** runs from the first line's cap height, the top of its capitals, to
  the last line's baseline. It centers capitals, title case and digits, as most buttons' labels
  are.
- **`TextBox::XHeight`** runs from the first line's x-height, the top of its lowercase letters,
  to the last line's baseline. It centers lowercase text, and Devanagari, whose letters hang
  from a headline at the x-height.

#### Rust

```rust
# use proteus_sdk::glam::Vec2;
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Text, TextBox};
# let mut app = Proteus::new();
let label = Text::new("Save changes", 14.0).with_box(TextBox::CapHeight);
app.component(
    ComponentSpec::new(QuadState {
        size: Vec2::new(140.0, 36.0),
        ..Default::default()
    })
    .text(label),
);
```

#### TypeScript

```ts
app.component({
  geometry: { ...app.get(card)!.geometry, size: { width: 140, height: 36 } },
  text: { content: "Save changes", sizePx: 14, box: "capHeight" },
});
```

- **Only the box changes, not the image.** Ink outside the box, such as descenders, accents and
  Devanagari's vowel marks, still draws, outside it, so leave room for it in a button's padding.
- **The heights come from the fonts that draw the first line,** taking the tallest, so text
  drawn in a fallback font is boxed by that font's measurements, not by those of the text's own
  family.
- **Everything that uses a text's size uses its box:** `measure_text`, `baked_text_size`, the
  anchor that places the text, and the gaps for components in it.

The [essays example](../../examples/essays) trims its back button's and year tag's boxes to the
cap height, and its Hindi button's to the x-height.

## Measuring text

`measure_text` returns a text's size, in logical pixels, without drawing it. An app can lay out
around a text before it's drawn, such as to fit a button to its label. The size is the one the
text is drawn at, and the one `baked_text_size` returns once the text is drawn.

#### Rust

```rust
# use proteus_sdk::glam::Vec2;
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Text};
# let mut app = Proteus::new();
let label = Text::new("Back to essays", 14.0);
// `None` only while a font the text names is still loading.
if let Some(size) = app.measure_text(&label) {
    // A button as wide as its label, with 20 pixels each side.
    app.component(
        ComponentSpec::new(QuadState {
            size: Vec2::new(size.x + 40.0, 36.0),
            ..Default::default()
        })
        .text(label),
    );
}
```

#### TypeScript

```ts
const label = { content: "Back to essays", sizePx: 14 };
// `undefined` only while a font the text names is still loading.
const size = app.measureText(label);
if (size) {
  // A button as wide as its label, with 20 pixels each side.
  app.component({
    geometry: { ...app.get(card)!.geometry, size: { width: size.width + 40, height: 36 } },
    text: label,
  });
}
```

- **It includes the ink.** Measuring lays the text out as drawing does, and works out where each
  glyph's pixels go without drawing them. Ink that reaches past the text's box, such as an
  italic's overhang, widens the measured size, as it widens the drawn text.
- **It's rounded to the display.** A text is drawn in whole pixels at the display's scale, so
  it's measured that way too. On a display of another scale, a text can measure under a
  pixel differently, so measure it again when the scale changes.
- **A font that's loading.** A text in a family expected with `WhileLoading::Hide`, or
  `whileLoading: "hide"`, measures `None`, or `undefined`, since it isn't drawn yet. With
  `DefaultFont`, a text measures in the default font, which is what it's drawn in meanwhile. A
  font loaded with `load_font` measures at once.
- **The cost.** Measuring takes about 5 µs for a label and 25 µs for a line of 80 characters,
  natively, in the text benchmark. That's half to three quarters of what drawing the text costs.
  The first time a glyph is measured costs more, since its outline is read. The glyph's pixel
  box is kept after that.

## Changing text

To change a component's text, use `set_text`, which replaces it. The host renders the new text
before it next draws.

#### Rust

```rust
# use proteus_sdk::{ComponentSpec, Proteus, QuadState, Text};
# let mut app = Proteus::new();
# let score = app.component(ComponentSpec::new(QuadState::default()).text(Text::new("0", 24.0)));
score.set_text(&mut app, Text::new("12", 24.0))?;
# Ok::<(), proteus_sdk::HandleError>(())
```

#### TypeScript

```ts
card.setText({ content: "12", sizePx: 24 });
```

Setting a text bakes it again. The text is laid out and drawn into a new image, which costs
about 0.03 ms for a short label, 0.12 ms for a line of 80 characters, and 0.6 ms for a five-line
paragraph, measured natively at a display scale of 2. Changing only the text's color or alpha
costs nothing, unless it has a colored span; see [Colored spans](#colored-spans).

Once the host has rendered a text, `baked_text_size` returns the text's size, the same size
[measuring](#measuring-text) gives before it's drawn.

## Limits

- **Lines aren't justified.** They align at the start, center or end of their box.
- **A text wraps at a width of its own,** not at the component's width, and doesn't wrap again
  when the component changes size.
- **Recoloring a text with a colored span bakes it again,** since the text's image is drawn with
  one tint; see [Colored spans](#colored-spans). A plain text's color, and any text's alpha,
  change at no cost.
- **No text input.** There's no text field, cursor or selection, and no input methods for
  composing text. A key's text arrives in the key's event, for an app to use itself.
- **Only the app's fonts are used,** never the system's, so text looks the same everywhere.
  Color emoji isn't drawn.
