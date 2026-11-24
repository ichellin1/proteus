//! Measures Proteus's native frame time as the number of components grows,
//! with the real engine and renderer, drawing off-screen.
//!
//! ```text
//! cargo run --release -p proteus-bench                    # every scene
//! cargo run --release -p proteus-bench -- small           # or one of them
//! cargo run --release -p proteus-bench -- small 500,2000  # at other counts
//! cargo run --release -p proteus-bench -- phases          # the CPU's phases
//! cargo run --release -p proteus-bench -- video           # full-screen video
//! cargo run --release -p proteus-bench -- text            # baking text
//! ```
//!
//! Three scenes, each at every count in [`COUNTS`], the same as the browser
//! benchmark's, `benches/browser`: N components with random positions and
//! colors, and sizes that depend on the scene.
//!
//! - **small:** 4 to 8 pixels across, still. Filling pixels barely matters,
//!   so what's left is the cost per component.
//! - **large:** 20 to 200 pixels across, still, overlapping many times at high
//!   counts, so filling pixels dominates.
//! - **moving:** 10 to 40 pixels across, every one always moving: each gets a
//!   new random place every half second and moves there over half a second.
//!
//! And one that runs only when asked for, at 1, 10 and 50 components unless
//! told otherwise:
//!
//! - **video:** components the size of the window showing a video, stacked,
//!   with a new 1280 × 800 frame uploaded every frame, as a playing video is.
//!   One is a full-screen video; more stack the per-pixel work, so that a
//!   change to the shader's cost shows.
//!
//! Each frame runs the engine, `Engine::frame`, exactly as a host does, into
//! a 1280 × 800 texture rather than a window, so the display's refresh rate
//! doesn't cap it. As in a real app, the CPU starts the next frame while the
//! GPU draws the last: up to [`IN_FLIGHT`] frames are queued for the GPU.
//! Waiting for the GPU after every frame instead would leave both idle
//! between frames, and Apple's power management, among others, then keeps
//! their clocks low, which made small scenes measure slower than large ones.
//!
//! After [`WARM_UP`] frames, [`FRAMES`] frames are timed two ways:
//!
//! - **CPU:** from the start of the frame until its GPU work is submitted.
//! - **Frame interval:** from the start of one frame to the start of the
//!   next. With the GPU kept busy, that settles at the slower of the CPU and
//!   the GPU: the frame rate an app would get, with no display to wait for.
//!
//! `phases` needs no GPU. It times the parts of a still frame's CPU work that
//! run on the components themselves: `Proteus::tick`, the whole per-frame
//! update; `Proteus::refresh_cascades`, which works out each component's
//! visibility and opacity from its parents' (the tick works them out once
//! too, so a frame pays for them twice); and `collect_instances`, which turns
//! every component into what the GPU draws, from scratch. Then it times what
//! the renderer does instead, `InstanceCache::update`, which redoes only the
//! components that changed: on a still frame, and on a frame where 1% of the
//! components moved.
//!
//! `text` times baking text, at bake scales 1 and 2, on two workloads:
//! [`LABELS`] short labels from 14 to 24 pixels, like a screen of buttons and
//! tiles, and [`LINES`] long lines of 16-pixel text, the nearest thing to a
//! paragraph while text is one line. It times two things:
//!
//! - **Rasterizing:** `TextRasterizer::rasterize_text` alone, for every text
//!   in the workload, with no GPU: the median of [`RASTER_PASSES`] passes.
//!   This is the part a new text engine replaces.
//! - **Measuring:** `Typesetter::measure` alone, for every text, as
//!   `measure_text` does, with no GPU: the median of [`RASTER_PASSES`]
//!   passes.
//! - **Frames:** frames in which every text is replaced with `set_text`, so
//!   the engine bakes all of them again, timed as the other scenes are:
//!   rasterizing, adding each to the atlas and uploading it, then drawing.
//!
//! The results are printed as Markdown tables, with the GPU and platform they
//! ran on, ready to record.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use proteus_host_winit::DirHostServices;
use proteus_render::{TextLayout, TextRasterizer, Typesetter};
use proteus_runtime::glam::{Vec2, Vec3, Vec4};
use proteus_runtime::{wgpu, App, Engine, Frame, ProteusConfig, Viewport};
use proteus_sdk::{ComponentSpec, Easing, Handle, QuadState, Text, TransitionConfig, VideoHandle};

