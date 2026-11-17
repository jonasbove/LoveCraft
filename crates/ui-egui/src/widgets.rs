//! ProseCraft's recurring interface elements, drawn with the palette.

use egui::{Align2, CornerRadius, Frame, Id, Margin, Rect, RichText, Sense, Stroke, Ui, vec2};

use crate::fonts;
use crate::theme::Palette;

/// The small, tracked capitals ProseCraft puts above sections ("CHAPTERS", "YOUR WRITING").
pub fn eyebrow(ui: &mut Ui, palette: &Palette, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .font(fonts::semibold(10.0))
            .color(palette.eyebrow)
            .extra_letter_spacing(1.3),
    );
}

/// A raised card (outline chapters, comments, timeline moments).
pub fn card(palette: &Palette) -> Frame {
    Frame::new()
        .fill(palette.surface_raised)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(14, 12))
}

/// A segmented switch of tabs (ProseCraft's view switch and sidebar tabs). Each segment is a
/// `craft_ui` tab: role Tab, its selection exposed, focusable, Enter or Space selects it.
/// Returns the index chosen this frame.
pub fn segmented(
    ui: &mut Ui,
    palette: &Palette,
    id: &str,
    labels: &[&str],
    selected: usize,
    size: f32,
) -> Option<usize> {
    let font = egui::FontId::proportional(size);
    // Padding shrinks (down to 8 points) so every segment fits the space there is.
    let widths: Vec<f32> = labels
        .iter()
        .map(|label| {
            ui.fonts_mut(|f| {
                f.layout_no_wrap((*label).to_owned(), font.clone(), palette.text_primary)
                    .size()
                    .x
            })
        })
        .collect();
    let gaps = 6.0 + 2.0 * labels.len().saturating_sub(1) as f32;
    let room = ui.available_width() - gaps - widths.iter().sum::<f32>();
    let padding = (room / labels.len().max(1) as f32).clamp(8.0, 26.0);
    let mut chosen = None;
    Frame::new()
        .fill(palette.control_bg)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (index, label) in labels.iter().enumerate() {
                    let text_width = widths.get(index).copied().unwrap_or_default();
                    let (rect, _) = ui.allocate_exact_size(
                        vec2(text_width + padding, size + 14.0),
                        Sense::hover(),
                    );
                    let is_selected = index == selected;
                    let response =
                        craft_ui::tabs::Tab::new(Id::unique((id, *label)), label, is_selected)
                            .show_at(ui, rect, |ui, response| {
                                let fill = if is_selected {
                                    Some(palette.surface_raised)
                                } else if response.hovered() {
                                    Some(palette.hover_bg)
                                } else {
                                    None
                                };
                                if let Some(fill) = fill {
                                    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
                                }
                                let color = if is_selected {
                                    palette.text_primary
                                } else {
                                    palette.text_secondary
                                };
                                ui.painter().text(
                                    rect.center(),
                                    Align2::CENTER_CENTER,
                                    *label,
                                    font.clone(),
                                    color,
                                );
                            });
                    if response.clicked() {
                        chosen = Some(index);
                    }
                }
            });
        });
    chosen
}

/// The thin goal bar under ProseCraft's word goals.
pub fn goal_bar(ui: &mut Ui, palette: &Palette, fraction: f32) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 5.0), Sense::hover());
    let fraction = fraction.clamp(0.0, 1.0);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::ProgressIndicator,
            true,
            format!("{:.0}% of goal", fraction * 100.0),
        )
    });
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), palette.control_bg);
    if fraction > 0.0 {
        let filled = Rect::from_min_size(rect.min, vec2(rect.width() * fraction, rect.height()));
        ui.painter()
            .rect_filled(filled, CornerRadius::same(3), palette.accent);
    }
}
