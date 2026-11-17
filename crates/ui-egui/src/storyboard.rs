//! ProseCraft's Storyboard: the Story elements to add on the left, and a freeform canvas of
//! cards and diagram drawings (lines, arrows, boxes, text) that pans and zooms.

use egui::{Pos2, Rect, RichText, Stroke, Ui, Vec2, accesskit};
use prosecraft_core::js::JsValue;
use prosecraft_core::project::Project;
use prosecraft_core::storyboard::{self, CARD_WIDTH, Card, Drawing, MAX_ZOOM, MIN_ZOOM, NUDGE};

/// The diagram tools (`storyboardDrawingTools`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tool {
    #[default]
    Select,
    Pan,
    Line,
    Arrow,
    Box,
    Text,
}

impl Tool {
    const ALL: [(Self, &'static str, &'static str); 6] = [
        (Self::Select, "Select", "↖"),
        (Self::Pan, "Pan", "✋"),
        (Self::Line, "Line", "—"),
        (Self::Arrow, "Arrow", "→"),
        (Self::Box, "Box", "▢"),
        (Self::Text, "Text", "T"),
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Create,
    Move,
    Resize,
}

/// A drawing being drawn, moved or resized: what it was, and what it is now.
#[derive(Clone, Debug, PartialEq)]
struct Interaction {
    mode: Mode,
    start: (f64, f64),
    moved: Vec2,
    original: Drawing,
    current: Drawing,
}

/// The board's tool state (per project, for this session).
#[derive(Clone, Debug, PartialEq)]
struct Tools {
    tool: Tool,
    selected: Option<String>,
    /// `defaultStoryboardTextFontSize`.
    font_size: f64,
    interaction: Option<Interaction>,
    /// After adding a drawing: how many there were, so the new (last) one is selected (and a
    /// text one edited) once it is there.
    added: Option<usize>,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            selected: None,
            font_size: 14.0,
            interaction: None,
            added: None,
        }
    }
}

const TEXT_MIN: (f64, f64) = (100.0, 40.0);
const BOX_MIN: (f64, f64) = (24.0, 24.0);
const DRAWING_NUDGE: f64 = 12.0;

