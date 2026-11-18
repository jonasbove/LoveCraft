//! ProseCraft's Sources dialog (`CitationLibraryDialog.svelte`): the project's citation library,
//! searched, imported from BibTeX, RIS or CSL-JSON, added and edited by hand, and deleted (with
//! a warning when the text still cites a source). Zotero's local connection isn't ported.

use egui::{RichText, Ui};
use prosecraft_core::citations::import::import_citations;
use prosecraft_core::citations::library::{
    CitationKeySource, CitedIndex, ManualSourceKind, csl_type_to_manual_kind, format_author_list,
    format_item_contributors, generate_citation_key, item_year_text, manual_kind_to_csl_type,
    matches_library_query, merge_imported_citations, parse_author_list, source_type_label,
};
use prosecraft_core::csl::normalize_csl_items_detailed;
use prosecraft_core::js::{JsObject, JsValue};
use prosecraft_core::project::Project;

use crate::editing::Intent;
use crate::{AppState, ProseApp};

/// The manual source form's fields (`ManualSourceDraft`).
#[derive(Clone, Debug, Default, PartialEq)]
struct Draft {
    kind: Option<ManualSourceKind>,
    custom_type: Option<String>,
    title: String,
    authors: String,
    editors: String,
    year: String,
    container_title: String,
    publisher: String,
    publisher_place: String,
    edition: String,
    volume: String,
    issue: String,
    page: String,
    isbn: String,
    doi: String,
    url: String,
}

/// The dialog's state while it is open.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SourcesDialog {
    search: String,
    import_status: Option<(String, Vec<String>)>,
    /// The form, while adding (`None` id) or editing a source.
    form: Option<(Option<String>, Draft)>,
    form_error: String,
    pending_delete: Option<(String, usize)>,
    /// Which documents cite which sources, and a hash of the documents it was read from (read
    /// again only when they change, not every frame).
    cited: Option<(u64, CitedIndex)>,
    /// The Zotero section, on the desktop.
    zotero: crate::zotero::ZoteroPanel,
}

/// A hash of the documents' text and footnotes, to tell when [`CitedIndex`] is out of date.
fn documents_hash(project: &Project) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for doc in &project.documents {
        doc.id.hash(&mut hasher);
        doc.content.hash(&mut hasher);
        for footnote in &doc.footnotes {
            footnote.html.hash(&mut hasher);
        }
    }
    hasher.finish()
}

const KINDS: [ManualSourceKind; 4] = [
    ManualSourceKind::Book,
    ManualSourceKind::Article,
    ManualSourceKind::Chapter,
    ManualSourceKind::Website,
];

fn kind_label(kind: ManualSourceKind) -> &'static str {
    match kind {
        ManualSourceKind::Book => "Book",
        ManualSourceKind::Article => "Article",
        ManualSourceKind::Chapter => "Chapter",
        ManualSourceKind::Website => "Website",
    }
}

