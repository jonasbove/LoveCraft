//! The Network view (ProseCraft `NetworkView.svelte`, `NetworkCards.svelte`,
//! `NetworkMatrix.svelte`, `NetworkSuggestions.svelte`, `NodeSummaryPanel.svelte` and
//! `RelationDetailPanel.svelte`, upstream `bcb884d`): the Wiki's pages and the relations between
//! them as cards, a network or a matrix, filtered by kind, type and chapter, with relation
//! suggestions from scenes pages share, and each page's character card.

use std::collections::HashMap;

use egui::{RichText, Stroke, Ui};
use prosecraft_core::js::JsValue;
use prosecraft_core::network::graph::{self, Point, Status};
use prosecraft_core::network::layout::{LayoutEdge, LayoutNode, Options, layout};
use prosecraft_core::network::meta::{RELATION_TYPES, Relation};
use prosecraft_core::network::relations::PagePair;
use prosecraft_core::network::relations::{
    self, Appearances, Doc, Sourced, Story, relation_active_at, relation_boundary_indexes,
};

use crate::editing::Intent;
use crate::relation_graph::{
    self, GraphState, kind_color, kind_name, plural, relation_color, relation_name,
};
use crate::theme::Palette;
use crate::views::name_field;
use crate::widgets::{card, eyebrow, segmented};
use crate::{Selection, fonts};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    Cards,
    #[default]
    Network,
    Matrix,
}

/// The suggestion form opened under one pair (`NetworkSuggestions.svelte`).
#[derive(Clone, Debug, Default)]
struct SuggestionForm {
    key: String,
    kind: &'static str,
    label: String,
    a_to_b: bool,
}

/// The view's state, kept while the app runs (ProseCraft keeps it while the view is mounted).
#[derive(Clone, Debug)]
pub struct NetworkState {
    pub mode: Mode,
    kind_filter: Option<&'static str>,
    type_filter: Option<&'static str>,
    all_chapters: bool,
    chapter_index: usize,
    selected: Option<String>,
    selected_edge: Option<String>,
    /// The page whose character card is open.
    pub card: Option<String>,
    adding: bool,
    add_source: String,
    add_target: String,
    add_type: &'static str,
    suggestions_open: bool,
    form: Option<SuggestionForm>,
    positions: HashMap<String, Point>,
    seed: f64,
    layout_key: String,
    graph: GraphState,
    pub sheet: crate::character_sheet::SheetState,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self {
            mode: Mode::Network,
            kind_filter: None,
            type_filter: None,
            all_chapters: true,
            chapter_index: 0,
            selected: None,
            selected_edge: None,
            card: None,
            adding: false,
            add_source: String::new(),
            add_target: String::new(),
            add_type: "other",
            suggestions_open: false,
            form: None,
            positions: HashMap::new(),
            seed: 1.0,
            layout_key: String::new(),
            graph: GraphState::default(),
            sheet: crate::character_sheet::SheetState::default(),
        }
    }
}

impl NetworkState {
    /// "Open character card" from the Wiki: the Cards view with the page's card open.
    pub fn open_card(&mut self, id: &str) {
        self.mode = Mode::Cards;
        self.card = Some(id.to_owned());
    }
}

/// What the view reads from the project, worked out once per change.
pub(crate) struct Derived {
    pub story: Story,
    pub appearances: Vec<Appearances>,
    pub relations: Vec<Sourced>,
    pub suggestions: Vec<PagePair>,
}

impl Derived {
    pub fn new(project: &JsValue) -> Self {
        let story = Story::from_project(project);
        let appearances = relations::appearances(&story);
        let relations = relations::relations_of(&story.docs);
        let dismissed = prosecraft_core::network::edit::dismissed(project);
        let suggestions = relations::suggest_relations(&appearances, &relations, &dismissed, 3);
        Self {
            story,
            appearances,
            relations,
            suggestions,
        }
    }

    pub fn appearance(&self, id: &str) -> Option<&Appearances> {
        self.appearances.iter().find(|a| a.page_id == id)
    }

    pub fn title(&self, id: &str) -> Option<&str> {
        self.story
            .pages()
            .find(|d| d.id == id)
            .map(|d| d.title.as_str())
    }

    fn pages(&self) -> Vec<graph::Page> {
        self.story
            .pages()
            .map(|d| graph::Page {
                id: d.id.clone(),
                title: d.title.clone(),
                kind: self.story.page_kind(d),
            })
            .collect()
    }

    fn scene_chapters(&self) -> HashMap<String, String> {
        self.story
            .docs
            .iter()
            .filter(|d| !d.trashed && !d.is_wiki())
            .filter_map(|d| Some((d.id.clone(), d.chapter_id.clone()?)))
            .collect()
    }
}

/// What the panes of the view hand back.
pub(crate) struct Out<'a> {
    pub editable: bool,
    pub intents: &'a mut Vec<Intent>,
    /// Fresh ids for new relations and template fields.
    pub ids: &'a mut Vec<String>,
    pub open: Option<Selection>,
}

impl Out<'_> {
    pub fn new_id(&mut self) -> String {
        self.ids
            .pop()
            .unwrap_or_else(|| format!("id-{}", self.intents.len()))
    }
}

const KIND_FILTERS: [(&str, &str); 5] = [
    ("character", "Characters"),
    ("place", "Places"),
    ("organisation", "Organisations"),
    ("item", "Items"),
    ("other", "Other"),
];

fn muted(palette: &Palette, text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .size(12.0)
        .color(palette.text_muted)
}

