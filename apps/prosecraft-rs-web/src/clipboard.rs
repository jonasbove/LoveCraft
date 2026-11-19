//! The rich editor's clipboard in the browser, as the desktop app has it: a paste reads the
//! clipboard's `text/html` beside the text eframe hands egui, and a copy or cut in the editor
//! puts `text/html` as well as `text/plain` on the clipboard.
//!
//! eframe listens for `paste`, `copy` and `cut` on the document (bubbling) and prevents their
//! default. These listeners capture the same events first: a paste's HTML is kept for the app
//! to read when it handles egui's paste of the text (it consumes that paste, so the two never
//! both apply); a paste with HTML and no text, which eframe sends nothing for (a footnote
//! marker copied alone), is handed to the app as its own paste. A copy's or cut's
//! `DataTransfer` is kept while eframe runs the app inside the event, and the app's clipboard
//! writer fills it there. A copy from the app's Edit menu isn't a browser event: it goes
//! through the async Clipboard API, within the menu click's user activation.

use std::cell::RefCell;
use std::sync::Arc;

use eframe::wasm_bindgen::{JsCast, JsValue, closure::Closure};
use prosecraft_ui_egui::ProseApp;
use wasm_bindgen_futures::JsFuture;

use crate::html_slot::HtmlSlot;

thread_local! {
    /// The `copy` or `cut` event being handled: where the editor's HTML and text go.
    static TRANSFER: RefCell<Option<web_sys::DataTransfer>> = const { RefCell::new(None) };
}

fn capture(
    document: &web_sys::Document,
    event: &str,
    handle: impl FnMut(web_sys::ClipboardEvent) + 'static,
) -> Result<(), JsValue> {
    let listener = Closure::<dyn FnMut(web_sys::ClipboardEvent)>::new(handle);
    document.add_event_listener_with_callback_and_bool(
        event,
        listener.as_ref().unchecked_ref(),
        true,
    )?;
    // Listening for the page's life.
    listener.forget();
    Ok(())
}

/// Listens on the page and gives the app its clipboard readers and writer. Called while
/// eframe creates the app, before it listens itself; if it fails, paste and copy are plain
/// text.
pub fn install(app: &mut ProseApp, ctx: &eframe::egui::Context) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let slot = HtmlSlot::default();
    let pasted = slot.clone();
    let repaint = ctx.clone();
    capture(&document, "paste", move |event| {
        let data = event.clipboard_data();
        // The text eframe reads (and pastes when it isn't empty).
        let text = data
            .as_ref()
            .and_then(|d| d.get_data("text").ok())
            .unwrap_or_default()
            .replace("\r\n", "\n");
        pasted.set(data.and_then(|d| d.get_data("text/html").ok()), &text);
        if text.is_empty() {
            repaint.request_repaint();
        }
    })?;
    for event in ["copy", "cut"] {
        let window = window.clone();
        capture(&document, event, move |event| {
            TRANSFER.with(|t| *t.borrow_mut() = event.clipboard_data());
            // Gone once the event is over (a copy outside the editor leaves it unused).
            let clear = Closure::once_into_js(|| TRANSFER.with(|t| *t.borrow_mut() = None));
            let _ = window.set_timeout_with_callback(clear.unchecked_ref());
        })?;
    }
    let textless = slot.clone();
    app.set_html_paste(Arc::new(move || textless.take_textless()));
    app.set_clipboard_html(Arc::new(move || slot.get()));
    app.set_clipboard_writer(Arc::new(move |html: &str, text: &str| {
        match TRANSFER.with(|t| t.borrow_mut().take()) {
            Some(data) => {
                let _ = data.set_data("text/html", html);
                // None for a footnote marker alone, as the browser writes none.
                if !text.is_empty() {
                    let _ = data.set_data("text/plain", text);
                }
            }
            None => write_async(html, text),
        }
    }));
    Ok(())
}

/// A copy with no clipboard event (Edit > Copy and Cut): the HTML and its text through
/// `navigator.clipboard.write`; the text alone (`writeText`) when the browser refuses that.
fn write_async(html: &str, text: &str) {
    let Some(clipboard) = web_sys::window()
        .and_then(|w| js_sys::Reflect::get(&w.navigator(), &"clipboard".into()).ok())
        .filter(|c| c.is_object())
    else {
        return;
    };
    let written = clipboard_item(html, text)
        .and_then(|item| call(&clipboard, "write", &js_sys::Array::of1(&item)));
    let text = text.to_owned();
    wasm_bindgen_futures::spawn_local(async move {
        let refused = match written {
            Ok(promise) => JsFuture::from(promise).await.is_err(),
            Err(_) => true,
        };
        if refused
            && !text.is_empty()
            && let Ok(promise) = call(&clipboard, "writeText", &JsValue::from_str(&text))
        {
            let _ = JsFuture::from(promise).await;
        }
    });
}

/// `target[method](arg)`, a promise.
fn call(target: &JsValue, method: &str, arg: &JsValue) -> Result<js_sys::Promise, JsValue> {
    let function: js_sys::Function = js_sys::Reflect::get(target, &method.into())?.dyn_into()?;
    function.call1(target, arg)?.dyn_into()
}

/// A `ClipboardItem` of the HTML and its text (none for an empty text).
fn clipboard_item(html: &str, text: &str) -> Result<JsValue, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let item: js_sys::Function =
        js_sys::Reflect::get(&window, &"ClipboardItem".into())?.dyn_into()?;
    let record = js_sys::Object::new();
    for (kind, data) in [("text/html", html), ("text/plain", text)] {
        if data.is_empty() {
            continue;
        }
        let options = web_sys::BlobPropertyBag::new();
        options.set_type(kind);
        let blob = web_sys::Blob::new_with_str_sequence_and_options(
            &js_sys::Array::of1(&JsValue::from_str(data)),
            &options,
        )?;
        js_sys::Reflect::set(&record, &kind.into(), &blob)?;
    }
    js_sys::Reflect::construct(&item, &js_sys::Array::of1(&record))
}
