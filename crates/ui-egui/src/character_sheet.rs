//! A Wiki page's character card (ProseCraft `CharacterCardPanel.svelte`,
//! `CharacterSheet.svelte`, `SheetTemplateEditor.svelte` and `AppearanceStrip.svelte`, upstream
//! `bcb884d`): the page's aliases and sheet fields from a template, its relations with a small
//! neighbourhood graph, where it appears chapter by chapter, and the template editor.

use egui::{RichText, Stroke, Ui};
use prosecraft_core::js::JsValue;
use prosecraft_core::network::graph;
use prosecraft_core::network::layout::{LayoutEdge, LayoutNode, Options, layout};
use prosecraft_core::network::meta::{Meta, normalize_meta};
use prosecraft_core::network::relations::{Doc, reverse_view};
use prosecraft_core::network::templates::{
    self, Field, MAX_TEMPLATE_FIELDS, MAX_TEMPLATES, Template, ValueError,
};
use prosecraft_core::network::tile::chapter_bars;

use crate::editing::Intent;
use crate::network_view::{Derived, NetworkState, Out};
use crate::relation_graph::{kind_name, plural, relation_color, relation_name};
use crate::theme::Palette;
use crate::views::{name_field, saved_field_response};
use crate::widgets::{card, eyebrow};
use crate::{Selection, fonts};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Profile,
    Relations,
    Appearances,
    Notes,
}

/// The template editor's draft (`SheetTemplateEditor.svelte`).
#[derive(Clone, Debug)]
pub struct Editor {
    draft: Template,
    status: String,
}

#[derive(Clone, Debug, Default)]
pub struct SheetState {
    page: String,
    tab: Tab,
    alias: String,
    error: Option<(String, &'static str)>,
    editor: Option<Editor>,
}

fn muted(palette: &Palette, text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .size(12.0)
        .color(palette.text_muted)
}

fn project_templates(project: &JsValue) -> Vec<Template> {
    templates::project_templates(project)
}

/// `commit`: keys set, `None` or "" removing them, then normalized.
fn commit(meta: Option<&JsValue>, patch: Vec<(&str, Option<JsValue>)>) -> Option<JsValue> {
    let mut next = meta
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default();
    for (key, value) in patch {
        match value {
            Some(JsValue::String(s)) if s.is_empty() => {
                next.remove(key);
            }
            None => {
                next.remove(key);
            }
            Some(v) => next.insert(key, v),
        }
    }
    normalize_meta(&JsValue::Object(next))
}

fn set_meta(out: &mut Out, page: &Doc, meta: Option<JsValue>) {
    out.intents.push(Intent::SetMeta {
        document: page.id.clone(),
        meta,
    });
}

/// `CharacterCardPanel.svelte`: the sheet in a panel at the right, closed with × or Escape.
pub(crate) fn card_panel(
    ui: &mut Ui,
    palette: &Palette,
    project: &JsValue,
    derived: &Derived,
    page: &Doc,
    state: &mut NetworkState,
    out: &mut Out,
) {
    if state.sheet.page != page.id {
        state.sheet = SheetState {
            page: page.id.clone(),
            ..SheetState::default()
        };
    }
    let mut open = true;
    let title = if page.title.is_empty() {
        "Untitled"
    } else {
        &page.title
    };
    egui::Window::new(format!("{title} character card"))
        .id(egui::Id::unique("character-card"))
        .title_bar(false)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-16.0, 80.0))
        .default_width(420.0)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                eyebrow(ui, palette, "Character card");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let close = ui.small_button("×");
                    name_field(ui, &close, "Close character card");
                    if close.clicked() {
                        open = false;
                    }
                });
            });
            egui::ScrollArea::vertical()
                .max_height(ui.ctx().content_rect().height() - 160.0)
                .show(ui, |ui| {
                    sheet(ui, palette, project, derived, page, &mut state.sheet, out);
                });
        });
    if state.sheet.editor.is_none() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    if !open {
        state.card = None;
    }
}

