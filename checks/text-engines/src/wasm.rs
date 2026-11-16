//! The timings, for a browser or Node, through wasm-bindgen: `out/web/` for
//! a browser, `out/node/` for Node. Built by `scripts/wasm.sh`.

use wasm_bindgen::prelude::*;

use crate::{cases, time_workload, Engine};

/// `performance.now()`, which browsers and Node both have.
pub fn now_ms() -> f64 {
    let performance = js_sys::Reflect::get(&js_sys::global(), &"performance".into()).unwrap();
    let now = js_sys::Reflect::get(&performance, &"now".into()).unwrap();
    now.unchecked_into::<js_sys::Function>()
        .call0(&performance)
        .unwrap()
        .as_f64()
        .unwrap()
}

/// Runs the workloads through each engine, with and without a glyph cache,
/// and returns a Markdown table.
#[wasm_bindgen]
pub fn run_timings(passes: usize) -> String {
    let mut engines: Vec<Box<dyn Engine>> = vec![
        Box::new(crate::fd::Fontdue::default()),
        Box::new(crate::cosmic::Cosmic::default()),
        Box::new(crate::parley_swash::Parley::default()),
    ];
    let mut out = String::from(
        "| Workload | Scale | Engine | Layout | Layout and rasterizing | With a warm glyph cache |\n\
         |---|---:|---|---:|---:|---:|\n",
    );
    for (name, workload) in [
        ("500 labels", cases::labels()),
        ("20 lines", cases::lines()),
        ("20 paragraphs", cases::paragraphs()),
    ] {
        for scale in [1.0, 2.0] {
            for e in engines.iter_mut() {
                e.set_glyph_cache(false);
                let (layout, total) = time_workload(e.as_mut(), &workload, scale, passes);
                e.set_glyph_cache(true);
                let (_, warm) = time_workload(e.as_mut(), &workload, scale, passes);
                e.set_glyph_cache(false);
                out += &format!(
                    "| {name} | {scale} | {} | {layout:.2} | {total:.2} | {warm:.2} |\n",
                    e.name()
                );
            }
        }
    }
    out
}