fn graph_model(state: &NetworkState, derived: &Derived) -> graph::Model {
    let pages = derived.pages();
    let scene_chapters = derived.scene_chapters();
    let last = derived.story.chapters.len().saturating_sub(1);
    let chapter = (!state.all_chapters)
        .then(|| derived.story.chapters.get(state.chapter_index.min(last)))
        .flatten();
    let kinds = state.kind_filter.map(|k| [k]);
    let mut model = graph::build(&graph::Input {
        pages: &pages,
        relations: &derived.relations,
        appearances: &derived.appearances,
        suggestions: &derived.suggestions,
        chapters: &derived.story.chapters,
        chapter_id: chapter.map(String::as_str),
        scene_chapters: Some(&scene_chapters),
        kinds: kinds.as_ref().map(|k| &k[..]),
    });
    if let Some(kind) = state.type_filter {
        model
            .edges
            .retain(|e| e.status == Status::Declared && e.kind == kind);
    }
    model
}

/// `$effect` on the structure: positions for new pages, the old ones kept.
fn relayout_if_changed(state: &mut NetworkState, model: &graph::Model) {
    let key = format!(
        "{}|{}",
        model
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>()
            .join("\u{0}"),
        model
            .edges
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>()
            .join("\u{0}")
    );
    if key == state.layout_key {
        return;
    }
    state.layout_key = key;
    let nodes: Vec<LayoutNode> = model
        .nodes
        .iter()
        .map(|n| LayoutNode {
            id: &n.id,
            at: None,
            pinned: false,
        })
        .collect();
    let edges: Vec<LayoutEdge> = model
        .edges
        .iter()
        .map(|e| LayoutEdge {
            from: &e.from,
            to: &e.to,
            weight: None,
        })
        .collect();
    let placed = layout(
        &nodes,
        &edges,
        &Options {
            previous: state.positions.clone(),
            ..Options::default()
        },
    );
    state.positions.extend(placed);
}

fn header(
    ui: &mut Ui,
    palette: &Palette,
    state: &mut NetworkState,
    derived: &Derived,
    summary: &str,
    out: &mut Out,
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            eyebrow(ui, palette, "WIKI · NETWORK");
            ui.label(
                RichText::new("Network")
                    .font(fonts::serif(32.0))
                    .color(palette.text_heading),
            );
            ui.label(muted(palette, summary));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::BOTTOM), |ui| {
            let beside = ui.add_enabled(
                state.selected.is_some(),
                egui::Button::new("Open graph beside"),
            );
            if beside.clicked()
                && let Some(id) = &state.selected
            {
                out.intents
                    .push(Intent::OpenBeside(prosecraft_core::split::graph_target(
                        Some(id),
                    )));
            }
            let pages = derived.story.pages().count();
            let add = ui.add_enabled(
                out.editable && pages >= 2,
                egui::Button::new("+ Add relation"),
            );
            if add.clicked() {
                state.selected_edge = None;
                state.adding = true;
            }
            let toggle = ui.selectable_label(
                state.suggestions_open,
                format!("Suggestions {}", derived.suggestions.len()),
            );
            name_field(ui, &toggle, "Suggestions");
            if toggle.clicked() {
                state.suggestions_open = !state.suggestions_open;
            }
            legend(ui, palette);
        });
    });
}

fn legend(ui: &mut Ui, palette: &Palette) {
    for (text, dashed) in [("Suggested", true), ("Declared", false)] {
        ui.label(muted(palette, text));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 10.0), egui::Sense::hover());
        let y = rect.center().y;
        let stroke = Stroke::new(
            1.6,
            if dashed {
                palette.text_faint
            } else {
                palette.accent
            },
        );
        if dashed {
            for x in [rect.left(), rect.left() + 8.0, rect.left() + 16.0] {
                ui.painter().line_segment(
                    [egui::pos2(x, y), egui::pos2((x + 4.0).min(rect.right()), y)],
                    stroke,
                );
            }
        } else {
            ui.painter().line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                stroke,
            );
        }
    }
}

fn filter_combo(
    ui: &mut Ui,
    id: &str,
    name: &str,
    value: &mut Option<&'static str>,
    options: &[(&'static str, &'static str)],
) {
    ui.label(RichText::new(name).size(12.0));
    let shown = value
        .and_then(|v| options.iter().find(|(k, _)| *k == v))
        .map_or("All", |(_, l)| *l);
    let combo = craft_ui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "All");
            for (key, label) in options {
                ui.selectable_value(value, Some(*key), *label);
            }
        });
    let name = name.to_owned();
    ui.ctx()
        .accesskit_node_builder(combo.response.id, move |node| node.set_label(name));
}

