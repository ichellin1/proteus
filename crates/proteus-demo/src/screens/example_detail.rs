//! `ExampleDetail` — reached from `screens::examples_home`'s category grid.
//! One shared panel entity, holding one of 6 categories' content at a time:
//! Effects, Text, Transforms & Animation (static labelled rows demonstrating
//! `DropShadow`/`Glow`/`Border`/`Opacity`, `Text` styling, and
//! rotation/scale/continuous animation), Stress Tests (Burst Spawn /
//! Texture Churn — dynamic runtime entity spawn/despawn, a different
//! pattern from the other three's static content; the actual spawn/despawn
//! and RNG live in `Demo`, not here, since they need `&mut Demo` state —
//! this module just spawns the two buttons/result/warning text and lays
//! them out), and Layout/3D (not built yet — `placeholder_message` says so
//! rather than leaving the panel blank).
//!
//! ## Standalone content, not `ChildOf` the panel
//!
//! The obvious shape — parent each category's rows `ChildOf(panel)` — forces
//! a reparenting dance: only the *active* category can be attached when a
//! merge starts, because the merge's bake step walks the panel's subtree
//! ignoring `Visibility`, so any other category's rows would leak into the
//! crossfade snapshot. Nothing here needs that workaround: content is
//! spawned as ordinary standalone entities, positioned once in *absolute*
//! world coordinates from the panel's known target rect (the panel's own
//! `QuadState` never changes once a merge starts — it's the fixed
//! destination the sources converge onto), and simply hidden/revealed by
//! `Demo`'s own timer-based reveal (mirrors `screens::home`'s labels) —
//! no reparenting, no risk of a bake ever seeing the wrong category.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{Border, ComponentSpec, DropShadow, Glow, Handle, Proteus, QuadState, Text};

use super::examples_home::CATEGORY_TITLES;

pub(crate) const PANEL_PADDING_PX: f32 = 24.0;
const HEADING_PANEL_GAP_PX: f32 = 20.0;
/// Local Z for all row content — above the panel's own 0.5 so it renders on
/// top of the panel; headings use 0.51 for the same reason.
pub(crate) const CONTENT_Z: f32 = 0.51;

/// `panel`'s theme-blend target in `Demo::advance_theme`. Unlike
/// `screens::home`'s nav-button pair, this one's light/dark values are
/// genuinely different.
pub const CORNER_RADIUS: f32 = 12.0;
/// Dark-theme counterpart of [`CORNER_RADIUS`] — genuinely different here.
pub const CORNER_RADIUS_DARK: f32 = 18.0;
const BORDER_WIDTH: f32 = 3.0;
/// Neutral grey (not violet-tinted like everything else in this demo) —
/// deliberately opaque and desaturated, so the effect/text/transform
/// examples sitting on top of it (many close to white) read clearly against
/// it instead of blending into the app's own light background. Unlike every
/// other themed color in this crate, this one does *not* lerp to a darker
/// variant in dark mode (`Demo::advance_theme` never touches `panel`'s own
/// `color`, only its `Border`/corner radius) — a darkened panel read as
/// near-black, at odds with the rest of the demo's dark-mode surfaces; this
/// backdrop stays the same light grey in both themes.
const LIGHT_GREY: Vec4 = Vec4::new(0.82, 0.82, 0.83, 1.0);
/// A non-brand accent color, used only for the "different color" examples
/// in the Glow/Border rows and the Text screen's Color row — everything
/// else in this demo is violet.
const ACCENT: Vec4 = Vec4::new(0.95, 0.55, 0.25, 1.0);
fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

/// Vertical space reserved at the top of the viewport so the panel never
/// overlaps `screens::nav`'s buttons. Sized for this crate's own (larger,
/// placeholder) nav buttons, not for a bare icon row.
const TOP_CLEARANCE_PX: f32 = 110.0;
/// Vertical space reserved at the bottom of the viewport, for the Stress
/// Tests panel's own bottom edge.
const BOTTOM_CLEARANCE_PX: f32 = 40.0;

/// Fixed content height per category, indexed by category (Effects, Text,
/// Transforms & Animation). Category 3 (Stress Tests) isn't fixed-height — its panel
/// stretches to the bottom clearance instead (see `stress_panel_target`).
const CONTENT_HEIGHT: [f32; 3] = [
    3.0 * 90.0 + 64.0 + 18.0 + 18.0, // Effects = 370.0
    2.0 * 90.0 + 56.0,               // Text = 236.0
    2.0 * 120.0 + 64.0 + 2.0 * 20.0, // Transforms = 344.0
];

/// Flat placeholder content height for categories 4/5 (Layout, 3D) — not
/// built yet, just tall enough to hold `placeholder_message`.
const PLACEHOLDER_CONTENT_HEIGHT_PX: f32 = 100.0;