fn text_edit_id(drawing: &str) -> egui::Id {
    egui::Id::unique(("board-text", drawing))
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 {
        ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

fn drawing_rect(drawing: &Drawing) -> Rect {
    match drawing {
        Drawing::Line { start, end, .. } => Rect::from_two_pos(
            Pos2::new(start.0 as f32, start.1 as f32),
            Pos2::new(end.0 as f32, end.1 as f32),
        ),
        Drawing::Box {
            x,
            y,
            width,
            height,
            ..
        }
        | Drawing::Text {
            x,
            y,
            width,
            height,
            ..
        } => Rect::from_min_size(
            Pos2::new(*x as f32, *y as f32),
            Vec2::new(*width as f32, *height as f32),
        ),
    }
}

fn type_name(drawing: &Drawing) -> &'static str {
    match drawing {
        Drawing::Line { arrow: true, .. } => "arrow",
        Drawing::Line { .. } => "line",
        Drawing::Box { .. } => "box",
        Drawing::Text { .. } => "text",
    }
}

use crate::editing::Intent;
use crate::theme::Palette;
use crate::widgets::eyebrow;

/// Something being dragged from Story elements onto the canvas: its kind, source and title.
#[derive(Clone, Debug, Default, PartialEq)]
struct SourceDrag {
    kind: String,
    source: String,
    title: String,
    note: String,
}

/// Gives a widget on the canvas its on-screen bounds for screen readers (egui reports the
/// canvas's own coordinates, before the pan and zoom).
fn screen_bounds(ui: &Ui, response: &egui::Response) {
    if let Some(to_global) = ui.ctx().layer_transform_to_global(ui.layer_id()) {
        let rect = to_global * response.rect;
        ui.ctx().accesskit_node_builder(response.id, |node| {
            node.set_bounds(accesskit::Rect {
                x0: rect.min.x.into(),
                y0: rect.min.y.into(),
                x1: rect.max.x.into(),
                y1: rect.max.y.into(),
            });
        });
    }
}

fn kind_name(kind: &str) -> &str {
    if kind == "Wiki" { "wiki" } else { kind }
}

/// The view.
pub(crate) fn show(
    ui: &mut Ui,
    palette: &Palette,
    data: &JsValue,
    project: &Project,
    editable: bool,
    intents: &mut Vec<Intent>,
) {
    let cards = storyboard::cards(data);
    let drawings = storyboard::drawings(data);
    let view_id = ui.make_persistent_id(("board-view", &project.id));
    let drag_id = ui.make_persistent_id("board-source-drag");
    // ProseCraft's `boardPan` (the world point at the canvas's top-left) and `boardZoom`, kept
    // apart from the canvas's size, so resizing the window doesn't zoom.
    let (mut pan, mut zoom): (Pos2, f32) = ui
        .data(|d| d.get_temp(view_id))
        .unwrap_or((Pos2::ZERO, 1.0));
    let canvas_size_id = view_id.with("size");
    let last_size: Vec2 = ui
        .data(|d| d.get_temp(canvas_size_id))
        .unwrap_or(Vec2::new(700.0, 600.0));
    let mut reset = false;
    let center = |offset: f64| {
        storyboard::point_at_viewport_center(
            last_size.x as f64,
            last_size.y as f64,
            (pan.x as f64, pan.y as f64),
            offset,
            zoom as f64,
        )
    };
    let add_offset = ((cards.len() % 8) * 24) as f64;
    let tools_id = view_id.with("tools");
    let mut tools: Tools = ui.data(|d| d.get_temp(tools_id)).unwrap_or_default();
    if let Some(before) = tools.added
        && drawings.len() > before
    {
        tools.added = None;
        if let Some(last) = drawings.last() {
            tools.selected = Some(last.id().to_owned());
            if matches!(last, Drawing::Text { .. }) {
                ui.memory_mut(|m| m.request_focus(text_edit_id(last.id())));
            }
        }
    }
    if tools
        .selected
        .as_ref()
        .is_some_and(|id| !drawings.iter().any(|d| d.id() == id))
        && tools.added.is_none()
    {
        tools.selected = None;
    }
    // Escape drops a drawing being drawn, moved or resized.
    if tools.interaction.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        tools.interaction = None;
    }
    // Delete or Backspace removes the selected drawing (not while typing).
    if editable
        && !ui.ctx().text_edit_focused()
        && let Some(id) = tools.selected.clone()
        && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
    {
        intents.push(Intent::RemoveDrawing(id));
        tools.selected = None;
    }
    let add = |intents: &mut Vec<Intent>,
               kind: &str,
               source: &str,
               title: &str,
               note: &str,
               at: (f64, f64)| {
        intents.push(Intent::AddCard {
            kind: kind.to_owned(),
            source: source.to_owned(),
            title: title.to_owned(),
            note: note.to_owned(),
            x: at.0,
            y: at.1,
        });
    };

    // The board's toolbar (ProseCraft's, beside the view switch).
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(editable, egui::Button::new("+ Add blank card"))
            .clicked()
        {
            add(
                intents,
                "blank",
                "",
                "Untitled card",
                "",
                center(add_offset),
            );
        }
        if ui
            .button("↺ Reset view")
            .on_hover_text("Reset zoom and position")
            .clicked()
        {
            // `resetBoardView`.
            reset = true;
        }
        ui.label(
            RichText::new(format!("{}%", (zoom * 100.0).round()))
                .size(12.0)
                .color(palette.text_muted),
        );
        ui.separator();
        // The diagram tools, as a set of toggles.
        let group = ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (tool, label, icon) in Tool::ALL {
                let on = tools.tool == tool;
                let button = ui
                    .add_enabled(
                        editable || matches!(tool, Tool::Select | Tool::Pan),
                        egui::Button::selectable(on, format!("{icon} {label}")),
                    )
                    .on_hover_text(format!("{label} tool"));
                let name = format!("{label} tool");
                ui.ctx().accesskit_node_builder(button.id, move |node| {
                    node.set_label(name);
                    node.set_toggled(if on {
                        accesskit::Toggled::True
                    } else {
                        accesskit::Toggled::False
                    });
                });
                if button.clicked() {
                    tools.tool = tool;
                }
            }
        });
        ui.ctx().accesskit_node_builder(group.response.id, |node| {
            node.set_role(accesskit::Role::Toolbar);
            node.set_label("Storyboard tools");
        });
        let selected_text = tools
            .selected
            .as_ref()
            .and_then(|id| drawings.iter().find(|d| d.id() == id))
            .and_then(|d| match d {
                Drawing::Text { font_size, .. } => Some((d.clone(), *font_size)),
                _ => None,
            });
        if tools.tool == Tool::Text || selected_text.is_some() {
            // The slider doesn't wrap by itself: start a new row when it wouldn't fit.
            if ui.available_size_before_wrap().x < ui.spacing().slider_width + 160.0 {
                ui.end_row();
            }
            ui.label(
                RichText::new("Text size")
                    .size(12.0)
                    .color(palette.text_secondary),
            );
            let mut size = selected_text.as_ref().map_or(tools.font_size, |(_, s)| *s);
            let slider = ui.add_enabled(
                editable,
                egui::Slider::new(&mut size, 8.0..=72.0)
                    .step_by(1.0)
                    .suffix("px")
                    .fixed_decimals(0),
            );
            crate::views::name_field(ui, &slider, "Diagram text font size");
            if slider.changed() {
                tools.font_size = size;
            }
            // Saved once the slider is let go, not at every step.
            if (slider.drag_stopped() || (slider.changed() && !slider.dragged()))
                && let Some((
                    Drawing::Text {
                        id,
                        x,
                        y,
                        width,
                        height,
                        text,
                        ..
                    },
                    _,
                )) = selected_text
            {
                intents.push(Intent::SetDrawing(Drawing::Text {
                    id,
                    x,
                    y,
                    width,
                    height,
                    text,
                    font_size: size,
                }));
            }
        }
        let delete = ui
            .add_enabled(
                editable && tools.selected.is_some(),
                egui::Button::new("Delete"),
            )
            .on_hover_text("Delete selected element");
        crate::views::name_field(ui, &delete, "Delete selected diagram element");
        if delete.clicked()
            && let Some(id) = tools.selected.take()
        {
            intents.push(Intent::RemoveDrawing(id));
        }
    });
    ui.add_space(6.0);

    let area = ui.available_rect_before_wrap();
    let sources_rect = Rect::from_min_size(area.min, Vec2::new(240.0, area.height()));
    let canvas_rect = Rect::from_min_max(
        Pos2::new(sources_rect.max.x + 12.0, area.min.y),
        Pos2::new(area.max.x, area.max.y - 22.0),
    );
    let mut dragging: Option<SourceDrag> = ui.data(|d| d.get_temp(drag_id));

    // Story elements.
    ui.scope_builder(egui::UiBuilder::new().max_rect(sources_rect), |ui| {
        egui::ScrollArea::vertical().id_salt("board-sources").auto_shrink([false, false]).show(ui, |ui| {
            eyebrow(ui, palette, "Story elements");
            ui.label(RichText::new("Add to storyboard").font(crate::fonts::serif(18.0)).color(palette.text_heading));
            ui.add(egui::Label::new(RichText::new("Drag an element onto the canvas, or use + to place it near the center.").size(12.0).color(palette.text_muted)).wrap());
            ui.add_space(6.0);
            let mut source = |ui: &mut Ui, kind: &str, id: &str, name: &str, title: &str, note: &str| {
                ui.horizontal(|ui| {
                    let handle = ui.add_enabled(editable, egui::Button::new("⠿").frame(false).sense(egui::Sense::drag())).on_hover_text("Drag onto canvas");
                    crate::views::name_field(ui, &handle, &format!("Drag {name} onto the storyboard canvas"));
                    if handle.drag_started() {
                        dragging = Some(SourceDrag { kind: kind.to_owned(), source: id.to_owned(), title: title.to_owned(), note: note.to_owned() });
                    }
                    ui.add(egui::Label::new(name).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let plus = ui.add_enabled(editable, egui::Button::new("+").small()).on_hover_text("Add to canvas");
                        crate::views::name_field(ui, &plus, &format!("Add {name} near the canvas center"));
                        if plus.clicked() {
                            add(intents, kind, id, title, note, center(add_offset));
                        }
                    });
                });
            };
            let group = |ui: &mut Ui, name: &str| {
                ui.add_space(6.0);
                ui.label(RichText::new(name).strong().size(12.5).color(palette.text_secondary));
            };
            group(ui, "Cards");
            source(ui, "blank", "", "Blank card", "Untitled card", "");
            group(ui, "Chapters");
            let mut any = false;
            for chapter in project.live_chapters() {
                any = true;
                let label = crate::views::label_of(project, &chapter.id);
                source(ui, "chapter", &chapter.id, &label, &label, chapter.summary.as_deref().unwrap_or(""));
            }
            if !any {
                ui.label(RichText::new("No chapters yet").size(12.0).color(palette.text_muted));
            }
            group(ui, "Scenes");
            let scenes: Vec<_> = project.documents.iter().filter(|d| d.chapter_id.is_some() && !d.trashed).collect();
            for doc in &scenes {
                let name = if doc.title.is_empty() { "Untitled scene" } else { doc.title.as_str() };
                source(ui, "scene", &doc.id, name, if doc.title.is_empty() { "Untitled" } else { &doc.title }, "");
            }
            if scenes.is_empty() {
                ui.label(RichText::new("No scenes yet").size(12.0).color(palette.text_muted));
            }
            group(ui, "Wiki");
            let pages: Vec<_> = project.documents.iter().filter(|d| d.is_wiki() && !d.trashed).collect();
            for doc in &pages {
                let name = if doc.title.is_empty() { "Untitled" } else { doc.title.as_str() };
                source(ui, "Wiki", &doc.id, name, name, "");
            }
            if pages.is_empty() {
                ui.label(RichText::new("No wiki pages yet").size(12.0).color(palette.text_muted));
            }
        });
    });

    // The canvas: a dotted grid under a pannable, zoomable scene.
    if reset {
        (pan, zoom) = (Pos2::ZERO, 1.0);
    }
    let scene_area = canvas_rect.shrink(1.0);
    let mut scene_rect = Rect::from_min_size(pan, scene_area.size() / zoom);
    let painter = ui.painter_at(canvas_rect);
    painter.rect_filled(canvas_rect, 8.0, palette.page_bg);
    painter.rect_stroke(
        canvas_rect,
        8.0,
        Stroke::new(1.0, palette.border),
        egui::StrokeKind::Inside,
    );
    let step = 24.0 * zoom;
    if step >= 6.0 {
        let dot = palette.border;
        let start_x =
            canvas_rect.min.x + (8.0 * zoom - (pan.x * zoom).rem_euclid(step)).rem_euclid(step);
        let start_y =
            canvas_rect.min.y + (8.0 * zoom - (pan.y * zoom).rem_euclid(step)).rem_euclid(step);
        let mut y = start_y;
        while y < canvas_rect.max.y {
            let mut x = start_x;
            while x < canvas_rect.max.x {
                painter.circle_filled(Pos2::new(x, y), 1.0, dot);
                x += step;
            }
            y += step;
        }
    }
    let mut to_world = None;
    // The Select and Pan tools pan with a drag on the background (Space + drag and the middle
    // mouse always do); the drawing tools draw with it.
    let space = ui.input(|i| i.key_down(egui::Key::Space)) && !ui.ctx().text_edit_focused();
    let pans = matches!(tools.tool, Tool::Select | Tool::Pan) || space;
    let pan_buttons = if pans {
        egui::DragPanButtons::PRIMARY | egui::DragPanButtons::MIDDLE
    } else {
        egui::DragPanButtons::MIDDLE
    };
    let scene = ui.scope_builder(egui::UiBuilder::new().max_rect(scene_area), |ui| {
        egui::Scene::new()
            .zoom_range(MIN_ZOOM..=MAX_ZOOM)
            .drag_pan_buttons(pan_buttons)
            .show(ui, &mut scene_rect, |ui| {
                to_world = ui.ctx().layer_transform_from_global(ui.layer_id());
                drawing_layer(ui, palette, &drawings, &mut tools, editable, intents);
                for card in &cards {
                    card_ui(ui, palette, card, editable, intents, canvas_rect);
                }
            })
    });
    ui.data_mut(|d| {
        let zoom = storyboard::clamp_zoom(scene_area.width() / scene_rect.width());
        d.insert_temp(view_id, (scene_rect.min, zoom));
        d.insert_temp(canvas_size_id, scene_area.size());
    });
    let scene_ui = scene.inner.response;
    if editable && !space {
        draw_on_background(ui, &scene_ui, to_world, &mut tools, drawings.len(), intents);
    }
    ui.data_mut(|d| d.insert_temp(tools_id, tools));
    if cards.is_empty() && drawings.is_empty() {
        painter.text(
            canvas_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Add a card or choose a diagram tool to start planning on the canvas.",
            egui::FontId::proportional(13.0),
            palette.text_muted,
        );
    }
    ui.ctx().accesskit_node_builder(scene_ui.id, |node| {
        node.set_role(accesskit::Role::Group);
        node.set_label("Freeform storyboard");
        node.set_bounds(accesskit::Rect {
            x0: canvas_rect.min.x.into(),
            y0: canvas_rect.min.y.into(),
            x1: canvas_rect.max.x.into(),
            y1: canvas_rect.max.y.into(),
        });
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_max(Pos2::new(canvas_rect.min.x, canvas_rect.max.y + 4.0), area.max)), |ui| {
        ui.label(RichText::new("Draw with the toolbar · Pan tool, Space + drag, or middle mouse · Ctrl + scroll or pinch to zoom").size(11.5).color(palette.text_muted));
    });

    // A Story element being dragged: "Drop to add card" follows the pointer; dropped on the
    // canvas, it becomes a card there (ProseCraft holds it by 110, 80).
    if let Some(drag) = &dragging {
        let pointer = ui.input(|i| i.pointer.latest_pos());
        if let Some(pos) = pointer {
            egui::Area::new(drag_id.with("preview"))
                .order(egui::Order::Tooltip)
                .fixed_pos(pos - Vec2::new(110.0, 80.0) * zoom)
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(palette.surface)
                        .stroke(Stroke::new(1.0, palette.accent))
                        .corner_radius(8.0)
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Drop to add card").color(palette.text_secondary),
                            );
                        });
                });
        }
        if ui.input(|i| i.pointer.any_released()) {
            if let (Some(pos), Some(to_world)) = (pointer, to_world)
                && canvas_rect.contains(pos)
            {
                let world = to_world * pos;
                add(
                    intents,
                    &drag.kind,
                    &drag.source,
                    &drag.title,
                    &drag.note,
                    (world.x as f64 - 110.0, world.y as f64 - 80.0),
                );
            }
            dragging = None;
        }
    }
    ui.data_mut(|d| match dragging {
        Some(drag) => {
            d.insert_temp(drag_id, drag);
        }
        None => {
            d.remove_temp::<SourceDrag>(drag_id);
        }
    });
}