fn text(item: &JsValue, key: &str) -> String {
    item.get(key)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

fn names(item: &JsValue, key: &str) -> Vec<JsValue> {
    item.get(key)
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .to_vec()
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `startEdit`: the form filled from a source.
fn draft_for(item: &JsValue) -> Draft {
    let kind_type = text(item, "type");
    let kind = csl_type_to_manual_kind(&kind_type);
    Draft {
        kind: Some(kind.unwrap_or(ManualSourceKind::Book)),
        custom_type: kind.is_none().then_some(kind_type),
        title: text(item, "title"),
        authors: format_author_list(&names(item, "author")),
        editors: format_author_list(&names(item, "editor")),
        year: item_year_text(item),
        container_title: text(item, "container-title"),
        publisher: text(item, "publisher"),
        publisher_place: text(item, "publisher-place"),
        edition: text(item, "edition"),
        volume: text(item, "volume"),
        issue: text(item, "issue"),
        page: text(item, "page"),
        isbn: text(item, "ISBN"),
        doi: text(item, "DOI"),
        url: text(item, "URL"),
    }
}

fn js_trim(s: &str) -> &str {
    s.trim_matches(prosecraft_core::html::is_js_whitespace)
}

/// `buildIssuedDate`: a year of up to four digits as a date part, anything else as literal.
fn issued(raw: &str) -> Option<JsValue> {
    let trimmed = js_trim(raw);
    if trimmed.is_empty() {
        return None;
    }
    let mut date = JsObject::new();
    if trimmed.len() <= 4 && trimmed.bytes().all(|b| b.is_ascii_digit()) {
        let n = prosecraft_core::js::string_to_number(trimmed);
        date.insert(
            "date-parts",
            JsValue::Array(vec![JsValue::Array(vec![JsValue::Number(n)])]),
        );
    } else {
        date.insert("literal", JsValue::str(trimmed));
    }
    Some(JsValue::Object(date))
}

/// `saveManualSource`: the library with the form's source added or replaced, or why not.
fn save(items: &[JsValue], editing: Option<&str>, draft: &Draft) -> Result<Vec<JsValue>, String> {
    let kind = draft.kind.unwrap_or(ManualSourceKind::Book);
    let author = parse_author_list(&draft.authors);
    let editor = if kind == ManualSourceKind::Chapter {
        parse_author_list(&draft.editors)
    } else {
        None
    };
    let title = js_trim(&draft.title).to_owned();
    let year = js_trim(&draft.year).to_owned();
    let csl_type = draft
        .custom_type
        .clone()
        .unwrap_or_else(|| manual_kind_to_csl_type(kind).to_owned());
    let existing = editing.and_then(|id| items.iter().find(|i| text(i, "id") == id));
    let id = match existing {
        Some(item) => text(item, "id"),
        None => generate_citation_key(
            &CitationKeySource {
                author: author.as_deref(),
                editor: editor.as_deref(),
                title: Some(&title),
                year: Some(&year),
            },
            items,
            None,
        ),
    };
    let mut candidate = JsObject::new();
    candidate.insert("id", JsValue::str(&id));
    candidate.insert("type", JsValue::str(&csl_type));
    if !title.is_empty() {
        candidate.insert("title", JsValue::str(&title));
    }
    if let Some(author) = author {
        candidate.insert("author", JsValue::Array(author));
    }
    if let Some(editor) = editor {
        candidate.insert("editor", JsValue::Array(editor));
    }
    if let Some(date) = issued(&year) {
        candidate.insert("issued", date);
    }
    for (key, value) in [
        ("container-title", &draft.container_title),
        ("publisher", &draft.publisher),
        ("publisher-place", &draft.publisher_place),
        ("edition", &draft.edition),
        ("volume", &draft.volume),
        ("issue", &draft.issue),
        ("page", &draft.page),
        ("ISBN", &draft.isbn),
        ("DOI", &draft.doi),
        ("URL", &draft.url),
    ] {
        let value = js_trim(value);
        if !value.is_empty() {
            candidate.insert(key, JsValue::str(value));
        }
    }
    let kept = |key: &str, fallback: &str| {
        existing
            .and_then(|i| i.get(key))
            .filter(|v| v.as_js_string().is_some())
            .cloned()
            .unwrap_or_else(|| JsValue::str(fallback))
    };
    candidate.insert("citeKey", kept("citeKey", &id));
    candidate.insert("provider", kept("provider", "manual"));
    let normalized =
        normalize_csl_items_detailed(&JsValue::Array(vec![JsValue::Object(candidate)]));
    let Some(saved) = normalized.items.into_iter().next() else {
        return Err(normalized
            .warnings
            .into_iter()
            .next()
            .unwrap_or_else(|| "Please enter at least a title, author, or URL.".into()));
    };
    let mut saved = saved.as_object().cloned().unwrap_or_default();
    saved.insert("id", JsValue::str(&id));
    let saved = JsValue::Object(saved);
    Ok(match existing {
        Some(old) => items
            .iter()
            .map(|i| {
                if text(i, "id") == text(old, "id") {
                    saved.clone()
                } else {
                    i.clone()
                }
            })
            .collect(),
        None => items.iter().cloned().chain([saved]).collect(),
    })
}

impl ProseApp {
    /// A citation file read from disk (`handleFileImport`): its sources merged into the library.
    pub fn import_citation_file(&mut self, file_name: &str, text: &str) {
        let Some(project) = self
            .state
            .library
            .as_ref()
            .and_then(|l| l.projects().get(self.state.active))
        else {
            return;
        };
        let imported = import_citations(text, file_name);
        let merged = merge_imported_citations(&project.view.citations, &imported.items);
        if merged.added_count > 0 {
            self.state.intents.push(Intent::SetCitations(merged.items));
        }
        let mut warnings = imported.warnings;
        warnings.extend(merged.warnings);
        let message = format!(
            "Imported {} from {file_name}.",
            plural(merged.added_count, "source", "sources")
        );
        if let Some(dialog) = self.state.sources.as_mut() {
            dialog.import_status = Some((message, warnings));
        }
    }
}

fn field(ui: &mut Ui, label: &str, value: &mut String, hint: &str) {
    ui.label(RichText::new(label).size(12.0));
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    );
    crate::views::name_field(ui, &response, label);
}

/// One field of the form: its label, value, placeholder, and whether it spans both columns
/// (ProseCraft's `source-field-wide`).
type Field<'a> = (&'a str, &'a mut String, &'a str, bool);

