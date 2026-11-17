//! The binder: project switcher, chapters and scenes, the Wiki, and the Trash
//! (ProseCraft's left sidebar).

use egui::accesskit::Role;
use egui::{RichText, Ui};
use prosecraft_core::project::{Project, WikiItemKind};

use crate::editing::Intent;
use crate::theme::Palette;
use crate::widgets::eyebrow;
use crate::{AppState, Selection, fonts};

/// A row of the binder tree: a selectable label exposed as a tree item with its level and
/// selection, so a screen reader hears "Opening, level 2, selected".
fn tree_row(
    ui: &mut Ui,
    palette: &Palette,
    label: &str,
    level: usize,
    selected: bool,
) -> egui::Response {
    let color = if selected {
        palette.selected_text
    } else {
        palette.text_primary
    };
    let response = ui.selectable_label(selected, RichText::new(label).size(13.5).color(color));
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::TreeItem);
        node.set_level(level);
        node.set_selected(selected);
    });
    response
}

/// A row's label as ProseCraft's coloured dot (with its name for screen readers).
fn label_dot(ui: &mut Ui, palette: &Palette, project: &Project, label: Option<&str>) {
    if let Some(label) = label.and_then(|id| project.label(id)) {
        let dot = crate::labels::dot(
            ui,
            crate::labels::label_color(&label.color, palette.text_muted),
            8.0,
        )
        .on_hover_text(&label.name);
        let name = format!("Label: {}", label.name);
        ui.ctx().accesskit_node_builder(dot.id, move |node| {
            node.set_role(Role::Image);
            node.set_label(name);
        });
    }
}

/// The menu's Labels section: each label, then "No label" (`applyLabel`).
fn label_choices(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    intents: &mut Vec<Intent>,
    chapter: bool,
    id: &str,
    current: Option<&str>,
) {
    for label in &project.labels {
        let checked = current == Some(label.id.as_str());
        let response = ui
            .horizontal(|ui| {
                crate::labels::dot(
                    ui,
                    crate::labels::label_color(&label.color, palette.text_muted),
                    8.0,
                );
                ui.add(egui::Button::selectable(checked, label.name.as_str()))
            })
            .inner;
        if response.clicked() {
            intents.push(Intent::SetLabel {
                chapter,
                id: id.to_owned(),
                label: Some(label.id.clone()),
            });
            ui.close();
        }
    }
    if ui
        .add(egui::Button::selectable(current.is_none(), "No label"))
        .clicked()
    {
        intents.push(Intent::SetLabel {
            chapter,
            id: id.to_owned(),
            label: None,
        });
        ui.close();
    }
}

/// A row's menu, both on right-click (ProseCraft's context menu) and from a quiet "⋯" button
/// that the keyboard reaches (always there, so it can take the focus and keep its menu open).
fn row_menu(ui: &mut Ui, row: &egui::Response, name: &str, mut add: impl FnMut(&mut Ui)) {
    row.context_menu(|ui| add(ui));
    let muted = ui.visuals().weak_text_color();
    let button = egui::Button::new(RichText::new("⋯").color(muted)).frame(false);
    let response = egui::containers::menu::MenuButton::from_button(button)
        .ui(ui, |ui| add(ui))
        .0;
    crate::views::name_field(ui, &response, name);
}

fn indent(ui: &mut Ui, depth: usize, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(16.0 * depth as f32);
        add(ui);
    });
}