/// The world point under the pointer.
fn pointer_world(ui: &Ui, to_world: Option<egui::emath::TSTransform>) -> Option<Pos2> {
    Some(to_world? * ui.input(|i| i.pointer.latest_pos())?)
}

/// Drawing on the background with the Line, Arrow, Box and Text tools; with Select, a click on
/// the background clears the selection.
fn draw_on_background(
    ui: &Ui,
    background: &egui::Response,
    to_world: Option<egui::emath::TSTransform>,
    tools: &mut Tools,
    count: usize,
    intents: &mut Vec<Intent>,
) {
    let Some(to_world) = to_world else { return };
    let round = |p: Pos2| {
        (
            storyboard::js_round(p.x as f64),
            storyboard::js_round(p.y as f64),
        )
    };
    match tools.tool {
        Tool::Select => {
            if background.clicked() {
                tools.selected = None;
            }
        }
        Tool::Pan => {}
        Tool::Text => {
            if (background.clicked_by(egui::PointerButton::Primary)
                || background.drag_started_by(egui::PointerButton::Primary))
                && let Some(p) = ui
                    .input(|i| i.pointer.press_origin())
                    .or_else(|| ui.input(|i| i.pointer.latest_pos()))
            {
                let (x, y) = round(to_world * p);
                intents.push(Intent::AddDrawing(Drawing::Text {
                    id: String::new(),
                    x,
                    y,
                    width: 180.0,
                    height: 56.0,
                    text: String::new(),
                    font_size: tools.font_size,
                }));
                tools.added = Some(count);
            }
        }
        Tool::Line | Tool::Arrow | Tool::Box => {
            if background.drag_started_by(egui::PointerButton::Primary)
                && let Some(p) = ui.input(|i| i.pointer.press_origin())
            {
                let start = round(to_world * p);
                let original = if tools.tool == Tool::Box {
                    let (x, y, width, height) = storyboard::bounds_from_points(start, start, 24.0);
                    Drawing::Box {
                        id: String::new(),
                        x,
                        y,
                        width,
                        height,
                    }
                } else {
                    Drawing::Line {
                        id: String::new(),
                        arrow: tools.tool == Tool::Arrow,
                        start,
                        end: start,
                    }
                };
                tools.selected = None;
                tools.interaction = Some(Interaction {
                    mode: Mode::Create,
                    start,
                    moved: Vec2::ZERO,
                    current: original.clone(),
                    original,
                });
            }
            let Some(interaction) = tools
                .interaction
                .as_mut()
                .filter(|i| i.mode == Mode::Create)
            else {
                return;
            };
            if let Some(p) = pointer_world(ui, Some(to_world)) {
                let point = (p.x as f64, p.y as f64);
                interaction.current = match &interaction.original {
                    Drawing::Line {
                        id, arrow, start, ..
                    } => Drawing::Line {
                        id: id.clone(),
                        arrow: *arrow,
                        start: *start,
                        end: round(p),
                    },
                    _ => {
                        let (x, y, width, height) =
                            storyboard::bounds_from_points(interaction.start, point, 24.0);
                        Drawing::Box {
                            id: String::new(),
                            x,
                            y,
                            width,
                            height,
                        }
                    }
                };
            }
            if background.drag_stopped() {
                let end = pointer_world(ui, Some(to_world))
                    .map_or(interaction.start, |p| (p.x as f64, p.y as f64));
                // A press without a real drag adds nothing (ProseCraft's 3-unit threshold).
                if (end.0 - interaction.start.0).hypot(end.1 - interaction.start.1) >= 3.0 {
                    intents.push(Intent::AddDrawing(interaction.current.clone()));
                    tools.added = Some(count);
                }
                tools.interaction = None;
            }
        }
    }
}

