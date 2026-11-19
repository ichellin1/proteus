/**
 * Proteus TypeScript SDK example: a reading app, the same app as the Rust
 * essays example.
 *
 * A row of cards, one for each of three public-domain essays. Each card's
 * year is aligned to the end of its column; its title wraps onto at most two
 * lines, ending in "…"; its opening is spaced at a line height of 1.45 and
 * stops after five lines, ending in an ellipsis of the card's own; and its
 * source is clipped after two lines. A card opens into its essay (1→1), whose
 * byline is two lines split with a `\n`, whose opening wraps at the column's
 * width, and whose pull quote is centered. While an essay is open, the
 * buttons at the top change its alignment and line height.
 *
 * The essays are Mary Wollstonecraft's "A Vindication of the Rights of Woman"
 * (1792), Henry David Thoreau's "Walking" (1862) and Robert Louis Stevenson's
 * "An Apology for Idlers" (1877), from their Project Gutenberg editions, each
 * cited where it's shown.
 *
 * Techniques worth copying:
 *   - Text is placed by its component's anchor: with an anchor of (0, 0), the
 *     text's top-left corner is at the component's position. A card's texts
 *     are small children, placed from the card's top-left corner.
 *   - A wrap width is the text's box: lines align within it, and the box
 *     doesn't change size when the words do, so the year, aligned to the end,
 *     lines up with the card's right edge whatever it says.
 *   - An ellipsis wider than a line takes lines of its own: the third card's
 *     opening ends in a whole sentence.
 *   - Texts are hidden while their card or the essay changes shape, and shown
 *     when it arrives, since children move with their parent but don't
 *     stretch with it.
 *   - Changing a text's alignment or line height means setting it again:
 *     `setText` replaces it, and the host lays it out again.
 *
 * Not shown here: fonts and weights, other scripts, mixed styles within a
 * text, measuring text, and components within a line of text.
 */

import { colorFrom, mount } from "proteus-sdk";
import type { Color, ComponentSpec, Geometry, Handle, ProteusApp, TextSpec } from "proteus-sdk";

const BACKGROUND = colorFrom("#1d1b26");
const PAPER = colorFrom("#f4f1ea");
const INK = colorFrom("#24212b");
const MUTED = colorFrom("#6b6676");
const ACCENT = colorFrom("#7a5fb0");
/** A button, light enough to stand out from the background. */
const BUTTON = colorFrom("#4d4a58");
const BUTTON_HOVER = colorFrom("#5f5c6a");
const WHITE = colorFrom("#ffffff");
const CLEAR: Color = { r: 0, g: 0, b: 0, a: 0 };

const OPEN = { duration: 0.45, easing: "easeOutCubic" } as const;

/** An essay: what its card shows, and what it opens into. */
interface Essay {
  year: string;
  title: string;
  author: string;
  /** Its first sentences, from its Project Gutenberg edition. */
  opening: string;
  /** What ends the opening on the card when it runs past five lines. */
  more: string;
  /** A line from later in the essay. */
  quote: string;
  /** Where the text is from: two lines on the card, in full in the essay. */
  source: string;
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
    source:
      "From “A Vindication of the Rights of Woman” by Mary Wollstonecraft (1792), " +
      "Project Gutenberg eBook #3420. Public domain.",
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
    more: " read more",
    quote: "“In Wildness is the preservation of the World.”",
    source: "From “Walking” by Henry David Thoreau (1862), Project Gutenberg eBook #1022. Public domain.",
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
    source:
      "From “An Apology for Idlers” by Robert Louis Stevenson (1877), in " +
      "“Virginibus Puerisque”, Project Gutenberg eBook #386. Public domain.",
  },
];

const CARD_WIDTH = 320;
const CARD_HEIGHT = 300;
const CARD_Y = -20;
const CARD_SPACING = 350;
const CARD_PADDING = 20;
const CARD_COLUMN = CARD_WIDTH - 2 * CARD_PADDING;

const PANEL_WIDTH = 960;
const PANEL_HEIGHT = 600;
const PANEL_Y = -40;
const PANEL_PADDING = 40;
const PANEL_COLUMN = PANEL_WIDTH - 2 * PANEL_PADDING;

/** How far the title sits from the canvas's left edge, and the settings bar from its right edge. */
const MARGIN = 40;
const TITLE_TOP = 362;
/** The middle of the title's line, where the settings bar is centered. */
const BAR_Y = TITLE_TOP - 19;
const SETTING_WIDTH = 64;
const SETTING_HEIGHT = 30;
/** From one setting's button to the next. */
const SETTING_STEP = SETTING_WIDTH + 6;
/** The widths of the settings' labels, and the gap between the two settings. */
const ALIGN_LABEL = 50;
const LINE_HEIGHT_LABEL = 90;
const SETTINGS_GAP = 24;
/** The settings bar's width: each label and its three buttons, and the gap. */
const BAR_WIDTH = ALIGN_LABEL + LINE_HEIGHT_LABEL + 2 * (2 * SETTING_STEP + SETTING_WIDTH) + SETTINGS_GAP;

