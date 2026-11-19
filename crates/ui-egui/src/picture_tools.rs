//! The selected picture's tools (ProseCraft's `image-edit-toolbar`): pixel sizes, the column
//! width with a custom percentage, alignment, text flow and removal, and the resize handle on
//! the picture itself.

use egui::{RichText, Ui};
use prosecraft_core::html::image_layout::{self, ImageLayout};

use crate::editing::Draft;
use crate::theme::Palette;

/// `IMAGE_SIZE_OPTIONS`: size, label, accessible name.
const SIZES: [(&str, &str, &str); 4] = [
    ("small", "Small", "Small image size"),
    ("medium", "Medium", "Medium image size"),
    ("large", "Large", "Large image size"),
    ("full", "Full", "Full width"),
];
/// `IMAGE_ALIGN_OPTIONS`.
const ALIGNS: [(&str, &str); 3] = [("left", "Left"), ("center", "Center"), ("right", "Right")];

fn key(document: &str) -> egui::Id {
    egui::Id::unique(("selected-picture", document))
}

/// The selected picture of the open document: its atom's index.
pub(crate) fn selected(ctx: &egui::Context, document: &str) -> Option<usize> {
    ctx.data(|d| d.get_temp::<usize>(key(document)))
}

/// `selectEditorImage`.
pub(crate) fn select(ctx: &egui::Context, document: &str, atom: Option<usize>) {
    ctx.data_mut(|d| match atom {
        Some(atom) => {
            d.insert_temp(key(document), atom);
        }
        None => d.remove::<usize>(key(document)),
    });
}

/// Whether the custom width field is open (`customImageWidthEditing`) and its draft.
#[derive(Clone, Default)]
struct Custom {
    editing: bool,
    draft: Option<String>,
}

fn pressed(ui: &mut Ui, on: bool, label: &str, name: &str) -> bool {
    let button = ui.selectable_label(on, label);
    let name = name.to_owned();
    ui.ctx().accesskit_node_builder(button.id, move |node| {
        node.set_label(name);
        node.set_toggled(if on {
            egui::accesskit::Toggled::True
        } else {
            egui::accesskit::Toggled::False
        });
    });
    button.clicked()
}

fn group_label(ui: &mut Ui, palette: &Palette, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(palette.text_muted));
}

/// A picture's changed markup put back in the draft.
fn replace(draft: &mut Draft, atom: usize, html: &str, now: f64) {
    draft.body = draft.doc.replace_atom(atom, html);
    draft.body_dirty = true;
    draft.changed_at = now;
}

/// The toolbar, when a picture is selected. `column` is the text column's width.
pub(crate) fn toolbar(
    ui: &mut Ui,
    palette: &Palette,
    document: &str,
    draft: &mut Draft,
    column: f32,
    now: f64,
) {
    let Some(atom) = selected(ui.ctx(), document) else {
        return;
    };
    let Some(html) = draft.doc.atoms.get(atom).cloned() else {
        select(ui.ctx(), document, None);
        return;
    };
    let Some(picture) = crate::pictures::info(&html) else {
        select(ui.ctx(), document, None);
        return;
    };
    let layout = picture.layout;
    let custom_id = egui::Id::unique(("picture-custom-width", document));
    let mut custom = ui
        .ctx()
        .data(|d| d.get_temp::<Custom>(custom_id))
        .unwrap_or_default();
    let mut next: Option<ImageLayout> = None;
    let mut pixels: Option<Option<(u32, u32)>> = None;
    let mut remove = false;
    let frame = egui::Frame::new()
        .fill(palette.control_bg)
        .stroke(egui::Stroke::new(1.0, palette.border))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                group_label(ui, palette, "Image size");
                // `setSelectedImageWidth`: pixels from the column, the height from the picture.
                for (label, percent) in [
                    ("50%", Some(50.0)),
                    ("75%", Some(75.0)),
                    ("Fit width", Some(100.0)),
                    ("Original size", None),
                ] {
                    if ui.button(label).clicked() {
                        pixels = Some(percent.map(|p: f32| {
                            let width = (column * p / 100.0).round().max(1.0);
                            let ratio = picture.size.1 as f32 / picture.size.0.max(1) as f32;
                            (width as u32, (width * ratio).round() as u32)
                        }));
                        custom = Custom::default();
                    }
                }
                ui.separator();
                group_label(ui, palette, "Column width");
                for (size, label, name) in SIZES {
                    if pressed(ui, layout.size == size, label, name) {
                        custom = Custom::default();
                        next = Some(ImageLayout {
                            size: image_layout::size_static(size),
                            width: None,
                            ..layout
                        });
                    }
                }
                if pressed(ui, layout.size == "custom", "Custom", "Custom") {
                    // `useCustomImageWidth`: the width it has now.
                    custom = Custom {
                        editing: true,
                        draft: None,
                    };
                    let now_percent = match layout.width {
                        Some(w) if layout.size == "custom" => w,
                        _ => image_layout::clamp_width(f64::from(
                            picture.drawn(column).x / column.max(1.0) * 100.0,
                        )),
                    };
                    next = Some(ImageLayout {
                        size: "custom",
                        width: Some(now_percent),
                        ..layout
                    });
                }
                if layout.size == "custom" || custom.editing {
                    let mut text = custom
                        .draft
                        .clone()
                        .unwrap_or_else(|| layout.width.unwrap_or(100).to_string());
                    let field = ui.add(egui::TextEdit::singleline(&mut text).desired_width(40.0));
                    crate::views::name_field(ui, &field, "Custom image width in percent");
                    ui.label("%");
                    if field.changed() {
                        if let Ok(width) = text.trim().parse::<f64>()
                            && width.is_finite()
                        {
                            next = Some(ImageLayout {
                                size: "custom",
                                width: Some(image_layout::clamp_width(width)),
                                ..layout
                            });
                        }
                        custom.draft = Some(text);
                    }
                    if field.lost_focus() {
                        custom.draft = None;
                    }
                }
                ui.separator();
                group_label(ui, palette, "Align");
                for (align, label) in ALIGNS {
                    if pressed(
                        ui,
                        layout.align == align,
                        label,
                        &format!("Align image {}", label.to_lowercase()),
                    ) {
                        next = Some(ImageLayout {
                            align: image_layout::align_static(align),
                            ..layout
                        });
                    }
                }
                ui.separator();
                group_label(ui, palette, "Text flow");
                // `setSelectedImageWrap`: wrapping aligns the picture to its side.
                for (wrap, label, name, align) in [
                    ("block", "Own line", "Image on its own line", None),
                    (
                        "wrap-left",
                        "Wrap left",
                        "Wrap text beside left-aligned image",
                        Some("left"),
                    ),
                    (
                        "wrap-right",
                        "Wrap right",
                        "Wrap text beside right-aligned image",
                        Some("right"),
                    ),
                ] {
                    if pressed(ui, layout.wrap == wrap, label, name) {
                        next = Some(ImageLayout {
                            wrap: image_layout::wrap_static(wrap),
                            align: align.map_or(layout.align, image_layout::align_static),
                            ..layout
                        });
                    }
                }
                ui.separator();
                if ui
                    .button(RichText::new("Remove image").color(palette.danger))
                    .clicked()
                {
                    remove = true;
                }
            });
        });
    ui.ctx().accesskit_node_builder(frame.response.id, |node| {
        node.set_role(egui::accesskit::Role::Toolbar);
        node.set_label("Selected image tools");
    });
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        select(ui.ctx(), document, None);
    }
    if remove {
        let (_, at) = draft.doc.remove_atom(atom);
        draft.body = draft.doc.text.clone();
        draft.body_dirty = true;
        draft.changed_at = now;
        draft.caret = at;
        select(ui.ctx(), document, None);
        custom = Custom::default();
    } else if let Some(size) = pixels {
        replace(draft, atom, &image_layout::set_pixel_size(&html, size), now);
    } else if let Some(layout) = next {
        replace(
            draft,
            atom,
            &image_layout::apply_to_html(&html, &layout),
            now,
        );
    }
    ui.ctx().data_mut(|d| d.insert_temp(custom_id, custom));
}