/// The diagram drawings, under the cards: shapes, text labels, the selection and its handles.
fn drawing_layer(
    ui: &mut Ui,
    palette: &Palette,
    drawings: &[Drawing],
    tools: &mut Tools,
    editable: bool,
    intents: &mut Vec<Intent>,
) {
    let to_world = ui.ctx().layer_transform_from_global(ui.layer_id());
    let pointer = pointer_world(ui, to_world);
    let mut shown: Vec<Drawing> = drawings.to_vec();
    if let Some(interaction) = &tools.interaction {
        match interaction.mode {
            Mode::Create => shown.push(interaction.current.clone()),
            _ => {
                if let Some(d) = shown
                    .iter_mut()
                    .find(|d| d.id() == interaction.current.id())
                {
                    *d = interaction.current.clone();
                }
            }
        }
    }
    let select = tools.tool == Tool::Select && editable;
    for drawing in &shown {
        let selected = tools.selected.as_deref() == Some(drawing.id()) && !drawing.id().is_empty();
        let rect = drawing_rect(drawing);
        let stroke_color = if selected {
            palette.selected_text
        } else {
            palette.accent
        };
        match drawing {
            Drawing::Line {
                arrow, start, end, ..
            } => {
                let (a, b) = (
                    Pos2::new(start.0 as f32, start.1 as f32),
                    Pos2::new(end.0 as f32, end.1 as f32),
                );
                ui.painter()
                    .line_segment([a, b], Stroke::new(2.5, stroke_color));
                if *arrow && a != b {
                    let dir = (b - a).normalized();
                    let side = Vec2::new(-dir.y, dir.x);
                    let head = 12.0;
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![
                            b,
                            b - dir * head + side * head * 0.5,
                            b - dir * head - side * head * 0.5,
                        ],
                        stroke_color,
                        Stroke::NONE,
                    ));
                }
            }
            Drawing::Box { .. } => {
                ui.painter()
                    .rect_filled(rect, 0.0, palette.selected_bg.gamma_multiply(0.18));
                ui.painter().rect_stroke(
                    rect,
                    0.0,
                    Stroke::new(2.0, stroke_color),
                    egui::StrokeKind::Middle,
                );
                if selected {
                    ui.painter().rect_stroke(
                        rect.expand(3.0),
                        0.0,
                        Stroke::new(1.0, palette.accent),
                        egui::StrokeKind::Middle,
                    );
                }
            }
            Drawing::Text { .. } => {}
        }
        if drawing.id().is_empty() {
            continue; // being drawn
        }
        if let Drawing::Text {
            id,
            text,
            font_size,
            ..
        } = drawing
        {
            text_drawing(
                ui, palette, drawing, id, text, *font_size, selected, tools, editable, intents,
            );
            continue;
        }
        // Shapes are focusable buttons (Select moves them; arrow keys nudge by 12).
        let near = pointer.is_some_and(|p| match drawing {
            Drawing::Line { start, end, .. } => {
                distance_to_segment(
                    p,
                    Pos2::new(start.0 as f32, start.1 as f32),
                    Pos2::new(end.0 as f32, end.1 as f32),
                ) <= 7.0
            }
            _ => rect.contains(p),
        });
        let sense = if select && near {
            egui::Sense::click_and_drag() | egui::Sense::FOCUSABLE
        } else {
            egui::Sense::FOCUSABLE
        };
        let response = ui.interact(
            rect.expand(7.0),
            egui::Id::unique(("board-drawing", drawing.id())),
            sense,
        );
        let name = format!("Select {} diagram element", type_name(drawing));
        ui.ctx().accesskit_node_builder(response.id, move |node| {
            node.set_role(accesskit::Role::Button);
            node.set_label(name);
            node.set_toggled(if selected {
                accesskit::Toggled::True
            } else {
                accesskit::Toggled::False
            });
        });
        screen_bounds(ui, &response);
        if response.clicked() || response.gained_focus() || response.drag_started() {
            tools.selected = Some(drawing.id().to_owned());
        }
        drag_drawing(&response, drawing, Mode::Move, tools, intents);
        if response.has_focus() && editable {
            nudge_drawing(ui, &response, drawing, tools, intents);
        }
        if selected && matches!(drawing, Drawing::Box { .. }) && editable {
            resize_handle(ui, palette, drawing, "Resize box", tools, intents);
        }
    }
}

