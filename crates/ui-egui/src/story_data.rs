//! A scene's story data (ProseCraft `StoryDataPanel.svelte`, upstream `bcb884d`): under the
//! scene's title, its story date, place, confirmed characters (with the ones its text mentions
//! offered to confirm), tags, and the raw metadata. Never exported.

use egui::{RichText, Ui};
use prosecraft_core::js::JsValue;
use prosecraft_core::mentions::Matcher;
use prosecraft_core::network::meta::{
    Meta, normalize_meta, parse_story_meta, serialize_story_meta,
};
use prosecraft_core::network::relations::{Doc, Story};

use crate::editing::Intent;
use crate::theme::Palette;
use crate::views::{name_field, saved_field_response};

#[derive(Clone, Default)]
struct Raw {
    open: bool,
    text: String,
    error: String,
    tag: String,
}

fn muted(palette: &Palette, text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .size(12.0)
        .color(palette.text_muted)
}

/// `commit`: keys set; `None`, "" or an empty list removes them; then normalized.
fn commit(meta: Option<&JsValue>, key: &str, value: Option<JsValue>) -> Option<JsValue> {
    let mut next = meta
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default();
    let empty = match &value {
        None => true,
        Some(JsValue::String(s)) => s.is_empty(),
        Some(JsValue::Array(a)) => a.is_empty(),
        Some(_) => false,
    };
    match value {
        Some(v) if !empty => next.insert(key, v),
        _ => {
            next.remove(key);
        }
    }
    normalize_meta(&JsValue::Object(next))
}

fn strings(list: &[String]) -> JsValue {
    JsValue::Array(list.iter().map(|s| JsValue::str(s)).collect())
}

/// Characters the scene's text mentions that aren't confirmed (`detectedIds`).
fn detected(story: &Story, scene: &Doc, confirmed: &[String]) -> Vec<String> {
    let pool = story.character_pages();
    let mut articles: Vec<(&str, &str)> = Vec::new();
    for page in &pool {
        articles.push((&page.title, &page.id));
        for alias in &page.aliases {
            articles.push((alias, &page.id));
        }
    }
    let matcher = Matcher::new(articles.iter().map(|(t, _)| *t));
    let text = prosecraft_core::html::html_to_plain_text(
        &scene.content,
        &prosecraft_core::html::PlainTextOptions::default(),
    );
    let chars: Vec<char> = text.chars().collect();
    let mut ids: Vec<String> = Vec::new();
    for (_, article) in matcher.find(&chars) {
        let id = articles[article].1;
        if !confirmed.iter().any(|c| c == id) && !ids.iter().any(|i| i == id) {
            ids.push(id.to_owned());
        }
    }
    ids
}

