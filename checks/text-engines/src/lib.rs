//! Lays out and rasterizes the same text with three engines, so v0.4.0 can
//! choose one: `fontdue`'s own layout, cosmic-text, and parley with swash.
//!
//! Each engine turns a [`Case`] into a [`Mask`]: one image of the whole text,
//! coverage only, as Proteus bakes text today. The native runner,
//! `src/main.rs`, saves the masks as PNGs and prints timings; the `wasm`
//! module runs the timings in a browser or Node.
//!
//! This crate is a throwaway: it lives outside the workspace, and is deleted
//! when v0.4.0 ships. `docs/design/text.md` holds what it found.

pub mod cases;
#[cfg(feature = "cosmic")]
pub mod cosmic;
#[cfg(feature = "fontdue")]
pub mod fd;
#[cfg(feature = "parley")]
pub mod parley_swash;
#[cfg(all(target_arch = "wasm32", feature = "bench"))]
pub mod wasm;

pub use cases::{Align, Case, Face, Span};

/// The fonts, embedded so the wasm builds have them too.
pub mod fonts {
    /// The static Inter Bold Proteus embeds today.
    pub static INTER_BOLD: &[u8] =
        include_bytes!("../Inter-Bold.ttf");
    /// Inter, variable in weight and optical size.
    #[cfg(feature = "all-fonts")]
    pub static INTER: &[u8] = include_bytes!("../fonts/Inter[opsz,wght].ttf");
    /// Inter Italic, variable in weight and optical size.
    #[cfg(feature = "all-fonts")]
    pub static INTER_ITALIC: &[u8] = include_bytes!("../fonts/Inter-Italic[opsz,wght].ttf");
    /// Noto Sans Devanagari, for Hindi, which Inter lacks.
    #[cfg(feature = "all-fonts")]
    pub static DEVANAGARI: &[u8] = include_bytes!("../fonts/NotoSansDevanagari[wdth,wght].ttf");
}

/// A whole text's coverage, one byte a pixel, rows top to bottom.
#[derive(Clone, Debug, Default)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
    /// The number of lines laid out.
    pub lines: usize,
}

/// One rasterized glyph, placed in the text's pixels: `left` and `top` are
/// its bitmap's top-left corner, y down from the top of the layout.
pub struct GlyphImage<'a> {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub alpha: &'a [u8],
}

impl Mask {
    /// An empty mask of `width` by `height` pixels.
    pub fn new(width: u32, height: u32, lines: usize) -> Self {
        Self {
            width,
            height,
            alpha: vec![0; (width * height) as usize],
            lines,
        }
    }

    /// Adds `glyph`'s coverage, clipped to the mask. Overlapping glyphs keep
    /// the larger coverage, as Proteus's rasterizer does.
    pub fn draw(&mut self, glyph: &GlyphImage) {
        for gy in 0..glyph.height as i32 {
            let y = glyph.top + gy;
            if y < 0 || y >= self.height as i32 {
                continue;
            }
            for gx in 0..glyph.width as i32 {
                let x = glyph.left + gx;
                if x < 0 || x >= self.width as i32 {
                    continue;
                }
                let a = glyph.alpha[(gy * glyph.width as i32 + gx) as usize];
                let i = (y as u32 * self.width + x as u32) as usize;
                self.alpha[i] = self.alpha[i].max(a);
            }
        }
    }

    /// Fills a rectangle with half coverage: where an inline box sits.
    pub fn fill_box(&mut self, left: i32, top: i32, width: u32, height: u32) {
        for y in top.max(0)..(top + height as i32).min(self.height as i32) {
            for x in left.max(0)..(left + width as i32).min(self.width as i32) {
                self.alpha[(y as u32 * self.width + x as u32) as usize] = 128;
            }
        }
    }
}

/// What an engine can do, beyond laying out one style of text.
#[derive(Clone, Copy, Debug)]
pub struct Support {
    pub shaping: &'static str,
    pub kerning: &'static str,
    pub variable_weights: &'static str,
    pub fallback: &'static str,
    pub spans: &'static str,
    pub align: &'static str,
    pub justify: &'static str,
    pub ellipsis: &'static str,
    pub inline_boxes: &'static str,
    pub measure_without_raster: &'static str,
}

/// An engine, for the runner: `measure` lays out only, `render` lays out and
/// rasterizes.
pub trait Engine {
    fn name(&self) -> &'static str;
    fn support(&self) -> Support;
    /// The text's size, laid out but not rasterized.
    fn measure(&mut self, case: &Case, scale: f32) -> (f32, f32);
    fn render(&mut self, case: &Case, scale: f32) -> Mask;
    /// Keeps each glyph's image after rasterizing it, to use again, as a
    /// real implementation would. Off by default.
    fn set_glyph_cache(&mut self, _on: bool) {}
}

/// Milliseconds from a fixed start: `performance.now()` in a browser or Node.
#[cfg(target_arch = "wasm32")]
pub fn now_ms() -> f64 {
    #[cfg(feature = "bench")]
    {
        wasm::now_ms()
    }
    #[cfg(not(feature = "bench"))]
    {
        0.0
    }
}

/// Milliseconds from the first call.
#[cfg(not(target_arch = "wasm32"))]
pub fn now_ms() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

/// Medians over `passes`, after a few warm-up passes, of laying out every
/// case, and of laying out and rasterizing every case, in milliseconds.
pub fn time_workload(
    engine: &mut dyn Engine,
    cases: &[Case],
    scale: f32,
    passes: usize,
) -> (f64, f64) {
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let mut layout = Vec::with_capacity(passes);
    let mut total = Vec::with_capacity(passes);
    for i in 0..passes + 5 {
        let start = now_ms();
        for case in cases {
            core::hint::black_box(engine.measure(case, scale));
        }
        let laid_out = now_ms();
        for case in cases {
            core::hint::black_box(engine.render(case, scale));
        }
        let end = now_ms();
        if i >= 5 {
            layout.push(laid_out - start);
            total.push(end - laid_out);
        }
    }
    (median(layout), median(total))
}

/// For the wasm size builds: lays out and rasterizes one string with the one
/// engine built, so none of it is left out of the build.
#[cfg(not(feature = "bench"))]
#[no_mangle]
pub extern "C" fn probe(size: f32) -> u32 {
    let case = Case {
        name: "probe",
        about: "",
        spans: vec![Span::bold("Hello, world")],
        size,
        width: Some(200.0),
        align: Align::Start,
        inline_box: None,
    };
    // The font is in every build, so the build with no engine measures it,
    // and each engine's own size is its build's size less that one's.
    let mut width = core::hint::black_box(fonts::INTER_BOLD).len() as u32 % 2;
    #[cfg(feature = "fontdue")]
    {
        width += fd::Fontdue::default().render(&case, 1.0).width;
    }
    #[cfg(feature = "cosmic")]
    {
        width += cosmic::Cosmic::default().render(&case, 1.0).width;
    }
    #[cfg(feature = "parley")]
    {
        width += parley_swash::Parley::default().render(&case, 1.0).width;
    }
    let _ = &case;
    width
}