/// Moving or resizing a drawing by dragging `response`.
fn drag_drawing(
    response: &egui::Response,
    drawing: &Drawing,
    mode: Mode,
    tools: &mut Tools,
    intents: &mut Vec<Intent>,
) {
    if response.drag_started() {
        tools.selected = Some(drawing.id().to_owned());
        tools.interaction = Some(Interaction {
            mode,
            start: (0.0, 0.0),
            moved: Vec2::ZERO,
            original: drawing.clone(),
            current: drawing.clone(),
        });
    }
    let Some(interaction) = tools
        .interaction
        .as_mut()
        .filter(|i| i.mode == mode && i.original.id() == drawing.id())
    else {
        return;
    };
    if response.dragged() {
        interaction.moved += response.drag_delta();
        let (dx, dy) = (interaction.moved.x as f64, interaction.moved.y as f64);
        interaction.current = match mode {
            Mode::Resize => {
                let (w, h) = if matches!(interaction.original, Drawing::Text { .. }) {
                    TEXT_MIN
                } else {
                    BOX_MIN
                };
                storyboard::resize_drawing(&interaction.original, dx, dy, w, h)
            }
            _ => storyboard::move_drawing(&interaction.original, dx, dy),
        };
    }
    if response.drag_stopped() {
        if interaction.current != interaction.original {
            intents.push(Intent::SetDrawing(interaction.current.clone()));
        }
        tools.interaction = None;
    }
}

