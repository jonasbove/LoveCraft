//! The HTML of the browser's last paste, kept for the app to read when it handles that paste.

use std::sync::{Arc, Mutex};

/// Shared between the page's paste listener and the app (whose clipboard readers must be
/// `Send + Sync`, so a mutex, though the page has one thread).
#[derive(Clone, Default)]
pub struct HtmlSlot(Arc<Mutex<Option<String>>>);

impl HtmlSlot {
    /// What a paste held as `text/html`: replaced on every paste, so HTML from an earlier one
    /// is never read with this one's text. An empty string is none.
    pub fn set(&self, html: Option<String>) {
        if let Ok(mut held) = self.0.lock() {
            *held = html.filter(|h| !h.is_empty());
        }
    }

    pub fn get(&self) -> Option<String> {
        self.0.lock().ok().and_then(|held| held.clone())
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
        page.set(Some("<p><em>dark</em></p>".into()));
        assert_eq!(slot.get().as_deref(), Some("<p><em>dark</em></p>"));
        // Read again for the same paste: still there.
        assert_eq!(slot.get().as_deref(), Some("<p><em>dark</em></p>"));
        // A paste of plain text only, or of an empty `text/html`, leaves no HTML behind.
        page.set(None);
        assert_eq!(slot.get(), None);
        page.set(Some("<b>x</b>".into()));
        page.set(Some(String::new()));
        assert_eq!(slot.get(), None);
    }
}
