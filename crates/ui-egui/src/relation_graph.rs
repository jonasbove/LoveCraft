//! The relation network canvas (ProseCraft `RelationGraph.svelte`, upstream `bcb884d`): pages
//! as circles with their initials, relations as arrows coloured by type, suggestions as dashed
//! lines with a scene count. Drag a page to move it, drag the background to pan, scroll or use
//! the buttons to zoom, double-click a page to open it; the search field centres on matches.
//! Each page and each relation is a widget of its own, named as ProseCraft names them.

use std::collections::{HashMap, HashSet};

use egui::{Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use prosecraft_core::network::graph::{
    Edge, Node, Point, Status, graph_bounds, incident_edge_ids, matching_node_ids, neighbour_ids,
};
use prosecraft_core::network::layout::{View, fit_view};

use crate::theme::Palette;
use crate::views::name_field;

const NODE_RADIUS: f32 = 26.0;
const MIN_SCALE: f64 = 0.25;
const MAX_SCALE: f64 = 3.0;
const MAP_WIDTH: f32 = 150.0;
const MAP_HEIGHT: f32 = 100.0;

/// A page kind's colour (`kindColors`).
pub(crate) fn kind_color(palette: &Palette, kind: &str) -> Color32 {
    match kind {
        "character" => palette.accent,
        "place" => palette.link,
        "organisation" => palette.warning,
        "item" => palette.quote,
        _ => palette.text_muted,
    }
}

/// A page kind's name (`kindNames`, `KIND_LABELS` of the summary and the sheet).
pub(crate) fn kind_name(kind: &str) -> &'static str {
    match kind {
        "character" => "Character",
        "place" => "Place",
        "organisation" => "Organisation",
        "item" => "Item",
        _ => "Page",
    }
}

/// A relation type's colour (`relationColors`).
pub(crate) fn relation_color(palette: &Palette, kind: &str) -> Color32 {
    match kind {
        "family" => palette.accent,
        "friend" => palette.success,
        "rival" => palette.danger,
        "love" => palette.quote,
        "mentor" => palette.link,
        "employer" => palette.warning,
        _ => palette.text_muted,
    }
}

/// A relation type's name (`RELATION_LABELS`).
pub(crate) fn relation_name(kind: &str) -> &'static str {
    match kind {
        "family" => "Family",
        "friend" => "Friend",
        "rival" => "Rival",
        "love" => "Love",
        "mentor" => "Mentor",
        "employer" => "Employer",
        _ => "Other",
    }
}

pub(crate) fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `label.slice(0, n)` with an ellipsis when longer (UTF-16 lengths, as ProseCraft counts).
pub(crate) fn short_label(label: &str, max: usize) -> String {
    let units: Vec<u16> = label.encode_utf16().collect();
    if units.len() > max {
        format!("{}…", String::from_utf16_lossy(&units[..max - 1]))
    } else {
        label.to_owned()
    }
}

/// A page name's distance below its circle's edge, and its font size, at 100%.
const LABEL_OFFSET: f32 = 15.0;
const LABEL_SIZE: f32 = 11.0;
/// The clear space kept between two names, and around a relation's label.
const LABEL_GAP: f32 = 6.0;

/// The widest each page name may be so that no two names on the same line run into each other:
/// half the horizontal distance to every name it would otherwise touch, less a gap. `items` are
/// each name's centre and natural width; `height` is a name's height. A name with nothing beside
/// it keeps its natural width. Names stay centred under their pages, so two names each kept to
/// these widths never overlap.
fn label_widths(items: &[(Pos2, f32)], height: f32) -> Vec<f32> {
    items
        .iter()
        .enumerate()
        .map(|(i, (at, natural))| {
            items
                .iter()
                .enumerate()
                .filter(|(j, (other, _))| {
                    *j != i && (other.y - at.y).abs() < height + LABEL_GAP / 2.0
                })
                .map(|(_, (other, _))| (other.x - at.x).abs() - LABEL_GAP)
                .fold(*natural, f32::min)
                .max(0.0)
        })
        .collect()
}

/// `text` (already `shortLabel`ed) shortened with "…" until `measure` says it fits in `width`;
/// empty when not even the ellipsis fits.
fn elide(text: &str, width: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(text) <= width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (0..chars.len()).rev() {
        let candidate = format!("{}…", chars[..keep].iter().collect::<String>().trim_end());
        if measure(&candidate) <= width {
            return candidate;
        }
    }
    String::new()
}

/// A page's circle and name as drawn this frame.
struct Placed {
    circle: Rect,
    /// The name's box, empty when the name doesn't fit at all.
    label: Rect,
    text: String,
}