/// Rough line height for the 28px category-title font — used only to stack
/// Stress Tests' title/buttons/panel from the viewport's top edge downward
/// (`stress_panel_target`), since (unlike categories 0–2, where the title
/// floats independently above an already-sized panel) here the panel's own
/// height depends on where the title+buttons end. A live baked-height
/// lookup would need threading `&Proteus` into a spot that's otherwise a
/// pure `viewport_size -> QuadState` function — not worth it for one
/// approximate value on a placeholder-fidelity screen.
const STRESS_HEADING_HEIGHT_PX: f32 = 34.0;
pub(crate) const STRESS_BUTTON_HEIGHT_PX: f32 = 46.0;
const STRESS_BUTTON_HORIZONTAL_PADDING_PX: f32 = 40.0;
const STRESS_BUTTON_GAP_PX: f32 = 24.0;
const STRESS_BUTTON_LABEL_SIZE_PX: f32 = 24.0;
const STRESS_BUTTON_LABEL_LETTER_SPACING_PX: f32 = STRESS_BUTTON_LABEL_SIZE_PX * 0.02;
const STRESS_BUTTON_CORNER_RADIUS: f32 = 20.0;
pub(crate) const RESULT_TEXT_RESERVED_HEIGHT_PX: f32 = 40.0;
pub(crate) const STRESS_TEST_DURATION: f32 = 3.0;
pub(crate) const BURST_SPAWN_COUNT: usize = 1000;
pub(crate) const BURST_SPAWN_ITEM_DURATION: f32 = 0.4;
pub(crate) const BURST_ITEM_SIZE: f32 = 16.0;
pub(crate) const TEXTURE_CHURN_SLOTS: usize = 8;
pub(crate) const TEXTURE_CHURN_SLOT_SIZE: f32 = 80.0;
pub(crate) const TEXTURE_CHURN_COLS: usize = 4;
pub(crate) const TEXTURE_CHURN_GAP_PX: f32 = 30.0;
pub(crate) const TEXTURE_CHURN_SIZE_MIN: f32 = 100.0;
pub(crate) const TEXTURE_CHURN_SIZE_SPREAD: f32 = 300.0;

struct EffectsContent {
    row_labels: [Handle; 4],
    row_boxes: [Vec<Handle>; 4],
    opacity_item_labels: [Handle; 5],
}

struct TextContent {
    row_labels: [Handle; 3],
    row_items: [Vec<Handle>; 3],
}

struct TransformsContent {
    row_labels: [Handle; 3],
    row_boxes: [Vec<Handle>; 3],
    item_labels: [Vec<Handle>; 3],
}

/// The two buttons + result/warning text — the *dynamic* Burst Spawn/
/// Texture Churn entities themselves are spawned/despawned by `Demo` at
/// runtime, not here (see the module doc).
pub struct StressContent {
    pub buttons: [Handle; 2],
    /// `pub` so `Demo::advance_theme` can blend their color alongside
    /// `buttons`' own Border/Glow — see `ExampleDetail::headings`' doc for
    /// why this is one of the few pieces of category content that *does*
    /// get the live theme lerp.
    pub button_labels: [Handle; 2],
    /// Seeded with a single space, not empty — an empty string leaves
    /// `bake_pending_text` with nothing to rasterize, so it never gains a
    /// `BakedText` and gets retried forever. `Demo::finalize_stress_test`
    /// updates `.content` then calls `Handle::free_resources` to force a
    /// re-bake (`Text` doesn't support in-place content changes otherwise —
    /// see `proteus_ui::text`'s own doc).
    pub result_text: Handle,
    pub warning_text: Handle,
}

pub struct ExampleDetail {
    pub panel: Handle,
    pub transforms_continuous_box: Handle,
    pub stress: StressContent,
    /// The 6 category titles — `pub` so `Demo::advance_theme` can blend
    /// their color; unlike each category's own row labels/content, these
    /// alone get the live theme lerp — see `Demo::advance_theme`'s doc for
    /// why the two aren't treated the same. Indices 4/5 (Layout, 3D) get a
    /// heading even though they have no real content, so
    /// `content_handles`/`layout_content` don't need a special case for
    /// "no heading", and because a bare `placeholder_message` with no title
    /// above it read as more broken than intentional once actually on
    /// screen.
    pub headings: [Handle; 6],
    /// Shown only for categories 4/5 — see `content_handles`'s doc.
    pub placeholder_message: Handle,
    effects: EffectsContent,
    text: TextContent,
    transforms: TransformsContent,
}

