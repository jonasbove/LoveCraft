//! The binder: project switcher, chapters and scenes, the Wiki, and the Trash
//! (ProseCraft's left sidebar).

use egui::accesskit::Role;
use egui::{RichText, Ui};
use prosecraft_core::project::{Project, WikiItemKind};

use crate::{AppState, Selection};

/// A row of the binder tree: a selectable label exposed as a tree item with its level and
/// selection, so a screen reader hears "Opening, level 2, selected".
fn tree_row(ui: &mut Ui, label: &str, level: usize, selected: bool) -> bool {
    let response = ui.selectable_label(selected, label);
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::TreeItem);
        node.set_level(level);
        node.set_selected(selected);
    });
    response.clicked()
}

fn indent(ui: &mut Ui, depth: usize, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(14.0 * depth as f32);
        add(ui);
    });
}

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let Some(library) = state.library.as_ref() else {
        ui.label("No project open.");
        ui.label("Drop a ProseCraft backup (.prosecraft) on the window to open it.");
        return;
    };
    let projects = library.projects();
    let mut active = state.active.min(projects.len().saturating_sub(1));
    let Some(current) = projects.get(active) else {
        ui.label("This backup has no projects.");
        return;
    };

    craft_ui::ComboBox::new("project-switcher", "Project")
        .selected_text(current.view.name.as_str())
        .show_ui(ui, |ui| {
            for (index, project) in projects.iter().enumerate().filter(|(_, p)| !p.view.trashed) {
                ui.selectable_value(&mut active, index, project.view.name.as_str());
            }
        });
    if active != state.active {
        state.active = active;
        state.selection = None;
    }
    let Some(project) = library.projects().get(state.active).map(|p| &p.view) else {
        return;
    };
    if !project.author.is_empty() {
        ui.label(RichText::new(format!("by {}", project.author)).weak());
    }
    ui.separator();

    let mut chosen = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            chosen = chapters(ui, project, state.selection.as_ref());
            ui.add_space(8.0);
            chosen = wiki(ui, project, state.selection.as_ref()).or(chosen.take());
            ui.add_space(8.0);
            trash(ui, project);
        });
    if let Some(selection) = chosen {
        state.select(selection);
    }
}

fn chapters(ui: &mut Ui, project: &Project, selection: Option<&Selection>) -> Option<Selection> {
    let mut chosen = None;
    ui.heading("Chapters");
    let mut any = false;
    for chapter in project.live_chapters() {
        any = true;
        let selected = selection == Some(&Selection::Chapter(chapter.id.clone()));
        let title = if chapter.title.is_empty() {
            "Untitled chapter"
        } else {
            chapter.title.as_str()
        };
        if tree_row(ui, title, 1, selected) {
            chosen = Some(Selection::Chapter(chapter.id.clone()));
        }
        for scene in project.scenes_in_chapter(&chapter.id) {
            let selected = selection == Some(&Selection::Document(scene.id.clone()));
            let title = if scene.title.is_empty() {
                "Untitled scene"
            } else {
                scene.title.as_str()
            };
            indent(ui, 1, |ui| {
                if tree_row(ui, title, 2, selected) {
                    chosen = Some(Selection::Document(scene.id.clone()));
                }
            });
        }
    }
    if !any {
        ui.label(RichText::new("No chapters yet.").weak());
    }
    chosen
}

fn wiki(ui: &mut Ui, project: &Project, selection: Option<&Selection>) -> Option<Selection> {
    let mut chosen = None;
    ui.heading("Wiki");
    let items = project.wiki_tree();
    if items.is_empty() {
        ui.label(RichText::new("No Wiki pages yet.").weak());
    }
    for item in items {
        indent(ui, item.depth, |ui| match item.kind {
            WikiItemKind::Page => {
                let selected = selection == Some(&Selection::Document(item.id.clone()));
                let title = if item.title.is_empty() {
                    "Untitled page"
                } else {
                    item.title.as_str()
                };
                if tree_row(ui, title, item.depth + 1, selected) {
                    chosen = Some(Selection::Document(item.id.clone()));
                }
            }
            WikiItemKind::Folder => {
                ui.label(RichText::new(format!("{}/", item.title)).strong());
            }
            WikiItemKind::File => {
                ui.label(format!("File: {}", item.title));
            }
            WikiItemKind::Link => {
                ui.label(format!("Link: {}", item.title));
            }
        });
    }
    chosen
}

fn trash(ui: &mut Ui, project: &Project) {
    let (chapters, documents) = project.trash();
    let count = chapters.len() + documents.len();
    egui::CollapsingHeader::new(format!("Trash ({count})"))
        .id_salt("trash")
        .default_open(false)
        .show(ui, |ui| {
            if count == 0 {
                ui.label(RichText::new("The trash is empty.").weak());
            }
            for chapter in chapters {
                ui.label(format!("Chapter: {}", chapter.title));
            }
            for document in documents {
                ui.label(document.title.as_str());
            }
        });
}