/// Where every page's circle and name go, each name shortened to the room beside its
/// neighbours (the full name is the page's tooltip and accessible name).
fn place_labels(ui: &Ui, graph: &Graph, view: View, origin: Pos2) -> HashMap<String, Placed> {
    let scale = view.scale as f32;
    let radius = NODE_RADIUS * scale;
    let font = egui::FontId::proportional((LABEL_SIZE * scale).max(6.0));
    let measure = |t: &str| {
        ui.painter()
            .layout_no_wrap(t.to_owned(), font.clone(), Color32::WHITE)
            .size()
    };
    let height = measure("Ag").y;
    let pages: Vec<(&Node, Pos2, String)> = graph
        .nodes
        .iter()
        .map(|n| {
            let p = graph.positions.get(&n.id).copied().unwrap_or_default();
            let centre = to_screen(view, origin, p);
            (n, centre, short_label(&n.label, 20))
        })
        .collect();
    let items: Vec<(Pos2, f32)> = pages
        .iter()
        .map(|(_, centre, text)| {
            (
                *centre + Vec2::new(0.0, radius + LABEL_OFFSET * scale),
                measure(text).x,
            )
        })
        .collect();
    let widths = label_widths(&items, height);
    pages
        .into_iter()
        .zip(items.iter().zip(widths))
        .map(|((node, centre, text), ((at, _), width))| {
            let text = elide(&text, width, |t| measure(t).x);
            let label = if text.is_empty() {
                Rect::NOTHING
            } else {
                Rect::from_center_size(*at, Vec2::new(measure(&text).x, height))
            };
            let placed = Placed {
                circle: Rect::from_center_size(centre, Vec2::splat(radius * 2.0)),
                label,
                text,
            };
            (node.id.clone(), placed)
        })
        .collect()
}

/// The points `fitView` frames: each page's circle and the name below it, not only its centre,
/// so Fit never cuts off the lowest name (`widths` are the names' widths at 100%).
fn fit_points(graph: &Graph, widths: &HashMap<String, f32>) -> Vec<Point> {
    let r = f64::from(NODE_RADIUS);
    let below = f64::from(NODE_RADIUS + LABEL_OFFSET + LABEL_SIZE);
    graph
        .nodes
        .iter()
        .filter_map(|n| {
            let p = graph.positions.get(&n.id)?;
            let half = r.max(f64::from(widths.get(&n.id).copied().unwrap_or(0.0)) / 2.0);
            Some([
                Point {
                    x: p.x - half,
                    y: p.y - r,
                },
                Point {
                    x: p.x + half,
                    y: p.y + below,
                },
            ])
        })
        .flatten()
        .collect()
}

/// Each page name's width at 100%, for [`fit_points`].
fn natural_widths(ui: &Ui, graph: &Graph) -> HashMap<String, f32> {
    let font = egui::FontId::proportional(LABEL_SIZE);
    graph
        .nodes
        .iter()
        .map(|n| {
            let text = short_label(&n.label, 20);
            let width = ui
                .painter()
                .layout_no_wrap(text, font.clone(), Color32::WHITE)
                .size()
                .x;
            (n.id.clone(), width)
        })
        .collect()
}

/// The space Fit leaves around the pages and their names.
const FIT_PADDING: f64 = 24.0;

/// The canvas's own state: the view transform, the search and what is being dragged.
#[derive(Clone, Debug)]
pub struct GraphState {
    pub view: View,
    pub query: String,
    active_match: usize,
    last_fit: String,
    dragging: Option<(String, Point)>,
}

impl Default for GraphState {
    fn default() -> Self {
        Self {
            view: View {
                scale: 1.0,
                translate: Point::default(),
            },
            query: String::new(),
            active_match: 0,
            last_fit: String::new(),
            dragging: None,
        }
    }
}

/// What to draw and how.
pub struct Graph<'a> {
    pub nodes: &'a [Node],
    pub edges: &'a [Edge],
    pub positions: &'a HashMap<String, Point>,
    pub selected: Option<&'a str>,
    pub selected_edge: Option<&'a str>,
    pub highlight: &'a [String],
    pub dim_others: bool,
    pub compact: bool,
    /// "All pages" or "This chapter".
    pub all: bool,
}

/// What happened on the canvas this frame.
#[derive(Default)]
pub struct Events {
    /// A page selected (`Some(Some(id))`) or the selection cleared (`Some(None)`).
    pub select_node: Option<Option<String>>,
    pub select_edge: Option<Option<String>>,
    pub moved: Option<(String, Point)>,
    pub open: Option<String>,
}

