/**
 * Proteus TypeScript SDK example: a reading app, the same as the Rust essays
 * example.
 *
 * The app shows a row of cards, one for each of four public-domain essays.
 * Each card's year is aligned to the end of its column. Its title wraps onto at
 * most two lines and ends in "…". Its opening is spaced at a line height of
 * 1.45, stops after five lines, and ends in an ellipsis of the card's own. The
 * second card's ellipsis is bold, in the accent color, with an arrow after it.
 * Its source is clipped after two lines, and the titles in it are italic.
 *
 * A card opens into its essay (1→1). The essay's byline is two lines, split
 * with a `\n`, with the author in a heavier weight and the year smaller and
 * muted. Its opening wraps at the column's width, and its pull quote is
 * centered. While an essay is open, a toolbar at the top sets its body's size,
 * alignment, bold and italic, line height and font, as a text editor's does.
 *
 * The essays are Mary Wollstonecraft's "A Vindication of the Rights of Woman"
 * (1792), Henry David Thoreau's "Walking" (1862) and Robert Louis Stevenson's
 * "An Apology for Idlers" (1877), from their Project Gutenberg editions, and
 * Bharatendu Harishchandra's "भारतवर्ष की उन्नति कैसे हो सकती है?" ("How can
 * India progress?", 1884), in Hindi, from Hindi Wikisource. Each is cited where
 * it's shown. None of the essays' fonts has Hindi's script, Devanagari. The
 * app's fallback font, Noto Serif Devanagari, draws it.
 *
 * The essays are set in Lora, a serif downloaded from `fonts/`, with its italic
 * for the pull quotes. Titles are Inter at weight 800 and years at 500, from
 * the built-in variable font. Bylines are Inter slanted, since Inter's italic
 * isn't loaded. Buttons and sources are Inter with no weight set, so they're
 * bold. The toolbar's icons are Material Icons, which is also a font. The
 * toolbar's last button sets the body in Crimson Pro, which is downloaded when
 * it's first chosen.
 *
 * Techniques worth copying:
 *   - A font that's downloaded is given to `loadFont` as a promise, with its
 *     family's name. Texts in Lora aren't drawn until the font arrives, so they
 *     never change font once drawn. Lora's italic joins the same family.
 *   - A font that's downloaded only when needed is still named to `loadFont` at
 *     the start, with a promise that's settled once it's fetched. Crimson Pro
 *     works this way, with `whileLoading: "defaultFont"`, so its button and the
 *     body show the default font until it arrives, then swap to it.
 *   - A fallback font is named once, for the whole app, with
 *     `setFallbackFonts`. A character a text's font lacks is looked for there,
 *     then in the default font. Only the fonts the text needs are listed, so
 *     every character drawn is known to be covered.
 *   - An icon font's ligatures turn a name, such as "format_bold", into its
 *     icon. The toolbar's icons are text in Material Icons.
 *   - A chosen button, or one toggled on, has its hover style removed with
 *     `setStyle`. It then stays in the accent color under the pointer, and gets
 *     its hover style back when it's no longer chosen.
 *   - Text is placed by its component's anchor. With an anchor of (0, 0), the
 *     text's top-left corner is at the component's position. A card's texts
 *     are small children, placed from the card's top-left corner.
 *   - A wrap width is the text's box. Lines align within it, and the box
 *     doesn't change size when the words do, so the year, aligned to the end,
 *     lines up with the card's right edge whatever it says.
 *   - An ellipsis wider than a line takes lines of its own, as the third
 *     card's does.
 *   - Texts are hidden while their card or the essay changes shape, since
 *     children move with their parent but don't stretch with it. They're shown
 *     when it arrives.
 *   - Changing a text's alignment or line height means setting it again.
 *     `setText` replaces the text, and the host lays it out again.
 *   - A text can mix styles, given as `spans` rather than `content`. Each span
 *     takes whatever it doesn't set from the text. The byline sets the author's
 *     weight and the year's size and color, and a source's titles are italic.
 *   - An ellipsis can be spans too. The second card's "read more" is bold, in
 *     the accent color, with an arrow from Material Icons. An icon font
 *     centers its icons between its ascender and descender, so they sit high
 *     beside text. `verticalAlign: "middle"` centers the arrow on the text's
 *     lowercase letters instead.
 *   - A span can sit above or below the baseline. Each pull quote ends in a
 *     dagger raised with `verticalAlign: "super"`, and the source in the essay
 *     starts with one. An ellipsis given as a string, such as the first card's
 *     "…", is in the text's own style.
 *   - A text with a colored span is baked in its colors, so changing its color
 *     bakes it again. A plain text's color, and any text's alpha, cost nothing
 *     to change.
 *   - A text can be measured before it's drawn, with `measureText`. The
 *     essay's back button is as wide as its label, plus room each side.
 *
 * Not shown here: a weight a font doesn't have, a character no font has (which
 * is drawn as a box), and components within a line of text. Measuring a text
 * whose font is still loading, which gives `undefined`, and a text too large
 * for an atlas page are covered by the Rust tests only.
 */

