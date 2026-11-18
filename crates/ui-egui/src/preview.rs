//! The Publish view's live preview (`publish-preview` in ProseCraft's `PublishView.svelte`):
//! the document navigator and the chosen part of the book.
//!
//! ProseCraft renders the part as HTML in a frame and, for PDF and DOCX, cuts it into pages
//! in the browser (`paginatePreview`). Here the part is drawn from the PDF layout the export
//! uses ([`prosecraft_core::publish::pdf::layout_pdf`]), so the pages are the exported PDF's,
//! in the app's fonts: Source Serif 4 for Times, Inter for Helvetica, egui's monospace for
//! Courier.

use std::sync::Arc;

use egui::{Color32, FontFamily, FontId, RichText, Stroke, Ui};
use prosecraft_core::publish::pdf::{Layout, Op, layout_pdf};
use prosecraft_core::publish::pdf_text::Family;
use prosecraft_core::publish::pdf_writer::DataUrlSizes;
use prosecraft_core::publish::sections::{self, Group, Section};
use prosecraft_core::publish::{CompiledBook, PublishOptions};

use crate::theme::Palette;
use crate::widgets::eyebrow;

/// The book laid out, kept while the book and options stay the same.
#[derive(Clone)]
struct Cached {
    book: Arc<CompiledBook>,
    options: Arc<PublishOptions>,
    layout: Arc<Layout>,
}

fn layout(ctx: &egui::Context, book: &CompiledBook, options: &PublishOptions) -> Arc<Layout> {
    let id = egui::Id::unique("publish-preview-layout");
    if let Some(cached) = ctx.data(|d| d.get_temp::<Cached>(id))
        && *cached.book == *book
        && *cached.options == *options
    {
        return cached.layout;
    }
    let layout = Arc::new(layout_pdf(book, options, &DataUrlSizes::default()));
    ctx.data_mut(|d| {
        d.insert_temp(
            id,
            Cached {
                book: Arc::new(book.clone()),
                options: Arc::new(options.clone()),
                layout: layout.clone(),
            },
        );
    });
    layout
}

fn section_id() -> egui::Id {
    egui::Id::unique("publish-preview-section")
}

/// The font a PDF face is shown in.
fn font(face: prosecraft_core::publish::pdf_text::Face, size: f32) -> FontId {
    let named = |name: &str| FontFamily::Name(name.into());
    let family = match face.family {
        Family::Times if face.bold => named(crate::fonts::SERIF_SEMIBOLD),
        Family::Times if face.italic => named(crate::fonts::SERIF_ITALIC),
        Family::Times => named(crate::fonts::SERIF),
        Family::Helvetica if face.bold => named(crate::fonts::SEMIBOLD),
        Family::Helvetica => FontFamily::Proportional,
        Family::Courier => FontFamily::Monospace,
    };
    FontId::new(size, family)
}

/// How much smaller the app's face is drawn than the PDF face it stands for, so text takes
/// about the PDF's width: their widths of a pangram, at most 1.
fn fit_factor(ctx: &egui::Context, face: prosecraft_core::publish::pdf_text::Face) -> f32 {
    const SAMPLE: &str = "The quick brown fox jumps over the lazy dog, 1234567890.";
    let id = egui::Id::unique(("preview-fit", face.postscript_name()));
    if let Some(found) = ctx.data(|d| d.get_temp::<f32>(id)) {
        return found;
    }
    let ours = ctx.fonts_mut(|f| {
        f.layout_no_wrap(SAMPLE.to_owned(), font(face, 100.0), Color32::WHITE)
            .size()
            .x
    });
    let pdf = face.text_width(SAMPLE, 100.0) as f32;
    let factor = if ours > 0.0 {
        (pdf / ours).min(1.0)
    } else {
        1.0
    };
    ctx.data_mut(|d| d.insert_temp(id, factor));
    factor
}

