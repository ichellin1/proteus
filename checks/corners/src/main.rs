//! A window for checking rounded corners by eye.
//!
//! ```text
//! cargo run --release -p check-corners
//! ```
//!
//! Two rows of three components: a plain fill, an image and a fill with a
//! border.
//!
//! - **Top:** their corners go from a full circle to square and back in
//!   transitions, over and over. Each corner should stay a smooth curve, and
//!   the image and the border should follow it, at every radius along the way.
//! - **Bottom:** each splits into a grid of four small tiles, which then merge
//!   back. During the split, each piece shows its slice of the component and
//!   is drawn smaller than it, so the slices at the component's corners show
//!   whether those corners stay round, or break up toward square.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    proteus_host_winit::run(
        corners::Corners::default(),
        proteus_host_winit::RunConfig {
            title: "Rounded corners".to_string(),
            initial_size: (900, 760),
            ..Default::default()
        },
    );
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
mod corners {
    use std::sync::Arc;

    use proteus_runtime::{App, Frame};
    use proteus_sdk::glam::{Vec2, Vec3, Vec4};
    use proteus_sdk::{
        Border, ComponentSpec, Easing, Handle, Image, MergeLayout, QuadState, SplitStrategy, Text,
        TransitionConfig,
    };

    /// The distance between the components' centers, across a row.
    const SPACING: f32 = 280.0;
    /// The rows' heights: their centers' y.
    const TOP: f32 = 190.0;
    const BOTTOM: f32 = -170.0;
    /// How long each transition takes, in seconds.
    const TRANSITION: f32 = 1.5;
    /// How long each end holds still before the next transition, in seconds.
    const HOLD: f32 = 0.75;

    /// The top row's width and height. A radius of half of it is a circle.
    const SIDE: f32 = 220.0;
    /// The bottom row's width and height, and its corner radius.
    const SOURCE_SIDE: f32 = 200.0;
    const SOURCE_RADIUS: f32 = 40.0;
    /// The tiles' width and height, their corner radius, and the distance
    /// between their centers. Each slice is half the source's side, so a tile
    /// is drawn at less than half its slice's size.
    const TILE_SIDE: f32 = 44.0;
    const TILE_RADIUS: f32 = 10.0;
    const TILE_SPACING: f32 = 120.0;

    const FILL: Vec4 = Vec4::new(0.29, 0.56, 0.89, 1.0);
    const BORDER: Vec4 = Vec4::new(1.0, 1.0, 1.0, 1.0);

    /// What a component shows besides its fill.
    #[derive(Clone, Copy)]
    enum Kind {
        Fill,
        Image,
        Border,
    }

    impl Kind {
        const ALL: [Self; 3] = [Self::Fill, Self::Image, Self::Border];

        fn label(self) -> &'static str {
            match self {
                Self::Fill => "Fill",
                Self::Image => "Image",
                Self::Border => "Border",
            }
        }

