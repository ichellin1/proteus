//! [`PreloadedHostServices`]: [`HostServices`] that serve assets downloaded
//! before the app starts, and fetch anything else on request. See the crate
//! docs, "Loading assets".

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use proteus_runtime::{FetchId, FetchResult, HostServices, VideoStream};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// [`HostServices`] for the web: assets downloaded before the app starts are
/// served from memory, and anything else is fetched on request.
pub struct PreloadedHostServices {
    assets: HashMap<String, Arc<[u8]>>,
    base_url: String,
    next_id: u64,
    // An `AbortController` for each fetch in progress, so `cancel_fetch` can
    // abort it. Each task removes its own entry when its fetch settles.
    controllers: Rc<RefCell<HashMap<FetchId, web_sys::AbortController>>>,
    // Cancelled fetches. A task checks this just before delivering, so a
    // fetch that finished before it was cancelled is still discarded.
    cancelled: Rc<RefCell<HashSet<FetchId>>>,
    // Results waiting for the next `poll_fetches`, written by the fetch
    // tasks.
    completed: Rc<RefCell<Vec<FetchResult>>>,
}

impl PreloadedHostServices {
    /// Downloads every key in `keys`, relative to `base_url`, in parallel, and
    /// returns services that serve them from memory. A key that fails to
    /// download is logged, and `load_asset` returns `None` for it.
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

    // shell that builds the demo's video keys. Worth documenting publicly, or
    // replacing with a structured key, before other apps play video on the web.
    // Opens an HLS video. `key` is `"{dir}|{codecs}"`: the directory holding
    // the stream's manifest, and the codec string the browser needs to check
    // it can play the stream, which varies between files.
    fn open_video(&mut self, key: &str) -> Option<Box<dyn VideoStream>> {
        let Some((dir, codecs)) = key.split_once('|') else {
            log::error!("open_video: {key}: expected \"dir|codecs\"");
            return None;
        };
        let stream = crate::hls_video::open(dir.to_string(), codecs.to_string())?;
        Some(Box::new(stream))
    }
}

/// Fetches a URL's bytes. `signal`, if given, lets an `AbortController`
/// cancel the request; `fetch_async` and the HLS player use it.
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