/// The panel, for a live scene (`shouldShowStoryData`).
pub(crate) fn show(
    ui: &mut Ui,
    palette: &Palette,
    project: &JsValue,
    document: &str,
    open: &mut bool,
    editable: bool,
    intents: &mut Vec<Intent>,
) {
    let story = Story::from_project(project);
    let Some(scene) = story
        .doc(document)
        .filter(|d| prosecraft_core::network::tile::shows_story_data(d))
    else {
        return;
    };
    let meta = Meta(scene.meta.as_ref());
    let confirmed = meta.characters();
    let title = |id: &str| {
        story
            .doc(id)
            .map_or_else(|| id.to_owned(), |d| d.title.clone())
    };
    let place = meta
        .place()
        .map(|p| story.doc(&p).map_or(p.clone(), |d| d.title.clone()));
    let mut parts = Vec::new();
    if let Some(date) = meta.story_date() {
        parts.push(date);
    }
    if let Some(p) = place.clone().filter(|p| !p.is_empty()) {
        parts.push(p);
    }
    if !confirmed.is_empty() {
        parts.push(format!(
            "{} character{}",
            confirmed.len(),
            if confirmed.len() == 1 { "" } else { "s" }
        ));
    }
    let summary = if parts.is_empty() {
        "Add story date, place and characters".to_owned()
    } else {
        parts.join(" · ")
    };
    let toggle = ui
        .horizontal(|ui| {
            let arrow = if *open { "⏷" } else { "⏵" };
            let r = ui.selectable_label(*open, format!("{arrow} Story data"));
            ui.label(muted(palette, &summary));
            r
        })
        .inner;
    name_field(ui, &toggle, "Story data");
    if toggle.clicked() {
        *open = !*open;
    }
    if !*open {
        return;
    }
    let set = |intents: &mut Vec<Intent>, key: &str, value: Option<JsValue>| {
        intents.push(Intent::SetMeta {
            document: scene.id.clone(),
            meta: commit(scene.meta.as_ref(), key, value),
        });
    };
    let raw_id = ui.make_persistent_id(("story-data-raw", document));
    let mut raw: Raw = ui.data(|d| d.get_temp(raw_id)).unwrap_or_default();
    ui.add_enabled_ui(editable, |ui| {
        egui::Grid::new(("story-data", document))
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Story date");
                if let Some(text) = saved_field_response(
                    ui,
                    ("story-date", document),
                    &meta.story_date().unwrap_or_default(),
                    "e.g. Day 14, Year 412",
                    "Story date",
                    false,
                    40,
                    editable,
                )
                .0
                {
                    let text = text
                        .trim_matches(prosecraft_core::html::is_js_whitespace)
                        .to_owned();
                    set(intents, "storyDate", Some(JsValue::str(&text)));
                }
                ui.end_row();
                ui.label("Place");
                if let Some(text) = saved_field_response(
                    ui,
                    ("story-place", document),
                    &place.clone().unwrap_or_default(),
                    "Page or free text",
                    "Place",
                    false,
                    200,
                    editable,
                )
                .0
                {
                    let raw = text
                        .trim_matches(prosecraft_core::html::is_js_whitespace)
                        .to_owned();
                    // A place page's title stores the page's id.
                    let page = story
                        .pages()
                        .filter(|p| story.page_kind(p) == "place")
                        .find(|p| p.title.to_lowercase() == raw.to_lowercase());
                    set(
                        intents,
                        "place",
                        Some(JsValue::str(page.map_or(raw.as_str(), |p| p.id.as_str()))),
                    );
                }
                ui.end_row();
                ui.label("Characters");
                ui.horizontal_wrapped(|ui| {
                    for id in &confirmed {
                        ui.label(RichText::new(title(id)).background_color(palette.selected_bg));
                        let x = ui.small_button("×");
                        name_field(ui, &x, &format!("Remove {}", title(id)));
                        if x.clicked() {
                            let rest: Vec<String> =
                                confirmed.iter().filter(|c| *c != id).cloned().collect();
                            set(intents, "characters", Some(strings(&rest)));
                        }
                    }
                    for id in detected(&story, scene, &confirmed) {
                        let chip = ui
                            .button(format!("{} auto +", title(&id)))
                            .on_hover_text("Mentioned in this scene — click to confirm");
                        name_field(ui, &chip, &format!("{} auto", title(&id)));
                        if chip.clicked() {
                            let mut next = confirmed.clone();
                            next.push(id.clone());
                            set(intents, "characters", Some(strings(&next)));
                        }
                    }
                    let available: Vec<&Doc> = story
                        .character_pages()
                        .into_iter()
                        .filter(|p| !confirmed.contains(&p.id))
                        .collect();
                    if !available.is_empty() {
                        let mut chosen = None;
                        let combo = craft_ui::ComboBox::from_id_salt(("add-character", document))
                            .selected_text("+ Add character")
                            .show_ui(ui, |ui| {
                                for p in &available {
                                    if ui.selectable_label(false, &p.title).clicked() {
                                        chosen = Some(p.id.clone());
                                    }
                                }
                            });
                        ui.ctx().accesskit_node_builder(combo.response.id, |n| {
                            n.set_label("Add character")
                        });
                        if let Some(id) = chosen {
                            let mut next = confirmed.clone();
                            next.push(id);
                            set(intents, "characters", Some(strings(&next)));
                        }
                    }
                });
                ui.end_row();
                ui.label("Tags");
                ui.horizontal_wrapped(|ui| {
                    let tags = meta.tags();
                    for tag in &tags {
                        ui.label(RichText::new(tag).background_color(palette.selected_bg));
                        let x = ui.small_button("×");
                        name_field(ui, &x, &format!("Remove tag {tag}"));
                        if x.clicked() {
                            let rest: Vec<String> =
                                tags.iter().filter(|t| *t != tag).cloned().collect();
                            set(intents, "tags", Some(strings(&rest)));
                        }
                    }
                    let input = ui.add(
                        egui::TextEdit::singleline(&mut raw.tag)
                            .hint_text("+ tag")
                            .desired_width(80.0),
                    );
                    name_field(ui, &input, "Add tag");
                    if input.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        raw.tag.clear();
                    }
                    if input.lost_focus() {
                        // `addTag`: comma-separated, trimmed, each once.
                        let mut next = tags.clone();
                        for part in raw
                            .tag
                            .split(',')
                            .map(|p| p.trim_matches(prosecraft_core::html::is_js_whitespace))
                            .filter(|p| !p.is_empty())
                        {
                            if !next.iter().any(|t| t == part) {
                                next.push(part.to_owned());
                            }
                        }
                        if next.len() > tags.len() {
                            set(intents, "tags", Some(strings(&next)));
                        }
                        raw.tag.clear();
                    }
                });
                ui.end_row();
            });
        let fold = ui.selectable_label(
            raw.open,
            format!("{} Advanced: raw YAML", if raw.open { "⏷" } else { "⏵" }),
        );
        if fold.clicked() {
            raw.open = !raw.open;
            if raw.open {
                raw.text = serialize_story_meta(scene.meta.as_ref());
                raw.error.clear();
            }
        }
        if raw.open {
            let area = ui.add(
                egui::TextEdit::multiline(&mut raw.text)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(4),
            );
            name_field(ui, &area, "Scene metadata as YAML");
            if area.lost_focus() {
                match parse_story_meta(&raw.text) {
                    Ok(meta) => {
                        raw.error.clear();
                        raw.text = serialize_story_meta(meta.as_ref());
                        intents.push(Intent::SetMeta {
                            document: scene.id.clone(),
                            meta,
                        });
                    }
                    Err(e) => raw.error = e,
                }
            }
            if !raw.error.is_empty() {
                ui.label(RichText::new(&raw.error).color(palette.danger));
            }
        }
    });
    ui.label(muted(palette, "Never exported"));
    ui.data_mut(|d| d.insert_temp(raw_id, raw));
}