/// The fields in ProseCraft's two-column grid: narrow ones side by side, wide ones alone.
fn field_grid(ui: &mut Ui, fields: Vec<Field<'_>>) {
    let mut pending: Option<Field<'_>> = None;
    for f in fields {
        match (pending.take(), f.3) {
            (Some(left), false) => {
                ui.columns(2, |cols| {
                    field(&mut cols[0], left.0, left.1, left.2);
                    field(&mut cols[1], f.0, f.1, f.2);
                });
            }
            (Some(left), true) => {
                field(ui, left.0, left.1, left.2);
                field(ui, f.0, f.1, f.2);
            }
            (None, false) => pending = Some(f),
            (None, true) => field(ui, f.0, f.1, f.2),
        }
    }
    if let Some(left) = pending {
        field(ui, left.0, left.1, left.2);
    }
}

const NAMES_HINT: &str = "(Last, First; separated by semicolons)";

/// The form's fields for its kind of source.
fn form_fields(ui: &mut Ui, draft: &mut Draft) {
    let kind = draft.kind.unwrap_or(ManualSourceKind::Book);
    let mut chosen = kind;
    ui.columns(2, |cols| {
        cols[0].label(RichText::new("Type").size(12.0));
        let combo = craft_ui::ComboBox::from_id_salt("source-kind")
            .selected_text(kind_label(kind))
            .show_ui(&mut cols[0], |ui| {
                for k in KINDS {
                    ui.selectable_value(&mut chosen, k, kind_label(k));
                }
            });
        cols[0]
            .ctx()
            .accesskit_node_builder(combo.response.id, |node| node.set_label("Type"));
        field(&mut cols[1], "Year", &mut draft.year, "2020");
    });
    if chosen != kind {
        draft.kind = Some(chosen);
        draft.custom_type = None;
    }
    let authors = format!("Authors {NAMES_HINT}");
    let editors = format!("Editors {NAMES_HINT}");
    let Draft {
        title,
        authors: author_list,
        editors: editor_list,
        container_title,
        publisher,
        publisher_place,
        edition,
        volume,
        issue,
        page,
        isbn,
        doi,
        url,
        ..
    } = draft;
    let mut fields: Vec<Field<'_>> = vec![
        ("Title", title, "Title of the work", true),
        (&authors, author_list, "Müller, Hans; Schmidt, Anna", true),
    ];
    fields.extend(match chosen {
        ManualSourceKind::Book => vec![
            ("Publisher", publisher, "", false),
            ("Place", publisher_place, "", false),
            ("Edition", edition, "2", false),
            ("ISBN", isbn, "", false),
            ("DOI", doi, "", false),
            ("URL", url, "https://…", false),
        ],
        ManualSourceKind::Article => vec![
            ("Journal", container_title, "", true),
            ("Volume", volume, "", false),
            ("Issue", issue, "", false),
            ("Pages", page, "45-67", false),
            ("DOI", doi, "", false),
            ("URL", url, "https://…", true),
        ],
        ManualSourceKind::Chapter => vec![
            ("Book title", container_title, "", true),
            (&editors, editor_list, "", true),
            ("Publisher", publisher, "", false),
            ("Place", publisher_place, "", false),
            ("Pages", page, "45-67", false),
            ("ISBN / DOI", isbn, "", false),
        ],
        ManualSourceKind::Website => vec![
            ("Website", container_title, "", true),
            ("URL", url, "https://…", true),
        ],
    });
    field_grid(ui, fields);
}