impl ExampleDetail {
    /// Every entity belonging to category `idx` (0–5) — used to reveal them
    /// together once their merge transition completes, and to hide them
    /// together on the way out. See the module doc for why this is a plain
    /// `Visibility` toggle rather than a `ChildOf` cascade. Excludes
    /// `stress.warning_text`, which `Demo::advance_stress_warning_visibility`
    /// governs continuously on its own finer-grained condition (only when
    /// idle, no test running) rather than the enter/exit timing every other
    /// entity here follows.
    pub fn content_handles(&self, idx: usize) -> Vec<Handle> {
        let mut handles = vec![self.headings[idx]];
        match idx {
            0 => {
                handles.extend(self.effects.row_labels);
                handles.extend(self.effects.row_boxes.iter().flatten().copied());
                handles.extend(self.effects.opacity_item_labels);
            }
            1 => {
                handles.extend(self.text.row_labels);
                handles.extend(self.text.row_items.iter().flatten().copied());
            }
            2 => {
                handles.extend(self.transforms.row_labels);
                handles.extend(self.transforms.row_boxes.iter().flatten().copied());
                handles.extend(self.transforms.item_labels.iter().flatten().copied());
            }
            3 => {
                handles.extend(self.stress.buttons);
                handles.extend(self.stress.button_labels);
                handles.push(self.stress.result_text);
            }
            4 | 5 => {
                handles.push(self.placeholder_message);
            }
            _ => {}
        }
        handles
    }
}

fn heading(app: &mut Proteus, text: &str) -> Handle {
    app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, CONTENT_Z),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            ..Default::default()
        })
        .text(Text::new(text, 28.0).with_color(violet()))
        // Decorative, not a click target — see
        // `ComponentSpec::non_interactive`'s doc.
        .non_interactive(),
    )
}

fn row_label(app: &mut Proteus, text: &str, size_px: f32) -> Handle {
    app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, CONTENT_Z),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            ..Default::default()
        })
        .text(Text::new(text, size_px).with_color(violet()))
        .non_interactive(),
    )
}

fn effect_box(app: &mut Proteus, size: f32) -> Handle {
    app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, CONTENT_Z),
            size: Vec2::splat(size),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: Vec4::ONE,
            corner_radius: 10.0,
        })
        .non_interactive(),
    )
}