fn to_screen(view: View, origin: Pos2, p: Point) -> Pos2 {
    Pos2::new(
        origin.x + (p.x * view.scale + view.translate.x) as f32,
        origin.y + (p.y * view.scale + view.translate.y) as f32,
    )
}

fn zoom_at(state: &mut GraphState, at: Point, factor: f64) {
    let view = state.view;
    let scale = (view.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
    if scale == view.scale {
        return;
    }
    let wx = (at.x - view.translate.x) / view.scale;
    let wy = (at.y - view.translate.y) / view.scale;
    state.view = View {
        scale,
        translate: Point {
            x: at.x - wx * scale,
            y: at.y - wy * scale,
        },
    };
}

fn centre_on(state: &mut GraphState, size: Vec2, p: Point) {
    state.view.translate = Point {
        x: f64::from(size.x) / 2.0 - p.x * state.view.scale,
        y: f64::from(size.y) / 2.0 - p.y * state.view.scale,
    };
}

/// The toolbar: search, counts, zoom.
fn toolbar(
    ui: &mut Ui,
    palette: &Palette,
    graph: &Graph,
    state: &mut GraphState,
    size: Vec2,
    events: &mut Events,
) {
    ui.horizontal_wrapped(|ui| {
        let search = ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .hint_text("Find a page…")
                .desired_width(if graph.compact { 120.0 } else { 180.0 }),
        );
        name_field(ui, &search, "Search pages in the network");
        if search.changed() {
            state.active_match = 0;
            if let Some(first) = matching_node_ids(graph.nodes, &state.query).first()
                && let Some(p) = graph.positions.get(first)
            {
                centre_on(state, size, *p);
            }
        }
        if search.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.query.clear();
            state.active_match = 0;
        }
        if search.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let ids = matching_node_ids(graph.nodes, &state.query);
            if !ids.is_empty() {
                let index = state.active_match % ids.len();
                if let Some(p) = graph.positions.get(&ids[index]) {
                    centre_on(state, size, *p);
                }
                events.select_node = Some(Some(ids[index].clone()));
                events.select_edge = Some(None);
                state.active_match = (index + 1) % ids.len();
                search.request_focus();
            }
        }
        if !state.query.is_empty() {
            let clear = ui.small_button("×");
            name_field(ui, &clear, "Clear search");
            if clear.clicked() {
                state.query.clear();
                state.active_match = 0;
            }
        }
        let muted = |t: String| RichText::new(t).size(11.0).color(palette.text_muted);
        ui.label(muted(format!(
            "{} pages · {} links",
            graph.nodes.len(),
            graph.edges.len()
        )));
        ui.label(muted(
            if graph.all {
                "All pages"
            } else {
                "This chapter"
            }
            .to_owned(),
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let fit = ui.small_button("Fit").on_hover_text("Fit graph to view");
            name_field(ui, &fit, "Fit graph to view");
            if fit.clicked() {
                state.view = fit_view(
                    &fit_points(graph, &natural_widths(ui, graph)),
                    f64::from(size.x),
                    f64::from(size.y),
                    FIT_PADDING,
                );
            }
            let centre = Point {
                x: f64::from(size.x) / 2.0,
                y: f64::from(size.y) / 2.0,
            };
            let zoom_in = ui
                .add_enabled(state.view.scale < MAX_SCALE, egui::Button::new("+").small())
                .on_hover_text("Zoom in");
            name_field(ui, &zoom_in, "Zoom in");
            if zoom_in.clicked() {
                zoom_at(state, centre, 1.25);
            }
            let zoom_out = ui
                .add_enabled(state.view.scale > MIN_SCALE, egui::Button::new("−").small())
                .on_hover_text("Zoom out");
            name_field(ui, &zoom_out, "Zoom out");
            if zoom_out.clicked() {
                zoom_at(state, centre, 1.0 / 1.25);
            }
            ui.label(muted(format!("{}%", (state.view.scale * 100.0).round())));
        });
    });
}

struct EdgeView<'a> {
    edge: &'a Edge,
    a: Pos2,
    b: Pos2,
    mid: Pos2,
    dimmed: bool,
}

fn faded(c: Color32, dimmed: bool) -> Color32 {
    if dimmed { c.gamma_multiply(0.25) } else { c }
}

fn dashed(painter: &egui::Painter, a: Pos2, b: Pos2, stroke: Stroke) {
    let length = a.distance(b);
    let dir = (b - a) / length.max(1.0);
    let mut t = 0.0;
    while t < length {
        let end = (t + 5.0).min(length);
        painter.line_segment([a + dir * t, a + dir * end], stroke);
        t += 9.0;
    }
}

