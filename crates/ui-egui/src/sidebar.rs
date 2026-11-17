//! The right sidebar: Progress, Comments, Snapshots and Footnotes (ProseCraft's right sidebar).

use egui::{ProgressBar, RichText, Ui};
use prosecraft_core::html::{PlainTextOptions, html_to_plain_text};
use prosecraft_core::project::Project;

use crate::views::grouped;
use crate::{AppState, Selection, SideTab};

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    ui.horizontal_wrapped(|ui| {
        for tab in SideTab::ALL {
            let selected = state.side_tab == tab;
            let response = ui.selectable_label(selected, tab.label());
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_role(egui::accesskit::Role::Tab);
                node.set_selected(selected);
            });
            if response.clicked() {
                state.side_tab = tab;
            }
        }
    });
    ui.separator();
    let Some(project) = state
        .library
        .as_ref()
        .and_then(|l| l.projects().get(state.active))
        .map(|p| &p.view)
    else {
        ui.label(RichText::new("Nothing open.").weak());
        return;
    };
    // Comments, snapshots and footnotes belong to the open scene or page.
    let document = match &state.selection {
        Some(Selection::Document(id)) => Some(id.as_str()),
        _ => None,
    };
    egui::ScrollArea::vertical()
        .id_salt("sidebar-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| match state.side_tab {
            SideTab::Progress => progress(ui, project),
            SideTab::Comments => comments(ui, project, document),
            SideTab::Snapshots => snapshots(ui, project, document),
            SideTab::Footnotes => footnotes(ui, project, document),
        });
}

fn progress(ui: &mut Ui, project: &Project) {
    let words = project.word_count();
    ui.label(RichText::new("Project goal").strong());
    if project.project_goal > 0.0 {
        let fraction = (words as f64 / project.project_goal).clamp(0.0, 1.0) as f32;
        ui.add(ProgressBar::new(fraction).text(format!(
            "{} of {}",
            grouped(words),
            grouped(project.project_goal as usize)
        )));
    } else {
        ui.label(format!("{} words", grouped(words)));
    }
    ui.add_space(8.0);
    ui.label(RichText::new("Chapters").strong());
    for chapter in project.live_chapters() {
        ui.label(format!(
            "{}: {}",
            chapter.title,
            grouped(project.chapter_word_count(&chapter.id))
        ));
    }
}

fn pick_document(ui: &mut Ui) {
    ui.label(RichText::new("Open a scene or Wiki page to see its items.").weak());
}

fn comments(ui: &mut Ui, project: &Project, document: Option<&str>) {
    let Some(document) = document else {
        return pick_document(ui);
    };
    let list: Vec<_> = project
        .comments
        .iter()
        .filter(|c| c.document_id == document)
        .collect();
    if list.is_empty() {
        ui.label(RichText::new("No comments on this scene.").weak());
    }
    for comment in list {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(format!("“{}”", comment.quote)).italics());
            ui.label(comment.body.as_str());
            let mut meta = comment.author.clone();
            if comment.resolved {
                meta.push_str(" · resolved");
            }
            ui.label(RichText::new(meta).weak());
        });
    }
}

fn snapshots(ui: &mut Ui, project: &Project, document: Option<&str>) {
    let Some(document) = document else {
        return pick_document(ui);
    };
    let mut list: Vec<_> = project
        .snapshots
        .iter()
        .filter(|s| s.document_id == document)
        .collect();
    // Newest first, as ProseCraft lists them.
    list.sort_by(|a, b| b.created_at.total_cmp(&a.created_at));
    if list.is_empty() {
        ui.label(RichText::new("No snapshots of this scene.").weak());
    }
    for snapshot in list {
        let when = chrono::DateTime::from_timestamp_millis(snapshot.created_at as i64)
            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_default();
        let mut line = when;
        if snapshot.before_restore {
            line.push_str(" (before a restore)");
        }
        if let Some(note) = &snapshot.note {
            line.push_str(" · ");
            line.push_str(note);
        }
        ui.label(line);
    }
}

fn footnotes(ui: &mut Ui, project: &Project, document: Option<&str>) {
    let Some(document) = document.and_then(|id| project.document(id)) else {
        return pick_document(ui);
    };
    if document.footnotes.is_empty() {
        ui.label(RichText::new("No footnotes in this scene.").weak());
    }
    for (index, footnote) in document.footnotes.iter().enumerate() {
        let text = html_to_plain_text(&footnote.html, &PlainTextOptions::default());
        ui.label(format!("{}. {}", index + 1, text));
    }
}