fn toolbar(ui: &mut Ui, palette: &Palette, state: &mut NetworkState, model: &graph::Model) {
    ui.horizontal_wrapped(|ui| {
        let modes = [Mode::Cards, Mode::Network, Mode::Matrix];
        let current = modes.iter().position(|m| *m == state.mode).unwrap_or(1);
        if let Some(i) = segmented(
            ui,
            palette,
            "network-view",
            &["Cards", "Network", "Matrix"],
            current,
            12.0,
        ) {
            state.mode = modes[i];
        }
        ui.add_space(12.0);
        filter_combo(
            ui,
            "network-kind",
            "Kind",
            &mut state.kind_filter,
            &KIND_FILTERS,
        );
        let types: Vec<(&'static str, &'static str)> = RELATION_TYPES
            .iter()
            .map(|t| (*t, relation_name(t)))
            .collect();
        filter_combo(ui, "network-type", "Type", &mut state.type_filter, &types);
        let relayout = ui.add_enabled(!model.nodes.is_empty(), egui::Button::new("↺ Re-layout"));
        if relayout.clicked() {
            state.seed += 1.0;
            let nodes: Vec<LayoutNode> = model
                .nodes
                .iter()
                .map(|n| LayoutNode {
                    id: &n.id,
                    at: None,
                    pinned: false,
                })
                .collect();
            let edges: Vec<LayoutEdge> = model
                .edges
                .iter()
                .map(|e| LayoutEdge {
                    from: &e.from,
                    to: &e.to,
                    weight: None,
                })
                .collect();
            state.positions = layout(
                &nodes,
                &edges,
                &Options {
                    seed: Some(state.seed),
                    ..Options::default()
                },
            );
        }
    });
}

/// `NetworkCards.svelte`: one card per character page.
fn cards(ui: &mut Ui, palette: &Palette, state: &mut NetworkState, derived: &Derived) {
    let characters = derived.story.character_pages();
    if characters.is_empty() {
        ui.label(muted(
            palette,
            "No characters yet. Add a wiki page to the Characters folder first.",
        ));
        return;
    }
    let mut chips: HashMap<&str, Vec<(String, &str, bool)>> = HashMap::new();
    for s in &derived.relations {
        chips
            .entry(&s.source)
            .or_default()
            .push((s.relation.to.clone(), &s.relation.kind, true));
        chips
            .entry(&s.relation.to)
            .or_default()
            .push((s.source.clone(), &s.relation.kind, false));
    }
    ui.horizontal_wrapped(|ui| {
        for page in characters {
            let role = prosecraft_core::network::meta::Meta(page.meta.as_ref()).sheet_value("role");
            let scenes = derived
                .appearance(&page.id)
                .map_or(0, Appearances::scene_count);
            let response = card(palette)
                .show(ui, |ui| {
                    ui.set_width(220.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(card_initials(&page.title))
                                .strong()
                                .color(palette.accent),
                        );
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&page.title).strong());
                            if !role.is_empty() {
                                ui.label(muted(palette, &role));
                            }
                        });
                    });
                    ui.label(muted(palette, plural(scenes, "scene", "scenes")));
                    let list = chips
                        .get(page.id.as_str())
                        .map(Vec::as_slice)
                        .unwrap_or_default();
                    if !list.is_empty() {
                        ui.separator();
                    }
                    for (other, kind, out) in list.iter().take(2) {
                        ui.horizontal(|ui| {
                            ui.label(if *out { "→" } else { "←" });
                            ui.label(derived.title(other).unwrap_or("Unknown page"));
                            ui.label(
                                RichText::new(relation_name(kind))
                                    .color(relation_color(palette, kind)),
                            );
                        });
                    }
                })
                .response
                .interact(egui::Sense::click());
            name_field(ui, &response, &page.title);
            if response.clicked() {
                state.card = Some(page.id.clone());
            }
        }
    });
}

/// `initials` of the cards and the matrix: the first letters of the first two words.
fn card_initials(title: &str) -> String {
    let words: Vec<&str> = title
        .split(prosecraft_core::html::is_js_whitespace)
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return "?".into();
    }
    words
        .iter()
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}

/// `NetworkMatrix.svelte`: sources down, targets across, a dot per relation.
fn matrix(ui: &mut Ui, palette: &Palette, state: &NetworkState, derived: &Derived, out: &mut Out) {
    let collator = |a: &&Doc, b: &&Doc| {
        prosecraft_core::import::markdown_folder::natural_compare(&a.title, &b.title)
    };
    let mut pages: Vec<&Doc> = derived
        .story
        .pages()
        .filter(|p| {
            state
                .kind_filter
                .is_none_or(|k| derived.story.page_kind(p) == k)
        })
        .collect();
    pages.sort_by(collator);
    if pages.is_empty() {
        ui.label(muted(palette, "No pages to compare yet."));
        return;
    }
    let chapter = (!state.all_chapters)
        .then(|| derived.story.chapters.get(state.chapter_index))
        .flatten();
    let active: Vec<&Sourced> = derived
        .relations
        .iter()
        .filter(|s| {
            chapter.is_none_or(|c| relation_active_at(&s.relation, c, &derived.story.chapters))
        })
        .collect();
    egui::ScrollArea::both().max_height(420.0).show(ui, |ui| {
        egui::Grid::new("network-matrix")
            .spacing([2.0, 2.0])
            .show(ui, |ui| {
                ui.label(muted(palette, "Page"));
                for p in &pages {
                    ui.label(RichText::new(card_initials(&p.title)).size(11.0))
                        .on_hover_text(&p.title);
                }
                ui.end_row();
                for row in &pages {
                    let open = ui
                        .add(egui::Button::new(&row.title).frame(false))
                        .on_hover_text(&row.title);
                    if open.clicked() {
                        out.open = Some(Selection::Document(row.id.clone()));
                    }
                    for column in &pages {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(24.0, 20.0), egui::Sense::hover());
                        if row.id == column.id {
                            ui.painter().rect_filled(rect, 2.0, palette.hover_bg);
                        }
                        let dots: Vec<&&Sourced> = active
                            .iter()
                            .filter(|s| s.source == row.id && s.relation.to == column.id)
                            .collect();
                        for (i, s) in dots.iter().enumerate() {
                            let c = rect.center()
                                + egui::vec2(i as f32 * 6.0 - (dots.len() as f32 - 1.0) * 3.0, 0.0);
                            ui.painter().circle_filled(
                                c,
                                3.5,
                                relation_color(palette, &s.relation.kind),
                            );
                            let tip = format!(
                                "{} → {}: {}",
                                row.title,
                                column.title,
                                s.relation
                                    .label
                                    .clone()
                                    .unwrap_or_else(|| relation_name(&s.relation.kind).to_owned())
                            );
                            let r = ui.interact(
                                egui::Rect::from_center_size(c, egui::vec2(7.0, 7.0)),
                                ui.scope_id().with(("dot", &s.relation.id)),
                                egui::Sense::hover(),
                            );
                            r.on_hover_text(tip);
                        }
                    }
                    ui.end_row();
                }
            });
    });
    ui.label(muted(
        palette,
        "Rows are the relation source, columns the target. Hover a dot for its label.",
    ));
    let mut legend: Vec<&str> = derived
        .relations
        .iter()
        .map(|s| s.relation.kind.as_str())
        .collect();
    legend.sort_by_key(|k| relation_name(k));
    legend.dedup();
    ui.horizontal_wrapped(|ui| {
        for kind in legend {
            ui.label(RichText::new("●").color(relation_color(palette, kind)));
            ui.label(relation_name(kind));
        }
    });
}