import { colorFrom, mount } from "proteus-sdk";
import type { Color, ComponentSpec, Geometry, Handle, ProteusApp, SpanSpec, TextSpec } from "proteus-sdk";

const BACKGROUND = colorFrom("#1d1b26");
const PAPER = colorFrom("#f4f1ea");
const INK = colorFrom("#24212b");
const MUTED = colorFrom("#6b6676");
const ACCENT = colorFrom("#7a5fb0");
/** The color of a button, light enough to stand out from the background. */
const BUTTON = colorFrom("#4d4a58");
const BUTTON_HOVER = colorFrom("#5f5c6a");
const WHITE = colorFrom("#ffffff");
const CLEAR: Color = { r: 0, g: 0, b: 0, a: 0 };

const OPEN = { duration: 0.45, easing: "easeOutCubic" } as const;

/** An essay's texts, as its card shows them and as they appear once it's open. */
interface Essay {
  year: string;
  title: string;
  author: string;
  /** The essay's first sentences, from the edition `source` cites. */
  opening: string;
  /** The ellipsis that ends the opening on the card when it runs past five lines. */
  more: string;
  /** An icon from Material Icons after the ellipsis, which puts both in the accent color. */
  moreIcon?: string;
  /** A line from later in the essay. */
  quote: string;
  /**
   * Where the text is from, in parts that alternate between plain text and titles, which are
   * italic. The card clips it to two lines, and the essay shows it in full.
   */
  source: string[];
}

/** The mark that ties a pull quote to its source, a dagger, raised as a footnote mark is. */
const FOOTNOTE_MARK: SpanSpec = { content: "\u2020", verticalAlign: "super" };

/** Returns an essay's source as spans, with its titles in italic. */
function sourceSpans(essay: Essay): SpanSpec[] {
  return essay.source.map((content, i) => (i % 2 === 1 ? { content, italic: true } : { content }));
}