/// The component counts measured.
const COUNTS: [usize; 7] = [100, 1_000, 5_000, 10_000, 25_000, 50_000, 100_000];
/// Frames run before timing starts, so caches settle and the clocks rise.
const WARM_UP: usize = 120;
/// The most frames queued for the GPU at once.
const IN_FLIGHT: usize = 2;
/// Frames timed at each count.
const FRAMES: usize = 300;
/// The simulated time between frames: 60 frames a second.
const DT: f32 = 1.0 / 60.0;
const SIZE: (u32, u32) = (1280, 800);
/// The color format of a window on macOS and most desktops.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
/// How long each move takes in the moving scenario, in seconds.
const MOVE: f32 = 0.5;

#[derive(Clone, Copy, PartialEq)]
enum Scenario {
    Small,
    Large,
    Moving,
    Video,
}

impl Scenario {
    /// The scenes `all` runs. `video` runs only when asked for.
    const ALL: [Self; 3] = [Self::Small, Self::Large, Self::Moving];

    fn name(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Large => "large",
            Self::Moving => "moving",
            Self::Video => "video",
        }
    }

    /// The components' size range, from and to, and corner radius, in pixels.
    fn sizes(self) -> (f32, f32, f32) {
        match self {
            Self::Small => (4.0, 8.0, 2.0),
            Self::Large => (20.0, 200.0, 8.0),
            Self::Moving => (10.0, 40.0, 4.0),
            Self::Video => (0.0, 0.0, 0.0),
        }
    }
}

/// The counts `video` measures unless told otherwise.
const VIDEO_COUNTS: [usize; 3] = [1, 10, 50];

/// The labels in `text`'s labels workload: a quarter of the main atlas at a
/// scale of 2. Every text is replaced each frame, and a texture released in
/// `App::update` can't be evicted until the next frame, so the old and new
/// texts must fit at once, or some bakes fail and the frame times are wrong.
const LABELS: usize = 500;
/// The lines in `text`'s lines workload.
const LINES: usize = 20;
/// Passes over a workload when timing rasterizing alone.
const RASTER_PASSES: usize = 100;
/// What the labels say, in turn.
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
/// What the lines say, in turn: about 80 characters each, so that at 16
/// pixels and a scale of 2 each fits an atlas page.
const LINE_TEXTS: [&str; 4] = [
    "Proteus draws every component as a textured, rounded rectangle in one call.",
    "A split turns one component into several, and a merge turns several into one.",
    "Text is rasterized on the CPU, then added to the atlas and drawn like an image.",
    "Kerning pulls pairs such as AV, To and Wa closer; office and fluffy have ligatures.",
];

/// A small random-number generator (xorshift), so every run draws the same
/// scene without a dependency.
struct Random(u64);

impl Random {
    /// A number from 0 to 1.
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next()
    }

    /// A random place inside the window. Positions are measured from its
    /// center.
    fn position(&mut self) -> Vec3 {
        Vec3::new(
            self.between(-(SIZE.0 as f32) / 2.0, SIZE.0 as f32 / 2.0),
            self.between(-(SIZE.1 as f32) / 2.0, SIZE.1 as f32 / 2.0),
            0.0,
        )
    }

    /// A random size, place and color for a component of `scenario`.
    fn geometry(&mut self, scenario: Scenario) -> QuadState {
        let (min, max, radius) = scenario.sizes();
        let size = Vec2::new(self.between(min, max), self.between(min, max));
        QuadState {
            position: self.position(),
            size,
            color: Vec4::new(self.next(), self.next(), self.next(), 1.0),
            corner_radius: radius,
            ..Default::default()
        }
    }
}