/// The dialog.
pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let editable = state.editable;
    let Some(mut owned) = state.sources.take() else {
        return;
    };
    let Some(project) = state
        .library
        .as_ref()
        .and_then(|l| l.projects().get(state.active))
        .map(|e| &e.view)
    else {
        return;
    };
    let hash = documents_hash(project);
    let cited = owned
        .cited
        .take()
        .filter(|(h, _)| *h == hash)
        .unwrap_or_else(|| (hash, CitedIndex::new(&project.documents)));
    let items = &project.citations;
    let zotero = state.zotero.clone();
    let dialog = &mut owned;
    let mut open = true;
    let mut done = false;
    let mut change: Option<Vec<JsValue>> = None;
    let mut import = false;
    egui::Window::new("Sources")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(620.0, 560.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            crate::widgets::eyebrow(ui, &palette, "Project");
            ui.horizontal(|ui| {
                let search = ui.add(
                    egui::TextEdit::singleline(&mut dialog.search)
                        .hint_text("Search sources by title, author, year, type…")
                        .desired_width(300.0),
                );
                crate::views::name_field(ui, &search, "Search sources");
                if ui.add_enabled(editable, egui::Button::new("Import file…")).clicked() {
                    import = true;
                }
                if ui
                    .add_enabled(editable, egui::Button::new(RichText::new("Add manually").strong()))
                    .clicked()
                {
                    dialog.form = Some((None, Draft { kind: Some(ManualSourceKind::Book), ..Draft::default() }));
                    dialog.form_error.clear();
                    dialog.pending_delete = None;
                }
            });
            if let Some((message, warnings)) = &dialog.import_status {
                let mut dismiss = false;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(message).strong());
                    dismiss = ui.small_button("×").on_hover_text("Dismiss import message").clicked();
                });
                for warning in warnings {
                    ui.label(RichText::new(warning).size(11.0).color(palette.text_muted));
                }
                if dismiss {
                    dialog.import_status = None;
                }
            }
            egui::ScrollArea::vertical()
                .max_height(400.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some((editing, draft)) = dialog.form.as_mut() {
                        ui.label(
                            RichText::new(if editing.is_some() { "Edit source" } else { "Add source manually" })
                                .strong(),
                        );
                        form_fields(ui, draft);
                        if !dialog.form_error.is_empty() {
                            ui.label(RichText::new(&dialog.form_error).color(palette.danger));
                        }
                        let mut close = false;
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                close = true;
                            }
                            let label = if editing.is_some() { "Save changes" } else { "Add source" };
                            if ui.button(RichText::new(label).strong()).clicked() {
                                match save(items, editing.as_deref(), draft) {
                                    Ok(next) => {
                                        change = Some(next);
                                        close = true;
                                    }
                                    Err(error) => dialog.form_error = error,
                                }
                            }
                        });
                        if close {
                            dialog.form = None;
                            dialog.form_error.clear();
                        }
                        ui.separator();
                    }
                    let shown: Vec<&JsValue> = items
                        .iter()
                        .filter(|i| matches_library_query(i, &dialog.search))
                        .collect();
                    if items.is_empty() {
                        ui.label(
                            RichText::new("No sources in this project yet. Import a .bib, .ris, or CSL .json file, or add a source manually.")
                                .color(palette.text_muted),
                        );
                    } else if shown.is_empty() {
                        ui.label(
                            RichText::new(format!("No sources match “{}”.", dialog.search))
                                .color(palette.text_muted),
                        );
                    }
                    for item in shown {
                        source_row(ui, &palette, editable, &cited.1, items, item, dialog, &mut change);
                    }
                    if let Some(transport) = &zotero {
                        let added = crate::zotero::show(ui, &palette, &mut dialog.zotero, transport, items, editable);
                        if added.is_some() {
                            change = added;
                        }
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} in project", plural(items.len(), "source", "sources")))
                        .size(12.0)
                        .color(palette.text_muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    done = ui.button(RichText::new("Done").strong()).clicked();
                });
            });
        });
    if let Some(next) = change {
        state.intents.push(Intent::SetCitations(next));
    }
    if import {
        state.requests.push(crate::HostRequest::ImportCitations);
    }
    if !done && open {
        owned.cited = Some(cited);
        state.sources = Some(owned);
    }
}