/// `CharacterSheet.svelte`.
pub(crate) fn sheet(
    ui: &mut Ui,
    palette: &Palette,
    project: &JsValue,
    derived: &Derived,
    page: &Doc,
    state: &mut SheetState,
    out: &mut Out,
) {
    let meta = Meta(page.meta.as_ref());
    let kind = derived.story.page_kind(page);
    let all = templates::all_templates(project);
    let template = meta
        .sheet_template()
        .and_then(|id| all.iter().find(|t| t.id == id))
        .cloned();
    let fields = template
        .as_ref()
        .map(|t| t.fields.clone())
        .unwrap_or_default();
    let orphans: Vec<(String, String)> = meta
        .sheet()
        .into_iter()
        .filter(|(k, _)| !fields.iter().any(|f| f.id == *k))
        .collect();
    let aliases = meta.aliases();
    let has_sheet = kind == "character"
        || meta.sheet_template().is_some()
        || !orphans.is_empty()
        || !aliases.is_empty();
    if !has_sheet {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(out.editable, egui::Button::new("+ Add sheet"))
                .clicked()
            {
                set_meta(
                    out,
                    page,
                    commit(
                        page.meta.as_ref(),
                        vec![("sheetTemplate", Some(JsValue::str("blank")))],
                    ),
                );
            }
            ui.label(muted(palette, "Profile fields for this page."));
        });
        return;
    }
    head(
        ui,
        palette,
        page,
        kind,
        template.as_ref(),
        &aliases,
        state,
        out,
    );
    ui.horizontal(|ui| {
        for (tab, label) in [
            (Tab::Profile, "Profile"),
            (Tab::Relations, "Relations"),
            (Tab::Appearances, "Appearances"),
            (Tab::Notes, "Notes"),
        ] {
            if ui.selectable_label(state.tab == tab, label).clicked() {
                state.tab = tab;
            }
        }
    });
    ui.separator();
    match state.tab {
        Tab::Profile => profile(
            ui,
            palette,
            project,
            derived,
            page,
            template.as_ref(),
            &fields,
            &orphans,
            state,
            out,
        ),
        Tab::Relations => relations_tab(ui, palette, derived, page, out),
        Tab::Appearances => strip(ui, palette, derived, page, out),
        Tab::Notes => {
            ui.label(RichText::new("Notes").strong());
            ui.label(muted(palette, "Notes are the text below."));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn head(
    ui: &mut Ui,
    palette: &Palette,
    page: &Doc,
    kind: &str,
    template: Option<&Template>,
    aliases: &[String],
    state: &mut SheetState,
    out: &mut Out,
) {
    ui.horizontal(|ui| {
        let initial: String = page
            .title
            .trim_matches(prosecraft_core::html::is_js_whitespace)
            .chars()
            .next()
            .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect());
        ui.label(
            RichText::new(initial)
                .font(fonts::serif(26.0))
                .color(palette.accent),
        );
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(if page.title.is_empty() {
                        "Untitled"
                    } else {
                        &page.title
                    })
                    .font(fonts::serif(20.0)),
                );
                let mut subtitle = kind_name(kind).to_owned();
                if let Some(t) = template {
                    subtitle.push_str(&format!(" · {} template", t.name));
                }
                ui.label(muted(palette, subtitle));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(muted(palette, "Aliases"));
                for (index, alias) in aliases.iter().enumerate() {
                    ui.label(RichText::new(alias).background_color(palette.hover_bg));
                    let remove =
                        ui.add_enabled(out.editable, egui::Button::new("×").small().frame(false));
                    name_field(ui, &remove, &format!("Remove {alias}"));
                    if remove.clicked() {
                        let rest: Vec<JsValue> = aliases
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| *i != index)
                            .map(|(_, a)| JsValue::str(a))
                            .collect();
                        set_meta(
                            out,
                            page,
                            commit(
                                page.meta.as_ref(),
                                vec![("aliases", Some(JsValue::Array(rest)))],
                            ),
                        );
                    }
                }
                let input = ui.add_enabled(
                    out.editable,
                    egui::TextEdit::singleline(&mut state.alias)
                        .hint_text("+ alias")
                        .desired_width(90.0),
                );
                name_field(ui, &input, "Add alias");
                if input.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    state.alias.clear();
                }
                if input.lost_focus() {
                    add_alias(page, aliases, state, out);
                }
            });
        });
    });
}

/// `addAlias`: trimmed, and not again in another case.
fn add_alias(page: &Doc, aliases: &[String], state: &mut SheetState, out: &mut Out) {
    let alias = state
        .alias
        .trim_matches(prosecraft_core::html::is_js_whitespace)
        .to_owned();
    state.alias.clear();
    if alias.is_empty()
        || aliases
            .iter()
            .any(|a| a.to_lowercase() == alias.to_lowercase())
    {
        return;
    }
    let mut list: Vec<JsValue> = aliases.iter().map(|a| JsValue::str(a)).collect();
    list.push(JsValue::str(&alias));
    set_meta(
        out,
        page,
        commit(
            page.meta.as_ref(),
            vec![("aliases", Some(JsValue::Array(list)))],
        ),
    );
}