const ESSAYS: Essay[] = [
  {
    year: "1792",
    title: "A Vindication of the Rights of Woman: with Strictures on Political and Moral Subjects",
    author: "Mary Wollstonecraft",
    opening:
      "After considering the historic page, and viewing the living world with anxious " +
      "solicitude, the most melancholy emotions of sorrowful indignation have depressed my " +
      "spirits, and I have sighed when obliged to confess, that either nature has made a great " +
      "difference between man and man, or that the civilization, which has hitherto taken place " +
      "in the world, has been very partial.",
    more: "…",
    quote: "“I do not wish them to have power over men; but over themselves.”",
    source: [
      "From ",
      "A Vindication of the Rights of Woman",
      " by Mary Wollstonecraft (1792), Project Gutenberg eBook #3420. Public domain.",
    ],
  },
  {
    year: "1862",
    title: "Walking",
    author: "Henry David Thoreau",
    opening:
      "I wish to speak a word for Nature, for absolute Freedom and Wildness, as contrasted with " +
      "a freedom and culture merely civil,—to regard man as an inhabitant, or a part and " +
      "parcel of Nature, rather than a member of society. I wish to make an extreme statement, " +
      "if so I may make an emphatic one, for there are enough champions of civilization: the " +
      "minister and the school committee and every one of you will take care of that.",
    more: " read more ",
    moreIcon: "arrow_forward",
    quote: "“In Wildness is the preservation of the World.”",
    source: ["From ", "Walking", " by Henry David Thoreau (1862), Project Gutenberg eBook #1022. Public domain."],
  },
  {
    year: "1877",
    title: "An Apology for Idlers",
    author: "Robert Louis Stevenson",
    opening:
      "Just now, when every one is bound, under pain of a decree in absence convicting them of " +
      "lèse-respectability, to enter on some lucrative profession, and labour therein with " +
      "something not far short of enthusiasm, a cry from the opposite party who are content " +
      "when they have enough, and like to look on and enjoy in the meanwhile, savours a little " +
      "of bravado and gasconade. And yet this should not be.",
    more: " — continue reading the whole essay inside",
    quote:
      "“Extreme busyness, whether at school or college, kirk or market, is a symptom of " +
      "deficient vitality.”",
    source: [
      "From ",
      "An Apology for Idlers",
      " by Robert Louis Stevenson (1877), in ",
      "Virginibus Puerisque",
      ", Project Gutenberg eBook #386. Public domain.",
    ],
  },
  {
    year: "1884",
    title: "भारतवर्ष की उन्नति कैसे हो सकती है?",
    author: "भारतेन्दु हरिश्चंद्र",
    // This essay is in Devanagari, which none of the essays' fonts has, so the
    // fallback font, Noto Serif Devanagari, draws every character.
    opening: "आज बड़े ही आनंद का दिन है कि इस छोटे से नगर बलिया में हम इतने मनुष्यों को बड़े उत्साह से एक स्थान पर देखते हैं। इस अभागे आलसी देश में जो कुछ हो जाय वही बहुत कुछ है। बनारस ऐसे ऐसे बड़े नगरों में जब कुछ नहीं होता तो यह हम क्यों न कहैंगे कि बलिया में जो कुछ हमने देखा वह बहुत ही प्रशंसा के योग्य है।",
    more: " आगे पढ़ें",
    quote: "\u201cहमारे हिंदुस्तानी लोग तो रेल की गाड़ी हैं।\u201d",
    source: [
      "From ",
      "भारतवर्ष की उन्नति कैसे हो सकती है?",
      " by Bharatendu Harishchandra (1884), in ",
      "Bharatendu Samagra",
      " (1987), Hindi Wikisource. Public domain.",
    ],
  },
];

/**
 * The family the essays are set in, from two files in `fonts/`. They're from
 * Google Fonts under the SIL Open Font License, in `fonts/OFL-Lora.txt`.
 */
const LORA = "Lora";
/**
 * The second family the body can be set in. It's from Google Fonts under the
 * SIL Open Font License, in `fonts/OFL-CrimsonPro.txt`. The app downloads it
 * when it's first chosen. It has no italic file, so its italic is slanted.
 */
const CRIMSON = "Crimson Pro";
/**
 * The toolbar's icon font, Google's Material Icons. Its ligatures turn an
 * icon's name, such as "format_bold", into its glyph. It's under the Apache
 * License 2.0, in `fonts/LICENSE-MaterialIcons.txt`.
 */
const ICONS = "Material Icons";
/**
 * The fallback font for the Hindi essay, Noto Serif Devanagari. It's from Google
 * Fonts under the SIL Open Font License, in `fonts/OFL-NotoSerifDevanagari.txt`.
 */
const DEVANAGARI = "Noto Serif Devanagari";

/** Downloads a font file from the example's `fonts/` folder. */
function fontFile(file: string): Promise<ArrayBuffer> {
  return fetch(new URL(`../../fonts/${file}`, import.meta.url)).then((r) =>
    r.ok ? r.arrayBuffer() : Promise.reject(new Error(`${file}: ${r.status}`)),
  );
}

const CARD_WIDTH = 245;
const CARD_HEIGHT = 300;
const CARD_Y = -20;
const CARD_SPACING = 265;
const CARD_PADDING = 20;
const CARD_COLUMN = CARD_WIDTH - 2 * CARD_PADDING;