fn nudge_drawing(
    ui: &Ui,
    response: &egui::Response,
    drawing: &Drawing,
    tools: &mut Tools,
    intents: &mut Vec<Intent>,
) {
    ui.memory_mut(|m| {
        m.set_focus_lock_filter(
            response.id,
            egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..Default::default()
            },
        )
    });
    let delta = ui.input(|i| {
        [
            (egui::Key::ArrowLeft, (-DRAWING_NUDGE, 0.0)),
            (egui::Key::ArrowRight, (DRAWING_NUDGE, 0.0)),
            (egui::Key::ArrowUp, (0.0, -DRAWING_NUDGE)),
            (egui::Key::ArrowDown, (0.0, DRAWING_NUDGE)),
        ]
        .into_iter()
        .find(|(k, _)| i.key_pressed(*k))
        .map(|(_, d)| d)
    });
    if let Some((dx, dy)) = delta {
        tools.selected = Some(drawing.id().to_owned());
        intents.push(Intent::SetDrawing(storyboard::move_drawing(
            drawing, dx, dy,
        )));
    }
}

/// The accent square at a selected box's or label's lower-right corner.
fn resize_handle(
    ui: &mut Ui,
    palette: &Palette,
    drawing: &Drawing,
    name: &str,
    tools: &mut Tools,
    intents: &mut Vec<Intent>,
) {
    let rect = drawing_rect(drawing);
    let handle = Rect::from_center_size(rect.right_bottom(), Vec2::splat(11.0));
    let response = ui.interact(
        handle,
        egui::Id::unique(("board-resize", drawing.id())),
        egui::Sense::click_and_drag(),
    );
    ui.painter().rect_filled(handle, 2.0, palette.accent);
    ui.painter().rect_stroke(
        handle,
        2.0,
        Stroke::new(1.0, palette.surface),
        egui::StrokeKind::Inside,
    );
    crate::views::name_field(ui, &response, name);
    screen_bounds(ui, &response);
    drag_drawing(
        &response.on_hover_cursor(egui::CursorIcon::ResizeNwSe),
        drawing,
        Mode::Resize,
        tools,
        intents,
    );
}