/// The app being measured: `count` plain components.
struct Scene {
    scenario: Scenario,
    count: usize,
    /// Each component, and its geometry as created.
    components: Vec<(Handle, QuadState)>,
    random: Random,
    /// Seconds until the moving scenario's next round of moves.
    next_move: f32,
    /// The video scenario's video, and the two frames it alternates between.
    video: Option<(VideoHandle, [Vec<u8>; 2])>,
    /// Frames drawn so far.
    frames: usize,
}

impl App for Scene {
    fn setup(&mut self, f: &mut Frame) {
        if self.scenario == Scenario::Video {
            let video = f.proteus.create_video();
            // Two frames, one light and one dark, so that each frame's upload
            // is different, as a playing video's is.
            let frame = |shade: u8| {
                let mut rgba = vec![shade; (SIZE.0 * SIZE.1 * 4) as usize];
                rgba.iter_mut().skip(3).step_by(4).for_each(|a| *a = 255);
                rgba
            };
            let frames = [frame(200), frame(60)];
            video.upload_frame(f.proteus, SIZE.0, SIZE.1, &frames[0]);
            for _ in 0..self.count {
                let geometry = QuadState {
                    size: Vec2::new(SIZE.0 as f32, SIZE.1 as f32),
                    color: Vec4::ONE,
                    ..Default::default()
                };
                let handle = f.proteus.component(ComponentSpec::new(geometry.clone()));
                handle.show_video(f.proteus, &video).expect("just created");
                self.components.push((handle, geometry));
            }
            self.video = Some((video, frames));
            return;
        }
        for _ in 0..self.count {
            let geometry = self.random.geometry(self.scenario);
            let handle = f.proteus.component(ComponentSpec::new(geometry.clone()));
            self.components.push((handle, geometry));
        }
    }

    fn update(&mut self, f: &mut Frame, dt: f32) {
        self.frames += 1;
        if let Some((video, frames)) = &self.video {
            video.upload_frame(f.proteus, SIZE.0, SIZE.1, &frames[self.frames % 2]);
            return;
        }
        if self.scenario != Scenario::Moving {
            return;
        }
        self.next_move -= dt;
        if self.next_move > 0.0 {
            return;
        }
        self.next_move += MOVE;
        let config = TransitionConfig {
            duration: MOVE,
            delay: 0.0,
            easing: Easing::EaseInOutQuad,
        };
        // A new place for each, keeping its size and color.
        for (handle, geometry) in &self.components {
            let to = QuadState {
                position: self.random.position(),
                ..geometry.clone()
            };
            let _ = handle.animate_to(f.proteus, to, config);
        }
    }
}

/// Median and 99th percentile of `samples`, in milliseconds.
fn summary(samples: &mut [Duration]) -> (f64, f64) {
    samples.sort();
    let at = |q: f64| {
        let i = ((samples.len() - 1) as f64 * q).round() as usize;
        samples[i].as_secs_f64() * 1000.0
    };
    (at(0.5), at(0.99))
}

struct Result {
    count: usize,
    cpu: (f64, f64),
    interval: (f64, f64),
}

fn measure(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    scenario: Scenario,
    count: usize,
) -> Result {
    let mut config = ProteusConfig::web();
    // Room for every component: the default draws at most 4096 a frame.
    config.memory.max_instances = (count as u32).next_power_of_two().max(4096);
    let mut services = DirHostServices::new(".");
    let mut scene = Scene {
        scenario,
        count,
        components: Vec::with_capacity(count),
        random: Random(0x9e37_79b9_7f4a_7c15),
        next_move: 0.0,
        video: None,
        frames: 0,
    };
    let viewport = Viewport::new(Vec2::new(SIZE.0 as f32, SIZE.1 as f32), 1.0);
    let (mut cpu, mut interval) = run_frames(
        device,
        queue,
        view,
        viewport,
        config,
        &mut scene,
        &mut services,
    );
    Result {
        count,
        cpu: summary(&mut cpu),
        interval: summary(&mut interval),
    }
}