pub fn spawn(app: &mut Proteus) -> ExampleDetail {
    let panel = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.5),
            size: Vec2::new(400.0, 300.0),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            // Flat opaque light grey, not this crate's usual transparent
            // fill — see `LIGHT_GREY`'s own doc for why. No `Glow` — this
            // is a backdrop, not a click target.
            color: LIGHT_GREY,
            corner_radius: CORNER_RADIUS,
        })
        .border(Border::new(BORDER_WIDTH, violet()))
        // Backdrop, not a click target — see
        // `ComponentSpec::non_interactive`'s doc.
        .non_interactive(),
    );

    let headings = [
        heading(app, CATEGORY_TITLES[0]),
        heading(app, CATEGORY_TITLES[1]),
        heading(app, CATEGORY_TITLES[2]),
        heading(app, CATEGORY_TITLES[3]),
        heading(app, CATEGORY_TITLES[4]),
        heading(app, CATEGORY_TITLES[5]),
    ];

    // --- Layout / 3D (categories 4/5) — not built yet ---
    // One shared entity (both categories show the same generic message, so
    // there's no need for two near-identical ones) — same "flat, always-
    // there placeholder" idiom as `stress.warning_text`.
    let placeholder_message = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, CONTENT_Z),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            ..Default::default()
        })
        .text(
            Text::new(
                "This example isn't built yet — check back in a future update.",
                20.0,
            )
            .with_color(violet()),
        )
        .non_interactive(),
    );

    // --- Effects (category 0) ---
    let drop_shadow_label = row_label(app, "Drop Shadow", 16.0);
    let drop_shadow_boxes = vec![
        {
            let b = effect_box(app, 64.0);
            app.world_mut().entity_mut(b.id()).insert(DropShadow {
                offset: Vec2::new(2.0, -2.0),
                color: Vec4::new(0.0, 0.0, 0.0, 0.5),
                softness: 4.0,
                spread: 0.0,
            });
            b
        },
        {
            let b = effect_box(app, 64.0);
            app.world_mut().entity_mut(b.id()).insert(DropShadow {
                offset: Vec2::new(8.0, -8.0),
                color: Vec4::new(0.0, 0.0, 0.0, 0.8),
                softness: 2.0,
                spread: 2.0,
            });
            b
        },
        {
            let b = effect_box(app, 61.0);
            app.world_mut().entity_mut(b.id()).insert(DropShadow {
                offset: Vec2::new(6.0, -6.0),
                color: Vec4::new(0.0, 0.0, 0.0, 0.6),
                softness: 6.0,
                spread: 10.0,
            });
            b
        },
    ];
    let glow_label = row_label(app, "Glow", 16.0);
    let glow_boxes = vec![
        {
            let b = effect_box(app, 64.0);
            app.world_mut().entity_mut(b.id()).insert(Glow {
                radius: 6.0,
                color: violet(),
                intensity: 1.0,
            });
            b
        },
        {
            let b = effect_box(app, 64.0);
            app.world_mut().entity_mut(b.id()).insert(Glow {
                radius: 24.0,
                color: violet(),
                intensity: 1.0,
            });
            b
        },
        {
            let b = effect_box(app, 64.0);
            app.world_mut().entity_mut(b.id()).insert(Glow {
                radius: 16.0,
                color: ACCENT,
                intensity: 1.0,
            });
            b
        },
    ];
    let border_label = row_label(app, "Border", 16.0);
    let border_boxes = vec![
        {
            let b = effect_box(app, 64.0);
            app.world_mut()
                .entity_mut(b.id())
                .insert(Border::new(2.0, violet()));
            b
        },
        {
            let b = effect_box(app, 64.0);
            app.world_mut()
                .entity_mut(b.id())
                .insert(Border::new(8.0, violet()));
            b
        },
        {
            let b = effect_box(app, 64.0);
            app.world_mut()
                .entity_mut(b.id())
                .insert(Border::new(4.0, ACCENT));
            b
        },
    ];
    let opacity_label = row_label(app, "Opacity", 16.0);
    let mut opacity_boxes = Vec::with_capacity(5);
    let mut opacity_item_labels = Vec::with_capacity(5);
    for (alpha, caption) in [(0.25, "25%"), (0.5, "50%"), (0.75, "75%"), (1.0, "100%")] {
        let b = app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 0.0, CONTENT_Z),
                size: Vec2::splat(64.0),
                rotation: 0.0,
                scale: 1.0,
                anchor: Vec2::new(0.5, 0.5),
                color: Vec4::new(1.0, 1.0, 1.0, alpha),
                corner_radius: 10.0,
            })
            .non_interactive(),
        );
        opacity_boxes.push(b);
        opacity_item_labels.push(row_label(app, caption, 12.0));
    }
    // Nested Opacity cascade: parent 0.6 with a child also 0.6 multiplies
    // down to an effective 0.36 — distinguishes cascaded `Opacity` from a
    // flat `color.w` swatch. `ChildOf` here (not standalone): unlike every
    // other row this pair genuinely needs live parent-child composition, so
    // it stays an ordinary hierarchy relationship independent of the
    // panel — no bake/reparent hazard applies since neither entity is ever
    // `ChildOf` the panel itself.
    let nested_parent = effect_box(app, 64.0);
    let _ = nested_parent.set_opacity(app, 0.6);
    let nested_child = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::ZERO,
            size: Vec2::splat(36.0),
            rotation: 0.0,
            scale: 1.0,
            anchor: Vec2::new(0.5, 0.5),
            color: ACCENT,
            corner_radius: 6.0,
        })
        .non_interactive()
        .opacity(0.6),
    );
    let _ = nested_parent.add_child(app, nested_child);
    opacity_boxes.push(nested_parent);
    opacity_item_labels.push(row_label(app, "0.6 × 0.6", 12.0));

    let effects = EffectsContent {
        row_labels: [drop_shadow_label, glow_label, border_label, opacity_label],
        row_boxes: [drop_shadow_boxes, glow_boxes, border_boxes, opacity_boxes],
        opacity_item_labels: opacity_item_labels.try_into().unwrap(),
    };

    // --- Text (category 1) ---
    const SAMPLE: &str = "Sample Text";
    let text_grey = Vec4::new(0.16, 0.16, 0.17, 1.0);
    let sample = |app: &mut Proteus, text: Text| -> Handle {
        app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 0.0, CONTENT_Z),
                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                ..Default::default()
            })
            .text(text)
            .non_interactive(),
        )
    };
    let font_size_label = row_label(app, "Font Size", 16.0);
    let font_size_items: Vec<Handle> = [14.0, 20.0, 28.0, 40.0]
        .into_iter()
        .map(|size| sample(app, Text::new(SAMPLE, size).with_color(text_grey)))
        .collect();
    let color_label = row_label(app, "Color", 16.0);
    let color_items: Vec<Handle> = [text_grey, Vec4::ONE, ACCENT]
        .into_iter()
        .map(|color| sample(app, Text::new(SAMPLE, 20.0).with_color(color)))
        .collect();
    let letter_spacing_label = row_label(app, "Letter Spacing", 16.0);
    let letter_spacing_items: Vec<Handle> = [0.0, 4.0, 10.0]
        .into_iter()
        .map(|spacing| {
            sample(
                app,
                Text::new(SAMPLE, 20.0)
                    .with_color(text_grey)
                    .with_letter_spacing(spacing),
            )
        })
        .collect();
    let text = TextContent {
        row_labels: [font_size_label, color_label, letter_spacing_label],
        row_items: [font_size_items, color_items, letter_spacing_items],
    };

    // --- Transforms & Animation (category 2) ---
    let transform_box = |app: &mut Proteus, rotation: f32, scale: f32| -> Handle {
        app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 0.0, CONTENT_Z),
                size: Vec2::splat(64.0),
                rotation,
                scale,
                anchor: Vec2::new(0.5, 0.5),
                color: Vec4::ONE,
                corner_radius: 10.0,
            })
            .non_interactive(),
        )
    };
    let rotation_label = row_label(app, "Rotation", 16.0);
    let rotation_degrees = [0.0_f32, 20.0, -35.0];
    let rotation_boxes: Vec<Handle> = rotation_degrees
        .iter()
        .map(|deg| transform_box(app, deg.to_radians(), 1.0))
        .collect();
    let rotation_item_labels: Vec<Handle> = rotation_degrees
        .iter()
        .map(|deg| row_label(app, &format!("{deg:.0}°"), 12.0))
        .collect();
    let scale_label = row_label(app, "Scale", 16.0);
    let scale_factors = [0.6_f32, 1.0, 1.4];
    let scale_boxes: Vec<Handle> = scale_factors
        .iter()
        .map(|scale| transform_box(app, 0.0, *scale))
        .collect();
    let scale_item_labels: Vec<Handle> = scale_factors
        .iter()
        .map(|scale| row_label(app, &format!("{scale:.1}×"), 12.0))
        .collect();
    let continuous_label = row_label(app, "Continuous Animation", 16.0);
    let continuous_box = transform_box(app, 0.0, 1.0);

    let transforms = TransformsContent {
        row_labels: [rotation_label, scale_label, continuous_label],
        row_boxes: [rotation_boxes, scale_boxes, vec![continuous_box]],
        item_labels: [rotation_item_labels, scale_item_labels, Vec::new()],
    };

    // --- Stress Tests (category 3) ---
    // Same "no fill ever, border/glow only, violet label" Design System
    // treatment as `screens::home`/`screens::examples_home`'s buttons, down
    // to the same constants. Hover registration lives in `Demo::new`,
    // theme-color blend in `Demo::advance_theme`.
    let stress_button = |app: &mut Proteus, label: &str| -> (Handle, Handle) {
        let button = app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 0.0, CONTENT_Z),
                // Placeholder width — `layout_stress` overwrites it with
                // this button's own label width before it's ever shown.
                size: Vec2::new(200.0, STRESS_BUTTON_HEIGHT_PX),
                rotation: 0.0,
                scale: 1.0,
                anchor: Vec2::new(0.5, 0.5),
                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                corner_radius: STRESS_BUTTON_CORNER_RADIUS,
            })
            .border(Border::new(BORDER_WIDTH, violet()))
            .glow(Glow {
                radius: 0.0,
                color: violet(),
                intensity: 1.0,
            }),
        );
        let text = app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 0.0, CONTENT_Z),
                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                ..Default::default()
            })
            .text(
                Text::new(label, STRESS_BUTTON_LABEL_SIZE_PX)
                    .with_color(violet())
                    .with_letter_spacing(STRESS_BUTTON_LABEL_LETTER_SPACING_PX),
            )
            .non_interactive(),
        );
        (button, text)
    };
    let (burst_button, burst_label) = stress_button(app, "Run Burst Spawn");
    let (churn_button, churn_label) = stress_button(app, "Run Texture Churn");

    // Seeded with a single space — see `StressContent::result_text`'s doc.
    let result_text = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, CONTENT_Z),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            ..Default::default()
        })
        .text(Text::new(" ", 16.0).with_color(violet()))
        .non_interactive(),
    );
    let warning_text = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, CONTENT_Z),
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            ..Default::default()
        })
        .text(
            Text::new(
                "Warning: the Texture Churn test may be harmful to photosensitive users.",
                24.0,
            )
            .with_color(violet()),
        )
        .non_interactive(),
    );

    let stress = StressContent {
        buttons: [burst_button, churn_button],
        button_labels: [burst_label, churn_label],
        result_text,
        warning_text,
    };

    ExampleDetail {
        panel,
        transforms_continuous_box: continuous_box,
        stress,
        headings,
        placeholder_message,
        effects,
        text,
        transforms,
    }
}

