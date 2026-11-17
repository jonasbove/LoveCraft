//! The main panel: the view switch and the Outline, Write, Storyboard, Timeline and Publish views.

use egui::{RichText, Ui};
use prosecraft_core::html::{PlainTextOptions, RichText as RichMode, html_to_plain_text};
use prosecraft_core::project::Project;

use crate::theme::Palette;
use crate::widgets::{card, eyebrow, segmented};
use crate::{AppState, Selection, WorkspaceView, fonts};

/// The writing column's widest measure, as ProseCraft's editor.
const COLUMN: f32 = 720.0;

/// Thousands separators, as ProseCraft shows counts (`toLocaleString` in English).
pub(crate) fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub(crate) fn words(n: usize) -> String {
    format!("{} {}", grouped(n), if n == 1 { "word" } else { "words" })
}

/// Lays `add` out in a centred column at most `COLUMN` wide.
fn column(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    let inner = width.min(COLUMN);
    ui.horizontal(|ui| {
        ui.add_space(((width - inner) / 2.0).max(0.0));
        ui.vertical(|ui| {
            ui.set_width(inner);
            add(ui);
        });
    });
}

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    ui.add_space(14.0);
    let labels: Vec<&str> = WorkspaceView::ALL.iter().map(|v| v.label()).collect();
    let current = WorkspaceView::ALL
        .iter()
        .position(|v| *v == state.view)
        .unwrap_or(0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        if let Some(index) = segmented(ui, &palette, "workspace-tab", &labels, current, 13.0) {
            state.view = WorkspaceView::ALL.get(index).copied().unwrap_or_default();
        }
    });
    if let Some(status) = &state.status {
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            ui.label(RichText::new(status).size(12.0).color(palette.text_muted));
        });
    }
    let project = state
        .library
        .as_ref()
        .and_then(|l| l.projects().get(state.active))
        .map(|p| &p.view);
    let Some(project) = project else {
        column(ui, |ui| welcome(ui, &palette));
        return;
    };
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            column(ui, |ui| match state.view {
                WorkspaceView::Outline => chosen = outline(ui, &palette, project),
                WorkspaceView::Write => write(ui, &palette, project, state.selection.as_ref()),
                WorkspaceView::Storyboard => {
                    let text = format!(
                        "The storyboard ({} cards) isn't ported yet.",
                        project.storyboard_cards
                    );
                    not_ported(ui, &palette, "Storyboard", &text);
                }
                WorkspaceView::Timeline => timeline(ui, &palette, project),
                WorkspaceView::Publish => {
                    not_ported(ui, &palette, "Publish", "Publishing isn't ported yet.")
                }
            });
        });
    if let Some(selection) = chosen {
        state.select(selection);
    }
}

fn welcome(ui: &mut Ui, palette: &Palette) {
    ui.add_space(64.0);
    eyebrow(ui, palette, "prosecraft-rs");
    ui.label(
        RichText::new("Outline. Write. Publish.")
            .font(fonts::serif(38.0))
            .color(palette.text_heading),
    );
    ui.add_space(12.0);
    ui.label(
        RichText::new(
            "Open a ProseCraft backup to see your projects: drop a .prosecraft file on this window, or start prosecraft-rs with the file's path.",
        )
        .size(14.0)
        .color(palette.text_body),
    );
    ui.add_space(18.0);
    ui.label(
        RichText::new(prosecraft_core::about::credit_line())
            .size(12.0)
            .color(palette.text_muted),
    );
}

fn not_ported(ui: &mut Ui, palette: &Palette, title: &str, text: &str) {
    ui.add_space(40.0);
    eyebrow(ui, palette, title);
    ui.label(
        RichText::new(text)
            .font(fonts::serif(22.0))
            .color(palette.text_heading),
    );
    ui.add_space(6.0);
    ui.label(
        RichText::new("It is on the roadmap; your data for it is kept as it is.")
            .color(palette.text_muted),
    );
}

fn outline(ui: &mut Ui, palette: &Palette, project: &Project) -> Option<Selection> {
    ui.add_space(28.0);
    eyebrow(ui, palette, "Outline");
    ui.label(
        RichText::new(project.name.as_str())
            .font(fonts::serif(32.0))
            .color(palette.text_heading),
    );
    let chapters = project.live_chapters().count();
    let noun = if chapters == 1 { "chapter" } else { "chapters" };
    ui.label(
        RichText::new(format!(
            "{} in {chapters} {noun}",
            words(project.word_count())
        ))
        .color(palette.text_muted),
    );
    ui.add_space(18.0);
    let mut open = None;
    for chapter in project.live_chapters() {
        card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let title = if chapter.title.is_empty() {
                    "Untitled chapter"
                } else {
                    chapter.title.as_str()
                };
                let text = RichText::new(title)
                    .font(fonts::serif(19.0))
                    .color(palette.text_heading);
                if ui.add(egui::Button::new(text).frame(false)).clicked() {
                    open = Some(chapter.id.clone());
                }
                if let Some(label) = chapter.label_id.as_deref().and_then(|id| project.label(id)) {
                    ui.label(
                        RichText::new(label.name.as_str())
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                }
            });
            let scenes = project.scenes_in_chapter(&chapter.id).count();
            let meta = format!(
                "{scenes} {} · {}",
                if scenes == 1 { "scene" } else { "scenes" },
                words(project.chapter_word_count(&chapter.id))
            );
            ui.label(RichText::new(meta).size(12.0).color(palette.text_muted));
            if let Some(summary) = &chapter.summary {
                ui.add_space(4.0);
                ui.label(RichText::new(summary.as_str()).color(palette.text_body));
            }
        });
        ui.add_space(10.0);
    }
    open.map(Selection::Chapter)
}