/// A text label: its text, edited in place (left empty, it goes), and when selected its move
/// and resize handles.
#[allow(clippy::too_many_arguments)]
fn text_drawing(
    ui: &mut Ui,
    palette: &Palette,
    drawing: &Drawing,
    id: &str,
    text: &str,
    font_size: f64,
    selected: bool,
    tools: &mut Tools,
    editable: bool,
    intents: &mut Vec<Intent>,
) {
    let rect = drawing_rect(drawing);
    ui.painter().rect_filled(rect, 0.0, palette.surface);
    let border = if selected {
        palette.accent
    } else {
        palette.border_control
    };
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    for pair in corners.windows(2) {
        ui.painter().add(egui::Shape::dashed_line(
            &[pair[0], pair[1]],
            Stroke::new(1.0, border),
            4.0,
            3.0,
        ));
    }
    let edit_id = text_edit_id(id);
    let draft_id = edit_id.with("draft");
    let mut draft = ui
        .data(|d| d.get_temp::<String>(draft_id))
        .unwrap_or_else(|| text.to_owned());
    let response = ui
        .scope_builder(egui::UiBuilder::new().max_rect(rect.shrink(4.0)), |ui| {
            ui.add_enabled(
                editable,
                egui::TextEdit::multiline(&mut draft)
                    .id(edit_id)
                    .frame(egui::Frame::NONE)
                    .hint_text("Type a label…")
                    .char_limit(2000)
                    .font(egui::FontId::proportional(font_size as f32))
                    .desired_width(rect.width() - 8.0)
                    .desired_rows(1),
            )
        })
        .inner;
    crate::views::name_field(ui, &response, "Diagram text");
    screen_bounds(ui, &response);
    if response.changed() {
        ui.data_mut(|d| d.insert_temp(draft_id, draft.clone()));
    }
    if response.gained_focus() || response.clicked() {
        tools.selected = Some(id.to_owned());
    }
    if response.lost_focus() {
        ui.data_mut(|d| d.remove_temp::<String>(draft_id));
        // `finishStoryboardTextEdit`: a label left empty is removed.
        if draft.trim().is_empty() {
            intents.push(Intent::RemoveDrawing(id.to_owned()));
            if tools.selected.as_deref() == Some(id) {
                tools.selected = None;
            }
        } else if draft != text
            && let Drawing::Text {
                x,
                y,
                width,
                height,
                font_size,
                ..
            } = drawing
        {
            intents.push(Intent::SetDrawing(Drawing::Text {
                id: id.to_owned(),
                x: *x,
                y: *y,
                width: *width,
                height: *height,
                text: draft,
                font_size: *font_size,
            }));
        }
    }
    if selected && editable {
        let handle_rect = Rect::from_min_size(
            Pos2::new(rect.max.x - 34.0, rect.min.y - 12.0),
            Vec2::new(22.0, 20.0),
        );
        let handle = ui.interact(
            handle_rect,
            egui::Id::unique(("board-move", id)),
            egui::Sense::click_and_drag(),
        );
        ui.painter().rect_filled(handle_rect, 4.0, palette.surface);
        ui.painter().rect_stroke(
            handle_rect,
            4.0,
            Stroke::new(1.0, palette.border_control),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            handle_rect.center(),
            egui::Align2::CENTER_CENTER,
            "⠿",
            egui::FontId::proportional(13.0),
            palette.text_muted,
        );
        crate::views::name_field(ui, &handle, "Move text label");
        screen_bounds(ui, &handle);
        drag_drawing(
            &handle.on_hover_cursor(egui::CursorIcon::Grab),
            drawing,
            Mode::Move,
            tools,
            intents,
        );
        resize_handle(ui, palette, drawing, "Resize text area", tools, intents);
    }
}