fn edge_aria(edge: &Edge, from: &str, to: &str) -> String {
    match edge.status {
        Status::Suggested => {
            let count = edge.count.unwrap_or(0);
            format!(
                "Suggested relation between {from} and {to}, {} together. Select to review.",
                plural(count, "scene", "scenes")
            )
        }
        Status::Declared => {
            let kind = if edge.label.is_empty() {
                &edge.kind
            } else {
                &edge.label
            };
            format!("{from}, {kind}, {to}. Select to edit.")
        }
    }
}

/// Where a relation's label goes: beside the middle of its line as ProseCraft puts it, or, when
/// that covers a page, a page name or another label, the other side or further along the line.
/// `None` when every place is taken (the label is then left out; the relation still has its
/// widget at the middle, with its full name).
fn edge_label_rect(v: &EdgeView, size: Vec2, scale: f32, taken: &[Rect]) -> Option<Rect> {
    let along = v.b - v.a;
    let dir = along.normalized();
    let side = Vec2::new(-dir.y, dir.x) * 9.0 * scale;
    let candidates = [
        v.mid,
        v.mid - side * 2.0,
        v.a + along * 0.35 + side,
        v.a + along * 0.65 + side,
        v.a + along * 0.35 - side,
        v.a + along * 0.65 - side,
    ];
    candidates
        .into_iter()
        .map(|c| Rect::from_center_size(c, size))
        .find(|r| {
            let clear = r.expand(LABEL_GAP / 2.0);
            !taken.iter().any(|t| t.intersects(clear))
        })
}

/// Draws the edges and gives each one a widget at its middle; returns the one clicked.
fn edges(
    ui: &mut Ui,
    palette: &Palette,
    graph: &Graph,
    views: &[EdgeView],
    placed: &HashMap<String, Placed>,
    salt: egui::Id,
) -> Option<String> {
    // Page circles and names are kept clear of relation labels, and so are the labels already
    // placed.
    let mut taken: Vec<Rect> = placed
        .values()
        .flat_map(|p| [p.circle, p.label])
        .filter(|r| r.is_positive())
        .collect();
    let painter = ui.painter().clone();
    let scale = ui
        .data(|d| d.get_temp::<f32>(salt.with("scale")))
        .unwrap_or(1.0);
    let mut clicked = None;
    for v in views {
        let suggested = v.edge.status == Status::Suggested;
        let color = faded(
            if suggested {
                palette.text_faint
            } else {
                relation_color(palette, &v.edge.kind)
            },
            v.dimmed,
        );
        let selected = graph.selected_edge == Some(v.edge.id.as_str());
        if selected {
            painter.line_segment(
                [v.a, v.b],
                Stroke::new(7.0 * scale, color.gamma_multiply(0.3)),
            );
        }
        let stroke = Stroke::new(if selected { 2.4 } else { 1.6 } * scale.max(0.6), color);
        if suggested {
            dashed(&painter, v.a, v.b, stroke);
        } else {
            painter.line_segment([v.a, v.b], stroke);
            let dir = (v.b - v.a).normalized();
            let side = Vec2::new(-dir.y, dir.x);
            let tip = v.b;
            let size = 9.0 * scale;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    tip,
                    tip - dir * size + side * size / 2.0,
                    tip - dir * size - side * size / 2.0,
                ],
                color,
                Stroke::NONE,
            ));
        }
        let from = graph
            .nodes
            .iter()
            .find(|n| n.id == v.edge.from)
            .map_or(v.edge.from.as_str(), |n| n.label.as_str());
        let to = graph
            .nodes
            .iter()
            .find(|n| n.id == v.edge.to)
            .map_or(v.edge.to.as_str(), |n| n.label.as_str());
        let font = egui::FontId::proportional(10.0 * scale.clamp(0.7, 1.4));
        let (text, ink, pad) = if suggested {
            (
                plural(v.edge.count.unwrap_or(0), "scene", "scenes"),
                palette.text_muted,
                Vec2::new(10.0, 2.0),
            )
        } else {
            (short_label(&v.edge.label, 18), color, Vec2::new(4.0, 0.0))
        };
        let galley = painter.layout_no_wrap(text, font, ink);
        let size = galley.size() + pad;
        let hit = match edge_label_rect(v, size, scale, &taken) {
            Some(rect) if !galley.is_empty() => {
                taken.push(rect);
                // A backing in the canvas's colour, as ProseCraft's label halo, so the line
                // doesn't run through the words.
                let fill = if suggested {
                    palette.surface_raised
                } else {
                    palette.surface
                };
                painter.rect_filled(rect, CornerRadius::same(6), faded(fill, v.dimmed));
                painter.galley(rect.center() - galley.size() / 2.0, galley, ink);
                rect.expand(if suggested { 0.0 } else { 3.0 })
            }
            _ => Rect::from_center_size(v.mid, Vec2::splat(12.0)),
        };
        let response = ui.interact(hit, salt.with(("edge", &v.edge.id)), Sense::click());
        name_field(ui, &response, &edge_aria(v.edge, from, to));
        if response.clicked() {
            clicked = Some(v.edge.id.clone());
        }
    }
    clicked
}