/// The binder's footer: open a backup, and export the library as one.
fn footer(ui: &mut Ui, state: &mut AppState) {
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        if ui.button("Open backup…").clicked() {
            state.requests.push(crate::HostRequest::OpenBackup);
        }
        if state.has_home && state.library.is_some() && ui.button("Export backup…").clicked() {
            state.export_wanted = true;
        }
        if state.has_home && ui.button("Settings…").clicked() {
            state.settings_open = true;
        }
    });
}

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    egui::Panel::bottom("binder-footer")
        .show_separator_line(false)
        .resizable(false)
        .frame(egui::Frame::NONE)
        .show(ui, |ui| footer(ui, state));
    let palette = state.palette;
    let Some(library) = state.library.as_ref() else {
        eyebrow(ui, &palette, "No project open");
        ui.label(
            RichText::new("Drop a ProseCraft backup (.prosecraft) on the window to open it.")
                .color(palette.text_muted),
        );
        return;
    };
    let projects = library.projects();
    let mut active = state.active.min(projects.len().saturating_sub(1));
    let Some(current) = projects.get(active) else {
        if state.has_home && state.editable {
            eyebrow(ui, &palette, "No projects yet");
            ui.label(
                RichText::new("Start a project, or open a ProseCraft backup.")
                    .color(palette.text_muted),
            );
            ui.add_space(6.0);
            if ui.button("Create project").clicked() {
                state.starter = Some(crate::starter::StarterDraft::new());
            }
        } else {
            ui.label("This backup has no projects.");
        }
        return;
    };

    ui.add_space(6.0);
    // ProseCraft shows the project's name as the switcher, with no label beside it. The name is
    // what's visible, so it leads the accessible name; "Project" says what the control is.
    // Its gear and "+" beside it open Project settings and create a project. Laid out from the
    // right, so the switcher takes exactly the width that is left and the row never overflows.
    let actions = state.has_home && state.editable;
    let row_size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y + 6.0);
    let row = ui.allocate_ui_with_layout(
        row_size,
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            let create = actions && {
                let plus = ui.button("+").on_hover_text("Create project");
                crate::views::name_field(ui, &plus, "Create project");
                plus.clicked()
            };
            let settings = actions && {
                let gear = ui.button("⚙").on_hover_text("Project settings");
                crate::views::name_field(ui, &gear, "Project settings");
                gear.clicked()
            };
            let switcher = craft_ui::ComboBox::from_id_salt("project-switcher")
                .selected_text(
                    RichText::new(current.view.name.as_str())
                        .font(fonts::serif(17.0))
                        .color(palette.text_heading),
                )
                .width(ui.available_width())
                // A long name is shortened with "…" instead of widening the binder.
                .truncate()
                .show_ui(ui, |ui| {
                    for (index, project) in
                        projects.iter().enumerate().filter(|(_, p)| !p.view.trashed)
                    {
                        ui.selectable_value(&mut active, index, project.view.name.as_str());
                    }
                });
            (switcher, create, settings)
        },
    );
    let (switcher, create, settings) = row.inner;
    if settings {
        state.project_settings = Some(crate::project_settings::ProjectSettingsDraft::for_project(
            &current.view,
        ));
    }
    if create {
        state.starter = Some(crate::starter::StarterDraft::new());
    }
    ui.ctx()
        .accesskit_node_builder(switcher.response.id, |node| {
            node.set_label(current.view.name.as_str());
            node.set_description("Project");
        });
    if active != state.active {
        state.active = active;
        state.selection = None;
    }
    let Some(project) = library.projects().get(state.active).map(|p| &p.view) else {
        return;
    };
    if !project.author.is_empty() {
        ui.label(
            RichText::new(project.author.as_str())
                .size(12.0)
                .color(palette.text_muted),
        );
    }
    ui.add_space(14.0);

    let mut chosen = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut manage_labels = false;
            chosen = chapters(
                ui,
                &palette,
                project,
                state.selection.as_ref(),
                state.editable,
                &mut state.intents,
                &mut manage_labels,
            );
            if manage_labels {
                state.labels_dialog = Some(crate::labels::LabelsDraft::for_project(project));
            }
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(8.0);
            chosen = wiki(
                ui,
                &palette,
                project,
                state.selection.as_ref(),
                state.editable,
                &mut state.intents,
                &mut state.renaming_folder,
            )
            .or(chosen.take());
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(8.0);
            trash(ui, &palette, project, state.editable, &mut state.intents);
        });
    if let Some(selection) = chosen {
        state.select(selection);
    }
}

