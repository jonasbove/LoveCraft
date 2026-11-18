//! ProseCraft's citation picker (`CitationPicker.svelte`): choose sources from the project's
//! library for a citation at the caret, or change or remove an existing one. Each chosen
//! source can carry a locator (page, chapter or section), a prefix, a suffix, and suppress its
//! author; a preview shows the citation in the publish style.

use egui::{RichText, Ui};
use prosecraft_core::citations::format::{CitationContext, CitationInput, format_citation};
use prosecraft_core::citations::library::{
    format_item_contributors, item_year_text, matches_library_query, source_type_label,
};
use prosecraft_core::cite::{CiteRefItem, cite_marker_html};
use prosecraft_core::html::is_js_whitespace;
use prosecraft_core::js::JsValue;

use crate::AppState;
use crate::editing::Intent;

/// Where the citation goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// A new citation at this character of the scene.
    Insert { at: usize },
    /// The scene's `atom`th atom, an existing citation.
    Edit { atom: usize },
}

/// One chosen source while the picker is open (every field a plain value, as the form holds).
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    id: String,
    locator: String,
    label: String,
    prefix: String,
    suffix: String,
    suppress_author: bool,
}

impl Entry {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            locator: String::new(),
            label: "page".into(),
            prefix: String::new(),
            suffix: String::new(),
            suppress_author: false,
        }
    }

    /// `cleanCiteEntry`.
    fn clean(&self) -> CiteRefItem {
        let locator = self.locator.trim_matches(is_js_whitespace);
        let prefix = self.prefix.trim_start_matches(is_js_whitespace);
        let suffix = self.suffix.trim_matches(is_js_whitespace);
        CiteRefItem {
            id: self.id.clone(),
            locator: (!locator.is_empty()).then(|| locator.to_owned()),
            label: (!locator.is_empty()).then(|| self.label.clone()),
            prefix: (!prefix.trim_matches(is_js_whitespace).is_empty()).then(|| prefix.to_owned()),
            suffix: (!suffix.is_empty()).then(|| suffix.to_owned()),
            suppress_author: self.suppress_author,
        }
    }
}

/// The picker while it is open.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CitationPicker {
    pub document: String,
    pub target: Target,
    /// Where it opens (beside the caret or the citation), once the editor knows.
    pub pos: Option<egui::Pos2>,
    selected: Vec<Entry>,
    active: usize,
    search: String,
    focus_search: bool,
}

impl CitationPicker {
    /// The picker for a new citation, or seeded with an existing one's sources.
    pub fn new(document: &str, target: Target, initial: &[CiteRefItem]) -> Self {
        Self {
            document: document.to_owned(),
            target,
            pos: None,
            selected: initial
                .iter()
                .map(|c| Entry {
                    id: c.id.clone(),
                    locator: c.locator.clone().unwrap_or_default(),
                    label: c.label.clone().unwrap_or_else(|| "page".into()),
                    prefix: c.prefix.clone().unwrap_or_default(),
                    suffix: c.suffix.clone().unwrap_or_default(),
                    suppress_author: c.suppress_author,
                })
                .collect(),
            active: 0,
            search: String::new(),
            focus_search: true,
        }
    }

    fn editing(&self) -> bool {
        matches!(self.target, Target::Edit { .. })
    }
}

fn text(item: &JsValue, key: &str) -> String {
    item.get(key)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

/// Whether the source is the entry's (by id or cite key).
fn is_source(item: &JsValue, id: &str) -> bool {
    let key = text(item, "citeKey");
    text(item, "id") == id || (!key.is_empty() && key == id)
}

/// The library by id, then by cite key (`lookupByKey`).
fn lookup<'a>(items: &'a [JsValue], id: &str) -> Option<&'a JsValue> {
    items
        .iter()
        .find(|i| text(i, "id") == id)
        .or_else(|| items.iter().find(|i| text(i, "citeKey") == id))
}

/// `labelForChip`: the first author's or editor's surname (or the title), and the year.
fn chip_label(items: &[JsValue], id: &str) -> String {
    let Some(item) = lookup(items, id) else {
        return format!("@{id}");
    };
    let first = ["author", "editor"].iter().find_map(|key| {
        item.get(key)
            .and_then(JsValue::as_array)
            .and_then(|names| names.first())
            .cloned()
    });
    let lead = first
        .as_ref()
        .and_then(|n| {
            n.get("family")
                .or_else(|| n.get("literal"))
                .and_then(JsValue::as_js_string)
        })
        .map(|s| s.to_string_lossy())
        .or_else(|| {
            item.get("title")
                .and_then(JsValue::as_js_string)
                .map(|s| s.to_string_lossy())
        })
        .unwrap_or_else(|| text(item, "id"));
    let year = item_year_text(item);
    if year.is_empty() {
        lead
    } else {
        format!("{lead} ({year})")
    }
}