/// `NetworkSuggestions.svelte`.
fn suggestions(
    ui: &mut Ui,
    palette: &Palette,
    state: &mut NetworkState,
    derived: &Derived,
    out: &mut Out,
) {
    card(palette).show(ui, |ui| {
        ui.set_width(300.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Suggestions").strong());
            ui.label(muted(palette, derived.suggestions.len().to_string()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close = ui.small_button("×");
                name_field(ui, &close, "Close suggestions");
                if close.clicked() {
                    state.suggestions_open = false;
                }
            });
        });
        ui.label(muted(
            palette,
            "Pages that keep appearing together without a declared relation.",
        ));
        if derived.suggestions.is_empty() {
            ui.label(muted(palette, "No suggestions right now."));
            return;
        }
        let title = |id: &str| derived.title(id).unwrap_or("Unknown page").to_owned();
        for pair in &derived.suggestions {
            ui.separator();
            let both = format!("{} and {}", title(&pair.a), title(&pair.b));
            ui.label(RichText::new(&both).strong());
            ui.label(muted(
                palette,
                format!(
                    "Appear together in {} · no relation yet",
                    plural(pair.count, "scene", "scenes")
                ),
            ));
            let open = state.form.as_ref().is_some_and(|f| f.key == pair.key);
            ui.horizontal(|ui| {
                let accept = ui.add_enabled(out.editable, egui::Button::new("Accept"));
                if accept.clicked() {
                    state.form = if open {
                        None
                    } else {
                        Some(SuggestionForm {
                            key: pair.key.clone(),
                            kind: "other",
                            label: String::new(),
                            a_to_b: true,
                        })
                    };
                }
                if ui
                    .add_enabled(out.editable, egui::Button::new("Dismiss"))
                    .clicked()
                {
                    out.intents
                        .push(Intent::DismissSuggestion(pair.key.clone()));
                }
            });
            if open {
                suggestion_form(ui, state, pair, &title, out);
            }
        }
    });
}

fn type_chips(ui: &mut Ui, current: &mut &'static str) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for kind in RELATION_TYPES {
            if ui
                .selectable_label(*current == kind, relation_name(kind))
                .clicked()
            {
                *current = kind;
                changed = true;
            }
        }
    });
    changed
}

fn suggestion_form(
    ui: &mut Ui,
    state: &mut NetworkState,
    pair: &PagePair,
    title: &dyn Fn(&str) -> String,
    out: &mut Out,
) {
    let Some(form) = state.form.as_mut() else {
        return;
    };
    type_chips(ui, &mut form.kind);
    ui.horizontal(|ui| {
        ui.label("Label");
        let label = ui.add(
            egui::TextEdit::singleline(&mut form.label)
                .hint_text("Optional, e.g. Estranged brother"),
        );
        name_field(ui, &label, "Label");
    });
    ui.horizontal_wrapped(|ui| {
        if ui
            .selectable_label(
                form.a_to_b,
                format!("{} to {}", title(&pair.a), title(&pair.b)),
            )
            .clicked()
        {
            form.a_to_b = true;
        }
        if ui
            .selectable_label(
                !form.a_to_b,
                format!("{} to {}", title(&pair.b), title(&pair.a)),
            )
            .clicked()
        {
            form.a_to_b = false;
        }
    });
    let mut done = false;
    ui.horizontal(|ui| {
        if ui.button("Create relation").clicked() {
            let (source, mut relation) =
                graph::relation_from_suggestion(pair, form.kind, &form.label, form.a_to_b);
            relation.id = out.new_id();
            state.selected = None;
            state.adding = false;
            state.selected_edge = Some(format!("declared:{source}:{}", relation.id));
            out.intents.push(Intent::AddRelation { source, relation });
            state.suggestions_open = false;
            done = true;
        }
        if ui.button("Cancel").clicked() {
            done = true;
        }
    });
    if done {
        state.form = None;
    }
}

