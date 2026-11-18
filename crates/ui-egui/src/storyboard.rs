//! ProseCraft's Storyboard: the Story elements to add on the left, and a freeform canvas of
//! cards (with the diagram drawings) that pans and zooms. Drawing tools come later.

use egui::{Pos2, Rect, RichText, Stroke, Ui, Vec2, accesskit};
use prosecraft_core::js::JsValue;
use prosecraft_core::project::Project;
use prosecraft_core::storyboard::{self, CARD_WIDTH, Card, Drawing, MAX_ZOOM, MIN_ZOOM, NUDGE};

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
    let mut scene_rect: Rect = ui.data(|d| d.get_temp(view_id)).unwrap_or(Rect::NOTHING);
    let canvas_size_id = view_id.with("size");
    let last_size: Vec2 = ui
        .data(|d| d.get_temp(canvas_size_id))
        .unwrap_or(Vec2::new(700.0, 600.0));
    let zoom = if scene_rect.is_positive() {
        last_size.x / scene_rect.width()
    } else {
        1.0
    };
    let pan = if scene_rect.is_positive() {
        scene_rect.min
    } else {
        Pos2::ZERO
    };
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
    ui.horizontal(|ui| {
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
            scene_rect = Rect::NOTHING;
        }
        ui.label(
            RichText::new(format!("{}%", (zoom * 100.0).round()))
                .size(12.0)
                .color(palette.text_muted),
        );
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
    if !scene_rect.is_positive() {
        scene_rect = Rect::from_min_size(Pos2::ZERO, canvas_rect.size());
    }
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
    let scene = ui.scope_builder(
        egui::UiBuilder::new().max_rect(canvas_rect.shrink(1.0)),
        |ui| {
            egui::Scene::new()
                .zoom_range(MIN_ZOOM..=MAX_ZOOM)
                .show(ui, &mut scene_rect, |ui| {
                    to_world = ui.ctx().layer_transform_from_global(ui.layer_id());
                    paint_drawings(ui, palette, &drawings);
                    for card in &cards {
                        card_ui(ui, palette, card, editable, intents, canvas_rect);
                    }
                })
        },
    );
    ui.data_mut(|d| {
        d.insert_temp(view_id, scene_rect);
        d.insert_temp(canvas_size_id, canvas_rect.size());
    });
    let scene_ui = scene.response;
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
        ui.label(RichText::new("Drag the background or use the middle mouse to pan · Ctrl + scroll or pinch to zoom · Arrow keys nudge a focused card").size(11.5).color(palette.text_muted));
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

/// The diagram drawings, under the cards.
fn paint_drawings(ui: &Ui, palette: &Palette, drawings: &[Drawing]) {
    let painter = ui.painter();
    let stroke = Stroke::new(2.0, palette.text_secondary);
    for drawing in drawings {
        match drawing {
            Drawing::Line {
                arrow, start, end, ..
            } => {
                let (a, b) = (
                    Pos2::new(start.0 as f32, start.1 as f32),
                    Pos2::new(end.0 as f32, end.1 as f32),
                );
                painter.line_segment([a, b], stroke);
                if *arrow && a != b {
                    let dir = (b - a).normalized();
                    let side = Vec2::new(-dir.y, dir.x);
                    let head = 12.0;
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            b,
                            b - dir * head + side * head * 0.5,
                            b - dir * head - side * head * 0.5,
                        ],
                        palette.text_secondary,
                        Stroke::NONE,
                    ));
                }
            }
            Drawing::Box {
                x,
                y,
                width,
                height,
                ..
            } => {
                let rect = Rect::from_min_size(
                    Pos2::new(*x as f32, *y as f32),
                    Vec2::new(*width as f32, *height as f32),
                );
                painter.rect_stroke(rect, 6.0, stroke, egui::StrokeKind::Inside);
            }
            Drawing::Text {
                x,
                y,
                width,
                text,
                font_size,
                ..
            } => {
                let galley = painter.layout(
                    text.clone(),
                    egui::FontId::proportional(*font_size as f32),
                    palette.text_body,
                    *width as f32,
                );
                painter.galley(Pos2::new(*x as f32, *y as f32), galley, palette.text_body);
            }
        }
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