#[allow(clippy::too_many_arguments)]
fn source_row(
    ui: &mut Ui,
    palette: &crate::theme::Palette,
    editable: bool,
    cited_index: &CitedIndex,
    items: &[JsValue],
    item: &JsValue,
    dialog: &mut SourcesDialog,
    change: &mut Option<Vec<JsValue>>,
) {
    let id = text(item, "id");
    let title = text(item, "title");
    let cited = cited_index.count(item).document_count;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if title.is_empty() {
                        "(Untitled)"
                    } else {
                        &title
                    })
                    .strong(),
                );
                ui.label(
                    RichText::new(source_type_label(&text(item, "type")))
                        .size(10.0)
                        .color(palette.text_muted),
                );
            });
            let contributors = format_item_contributors(item);
            let year = item_year_text(item);
            let key = item
                .get("citeKey")
                .and_then(JsValue::as_js_string)
                .map_or_else(|| id.clone(), |k| k.to_string_lossy());
            let mut meta = vec![
                if contributors.is_empty() {
                    "Unknown author".to_owned()
                } else {
                    contributors
                },
                if year.is_empty() {
                    "n.d.".to_owned()
                } else {
                    year
                },
                format!("@{key}"),
            ];
            if cited > 0 {
                meta.push(format!(
                    "Cited in {}",
                    plural(cited, "document", "documents")
                ));
            }
            ui.label(
                RichText::new(meta.join(" · "))
                    .size(11.0)
                    .color(palette.text_muted),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let name = if title.is_empty() {
                id.clone()
            } else {
                title.clone()
            };
            let delete = ui
                .add_enabled(editable, egui::Button::new("Delete"))
                .on_hover_text("Delete source");
            crate::views::name_field(ui, &delete, &format!("Delete {name}"));
            let edit = ui
                .add_enabled(editable, egui::Button::new("Edit"))
                .on_hover_text("Edit source");
            crate::views::name_field(ui, &edit, &format!("Edit {name}"));
            if edit.clicked() {
                dialog.form = Some((Some(id.clone()), draft_for(item)));
                dialog.form_error.clear();
                dialog.pending_delete = None;
            }
            if delete.clicked() {
                if cited > 0 {
                    dialog.pending_delete = Some((id.clone(), cited));
                } else {
                    *change = Some(
                        items
                            .iter()
                            .filter(|i| text(i, "id") != id)
                            .cloned()
                            .collect(),
                    );
                }
            }
        });
    });
    if let Some((pending, count)) = dialog.pending_delete.clone()
        && pending == id
    {
        ui.label(
            RichText::new(format!(
                "Cited in {}. Deleting this source will leave unresolved citations in your text.",
                plural(count, "document", "documents")
            ))
            .color(palette.danger),
        );
        ui.horizontal(|ui| {
            if ui.button("Delete anyway").clicked() {
                dialog.pending_delete = None;
                if dialog
                    .form
                    .as_ref()
                    .is_some_and(|(e, _)| e.as_deref() == Some(id.as_str()))
                {
                    dialog.form = None;
                }
                *change = Some(
                    items
                        .iter()
                        .filter(|i| text(i, "id") != id)
                        .cloned()
                        .collect(),
                );
            }
            if ui.button("Cancel").clicked() {
                dialog.pending_delete = None;
            }
        });
    }
    ui.separator();
}