/// `NodeSummaryPanel.svelte`.
fn node_summary(
    ui: &mut Ui,
    palette: &Palette,
    state: &mut NetworkState,
    derived: &Derived,
    page: &Doc,
    out: &mut Out,
) {
    let kind = derived.story.page_kind(page);
    card(palette).show(ui, |ui| {
        ui.set_width(280.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                eyebrow(ui, palette, "Selected page");
                ui.label(RichText::new(&page.title).font(fonts::serif(20.0)));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(kind_name(kind)).color(kind_color(palette, kind)));
                    ui.label(muted(palette, "·"));
                    let scenes = derived
                        .appearance(&page.id)
                        .map_or(0, Appearances::scene_count);
                    ui.label(muted(palette, plural(scenes, "scene", "scenes")));
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let close = ui.small_button("×");
                name_field(ui, &close, "Close");
                if close.clicked() {
                    state.selected = None;
                    state.adding = false;
                }
            });
        });
        if !page.aliases.is_empty() {
            ui.label(muted(palette, "Also known as"));
            ui.horizontal_wrapped(|ui| {
                for alias in &page.aliases {
                    ui.label(RichText::new(alias).background_color(palette.hover_bg));
                }
            });
        }
        let rows = relation_rows(derived, &page.id);
        ui.label(muted(palette, format!("Relations · {}", rows.len())));
        if rows.is_empty() {
            ui.label(muted(palette, "No relations yet."));
        }
        for (relation, other, outgoing) in &rows {
            ui.horizontal(|ui| {
                ui.label(RichText::new("●").color(relation_color(palette, &relation.kind)));
                ui.label(other);
                let label = relation
                    .label
                    .clone()
                    .unwrap_or_else(|| relation_name(&relation.kind).to_owned());
                ui.label(
                    RichText::new(format!("{} {label}", if *outgoing { "→" } else { "←" }))
                        .color(relation_color(palette, &relation.kind)),
                );
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button("Open page").clicked() {
                out.open = Some(Selection::Document(page.id.clone()));
            }
            if ui.button("Open character card").clicked() {
                state.card = Some(page.id.clone());
            }
            if ui
                .add_enabled(out.editable, egui::Button::new("+ Add relation from here"))
                .clicked()
            {
                state.selected_edge = None;
                state.adding = true;
            }
        });
    });
}

/// `selectedRelationList`: a page's relations both ways, with the other page's title.
fn relation_rows(derived: &Derived, id: &str) -> Vec<(Relation, String, bool)> {
    let title = |other: &str| derived.title(other).unwrap_or("Untitled page").to_owned();
    derived
        .relations
        .iter()
        .filter_map(|s| {
            if s.source == id {
                Some((s.relation.clone(), title(&s.relation.to), true))
            } else if s.relation.to == id {
                Some((s.relation.clone(), title(&s.source), false))
            } else {
                None
            }
        })
        .collect()
}

fn page_combo(
    ui: &mut Ui,
    id: &str,
    name: &str,
    value: &mut String,
    pages: &[&Doc],
    placeholder: Option<&str>,
) -> bool {
    let shown = pages
        .iter()
        .find(|p| p.id == *value)
        .map_or_else(|| placeholder.unwrap_or("").to_owned(), |p| p.title.clone());
    let before = value.clone();
    let combo = craft_ui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            if let Some(text) = placeholder {
                ui.selectable_value(value, String::new(), text);
            }
            for p in pages {
                ui.selectable_value(value, p.id.clone(), &p.title);
            }
        });
    let name = name.to_owned();
    ui.ctx()
        .accesskit_node_builder(combo.response.id, move |node| node.set_label(name));
    *value != before
}

/// `RelationDetailPanel.svelte`, adding.
fn add_panel(
    ui: &mut Ui,
    palette: &Palette,
    state: &mut NetworkState,
    derived: &Derived,
    out: &mut Out,
) {
    let mut sorted: Vec<&Doc> = derived.story.pages().collect();
    sorted.sort_by(|a, b| {
        prosecraft_core::import::markdown_folder::natural_compare(&a.title, &b.title)
    });
    card(palette).show(ui, |ui| {
        ui.set_width(280.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                eyebrow(ui, palette, "New relation");
                ui.label(RichText::new("Add relation").font(fonts::serif(20.0)));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let close = ui.small_button("×");
                name_field(ui, &close, "Close");
                if close.clicked() {
                    state.adding = false;
                }
            });
        });
        if sorted.len() < 2 {
            ui.label(muted(
                palette,
                "Create at least two wiki pages to link them.",
            ));
            return;
        }
        if state.add_source.is_empty() || !sorted.iter().any(|p| p.id == state.add_source) {
            state.add_source = state
                .selected
                .clone()
                .filter(|s| sorted.iter().any(|p| p.id == *s))
                .unwrap_or_else(|| sorted[0].id.clone());
        }
        ui.label(muted(palette, "From"));
        if page_combo(
            ui,
            "relation-from",
            "From",
            &mut state.add_source,
            &sorted,
            None,
        ) && state.add_target == state.add_source
        {
            state.add_target.clear();
        }
        ui.label(muted(palette, "To"));
        let targets: Vec<&Doc> = sorted
            .iter()
            .copied()
            .filter(|p| p.id != state.add_source)
            .collect();
        page_combo(
            ui,
            "relation-to",
            "To",
            &mut state.add_target,
            &targets,
            Some("Choose a page"),
        );
        ui.label(muted(palette, "Type"));
        type_chips(ui, &mut state.add_type);
        ui.horizontal(|ui| {
            let can = out.editable
                && !state.add_target.is_empty()
                && state.add_target != state.add_source;
            if ui
                .add_enabled(can, egui::Button::new("+ Add relation"))
                .clicked()
            {
                let id = out.new_id();
                let source = state.add_source.clone();
                out.intents.push(Intent::AddRelation {
                    source: source.clone(),
                    relation: Relation {
                        id: id.clone(),
                        to: state.add_target.clone(),
                        kind: state.add_type.to_owned(),
                        label: None,
                        tension: None,
                        from: None,
                        until: None,
                        note: None,
                    },
                });
                state.adding = false;
                state.selected = None;
                state.selected_edge = Some(format!("declared:{source}:{id}"));
                state.add_target.clear();
            }
            if ui.button("Cancel").clicked() {
                state.adding = false;
            }
        });
    });
}