/// The selected picture's outline and its resize handle (`startImageResize`,
/// `moveImageResize`): dragging shows the new width, which is kept when the drag ends.
pub(crate) fn handle(
    ui: &mut Ui,
    palette: &Palette,
    document: &str,
    draft: &mut Draft,
    picture_rect: egui::Rect,
    column: f32,
    now: f64,
) {
    let Some(atom) = selected(ui.ctx(), document) else {
        return;
    };
    let Some(html) = draft.doc.atoms.get(atom).cloned() else {
        return;
    };
    let painter = ui.painter();
    let corner = egui::Rect::from_center_size(picture_rect.right_bottom(), egui::vec2(12.0, 12.0));
    let response = ui.interact(
        corner,
        egui::Id::unique(("picture-resize", document)),
        egui::Sense::drag(),
    );
    crate::views::name_field(ui, &response, "Resize image");
    let start_id = egui::Id::unique(("picture-resize-start", document));
    let width_now = picture_rect.width() / column.max(1.0) * 100.0;
    let mut outline = picture_rect;
    if response.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(start_id, width_now));
    }
    let start = ui.ctx().data(|d| d.get_temp::<f32>(start_id));
    let dragged = response.total_drag_delta().map_or(0.0, |d| d.x);
    let percent =
        start.map(|s| image_layout::clamp_width(f64::from(s + dragged / column.max(1.0) * 100.0)));
    // The width as last dragged: on the frame the drag stops, it has no delta any more.
    let last_id = start_id.with("last");
    if response.dragged()
        && let Some(percent) = percent
    {
        ui.ctx().data_mut(|d| d.insert_temp(last_id, percent));
        let height = picture_rect.height() * (percent as f32 * column / 100.0)
            / picture_rect.width().max(1.0);
        outline = egui::Rect::from_min_size(
            picture_rect.min,
            egui::vec2(percent as f32 * column / 100.0, height),
        );
    }
    if response.drag_stopped() {
        let percent = ui.ctx().data(|d| d.get_temp::<u32>(last_id)).or(percent);
        if let Some(percent) = percent
            && let Some(picture) = crate::pictures::info(&html)
        {
            let layout = ImageLayout {
                size: "custom",
                width: Some(percent),
                ..picture.layout
            };
            replace(
                draft,
                atom,
                &image_layout::apply_to_html(&html, &layout),
                now,
            );
        }
        ui.ctx().data_mut(|d| {
            d.remove::<f32>(start_id);
            d.remove::<u32>(last_id);
        });
    }
    painter.rect_stroke(
        outline.expand(3.0),
        2.0,
        egui::Stroke::new(2.0, palette.accent),
        egui::StrokeKind::Outside,
    );
    let corner = egui::Rect::from_center_size(
        outline.right_bottom() + egui::vec2(3.0, 3.0),
        egui::vec2(10.0, 10.0),
    );
    painter.rect_filled(corner, 2.0, palette.accent);
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
}