/// The panel's target rest geometry for category `idx` — computed fresh
/// each time, not cached.
/// Corner radius here is the static light-theme value (`CORNER_RADIUS`) —
/// `Demo::advance_theme` overwrites it every tick with the live blended
/// value once the panel actually lands, same "settle to a hardcoded light
/// value, `advance_theme` immediately overwrites it with the real one"
/// convention `screens::home`'s buttons already use.
pub fn panel_target(idx: usize, viewport_size: Vec2) -> QuadState {
    if idx == 3 {
        return stress_panel_target(viewport_size);
    }
    let width = (viewport_size.x * 0.85).min(1000.0);
    let max_height = (viewport_size.y - 2.0 * TOP_CLEARANCE_PX).max(0.0);
    // 4/5 (Layout, 3D) aren't built yet — `PLACEHOLDER_CONTENT_HEIGHT_PX`,
    // just tall enough for `placeholder_message`.
    let content_height = match idx {
        0..=2 => CONTENT_HEIGHT[idx],
        4 | 5 => PLACEHOLDER_CONTENT_HEIGHT_PX,
        _ => 0.0,
    };
    let height = (content_height + 2.0 * PANEL_PADDING_PX).min(max_height);
    QuadState {
        position: Vec3::new(0.0, 0.0, 0.5),
        size: Vec2::new(width, height),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: LIGHT_GREY,
        corner_radius: CORNER_RADIUS,
    }
}