/// One page, `width` points wide on screen.
fn page(ui: &mut Ui, layout: &Layout, index: usize, width: f32) {
    let scale = width / layout.width as f32;
    let size = egui::vec2(width, layout.height as f32 * scale);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect.expand(6.0));
    painter.rect_filled(
        rect.translate(egui::vec2(0.0, 3.0)),
        2.0,
        Color32::from_black_alpha(40),
    );
    painter.rect_filled(rect, 2.0, Color32::WHITE);
    let at = |x: f64, y: f64| rect.min + egui::vec2(x as f32 * scale, y as f32 * scale);
    let rgb = |c: [u8; 3]| Color32::from_rgb(c[0], c[1], c[2]);
    let Some(page) = layout.pages.get(index) else {
        return;
    };
    for op in &page.ops {
        match op {
            Op::Text {
                x,
                y,
                face,
                size,
                color,
                text,
            } => {
                // The app's faces are wider than the PDF's: a word drawn wider than the PDF
                // sets it is shrunk to its width, so it keeps its place and the spaces.
                // One factor per face first (the app's face against the PDF's on a sentence),
                // so a line keeps one size; a word still wider by more than a
                // space is shrunk on its own.
                let px = (*size as f32 * scale * fit_factor(ui.ctx(), *face)).max(1.0);
                let pdf_width = face.text_width(text, *size) as f32 * scale;
                let galley = painter.layout_no_wrap(text.clone(), font(*face, px), rgb(*color));
                let galley = if galley.size().x > pdf_width + px * 0.2 && galley.size().x > 0.0 {
                    let fitted = (px * pdf_width / galley.size().x).max(1.0);
                    painter.layout_no_wrap(text.clone(), font(*face, fitted), rgb(*color))
                } else {
                    galley
                };
                // The baseline: about a fifth of the line above the text's bottom.
                let height = galley.size().y;
                let pos = at(*x, *y) - egui::vec2(0.0, height * 0.78);
                painter.galley(pos, galley, rgb(*color));
            }
            Op::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                width,
            } => {
                painter.line_segment(
                    [at(*x1, *y1), at(*x2, *y2)],
                    Stroke::new((*width as f32 * scale).max(0.5), rgb(*color)),
                );
            }
            Op::Rect {
                x,
                y,
                width,
                height,
                color,
            } => {
                painter.rect_filled(
                    egui::Rect::from_min_max(at(*x, *y), at(x + width, y + height)),
                    0.0,
                    rgb(*color),
                );
            }
            Op::Image {
                src,
                x,
                y,
                width,
                height,
            } => {
                if let Some(texture) = crate::publish::data_url_texture(ui.ctx(), src) {
                    painter.image(
                        texture.id(),
                        egui::Rect::from_min_max(at(*x, *y), at(x + width, y + height)),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
            }
        }
    }
    let label = format!("Page {}", index + 1);
    ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::Image);
        n.set_label(label);
    });
}

/// The navigator: Previous / Next, "i / n", and the parts by group (chapters numbered).
fn navigator(ui: &mut Ui, palette: &Palette, list: &[Section], active: usize) -> Option<usize> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("NAVIGATE")
                .size(10.0)
                .color(palette.text_muted),
        );
        let previous = ui
            .add_enabled(active > 0, egui::Button::new("‹").small())
            .on_hover_text("Previous section");
        crate::views::name_field(ui, &previous, "Previous section");
        if previous.clicked() {
            chosen = Some(active - 1);
        }
        ui.label(RichText::new(format!("{} / {}", active + 1, list.len())).size(11.0));
        let next = ui
            .add_enabled(active + 1 < list.len(), egui::Button::new("›").small())
            .on_hover_text("Next section");
        crate::views::name_field(ui, &next, "Next section");
        if next.clicked() {
            chosen = Some(active + 1);
        }
    });
    for group in [Group::Front, Group::Chapters, Group::Back] {
        let members: Vec<(usize, &Section)> = list
            .iter()
            .enumerate()
            .filter(|(_, s)| s.group == group)
            .collect();
        if members.is_empty() {
            continue;
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(group.label())
                .size(11.0)
                .strong()
                .color(palette.text_secondary),
        );
        for (number, (index, section)) in members.into_iter().enumerate() {
            let text = if group == Group::Chapters {
                format!("{:02}  {}", number + 1, section.label)
            } else {
                section.label.clone()
            };
            let item = ui
                .add(
                    egui::Button::selectable(index == active, RichText::new(text).size(12.0))
                        .truncate(),
                )
                .on_hover_text(&section.label);
            if item.clicked() && index != active {
                chosen = Some(index);
            }
        }
    }
    chosen
}

/// The navigator when the column is narrow: Previous / Next and a list to pick from.
fn picker(ui: &mut Ui, list: &[Section], active: usize) -> Option<usize> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        let previous = ui
            .add_enabled(active > 0, egui::Button::new("‹").small())
            .on_hover_text("Previous section");
        crate::views::name_field(ui, &previous, "Previous section");
        if previous.clicked() {
            chosen = Some(active - 1);
        }
        let mut picked = active;
        let combo = craft_ui::ComboBox::from_id_salt("preview-section")
            .selected_text(format!(
                "{} / {} · {}",
                active + 1,
                list.len(),
                list[active].label
            ))
            .width((ui.available_width() - 30.0).max(80.0))
            .show_ui(ui, |ui| {
                for (index, section) in list.iter().enumerate() {
                    ui.selectable_value(&mut picked, index, &section.label);
                }
            });
        ui.ctx()
            .accesskit_node_builder(combo.response.id, |n| n.set_label("Document navigator"));
        if picked != active {
            chosen = Some(picked);
        }
        let next = ui
            .add_enabled(active + 1 < list.len(), egui::Button::new("›").small())
            .on_hover_text("Next section");
        crate::views::name_field(ui, &next, "Next section");
        if next.clicked() {
            chosen = Some(active + 1);
        }
    });
    chosen
}

