//! What the browser gives the app in place of the desktop's: the library kept in IndexedDB,
//! settings in `localStorage`, files chosen with a file input and saved as downloads.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use eframe::wasm_bindgen::{JsCast, JsValue, closure::Closure};
use prosecraft_core::storage::crypto::{CryptoError, RandomSource};
use prosecraft_ui_egui::LibraryStore;
use wasm_bindgen_futures::JsFuture;

/// The IndexedDB database. It and the settings' prefix keep the app's former name
/// (ProseCraft-rs), so an updated app finds the library and settings the browser has.
const DATABASE: &str = prosecraft_core::about::ids::WEB_DATABASE;
const STORE: &str = "library";
const LIBRARY_KEY: &str = "library";
/// Settings live in `localStorage` under this prefix (they are a few bytes each).
const SETTING_PREFIX: &str = prosecraft_core::about::ids::WEB_SETTING_PREFIX;

fn window() -> Option<web_sys::Window> {
    web_sys::window()
}

/// A request's result, once it has one.
fn request_future(request: &web_sys::IdbRequest) -> JsFuture {
    let request = request.clone();
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let done = request.clone();
        let on_success = Closure::once_into_js(move |_: web_sys::Event| {
            let _ = resolve.call1(&JsValue::NULL, &done.result().unwrap_or(JsValue::UNDEFINED));
        });
        let on_error = Closure::once_into_js(move |_: web_sys::Event| {
            let _ = reject.call1(
                &JsValue::NULL,
                &JsValue::from_str("IndexedDB request failed"),
            );
        });
        request.set_onsuccess(Some(on_success.unchecked_ref()));
        request.set_onerror(Some(on_error.unchecked_ref()));
    });
    JsFuture::from(promise)
}

/// The database, created with its one store on first use.
async fn open_database() -> Result<web_sys::IdbDatabase, JsValue> {
    let factory = window()
        .and_then(|w| w.indexed_db().ok().flatten())
        .ok_or_else(|| JsValue::from_str("this browser has no IndexedDB"))?;
    let request = factory.open_with_u32(DATABASE, 1)?;
    let upgrade = Closure::once_into_js(move |event: web_sys::Event| {
        let database = event
            .target()
            .and_then(|t| t.dyn_into::<web_sys::IdbOpenDbRequest>().ok())
            .and_then(|r| r.result().ok())
            .and_then(|r| r.dyn_into::<web_sys::IdbDatabase>().ok());
        if let Some(database) = database {
            let _ = database.create_object_store(STORE);
        }
    });
    request.set_onupgradeneeded(Some(upgrade.unchecked_ref()));
    let database = request_future(&request).await?;
    database.dyn_into::<web_sys::IdbDatabase>()
}

fn store(
    database: &web_sys::IdbDatabase,
    mode: web_sys::IdbTransactionMode,
) -> Result<web_sys::IdbObjectStore, JsValue> {
    database
        .transaction_with_str_and_mode(STORE, mode)?
        .object_store(STORE)
}

/// The library's text as last saved in this browser, if any.
pub async fn load_library() -> Result<Option<String>, JsValue> {
    let database = open_database().await?;
    let request = store(&database, web_sys::IdbTransactionMode::Readonly)?
        .get(&JsValue::from_str(LIBRARY_KEY))?;
    Ok(request_future(&request).await?.as_string())
}

async fn put_library(text: &str) -> Result<(), JsValue> {
    let database = open_database().await?;
    let request = store(&database, web_sys::IdbTransactionMode::Readwrite)?
        .put_with_key(&JsValue::from_str(text), &JsValue::from_str(LIBRARY_KEY))?;
    request_future(&request).await.map(|_| ())
}

/// The newest text to write, and whether a writer is running. Saves come about once a second;
/// writes are asynchronous, so only the newest waiting text is written and none overtakes another.
#[derive(Default)]
struct Pending {
    text: Option<String>,
    writing: bool,
    failed: Option<String>,
}

/// The library kept in IndexedDB.
pub struct BrowserStore {
    pending: Rc<RefCell<Pending>>,
}

impl BrowserStore {
    pub fn new() -> Self {
        Self {
            pending: Rc::default(),
        }
    }

    fn write_newest(pending: Rc<RefCell<Pending>>) {
        wasm_bindgen_futures::spawn_local(async move {
            loop {
                let Some(text) = pending.borrow_mut().text.take() else {
                    pending.borrow_mut().writing = false;
                    return;
                };
                let failed = put_library(&text).await.err().map(|e| {
                    e.as_string()
                        .unwrap_or_else(|| "the browser refused to store it".into())
                });
                pending.borrow_mut().failed = failed;
            }
        });
    }
}

impl LibraryStore for BrowserStore {
    fn save(&mut self, text: &str) -> Result<(), String> {
        let mut pending = self.pending.borrow_mut();
        // A write that failed since the last save is reported now (the save that follows retries).
        let failed = pending.failed.take();
        pending.text = Some(text.to_owned());
        if !pending.writing {
            pending.writing = true;
            drop(pending);
            Self::write_newest(Rc::clone(&self.pending));
        }
        match failed {
            Some(why) => Err(format!(
                "The library couldn't be stored in this browser: {why}"
            )),
            None => Ok(()),
        }
    }