/// Stress Tests' panel doesn't have a fixed content height like the other 3
/// categories — instead the whole assembly (title, then the two buttons,
/// then the panel) stacks top-down from the viewport's own top edge, and
/// the panel stretches down to `BOTTOM_CLEARANCE_PX` above the viewport's
/// bottom edge.
fn stress_panel_target(viewport_size: Vec2) -> QuadState {
    let heading_top_y = viewport_size.y / 2.0 - TOP_CLEARANCE_PX;
    let heading_bottom_y = heading_top_y - STRESS_HEADING_HEIGHT_PX;
    let buttons_top_y = heading_bottom_y - HEADING_PANEL_GAP_PX;
    let buttons_bottom_y = buttons_top_y - STRESS_BUTTON_HEIGHT_PX;
    let panel_top_edge = buttons_bottom_y - HEADING_PANEL_GAP_PX;
    let panel_bottom_edge = -viewport_size.y / 2.0 + BOTTOM_CLEARANCE_PX;
    let height = (panel_top_edge - panel_bottom_edge).max(0.0);
    let position_y = (panel_top_edge + panel_bottom_edge) / 2.0;
    let width = (viewport_size.x * 0.85).min(1000.0);
    QuadState {
        position: Vec3::new(0.0, position_y, 0.5),
        size: Vec2::new(width, height),
        rotation: 0.0,
        scale: 1.0,
        anchor: Vec2::new(0.5, 0.5),
        color: LIGHT_GREY,
        corner_radius: CORNER_RADIUS,
    }
}

/// Positions category `idx`'s heading and every row/item — all in absolute
/// world coordinates derived from `panel`'s (fixed, already-settled) target
/// rect. Safe to call every tick: idempotent once every baked size involved
/// has stabilized (mirrors `screens::splash::recenter`'s "wait on baking"
/// pattern) — rows silently keep their previous position for a frame or two
/// if a label hasn't baked yet.
pub fn layout_content(app: &mut Proteus, detail: &ExampleDetail, idx: usize, panel: &QuadState) {
    // Stress Tests stacks title → buttons → panel as one assembly (see
    // `layout_stress`'s doc) rather than "title floats a fixed gap above an
    // already-sized panel" like the other 3 categories, so it positions its
    // own heading instead of using the generic placement below.
    if idx == 3 {
        layout_stress(app, detail, panel);
        return;
    }

    let heading = detail.headings[idx];
    if let Some(size) = heading.baked_text_size(app) {
        let x = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX + size.x / 2.0;
        let y = panel.position.y + panel.size.y / 2.0 + HEADING_PANEL_GAP_PX + size.y / 2.0;
        if let Some(mut qs) = app.world_mut().get_mut::<QuadState>(heading.id()) {
            qs.position.x = x;
            qs.position.y = y;
        }
    }

    match idx {
        0 => layout_effects(app, detail, panel),
        1 => layout_text(app, detail, panel),
        2 => layout_transforms(app, detail, panel),
        4 | 5 => place(
            app,
            detail.placeholder_message,
            panel.position.x,
            panel.position.y,
        ),
        _ => {}
    }
}

fn place(app: &mut Proteus, handle: Handle, world_x: f32, world_y: f32) {
    if let Some(mut qs) = app.world_mut().get_mut::<QuadState>(handle.id()) {
        qs.position.x = world_x;
        qs.position.y = world_y;
    }
}

fn label_width(app: &Proteus, handle: Handle) -> f32 {
    handle.baked_text_size(app).map(|s| s.x).unwrap_or(0.0)
}