/// One card: the move handle (drag it, or arrow keys), its kind, remove, the title and the note.
fn card_ui(
    ui: &mut Ui,
    palette: &Palette,
    card: &Card,
    editable: bool,
    intents: &mut Vec<Intent>,
    canvas: Rect,
) {
    let offset_id = ui.make_persistent_id(("card-drag", &card.id));
    let offset: Vec2 = ui.data(|d| d.get_temp(offset_id)).unwrap_or_default();
    let at = Pos2::new(card.x as f32, card.y as f32) + offset;
    let blank = card.kind == "blank";
    let name = if card.title.is_empty() {
        "card"
    } else {
        card.title.as_str()
    };
    let frame = egui::Frame::new()
        .fill(palette.surface)
        .stroke(if blank {
            Stroke::NONE
        } else {
            Stroke::new(
                1.0,
                if offset != Vec2::ZERO {
                    palette.accent
                } else {
                    palette.border
                },
            )
        })
        .corner_radius(8.0)
        .inner_margin(11.0);
    let shown = ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(at, Vec2::new(CARD_WIDTH, 2000.0)))
            .id_salt(("card", &card.id)),
        |ui| {
            frame.show(ui, |ui| {
                ui.set_width(CARD_WIDTH - 22.0);
                ui.horizontal(|ui| {
                    let handle = ui
                        .add_enabled(
                            editable,
                            egui::Button::new(RichText::new("⠿").color(palette.text_muted))
                                .frame(false)
                                .sense(egui::Sense::click_and_drag()),
                        )
                        .on_hover_text("Drag to move; arrow keys to nudge");
                    crate::views::name_field(
                        ui,
                        &handle,
                        &format!(
                            "Move {name}. Use arrow keys to nudge the card, or drag to position it."
                        ),
                    );
                    screen_bounds(ui, &handle);
                    if handle.dragged() {
                        let next = offset + handle.drag_delta();
                        ui.data_mut(|d| d.insert_temp(offset_id, next));
                    }
                    if handle.drag_stopped() {
                        ui.data_mut(|d| d.remove_temp::<Vec2>(offset_id));
                        // Dropped outside the canvas, the card stays where it was.
                        let inside = ui
                            .input(|i| i.pointer.latest_pos())
                            .is_some_and(|p| canvas.contains(p));
                        if inside && offset != Vec2::ZERO {
                            intents.push(Intent::MoveCard {
                                id: card.id.clone(),
                                x: at.x as f64,
                                y: at.y as f64,
                            });
                        }
                    }
                    if handle.has_focus() {
                        ui.memory_mut(|m| {
                            m.set_focus_lock_filter(
                                handle.id,
                                egui::EventFilter {
                                    horizontal_arrows: true,
                                    vertical_arrows: true,
                                    ..Default::default()
                                },
                            )
                        });
                        let delta = ui.input(|i| {
                            [
                                (egui::Key::ArrowLeft, (-NUDGE, 0.0)),
                                (egui::Key::ArrowRight, (NUDGE, 0.0)),
                                (egui::Key::ArrowUp, (0.0, -NUDGE)),
                                (egui::Key::ArrowDown, (0.0, NUDGE)),
                            ]
                            .into_iter()
                            .find(|(k, _)| i.key_pressed(*k))
                            .map(|(_, d)| d)
                        });
                        if let Some((dx, dy)) = delta {
                            intents.push(Intent::MoveCard {
                                id: card.id.clone(),
                                x: card.x + dx,
                                y: card.y + dy,
                            });
                        }
                    }
                    ui.label(
                        RichText::new(kind_name(&card.kind))
                            .size(11.0)
                            .color(palette.eyebrow),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let remove = ui
                            .add_enabled(
                                editable,
                                egui::Button::new(RichText::new("×").color(palette.text_muted))
                                    .frame(false),
                            )
                            .on_hover_text("Remove from storyboard");
                        crate::views::name_field(
                            ui,
                            &remove,
                            &format!("Remove {name} from storyboard"),
                        );
                        screen_bounds(ui, &remove);
                        if remove.clicked() {
                            intents.push(Intent::RemoveCard(card.id.clone()));
                        }
                    });
                });
                ui.add_space(4.0);
                let (title, title_response) = crate::views::saved_field_response(
                    ui,
                    ("card-title", &card.id),
                    &card.title,
                    "",
                    "Storyboard card title",
                    false,
                    storyboard::TITLE_LIMIT,
                    editable,
                );
                screen_bounds(ui, &title_response);
                if let Some(title) = title {
                    intents.push(Intent::SetCardTitle {
                        id: card.id.clone(),
                        title,
                    });
                }
                let chapter = card.kind == "chapter";
                let (hint, label) = if chapter {
                    (
                        "Chapter summary — synced with Outline",
                        format!("Chapter summary for {}", card.title),
                    )
                } else {
                    (
                        "Write a note about this story element…",
                        format!("Planning notes for {}", card.title),
                    )
                };
                let limit = if chapter {
                    storyboard::SUMMARY_LIMIT
                } else {
                    usize::MAX
                };
                let (note, note_response) = crate::views::saved_field_response(
                    ui,
                    ("card-note", &card.id),
                    &card.note,
                    hint,
                    &label,
                    true,
                    limit,
                    editable,
                );
                screen_bounds(ui, &note_response);
                if let Some(note) = note {
                    intents.push(Intent::SetCardNote {
                        id: card.id.clone(),
                        note,
                    });
                }
            })
        },
    );
    let rect = shown.inner.response.rect;
    if blank {
        // ProseCraft's blank cards have a dashed border.
        let color = if offset != Vec2::ZERO {
            palette.accent
        } else {
            palette.border
        };
        let r = rect.shrink(0.5);
        let corners = [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            r.left_top(),
        ];
        for pair in corners.windows(2) {
            ui.painter().add(egui::Shape::dashed_line(
                &[pair[0], pair[1]],
                Stroke::new(1.0, color),
                5.0,
                4.0,
            ));
        }
    }
}