fn edge_views<'a>(
    graph: &'a Graph,
    origin: Pos2,
    view: View,
    focus: &HashSet<String>,
    focused_edges: &HashSet<String>,
    has_focus: bool,
) -> Vec<EdgeView<'a>> {
    let r = f64::from(NODE_RADIUS);
    graph
        .edges
        .iter()
        .map(|edge| {
            let from = graph.positions.get(&edge.from).copied().unwrap_or_default();
            let to = graph.positions.get(&edge.to).copied().unwrap_or_default();
            let (dx, dy) = (to.x - from.x, to.y - from.y);
            let length = dx.hypot(dy);
            let length = if length == 0.0 { 1.0 } else { length };
            let (ux, uy) = (dx / length, dy / length);
            let offset = edge.lane * 9.0;
            let a = Point {
                x: from.x + ux * r - uy * offset,
                y: from.y + uy * r + ux * offset,
            };
            let b = Point {
                x: to.x - ux * (r + 4.0) - uy * offset,
                y: to.y - uy * (r + 4.0) + ux * offset,
            };
            let mid = Point {
                x: (a.x + b.x) / 2.0 - uy * 9.0,
                y: (a.y + b.y) / 2.0 + ux * 9.0,
            };
            let dimmed = graph.dim_others
                && has_focus
                && !focused_edges.contains(&edge.id)
                && !(focus.contains(&edge.from) && focus.contains(&edge.to));
            EdgeView {
                edge,
                a: to_screen(view, origin, a),
                b: to_screen(view, origin, b),
                mid: to_screen(view, origin, mid),
                dimmed,
            }
        })
        .collect()
}

/// The pages in focus (the selection, its neighbours, the highlighted ones), if any.
struct Focus {
    ids: HashSet<String>,
    any: bool,
}

/// The pages; returns what was clicked, double-clicked or dragged.
#[allow(clippy::too_many_arguments)]
fn nodes(
    ui: &mut Ui,
    palette: &Palette,
    graph: &Graph,
    state: &mut GraphState,
    origin: Pos2,
    placed: &HashMap<String, Placed>,
    focus: &Focus,
    events: &mut Events,
) {
    let view = state.view;
    let scale = view.scale as f32;
    let matches: HashSet<String> = matching_node_ids(graph.nodes, &state.query)
        .into_iter()
        .collect();
    let neighbours = graph
        .selected
        .map(|s| neighbour_ids(graph.edges, s))
        .unwrap_or_default();
    let painter = ui.painter().clone();
    for node in graph.nodes {
        let p = graph.positions.get(&node.id).copied().unwrap_or_default();
        let centre = to_screen(view, origin, p);
        let radius = NODE_RADIUS * scale;
        let dimmed = graph.dim_others && focus.any && !focus.ids.contains(&node.id);
        let color = faded(kind_color(palette, node.kind), dimmed);
        let selected = graph.selected == Some(node.id.as_str());
        if selected {
            painter.circle_stroke(
                centre,
                radius + 6.0 * scale,
                Stroke::new(2.0, palette.accent),
            );
        }
        if matches.contains(&node.id) {
            painter.circle_stroke(
                centre,
                radius + 4.0 * scale,
                Stroke::new(2.0, palette.warning),
            );
        }
        let fill =
            if selected || graph.highlight.contains(&node.id) || neighbours.contains(&node.id) {
                color.gamma_multiply(0.25)
            } else {
                faded(palette.surface_raised, dimmed)
            };
        painter.circle(centre, radius, fill, Stroke::new(2.0, color));
        painter.text(
            centre,
            egui::Align2::CENTER_CENTER,
            &node.initials,
            egui::FontId::proportional((13.0 * scale).max(6.0)),
            color,
        );
        if let Some(label) = placed.get(&node.id).filter(|p| !p.text.is_empty()) {
            // A backing in the canvas's colour (ProseCraft's label halo) keeps lines off the name.
            painter.rect_filled(
                label.label.expand2(Vec2::new(2.0, 0.0)),
                CornerRadius::same(4),
                faded(palette.surface, dimmed),
            );
            painter.text(
                label.label.center(),
                egui::Align2::CENTER_CENTER,
                &label.text,
                egui::FontId::proportional((LABEL_SIZE * scale).max(6.0)),
                faded(palette.text_primary, dimmed),
            );
        }
        let degree = graph
            .edges
            .iter()
            .filter(|e| e.from == node.id || e.to == node.id)
            .count();
        let response = ui
            .interact(
                Rect::from_center_size(centre, Vec2::splat(radius * 2.0)),
                ui.scope_id().with(("graph-node", &node.id)),
                Sense::click_and_drag(),
            )
            .on_hover_text(&node.label);
        name_field(
            ui,
            &response,
            &format!(
                "{}, {}, {}",
                node.label,
                kind_name(node.kind),
                plural(degree, "link", "links")
            ),
        );
        if response.drag_started() || response.clicked() {
            events.select_node = Some(Some(node.id.clone()));
            events.select_edge = Some(None);
        }
        if response.drag_started() {
            state.dragging = Some((node.id.clone(), p));
        }
        if response.dragged()
            && let Some((id, at)) = &mut state.dragging
            && *id == node.id
        {
            let d = response.drag_delta();
            at.x += f64::from(d.x) / view.scale;
            at.y += f64::from(d.y) / view.scale;
            events.moved = Some((id.clone(), *at));
        }
        if response.drag_stopped() {
            state.dragging = None;
        }
        if response.double_clicked() {
            events.open = Some(node.id.clone());
        }
    }
}

