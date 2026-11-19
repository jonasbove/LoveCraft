//! The HTML of the browser's last paste, kept for the app to read when it handles that paste.

use std::sync::{Arc, Mutex};

/// Shared between the page's paste listener and the app (whose clipboard readers must be
/// `Send + Sync`, so a mutex, though the page has one thread).
#[derive(Clone, Default)]
pub struct HtmlSlot(Arc<Mutex<Held>>);

#[derive(Default)]
struct Held {
    /// The last paste's HTML, read with egui's paste of its text.
    html: Option<String>,
    /// HTML of a paste with no text, which eframe sends no paste event for: the app takes it.
    textless: Option<String>,
}

impl HtmlSlot {
    /// What a paste held as `text/html` and as text: replaced on every paste, so HTML from an
    /// earlier one is never read with this one's text. An empty string is none. HTML with no
    /// text is also kept for the app to take, as its paste.
    pub fn set(&self, html: Option<String>, text: &str) {
        if let Ok(mut held) = self.0.lock() {
            held.html = html.filter(|h| !h.is_empty());
            held.textless = held.html.clone().filter(|_| text.is_empty());
        }
    }

    pub fn get(&self) -> Option<String> {
        self.0.lock().ok().and_then(|held| held.html.clone())
    }

    /// The HTML of a paste that had no text, once.
    pub fn take_textless(&self) -> Option<String> {
        self.0.lock().ok().and_then(|mut held| held.textless.take())
    }
}

#[cfg(test)]
mod tests {
    use super::HtmlSlot;

    #[test]
    fn each_paste_replaces_the_last_ones_html() {
        let slot = HtmlSlot::default();
        assert_eq!(slot.get(), None);
        let page = slot.clone();
        page.set(Some("<p><em>dark</em></p>".into()), "dark");
        assert_eq!(slot.get().as_deref(), Some("<p><em>dark</em></p>"));
        // Read again for the same paste: still there.
        assert_eq!(slot.get().as_deref(), Some("<p><em>dark</em></p>"));
        // A paste of plain text only, or of an empty `text/html`, leaves no HTML behind.
        page.set(None, "plain");
        assert_eq!(slot.get(), None);
        page.set(Some("<b>x</b>".into()), "x");
        page.set(Some(String::new()), "");
        assert_eq!(slot.get(), None);
        assert_eq!(slot.take_textless(), None);
    }

    #[test]
    fn html_with_no_text_is_taken_once_as_its_paste() {
        let slot = HtmlSlot::default();
        let note = r#"<sup class="footnote-ref" data-footnote-id="fn-1"></sup>"#;
        slot.set(Some(note.into()), "");
        assert_eq!(slot.take_textless().as_deref(), Some(note));
        assert_eq!(slot.take_textless(), None);
        assert_eq!(slot.get().as_deref(), Some(note));
        // With text, eframe pastes it and the app reads the HTML then: nothing to take.
        slot.set(Some("<p>dark</p>".into()), "dark");
        assert_eq!(slot.take_textless(), None);
        // A later paste with text drops one not yet taken.
        slot.set(Some(note.into()), "");
        slot.set(Some("<p>dark</p>".into()), "dark");
        assert_eq!(slot.take_textless(), None);
    }
}
