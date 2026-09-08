//! The Splash screen — the demo's entry point. Auto-advances to `Home`
//! after a hold timer once spawned; no click-driven navigation (the
//! original demo has no reverse edge back to Splash — see PLANNING.md's
//! M12.5 research).
//!
//! Fidelity pass 2 of 3 (per an explicit staged scope call): pass 1 landed
//! the real animated logo mark (light treatment, 19-frame loop) and a
//! wordmark sized/positioned per the Brand Spec; this pass adds the intro
//! fade/slide-in (`Demo::advance_intro`) and delays the hold countdown until
//! it settles, both matching `proteus-shell-native::advance_intro_and_hover`
//! exactly. Still deferred: the light/dark theme toggle + full-window
//! background image (pass 3) — an app-wide system this crate doesn't have
//! yet, not something specific to Splash's own content.
//!
//! Frame baking/cycling: loading the 19 PNGs and registering them into
//! `main_atlas` is shell-only asset I/O (same convention as `Text`/`Image`
//! baking — see the crate-root doc), done once at startup via
//! [`crate::Demo::set_logo_frames`]. Cycling *which* already-baked frame is
//! shown each tick is ordinary app state, so that part lives in
//! `Demo::advance_logo_animation`, not here.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{ComponentSpec, Handle, Proteus, QuadState, Text};

/// `proteus-shell-native::INTRO_DELAY` — how long Splash waits, fully
/// invisible, before the intro fade/slide-in starts.
pub const INTRO_DELAY_SECS: f32 = 1.0;
/// `proteus-shell-native::INTRO_DURATION` — seconds for the entry fade
/// (opacity 0 → 1, in lockstep with the slide-in).
pub const INTRO_DURATION_SECS: f32 = 0.6;
/// `proteus-shell-native::INTRO_SLIDE_DISTANCE_PX` — how far left of its
/// resting (centered) position the composite starts, sliding in as it fades.
pub const INTRO_SLIDE_DISTANCE_PX: f32 = 250.0;

/// `proteus-shell-native::SPLASH_HOLD_DURATION` — how long Splash holds,
/// once the intro fade/slide-in has fully settled, before auto-advancing to
/// Home.
pub const HOLD_SECS: f32 = 1.5;

/// `proteus-shell-native::LOGO_FRAME_DURATION` — the Brand Spec's suggested
/// playback rate (~11fps).
pub const LOGO_FRAME_DURATION: f32 = 0.09;

/// Brand Spec violet — same value as `proteus-shell-native::violet()`.
fn violet() -> Vec4 {
    Vec4::new(115.0 / 255.0, 90.0 / 255.0, 204.0 / 255.0, 1.0)
}

/// The mark's native aspect ratio is 104:144 (13:18, portrait) —
/// `proteus-shell-native::LOGO_MARK_HEIGHT`/`LOGO_MARK_WIDTH`.
const LOGO_MARK_HEIGHT: f32 = 200.0;
const LOGO_MARK_WIDTH: f32 = LOGO_MARK_HEIGHT * 104.0 / 144.0;

/// `proteus-shell-native::COMPOSITE_SCALE` — the mark+wordmark composite's
/// resting size, applied as `button`'s own `QuadState::scale` so it scales
/// and moves the child wordmark for free (see `hierarchy::compose_with_parent`).
const COMPOSITE_SCALE: f32 = 0.75;

const WORDMARK_TEXT: &str = "PROTEUS";
/// `proteus-shell-native::WORDMARK_SIZE_PX` — the wordmark bakes at full
/// resolution and is only *displayed* smaller (via `COMPOSITE_SCALE`),
/// crisper than rasterizing directly at the smaller size.
const WORDMARK_SIZE_PX: f32 = 90.0;
/// Brand Spec: "letter-spacing 0.06em" — 0.06 × font size.
const WORDMARK_LETTER_SPACING_PX: f32 = WORDMARK_SIZE_PX * 0.06;
/// Gap between the mark's right edge and the wordmark's left edge.
const WORDMARK_GAP_PX: f32 = 65.0;

pub struct Splash {
    pub button: Handle,
    pub wordmark: Handle,
}

pub fn spawn(app: &mut Proteus) -> Splash {
    let button = app.component(ComponentSpec::new(QuadState {
        position: Vec3::new(0.0, 0.0, 0.5),
        size: Vec2::new(LOGO_MARK_WIDTH, LOGO_MARK_HEIGHT),
        rotation: 0.0,
        scale: COMPOSITE_SCALE,
        anchor: Vec2::new(0.5, 0.5),
        // Starts fully transparent — `Demo::advance_intro` fades this to 1
        // once `INTRO_DELAY_SECS` has elapsed. The mark's own baked frame
        // supplies all color otherwise (untinted at full alpha).
        color: Vec4::new(1.0, 1.0, 1.0, 0.0),
        corner_radius: 0.0,
    }));

    // Local X starts at 0 — `recenter` moves both this and `button` once the
    // wordmark's actual baked width is known (needed for pixel-accurate
    // centering; see WORDMARK_GAP_PX's doc neighbor).
    let wordmark = app.component(
        ComponentSpec::new(QuadState {
            position: Vec3::new(0.0, 0.0, 0.1),
            // Transparent — this quad exists only to host the baked text
            // overlay (see `collect_instances`'s two-instance-per-text-
            // entity doc); an opaque fill here would hide the glyphs under
            // an identical background.
            color: Vec4::new(1.0, 1.0, 1.0, 0.0),
            ..Default::default()
        })
        .text(
            // Starts at alpha 0 too — fades in with the mark, in lockstep
            // (see `Demo::advance_intro`).
            Text::new(WORDMARK_TEXT, WORDMARK_SIZE_PX)
                .with_color(Vec4::new(violet().x, violet().y, violet().z, 0.0))
                .with_letter_spacing(WORDMARK_LETTER_SPACING_PX),
        )
        // Decorative label, not a click target — see `ComponentSpec::non_interactive`'s
        // doc for why a stray `Interactable` here could steal a click from
        // whatever's underneath it.
        .non_interactive(),
    );
    button.add_child(app, wordmark);

    Splash { button, wordmark }
}

/// Re-centers the mark+wordmark composite once the wordmark's actual baked
/// glyph width is known — can't be computed at spawn time, since baking
/// happens lazily, after spawn (see the module doc). Safe to call every
/// tick: idempotent for a given `slide_offset`, since the same inputs always
/// yield the same result. Mirrors
/// `proteus-shell-native::advance_intro_and_hover`'s centering+slide math;
/// `slide_offset` is `Demo::advance_intro`'s contribution (0 once the intro
/// has fully settled).
pub fn recenter(app: &mut Proteus, splash: &Splash, slide_offset: f32) {
    let Some(wordmark_width) = splash.wordmark.baked_text_size(app).map(|s| s.x) else {
        return;
    };
    let rest_x = -COMPOSITE_SCALE * (WORDMARK_GAP_PX + wordmark_width) / 2.0;
    if let Some(mut qs) = app.world_mut().get_mut::<QuadState>(splash.button.id()) {
        qs.position.x = rest_x - slide_offset;
    }
    if let Some(mut qs) = app.world_mut().get_mut::<QuadState>(splash.wordmark.id()) {
        qs.position.x = LOGO_MARK_WIDTH / 2.0 + WORDMARK_GAP_PX + wordmark_width / 2.0;
    }
}