/// The chosen part's pages: from the page it starts on to the page the next part starts on.
fn stage(ui: &mut Ui, palette: &Palette, layout: &Layout, section: &Section, height: f32) {
    let width = (ui.available_width() - 24.0).clamp(160.0, 560.0);
    let start = layout
        .sections
        .iter()
        .find(|(id, _)| *id == section.id)
        .map(|(_, p)| *p);
    let end = layout
        .sections
        .iter()
        .skip_while(|(id, _)| *id != section.id)
        .nth(1)
        .map_or(layout.pages.len(), |(_, p)| *p);
    egui::ScrollArea::vertical()
        .id_salt(("preview-stage", section.id.as_str()))
        .max_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(6.0);
            match start {
                Some(start) => {
                    for index in start..end.max(start + 1) {
                        page(ui, layout, index, width);
                        ui.add_space(14.0);
                    }
                }
                // A part the PDF leaves out (a bibliography with no entries).
                None => {
                    ui.label(RichText::new("This part has no pages.").color(palette.text_muted));
                }
            }
        });
}

/// `publish-preview`.
pub(crate) fn show(
    ui: &mut Ui,
    palette: &Palette,
    project_name: &str,
    badge: &str,
    book: &CompiledBook,
    options: &PublishOptions,
) {
    eyebrow(ui, palette, "Live preview");
    let name = if project_name.is_empty() {
        "Untitled"
    } else {
        project_name
    };
    ui.add(
        egui::Label::new(
            RichText::new(name)
                .font(crate::fonts::serif(20.0))
                .color(palette.text_heading),
        )
        .wrap(),
    );
    ui.label(RichText::new(badge).size(11.0).color(palette.text_muted));
    ui.add_space(8.0);
    let list = sections::list(book, options);
    let previewable =
        !book.chapters.is_empty() || !book.front_matter.is_empty() || !book.back_matter.is_empty();
    if !previewable || list.is_empty() {
        ui.label(RichText::new("Nothing to publish yet").strong());
        ui.label(
            RichText::new("Add a scene to a chapter and it will appear here, ready to export.")
                .color(palette.text_muted),
        );
        return;
    }
    let wanted = ui.data(|d| d.get_temp::<String>(section_id()));
    let mut active = wanted
        .and_then(|id| list.iter().position(|s| s.id == id))
        .unwrap_or(0);
    let layout = layout(ui.ctx(), book, options);
    // The navigator beside the pages when there is room; else a picker above them.
    let wide = ui.available_width() >= 170.0 + 12.0 + 300.0;
    let height = (ui.available_height() - if wide { 40.0 } else { 80.0 }).max(200.0);
    let choose = |ui: &mut Ui, index: usize, active: &mut usize| {
        *active = index;
        let id = list[index].id.clone();
        ui.data_mut(|d| d.insert_temp(section_id(), id));
    };
    if wide {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(170.0);
                egui::ScrollArea::vertical()
                    .id_salt("preview-nav")
                    .max_height(height)
                    .show(ui, |ui| {
                        if let Some(index) = navigator(ui, palette, &list, active) {
                            choose(ui, index, &mut active);
                        }
                    });
            });
            ui.add_space(12.0);
            ui.vertical(|ui| stage(ui, palette, &layout, &list[active], height));
        });
    } else {
        if let Some(index) = picker(ui, &list, active) {
            choose(ui, index, &mut active);
        }
        ui.add_space(6.0);
        stage(ui, palette, &layout, &list[active], height);
    }
    let section = &list[active];
    let paginated = matches!(options.format.as_str(), "pdf" | "docx");
    let mut caption = if paginated {
        format!("Paginated preview of {}.", section.label)
    } else {
        format!("Previewing {}.", section.label)
    };
    caption.push_str(&format!(" Section {} of {}.", active + 1, list.len()));
    if let Some(pages) = book.page_estimate(options) {
        caption.push_str(&format!(
            " Estimated {pages} {} for the full book.",
            if pages == 1 { "page" } else { "pages" }
        ));
    }
    ui.add(egui::Label::new(RichText::new(caption).size(11.0).color(palette.text_muted)).wrap());
}
