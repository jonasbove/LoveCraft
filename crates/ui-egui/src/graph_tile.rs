//! The graph tile of a split pane (ProseCraft `GraphTile.svelte` and `GraphNodeCard.svelte`,
//! upstream `bcb884d`): the characters of the scene being written and their relations (or one
//! Wiki page's), the detected characters to confirm, and a card for the page selected.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{RichText, Ui};
use prosecraft_core::js::JsValue;
use prosecraft_core::network::graph::{self, Point};
use prosecraft_core::network::layout::{LayoutEdge, LayoutNode, Options, layout};
use prosecraft_core::network::relations::{Mode, relation_active_at, reverse_view};
use prosecraft_core::network::tile::tile_model;

use crate::network_view::Derived;
use crate::relation_graph::{
    self, GraphState, kind_color, kind_name, plural, relation_color, relation_name,
};
use crate::theme::Palette;
use crate::views::name_field;
use crate::widgets::card;

/// What the network reads from the project, worked out once per stored revision and shared by
/// the view and the panes. `stored` is the library's record (its `id`, `revision`, `updatedAt`
/// and `project`).
pub(crate) fn derived(ctx: &egui::Context, stored: &JsValue) -> Arc<Derived> {
    let field = |k: &str| prosecraft_core::js::stringify(stored.get(k).unwrap_or(&JsValue::Null));
    let key = format!(
        "{}:{}:{}",
        field("id"),
        field("revision"),
        field("updatedAt")
    );
    let id = egui::Id::unique("network-derived");
    if let Some((k, d)) = ctx.data(|m| m.get_temp::<(String, Arc<Derived>)>(id))
        && k == key
    {
        return d;
    }
    let d = Arc::new(Derived::new(
        stored.get("project").unwrap_or(&JsValue::Null),
    ));
    ctx.data_mut(|m| m.insert_temp(id, (key, d.clone())));
    d
}

#[derive(Clone, Default)]
struct TileState {
    positions: HashMap<String, Point>,
    key: String,
    selected: Option<String>,
    graph: GraphState,
}

/// What a graph tile asks for.
#[derive(Default)]
pub(crate) struct TileAsked {
    pub confirm: Option<String>,
    pub open: Option<String>,
    pub beside: Option<String>,
}

pub(crate) fn show(
    ui: &mut Ui,
    palette: &Palette,
    stored: &JsValue,
    pane: &str,
    scene: Option<&str>,
    page: Option<&str>,
    height: f32,
) -> TileAsked {
    let mut asked = TileAsked::default();
    let derived = derived(ui.ctx(), stored);
    let tile = tile_model(&derived.story, scene, page);
    let pages: Vec<graph::Page> = tile
        .pages
        .iter()
        .map(|p| graph::Page {
            id: p.id.clone(),
            title: p.title.clone(),
            kind: p.kind,
        })
        .collect();
    let model = graph::build(&graph::Input {
        pages: &pages,
        relations: &tile.relations,
        appearances: &derived.appearances,
        suggestions: &[],
        chapters: &derived.story.chapters,
        chapter_id: None,
        scene_chapters: None,
        kinds: None,
    });
    let title = match page {
        Some(id) => format!(
            "Graph: {}",
            derived
                .story
                .docs
                .iter()
                .find(|d| d.id == id)
                .map_or("page", |d| d.title.as_str())
        ),
        None => "Graph: characters in this scene".to_owned(),
    };
    ui.label(
        RichText::new(&title)
            .strong()
            .size(15.0)
            .color(palette.text_heading),
    );
    let id = egui::Id::unique("graph-tile").with(pane);
    let mut state: TileState = ui.data(|d| d.get_temp(id)).unwrap_or_default();
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
    if key != state.key {
        state.key = key;
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
                previous: state.positions.clone(),
                ..Options::default()
            },
        );
    }
    if model.nodes.is_empty() {
        let empty = if page.is_some() {
            "This page has no relations yet."
        } else if tile.scene_id.is_some() {
            "No characters detected in this scene yet."
        } else {
            "Open a scene to see its character graph."
        };
        ui.label(RichText::new(empty).size(12.0).color(palette.text_muted));
    } else {
        let highlight: Vec<String> = tile.characters.iter().map(|c| c.id.clone()).collect();
        let graph = relation_graph::Graph {
            nodes: &model.nodes,
            edges: &model.edges,
            positions: &state.positions,
            selected: state.selected.as_deref(),
            selected_edge: None,
            highlight: &highlight,
            dim_others: true,
            compact: true,
            all: false,
        };
        let events = relation_graph::show(
            ui,
            palette,
            &graph,
            &mut state.graph,
            (height - 120.0).max(220.0),
        );
        if let Some(node) = events.select_node {
            state.selected = node;
        }
        if let Some((n, at)) = events.moved {
            state.positions.insert(n, at);
        }
        if let Some(open) = events.open {
            asked.beside = Some(open);
        }
        if let Some(node) = state
            .selected
            .clone()
            .and_then(|s| model.nodes.iter().find(|n| n.id == s).cloned())
        {
            node_card(
                ui,
                palette,
                &derived,
                &node,
                tile.scene_id.as_deref(),
                tile.chapter_id.as_deref(),
                &mut state,
                &mut asked,
            );
        }
    }
    for c in &tile.suggestions {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("Suggested: {} appears here (add?)", c.label)).size(12.0),
            );
            let confirm = ui.button("Confirm");
            name_field(ui, &confirm, &format!("Confirm {}", c.label));
            if confirm.clicked() {
                asked.confirm = Some(c.id.clone());
            }
        });
    }
    ui.data_mut(|d| d.insert_temp(id, state));
    asked
}