    fn now_ms(&self) -> f64 {
        js_sys::Date::now()
    }

    fn new_id(&mut self) -> String {
        window()
            .and_then(|w| w.crypto().ok())
            .map(|c| c.random_uuid())
            .unwrap_or_else(|| format!("id-{}", js_sys::Date::now()))
    }

    fn today(&self) -> String {
        let date = js_sys::Date::new_0();
        format!(
            "{:04}-{:02}-{:02}",
            date.get_full_year(),
            date.get_month() + 1,
            date.get_date()
        )
    }

    fn load_setting(&mut self, key: &str) -> Option<String> {
        let storage = window()?.local_storage().ok()??;
        storage.get_item(&format!("{SETTING_PREFIX}{key}")).ok()?
    }

    fn save_setting(&mut self, key: &str, value: Option<&str>) -> Result<(), String> {
        let storage = window()
            .and_then(|w| w.local_storage().ok().flatten())
            .ok_or("this browser keeps no settings")?;
        let key = format!("{SETTING_PREFIX}{key}");
        match value {
            Some(value) => storage.set_item(&key, value),
            None => storage.remove_item(&key),
        }
        .map_err(|_| "the browser refused to keep the setting".to_owned())
    }
}

/// The browser's `crypto.getRandomValues`.
pub struct BrowserRandom;

impl RandomSource for BrowserRandom {
    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), CryptoError> {
        let crypto = window()
            .and_then(|w| w.crypto().ok())
            .ok_or(CryptoError::NoRandomness)?;
        // `getRandomValues` takes at most 65 536 bytes at a time.
        for chunk in bytes.chunks_mut(65_536) {
            crypto
                .get_random_values_with_u8_array(chunk)
                .map_err(|_| CryptoError::NoRandomness)?;
        }
        Ok(())
    }
}

/// A file the person chose, read: what it was chosen for, its name and its bytes.
pub struct Chosen<T> {
    pub then: T,
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Files chosen and read, waiting for the next frame.
pub type Inbox<T> = Rc<RefCell<VecDeque<Chosen<T>>>>;

/// Asks for a file with a file input (`accept` like ".epub,.zip"); once one is chosen and read,
/// it waits in `inbox` and the app repaints.
pub fn choose_file<T: 'static>(
    accept: &str,
    then: T,
    inbox: &Inbox<T>,
    ctx: &eframe::egui::Context,
) {
    let Some(document) = window().and_then(|w| w.document()) else {
        return;
    };
    let Ok(input) = document
        .create_element("input")
        .map(|e| e.unchecked_into::<web_sys::HtmlInputElement>())
    else {
        return;
    };
    input.set_type("file");
    input.set_accept(accept);
    let inbox = Rc::clone(inbox);
    let ctx = ctx.clone();
    let reader = input.clone();
    let mut then = Some(then);
    let on_change = Closure::<dyn FnMut()>::new(move || {
        let Some(file) = reader.files().and_then(|f| f.get(0)) else {
            return;
        };
        let Some(then) = then.take() else { return };
        let inbox = Rc::clone(&inbox);
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let Ok(buffer) = JsFuture::from(file.array_buffer()).await else {
                return;
            };
            let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
            inbox.borrow_mut().push_back(Chosen {
                then,
                name: file.name(),
                bytes,
            });
            ctx.request_repaint();
        });
    });
    input.set_onchange(Some(on_change.as_ref().unchecked_ref()));
    // The input lives as long as the page may still answer it.
    on_change.forget();
    input.click();
}

/// Reads a file dropped on the page; once read, it waits in `inbox` and the app repaints.
pub fn read_dropped<T: 'static>(
    file: std::sync::Arc<dyn eframe::egui::DroppedFile>,
    then: T,
    inbox: &Inbox<T>,
    ctx: &eframe::egui::Context,
) {
    let inbox = Rc::clone(inbox);
    let ctx = ctx.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let name = file
            .path()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Ok(bytes) = file.bytes_async().await {
            inbox.borrow_mut().push_back(Chosen { then, name, bytes });
            ctx.request_repaint();
        }
    });
}

/// Saves `bytes` as a download named `file_name`.
pub fn download(file_name: &str, bytes: &[u8], media_type: &str) -> Result<(), JsValue> {
    let document = window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::of1(&array);
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(media_type);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let anchor = document
        .create_element("a")?
        .unchecked_into::<web_sys::HtmlAnchorElement>();
    anchor.set_href(&url);
    anchor.set_download(file_name);
    anchor.click();
    // Revoked once the download has had time to start; at once, some browsers cancel it.
    let revoke = Closure::once_into_js(move || {
        let _ = web_sys::Url::revoke_object_url(&url);
    });
    if let Some(w) = window() {
        w.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 60_000)?;
    }
    Ok(())
}
