//! [`PreloadedHostServices`] — a synchronous [`HostServices`] backed by
//! bytes fetched up front. See the crate-root doc's "Asset loading" section
//! for why this exists instead of an async `HostServices`.

use std::collections::HashMap;
use std::sync::Arc;

use proteus_runtime::HostServices;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

pub struct PreloadedHostServices {
    assets: HashMap<String, Arc<[u8]>>,
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
            match fetch_bytes(&url).await {
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
        Self { assets }
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
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let resp_value = JsFuture::from(window.fetch_with_str(url)).await?;
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