/// One of `text`'s workloads.
#[derive(Clone, Copy)]
enum TextWorkload {
    Labels,
    Lines,
}

impl TextWorkload {
    fn name(self) -> &'static str {
        match self {
            Self::Labels => "labels",
            Self::Lines => "lines",
        }
    }

    /// The workload's texts, the same on every run.
    fn texts(self) -> Vec<Text> {
        match self {
            Self::Labels => (0..LABELS)
                .map(|i| {
                    let size = 14.0 + (i % 6) as f32 * 2.0;
                    Text::new(LABEL_WORDS[i % LABEL_WORDS.len()], size)
                })
                .collect(),
            Self::Lines => (0..LINES)
                .map(|i| Text::new(LINE_TEXTS[i % LINE_TEXTS.len()], 16.0))
                .collect(),
        }
    }
}

/// The app `text` measures: a component for each text, every one replaced
/// with `set_text` each frame, so all of them are baked again.
struct TextScene {
    texts: Vec<Text>,
    components: Vec<Handle>,
    random: Random,
}

impl App for TextScene {
    fn setup(&mut self, f: &mut Frame) {
        for text in &self.texts {
            let geometry = QuadState {
                position: self.random.position(),
                size: Vec2::new(40.0, 20.0),
                ..Default::default()
            };
            let spec = ComponentSpec::new(geometry).text(text.clone());
            self.components.push(f.proteus.component(spec));
        }
    }

    fn update(&mut self, f: &mut Frame, _dt: f32) {
        for (handle, text) in self.components.iter().zip(&self.texts) {
            handle
                .set_text(f.proteus, text.clone())
                .expect("components live as long as the scene");
        }
    }
}

/// `text`'s results for one workload at one bake scale.
struct TextResult {
    /// Rasterizing every text, with no GPU: the median, in milliseconds.
    raster: f64,
    /// Measuring every text, with no GPU: the median, in milliseconds.
    measure: f64,
    cpu: (f64, f64),
    interval: (f64, f64),
}

fn measure_text(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    workload: TextWorkload,
    scale: f32,
) -> TextResult {
    let texts = workload.texts();

    let mut typesetter = Typesetter::with_embedded_font();
    let mut rasterizer = TextRasterizer::new();
    let mut passes = Vec::with_capacity(RASTER_PASSES);
    for i in 0..WARM_UP.min(20) + RASTER_PASSES {
        let start = Instant::now();
        for text in &texts {
            let glyphs = rasterizer.rasterize_text(
                &mut typesetter,
                &text.content,
                text.size_px * scale,
                text.letter_spacing_px * scale,
            );
            std::hint::black_box(glyphs);
        }
        if i >= WARM_UP.min(20) {
            passes.push(start.elapsed());
        }
    }
    let mut measure_passes = Vec::with_capacity(RASTER_PASSES);
    for i in 0..WARM_UP.min(20) + RASTER_PASSES {
        let start = Instant::now();
        for text in &texts {
            let mut layout = TextLayout::new(&text.content, text.size_px);
            layout.letter_spacing_px = text.letter_spacing_px;
            std::hint::black_box(typesetter.measure(&layout, scale));
        }
        if i >= WARM_UP.min(20) {
            measure_passes.push(start.elapsed());
        }
    }

    let mut config = ProteusConfig::web();
    config.memory.max_instances = (2 * texts.len() as u32).next_power_of_two().max(4096);
    config.resources.max_bake_scale = scale;
    let mut services = DirHostServices::new(".");
    let mut scene = TextScene {
        texts,
        components: Vec::new(),
        random: Random(0x9e37_79b9_7f4a_7c15),
    };
    // The target stays 1280 × 800 pixels; at a scale of 2 it's half as many
    // logical pixels across.
    let logical = Vec2::new(SIZE.0 as f32, SIZE.1 as f32) / scale;
    let viewport = Viewport::new(logical, scale);
    let (mut cpu, mut interval) = run_frames(
        device,
        queue,
        view,
        viewport,
        config,
        &mut scene,
        &mut services,
    );
    TextResult {
        raster: summary(&mut passes).0,
        measure: summary(&mut measure_passes).0,
        cpu: summary(&mut cpu),
        interval: summary(&mut interval),
    }
}

