//! ProseCraft's footnote editor (`FootnotePopover`): a footnote's text, with bold and italic,
//! Remove footnote and Done. Opened by inserting a footnote, clicking its marker, or Edit in
//! the Footnotes tab.

use egui::{RichText, Ui};
use prosecraft_core::footnotes;
use prosecraft_core::richtext::{Mark, RichDoc};

use crate::AppState;
use crate::editing::Intent;

/// The footnote being edited.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FootnoteEdit {
    pub document: String,
    pub id: String,
    /// The text and its formatting, when the footnote's HTML can be edited here.
    doc: Option<RichDoc>,
    body: String,
    /// The definition as it was opened.
    html: String,
    focus: bool,
}

impl FootnoteEdit {
    pub fn open(document: &str, id: &str, html: &str) -> Self {
        // An empty footnote is an empty paragraph.
        let source = if html.is_empty() {
            "<p><br></p>".to_owned()
        } else {
            format!("<p>{html}</p>")
        };
        let doc = RichDoc::from_html(&source);
        Self {
            document: document.to_owned(),
            id: id.to_owned(),
            body: doc.as_ref().map(|d| d.text.clone()).unwrap_or_default(),
            doc,
            html: html.to_owned(),
            focus: true,
        }
    }

    /// The definition's inline HTML now.
    fn html(&self) -> Option<String> {
        let html = self.doc.as_ref()?.to_html();
        let inner = html
            .strip_prefix("<p>")
            .and_then(|h| h.strip_suffix("</p>"))
            .unwrap_or(&html);
        Some(if inner == "<br>" {
            String::new()
        } else {
            inner.to_owned()
        })
    }
}

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let editable = state.editable;
    // A request to edit one (from a marker or the Footnotes tab) opens it with its definition.
    if let Some((document, id)) = state.footnote_open.take() {
        let html = state
            .library
            .as_ref()
            .and_then(|l| l.projects().get(state.active))
            .and_then(|p| p.view.document(&document))
            .and_then(|d| d.footnotes.iter().find(|f| f.id == id))
            .map(|f| f.html.clone())
            .unwrap_or_default();
        state.footnote_edit = Some(FootnoteEdit::open(&document, &id, &html));
    }
    let Some(edit) = state.footnote_edit.as_mut() else {
        return;
    };
    // Its number in the scene as it is now.
    let live = state
        .draft
        .as_ref()
        .filter(|d| d.document == edit.document)
        .map(|d| d.doc.to_html());
    let content = live.or_else(|| {
        state
            .library
            .as_ref()
            .and_then(|l| l.projects().get(state.active))
            .and_then(|p| p.view.document(&edit.document))
            .map(|d| d.content.clone())
    });
    let number = content
        .and_then(|html| {
            footnotes::numbered_ids(&html)
                .iter()
                .position(|i| *i == edit.id)
        })
        .map(|i| i + 1);
    let title = number.map_or("Footnote".to_owned(), |n| format!("Footnote {n}"));
    let mut open = true;
    let (mut done, mut remove) = (false, false);
    let field_id = egui::Id::unique(("footnote-text", &edit.id));
    egui::Window::new(&title)
        .id(egui::Id::unique("footnote-editor"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(380.0, 190.0))
        .anchor(egui::Align2::CENTER_TOP, [0.0, 140.0])
        .show(ui.ctx(), |ui| {
            match edit.doc.as_mut() {
                Some(doc) => {
                    let selection = egui::TextEdit::load_state(ui.ctx(), field_id)
                        .and_then(|s| s.cursor.char_range())
                        .map(|r| {
                            let (a, b) = (r.primary.index.0, r.secondary.index.0);
                            a.min(b)..a.max(b)
                        });
                    let tools = ui.horizontal(|ui| {
                        for (mark, text, name) in [
                            (Mark::Bold, RichText::new("B").strong(), "Bold"),
                            (Mark::Italic, RichText::new("I").italics(), "Italic"),
                        ] {
                            let button = ui
                                .add_enabled(editable, egui::Button::new(text))
                                .on_hover_text(name);
                            crate::views::name_field(ui, &button, name);
                            if button.clicked()
                                && let Some(range) = selection.clone().filter(|r| !r.is_empty())
                            {
                                doc.toggle_mark(range, mark);
                                ui.memory_mut(|m| m.request_focus(field_id));
                            }
                        }
                    });
                    ui.ctx().accesskit_node_builder(tools.response.id, |node| {
                        node.set_role(egui::accesskit::Role::Toolbar);
                        node.set_label("Footnote formatting");
                    });
                    let layout_doc = doc.clone();
                    let mut layouter = move |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                        let job = crate::views::layout_doc(
                            &layout_doc,
                            text.as_str(),
                            &palette,
                            wrap,
                            &[],
                        );
                        ui.fonts_mut(|f| f.layout_job(job))
                    };
                    let output = egui::TextEdit::multiline(&mut edit.body)
                        .id(field_id)
                        .hint_text("Write footnote…")
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .interactive(editable)
                        .layouter(&mut layouter)
                        .show(ui);
                    crate::views::name_field(ui, &output.response, "Footnote text");
                    if std::mem::take(&mut edit.focus) {
                        output.response.request_focus();
                    }
                    if output.response.changed() {
                        // A footnote is inline: one paragraph.
                        let single = edit.body.replace('\n', " ");
                        let cursor = output.cursor_range.map(|r| r.primary.index.0);
                        edit.body = doc.apply_text_at(&single, cursor);
                    }
                }
                None => {
                    // HTML this editor can't hold exactly stays as it is.
                    let text = prosecraft_core::html::html_to_plain_text(
                        &edit.html,
                        &prosecraft_core::html::PlainTextOptions::default(),
                    );
                    ui.label(text);
                    ui.label(
                        RichText::new("This footnote's formatting can't be edited here yet.")
                            .size(12.0)
                            .color(palette.text_muted),
                    );
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let button = ui.add_enabled(
                    editable,
                    egui::Button::new(RichText::new("Remove footnote").color(palette.danger)),
                );
                remove = button.clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    done = ui.button(RichText::new("Done").strong()).clicked();
                });
            });
        });
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if remove {
        state.intents.push(Intent::RemoveFootnote {
            document: edit.document.clone(),
            id: edit.id.clone(),
        });
        state.footnote_edit = None;
    } else if done || !open || escape {
        // Closing keeps what was written (ProseCraft saves as you type).
        if editable && let Some(html) = edit.html().filter(|h| *h != edit.html) {
            state.intents.push(Intent::SetFootnote {
                document: edit.document.clone(),
                id: edit.id.clone(),
                html,
            });
        }
        state.footnote_edit = None;
    }
}
