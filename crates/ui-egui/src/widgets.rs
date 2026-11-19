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
    let measure = |font: &egui::FontId| -> Vec<f32> {
        labels
            .iter()
            .map(|label| {
                ui.fonts_mut(|f| {
                    f.layout_no_wrap((*label).to_owned(), font.clone(), palette.text_primary)
                        .size()
                        .x
                })
            })
            .collect()
    };
    let count = labels.len().max(1) as f32;
    // The frame's inner margin and stroke each side, and the 2-point gaps between segments.
    let gaps = 8.0 + 2.0 * labels.len().saturating_sub(1) as f32;
    let available = ui.available_width();
    // Padding shrinks (down to 6 points) so every segment fits the space there is; when even
    // that is too wide, the text gets smaller (down to 9 points) rather than the segments
    // running past the edge, which would widen everything below them.
    let mut font = egui::FontId::proportional(size);
    let mut widths = measure(&font);
    let text: f32 = widths.iter().sum();
    let narrowest = text + gaps + 6.0 * count;
    if narrowest > available && text > 0.0 {
        let fitted = (size * (available - gaps - 6.0 * count) / text).clamp(9.0, size);
        font = egui::FontId::proportional(fitted);
        widths = measure(&font);
    }
    let room = available - gaps - widths.iter().sum::<f32>();
    let padding = (room / count).clamp(6.0, 26.0);
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

#[cfg(test)]
mod tests {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn segments_fit_a_narrow_column_and_leave_its_width_alone() {
        // The right sidebar's four tabs in a column narrower than they are at full size: they
        // used to run past its edge, and the column widened, so everything right-aligned below
        // them (the Comments tab's count) ended up past the margin.
        let palette = crate::theme::Palette::for_scheme("prosecraft", true);
        let harness = egui_kittest::Harness::builder()
            .with_size([400.0, 200.0])
            .build_ui(move |ui| {
                let column = egui::Rect::from_min_size(ui.max_rect().min, egui::vec2(230.0, 150.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
                    let labels = ["Progress", "Comments", "Snapshots", "Footnotes"];
                    super::segmented(ui, &palette, "test-tabs", &labels, 0, 12.0);
                    ui.horizontal(|ui| {
                        ui.label("This chapter");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label("2")
                        });
                    });
                });
            });
        let left = harness.get_by_label("This chapter").rect().left();
        let right = left + 230.0;
        for tab in ["Progress", "Comments", "Snapshots", "Footnotes"] {
            let rect = harness.get_by_role_and_label(Role::Tab, tab).rect();
            assert!(
                rect.right() <= right + 0.5,
                "{tab} ends at {}",
                rect.right()
            );
        }
        let count = harness.get_by_label("2").rect();
        assert!(
            (count.right() - right).abs() <= 0.5,
            "the count ends at {}",
            count.right()
        );
    }
}
