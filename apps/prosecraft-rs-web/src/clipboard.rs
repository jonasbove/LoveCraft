//! The rich editor's clipboard in the browser, as the desktop app has it: a paste reads the
//! clipboard's `text/html` beside the text eframe hands egui, and a copy or cut in the editor
//! puts `text/html` as well as `text/plain` on the clipboard.
//!
//! eframe listens for `paste`, `copy` and `cut` on the document (bubbling) and prevents their
//! default. These listeners capture the same events first: a paste's HTML is kept for the app
//! to read when it handles egui's paste of the text (it consumes that paste, so the two never
//! both apply); a copy's or cut's `DataTransfer` is kept while eframe runs the app inside the
//! event, and the app's clipboard writer fills it there.

use std::cell::RefCell;
use std::sync::Arc;

use eframe::wasm_bindgen::{JsCast, JsValue, closure::Closure};
use prosecraft_ui_egui::ProseApp;

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

/// Listens on the page and gives the app its clipboard readers and writer. Called before
/// eframe starts, so these listeners come first; if it fails, paste and copy are plain text.
pub fn install(app: &mut ProseApp) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let slot = HtmlSlot::default();
    let pasted = slot.clone();
    capture(&document, "paste", move |event| {
        pasted.set(
            event
                .clipboard_data()
                .and_then(|d| d.get_data("text/html").ok()),
        );
    })?;
    for event in ["copy", "cut"] {
        capture(&document, event, |event| {
            TRANSFER.with(|t| *t.borrow_mut() = event.clipboard_data());
        })?;
    }
    app.set_clipboard_html(Arc::new(move || slot.get()));
    app.set_clipboard_writer(Arc::new(|html: &str, text: &str| {
        TRANSFER.with(|t| {
            if let Some(data) = t.borrow_mut().take() {
                let _ = data.set_data("text/html", html);
                let _ = data.set_data("text/plain", text);
            }
        });
    }));
    Ok(())
}
