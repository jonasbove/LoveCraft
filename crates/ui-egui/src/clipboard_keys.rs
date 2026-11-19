//! Pastes whose clipboard holds HTML but no text, which the hosts send no paste event for.
//!
//! A copy of a footnote marker or a citation alone has no text (a browser writes none for
//! one), so egui-winit and eframe's web runner, which paste only a non-empty text, never tell
//! the app. On the desktop the host leaves the clipboard keys to the app
//! (`NativeOptions::clipboard_shortcuts` off): Cmd/Ctrl+X and C become egui's cut and copy here,
//! as egui-winit makes them, and Cmd/Ctrl+V asks the host to paste
//! (`ViewportCommand::RequestPaste`, the text or an image as before) and is noted for the
//! editor, which reads the clipboard's HTML when the text is empty. In the browser the page's
//! paste listener keeps such a paste's HTML for the app to take.

use crate::ClipboardHtml;

/// A paste with HTML and no text in this frame, for the rich editor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum HtmlPaste {
    #[default]
    None,
    /// The paste keys or Edit > Paste on the desktop: the clipboard's HTML, if its text is
    /// empty, is read when the editor has the keys.
    Asked,
    /// The page's paste event, which held this HTML and no text.
    Held(String),
}

impl HtmlPaste {
    /// The HTML to paste: read through the host's `read` (the clipboard's HTML when its text
    /// is empty) for a paste asked for, the page's for one it held.
    pub(crate) fn html(&self, read: Option<&ClipboardHtml>) -> Option<String> {
        match self {
            HtmlPaste::None => None,
            HtmlPaste::Asked => read.and_then(|read| read()),
            HtmlPaste::Held(html) => Some(html.clone()),
        }
    }
}

/// Turns the clipboard keys among `events` into egui's cut and copy, as egui-winit does with
/// its translation on, and takes out the paste keys: whether they were pressed.
pub(crate) fn translate(events: &mut Vec<egui::Event>) -> bool {
    let mut paste = false;
    events.retain_mut(|event| {
        let egui::Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = event
        else {
            return true;
        };
        let (key, modifiers) = (*key, *modifiers);
        if is_cut(modifiers, key) {
            *event = egui::Event::Cut;
        } else if is_copy(modifiers, key) {
            *event = egui::Event::Copy;
        } else if is_paste(modifiers, key) {
            paste = true;
            return false;
        }
        true
    });
    paste
}

// egui-winit's clipboard chords (`is_cut_command`, `is_copy_command`, `is_paste_command`).

fn is_cut(modifiers: egui::Modifiers, key: egui::Key) -> bool {
    key == egui::Key::Cut
        || (modifiers.command_only() && key == egui::Key::X)
        || (cfg!(target_os = "windows") && modifiers.shift && key == egui::Key::Delete)
}

fn is_copy(modifiers: egui::Modifiers, key: egui::Key) -> bool {
    key == egui::Key::Copy
        || (modifiers.command_only() && key == egui::Key::C)
        || (cfg!(target_os = "windows") && modifiers.ctrl && key == egui::Key::Insert)
}

fn is_paste(modifiers: egui::Modifiers, key: egui::Key) -> bool {
    key == egui::Key::Paste
        || (modifiers.command_only() && key == egui::Key::V)
        || (cfg!(target_os = "windows") && modifiers.shift && key == egui::Key::Insert)
}

#[cfg(test)]
mod tests {
    use super::{HtmlPaste, translate};
    use egui::{Event, Key, Modifiers};

    fn press(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn the_clipboard_keys_become_cut_copy_and_a_paste() {
        let mut events = vec![
            press(Key::X, Modifiers::COMMAND),
            press(Key::C, Modifiers::COMMAND),
            press(Key::A, Modifiers::COMMAND),
        ];
        assert!(!translate(&mut events));
        assert_eq!(
            events,
            [Event::Cut, Event::Copy, press(Key::A, Modifiers::COMMAND)]
        );
        let mut events = vec![press(Key::V, Modifiers::COMMAND), Event::Text("v".into())];
        assert!(translate(&mut events));
        assert_eq!(events, [Event::Text("v".into())]);
        // Ctrl+Alt+C (Insert citation) and Ctrl+Shift+V are other shortcuts; a release is not
        // a press.
        let mut events = vec![
            press(Key::C, Modifiers::COMMAND | Modifiers::ALT),
            press(Key::V, Modifiers::COMMAND | Modifiers::SHIFT),
            Event::Key {
                key: Key::V,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers::COMMAND,
            },
        ];
        let before = events.clone();
        assert!(!translate(&mut events));
        assert_eq!(events, before);
    }

    #[test]
    fn an_asked_paste_reads_the_host_and_a_held_one_is_its_html() {
        let read: crate::ClipboardHtml = std::sync::Arc::new(|| Some("<p>x</p>".into()));
        assert_eq!(HtmlPaste::None.html(Some(&read)), None);
        assert_eq!(
            HtmlPaste::Asked.html(Some(&read)).as_deref(),
            Some("<p>x</p>")
        );
        assert_eq!(HtmlPaste::Asked.html(None), None);
        assert_eq!(
            HtmlPaste::Held("<p>y</p>".into()).html(None).as_deref(),
            Some("<p>y</p>")
        );
    }
}