fn set_value(out: &mut Out, page: &Doc, field: &str, value: &str) {
    let next = templates::set_sheet_value(page.meta.as_ref(), field, value);
    set_meta(out, page, next.as_ref().and_then(normalize_meta));
}

#[allow(clippy::too_many_arguments)]
fn profile(
    ui: &mut Ui,
    palette: &Palette,
    project: &JsValue,
    derived: &Derived,
    page: &Doc,
    template: Option<&Template>,
    fields: &[Field],
    orphans: &[(String, String)],
    state: &mut SheetState,
    out: &mut Out,
) {
    let all = templates::all_templates(project);
    ui.horizontal(|ui| {
        ui.label("Template");
        let shown = template.map_or("No template", |t| t.name.as_str());
        let mut chosen: Option<Option<Template>> = None;
        let combo = craft_ui::ComboBox::from_id_salt(("sheet-template", &page.id))
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(template.is_none(), "No template")
                    .clicked()
                {
                    chosen = Some(None);
                }
                for t in &all {
                    if ui
                        .selectable_label(template.is_some_and(|c| c.id == t.id), &t.name)
                        .clicked()
                    {
                        chosen = Some(Some(t.clone()));
                    }
                }
            });
        ui.ctx()
            .accesskit_node_builder(combo.response.id, |node| node.set_label("Template"));
        match chosen {
            Some(None) if out.editable => {
                state.error = None;
                set_meta(
                    out,
                    page,
                    commit(page.meta.as_ref(), vec![("sheetTemplate", None)]),
                );
            }
            Some(Some(t)) if out.editable => apply_template(out, page, &t, state),
            _ => {}
        }
        if ui
            .add_enabled(out.editable, egui::Button::new("Manage templates"))
            .clicked()
        {
            let draft = template
                .cloned()
                .or_else(|| all.first().cloned())
                .unwrap_or(Template {
                    id: "blank".into(),
                    name: "Blank".into(),
                    fields: Vec::new(),
                });
            state.editor = Some(Editor {
                draft,
                status: String::new(),
            });
        }
    });
    if fields.is_empty() {
        ui.label(muted(
            palette,
            if template.is_some() {
                "This template has no fields yet. Pick another template or edit it in the template editor."
            } else {
                "Pick a template to add profile fields."
            },
        ));
    } else {
        ui.label(muted(palette, "Profile fields"));
        for field in fields {
            field_row(ui, palette, derived, page, field, state, out);
        }
    }
    if !orphans.is_empty() {
        ui.label(muted(palette, "Other fields"));
        ui.label(muted(
            palette,
            "Values without a field in this template are kept here.",
        ));
        let labels: Vec<Field> = all.iter().flat_map(|t| t.fields.clone()).collect();
        for (key, value) in orphans {
            let label = labels
                .iter()
                .find(|f| f.id == *key)
                .map_or(key.as_str(), |f| f.label.as_str());
            ui.label(label);
            if let Some(text) = saved_field_response(
                ui,
                ("sheet-orphan", key),
                value,
                "",
                label,
                false,
                8000,
                out.editable,
            )
            .0
            {
                set_value(out, page, key, &text);
            }
        }
    }
    if state.editor.is_some() {
        template_editor(ui, palette, project, page, state, out);
    }
}

fn apply_template(out: &mut Out, page: &Doc, template: &Template, state: &mut SheetState) {
    state.error = None;
    let (meta, _) = templates::apply_template(page.meta.as_ref(), template);
    let mut object = meta.as_object().cloned().unwrap_or_default();
    object.insert("sheetTemplate", JsValue::str(&template.id));
    set_meta(out, page, normalize_meta(&JsValue::Object(object)));
}