fn chapter_choice(
    ui: &mut Ui,
    id: &str,
    name: &str,
    value: Option<&str>,
    chapters: &[String],
    blank: &str,
) -> Option<Option<String>> {
    let shown = value
        .and_then(|v| chapters.iter().position(|c| c == v))
        .map_or_else(|| blank.to_owned(), |i| format!("Ch {}", i + 1));
    let mut chosen = None;
    let combo = craft_ui::ComboBox::from_id_salt(id)
        .selected_text(shown)
        .width(90.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(value.is_none(), blank).clicked() {
                chosen = Some(None);
            }
            for (i, c) in chapters.iter().enumerate() {
                if ui
                    .selectable_label(value == Some(c.as_str()), format!("Ch {}", i + 1))
                    .clicked()
                {
                    chosen = Some(Some(c.clone()));
                }
            }
        });
    let name = name.to_owned();
    ui.ctx()
        .accesskit_node_builder(combo.response.id, move |node| node.set_label(name));
    chosen
}

/// `validityHint`.
fn validity(relation: &Relation, chapters: &[String]) -> String {
    let index = |id: &Option<String>| {
        id.as_ref()
            .and_then(|i| chapters.iter().position(|c| c == i))
    };
    match (index(&relation.from), index(&relation.until)) {
        (None, None) => "Active in every chapter".into(),
        (None, Some(u)) => format!("Until Ch {}, inclusive", u + 1),
        (Some(f), None) => format!("From Ch {}, open-ended", f + 1),
        (Some(f), Some(u)) if f > u => "The end comes before the start".into(),
        (Some(f), Some(u)) => format!("Ch {} to Ch {}", f + 1, u + 1),
    }
}

fn patch(out: &mut Out, source: &str, relation: &str, key: &str, value: Option<JsValue>) {
    out.intents.push(Intent::UpdateRelation {
        source: source.to_owned(),
        relation: relation.to_owned(),
        patch: vec![(key.to_owned(), value)],
    });
}

/// `RelationDetailPanel.svelte`, a selected relation.
fn relation_panel(
    ui: &mut Ui,
    palette: &Palette,
    state: &mut NetworkState,
    derived: &Derived,
    selection: &Sourced,
    out: &mut Out,
) {
    let r = &selection.relation;
    let source_title = derived
        .title(&selection.source)
        .unwrap_or("Unknown page")
        .to_owned();
    let target_title = derived.title(&r.to).unwrap_or("Unknown page").to_owned();
    card(palette).show(ui, |ui| {
        ui.set_width(280.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                eyebrow(ui, palette, "Selected relation");
                ui.label(RichText::new(&source_title).font(fonts::serif(18.0)));
                ui.label(RichText::new(format!("→ {target_title}")).font(fonts::serif(18.0)));
                ui.label(muted(
                    palette,
                    format!("Directed · type {}", relation_name(&r.kind).to_lowercase()),
                ));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let close = ui.small_button("×");
                name_field(ui, &close, "Close");
                if close.clicked() {
                    state.selected_edge = None;
                }
            });
        });
        ui.label(RichText::new("Declared").size(11.0).color(palette.accent));
        ui.add_enabled_ui(out.editable, |ui| {
            ui.label(muted(palette, "Type"));
            let mut kind = RELATION_TYPES
                .iter()
                .copied()
                .find(|k| *k == r.kind)
                .unwrap_or("other");
            if type_chips(ui, &mut kind) {
                patch(
                    out,
                    &selection.source,
                    &r.id,
                    "type",
                    Some(JsValue::str(kind)),
                );
            }
            ui.label(muted(palette, "Label"));
            if let Some(text) = crate::views::saved_field_response(
                ui,
                ("relation-label", &r.id),
                r.label.as_deref().unwrap_or(""),
                "e.g. Estranged brother",
                "Label",
                false,
                120,
                out.editable,
            )
            .0
            {
                patch(
                    out,
                    &selection.source,
                    &r.id,
                    "label",
                    (!text.is_empty()).then(|| JsValue::str(&text)),
                );
            }
            tension(ui, palette, selection, out);
            ui.label(muted(palette, "Valid from / until"));
            ui.horizontal(|ui| {
                if let Some(v) = chapter_choice(
                    ui,
                    &format!("from-{}", r.id),
                    "Valid from chapter",
                    r.from.as_deref(),
                    &derived.story.chapters,
                    "Start",
                ) {
                    patch(
                        out,
                        &selection.source,
                        &r.id,
                        "from",
                        v.map(|c| JsValue::str(&c)),
                    );
                }
                ui.label("–");
                if let Some(v) = chapter_choice(
                    ui,
                    &format!("until-{}", r.id),
                    "Valid until chapter",
                    r.until.as_deref(),
                    &derived.story.chapters,
                    "Open",
                ) {
                    patch(
                        out,
                        &selection.source,
                        &r.id,
                        "until",
                        v.map(|c| JsValue::str(&c)),
                    );
                }
            });
            ui.label(muted(palette, validity(r, &derived.story.chapters)));
            ui.label(muted(palette, "Note"));
            if let Some(text) = crate::views::saved_field_response(
                ui,
                ("relation-note", &r.id),
                r.note.as_deref().unwrap_or(""),
                "What changes between them?",
                "Note",
                true,
                2000,
                out.editable,
            )
            .0
            {
                patch(
                    out,
                    &selection.source,
                    &r.id,
                    "note",
                    (!text.is_empty()).then(|| JsValue::str(&text)),
                );
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Open both pages").clicked() {
                out.intents
                    .push(Intent::OpenBeside(prosecraft_core::split::doc_target(
                        &r.to,
                    )));
                out.open = Some(Selection::Document(selection.source.clone()));
            }
            let delete = ui.add_enabled(
                out.editable,
                egui::Button::new(RichText::new("Delete relation").color(palette.danger)),
            );
            if delete.clicked() {
                ui.data_mut(|d| {
                    d.insert_temp(egui::Id::unique("network-confirm-delete"), r.id.clone())
                });
            }
        });
        ui.label(muted(palette, "Changes save immediately"));
        confirm_delete(
            ui,
            palette,
            state,
            selection,
            &source_title,
            &target_title,
            out,
        );
    });
}