        /// `spec` with this kind's image or border. The image is scaled down
        /// so its longer side is at most `max_side` pixels: the photo is far
        /// larger than any component here, and seven full-size copies don't
        /// fit in the atlas.
        fn apply(self, spec: ComponentSpec, photo: &Arc<[u8]>, max_side: u32) -> ComponentSpec {
            match self {
                Self::Fill => spec,
                Self::Image => spec.image(Image::new(photo.clone()).with_max_side(max_side)),
                Self::Border => spec.border(Border {
                    width: 6.0,
                    color: BORDER,
                    offset: -1.0,
                }),
            }
        }
    }

    /// A bottom-row component, and the tiles it splits into.
    struct Split {
        source: Handle,
        tiles: Vec<Handle>,
    }

    #[derive(Default)]
    pub struct Corners {
        /// The top row: each component, and its geometry as created.
        rounding: Vec<(Handle, QuadState)>,
        /// The bottom row.
        splits: Vec<Split>,
        /// Whether the top row's corners are round, or on their way to round.
        round: bool,
        /// Whether the bottom row is split into tiles, or on its way to it.
        split: bool,
        /// Seconds until the next transitions start.
        next_transition: f32,
    }

    fn square(position: Vec3, side: f32, radius: f32) -> QuadState {
        QuadState {
            position,
            size: Vec2::splat(side),
            color: FILL,
            corner_radius: radius,
            ..Default::default()
        }
    }

    fn label(f: &mut Frame, x: f32, y: f32, text: &str) {
        f.proteus.component(
            ComponentSpec::new(QuadState {
                position: Vec3::new(x, y, 0.0),
                size: Vec2::new(SIDE, 30.0),
                color: Vec4::ZERO,
                ..Default::default()
            })
            .text(Text::new(text, 18.0))
            .non_interactive(),
        );
    }

    impl App for Corners {
        fn setup(&mut self, f: &mut Frame) {
            let photo: Arc<[u8]> = std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../crates/proteus-demo/assets/images/jellyfish.jpg"
            ))
            .expect("the demo's jellyfish.jpg")
            .into();

            for (i, kind) in Kind::ALL.into_iter().enumerate() {
                let x = (i as f32 - 1.0) * SPACING;

                let geometry = square(Vec3::new(x, TOP, 0.0), SIDE, SIDE / 2.0);
                let handle = f.proteus.component(kind.apply(
                    ComponentSpec::new(geometry.clone()),
                    &photo,
                    512,
                ));
                self.rounding.push((handle, geometry));
                label(f, x, TOP - SIDE / 2.0 - 20.0, kind.label());

                let source = f.proteus.component(kind.apply(
                    ComponentSpec::new(square(
                        Vec3::new(x, BOTTOM, 0.0),
                        SOURCE_SIDE,
                        SOURCE_RADIUS,
                    )),
                    &photo,
                    512,
                ));
                // Row by row from the top-left, as the grid split fills them.
                let tiles = [(-1.0, 1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)]
                    .map(|(dx, dy)| {
                        let at = Vec3::new(
                            x + dx * TILE_SPACING / 2.0,
                            BOTTOM + dy * TILE_SPACING / 2.0,
                            0.0,
                        );
                        f.proteus.component(
                            kind.apply(
                                ComponentSpec::new(square(at, TILE_SIDE, TILE_RADIUS)),
                                &photo,
                                128,
                            )
                            .visible(false),
                        )
                    })
                    .to_vec();
                self.splits.push(Split { source, tiles });
                label(f, x, BOTTOM - SOURCE_SIDE / 2.0 - 20.0, kind.label());
            }
            self.round = true;
            self.split = false;
            self.next_transition = HOLD;
        }

        fn update(&mut self, f: &mut Frame, dt: f32) {
            self.next_transition -= dt;
            if self.next_transition > 0.0 {
                return;
            }
            self.next_transition += TRANSITION + HOLD;
            let config = TransitionConfig {
                duration: TRANSITION,
                delay: 0.0,
                easing: Easing::EaseInOutQuad,
            };

            self.round = !self.round;
            let radius = if self.round { SIDE / 2.0 } else { 0.0 };
            for (handle, geometry) in &self.rounding {
                let to = QuadState {
                    corner_radius: radius,
                    ..geometry.clone()
                };
                let _ = handle.animate_to(f.proteus, to, config);
            }

            self.split = !self.split;
            let grid = (2, 2);
            for split in &self.splits {
                let started = if self.split {
                    split.source.split_to(
                        f.proteus,
                        &split.tiles,
                        config,
                        SplitStrategy::Grid {
                            cols: grid.0,
                            rows: grid.1,
                        },
                    )
                } else {
                    split.source.merge_from(
                        f.proteus,
                        &split.tiles,
                        config,
                        MergeLayout::Grid {
                            cols: grid.0,
                            rows: grid.1,
                        },
                    )
                };
                if let Err(e) = started {
                    eprintln!("check-corners: the split or merge didn't start: {e}");
                }
            }
        }
    }
}