fn field_row(
    ui: &mut Ui,
    palette: &Palette,
    derived: &Derived,
    page: &Doc,
    field: &Field,
    state: &mut SheetState,
    out: &mut Out,
) {
    let meta = Meta(page.meta.as_ref());
    let value = meta.sheet_value(&field.id);
    ui.label(&field.label);
    match field.kind.as_str() {
        "choice" => {
            let mut chosen = None;
            let combo = craft_ui::ComboBox::from_id_salt(("sheet-choice", &page.id, &field.id))
                .selected_text(if value.is_empty() {
                    "Choose…"
                } else {
                    value.as_str()
                })
                .show_ui(ui, |ui| {
                    if ui.selectable_label(value.is_empty(), "Choose…").clicked() {
                        chosen = Some(String::new());
                    }
                    for option in field.options.as_deref().unwrap_or_default() {
                        if ui.selectable_label(value == *option, option).clicked() {
                            chosen = Some(option.clone());
                        }
                    }
                });
            let label = field.label.clone();
            ui.ctx()
                .accesskit_node_builder(combo.response.id, move |node| node.set_label(label));
            if let Some(text) = chosen.filter(|_| out.editable) {
                check_and_set(out, page, field, &text, state);
            }
        }
        "link" => {
            // A typed title that matches a Wiki page stores the page's id.
            let shown = derived.title(&value).map_or(value.clone(), str::to_owned);
            if let Some(text) = saved_field_response(
                ui,
                ("sheet-link", &field.id),
                &shown,
                "",
                &field.label,
                false,
                8000,
                out.editable,
            )
            .0
            {
                let text = text
                    .trim_matches(prosecraft_core::html::is_js_whitespace)
                    .to_owned();
                let target = derived.story.pages().find(|p| {
                    p.id != page.id
                        && p.title
                            .trim_matches(prosecraft_core::html::is_js_whitespace)
                            .to_lowercase()
                            == text.to_lowercase()
                });
                match target {
                    Some(p) => {
                        state.error = None;
                        set_value(out, page, &field.id, &p.id);
                    }
                    None => check_and_set(out, page, field, &text, state),
                }
            }
        }
        kind => {
            if let Some(text) = saved_field_response(
                ui,
                ("sheet-field", &field.id),
                &value,
                "",
                &field.label,
                kind == "longtext",
                8000,
                out.editable,
            )
            .0
            {
                check_and_set(out, page, field, &text, state);
            }
        }
    }
    if let Some((id, message)) = &state.error
        && *id == field.id
    {
        ui.label(RichText::new(*message).color(palette.danger));
    }
}

/// `onFieldChange`: a value its field refuses is reported and not stored.
fn check_and_set(out: &mut Out, page: &Doc, field: &Field, text: &str, state: &mut SheetState) {
    match templates::validate_sheet_value(field, text) {
        Some(error) => {
            state.error = Some((
                field.id.clone(),
                match error {
                    ValueError::Number => "Enter a number.",
                    ValueError::Choice => "Choose one of the listed options.",
                },
            ));
        }
        None => {
            state.error = None;
            set_value(out, page, &field.id, text);
        }
    }
}

/// `chapterRange`.
fn chapter_range(
    relation: &prosecraft_core::network::meta::Relation,
    chapters: &[String],
) -> String {
    let index = |id: &Option<String>| {
        id.as_ref()
            .and_then(|i| chapters.iter().position(|c| c == i))
    };
    match (index(&relation.from), index(&relation.until)) {
        (None, None) => "All chapters".into(),
        (Some(f), Some(u)) if f == u => format!("Ch {}", f + 1),
        (Some(f), Some(u)) => format!("Ch {}–{}", f + 1, u + 1),
        (Some(f), None) => format!("From Ch {}", f + 1),
        (None, Some(u)) => format!("Until Ch {}", u + 1),
    }
}

fn relations_tab(ui: &mut Ui, palette: &Palette, derived: &Derived, page: &Doc, out: &mut Out) {
    let outgoing = derived
        .relations
        .iter()
        .filter(|s| s.source == page.id)
        .cloned();
    let incoming = reverse_view(&derived.relations)
        .into_iter()
        .filter(|s| s.source == page.id);
    let live = |id: &str| derived.story.docs.iter().any(|d| d.id == id && !d.trashed);
    let rows: Vec<_> = outgoing
        .chain(incoming)
        .filter(|s| live(&s.relation.to))
        .collect();
    if rows.is_empty() {
        ui.label(muted(
            palette,
            "No relations yet. Add them in the network view.",
        ));
    }
    for s in &rows {
        let r = &s.relation;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("● {}", relation_name(&r.kind)))
                    .color(relation_color(palette, &r.kind)),
            );
            ui.label(if r.id.starts_with("reverse:") {
                "←"
            } else {
                "→"
            });
            let title = derived
                .story
                .docs
                .iter()
                .find(|d| d.id == r.to)
                .map_or("", |d| d.title.as_str());
            ui.label(title);
            if let Some(label) = &r.label {
                ui.label(muted(palette, label));
            }
            ui.label(muted(palette, chapter_range(r, &derived.story.chapters)));
        });
    }
    ui.label(muted(palette, "Neighbourhood"));
    mini_graph(ui, palette, derived, page);
    if ui.button("Open graph beside").clicked() {
        out.intents
            .push(Intent::OpenBeside(prosecraft_core::split::graph_target(
                Some(&page.id),
            )));
    }
}

