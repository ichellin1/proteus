//! The Home screen — the demo's navigation hub. Three nav buttons; clicking
//! them will drive further transitions once their target screens exist
//! (later M12.5 steps — `VideoTiles`, `Loading`/`Gallery`, `ExamplesHome`),
//! not wired yet.
//!
//! Placeholder pass (M12.5 Step 2): solid-color buttons, no background
//! image, no icons/theme chrome — deferred to a later pass, same "structure
//! first" call as `screens::splash`.

use glam::{Vec2, Vec3, Vec4};

use proteus_sdk::{Border, ComponentSpec, Handle, Proteus, QuadState, Text};

const BUTTON_SIZE: Vec2 = Vec2::new(240.0, 56.0);
const BUTTON_GAP: f32 = 16.0;
const LABELS: [&str; 3] = ["Videos", "Gallery", "Examples"];
const ACCENT: Vec4 = Vec4::new(0.36, 0.31, 0.86, 1.0);

pub struct Home {
    pub nav_buttons: [Handle; 3],
    /// Not read directly yet — see `screens::splash::Splash::wordmark`'s doc
    /// for why that's fine.
    #[allow(dead_code)]
    pub nav_labels: [Handle; 3],
}

pub fn spawn(app: &mut Proteus) -> Home {
    let total_height = BUTTON_SIZE.y * 3.0 + BUTTON_GAP * 2.0;
    let top_y = total_height / 2.0 - BUTTON_SIZE.y / 2.0;

    let mut nav_buttons = Vec::with_capacity(3);
    let mut nav_labels = Vec::with_capacity(3);

    for (i, label) in LABELS.iter().enumerate() {
        let y = top_y - i as f32 * (BUTTON_SIZE.y + BUTTON_GAP);

        let button = app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, y, 0.0),
                size: BUTTON_SIZE,
                rotation: 0.0,
                scale: 1.0,
                anchor: Vec2::new(0.5, 0.5),
                color: ACCENT,
                corner_radius: 12.0,
            })
            .border(Border::new(2.0, Vec4::ONE)),
        );

        let text = app.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(0.0, 0.0, 0.1),
                size: Vec2::new(BUTTON_SIZE.x - 24.0, 24.0),
                rotation: 0.0,
                scale: 1.0,
                anchor: Vec2::new(0.5, 0.5),
                // Transparent — see `screens::splash::spawn`'s wordmark doc:
                // this quad only hosts the baked text overlay, so an opaque
                // fill here would hide the (opaque white) glyphs under an
                // identical white background.
                color: Vec4::new(1.0, 1.0, 1.0, 0.0),
                corner_radius: 0.0,
            })
            .text(Text::new(*label, 18.0)),
        );
        button.add_child(app, text);

        nav_buttons.push(button);
        nav_labels.push(text);
    }

    Home {
        nav_buttons: nav_buttons.try_into().unwrap(),
        nav_labels: nav_labels.try_into().unwrap(),
    }
}