/// Makes an engine for `app` and runs [`WARM_UP`] frames, then times
/// [`FRAMES`] frames: the CPU's time for each and the interval between them.
fn run_frames<A: App>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    viewport: Viewport,
    config: ProteusConfig,
    app: &mut A,
    services: &mut DirHostServices,
) -> (Vec<Duration>, Vec<Duration>) {
    let mut engine = Engine::new(device, queue, FORMAT, viewport, config, app, services);

    let mut cpu = Vec::with_capacity(FRAMES);
    let mut interval = Vec::with_capacity(FRAMES);
    let mut in_flight = VecDeque::new();
    let mut last_start: Option<Instant> = None;
    for i in 0..WARM_UP + FRAMES + 1 {
        let start = Instant::now();
        if i > WARM_UP {
            interval.push(start - last_start.expect("set on the previous frame"));
        }
        last_start = Some(start);
        if i == WARM_UP + FRAMES {
            break;
        }
        engine.frame(DT, view, app, services);
        // An empty submission, to wait on later: it finishes after the
        // frame's own work.
        in_flight.push_back(queue.submit(std::iter::empty()));
        if i >= WARM_UP {
            cpu.push(start.elapsed());
        }
        while in_flight.len() > IN_FLIGHT {
            let oldest = in_flight.pop_front().expect("more than IN_FLIGHT queued");
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(oldest),
                    timeout: None,
                })
                .expect("waiting for the GPU failed");
        }
    }
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("waiting for the GPU failed");
    (cpu, interval)
}

/// Times `tick`, `refresh_cascades`, `collect_instances` and
/// `InstanceCache::update` on `count` still components, then
/// `InstanceCache::update` with 1% of them moving, with no GPU: the median of
/// [`FRAMES`] frames each, in milliseconds.
fn phases(count: usize) -> [f64; 5] {
    let mut proteus = proteus_sdk::Proteus::new();
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    let handles: Vec<Handle> = (0..count)
        .map(|_| proteus.component(ComponentSpec::new(random.geometry(Scenario::Large))))
        .collect();
    let mut cache = proteus_ui::InstanceCache::default();
    let mut times: [Vec<Duration>; 5] = Default::default();
    for i in 0..WARM_UP + FRAMES {
        let start = Instant::now();
        proteus.tick(DT);
        let ticked = Instant::now();
        proteus.refresh_cascades();
        let cascaded = Instant::now();
        let instances = proteus_ui::collect_instances(proteus.world_mut());
        let collected = Instant::now();
        cache.update(proteus.world_mut());
        let updated = Instant::now();
        assert_eq!(instances.len(), count);
        assert_eq!(cache.instances().len(), count);
        if i >= WARM_UP {
            times[0].push(ticked - start);
            times[1].push(cascaded - ticked);
            times[2].push(collected - cascaded);
            times[3].push(updated - collected);
        }
    }
    // Every frame, a different 1% of the components move a pixel.
    let moving = (count / 100).max(1);
    for i in 0..WARM_UP + FRAMES {
        for handle in handles.iter().cycle().skip(i * moving).take(moving) {
            let mut geometry = proteus.get(*handle).expect("alive").geometry;
            geometry.position.x += 1.0;
            handle
                .set_declared_geometry(&mut proteus, geometry)
                .expect("alive");
        }
        proteus.tick(DT);
        proteus.refresh_cascades();
        let start = Instant::now();
        cache.update(proteus.world_mut());
        if i >= WARM_UP {
            times[4].push(start.elapsed());
        }
    }
    times.map(|mut t| summary(&mut t).0)
}