/** The alignments the body can take, with their buttons' labels. */
const ALIGNS = [
  ["Start", "start"],
  ["Center", "center"],
  ["End", "end"],
] as const;
/** The line heights the body can take. */
const LINE_HEIGHTS = [1.2, 1.5, 1.8];

/** Geometry for a box of the given size at `(x, y, z)`, placed by `anchor`. */
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
 * A component that only shows `text`, with the text's top-left corner at
 * `(x, y)`, relative to the parent it's given to.
 */
function textAt(x: number, y: number, text: TextSpec, visible = true): ComponentSpec {
  return {
    geometry: box(x, y, 0, 1, 1, CLEAR, 0, { x: 0, y: 0 }),
    nonInteractive: true,
    visible,
    text,
  };
}

/** A small button, with its label, lighter on hover. */
function button(x: number, y: number, width: number, height: number, label: string, visible = true): ComponentSpec {
  return {
    geometry: box(x, y, 0, width, height, BUTTON, height / 2),
    hover: { color: BUTTON_HOVER },
    pressed: { scale: 0.96 },
    visible,
    text: { content: label, sizePx: 14, color: WHITE },
  };
}

/**
 * A setting's button: shown in the accent color, and left alone by the
 * pointer, while disabled, which is how the chosen one is marked.
 */
function settingButton(x: number, label: string): ComponentSpec {
  return {
    ...button(x, BAR_Y, SETTING_WIDTH, SETTING_HEIGHT, label, false),
    disabled: { color: ACCENT },
  };
}

