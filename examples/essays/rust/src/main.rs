//! Runs the essays example in a desktop window. In a browser, `start` in
//! `lib.rs` runs it instead, so `main` is empty there.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    proteus_host_winit::run(
        essays::Essays::default(),
        proteus_host_winit::RunConfig {
            title: "Essays".to_string(),
            initial_size: (1100, 760),
            // Points at the example's folder, which holds `fonts/`, wherever
            // the app is run from.
            asset_dir: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".."),
            proteus: essays::config(),
        },
    );
}

#[cfg(target_arch = "wasm32")]
fn main() {}
