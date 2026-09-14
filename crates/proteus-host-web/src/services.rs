//! [`PreloadedHostServices`] — a [`HostServices`] backed by bytes fetched up
//! front (`load_asset`), plus a real async `fetch` primitive for everything
//! that can't wait for setup (`fetch_async`, M13.4). See the crate-root
//! doc's "Asset loading" section for why the prefetch path exists at all.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use proteus_runtime::{FetchId, FetchResult, HostServices, VideoStream};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

pub struct PreloadedHostServices {
    assets: HashMap<String, Arc<[u8]>>,
    base_url: String,
    next_id: u64,
    /// Live `AbortController`s for in-flight `fetch_async` calls, keyed by
    /// id — `cancel_fetch` looks one up to actually interrupt the request
    /// (unlike the native host, which can't cut a blocking call short).
    /// Shared with the spawned task itself so it can remove its own entry
    /// once the fetch settles, whether or not it was cancelled.
    controllers: Rc<RefCell<HashMap<FetchId, web_sys::AbortController>>>,
    /// Ids `cancel_fetch` has been told to drop — checked by the spawned
    /// task right before it would otherwise deliver a result, so a fetch
    /// that had already resolved by the time `cancel_fetch` ran is still
    /// discarded instead of surfacing from `poll_fetches`.
    cancelled: Rc<RefCell<HashSet<FetchId>>>,
    /// Results ready for the next `poll_fetches` call — written to by
    /// `spawn_local` tasks running independently of any `&mut self` call.
    completed: Rc<RefCell<Vec<FetchResult>>>,
}

impl PreloadedHostServices {
    /// Fetch every key in `keys` (relative to `base_url`) concurrently and
    /// return a [`HostServices`] impl that serves them synchronously from
    /// memory. A key whose fetch fails (network error or non-2xx status) is
    /// logged and simply absent — `load_asset` returns `None` for it, same
    /// graceful degradation as `DirHostServices` on a missing file.
    pub async fn fetch(base_url: &str, keys: &[&str]) -> Self {
        let fetches = keys.iter().map(|key| async move {
            let url = format!("{}/{}", base_url.trim_end_matches('/'), key);
            match fetch_bytes(&url, None).await {
                Ok(bytes) => Some((key.to_string(), Arc::<[u8]>::from(bytes))),
                Err(e) => {
                    log::warn!("PreloadedHostServices: {key}: {url}: {e:?}");
                    None
                }
            }
        });
        let assets = futures_util::future::join_all(fetches)
            .await
            .into_iter()
            .flatten()
            .collect();
        Self {
            assets,
            base_url: base_url.to_string(),
            next_id: 0,
            controllers: Rc::new(RefCell::new(HashMap::new())),
            cancelled: Rc::new(RefCell::new(HashSet::new())),
            completed: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn resolve_url(&self, key_or_url: &str) -> String {
        if key_or_url.starts_with("http://") || key_or_url.starts_with("https://") {
            key_or_url.to_string()
        } else {
            format!("{}/{}", self.base_url.trim_end_matches('/'), key_or_url)
        }
    }
}

impl HostServices for PreloadedHostServices {
    fn load_asset(&mut self, key: &str) -> Option<Arc<[u8]>> {
        let bytes = self.assets.get(key).cloned();
        if bytes.is_none() {
            log::warn!("load_asset: {key}: not in the preloaded set");
        }
        bytes
    }

    fn fetch_async(&mut self, key_or_url: &str) -> FetchId {
        let id = FetchId(self.next_id);
        self.next_id += 1;
        let url = self.resolve_url(key_or_url);

        let controller = web_sys::AbortController::new().expect("AbortController::new");
        let signal = controller.signal();
        self.controllers.borrow_mut().insert(id, controller);

        let controllers = self.controllers.clone();
        let cancelled = self.cancelled.clone();
        let completed = self.completed.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = fetch_bytes(&url, Some(&signal)).await;
            controllers.borrow_mut().remove(&id);
            if cancelled.borrow_mut().remove(&id) {
                return;
            }
            let bytes = match result {
                Ok(bytes) => Some(Arc::<[u8]>::from(bytes)),
                Err(e) => {
                    log::warn!("fetch_async: {url}: {e:?}");
                    None
                }
            };
            completed.borrow_mut().push((id, bytes));
        });
        id
    }

    fn poll_fetches(&mut self) -> Vec<FetchResult> {
        std::mem::take(&mut *self.completed.borrow_mut())
    }

    fn cancel_fetch(&mut self, id: FetchId) {
        if let Some(controller) = self.controllers.borrow_mut().remove(&id) {
            controller.abort();
        }
        self.cancelled.borrow_mut().insert(id);
    }

    /// `key` is `"{dir}|{codecs}"` (M13.4 step 4b) — HLS needs two pieces of
    /// per-video information (the manifest directory *and* the exact MP4
    /// container codec string `MediaSource.isTypeSupported` needs, which
    /// differs per file — e.g. one tile's source has an audio track, the
    /// others don't), unlike a plain key/URL everywhere else in this trait.
    /// This encoding is a private detail between this method and whichever
    /// shell constructs `DemoApp`'s `video_keys` for the web host — native's
    /// own `video_keys` never needs it, since `.mp4` decode has no
    /// comparable codec-negotiation step.
    fn open_video(&mut self, key: &str) -> Option<Box<dyn VideoStream>> {
        let Some((dir, codecs)) = key.split_once('|') else {
            log::error!("open_video: {key}: expected \"dir|codecs\"");
            return None;
        };
        let stream = crate::hls_video::open(dir.to_string(), codecs.to_string())?;
        Some(Box::new(stream))
    }
}

/// `signal`, when given, ties the request to an `AbortController` — used by
/// `fetch_async` (never by the prefetch-up-front `fetch`, which has nothing
/// to cancel: it's awaited to completion during setup anyway) and by
/// [`crate::hls_video`]'s manifest/segment fetches (M13.4 step 4b).
pub(crate) async fn fetch_bytes(
    url: &str,
    signal: Option<&web_sys::AbortSignal>,
) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let promise = match signal {
        Some(signal) => {
            let init = web_sys::RequestInit::new();
            init.set_signal(Some(signal));
            window.fetch_with_str_and_init(url, &init)
        }
        None => window.fetch_with_str(url),
    };
    let resp_value = JsFuture::from(promise).await?;
    let resp: web_sys::Response = resp_value
        .dyn_into()
        .map_err(|_| JsValue::from_str("fetch: response was not a Response"))?;
    if !resp.ok() {
        return Err(JsValue::from_str(&format!(
            "{} {}",
            resp.status(),
            resp.status_text()
        )));
    }
    let buf_value = JsFuture::from(resp.array_buffer()?).await?;
    let array = js_sys::Uint8Array::new(&buf_value);
    Ok(array.to_vec())
}