/// `confirmDestructiveAction` before deleting a relation.
fn confirm_delete(
    ui: &mut Ui,
    palette: &Palette,
    state: &mut NetworkState,
    selection: &Sourced,
    source: &str,
    target: &str,
    out: &mut Out,
) {
    let key = egui::Id::unique("network-confirm-delete");
    let asking = ui
        .data(|d| d.get_temp::<String>(key))
        .is_some_and(|id| id == selection.relation.id);
    if !asking {
        return;
    }
    let mut answer = None;
    egui::Modal::new(egui::Id::unique("network-delete-modal")).show(ui.ctx(), |ui| {
        ui.label(format!(
            "Delete the relation between “{source}” and “{target}”?"
        ));
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("Delete").color(palette.danger))
                .clicked()
            {
                answer = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    if let Some(yes) = answer {
        ui.data_mut(|d| d.remove_temp::<String>(key));
        if yes {
            out.intents.push(Intent::RemoveRelation {
                source: selection.source.clone(),
                relation: selection.relation.id.clone(),
            });
            state.selected_edge = None;
        }
    }
}

fn tension(ui: &mut Ui, palette: &Palette, selection: &Sourced, out: &mut Out) {
    let r = &selection.relation;
    let level = r.tension.unwrap_or(0.0);
    ui.horizontal(|ui| {
        ui.label(muted(palette, "Tension"));
        ui.label(if level > 0.0 {
            format!("{level} / 5")
        } else {
            "Not set".into()
        });
        if level > 0.0 && ui.small_button("Clear").clicked() {
            patch(out, &selection.source, &r.id, "tension", None);
        }
    });
    ui.horizontal(|ui| {
        for n in 1..=5u8 {
            let on = level >= f64::from(n);
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(28.0, 10.0), egui::Sense::click());
            ui.painter().rect_filled(
                rect,
                3.0,
                if on { palette.accent } else { palette.hover_bg },
            );
            name_field(ui, &response, &format!("Tension {n}"));
            if response.clicked() {
                patch(
                    out,
                    &selection.source,
                    &r.id,
                    "tension",
                    Some(JsValue::Number(f64::from(n))),
                );
            }
        }
    });
}

fn chapter_footer(
    ui: &mut Ui,
    palette: &Palette,
    state: &mut NetworkState,
    derived: &Derived,
    declared: usize,
) {
    let chapters = derived.story.chapters.len();
    let last = chapters.saturating_sub(1);
    let label = if chapters == 0 {
        "No chapters".to_owned()
    } else {
        format!("Ch {} of {chapters}", state.chapter_index.min(last) + 1)
    };
    ui.horizontal(|ui| {
        eyebrow(ui, palette, "Chapter");
        if ui
            .selectable_label(state.all_chapters, "All chapters")
            .clicked()
        {
            state.all_chapters = !state.all_chapters;
        }
        ui.label(if state.all_chapters {
            "All".to_owned()
        } else {
            label
        });
        ui.label(muted(palette, format!("{declared} declared")));
    });
    let mut index = state.chapter_index.min(last);
    let slider = ui.add_enabled(
        !state.all_chapters && chapters > 0,
        egui::Slider::new(&mut index, 0..=last).show_value(false),
    );
    name_field(ui, &slider, "Chapter");
    state.chapter_index = index;
    // Marks where the set of active relations changes.
    let rect = slider.rect;
    for boundary in relation_boundary_indexes(&derived.relations, &derived.story.chapters) {
        if boundary > 0 && boundary <= last {
            let x = rect.left() + rect.width() * boundary as f32 / last.max(1) as f32;
            ui.painter().line_segment(
                [
                    egui::pos2(x, rect.bottom()),
                    egui::pos2(x, rect.bottom() + 4.0),
                ],
                Stroke::new(2.0, palette.accent),
            );
        }
    }
    match state.mode {
        Mode::Cards => {
            ui.label(muted(palette, "Click a card to open its character card."));
        }
        Mode::Network => {
            ui.label(muted(
                palette,
                "Drag to rearrange · scroll to zoom · double-click a page to open it",
            ));
        }
        Mode::Matrix => {}
    }
}

fn selected_relation<'a>(
    state: &NetworkState,
    derived: &'a Derived,
    model: &graph::Model,
) -> Option<&'a Sourced> {
    let edge = state.selected_edge.as_ref()?;
    if !model.edges.iter().any(|e| &e.id == edge) {
        return None;
    }
    derived
        .relations
        .iter()
        .find(|s| format!("declared:{}:{}", s.source, s.relation.id) == *edge)
}