fn main() {
    let only = std::env::args().nth(1);
    if only.as_deref() == Some("phases") {
        let counts: Vec<usize> = std::env::args()
            .nth(2)
            .map(|list| {
                list.split(',')
                    .filter_map(|n| n.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_else(|| COUNTS.to_vec());
        println!("### phases (no GPU; medians)\n");
        println!(
            "| Components | tick (ms) | refresh_cascades (ms) | collect_instances (ms) \
             | cache, still (ms) | cache, 1% moving (ms) |"
        );
        println!("|---:|---:|---:|---:|---:|---:|");
        for count in counts {
            let [tick, cascades, collect, still, moving] = phases(count);
            println!(
                "| {count} | {tick:.2} | {cascades:.2} | {collect:.2} | {still:.2} | {moving:.2} |"
            );
        }
        return;
    }
    let text = only.as_deref() == Some("text");
    let scenarios: Vec<Scenario> = if text {
        Vec::new()
    } else if only.as_deref() == Some("video") {
        vec![Scenario::Video]
    } else {
        Scenario::ALL
            .into_iter()
            .filter(|s| {
                only.as_deref()
                    .is_none_or(|name| name == "all" || name == s.name())
            })
            .collect()
    };
    let counts: Option<Vec<usize>> = std::env::args().nth(2).map(|list| {
        list.split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect()
    });
    let counts = counts.unwrap_or_else(|| {
        if scenarios == [Scenario::Video] {
            VIDEO_COUNTS.to_vec()
        } else {
            COUNTS.to_vec()
        }
    });
    if !text && (scenarios.is_empty() || counts.is_empty()) {
        eprintln!(
            "usage: proteus-bench [all|small|large|moving|video|phases|text] [count,count,...]"
        );
        std::process::exit(2);
    }

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("no GPU adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("proteus-bench"),
        // What the native host asks for.
        required_limits: proteus_host_winit::limits(),
        ..Default::default()
    }))
    .expect("couldn't create a GPU device");
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("proteus-bench target"),
        size: wgpu::Extent3d {
            width: SIZE.0,
            height: SIZE.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let info = adapter.get_info();
    println!(
        "GPU: {} ({:?}), on {} {}, {} × {}, release build: {}\n",
        info.name,
        info.backend,
        std::env::consts::OS,
        std::env::consts::ARCH,
        SIZE.0,
        SIZE.1,
        !cfg!(debug_assertions),
    );
    if text {
        println!("### text\n");
        println!(
            "| Workload | Texts | Bake scale | Rasterizing (ms) | Measuring (ms) | CPU median (ms) \
             | CPU p99 (ms) | Frame interval median (ms) | Frame interval p99 (ms) |"
        );
        println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
        for workload in [TextWorkload::Labels, TextWorkload::Lines] {
            for scale in [1.0, 2.0] {
                let r = measure_text(&device, &queue, &view, workload, scale);
                println!(
                    "| {} | {} | {scale} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
                    workload.name(),
                    workload.texts().len(),
                    r.raster,
                    r.measure,
                    r.cpu.0,
                    r.cpu.1,
                    r.interval.0,
                    r.interval.1
                );
            }
        }
        return;
    }
    for scenario in scenarios {
        println!("### {}\n", scenario.name());
        println!("| Components | CPU median (ms) | CPU p99 (ms) | Frame interval median (ms) | Frame interval p99 (ms) |");
        println!("|---:|---:|---:|---:|---:|");
        for &count in &counts {
            let r = measure(&device, &queue, &view, scenario, count);
            println!(
                "| {} | {:.2} | {:.2} | {:.2} | {:.2} |",
                r.count, r.cpu.0, r.cpu.1, r.interval.0, r.interval.1
            );
        }
        println!();
    }
}