/// `miniGraph`: the page and its neighbours in a small drawing.
fn mini_graph(ui: &mut Ui, palette: &Palette, derived: &Derived, page: &Doc) {
    const W: f32 = 260.0;
    const H: f32 = 140.0;
    const PAD: f32 = 22.0;
    let pages: Vec<graph::Page> = derived
        .story
        .pages()
        .map(|d| graph::Page {
            id: d.id.clone(),
            title: d.title.clone(),
            kind: derived.story.page_kind(d),
        })
        .collect();
    let model = graph::build(&graph::Input {
        pages: &pages,
        relations: &derived.relations,
        appearances: &derived.appearances,
        suggestions: &[],
        chapters: &derived.story.chapters,
        chapter_id: None,
        scene_chapters: None,
        kinds: None,
    });
    let mut near = vec![page.id.clone()];
    for e in &model.edges {
        if e.from == page.id {
            near.push(e.to.clone());
        }
        if e.to == page.id {
            near.push(e.from.clone());
        }
    }
    let nodes: Vec<&graph::Node> = model
        .nodes
        .iter()
        .filter(|n| near.contains(&n.id))
        .collect();
    let edges: Vec<&graph::Edge> = model
        .edges
        .iter()
        .filter(|e| near.contains(&e.from) && near.contains(&e.to))
        .collect();
    if nodes.is_empty() {
        ui.label(muted(palette, "No connected pages yet."));
        return;
    }
    let points = layout(
        &nodes
            .iter()
            .map(|n| LayoutNode {
                id: &n.id,
                at: None,
                pinned: false,
            })
            .collect::<Vec<_>>(),
        &edges
            .iter()
            .map(|e| LayoutEdge {
                from: &e.from,
                to: &e.to,
                weight: None,
            })
            .collect::<Vec<_>>(),
        &Options {
            width: Some(f64::from(W)),
            height: Some(f64::from(H)),
            iterations: Some(160.0),
            min_distance: Some(46.0),
            seed: Some(7.0),
            ..Options::default()
        },
    );
    let xs: Vec<f64> = nodes
        .iter()
        .map(|n| points.get(&n.id).map_or(0.0, |p| p.x))
        .collect();
    let ys: Vec<f64> = nodes
        .iter()
        .map(|n| points.get(&n.id).map_or(0.0, |p| p.y))
        .collect();
    let (min_x, max_x) = xs
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| {
            (a.min(*x), b.max(*x))
        });
    let (min_y, max_y) = ys
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), y| {
            (a.min(*y), b.max(*y))
        });
    let (rect, response) = ui.allocate_exact_size(egui::vec2(W, H), egui::Sense::hover());
    name_field(ui, &response, "This page and its relations");
    let place = |id: &str| {
        let p = points.get(id).copied().unwrap_or_default();
        let x = if max_x == min_x {
            W / 2.0
        } else {
            PAD + ((p.x - min_x) / (max_x - min_x)) as f32 * (W - PAD * 2.0)
        };
        let y = if max_y == min_y {
            H / 2.0
        } else {
            PAD + ((p.y - min_y) / (max_y - min_y)) as f32 * (H - PAD * 2.0)
        };
        rect.min + egui::vec2(x, y)
    };
    let painter = ui.painter_at(rect);
    for e in edges {
        painter.line_segment(
            [place(&e.from), place(&e.to)],
            Stroke::new(1.5, relation_color(palette, &e.kind)),
        );
    }
    for n in nodes {
        let c = place(&n.id);
        let me = n.id == page.id;
        painter.circle(
            c,
            if me { 10.0 } else { 8.0 },
            if me {
                palette.accent
            } else {
                palette.surface_raised
            },
            Stroke::new(
                1.0,
                if me {
                    palette.accent
                } else {
                    palette.border_control
                },
            ),
        );
        painter.text(
            c,
            egui::Align2::CENTER_CENTER,
            &n.initials,
            egui::FontId::proportional(8.0),
            if me {
                palette.page_bg
            } else {
                palette.text_primary
            },
        );
        painter.text(
            c + egui::vec2(0.0, 19.0),
            egui::Align2::CENTER_CENTER,
            &n.label,
            egui::FontId::proportional(9.0),
            palette.text_muted,
        );
    }
}