fn chapters(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    selection: Option<&Selection>,
    editable: bool,
    intents: &mut Vec<Intent>,
    manage_labels: &mut bool,
) -> Option<Selection> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        eyebrow(ui, palette, "Chapters");
        if editable {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // An icon button, as ProseCraft's: "+" on screen, "Add chapter" for screen readers.
                let add = ui.small_button("+").on_hover_text("Add chapter");
                ui.ctx()
                    .accesskit_node_builder(add.id, |node| node.set_label("Add chapter"));
                if add.clicked() {
                    intents.push(Intent::AddChapter);
                }
            });
        }
    });
    ui.add_space(4.0);
    let mut any = false;
    for chapter in project.live_chapters() {
        any = true;
        let selected = selection == Some(&Selection::Chapter(chapter.id.clone()));
        let title = crate::views::label_of(project, &chapter.id);
        let row = ui
            .horizontal(|ui| {
                let row = tree_row(ui, palette, &title, 1, selected);
                label_dot(ui, palette, project, chapter.label_id.as_deref());
                row
            })
            .inner;
        if row.clicked() {
            chosen = Some(Selection::Chapter(chapter.id.clone()));
        }
        if editable {
            row_menu(ui, &row, &format!("Actions for {title}"), |ui| {
                label_choices(
                    ui,
                    palette,
                    project,
                    intents,
                    true,
                    &chapter.id,
                    chapter.label_id.as_deref(),
                );
                ui.separator();
                if ui.button("Add scene").clicked() {
                    intents.push(Intent::AddScene {
                        chapter: chapter.id.clone(),
                    });
                    ui.close();
                }
                if ui
                    .button(RichText::new("Move to Trash").color(palette.danger))
                    .clicked()
                {
                    intents.push(Intent::TrashChapter(chapter.id.clone()));
                    ui.close();
                }
                ui.separator();
                if ui.button("Manage labels…").clicked() {
                    *manage_labels = true;
                    ui.close();
                }
            });
        }
        for scene in project.scenes_in_chapter(&chapter.id) {
            let selected = selection == Some(&Selection::Document(scene.id.clone()));
            let title = if scene.title.is_empty() {
                "Untitled scene"
            } else {
                scene.title.as_str()
            };
            indent(ui, 1, |ui| {
                let row = tree_row(ui, palette, title, 2, selected);
                label_dot(ui, palette, project, scene.label_id.as_deref());
                if row.clicked() {
                    chosen = Some(Selection::Document(scene.id.clone()));
                }
                if editable {
                    row_menu(ui, &row, &format!("Actions for {title}"), |ui| {
                        label_choices(
                            ui,
                            palette,
                            project,
                            intents,
                            false,
                            &scene.id,
                            scene.label_id.as_deref(),
                        );
                        ui.separator();
                        if ui
                            .button(RichText::new("Move to Trash").color(palette.danger))
                            .clicked()
                        {
                            intents.push(Intent::TrashDocument(scene.id.clone()));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Manage labels…").clicked() {
                            *manage_labels = true;
                            ui.close();
                        }
                    });
                }
            });
        }
    }
    if !any {
        ui.label(RichText::new("No chapters yet.").color(palette.text_muted));
    }
    chosen
}