/// The picker's preview: the chosen sources formatted, missing ones kept as `[@key]`.
fn preview(items: &[JsValue], selected: &[Entry], style: &str, locale: &str) -> Option<String> {
    if selected.is_empty() {
        return None;
    }
    let mut inputs = Vec::new();
    let mut missing = Vec::new();
    for entry in selected {
        let clean = entry.clean();
        let Some(item) = lookup(items, &clean.id) else {
            missing.push(format!("@{}", clean.id));
            continue;
        };
        inputs.push(CitationInput {
            item: item.clone(),
            locator: clean.locator,
            label: clean.label,
            prefix: clean.prefix,
            suffix: clean.suffix,
            suppress_author: clean.suppress_author,
        });
    }
    if inputs.is_empty() {
        return Some(format!("[{}]", missing.join("; ")));
    }
    let formatted = format_citation(
        style,
        &inputs,
        &CitationContext {
            index: Some(1.0),
            locale: locale.to_owned(),
            library: Some(items.to_vec()),
            ..CitationContext::default()
        },
    );
    Some(if missing.is_empty() {
        formatted.text
    } else {
        format!("{} [{}]", formatted.text, missing.join("; "))
    })
}

/// What the picker asked for this frame.
enum Outcome {
    Commit(String),
    Remove,
    Close,
    OpenSources,
}

const LOCATORS: [(&str, &str); 3] = [
    ("page", "Page"),
    ("chapter", "Chapter"),
    ("section", "Section"),
];

fn library_list(
    ui: &mut Ui,
    palette: &crate::theme::Palette,
    items: &[JsValue],
    picker: &mut CitationPicker,
) -> bool {
    let mut open_sources = false;
    egui::ScrollArea::vertical()
        .id_salt("picker-library")
        .max_height(200.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if items.is_empty() {
                ui.label("No sources in this project yet.");
                open_sources = ui.button("Open Sources…").clicked();
                return;
            }
            let shown: Vec<&JsValue> = items
                .iter()
                .filter(|i| matches_library_query(i, &picker.search))
                .collect();
            if shown.is_empty() {
                ui.label(
                    RichText::new(format!("No sources match “{}”.", picker.search))
                        .color(palette.text_muted),
                );
            }
            for item in shown {
                let chosen = picker.selected.iter().position(|e| is_source(item, &e.id));
                let title = text(item, "title");
                let title = if title.is_empty() {
                    "(Untitled)".to_owned()
                } else {
                    title
                };
                let contributors = format_item_contributors(item);
                let year = item_year_text(item);
                let meta = format!(
                    "{} · {} · {}",
                    if contributors.is_empty() {
                        "Unknown author"
                    } else {
                        &contributors
                    },
                    if year.is_empty() { "n.d." } else { &year },
                    source_type_label(&text(item, "type"))
                );
                let row = ui.add(
                    egui::Button::selectable(
                        chosen.is_some(),
                        format!("{}  {title}", if chosen.is_some() { "✓" } else { "+" }),
                    )
                    .min_size(egui::vec2(ui.available_width(), 0.0)),
                );
                crate::views::name_field(ui, &row, &title);
                ui.label(RichText::new(meta).size(11.0).color(palette.text_muted));
                if row.clicked() {
                    // `addSource`: a source already chosen becomes the active one.
                    picker.active = chosen.unwrap_or_else(|| {
                        picker.selected.push(Entry::new(&text(item, "id")));
                        picker.selected.len() - 1
                    });
                }
            }
        });
    open_sources
}

fn chips(ui: &mut Ui, items: &[JsValue], picker: &mut CitationPicker) {
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        for (index, entry) in picker.selected.iter().enumerate() {
            let label = chip_label(items, &entry.id);
            if ui
                .selectable_label(index == picker.active, &label)
                .clicked()
            {
                picker.active = index;
            }
            let x = ui.small_button("×");
            crate::views::name_field(ui, &x, &format!("Remove {label}"));
            if x.clicked() {
                remove = Some(index);
            }
        }
    });
    if let Some(index) = remove {
        // `removeSelectedAt`.
        picker.selected.remove(index);
        if picker.active >= picker.selected.len() {
            picker.active = picker.selected.len().saturating_sub(1);
        }
    }
}

fn labelled_field(ui: &mut Ui, label: &str, value: &mut String, hint: &str) {
    ui.label(RichText::new(label).size(12.0));
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    );
    crate::views::name_field(ui, &response, label);
}

