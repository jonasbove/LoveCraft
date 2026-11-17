//! The main panel: the workspace tabs and their views.

use egui::{Id, RichText, Ui, vec2};
use prosecraft_core::html::{PlainTextOptions, RichText as RichMode, html_to_plain_text};
use prosecraft_core::project::Project;

use crate::{AppState, Selection, WorkspaceView};

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

fn words(n: usize) -> String {
    format!("{} {}", grouped(n), if n == 1 { "word" } else { "words" })
}

fn tabs(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        for view in WorkspaceView::ALL {
            let label = view.label();
            let font = egui::TextStyle::Button.resolve(ui.style());
            let width = ui.fonts_mut(|f| {
                f.layout_no_wrap(label.to_owned(), font, egui::Color32::WHITE)
                    .size()
                    .x
            }) + 24.0;
            let (rect, _) = ui.allocate_exact_size(vec2(width, 28.0), egui::Sense::hover());
            let selected = state.view == view;
            let response =
                craft_ui::tabs::Tab::new(Id::unique(("workspace-tab", label)), label, selected)
                    .show_at(ui, rect, |ui, response| {
                        let visuals = ui.style().interact_selectable(response, selected);
                        ui.painter().rect_filled(
                            rect.shrink(1.0),
                            4.0,
                            if selected {
                                visuals.bg_fill
                            } else {
                                visuals.weak_bg_fill
                            },
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            label,
                            egui::TextStyle::Button.resolve(ui.style()),
                            visuals.text_color(),
                        );
                    });
            if response.clicked() {
                state.view = view;
            }
        }
    });
}

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    tabs(ui, state);
    ui.separator();
    if let Some(status) = &state.status {
        ui.label(RichText::new(status).weak());
    }
    let project = state
        .library
        .as_ref()
        .and_then(|l| l.projects().get(state.active))
        .map(|p| &p.view);
    let Some(project) = project else {
        welcome(ui);
        return;
    };
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match state.view {
            WorkspaceView::Outline => chosen = outline(ui, project),
            WorkspaceView::Write => write(ui, project, state.selection.as_ref()),
            WorkspaceView::Storyboard => not_ported(
                ui,
                &format!(
                    "The storyboard ({} cards) isn't ported yet.",
                    project.storyboard_cards
                ),
            ),
            WorkspaceView::Timeline => timeline(ui, project),
            WorkspaceView::Publish => not_ported(ui, "Publishing isn't ported yet."),
        });
    if let Some(selection) = chosen {
        state.select(selection);
    }
}

fn welcome(ui: &mut Ui) {
    ui.add_space(24.0);
    ui.heading("Outline. Write. Publish.");
    ui.label(
        "Open a ProseCraft backup to see your projects: drop a .prosecraft file on this window,",
    );
    ui.label("or start prosecraft-rs with the file's path.");
    ui.add_space(8.0);
    ui.label(RichText::new(prosecraft_core::about::credit_line()).weak());
}

fn not_ported(ui: &mut Ui, text: &str) {
    ui.add_space(16.0);
    ui.label(text);
    ui.label(RichText::new("It is on the roadmap; your data for it is kept as it is.").weak());
}

fn outline(ui: &mut Ui, project: &Project) -> Option<Selection> {
    ui.heading(project.name.as_str());
    let chapters = project.live_chapters().count();
    let noun = if chapters == 1 { "chapter" } else { "chapters" };
    ui.label(format!(
        "{} in {chapters} {noun}",
        words(project.word_count())
    ));
    ui.add_space(8.0);
    let mut open = None;
    for chapter in project.live_chapters() {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let title = if chapter.title.is_empty() {
                    "Untitled chapter"
                } else {
                    chapter.title.as_str()
                };
                if ui.link(RichText::new(title).strong()).clicked() {
                    open = Some(chapter.id.clone());
                }
                if let Some(label) = chapter.label_id.as_deref().and_then(|id| project.label(id)) {
                    ui.label(RichText::new(label.name.as_str()).weak());
                }
            });
            let scenes = project.scenes_in_chapter(&chapter.id).count();
            ui.label(format!(
                "{} {} · {}",
                scenes,
                if scenes == 1 { "scene" } else { "scenes" },
                words(project.chapter_word_count(&chapter.id))
            ));
            if let Some(summary) = &chapter.summary {
                ui.label(summary.as_str());
            }
        });
        ui.add_space(4.0);
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

fn write(ui: &mut Ui, project: &Project, selection: Option<&Selection>) {
    let note = "Read-only preview: editing arrives with the editor.";
    match selection {
        Some(Selection::Document(id)) => match project.document(id) {
            Some(document) => {
                ui.heading(document.title.as_str());
                ui.label(
                    RichText::new(format!("{} · {note}", words(document.word_count()))).weak(),
                );
                ui.add_space(8.0);
                ui.label(plain(&document.content));
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
            ui.heading(title);
            ui.label(
                RichText::new(format!(
                    "{} · {note}",
                    words(project.chapter_word_count(id))
                ))
                .weak(),
            );
            for (index, scene) in project.scenes_in_chapter(id).enumerate() {
                if index > 0 {
                    ui.vertical_centered(|ui| ui.label("⁂"));
                }
                ui.add_space(8.0);
                ui.label(RichText::new(scene.title.as_str()).strong());
                ui.label(plain(&scene.content));
            }
        }
        None => {
            ui.label("Choose a chapter, scene or Wiki page in the binder.");
        }
    }
}

fn timeline(ui: &mut Ui, project: &Project) {
    ui.heading("Timeline");
    if project.timeline.is_empty() {
        ui.label(RichText::new("No moments yet.").weak());
    }
    for node in &project.timeline {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(node.title.as_str()).strong());
            let when = [node.date.as_str(), node.time.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            if !when.is_empty() {
                ui.label(RichText::new(when).weak());
            }
            if !node.description.is_empty() {
                ui.label(node.description.as_str());
            }
        });
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
