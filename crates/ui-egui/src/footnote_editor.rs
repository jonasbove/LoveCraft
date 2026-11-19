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
    /// Where the caret goes next frame (after a citation put in).
    caret: Option<usize>,
    /// The footnote's citation labels (ProseCraft labels a note's citations on their own).
    cite_labels: std::cell::RefCell<prosecraft_core::citations::labels::LabelCache>,
    /// The link form (`linkEditorOpen`): the address typed, its error, and the passage it is
    /// for (`savedRange`).
    link: Option<LinkForm>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LinkForm {
    url: String,
    error: Option<&'static str>,
    range: std::ops::Range<usize>,
    focus: bool,
}

/// `applyLink`: an address (`https://` added without a scheme; web and email only) for the
/// passage, an existing link at the caret, or the address itself put in at the caret.
fn apply_link(doc: &mut RichDoc, form: &LinkForm) -> Result<usize, &'static str> {
    let entered = form.url.trim();
    if entered.is_empty() {
        return Err("Enter a link address.");
    }
    let href = prosecraft_core::richtext::link_href(entered)?;
    let range = form.range.clone();
    if !range.is_empty() {
        doc.set_link(range.clone(), &href);
        return Ok(range.end);
    }
    if let Some((existing, _)) = doc.link_at(range.start) {
        doc.set_link(existing.clone(), &href);
        return Ok(existing.end);
    }
    Ok(doc.insert_link(range.start, entered, &href).1)
}

/// `removeLink`: the link at the caret, or the links in the passage, undone.
fn remove_link(doc: &mut RichDoc, range: std::ops::Range<usize>) {
    match doc.link_at(range.start) {
        Some((existing, _)) if range.is_empty() => doc.unlink(existing),
        _ => doc.unlink(range),
    };
}

impl FootnoteEdit {
    pub fn open(document: &str, id: &str, html: &str) -> Self {
        // An empty footnote is an empty paragraph.
        let source = if html.is_empty() {
            "<p><br></p>".to_owned()
        } else {
            format!("<p>{html}</p>")
        };
        // Only a definition that is one paragraph's inline content (its HTML is written back
        // without the paragraph).
        let doc = RichDoc::from_html(&source)
            .filter(|d| d.lines == [prosecraft_core::richtext::LineKind::Paragraph]);
        Self {
            document: document.to_owned(),
            id: id.to_owned(),
            body: doc.as_ref().map(|d| d.text.clone()).unwrap_or_default(),
            doc,
            html: html.to_owned(),
            focus: true,
            caret: None,
            cite_labels: Default::default(),
            link: None,
        }
    }

    /// The text as the editor holds it.
    pub fn body(&self) -> &str {
        &self.body
    }

    /// The atoms' HTML, when the footnote can be edited here.
    pub fn atoms(&self) -> Option<&[String]> {
        self.doc.as_ref().map(|d| d.atoms.as_slice())
    }

    /// `insertCitationMarkerInNote`, and a citation changed: the marker at a character (the
    /// caret after it), or in place of a citation atom.
    pub fn place_citation(&mut self, target: crate::citation_picker::Target, html: &str) {
        let Some(doc) = self.doc.as_mut() else {
            return;
        };
        match target {
            crate::citation_picker::Target::Insert { at } => {
                self.body = doc.insert_atom(at, html);
                self.caret = Some((at + 1).min(self.body.chars().count()));
            }
            crate::citation_picker::Target::Edit { atom } => {
                if let Some(slot) = doc
                    .atoms
                    .get_mut(atom)
                    .filter(|h| crate::citation_picker::is_cite_marker(h))
                {
                    html.clone_into(slot);
                }
            }
        }
        self.focus = true;
    }

    /// The text takes the focus back next frame (the picker closed).
    pub fn refocus(&mut self) {
        self.focus = true;
    }