fn layout_effects(app: &mut Proteus, detail: &ExampleDetail, panel: &QuadState) {
    const ROW_INDENT_EXTRA: f32 = 10.0;
    const ROW_GAP_Y: f32 = 90.0;
    const LABEL_BOX_GAP: f32 = 24.0;
    const BOX_GAP: f32 = 30.0;
    const BOX_SIZE: f32 = 64.0;
    const OPACITY_LABEL_GAP: f32 = 18.0;

    let left_x = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX;
    let row_left_x = left_x + ROW_INDENT_EXTRA;
    let panel_top_y = panel.position.y + panel.size.y / 2.0;
    let top_y = panel_top_y - PANEL_PADDING_PX - BOX_SIZE / 2.0;
    let row_y = [
        top_y,
        top_y - ROW_GAP_Y,
        top_y - 2.0 * ROW_GAP_Y,
        top_y - 3.0 * ROW_GAP_Y,
    ];

    let boxes_start_x = row_left_x + label_width(app, detail.effects.row_labels[0]) + LABEL_BOX_GAP;

    for (row, &y) in row_y.iter().enumerate() {
        let label = detail.effects.row_labels[row];
        let lw = label_width(app, label);
        place(app, label, row_left_x + lw / 2.0, y);
        for (i, &box_entity) in detail.effects.row_boxes[row].iter().enumerate() {
            let x = boxes_start_x + BOX_SIZE / 2.0 + i as f32 * (BOX_SIZE + BOX_GAP);
            place(app, box_entity, x, y);
            if row == 3 {
                let caption = detail.effects.opacity_item_labels[i];
                place(app, caption, x, y - BOX_SIZE / 2.0 - OPACITY_LABEL_GAP);
            }
        }
    }
}

fn layout_text(app: &mut Proteus, detail: &ExampleDetail, panel: &QuadState) {
    const ROW_INDENT_EXTRA: f32 = 10.0;
    const ROW_GAP_Y: f32 = 90.0;
    const LABEL_ITEM_GAP: f32 = 24.0;
    const ITEM_GAP: f32 = 30.0;
    const ROW_HEIGHT: f32 = 56.0;

    let left_x = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX;
    let row_left_x = left_x + ROW_INDENT_EXTRA;
    let panel_top_y = panel.position.y + panel.size.y / 2.0;
    let top_y = panel_top_y - PANEL_PADDING_PX - ROW_HEIGHT / 2.0;
    let row_y = [top_y, top_y - ROW_GAP_Y, top_y - 2.0 * ROW_GAP_Y];

    let items_start_x = row_left_x + label_width(app, detail.text.row_labels[2]) + LABEL_ITEM_GAP;

    for (row, &y) in row_y.iter().enumerate() {
        let label = detail.text.row_labels[row];
        let lw = label_width(app, label);
        place(app, label, row_left_x + lw / 2.0, y);
        let mut running_x = items_start_x;
        for &item in &detail.text.row_items[row] {
            let iw = label_width(app, item);
            place(app, item, running_x + iw / 2.0, y);
            running_x += iw + ITEM_GAP;
        }
    }
}

fn layout_transforms(app: &mut Proteus, detail: &ExampleDetail, panel: &QuadState) {
    const ROW_INDENT_EXTRA: f32 = 10.0;
    const ROW_GAP_Y: f32 = 120.0;
    const LABEL_BOX_GAP: f32 = 29.0;
    const BOX_GAP: f32 = 40.0;
    const BOX_SIZE: f32 = 64.0;
    const ITEM_LABEL_GAP: f32 = 21.0;
    const EXTRA_VERTICAL_PADDING: f32 = 20.0;

    let left_x = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX;
    let row_left_x = left_x + ROW_INDENT_EXTRA;
    let panel_top_y = panel.position.y + panel.size.y / 2.0;
    let top_y = panel_top_y - PANEL_PADDING_PX - EXTRA_VERTICAL_PADDING - BOX_SIZE / 2.0;
    let row_y = [top_y, top_y - ROW_GAP_Y, top_y - 2.0 * ROW_GAP_Y];

    let boxes_start_x =
        row_left_x + label_width(app, detail.transforms.row_labels[2]) + LABEL_BOX_GAP;

    for (row, &y) in row_y.iter().enumerate() {
        let label = detail.transforms.row_labels[row];
        let lw = label_width(app, label);
        place(app, label, row_left_x + lw / 2.0, y);
        for (i, &box_entity) in detail.transforms.row_boxes[row].iter().enumerate() {
            let x = boxes_start_x + BOX_SIZE / 2.0 + i as f32 * (BOX_SIZE + BOX_GAP);
            place(app, box_entity, x, y);
            if let Some(&caption) = detail.transforms.item_labels[row].get(i) {
                place(app, caption, x, y - BOX_SIZE / 2.0 - ITEM_LABEL_GAP);
            }
        }
    }
}