fn network(
    ui: &mut Ui,
    palette: &Palette,
    state: &mut NetworkState,
    derived: &Derived,
    model: &graph::Model,
    out: &mut Out,
) {
    let relation = selected_relation(state, derived, model).cloned();
    let page = state
        .selected
        .as_ref()
        .and_then(|id| derived.story.pages().find(|p| p.id == *id))
        .cloned();
    let panel = state.adding || relation.is_some() || page.is_some();
    let height = (ui.available_height() - 140.0).max(360.0);
    let mut graph_rect = egui::Rect::NOTHING;
    ui.horizontal_top(|ui| {
        let width = ui.available_width() - if panel { 300.0 } else { 0.0 };
        ui.allocate_ui_with_layout(
            egui::vec2(width.max(240.0), height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                let graph = relation_graph::Graph {
                    nodes: &model.nodes,
                    edges: &model.edges,
                    positions: &state.positions,
                    selected: state.selected.as_deref(),
                    selected_edge: state.selected_edge.as_deref(),
                    highlight: &[],
                    dim_others: true,
                    compact: false,
                    all: state.all_chapters,
                };
                let mut gs = std::mem::take(&mut state.graph);
                let events = relation_graph::show(ui, palette, &graph, &mut gs, height - 40.0);
                state.graph = gs;
                apply_events(state, events, out);
                graph_rect = ui.min_rect();
            },
        );
        ui.vertical(|ui| {
            if state.adding {
                add_panel(ui, palette, state, derived, out);
            } else if let Some(r) = &relation {
                relation_panel(ui, palette, state, derived, r, out);
            } else if let Some(p) = &page {
                node_summary(ui, palette, state, derived, p, out);
            }
        });
    });
    // Over the graph's top left corner, as ProseCraft floats it.
    if state.suggestions_open {
        egui::Area::new(egui::Id::unique("network-suggestions"))
            .order(egui::Order::Foreground)
            .fixed_pos(graph_rect.left_top() + egui::vec2(12.0, 44.0))
            .show(ui.ctx(), |ui| suggestions(ui, palette, state, derived, out));
    }
}

fn apply_events(state: &mut NetworkState, events: relation_graph::Events, out: &mut Out) {
    // The graph reports "select X, then clear the other kind"; clearing one kind never undoes
    // a selection of the other.
    match (events.select_node, events.select_edge) {
        (Some(Some(node)), _) => {
            state.selected = Some(node);
            state.selected_edge = None;
            state.adding = false;
        }
        (_, Some(Some(edge))) => {
            state.selected_edge = Some(edge);
            state.selected = None;
            state.adding = false;
        }
        (node, edge) => {
            if node.is_some() {
                state.selected = None;
            }
            if edge.is_some() {
                state.selected_edge = None;
            }
            if node.is_some() || edge.is_some() {
                state.adding = false;
            }
        }
    }
    if let Some((id, at)) = events.moved {
        state.positions.insert(id, at);
    }
    if let Some(id) = events.open {
        out.open = Some(Selection::Document(id));
    }
}

/// The Network view.
pub(crate) fn show(
    ui: &mut Ui,
    palette: &Palette,
    project: &JsValue,
    derived: &Derived,
    state: &mut NetworkState,
    out: &mut Out,
) {
    let model = graph_model(state, derived);
    relayout_if_changed(state, &model);
    let chapters = derived.story.chapters.len();
    let summary = format!(
        "{} · {} · {}",
        plural(model.nodes.len(), "page", "pages"),
        plural(model.edges.len(), "edge", "edges"),
        if state.all_chapters {
            "all chapters".to_owned()
        } else if chapters == 0 {
            "No chapters".to_owned()
        } else {
            format!(
                "Ch {} of {chapters}",
                state.chapter_index.min(chapters - 1) + 1
            )
        }
    );
    ui.add_space(20.0);
    header(ui, palette, state, derived, &summary, out);
    toolbar(ui, palette, state, &model);
    ui.separator();
    let characters = derived.story.character_pages().len();
    match state.mode {
        Mode::Matrix => matrix(ui, palette, state, derived, out),
        _ if characters == 0 => {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No character pages yet").strong());
                ui.label(muted(
                    palette,
                    "Create a page in the Characters folder to start mapping relationships.",
                ));
            });
        }
        Mode::Cards => cards(ui, palette, state, derived),
        Mode::Network => network(ui, palette, state, derived, &model, out),
    }
    if state.mode == Mode::Matrix || characters > 0 {
        ui.separator();
        let declared = model
            .edges
            .iter()
            .filter(|e| e.status == Status::Declared)
            .count();
        chapter_footer(ui, palette, state, derived, declared);
    }
    if let Some(id) = state.card.clone() {
        match derived.story.pages().find(|p| p.id == id) {
            Some(page) => {
                let page = page.clone();
                crate::character_sheet::card_panel(
                    ui, palette, project, derived, &page, state, out,
                );
            }
            None => state.card = None,
        }
    }
}
