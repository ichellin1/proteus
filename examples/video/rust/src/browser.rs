//! A player for the web: the browser's own `<video>` element.
//!
//! The element plays the video, at its own pace. Each frame the app asks for,
//! the player draws the element's current picture onto a canvas and reads
//! its pixels back, unless the element's playback time hasn't moved since
//! the last read. While the video plays, its time moves every frame, so every
//! frame is read; while it's paused, nothing is.

use wasm_bindgen::{JsCast, JsValue};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlVideoElement};

use crate::{Picture, Player};

/// Plays a video in a `<video>` element that isn't on the page.
pub struct BrowserPlayer {
    element: HtmlVideoElement,
    canvas: HtmlCanvasElement,
    context: CanvasRenderingContext2d,
    /// The element's playback time, in seconds, at the last read.
    read_at: f64,
}

impl BrowserPlayer {
    /// Starts playing the video at `url`, looping and muted: a browser lets a
    /// muted video play without a click first.
    ///
    /// # Errors
    ///
    /// If the elements can't be created.
    pub fn new(url: &str) -> Result<Self, JsValue> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or("no document")?;
        let element: HtmlVideoElement = document.create_element("video")?.dyn_into()?;
        element.set_src(url);
        element.set_muted(true);
        element.set_loop(true);
        element.set_attribute("playsinline", "")?;
        let canvas: HtmlCanvasElement = document.create_element("canvas")?.dyn_into()?;
        // `willReadFrequently` tells the browser the canvas is read from often,
        // as the TypeScript SDK's `uploadFrom` does: the player reads every
        // frame back.
        let options = js_sys::Object::new();
        js_sys::Reflect::set(&options, &"willReadFrequently".into(), &true.into())?;
        let context: CanvasRenderingContext2d = canvas
            .get_context_with_context_options("2d", &options)?
            .ok_or("no 2d context")?
            .dyn_into()?;
        let _ = element.play();
        Ok(Self {
            element,
            canvas,
            context,
            read_at: -1.0,
        })
    }
}

impl Player for BrowserPlayer {
    fn next_frame(&mut self, _dt: f32) -> Option<Picture> {
        let (width, height) = (self.element.video_width(), self.element.video_height());
        if width == 0 || height == 0 {
            // Not loaded yet.
            return None;
        }
        let time = self.element.current_time();
        if time == self.read_at {
            return None;
        }
        self.read_at = time;

        if self.canvas.width() != width || self.canvas.height() != height {
            self.canvas.set_width(width);
            self.canvas.set_height(height);
        }
        self.context
            .draw_image_with_html_video_element(&self.element, 0.0, 0.0)
            .ok()?;
        let pixels = self
            .context
            .get_image_data(0.0, 0.0, width as f64, height as f64)
            .ok()?;
        Some(Picture {
            width,
            height,
            rgba: pixels.data().0,
        })
    }

    fn toggle_pause(&mut self) {
        if self.element.paused() {
            let _ = self.element.play();
        } else {
            let _ = self.element.pause();
        }
    }
}