    /// A citation removed from the footnote.
    pub fn remove_citation(&mut self, atom: usize) {
        let Some(doc) = self.doc.as_mut() else {
            return;
        };
        let seen = std::cell::Cell::new(0);
        let (body, at) = doc.remove_atoms(|html| {
            let index = seen.get();
            seen.set(index + 1);
            index == atom && crate::citation_picker::is_cite_marker(html)
        });
        if at.is_some() {
            self.body = body;
            self.caret = at;
        }
        self.focus = true;
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
    let (items, options) = state
        .library
        .as_ref()
        .and_then(|l| l.projects().get(state.active))
        .map(|p| {
            (
                p.view.citations.as_slice(),
                prosecraft_core::publish::PublishOptions::read(
                    p.stored
                        .get("project")
                        .and_then(|p| p.get("publishOptions")),
                ),
            )
        })
        .unwrap_or_default();
    let picker_open = state.citation_picker.is_some();
    let mut open_picker: Option<crate::citation_picker::CitationPicker> = None;
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
        .fixed_size(egui::vec2(
            380.0,
            if edit.link.is_some() { 240.0 } else { 190.0 },
        ))
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
                    let mut insert_cite = false;
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
                        // `openLinkEditor` (Ctrl+K in the note too): pressed while the form is
                        // open or the caret is in a link.
                        let caret = selection.clone().unwrap_or(0..0);
                        let inside = doc.link_at(caret.start).is_some();
                        let link = ui
                            .add_enabled(
                                editable,
                                egui::Button::selectable(edit.link.is_some() || inside, "Link"),
                            )
                            .on_hover_text("Add link");
                        crate::views::name_field(ui, &link, "Add link");
                        let keyed = ui.memory(|m| m.has_focus(field_id))
                            && ui.input_mut(|i| {
                                i.consume_key(egui::Modifiers::COMMAND, egui::Key::K)
                            });
                        if editable && (link.clicked() || keyed) {
                            edit.link = if edit.link.is_some() {
                                edit.focus = true;
                                None
                            } else {
                                let url = doc.link_at(caret.start).map(|l| l.1).unwrap_or_default();
                                Some(LinkForm {
                                    url,
                                    error: None,
                                    range: caret,
                                    focus: true,
                                })
                            };
                        }
                        let cite = ui
                            .add_enabled(editable, egui::Button::new("Cite"))
                            .on_hover_text("Insert citation");
                        crate::views::name_field(ui, &cite, "Insert citation");
                        insert_cite = cite.clicked();
                    });
                    ui.ctx().accesskit_node_builder(tools.response.id, |node| {
                        node.set_role(egui::accesskit::Role::Toolbar);
                        node.set_label("Footnote formatting");
                    });
                    if let Some(form) = edit.link.as_mut() {
                        let (mut apply, mut unlink, mut cancel) = (false, false, false);
                        ui.horizontal(|ui| {
                            let field = ui.add(
                                egui::TextEdit::singleline(&mut form.url)
                                    .hint_text("https://example.com")
                                    .desired_width(190.0),
                            );
                            crate::views::name_field(ui, &field, "Link URL");
                            if std::mem::take(&mut form.focus) {
                                field.request_focus();
                            }
                            apply = ui.button("Apply").clicked()
                                || (field.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                            if doc.link_at(form.range.start).is_some() || !form.range.is_empty() {
                                unlink = ui.button("Unlink").clicked();
                            }
                            cancel = ui.button("Cancel").clicked()
                                || (field.has_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Escape)));
                        });
                        if let Some(error) = form.error {
                            let label = ui.colored_label(palette.danger, error);
                            ui.ctx().accesskit_node_builder(label.id, move |node| {
                                node.set_role(egui::accesskit::Role::Alert);
                                node.set_label(error);
                            });
                        }
                        if apply {
                            match apply_link(doc, form) {
                                Ok(caret) => {
                                    edit.body = doc.text.clone();
                                    edit.caret = Some(caret);
                                    edit.focus = true;
                                    edit.link = None;
                                }
                                Err(error) => form.error = Some(error),
                            }
                        } else if unlink {
                            remove_link(doc, form.range.clone());
                            edit.body = doc.text.clone();
                            edit.focus = true;
                            edit.link = None;
                        } else if cancel {
                            edit.focus = true;
                            edit.link = None;
                        }
                    }
                    let cites = crate::views::CiteStyle {
                        items,
                        style: options.citation_style.clone(),
                        locale: options.citation_locale.clone(),
                        cache: &edit.cite_labels,
                    };
                    let layout_labels = crate::views::atom_labels(doc, &cites);
                    let layout_doc = doc.clone();
                    let mut layouter = move |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                        ui.fonts_mut(|f| {
                            crate::views::doc_galley(
                                f,
                                (&layout_doc, text.as_str()),
                                &palette,
                                wrap,
                                &layout_labels,
                                &crate::views::Look::default(),
                            )
                        })
                    };
                    if let Some(at) = edit.caret.take() {
                        let mut text_state =
                            egui::TextEdit::load_state(ui.ctx(), field_id).unwrap_or_default();
                        text_state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::one(
                                egui::text::CCursor::new(at),
                            )));
                        text_state.store(ui.ctx(), field_id);
                    }
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
                    // Its citations' labels; a click on one changes it in the picker.
                    let labels = crate::views::atom_labels(doc, &cites);
                    let painter = ui.painter_at(output.text_clip_rect.expand(4.0));
                    let markers = crate::views::paint_atoms(
                        &painter,
                        &palette,
                        doc,
                        &labels,
                        &output.galley,
                        output.galley_pos,
                        output.text_clip_rect.max.x,
                    );
                    if editable
                        && output.response.clicked()
                        && let Some(pos) = output.response.interact_pointer_pos()
                        && let Some((rect, crate::views::Clicked::Cite(atom))) = markers
                            .iter()
                            .find(|(rect, _)| rect.expand(2.0).contains(pos))
                    {
                        let cites = doc
                            .atoms
                            .get(*atom)
                            .and_then(|h| crate::views::cite_data(h))
                            .unwrap_or_default();
                        let mut picker = crate::citation_picker::CitationPicker::new(
                            &edit.document,
                            &edit.body,
                            crate::citation_picker::Target::Edit { atom: *atom },
                            &cites,
                        );
                        picker.footnote = Some(edit.id.clone());
                        picker.pos = Some(rect.left_bottom() + egui::vec2(0.0, 6.0));
                        open_picker = Some(picker);
                    }
                    if insert_cite {
                        // `insertCitationMarkerInNote`: after the selection, or at the end.
                        let at = selection
                            .clone()
                            .map_or(edit.body.chars().count(), |r| r.end);
                        let caret = output
                            .galley
                            .pos_from_cursor(egui::text::CCursor::new(at))
                            .translate(output.galley_pos.to_vec2());
                        let mut picker = crate::citation_picker::CitationPicker::new(
                            &edit.document,
                            &edit.body,
                            crate::citation_picker::Target::Insert { at },
                            &[],
                        );
                        picker.footnote = Some(edit.id.clone());
                        picker.pos = Some(caret.left_bottom() + egui::vec2(0.0, 6.0));
                        open_picker = Some(picker);
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
    if open_picker.is_some() {
        state.citation_picker = open_picker;
    }
    // Escape with the picker open closes the picker only.
    let escape = !picker_open && ui.input(|i| i.key_pressed(egui::Key::Escape));
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
