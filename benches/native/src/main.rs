//! Measures Proteus's native frame time as the number of components grows,
//! with the real engine and renderer, drawing off-screen.
//!
//! ```text
//! cargo run --release -p proteus-bench                    # both scenarios
//! cargo run --release -p proteus-bench -- still           # or one of them
//! cargo run --release -p proteus-bench -- still 500,2000  # at other counts
//! ```
//!
//! Two scenarios, each at every count in [`COUNTS`]:
//!
//! - **still:** N components with random positions, sizes and colors, which
//!   don't change.
//! - **moving:** the same, but every component is always in a transition:
//!   each moves to a new random place every half second, as the previous
//!   move ends.
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
//! The results are printed as Markdown tables, with the GPU and platform they
//! ran on, ready to record.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use proteus_host_winit::DirHostServices;
use proteus_runtime::glam::{Vec2, Vec3, Vec4};
use proteus_runtime::{wgpu, App, Engine, Frame, ProteusConfig, Viewport};
use proteus_sdk::{ComponentSpec, Easing, Handle, QuadState, TransitionConfig};

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
    Still,
    Moving,
}

impl Scenario {
    fn name(self) -> &'static str {
        match self {
            Self::Still => "still",
            Self::Moving => "moving",
        }
    }
}

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

    /// A random place, size and color for a component, inside the window.
    /// Positions are measured from its center.
    fn geometry(&mut self) -> QuadState {
        let size = Vec2::new(self.between(20.0, 200.0), self.between(20.0, 200.0));
        QuadState {
            position: Vec3::new(
                self.between(-(SIZE.0 as f32) / 2.0, SIZE.0 as f32 / 2.0),
                self.between(-(SIZE.1 as f32) / 2.0, SIZE.1 as f32 / 2.0),
                0.0,
            ),
            size,
            color: Vec4::new(self.next(), self.next(), self.next(), 1.0),
            corner_radius: 8.0,
            ..Default::default()
        }
    }
}

/// The app being measured: `count` plain components.
struct Scene {
    scenario: Scenario,
    count: usize,
    components: Vec<Handle>,
    random: Random,
    /// Seconds until the moving scenario's next round of moves.
    next_move: f32,
}

impl App for Scene {
    fn setup(&mut self, f: &mut Frame) {
        for _ in 0..self.count {
            let geometry = self.random.geometry();
            self.components
                .push(f.proteus.component(ComponentSpec::new(geometry)));
        }
    }

    fn update(&mut self, f: &mut Frame, dt: f32) {
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
        for &component in &self.components {
            let to = self.random.geometry();
            let _ = component.animate_to(f.proteus, to, config);
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
    };
    let viewport = Viewport::new(Vec2::new(SIZE.0 as f32, SIZE.1 as f32), 1.0);
    let mut engine = Engine::new(
        device,
        queue,
        FORMAT,
        viewport,
        config,
        &mut scene,
        &mut services,
    );

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
        engine.frame(DT, view, &mut scene, &mut services);
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
    Result {
        count,
        cpu: summary(&mut cpu),
        interval: summary(&mut interval),
    }
}

fn main() {
    let only = std::env::args().nth(1);
    let scenarios: Vec<Scenario> = [Scenario::Still, Scenario::Moving]
        .into_iter()
        .filter(|s| {
            only.as_deref()
                .is_none_or(|name| name == "all" || name == s.name())
        })
        .collect();
    let counts: Option<Vec<usize>> = std::env::args().nth(2).map(|list| {
        list.split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect()
    });
    let counts = counts.unwrap_or_else(|| COUNTS.to_vec());
    if scenarios.is_empty() || counts.is_empty() {
        eprintln!("usage: proteus-bench [all|still|moving] [count,count,...]");
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