/// `GraphNodeCard.svelte`.
#[allow(clippy::too_many_arguments)]
fn node_card(
    ui: &mut Ui,
    palette: &Palette,
    derived: &Derived,
    node: &graph::Node,
    scene: Option<&str>,
    chapter: Option<&str>,
    state: &mut TileState,
    asked: &mut TileAsked,
) {
    let appearance = derived.appearance(&node.id);
    let count = appearance.map_or(0, |a| a.scene_count());
    let mode = scene.and_then(|s| {
        appearance
            .and_then(|a| a.scenes.iter().find(|x| x.scene_id == s))
            .map(|x| x.mode)
    });
    let stored = derived.relations.clone();
    let mut seen = Vec::new();
    let mut rows = Vec::new();
    for s in stored.iter().cloned().chain(reverse_view(&stored)) {
        if s.source != node.id {
            continue;
        }
        let Some(other) = derived.title(&s.relation.to) else {
            continue;
        };
        let incoming = s.relation.id.starts_with("reverse:");
        let base = s.relation.id.trim_start_matches("reverse:").to_owned();
        if seen.contains(&base) {
            continue;
        }
        if let Some(c) = chapter
            && !relation_active_at(&s.relation, c, &derived.story.chapters)
        {
            continue;
        }
        seen.push(base);
        rows.push((
            s.relation.kind.clone(),
            s.relation.label.clone(),
            other.to_owned(),
            incoming,
        ));
    }
    card(palette).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&node.initials)
                    .strong()
                    .color(kind_color(palette, node.kind)),
            );
            ui.vertical(|ui| {
                ui.label(RichText::new(&node.label).strong());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(kind_name(node.kind))
                            .size(11.0)
                            .color(kind_color(palette, node.kind)),
                    );
                    ui.label(
                        RichText::new(format!("· {}", plural(count, "scene", "scenes"))).size(11.0),
                    );
                    if scene.is_some() {
                        let text = match mode {
                            Some(Mode::Confirmed) => "In this scene",
                            Some(Mode::Auto) => "In this scene · auto",
                            None => "Not in this scene",
                        };
                        ui.label(RichText::new(format!("· {text}")).size(11.0));
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let close = ui.small_button("×");
                name_field(ui, &close, "Close");
                if close.clicked() {
                    state.selected = None;
                }
            });
        });
        for (kind, label, other, incoming) in &rows {
            ui.horizontal(|ui| {
                let text = label
                    .clone()
                    .unwrap_or_else(|| relation_name(kind).to_owned());
                ui.label(
                    RichText::new(format!("{} {text}", if *incoming { "←" } else { "→" }))
                        .size(11.0)
                        .color(relation_color(palette, kind)),
                );
                ui.label(RichText::new(other).size(11.0));
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Open page").clicked() {
                asked.open = Some(node.id.clone());
            }
            if ui.button("Open beside").clicked() {
                asked.beside = Some(node.id.clone());
            }
        });
    });
}