/** A card's texts, in reading order. */
function cardTexts(app: ProteusApp, essay: Essay): Handle[] {
  const left = -CARD_WIDTH / 2 + CARD_PADDING;
  const top = CARD_HEIGHT / 2 - CARD_PADDING;
  const specs: ComponentSpec[] = [
    // Aligned to the end of the column, so it lines up with the card's right edge.
    textAt(left, top, { content: essay.year, sizePx: 13, color: MUTED, wrapWidth: CARD_COLUMN, align: "end" }),
    // At most two lines, ending in the default "…".
    textAt(left, top - 24, { content: essay.title, sizePx: 22, color: INK, wrapWidth: CARD_COLUMN, maxLines: 2 }),
    // At most five lines, ending in the card's own ellipsis.
    textAt(left, top - 86, {
      content: essay.opening,
      sizePx: 15,
      color: INK,
      wrapWidth: CARD_COLUMN,
      lineHeight: 1.45,
      maxLines: 5,
      overflow: { ellipsis: essay.more },
    }),
    // At most two lines, at the foot of the card, clipped with no mark.
    textAt(left, -CARD_HEIGHT / 2 + CARD_PADDING + 30, {
      content: essay.source,
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
  // The title sits a margin from the canvas's left edge, and the settings bar,
  // on the title's line, a margin from its right edge.
  app.component(textAt(-canvasWidth / 2 + MARGIN, TITLE_TOP, { content: "Essays", sizePx: 30, color: WHITE }));

  // The settings bar: a label, then its buttons, for each setting. Hidden
  // until an essay is open, since it changes only the essay.
  const bar: Handle[] = [];
  function row(label: string, x: number, labelWidth: number, labels: string[]): Handle[] {
    bar.push(
      app.component(textAt(x, BAR_Y + 9, { content: label, sizePx: 14, color: WHITE, wrapWidth: labelWidth }, false)),
    );
    const buttons = labels.map((text, i) =>
      app.component(settingButton(x + labelWidth + SETTING_WIDTH / 2 + i * SETTING_STEP, text)),
    );
    bar.push(...buttons);
    return buttons;
  }
  const barLeft = canvasWidth / 2 - MARGIN - BAR_WIDTH;
  const alignButtons = row(
    "Align",
    barLeft,
    ALIGN_LABEL,
    ALIGNS.map(([label]) => label),
  );
  const afterAlign = barLeft + ALIGN_LABEL + 2 * SETTING_STEP + SETTING_WIDTH + SETTINGS_GAP;
  const lineHeightButtons = row(
    "Line height",
    afterAlign,
    LINE_HEIGHT_LABEL,
    LINE_HEIGHTS.map((h) => String(h)),
  );

  // A card for each essay, with its texts as children.
  const cards: Handle[] = [];
  const cardContents: Handle[][] = [];
  ESSAYS.forEach((essay, i) => {
    const texts = cardTexts(app, essay);
    cards.push(
      app.component({
        geometry: box((i - 1) * CARD_SPACING, CARD_Y, 0, CARD_WIDTH, CARD_HEIGHT, PAPER, 18),
        hover: { scale: 1.03 },
        children: texts.map((h) => h.id()),
      }),
    );
    cardContents.push(texts);
  });

  // The essay panel, over the cards. It takes the pointer, so a click on the
  // open essay doesn't reach the cards under it, but not the keyboard, so it's
  // never a stop for focus.
  const left = -PANEL_WIDTH / 2 + PANEL_PADDING;
  const top = PANEL_HEIGHT / 2 - PANEL_PADDING;
  const hiddenText = (x: number, y: number) => app.component(textAt(x, y, { content: "", sizePx: 16 }, false));
  const back = app.component(button(left + 85, top - 18, 170, 36, "← All essays", false));
  const title = hiddenText(left, top - 60);
  const byline = hiddenText(left, top - 150);
  const body = hiddenText(left, top - 200);
  const quote = hiddenText(-300, -120);
  const source = hiddenText(left, -PANEL_HEIGHT / 2 + PANEL_PADDING + 14);
  const essayView = [title, byline, body, quote, source, back];
  const panel = app.component({
    geometry: box(0, PANEL_Y, 1, PANEL_WIDTH, PANEL_HEIGHT, PAPER, 18),
    visible: false,
    devices: ["pointer"],
    children: essayView.map((h) => h.id()),
  });

  const channel = app.transitionChannel();
  /** The essay open, or opening. */
  let open: number | undefined;
  let alignChoice = 0;
  let lineHeightChoice = 1;

  const setVisible = (handles: Handle[], visible: boolean) => handles.forEach((h) => h.setVisible(visible));

  /** The opening of essay `i`, set as the settings bar says. */
  function bodyText(i: number): TextSpec {
    return {
      content: ESSAYS[i].opening,
      sizePx: 17,
      color: INK,
      wrapWidth: PANEL_COLUMN,
      align: ALIGNS[alignChoice][1],
      lineHeight: LINE_HEIGHTS[lineHeightChoice],
    };
  }

  /** Turns card `i` into the essay panel. */
  function openEssay(i: number): void {
    if (open !== undefined) return;
    open = i;
    const essay = ESSAYS[i];
    title.setText({ content: essay.title, sizePx: 30, color: INK, wrapWidth: PANEL_COLUMN });
    // Two lines, split with a `\n`.
    byline.setText({ content: `${essay.author}\n${essay.year}`, sizePx: 14, color: MUTED });
    body.setText(bodyText(i));
    quote.setText({ content: essay.quote, sizePx: 20, color: ACCENT, wrapWidth: 600, align: "center" });
    source.setText({ content: essay.source, sizePx: 12, color: MUTED, wrapWidth: PANEL_COLUMN });
    setVisible(cardContents[i], false);
    channel.set(panel, cards[i], OPEN, true);
  }

  /** Turns the essay panel back into the card it came from. */
  function closeEssay(): void {
    if (open === undefined) return;
    const i = open;
    open = undefined;
    setVisible(essayView, false);
    setVisible(bar, false);
    channel.set(cards[i], panel, OPEN, true);
  }

  /**
   * Marks the `chosen` button of a row by disabling it, so it shows the accent
   * color, and hovering or clicking it does nothing, since it's chosen already.
   * The rest are enabled.
   */
  function select(buttons: Handle[], chosen: number): void {
    buttons.forEach((b, i) => b.setDisabled(i === chosen));
  }

  /** Applies the settings bar's choices to its buttons and the open body. */
  function apply(): void {
    select(alignButtons, alignChoice);
    select(lineHeightButtons, lineHeightChoice);
    if (open !== undefined) body.setText(bodyText(open));
  }
  apply();

  cards.forEach((card, i) => {
    card.onClick(() => openEssay(i));
    // Back from the essay: the card's texts return once it's whole.
    card.onTransitionComplete(() => setVisible(cardContents[i], true));
  });
  panel.onTransitionComplete(() => {
    setVisible(essayView, true);
    setVisible(bar, true);
  });
  back.onClick(closeEssay);
  alignButtons.forEach((b, i) =>
    b.onClick(() => {
      alignChoice = i;
      apply();
    }),
  );
  lineHeightButtons.forEach((b, i) =>
    b.onClick(() => {
      lineHeightChoice = i;
      apply();
    }),
  );
}

const canvas = document.getElementById("app") as HTMLCanvasElement;
await mount("app", {
  setup: (app) => main(app, canvas.clientWidth),
  config: {
    render: { clearColor: [BACKGROUND.r, BACKGROUND.g, BACKGROUND.b, 1] },
    input: { autofocus: true },
  },
});