fn minimap(ui: &Ui, palette: &Palette, graph: &Graph, state: &GraphState, rect: Rect) {
    let points: Vec<(Point, &Node)> = graph
        .nodes
        .iter()
        .map(|n| (graph.positions.get(&n.id).copied().unwrap_or_default(), n))
        .collect();
    let (bx, by, bw, bh) = graph_bounds(points.iter().map(|(p, _)| *p));
    let r = f64::from(NODE_RADIUS);
    let view = state.view;
    let size = rect.size();
    let shows = !graph.nodes.is_empty()
        && (graph.all
            || bw * view.scale > f64::from(size.x) - 24.0
            || bh * view.scale > f64::from(size.y) - 24.0);
    if !shows {
        return;
    }
    let (left, top) = (bx - r, by - r);
    let (w, h) = ((bw + r * 2.0).max(1.0), (bh + r * 2.0).max(1.0));
    let padding = 8.0;
    let s = ((f64::from(MAP_WIDTH) - padding * 2.0) / w)
        .min((f64::from(MAP_HEIGHT) - padding * 2.0) / h);
    let ox = f64::from(MAP_WIDTH) / 2.0 - (left + w / 2.0) * s;
    let oy = f64::from(MAP_HEIGHT) / 2.0 - (top + h / 2.0) * s;
    let map = Rect::from_min_size(
        rect.right_bottom() - Vec2::new(MAP_WIDTH + 10.0, MAP_HEIGHT + 10.0),
        Vec2::new(MAP_WIDTH, MAP_HEIGHT),
    );
    let painter = ui.painter_at(map);
    painter.rect(
        map,
        CornerRadius::same(8),
        palette.surface,
        Stroke::new(1.0, palette.border),
        egui::StrokeKind::Inside,
    );
    let at = |x: f64, y: f64| map.min + Vec2::new(x as f32, y as f32);
    for (p, node) in &points {
        let selected = graph.selected == Some(node.id.as_str());
        painter.circle_filled(
            at(p.x * s + ox, p.y * s + oy),
            if selected { 3.4 } else { 2.2 },
            kind_color(palette, node.kind).gamma_multiply(if selected { 1.0 } else { 0.75 }),
        );
    }
    let vx = ox + (-view.translate.x / view.scale) * s;
    let vy = oy + (-view.translate.y / view.scale) * s;
    let vw = ((f64::from(size.x) / view.scale) * s).max(3.0);
    let vh = ((f64::from(size.y) / view.scale) * s).max(3.0);
    painter.rect_stroke(
        Rect::from_min_size(at(vx, vy), Vec2::new(vw as f32, vh as f32)),
        CornerRadius::same(2),
        Stroke::new(1.0, palette.accent),
        egui::StrokeKind::Inside,
    );
}