const PANEL_WIDTH = 960;
const PANEL_HEIGHT = 600;
const PANEL_Y = -40;
const PANEL_PADDING = 40;
const PANEL_COLUMN = PANEL_WIDTH - 2 * PANEL_PADDING;

/** How far the title sits from the canvas's left edge, and the toolbar from its right edge. */
const MARGIN = 40;
const TITLE_TOP = 362;
/** The middle of the title's line, where the toolbar is centered. */
const BAR_Y = TITLE_TOP - 19;
const TOOL_WIDTH = 34;
const TOOL_HEIGHT = 30;
/** The gap between a group's buttons, and between groups. */
const TOOL_GAP = 4;
const GROUP_GAP = 14;
/** The room each side of a button's label. */
const BUTTON_PADDING = 20;

/** The body's sizes. Each pair is the size of a button's icon, then the size of the body's text. */
const SIZES = [
  [14, 15],
  [18, 17],
  [22, 20],
] as const;
/** The body's alignments, with their icons. */
const ALIGNS = [
  ["format_align_left", "start"],
  ["format_align_center", "center"],
  ["format_align_right", "end"],
] as const;
/** The body's line heights, with their icons. */
const SPACINGS = [
  ["density_small", 1.2],
  ["density_medium", 1.5],
  ["density_large", 1.8],
] as const;
/** The families the body can be set in. */
const FAMILIES = [LORA, CRIMSON];

/** The toolbar's choices for how the body is set. */
interface Settings {
  size: number;
  align: number;
  bold: boolean;
  italic: boolean;
  spacing: number;
  family: number;
}

/** Returns the geometry of a box of the given size at `(x, y, z)`, placed by `anchor`. */
function box(
  x: number,
  y: number,
  z: number,
  width: number,
  height: number,
  color: Color,
  cornerRadius = 0,
  anchor = { x: 0.5, y: 0.5 },
): Geometry {
  return {
    position: { x, y, z },
    size: { width, height },
    rotation: 0,
    scale: 1,
    anchor,
    color,
    cornerRadius,
  };
}

/**
 * Creates a component that only shows `text`, with the text's top-left corner
 * at `(x, y)`, relative to the parent it's given to.
 */
function textAt(x: number, y: number, text: TextSpec, visible = true): ComponentSpec {
  return {
    geometry: box(x, y, 0, 1, 1, CLEAR, 0, { x: 0, y: 0 }),
    nonInteractive: true,
    visible,
    text,
  };
}

/** Returns the text on a button. */
function buttonLabel(label: string): TextSpec {
  return { content: label, sizePx: 14, color: WHITE };
}

/** Creates a small button showing `label`, which turns lighter on hover. */
function button(x: number, y: number, width: number, height: number, label: string, visible = true): ComponentSpec {
  return {
    geometry: box(x, y, 0, width, height, BUTTON, height / 2),
    hover: { color: BUTTON_HOVER },
    pressed: { scale: 0.96 },
    visible,
    text: buttonLabel(label),
  };
}

/** Creates a toolbar button at `x` showing `label`, which is an icon or "Aa" in a family. */
function tool(x: number, label: TextSpec): ComponentSpec {
  return {
    geometry: box(x, BAR_Y, 0, TOOL_WIDTH, TOOL_HEIGHT, BUTTON, 8),
    hover: { color: BUTTON_HOVER },
    pressed: { scale: 0.94 },
    visible: false,
    text: { ...label, color: WHITE },
  };
}

/** Returns an icon from Material Icons, by name, at `sizePx`. */
function icon(name: string, sizePx: number): TextSpec {
  return { content: name, sizePx, family: ICONS };
}

/**
 * Marks `button` as on, in the accent color, or off. An on button has no hover
 * style, so the pointer leaves it as it is. An off button gets its hover style
 * back.
 */
function mark(button: Handle, on: boolean): void {
  button.setStyle("hover", on ? undefined : { color: BUTTON_HOVER });
  const geometry = button.declaredGeometry();
  if (geometry) button.setDeclaredGeometry({ ...geometry, color: on ? ACCENT : BUTTON });
}