/// Positions Stress Tests' title, both buttons (each sized to its own
/// baked label width, left-aligned as a pair from the panel's left edge),
/// the result text (centered near the panel's bottom, within
/// `RESULT_TEXT_RESERVED_HEIGHT_PX`), and the warning text (centered on the
/// panel). Stacking order top-to-bottom — title, then buttons, then the
/// panel — mirrors `stress_panel_target`'s own derivation of the panel's
/// top edge, so the two stay in sync.
fn layout_stress(app: &mut Proteus, detail: &ExampleDetail, panel: &QuadState) {
    let panel_top_y = panel.position.y + panel.size.y / 2.0;
    let buttons_y = panel_top_y + HEADING_PANEL_GAP_PX + STRESS_BUTTON_HEIGHT_PX / 2.0;
    let title_y = panel_top_y
        + HEADING_PANEL_GAP_PX
        + STRESS_BUTTON_HEIGHT_PX
        + HEADING_PANEL_GAP_PX
        + STRESS_HEADING_HEIGHT_PX / 2.0;
    let left_x = panel.position.x - panel.size.x / 2.0 + PANEL_PADDING_PX;

    let title = detail.headings[3];
    let title_w = label_width(app, title);
    place(app, title, left_x + title_w / 2.0, title_y);

    let label0_w = label_width(app, detail.stress.button_labels[0]);
    let button0_w = label0_w + STRESS_BUTTON_HORIZONTAL_PADDING_PX;
    if let Some(mut qs) = app
        .world_mut()
        .get_mut::<QuadState>(detail.stress.buttons[0].id())
    {
        qs.size.x = button0_w;
        qs.position.x = left_x + button0_w / 2.0;
        qs.position.y = buttons_y;
    }
    place(
        app,
        detail.stress.button_labels[0],
        left_x + button0_w / 2.0,
        buttons_y,
    );

    let button1_x = left_x + button0_w + STRESS_BUTTON_GAP_PX;
    let label1_w = label_width(app, detail.stress.button_labels[1]);
    let button1_w = label1_w + STRESS_BUTTON_HORIZONTAL_PADDING_PX;
    if let Some(mut qs) = app
        .world_mut()
        .get_mut::<QuadState>(detail.stress.buttons[1].id())
    {
        qs.size.x = button1_w;
        qs.position.x = button1_x + button1_w / 2.0;
        qs.position.y = buttons_y;
    }
    place(
        app,
        detail.stress.button_labels[1],
        button1_x + button1_w / 2.0,
        buttons_y,
    );

    let result_y = panel.position.y - panel.size.y / 2.0 + RESULT_TEXT_RESERVED_HEIGHT_PX / 2.0;
    place(app, detail.stress.result_text, panel.position.x, result_y);

    place(
        app,
        detail.stress.warning_text,
        panel.position.x,
        panel.position.y,
    );
}

/// Convert a fully-saturated, full-value HSV color (`hue_deg` in degrees,
/// wrapped to `[0, 360)`; s=1, v=1 fixed) to RGB — standard six-sector
/// conversion. Used only by `advance_continuous_animation`'s rainbow hue
/// cycle, which has no other place in this demo to live given every other
/// color is a fixed design token.
pub(crate) fn hsv_to_rgb(hue_deg: f32) -> Vec3 {
    let h = hue_deg.rem_euclid(360.0) / 60.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    match h as i32 {
        0 => Vec3::new(1.0, x, 0.0),
        1 => Vec3::new(x, 1.0, 0.0),
        2 => Vec3::new(0.0, 1.0, x),
        3 => Vec3::new(0.0, x, 1.0),
        4 => Vec3::new(x, 0.0, 1.0),
        _ => Vec3::new(1.0, 0.0, x),
    }
}

/// Drives the Transforms & Animation screen's "Continuous Animation" box:
/// full rotation every 4s, breathing scale 0.8×–1.2× (sine, 2s period),
/// full hue cycle every 5s. `elapsed` accumulates only while this screen is
/// active — the caller (`Demo::advance_example_animation`) is responsible
/// for pausing (not resetting) it otherwise, so the animation resumes from
/// wherever it left off rather than restarting.
pub fn advance_continuous_animation(app: &mut Proteus, detail: &ExampleDetail, elapsed: f32) {
    const ROTATION_PERIOD_SECS: f32 = 4.0;
    const SCALE_PERIOD_SECS: f32 = 2.0;
    const HUE_PERIOD_SECS: f32 = 5.0;
    const SCALE_MIN: f32 = 0.8;
    const SCALE_MAX: f32 = 1.2;

    let rotation = (elapsed / ROTATION_PERIOD_SECS) * std::f32::consts::TAU;
    let scale_mid = (SCALE_MIN + SCALE_MAX) / 2.0;
    let scale_amplitude = (SCALE_MAX - SCALE_MIN) / 2.0;
    let scale =
        scale_mid + scale_amplitude * (elapsed / SCALE_PERIOD_SECS * std::f32::consts::TAU).sin();
    let hue = (elapsed / HUE_PERIOD_SECS) * 360.0;
    let rgb = hsv_to_rgb(hue);

    if let Some(mut qs) = app
        .world_mut()
        .get_mut::<QuadState>(detail.transforms_continuous_box.id())
    {
        qs.rotation = rotation;
        qs.scale = scale;
        qs.color = Vec4::new(rgb.x, rgb.y, rgb.z, 1.0);
    }
}