/// `AppearanceStrip.svelte`: a bar per chapter, confirmed and detected scenes stacked.
fn strip(ui: &mut Ui, palette: &Palette, derived: &Derived, page: &Doc, out: &mut Out) {
    let bars = chapter_bars(derived.appearance(&page.id), &derived.story);
    let visited = bars.iter().filter(|b| b.total > 0).count();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Appearances").strong());
        ui.label(muted(
            palette,
            if visited == 0 {
                "No appearances yet".to_owned()
            } else {
                format!("{visited} of {} chapters", bars.len())
            },
        ));
    });
    if bars.is_empty() {
        ui.label(muted(
            palette,
            "Chapters show up here once the project has some.",
        ));
        return;
    }
    let peak = bars.iter().map(|b| b.total).max().unwrap_or(0).max(1);
    ui.horizontal(|ui| {
        for (index, bar) in bars.iter().enumerate() {
            let mut marks = Vec::new();
            if bar.first {
                marks.push("first appearance");
            }
            if bar.last {
                marks.push("last appearance");
            }
            let suffix = if marks.is_empty() {
                String::new()
            } else {
                format!(", {}", marks.join(", "))
            };
            let scenes = if bar.scene_titles.is_empty() {
                "no scenes".to_owned()
            } else {
                bar.scene_titles.join(", ")
            };
            let count = if bar.total == 1 {
                "1 scene".to_owned()
            } else {
                format!("{} scenes", bar.total)
            };
            let label = format!("Chapter {}: {count}{suffix} ({scenes})", index + 1);
            let sense = if bar.first_scene_id.is_some() {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            };
            let (rect, response) = ui.allocate_exact_size(egui::vec2(14.0, 44.0), sense);
            name_field(ui, &response, &label);
            let response = response.on_hover_text(&label);
            let painter = ui.painter();
            painter.rect_filled(rect, 2.0, palette.hover_bg);
            let height = rect.height() * bar.total as f32 / peak as f32;
            let total = bar.total.max(1) as f32;
            let auto = height * bar.auto as f32 / total;
            let confirmed = height * bar.confirmed as f32 / total;
            let bottom = rect.bottom();
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.left(), bottom - confirmed),
                    egui::pos2(rect.right(), bottom),
                ),
                1.0,
                palette.accent,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.left(), bottom - confirmed - auto),
                    egui::pos2(rect.right(), bottom - confirmed),
                ),
                1.0,
                palette.accent.gamma_multiply(0.4),
            );
            if bar.first || bar.last {
                painter.circle_filled(
                    egui::pos2(rect.center().x, rect.top() - 4.0),
                    2.0,
                    palette.warning,
                );
            }
            if response.clicked()
                && let Some(scene) = &bar.first_scene_id
            {
                out.open = Some(Selection::Document(scene.clone()));
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("■ Confirmed")
                .size(11.0)
                .color(palette.accent),
        );
        ui.label(
            RichText::new("■ Auto")
                .size(11.0)
                .color(palette.accent.gamma_multiply(0.4)),
        );
        ui.label(
            RichText::new("● First and last")
                .size(11.0)
                .color(palette.warning),
        );
    });
}

const FIELD_TYPES: [(&str, &str); 5] = [
    ("text", "Text"),
    ("longtext", "Long text"),
    ("number", "Number"),
    ("choice", "Choice"),
    ("link", "Page link"),
];

const SUGGESTED: [(&str, &str); 10] = [
    ("Role", "text"),
    ("Age", "number"),
    ("Appearance", "longtext"),
    ("Voice", "longtext"),
    ("Goals", "longtext"),
    ("Flaws", "longtext"),
    ("Secret", "longtext"),
    ("Languages", "text"),
    ("Birthplace", "link"),
    ("Occupation", "text"),
];

