//! Runs the gallery in a desktop window. In a browser, `start` in `lib.rs` runs
//! it instead, so this is empty there.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    proteus_host_winit::run(
        gallery::Gallery::default(),
        proteus_host_winit::RunConfig {
            title: "Gallery".to_string(),
            initial_size: (900, 640),
            proteus: gallery::config(),
            ..Default::default()
        },
    );
}

#[cfg(target_arch = "wasm32")]
fn main() {}