fn wiki(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    selection: Option<&Selection>,
    editable: bool,
    intents: &mut Vec<crate::editing::Intent>,
    renaming: &mut Option<(String, String)>,
) -> Option<Selection> {
    use crate::editing::Intent;
    let mut chosen = None;
    ui.horizontal(|ui| {
        eyebrow(ui, palette, "Wiki");
        if editable {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // ProseCraft's "+": Add page, Add folder (files and web links come later).
                let add = ui
                    .menu_button("+", |ui| {
                        if ui.button("Add page").clicked() {
                            intents.push(Intent::AddWikiPage { folder: None });
                            ui.close();
                        }
                        if ui.button("Add folder").clicked() {
                            intents.push(Intent::AddWikiFolder { parent: None });
                            ui.close();
                        }
                    })
                    .response;
                crate::views::name_field(ui, &add, "Add to Wiki");
            });
        }
    });
    ui.add_space(4.0);
    let items = project.wiki_tree();
    if items.is_empty() {
        ui.label(RichText::new("Nothing here yet").color(palette.text_muted));
    }
    let folders: Vec<(String, String, usize)> = items
        .iter()
        .filter(|i| i.kind == WikiItemKind::Folder)
        .map(|i| (i.id.clone(), i.title.clone(), i.depth))
        .collect();
    for item in &items {
        indent(ui, item.depth, |ui| match item.kind {
            WikiItemKind::Page => {
                let selected = selection == Some(&Selection::Document(item.id.clone()));
                let title = if item.title.is_empty() {
                    "Untitled page"
                } else {
                    item.title.as_str()
                };
                let row = tree_row(ui, palette, title, item.depth + 1, selected);
                if row.clicked() {
                    chosen = Some(Selection::Document(item.id.clone()));
                }
                if editable {
                    row_menu(ui, &row, &format!("Actions for {title}"), |ui| {
                        if ui.button("Open page").clicked() {
                            chosen = Some(Selection::Document(item.id.clone()));
                            ui.close();
                        }
                        ui.menu_button("Move to folder…", |ui| {
                            if ui.button("Unfiled").clicked() {
                                intents.push(Intent::MoveWikiPage {
                                    document: item.id.clone(),
                                    folder: None,
                                });
                                ui.close();
                            }
                            for (id, name, depth) in &folders {
                                if ui
                                    .button(format!("{}{name}", "  ".repeat(*depth)))
                                    .clicked()
                                {
                                    intents.push(Intent::MoveWikiPage {
                                        document: item.id.clone(),
                                        folder: Some(id.clone()),
                                    });
                                    ui.close();
                                }
                            }
                        });
                        if ui
                            .button(RichText::new("Move to Trash").color(palette.danger))
                            .clicked()
                        {
                            intents.push(Intent::TrashDocument(item.id.clone()));
                            ui.close();
                        }
                    });
                }
            }
            WikiItemKind::Folder => {
                if let Some((id, draft)) = renaming.as_mut().filter(|(id, _)| *id == item.id) {
                    // Renaming in place: Enter or leaving saves, Escape cancels.
                    let field = ui.add(
                        egui::TextEdit::singleline(draft)
                            .char_limit(60)
                            .desired_width(160.0),
                    );
                    crate::views::name_field(ui, &field, "Folder name");
                    if !field.has_focus() && !field.lost_focus() {
                        field.request_focus();
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        *renaming = None;
                    } else if field.lost_focus() {
                        intents.push(Intent::RenameWikiFolder {
                            id: id.clone(),
                            title: draft.clone(),
                        });
                        *renaming = None;
                    }
                    return;
                }
                let row = ui.add(
                    egui::Label::new(
                        RichText::new(item.title.as_str())
                            .size(13.5)
                            .color(palette.text_secondary)
                            .strong(),
                    )
                    .sense(egui::Sense::click()),
                );
                let name = item.title.clone();
                ui.ctx().accesskit_node_builder(row.id, move |node| {
                    node.set_role(Role::TreeItem);
                    node.set_label(name);
                });
                if editable {
                    let parent_title = folders
                        .iter()
                        .rev()
                        .skip_while(|(id, ..)| *id != item.id)
                        .find(|(_, _, d)| *d + 1 == item.depth)
                        .map_or("the Wiki root".to_owned(), |(_, t, _)| t.clone());
                    row_menu(
                        ui,
                        &row,
                        &format!("Actions for folder {}", item.title),
                        |ui| {
                            if ui.button("Add page").clicked() {
                                intents.push(Intent::AddWikiPage {
                                    folder: Some(item.id.clone()),
                                });
                                ui.close();
                            }
                            if ui.button("Add subfolder").clicked() {
                                intents.push(Intent::AddWikiFolder {
                                    parent: Some(item.id.clone()),
                                });
                                ui.close();
                            }
                            if ui.button("Rename folder").clicked() {
                                *renaming = Some((item.id.clone(), item.title.clone()));
                                ui.close();
                            }
                            ui.menu_button(RichText::new("Remove folder").color(palette.danger), |ui| {
                            ui.label(format!("Remove “{}”? Its folders, pages, files, and links will move to {parent_title}.", item.title));
                            if ui.button(RichText::new("Remove").color(palette.danger)).clicked() {
                                intents.push(Intent::RemoveWikiFolder(item.id.clone()));
                                ui.close();
                            }
                        });
                        },
                    );
                }
            }
            WikiItemKind::File => {
                ui.label(
                    RichText::new(format!("File: {}", item.title))
                        .size(13.0)
                        .color(palette.text_secondary),
                );
            }
            WikiItemKind::Link => {
                ui.label(
                    RichText::new(format!("Link: {}", item.title))
                        .size(13.0)
                        .color(palette.text_secondary),
                );
            }
        });
    }
    chosen
}

/// "Restore", named for its item so each button in the Trash can be told apart.
fn restore_button(ui: &mut Ui, item: &str) -> egui::Response {
    let response = ui.small_button("Restore");
    let name = format!("Restore {item}");
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_label(name));
    response
}

fn trash(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    editable: bool,
    intents: &mut Vec<Intent>,
) {
    let (chapters, documents) = project.trash();
    let count = chapters.len() + documents.len();
    egui::CollapsingHeader::new(
        RichText::new(format!("Trash ({count})"))
            .size(13.5)
            .color(palette.text_primary),
    )
    .id_salt("trash")
    .default_open(false)
    .show(ui, |ui| {
        if count == 0 {
            ui.label(RichText::new("Trash is empty").color(palette.text_muted));
        }
        for chapter in chapters {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("Chapter: {}", chapter.title))
                        .color(palette.text_secondary),
                );
                if editable && restore_button(ui, &chapter.title).clicked() {
                    intents.push(Intent::RestoreChapter(chapter.id.clone()));
                }
            });
        }
        for document in documents {
            ui.horizontal(|ui| {
                ui.label(RichText::new(document.title.as_str()).color(palette.text_secondary));
                if editable && restore_button(ui, &document.title).clicked() {
                    intents.push(Intent::RestoreDocument(document.id.clone()));
                }
            });
        }
    });
}
