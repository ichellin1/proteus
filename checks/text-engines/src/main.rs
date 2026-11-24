//! Runs every case through today's text and the three engines: saves an
//! image of each case to `out/`, one row per engine, and prints Markdown
//! tables of sizes, timings and support.
//!
//! ```text
//! cd checks/text-engines
//! scripts/fetch-fonts.sh
//! cargo run --release
//! ```

use std::fs::File;
use std::io::BufWriter;

use text_engines::cosmic::Cosmic;
use text_engines::fd::Fontdue;
use text_engines::parley_swash::Parley;
use text_engines::{cases, time_workload, Case, Engine, Face, Mask, Support};

/// Today's text: `proteus-render`'s `Typesetter` and `TextRasterizer`, in
/// Inter Bold whatever the case asks for. This was `fontdue` a character at a
/// time, one line, when the engines were compared.
struct Today(proteus_render::Typesetter, proteus_render::TextRasterizer);

impl Engine for Today {
    fn name(&self) -> &'static str {
        "today (v0.3.0)"
    }

    fn support(&self) -> Support {
        Support {
            shaping: "No: a glyph per character",
            kerning: "No",
            variable_weights: "No: one static font",
            fallback: "No",
            spans: "No",
            align: "One line only",
            justify: "No",
            ellipsis: "No",
            inline_boxes: "No",
            measure_without_raster: "No: only after baking",
        }
    }

    fn measure(&mut self, case: &Case, scale: f32) -> (f32, f32) {
        let m = self.render(case, scale);
        (m.width as f32 / scale, m.height as f32 / scale)
    }

    fn render(&mut self, case: &Case, scale: f32) -> Mask {
        let Some(g) = self
            .1
            .rasterize_text(&mut self.0, &case.text(), case.size * scale, 0.0)
        else {
            return Mask::default();
        };
        Mask {
            width: g.width,
            height: g.height,
            alpha: g.rgba_pixels.chunks(4).map(|p| p[3]).collect(),
            lines: 1,
        }
    }
}

/// Saves `masks` as one PNG, a row each, dark text on white, with a grey
/// rule between rows.
fn save_sheet(path: &str, masks: &[Mask]) {
    const GAP: u32 = 12;
    let width = masks.iter().map(|m| m.width).max().unwrap_or(1).max(1) + 2 * GAP;
    let height: u32 = masks
        .iter()
        .map(|m| m.height + 2 * GAP + 1)
        .sum::<u32>()
        .max(1);
    let mut pixels = vec![255u8; (width * height) as usize];
    let mut y0 = 0;
    for mask in masks {
        for y in 0..mask.height {
            for x in 0..mask.width {
                let a = mask.alpha[(y * mask.width + x) as usize];
                let i = ((y0 + GAP + y) * width + GAP + x) as usize;
                pixels[i] = 255 - a;
            }
        }
        y0 += mask.height + 2 * GAP;
        for x in 0..width {
            pixels[(y0 * width + x) as usize] = 200;
        }
        y0 += 1;
    }
    let file = BufWriter::new(File::create(path).unwrap());
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&pixels)
        .unwrap();
}

fn main() {
    std::fs::create_dir_all("out").unwrap();
    let mut engines: Vec<Box<dyn Engine>> = vec![
        Box::new(Today(
            proteus_render::Typesetter::with_embedded_font(),
            proteus_render::TextRasterizer::new(),
        )),
        Box::new(Fontdue::default()),
        Box::new(Cosmic::default()),
        Box::new(Parley::default()),
    ];

    println!("## Cases\n");
    println!("Each `out/<case>-<scale>x.png` has a row per engine, in this order:");
    for e in &engines {
        println!("- {}", e.name());
    }
    println!("\nToday's text draws only cases in Inter Bold, on one line.\n");
    print!("| Case | Scale |");
    for e in &engines {
        print!(" {} |", e.name());
    }
    println!();
    println!("|---|---:|{}", "---|".repeat(engines.len()));
    for case in cases::visual() {
        let scales: &[f32] = if case.name == "small" {
            &[1.0, 2.0]
        } else {
            &[2.0]
        };
        for &scale in scales {
            let masks: Vec<Mask> = engines
                .iter_mut()
                .map(|e| {
                    let today = e.name().starts_with("today");
                    if today && (case.spans[0].face != Face::InterBold || case.width.is_some()) {
                        Mask::default()
                    } else {
                        e.render(&case, scale)
                    }
                })
                .collect();
            save_sheet(&format!("out/{}-{}x.png", case.name, scale), &masks);
            print!("| {} | {scale} |", case.name);
            for (e, m) in engines.iter_mut().zip(&masks) {
                if m.width == 0 {
                    print!(" — |");
                } else {
                    let (w, h) = e.measure(&case, scale);
                    print!(" {w:.1} × {h:.1}, {} line(s) |", m.lines);
                }
            }
            println!();
        }
    }
    println!("\nSizes are in logical pixels, from laying out alone.\n");

    println!("## Timings\n");
    println!(
        "Medians of 50 passes, in milliseconds, on {} {}. Layout is `measure`; \
         rasterizing is laying out again and rasterizing, into one image per text.\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!(
        "| Workload | Scale | Engine | Layout | Layout and rasterizing | With a warm glyph cache |"
    );
    println!("|---|---:|---|---:|---:|---:|");
    for (name, workload) in [
        ("500 labels", cases::labels()),
        ("20 lines", cases::lines()),
        ("20 paragraphs", cases::paragraphs()),
    ] {
        for scale in [1.0, 2.0] {
            for e in engines.iter_mut() {
                if e.name().starts_with("today") && name == "20 paragraphs" {
                    continue;
                }
                e.set_glyph_cache(false);
                let (layout, total) = time_workload(e.as_mut(), &workload, scale, 50);
                e.set_glyph_cache(true);
                let (_, warm) = time_workload(e.as_mut(), &workload, scale, 50);
                e.set_glyph_cache(false);
                let warm = if e.name().starts_with("today") {
                    "—".to_string()
                } else {
                    format!("{warm:.2}")
                };
                let layout = if e.name().starts_with("today") {
                    "—".to_string()
                } else {
                    format!("{layout:.2}")
                };
                println!(
                    "| {name} | {scale} | {} | {layout} | {total:.2} | {warm} |",
                    e.name()
                );
            }
        }
    }

    println!("\n## Support\n");
    type Row = (&'static str, fn(&Support) -> &str);
    let rows: [Row; 10] = [
        ("Shaping", |s| s.shaping),
        ("Kerning", |s| s.kerning),
        ("Variable weights", |s| s.variable_weights),
        ("Fallback", |s| s.fallback),
        ("Spans", |s| s.spans),
        ("Alignment", |s| s.align),
        ("Justify", |s| s.justify),
        ("Ellipsis", |s| s.ellipsis),
        ("Inline boxes", |s| s.inline_boxes),
        ("Measure without rasterizing", |s| s.measure_without_raster),
    ];
    print!("| |");
    for e in &engines {
        print!(" {} |", e.name());
    }
    println!();
    println!("|---|{}", "---|".repeat(engines.len()));
    for (label, get) in rows {
        print!("| {label} |");
        for e in &engines {
            print!(" {} |", get(&e.support()));
        }
        println!();
    }
}