/// `onGraphKeydown`: arrow keys pan the canvas while it has the focus.
fn keys(ui: &Ui, state: &mut GraphState) {
    let (shift, keys) = ui.input(|i| {
        (
            i.modifiers.shift,
            [
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
            ],
        )
    });
    let step = if shift { 160.0 } else { 48.0 };
    let t = &mut state.view.translate;
    for (pressed, (dx, dy)) in
        keys.into_iter()
            .zip([(step, 0.0), (-step, 0.0), (0.0, step), (0.0, -step)])
    {
        if pressed {
            t.x += dx;
            t.y += dy;
        }
    }
}

/// Draws the graph; `height` is the canvas's height.
pub fn show(
    ui: &mut Ui,
    palette: &Palette,
    graph: &Graph,
    state: &mut GraphState,
    height: f32,
) -> Events {
    let mut events = Events::default();
    let width = ui.available_width();
    let size = Vec2::new(width, height);
    toolbar(ui, palette, graph, state, size, &mut events);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let canvas_name = format!(
        "Relationship network of {} and {}. Tab steps through pages, Enter selects, arrow keys pan.",
        plural(graph.nodes.len(), "page", "pages"),
        plural(graph.edges.len(), "link", "links")
    );
    name_field(ui, &response, &canvas_name);
    // `fitView` again when the pages or the canvas's size change.
    let fit_key = format!(
        "{}:{}:{}",
        graph
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>()
            .join("\u{0}"),
        rect.width().round(),
        rect.height().round()
    );
    if fit_key != state.last_fit {
        state.last_fit = fit_key;
        state.view = fit_view(
            &fit_points(graph, &natural_widths(ui, graph)),
            f64::from(rect.width()),
            f64::from(rect.height()),
            FIT_PADDING,
        );
    }
    ui.painter().rect(
        rect,
        CornerRadius::same(10),
        palette.surface,
        Stroke::new(1.0, palette.border),
        egui::StrokeKind::Inside,
    );
    // `updatePinch`: two fingers zoom around their middle and move the view with it.
    let pinch = ui.input(egui::InputState::multi_touch);
    if let Some(touch) = pinch.filter(|t| rect.contains(t.center_pos)) {
        let at = Point {
            x: f64::from(touch.center_pos.x - rect.min.x),
            y: f64::from(touch.center_pos.y - rect.min.y),
        };
        zoom_at(state, at, f64::from(touch.zoom_delta));
        state.view.translate.x += f64::from(touch.translation_delta.x);
        state.view.translate.y += f64::from(touch.translation_delta.y);
    } else if response.hovered() {
        // The wheel, and a trackpad's pinch (egui's zoom delta), around the pointer.
        let scroll = ui.input(|i| i.smooth_scroll_delta.y + (i.zoom_delta() - 1.0) * 400.0);
        if scroll != 0.0
            && let Some(pointer) = response.hover_pos()
        {
            let at = Point {
                x: f64::from(pointer.x - rect.min.x),
                y: f64::from(pointer.y - rect.min.y),
            };
            zoom_at(state, at, (-f64::from(scroll) * 0.0015).exp());
        }
    }
    if response.dragged() && state.dragging.is_none() && pinch.is_none() {
        let d = response.drag_delta();
        state.view.translate.x += f64::from(d.x);
        state.view.translate.y += f64::from(d.y);
    }
    if response.clicked() && (graph.selected.is_some() || graph.selected_edge.is_some()) {
        events.select_node = Some(None);
        events.select_edge = Some(None);
    }
    if response.has_focus() {
        keys(ui, state);
    }
    let focus: HashSet<String> = {
        let mut ids: HashSet<String> = graph.highlight.iter().cloned().collect();
        if let Some(s) = graph.selected {
            ids.insert(s.to_owned());
            ids.extend(neighbour_ids(graph.edges, s));
        }
        if let Some(e) = graph
            .selected_edge
            .and_then(|id| graph.edges.iter().find(|e| e.id == id))
        {
            ids.insert(e.from.clone());
            ids.insert(e.to.clone());
        }
        ids
    };
    let focused_edges: HashSet<String> = match (graph.selected_edge, graph.selected) {
        (Some(e), _) => HashSet::from([e.to_owned()]),
        (None, Some(n)) => incident_edge_ids(graph.edges, n),
        _ => HashSet::new(),
    };
    let has_focus =
        graph.selected.is_some() || graph.selected_edge.is_some() || !graph.highlight.is_empty();
    let salt = ui.scope_id().with("relation-graph");
    ui.data_mut(|d| d.insert_temp(salt.with("scale"), state.view.scale as f32));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect);
    let views = edge_views(
        graph,
        rect.min,
        state.view,
        &focus,
        &focused_edges,
        has_focus,
    );
    let placed = place_labels(&child, graph, state.view, rect.min);
    if let Some(edge) = edges(&mut child, palette, graph, &views, &placed, salt) {
        events.select_edge = Some(Some(edge));
        events.select_node = Some(None);
    }
    let focus = Focus {
        ids: focus,
        any: has_focus,
    };
    nodes(
        &mut child,
        palette,
        graph,
        state,
        rect.min,
        &placed,
        &focus,
        &mut events,
    );
    minimap(&child, palette, graph, state, rect);
    if graph.nodes.is_empty() {
        child.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No pages match this view.",
            egui::FontId::proportional(13.0),
            palette.text_muted,
        );
    }
    child.painter().text(
        rect.left_bottom() + Vec2::new(10.0, -10.0),
        egui::Align2::LEFT_BOTTOM,
        "Drag to move · Scroll to zoom",
        egui::FontId::proportional(10.0),
        palette.text_muted,
    );
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxes(items: &[(Pos2, f32)], height: f32) -> Vec<Rect> {
        items
            .iter()
            .zip(label_widths(items, height))
            .map(|((at, natural), width)| {
                Rect::from_center_size(*at, Vec2::new(natural.min(width), height))
            })
            .collect()
    }

    #[test]
    fn page_names_side_by_side_never_overlap() {
        // Two unconnected pages 34 apart (the layout's `minDistance`), with names wider than
        // that, and a third further along: drawn at full width, "Saltmarsh Quay" and "Lantern
        // Hill" ran into each other.
        let items = [
            (Pos2::new(100.0, 50.0), 80.0),
            (Pos2::new(134.0, 50.0), 64.0),
            (Pos2::new(260.0, 52.0), 90.0),
            // On another line: it keeps its whole name.
            (Pos2::new(110.0, 120.0), 120.0),
        ];
        let rects = boxes(&items, 14.0);
        for (i, a) in rects.iter().enumerate() {
            for b in rects.iter().skip(i + 1) {
                assert!(!a.intersects(*b), "{a:?} overlaps {b:?}");
            }
        }
        assert_eq!(rects[3].width(), 120.0);
        // Room is shared: each name gets at most the distance less the gap.
        assert!(rects[0].width() <= 34.0 - LABEL_GAP);
    }

    #[test]
    fn a_name_is_shortened_to_its_room_with_an_ellipsis() {
        let measure = |t: &str| t.chars().count() as f32 * 6.0;
        assert_eq!(elide("Saltmarsh Quay", 200.0, measure), "Saltmarsh Quay");
        let short = elide("Saltmarsh Quay", 40.0, measure);
        assert!(short.ends_with('…') && measure(&short) <= 40.0, "{short}");
        assert_eq!(elide("Saltmarsh Quay", 3.0, measure), "");
    }

    #[test]
    fn fit_keeps_the_lowest_name_inside_the_canvas() {
        // Two pages one above the other: Fit zooms in as far as it goes, and the lower page's
        // name below its circle fell outside the canvas when only the centres were framed.
        let nodes = [
            Node {
                id: "a".into(),
                label: "Ines Calder".into(),
                initials: "IC".into(),
                kind: "character",
            },
            Node {
                id: "b".into(),
                label: "Oren Lisle".into(),
                initials: "OL".into(),
                kind: "character",
            },
        ];
        let positions = HashMap::from([
            ("a".to_owned(), Point { x: 0.0, y: 0.0 }),
            ("b".to_owned(), Point { x: 0.0, y: 50.0 }),
        ]);
        let graph = Graph {
            nodes: &nodes,
            edges: &[],
            positions: &positions,
            selected: None,
            selected_edge: None,
            highlight: &[],
            dim_others: false,
            compact: false,
            all: true,
        };
        let widths = HashMap::from([("a".to_owned(), 60.0), ("b".to_owned(), 56.0)]);
        let (w, h) = (1000.0, 400.0);
        let view = fit_view(&fit_points(&graph, &widths), w, h, FIT_PADDING);
        let bottom = 50.0 + f64::from(NODE_RADIUS + LABEL_OFFSET + LABEL_SIZE);
        let top = -f64::from(NODE_RADIUS);
        let screen = |y: f64| y * view.scale + view.translate.y;
        assert!(screen(bottom) <= h, "the name ends at {}", screen(bottom));
        assert!(screen(top) >= 0.0);
    }
}