fn plain(html: &str) -> String {
    html_to_plain_text(
        html,
        &PlainTextOptions {
            rich: RichMode::Source,
        },
    )
}

/// The manuscript text: serif, ProseCraft's 1.85 line spacing, a gap between paragraphs.
fn manuscript(ui: &mut Ui, palette: &Palette, text: &str) {
    for paragraph in text.split("\n\n") {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = ui.available_width();
        let format = egui::TextFormat {
            font_id: fonts::serif(17.0),
            color: palette.text_body,
            line_height: Some(17.0 * 1.85),
            ..Default::default()
        };
        job.append(paragraph, 0.0, format);
        ui.label(job);
        ui.add_space(8.0);
    }
}

fn footer(ui: &mut Ui, palette: &Palette, words_count: usize) {
    ui.add_space(24.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(words(words_count))
                .size(12.0)
                .color(palette.text_muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new("Read-only preview: editing arrives with the editor")
                    .size(12.0)
                    .color(palette.text_muted),
            );
        });
    });
}

fn write(ui: &mut Ui, palette: &Palette, project: &Project, selection: Option<&Selection>) {
    ui.add_space(22.0);
    match selection {
        Some(Selection::Document(id)) => match project.document(id) {
            Some(document) => {
                let place = document
                    .chapter_id
                    .as_deref()
                    .and_then(|c| project.chapters.iter().find(|ch| ch.id == c))
                    .map(|c| c.title.clone())
                    .unwrap_or_else(|| "Wiki".to_owned());
                let kind = if document.is_wiki() { "Page" } else { "Scene" };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(place).size(12.0).color(palette.text_muted));
                    ui.label(RichText::new("·").size(12.0).color(palette.text_muted));
                    eyebrow(ui, palette, kind);
                });
                ui.add_space(10.0);
                ui.label(
                    RichText::new(document.title.as_str())
                        .font(fonts::serif(38.0))
                        .color(palette.text_heading),
                );
                ui.add_space(18.0);
                manuscript(ui, palette, &plain(&document.content));
                footer(ui, palette, document.word_count());
            }
            None => {
                ui.label("That document is no longer in this project.");
            }
        },
        Some(Selection::Chapter(id)) => {
            let title = project
                .chapters
                .iter()
                .find(|c| &c.id == id)
                .map(|c| c.title.as_str())
                .unwrap_or_default();
            eyebrow(ui, palette, "Chapter");
            ui.label(
                RichText::new(title)
                    .font(fonts::serif(38.0))
                    .color(palette.text_heading),
            );
            ui.add_space(18.0);
            for (index, scene) in project.scenes_in_chapter(id).enumerate() {
                if index > 0 {
                    ui.add_space(6.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("⁂").size(16.0).color(palette.text_muted))
                    });
                    ui.add_space(6.0);
                }
                ui.label(
                    RichText::new(scene.title.as_str())
                        .font(fonts::serif(22.0))
                        .color(palette.text_heading),
                );
                ui.add_space(8.0);
                manuscript(ui, palette, &plain(&scene.content));
            }
            footer(ui, palette, project.chapter_word_count(id));
        }
        None => {
            ui.add_space(40.0);
            let hint = RichText::new("Choose a chapter, scene or Wiki page in the binder.")
                .font(fonts::serif(20.0))
                .color(palette.text_muted);
            ui.label(hint);
        }
    }
}

fn timeline(ui: &mut Ui, palette: &Palette, project: &Project) {
    ui.add_space(28.0);
    eyebrow(ui, palette, "Timeline");
    ui.label(
        RichText::new("Moments")
            .font(fonts::serif(32.0))
            .color(palette.text_heading),
    );
    ui.add_space(14.0);
    if project.timeline.is_empty() {
        ui.label(RichText::new("No moments yet.").color(palette.text_muted));
    }
    for node in &project.timeline {
        card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let when = [node.date.as_str(), node.time.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            if !when.is_empty() {
                ui.label(RichText::new(when).size(12.0).color(palette.text_muted));
            }
            ui.label(
                RichText::new(node.title.as_str())
                    .font(fonts::serif(18.0))
                    .color(palette.text_heading),
            );
            if !node.description.is_empty() {
                ui.label(RichText::new(node.description.as_str()).color(palette.text_body));
            }
        });
        ui.add_space(10.0);
    }
}

#[cfg(test)]
mod tests {
    use super::grouped;

    #[test]
    fn groups_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(80_000), "80,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}