/** Marks the `chosen` button of a group as on, and the rest as off. */
function select(buttons: Handle[], chosen: number): void {
  buttons.forEach((b, i) => mark(b, i === chosen));
}

/**
 * Returns how the opening on `essay`'s card ends: in its ellipsis, in the opening's own style,
 * or, with an icon, in bold in the accent color with the icon after it.
 */
function cardEllipsis(essay: Essay): string | SpanSpec[] {
  if (essay.moreIcon === undefined) {
    return essay.more;
  }
  return [
    { content: essay.more, weight: 700, color: ACCENT },
    // An icon sits high beside text, since the icon font centers it between its ascender and
    // descender. `"middle"` centers it on the text's lowercase letters instead.
    { content: essay.moreIcon, family: ICONS, color: ACCENT, verticalAlign: "middle" },
  ];
}

/** Creates a card's texts for `essay`, in reading order. */
function cardTexts(app: ProteusApp, essay: Essay): Handle[] {
  const left = -CARD_WIDTH / 2 + CARD_PADDING;
  const top = CARD_HEIGHT / 2 - CARD_PADDING;
  const specs: ComponentSpec[] = [
    // The year is aligned to the end of the column, so it lines up with the card's right edge.
    textAt(left, top, { content: essay.year, sizePx: 13, weight: 500, color: MUTED, wrapWidth: CARD_COLUMN, align: "end" }),
    // The title takes at most two lines and ends in the default "…".
    textAt(left, top - 24, { content: essay.title, sizePx: 22, weight: 800, color: INK, wrapWidth: CARD_COLUMN, maxLines: 2 }),
    // The opening takes at most five lines and ends in the card's own ellipsis. It sits below
    // two lines of title in any of the scripts, and Hindi's is the tallest.
    textAt(left, top - 104, {
      content: essay.opening,
      family: LORA,
      sizePx: 15,
      color: INK,
      wrapWidth: CARD_COLUMN,
      lineHeight: 1.45,
      maxLines: 5,
      overflow: { ellipsis: cardEllipsis(essay) },
    }),
    // The source takes at most two lines at the foot of the card, and is clipped with no mark.
    textAt(left, -CARD_HEIGHT / 2 + CARD_PADDING + 30, {
      spans: sourceSpans(essay),
      sizePx: 12,
      color: MUTED,
      wrapWidth: CARD_COLUMN,
      maxLines: 2,
      overflow: "clip",
    }),
  ];
  return specs.map((spec) => app.component(spec));
}