/// The active source's locator, prefix, suffix and author setting.
fn fields(ui: &mut Ui, entry: &mut Entry) {
    ui.columns(2, |cols| {
        cols[0].label(RichText::new("Locator type").size(12.0));
        let shown = LOCATORS
            .iter()
            .find(|(v, _)| *v == entry.label)
            .map_or("Page", |(_, l)| l);
        let combo = craft_ui::ComboBox::from_id_salt("locator-type")
            .selected_text(shown)
            .show_ui(&mut cols[0], |ui| {
                for (value, label) in LOCATORS {
                    ui.selectable_value(&mut entry.label, value.to_owned(), label);
                }
            });
        cols[0]
            .ctx()
            .accesskit_node_builder(combo.response.id, |node| node.set_label("Locator type"));
        labelled_field(&mut cols[1], "Locator", &mut entry.locator, "42 or 42-44");
    });
    ui.columns(2, |cols| {
        labelled_field(&mut cols[0], "Prefix", &mut entry.prefix, "see ");
        labelled_field(&mut cols[1], "Suffix", &mut entry.suffix, "and passim");
    });
    ui.checkbox(&mut entry.suppress_author, "Suppress author");
}

fn footer(
    ui: &mut Ui,
    palette: &crate::theme::Palette,
    picker: &CitationPicker,
) -> Option<Outcome> {
    let mut outcome = None;
    ui.horizontal(|ui| {
        if picker.editing()
            && ui
                .button(RichText::new("Remove").color(palette.danger))
                .clicked()
        {
            outcome = Some(Outcome::Remove);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if picker.editing() { "Update" } else { "Insert" };
            let commit = ui.add_enabled(
                !picker.selected.is_empty(),
                egui::Button::new(RichText::new(label).strong()),
            );
            if commit.clicked() {
                outcome = Some(Outcome::Commit(String::new()));
            }
            if ui.button("Cancel").clicked() {
                outcome = Some(Outcome::Close);
            }
        });
    });
    outcome
}

/// The picker.
pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let Some(project) = state
        .library
        .as_ref()
        .and_then(|l| l.projects().get(state.active))
    else {
        state.citation_picker = None;
        return;
    };
    let items = project.view.citations.clone();
    let options = prosecraft_core::publish::PublishOptions::read(
        project
            .stored
            .get("project")
            .and_then(|p| p.get("publishOptions")),
    );
    let Some(picker) = state.citation_picker.as_mut() else {
        return;
    };
    let Some(pos) = picker.pos else {
        // The editor places it next frame.
        return;
    };
    let mut outcome = None;
    let title = if picker.editing() {
        "Edit citation"
    } else {
        "Insert citation"
    };
    let mut open = true;
    egui::Window::new(title)
        .id(egui::Id::unique("citation-picker"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(380.0)
        .fixed_pos(pos)
        .constrain(true)
        .show(ui.ctx(), |ui| {
            ui.set_width(380.0);
            ui.horizontal(|ui| {
                let search = ui.add(
                    egui::TextEdit::singleline(&mut picker.search)
                        .hint_text("Search sources…")
                        .desired_width(280.0),
                );
                crate::views::name_field(ui, &search, "Search citation sources");
                if std::mem::take(&mut picker.focus_search) {
                    search.request_focus();
                }
                if ui.link("Sources…").clicked() {
                    outcome = Some(Outcome::OpenSources);
                }
            });
            if library_list(ui, &palette, &items, picker) {
                outcome = Some(Outcome::OpenSources);
            }
            if !picker.selected.is_empty() {
                ui.separator();
                chips(ui, &items, picker);
                if let Some(entry) = picker.selected.get_mut(picker.active) {
                    fields(ui, entry);
                }
                if let Some(text) = preview(
                    &items,
                    &picker.selected,
                    &options.citation_style,
                    &options.citation_locale,
                ) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("PREVIEW").size(10.0).color(palette.eyebrow));
                        let shown = ui.label(&text);
                        crate::views::name_field(ui, &shown, &format!("Citation preview: {text}"));
                    });
                }
            }
            ui.separator();
            if let Some(o) = footer(ui, &palette, picker) {
                outcome = Some(o);
            }
            // Escape closes; Ctrl+Enter inserts (`handleKeydown`).
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                outcome = Some(Outcome::Close);
            }
            if !picker.selected.is_empty()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter))
            {
                outcome = Some(Outcome::Commit(String::new()));
            }
        });
    if !open {
        outcome = Some(Outcome::Close);
    }
    let outcome = match outcome {
        Some(Outcome::Commit(_)) => {
            let cites: Vec<CiteRefItem> = picker.selected.iter().map(Entry::clean).collect();
            cite_marker_html(&cites).map(Outcome::Commit)
        }
        other => other,
    };
    let document = picker.document.clone();
    let target = picker.target;
    match outcome {
        Some(Outcome::Commit(html)) => {
            state.intents.push(Intent::PlaceCitation {
                document,
                target,
                html,
            });
            state.citation_picker = None;
        }
        Some(Outcome::Remove) => {
            if let Target::Edit { atom } = target {
                state
                    .intents
                    .push(Intent::RemoveCitation { document, atom });
            }
            state.citation_picker = None;
        }
        Some(Outcome::Close) => state.citation_picker = None,
        Some(Outcome::OpenSources) => {
            state.citation_picker = None;
            state.sources = Some(crate::sources::SourcesDialog::default());
        }
        None => {}
    }
}