/// `SheetTemplateEditor.svelte`: edits a copy of a template; saving a built-in one stores the
/// author's own copy.
fn template_editor(
    ui: &mut Ui,
    palette: &Palette,
    project: &JsValue,
    page: &Doc,
    state: &mut SheetState,
    out: &mut Out,
) {
    let custom = project_templates(project);
    let catalog = templates::all_templates(project);
    let Some(editor) = state.editor.as_mut() else {
        return;
    };
    let mut close = false;
    let mut apply = None;
    egui::Modal::new(egui::Id::unique("sheet-template-editor")).show(ui.ctx(), |ui| {
        ui.set_width(760.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                eyebrow(ui, palette, "WIKI");
                ui.label(RichText::new("Character sheet template").font(fonts::serif(22.0)));
                ui.label(muted(
                    palette,
                    format!(
                        "{} · {}",
                        editor.draft.name,
                        plural(editor.draft.fields.len(), "field", "fields")
                    ),
                ));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let x = ui.small_button("×");
                name_field(ui, &x, "Close template editor");
                close |= x.clicked();
            });
        });
        ui.separator();
        ui.columns(3, |cols| {
            template_list(&mut cols[0], palette, &catalog, &custom, editor);
            field_list(&mut cols[1], palette, editor, out);
            preview(&mut cols[2], palette, page, editor, &catalog);
        });
        ui.separator();
        let stored = catalog.iter().find(|t| t.id == editor.draft.id);
        let dirty = stored != Some(&editor.draft);
        let is_custom = custom.iter().any(|t| t.id == editor.draft.id);
        ui.horizontal(|ui| {
            ui.label(muted(
                palette,
                if editor.status.is_empty() {
                    "Existing pages keep their values when a template changes."
                } else {
                    editor.status.as_str()
                },
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(RichText::new("Apply to this page").strong())
                    .clicked()
                {
                    let template = if dirty {
                        store_draft(editor, &custom, &catalog, out)
                    } else {
                        Some(editor.draft.clone())
                    };
                    if let Some(t) = template {
                        apply = Some(t);
                        close = true;
                    }
                }
                let disabled =
                    (!dirty && is_custom) || (!is_custom && custom.len() >= MAX_TEMPLATES);
                if ui
                    .add_enabled(!disabled, egui::Button::new("Save as my template"))
                    .clicked()
                {
                    store_draft(editor, &custom, &catalog, out);
                }
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
    });
    if let Some(t) = apply {
        apply_template(out, page, &t, state);
    }
    if close {
        state.editor = None;
    }
}

/// `storeDraft`: the draft saved over the author's template, or as a new copy of a built-in.
fn store_draft(
    editor: &mut Editor,
    custom: &[Template],
    catalog: &[Template],
    out: &mut Out,
) -> Option<Template> {
    let saved = editor.draft.clone();
    if custom.iter().any(|t| t.id == saved.id) {
        let list: Vec<Template> = custom
            .iter()
            .map(|t| {
                if t.id == saved.id {
                    saved.clone()
                } else {
                    t.clone()
                }
            })
            .collect();
        out.intents.push(Intent::SetSheetTemplates(list));
        editor.status = "Template saved.".into();
        return Some(saved);
    }
    if custom.len() >= MAX_TEMPLATES {
        editor.status = format!("You can save up to {MAX_TEMPLATES} templates.");
        return None;
    }
    let copy = Template {
        id: out.new_id(),
        name: templates::unique_template_name(catalog, &saved.name),
        fields: saved.fields,
    };
    let mut list = custom.to_vec();
    list.push(copy.clone());
    out.intents.push(Intent::SetSheetTemplates(list));
    editor.status = format!("Saved as “{}”.", copy.name);
    editor.draft = copy.clone();
    Some(copy)
}

fn template_list(
    ui: &mut Ui,
    palette: &Palette,
    catalog: &[Template],
    custom: &[Template],
    editor: &mut Editor,
) {
    ui.label(muted(palette, "Templates"));
    let built_in = catalog.len() - custom.len();
    for (index, t) in catalog.iter().enumerate() {
        if !custom.is_empty() && index == built_in {
            ui.label(muted(palette, "My templates"));
        }
        let text = format!("{}  {}", t.name, plural(t.fields.len(), "field", "fields"));
        if ui.selectable_label(t.id == editor.draft.id, text).clicked() {
            editor.draft = t.clone();
            editor.status.clear();
        }
    }
    ui.label(muted(
        palette,
        "Built-in templates are fixed; saving one stores your own copy.",
    ));
}

fn field_list(ui: &mut Ui, palette: &Palette, editor: &mut Editor, out: &mut Out) {
    let id = editor.draft.id.clone();
    let mut list = vec![editor.draft.clone()];
    ui.label(muted(palette, "Template name"));
    if let Some(name) = saved_field_response(
        ui,
        ("template-name", &id),
        &editor.draft.name,
        "Untitled template",
        "Template name",
        false,
        120,
        true,
    )
    .0
    {
        templates::rename_template(&mut list, &id, &name);
    }
    let full = editor.draft.fields.len() >= MAX_TEMPLATE_FIELDS;
    ui.horizontal(|ui| {
        ui.label(muted(
            palette,
            format!(
                "Fields {} of {MAX_TEMPLATE_FIELDS}",
                editor.draft.fields.len()
            ),
        ));
        if ui
            .add_enabled(!full, egui::Button::new("+ Add field"))
            .clicked()
        {
            let new = out.new_id();
            templates::add_field(&mut list, &id, "New field", "text", None, || new);
        }
    });
    let count = editor.draft.fields.len();
    for (index, field) in editor.draft.fields.iter().enumerate() {
        ui.horizontal(|ui| {
            if let Some(label) = saved_field_response(
                ui,
                ("template-field", &field.id),
                &field.label,
                "",
                &format!("Label of field {}", index + 1),
                false,
                120,
                true,
            )
            .0
            {
                templates::rename_field(&mut list, &id, &field.id, &label);
            }
        });
        ui.horizontal(|ui| {
            let shown = FIELD_TYPES
                .iter()
                .find(|(k, _)| *k == field.kind)
                .map_or("Text", |(_, l)| *l);
            let mut kind = None;
            let combo = craft_ui::ComboBox::from_id_salt(("field-type", &field.id))
                .selected_text(shown)
                .width(100.0)
                .show_ui(ui, |ui| {
                    for (k, l) in FIELD_TYPES {
                        if ui.selectable_label(field.kind == k, l).clicked() {
                            kind = Some(k);
                        }
                    }
                });
            let name = format!("Type of {}", field.label);
            ui.ctx()
                .accesskit_node_builder(combo.response.id, move |node| node.set_label(name));
            if let Some(k) = kind {
                templates::change_field_type(&mut list, &id, &field.id, k);
            }
            for (text, name, enabled, to) in [
                (
                    "↑",
                    format!("Move {} up", field.label),
                    index > 0,
                    index as f64 - 1.0,
                ),
                (
                    "↓",
                    format!("Move {} down", field.label),
                    index + 1 < count,
                    index as f64 + 1.0,
                ),
            ] {
                let b = ui.add_enabled(enabled, egui::Button::new(text).small());
                name_field(ui, &b, &name);
                if b.clicked() {
                    templates::reorder_field(&mut list, &id, &field.id, to);
                }
            }
            let remove = ui.add(egui::Button::new("🗑").small());
            name_field(ui, &remove, &format!("Remove {}", field.label));
            if remove.clicked() {
                templates::remove_field(&mut list, &id, &field.id);
            }
        });
        if field.kind == "choice" {
            let options = field.options.clone().unwrap_or_default().join(", ");
            if let Some(text) = saved_field_response(
                ui,
                ("field-options", &field.id),
                &options,
                "Options, separated by commas",
                &format!("Options of {}", field.label),
                false,
                2000,
                true,
            )
            .0
            {
                let parts: Vec<String> = text.split(',').map(str::to_owned).collect();
                templates::set_field_options(&mut list, &id, &field.id, &parts);
            }
        }
    }
    if editor.draft.fields.is_empty() {
        ui.label(muted(
            palette,
            "This template has no fields yet. Add one or pick a suggestion below.",
        ));
    }
    ui.label(muted(palette, "Suggested fields"));
    let used: Vec<String> = editor
        .draft
        .fields
        .iter()
        .map(|f| f.label.trim().to_lowercase())
        .collect();
    ui.horizontal_wrapped(|ui| {
        for (label, kind) in SUGGESTED {
            let picked = used.contains(&label.to_lowercase());
            if ui
                .add_enabled(
                    !full && !picked,
                    egui::Button::new(format!("+{label}")).selected(picked),
                )
                .clicked()
            {
                let new = out.new_id();
                templates::add_field(&mut list, &id, label, kind, None, || new);
            }
        }
    });
    ui.label(muted(
        palette,
        "Removing a field keeps the values pages already stored.",
    ));
    if let Some(next) = list.pop() {
        editor.draft = next;
    }
}

fn preview(ui: &mut Ui, palette: &Palette, page: &Doc, editor: &Editor, catalog: &[Template]) {
    let dirty = catalog.iter().find(|t| t.id == editor.draft.id) != Some(&editor.draft);
    ui.horizontal(|ui| {
        ui.label(muted(palette, "Live preview"));
        if dirty {
            ui.label(RichText::new("unsaved").size(10.0).color(palette.warning));
        }
    });
    let meta = Meta(page.meta.as_ref());
    card(palette).show(ui, |ui| {
        let title = page
            .title
            .trim_matches(prosecraft_core::html::is_js_whitespace);
        ui.label(RichText::new(if title.is_empty() { "Untitled" } else { title }).strong());
        let name = if editor.draft.name.is_empty() {
            "Untitled template"
        } else {
            &editor.draft.name
        };
        ui.label(muted(palette, format!("{name} template")));
        for field in &editor.draft.fields {
            let value = meta.sheet_value(&field.id);
            ui.horizontal(|ui| {
                ui.label(muted(palette, &field.label));
                ui.label(if value.is_empty() {
                    "—".to_owned()
                } else {
                    value
                });
            });
        }
        if editor.draft.fields.is_empty() {
            ui.label(muted(palette, "Empty sheet."));
        }
    });
    ui.label(muted(palette, "Preview updates as you edit."));
}