function main(app: ProteusApp, canvasWidth: number): void {
  // Lora's two files, upright and italic, and the icons are downloaded. Lora's files are one
  // family. Texts in Lora or the icons aren't drawn until their font arrives.
  for (const file of ["Lora-Variable.ttf", "Lora-Italic-Variable.ttf"]) {
    app.loadFont(fontFile(file), { family: LORA }).catch((e: unknown) => console.warn(e));
  }
  app.loadFont(fontFile("MaterialIcons-Regular.ttf"), { family: ICONS }).catch((e: unknown) => console.warn(e));
  // None of the other fonts has Hindi. A character a text's font lacks is looked for in the
  // fallback font, then in the default font. Texts wait for the fallback font, as they do for
  // Lora.
  app.loadFont(fontFile("NotoSerifDevanagari-Variable.ttf"), { family: DEVANAGARI }).catch((e: unknown) =>
    console.warn(e),
  );
  app.setFallbackFonts([DEVANAGARI]);
  // Crimson Pro isn't downloaded until it's first chosen, and the promise is settled then.
  // Until then, texts in it are drawn in the default font, and they're drawn in Crimson Pro
  // once it loads.
  let fetchCrimson = () => {};
  const crimson = new Promise<ArrayBuffer>((resolve, reject) => {
    fetchCrimson = () => {
      fetchCrimson = () => {};
      fontFile("CrimsonPro-Variable.ttf").then(resolve, reject);
    };
  });
  app.loadFont(crimson, { family: CRIMSON, whileLoading: "defaultFont" }).catch((e: unknown) => console.warn(e));

  // The title sits a margin from the canvas's left edge. The toolbar sits on the title's line,
  // a margin from the canvas's right edge.
  app.component(textAt(-canvasWidth / 2 + MARGIN, TITLE_TOP, { content: "Essays", sizePx: 30, color: WHITE }));

  // The toolbar's groups run left to right, and each button's label is made from the button's
  // index in its group. The toolbar is hidden until an essay is open, since it changes only the
  // essay. Bold and italic sit together, as one group.
  const groups: [number, (i: number) => TextSpec, number][] = [
    [SIZES.length, (i) => icon("format_size", SIZES[i][0]), GROUP_GAP],
    [ALIGNS.length, (i) => icon(ALIGNS[i][0], 20), GROUP_GAP],
    [1, () => icon("format_bold", 20), TOOL_GAP],
    [1, () => icon("format_italic", 20), GROUP_GAP],
    [SPACINGS.length, (i) => icon(SPACINGS[i][0], 20), GROUP_GAP],
    [FAMILIES.length, (i) => ({ content: "Aa", sizePx: 17, family: FAMILIES[i] }), 0],
  ];
  const widthOf = (n: number) => n * TOOL_WIDTH + (n - 1) * TOOL_GAP;
  const barWidth = groups.reduce((sum, [n, , gap]) => sum + widthOf(n) + gap, 0);
  let x = canvasWidth / 2 - MARGIN - barWidth + TOOL_WIDTH / 2;
  const made = groups.map(([count, label, gap]) => {
    const group: Handle[] = [];
    for (let i = 0; i < count; i++) {
      group.push(app.component(tool(x, label(i))));
      x += TOOL_WIDTH + TOOL_GAP;
    }
    x += gap - TOOL_GAP;
    return group;
  });
  const [sizeButtons, alignButtons, [boldButton], [italicButton], spacingButtons, familyButtons] = made;
  const toolbar = made.flat();

  // One card for each essay, with its texts as children.
  const cards: Handle[] = [];
  const cardContents: Handle[][] = [];
  ESSAYS.forEach((essay, i) => {
    const texts = cardTexts(app, essay);
    cards.push(
      app.component({
        geometry: box((i - (ESSAYS.length - 1) / 2) * CARD_SPACING, CARD_Y, 0, CARD_WIDTH, CARD_HEIGHT, PAPER, 18),
        hover: { scale: 1.03 },
        children: texts.map((h) => h.id()),
      }),
    );
    cardContents.push(texts);
  });

  // The essay panel sits over the cards. It takes the pointer, so a click on the open essay
  // doesn't reach the cards under it. It doesn't take the keyboard, so it's never a stop for
  // focus.
  const left = -PANEL_WIDTH / 2 + PANEL_PADDING;
  const top = PANEL_HEIGHT / 2 - PANEL_PADDING;
  const hiddenText = (x: number, y: number) => app.component(textAt(x, y, { content: "", sizePx: 16 }, false));
  // The back button fits its label, with room each side. The label is measured before it's
  // drawn, in the default font, which is never still loading.
  const backLabel = "← All essays";
  const labelSize = app.measureText(buttonLabel(backLabel));
  if (!labelSize) throw new Error("the default font is loaded");
  const backWidth = labelSize.width + 2 * BUTTON_PADDING;
  const back = app.component(button(left + backWidth / 2, top - 18, backWidth, 36, backLabel, false));
  const title = hiddenText(left, top - 60);
  const byline = hiddenText(left, top - 150);
  const body = hiddenText(left, top - 200);
  // The quote sits below the tallest body, which is 20 pixels at a line height of 1.8.
  const quote = hiddenText(-300, -150);
  const source = hiddenText(left, -PANEL_HEIGHT / 2 + PANEL_PADDING + 14);
  const essayView = [title, byline, body, quote, source, back];
  const panel = app.component({
    geometry: box(0, PANEL_Y, 1, PANEL_WIDTH, PANEL_HEIGHT, PAPER, 18),
    visible: false,
    devices: ["pointer"],
    children: essayView.map((h) => h.id()),
  });

  const channel = app.transitionChannel();
  /** The index of the essay that is open or opening, if any. */
  let open: number | undefined;
  const settings: Settings = { size: 1, align: 0, bold: false, italic: false, spacing: 1, family: 0 };

  const setVisible = (handles: Handle[], visible: boolean) => handles.forEach((h) => h.setVisible(visible));

  /** Returns the opening of essay `i`, set as the toolbar's choices say. */
  function bodyText(i: number): TextSpec {
    return {
      content: ESSAYS[i].opening,
      family: FAMILIES[settings.family],
      sizePx: SIZES[settings.size][1],
      weight: settings.bold ? 700 : 400,
      italic: settings.italic,
      color: INK,
      wrapWidth: PANEL_COLUMN,
      align: ALIGNS[settings.align][1],
      lineHeight: SPACINGS[settings.spacing][1],
    };
  }

  /** Turns card `i` into the essay panel. */
  function openEssay(i: number): void {
    if (open !== undefined) return;
    open = i;
    const essay = ESSAYS[i];
    title.setText({ content: essay.title, sizePx: 30, weight: 800, color: INK, wrapWidth: PANEL_COLUMN });
    // The byline is two lines, split with a `\n`: the author in a heavier weight, and the year
    // smaller and muted.
    byline.setText({
      spans: [
        { content: essay.author, weight: 600 },
        { content: `\n${essay.year}`, sizePx: 12, color: MUTED },
      ],
      sizePx: 14,
      color: INK,
      italic: true,
    });
    body.setText(bodyText(i));
    quote.setText({
      spans: [{ content: essay.quote }, { ...FOOTNOTE_MARK, sizePx: 14 }],
      sizePx: 20,
      family: LORA,
      italic: true,
      color: ACCENT,
      wrapWidth: 600,
      align: "center",
    });
    source.setText({
      spans: [FOOTNOTE_MARK, { content: " " }, ...sourceSpans(essay)],
      sizePx: 12,
      color: MUTED,
      wrapWidth: PANEL_COLUMN,
    });
    setVisible(cardContents[i], false);
    channel.set(panel, cards[i], OPEN, true);
  }

  /** Turns the essay panel back into the card it came from. */
  function closeEssay(): void {
    if (open === undefined) return;
    const i = open;
    open = undefined;
    setVisible(essayView, false);
    setVisible(toolbar, false);
    channel.set(cards[i], panel, OPEN, true);
  }

  /** Applies the toolbar's choices to its buttons and the open body. */
  function apply(): void {
    select(sizeButtons, settings.size);
    select(alignButtons, settings.align);
    mark(boldButton, settings.bold);
    mark(italicButton, settings.italic);
    select(spacingButtons, settings.spacing);
    select(familyButtons, settings.family);
    if (FAMILIES[settings.family] === CRIMSON) fetchCrimson();
    if (open !== undefined) body.setText(bodyText(open));
  }
  apply();

  cards.forEach((card, i) => {
    card.onClick(() => openEssay(i));
    // When the card returns from the essay, its texts come back once the card is whole.
    card.onTransitionComplete(() => setVisible(cardContents[i], true));
  });
  panel.onTransitionComplete(() => {
    setVisible(essayView, true);
    setVisible(toolbar, true);
  });
  back.onClick(closeEssay);
  // Each button changes one setting.
  const on = (button: Handle, change: () => void) =>
    button.onClick(() => {
      change();
      apply();
    });
  sizeButtons.forEach((b, i) => on(b, () => (settings.size = i)));
  alignButtons.forEach((b, i) => on(b, () => (settings.align = i)));
  on(boldButton, () => (settings.bold = !settings.bold));
  on(italicButton, () => (settings.italic = !settings.italic));
  spacingButtons.forEach((b, i) => on(b, () => (settings.spacing = i)));
  familyButtons.forEach((b, i) => on(b, () => (settings.family = i)));
}

const canvas = document.getElementById("app") as HTMLCanvasElement;
await mount("app", {
  setup: (app) => main(app, canvas.clientWidth),
  config: {
    render: { clearColor: [BACKGROUND.r, BACKGROUND.g, BACKGROUND.b, 1] },
    input: { autofocus: true },
  },
});
