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

/// A row being dragged in the binder (`startChapterDrag`, `startSceneDrag`).
#[derive(Clone, Debug, PartialEq)]
enum Dragged {
    Chapter(String),
    Scene(String),
}

fn drag_id() -> egui::Id {
    egui::Id::unique("binder-drag")
}

/// A row's place this frame, for dropping on, and its drag starting.
fn drag_row(
    ui: &Ui,
    row: &egui::Response,
    dragged: Dragged,
    rows: &mut Vec<(egui::Rect, Dragged)>,
) {
    let row = row.interact(egui::Sense::drag());
    if row.drag_started() {
        ui.ctx()
            .data_mut(|d| d.insert_temp(drag_id(), dragged.clone()));
    }
    rows.push((row.rect, dragged));
}

/// The drop while a row is dragged: a chapter before or after another (35% down its row, as
/// `updateChapterDropTarget`), a scene before or after another (half way, as
/// `handleSceneDragOver`) or at the end of a chapter (`handleChapterSceneDrop`); marked while
/// dragging, made when the button is let go.
fn drop_rows(
    ui: &Ui,
    palette: &Palette,
    rows: &[(egui::Rect, Dragged)],
    editable: bool,
    intents: &mut Vec<Intent>,
) {
    let Some(dragged) = ui.ctx().data(|d| d.get_temp::<Dragged>(drag_id())) else {
        return;
    };
    let released = ui.input(|i| i.pointer.any_released() || !i.pointer.any_down());
    let pos = ui.ctx().pointer_latest_pos();
    let hit = pos.and_then(|p| {
        rows.iter()
            .filter(|(r, _)| r.y_range().contains(p.y))
            .find(|(_, target)| {
                matches!(
                    (&dragged, target),
                    (Dragged::Chapter(_), Dragged::Chapter(_)) | (Dragged::Scene(_), _)
                )
            })
            .map(|(r, target)| (p, *r, target))
    });
    let mut intent = None;
    if let Some((p, rect, target)) = hit {
        let painter = ui.painter();
        let line =
            |y: f32| painter.hline(rect.x_range(), y, egui::Stroke::new(2.0, palette.accent));
        match (&dragged, target) {
            (Dragged::Chapter(id), Dragged::Chapter(over)) if id != over => {
                let after = p.y >= rect.top() + rect.height() * 0.35;
                line(if after { rect.bottom() } else { rect.top() });
                intent = Some(Intent::ReorderChapter {
                    id: id.clone(),
                    target: over.clone(),
                    after,
                });
            }
            (Dragged::Scene(id), Dragged::Scene(over)) if id != over => {
                let after = p.y >= rect.center().y;
                line(if after { rect.bottom() } else { rect.top() });
                intent = Some(Intent::MoveSceneToPosition {
                    scene: id.clone(),
                    target: over.clone(),
                    after,
                });
            }
            (Dragged::Scene(id), Dragged::Chapter(chapter)) => {
                painter.rect_stroke(
                    rect,
                    4.0,
                    egui::Stroke::new(1.5, palette.accent),
                    egui::StrokeKind::Outside,
                );
                intent = Some(Intent::MoveSceneToChapter {
                    scene: id.clone(),
                    chapter: chapter.clone(),
                });
            }
            _ => {}
        }
    }
    if released {
        ui.ctx().data_mut(|d| d.remove::<Dragged>(drag_id()));
        if editable && let Some(intent) = intent {
            intents.push(intent);
        }
    } else {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
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
        // ProseCraft's version button, beside the sidebar's status.
        let version = ui
            .small_button(format!("v{}", prosecraft_core::about::VERSION))
            .on_hover_text("About");
        crate::views::name_field(
            ui,
            &version,
            &format!("About {}", prosecraft_core::about::APP_NAME),
        );
        if version.clicked() {
            state.about_wanted = true;
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
            // `trashActiveProject` (it asks first); the last project can't go.
            let live = projects.iter().filter(|p| !p.view.trashed).count();
            if actions {
                let trash = ui.add_enabled_ui(live > 1, |ui| {
                    ui.menu_button("Trash", |ui| {
                        ui.label(format!(
                            "Move “{}” to Trash? You can restore it later.",
                            current.view.name
                        ));
                        if ui
                            .button(RichText::new("Move to Trash").color(palette.danger))
                            .clicked()
                        {
                            state.intents.push(Intent::TrashProject);
                            ui.close();
                        }
                    })
                });
                let response = trash.inner.response.on_hover_text(if live > 1 {
                    "Move project to Trash"
                } else {
                    "The last project cannot be moved to Trash"
                });
                crate::views::name_field(ui, &response, "Move current project to Trash");
            }
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
            // The label filter belongs to the project it was chosen in.
            let mut filter = state
                .label_filter
                .as_ref()
                .filter(|(p, l)| *p == project.id && project.label(l).is_some())
                .map(|(_, l)| l.clone());
            chosen = chapters(
                ui,
                &palette,
                project,
                state.selection.as_ref(),
                state.editable,
                &mut state.intents,
                &mut manage_labels,
                &mut state.renaming_scene,
                &mut filter,
                state.now_ms,
            );
            state.label_filter = filter.map(|l| (project.id.clone(), l));
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
            if let Some(status) = &state.wiki_status {
                ui.label(RichText::new(status).size(11.0).color(palette.text_muted));
            }
            ui.add_space(14.0);
            ui.separator();
            ui.add_space(8.0);
            let trashed: Vec<(String, String)> = library
                .projects()
                .iter()
                .filter(|p| p.view.trashed)
                .map(|p| (p.view.id.clone(), p.view.name.clone()))
                .collect();
            trash(
                ui,
                &palette,
                project,
                &trashed,
                state.editable,
                &mut state.intents,
            );
        });
    if let Some(selection) = chosen {
        state.select(selection);
    }
}

#[allow(clippy::too_many_arguments)]
fn chapters(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    selection: Option<&Selection>,
    editable: bool,
    intents: &mut Vec<Intent>,
    manage_labels: &mut bool,
    renaming: &mut Option<(String, String)>,
    filter: &mut Option<String>,
    now_ms: f64,
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
    // "Filter scenes by label": only the scenes with that label, and their chapters.
    if !project.labels.is_empty() {
        let current = filter.as_deref().and_then(|id| project.label(id));
        let combo = craft_ui::ComboBox::from_id_salt(("label-filter", &project.id))
            .selected_text(current.map_or("All labels", |l| l.name.as_str()))
            .show_ui(ui, |ui| {
                ui.selectable_value(filter, None, "All labels");
                for label in &project.labels {
                    ui.selectable_value(filter, Some(label.id.clone()), label.name.as_str());
                }
            });
        ui.ctx().accesskit_node_builder(combo.response.id, |node| {
            node.set_label("Filter scenes by label");
        });
        if let Some(label) = current {
            let chip = ui.small_button(format!("Filter: {} · clear", label.name));
            crate::views::name_field(ui, &chip, &format!("Clear label filter: {}", label.name));
            if chip.clicked() {
                *filter = None;
            }
        }
    }
    let shown = |scene: &&prosecraft_core::project::Document| {
        filter
            .as_deref()
            .is_none_or(|l| scene.label_id.as_deref() == Some(l))
    };
    let mut any = false;
    let mut drag_rows = Vec::new();
    for chapter in project.live_chapters() {
        if filter.is_some() && !project.scenes_in_chapter(&chapter.id).any(|s| shown(&s)) {
            continue;
        }
        any = true;
        let selected = selection == Some(&Selection::Chapter(chapter.id.clone()));
        let title = crate::views::label_of(project, &chapter.id);
        if let Some((id, draft)) = renaming.as_mut().filter(|(id, _)| *id == chapter.id) {
            // `saveChapterRename`: Enter or leaving saves a title that isn't blank.
            let field = ui.add(
                egui::TextEdit::singleline(draft)
                    .char_limit(120)
                    .desired_width(180.0),
            );
            crate::views::name_field(ui, &field, &format!("Rename {title}"));
            if !field.has_focus() && !field.lost_focus() {
                field.request_focus();
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                *renaming = None;
            } else if field.lost_focus() {
                let next = draft.trim();
                if !next.is_empty() {
                    intents.push(Intent::SetChapterTitle {
                        chapter: id.clone(),
                        title: next.to_owned(),
                    });
                }
                *renaming = None;
            }
            continue;
        }
        let row = ui
            .horizontal(|ui| {
                let row = tree_row(ui, palette, &title, 1, selected);
                label_dot(ui, palette, project, chapter.label_id.as_deref());
                row
            })
            .inner;
        if editable {
            drag_row(
                ui,
                &row,
                Dragged::Chapter(chapter.id.clone()),
                &mut drag_rows,
            );
        }
        // `openBesideTarget` with Alt+Click: the chapter's first scene.
        if row.clicked() && ui.input(|i| i.modifiers.alt) {
            if let Some(first) = project.scenes_in_chapter(&chapter.id).next() {
                intents.push(Intent::OpenBeside(prosecraft_core::split::doc_target(
                    &first.id,
                )));
            }
        } else if row.clicked() {
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
                if ui.button("Rename chapter").clicked() {
                    *renaming = Some((chapter.id.clone(), title.clone()));
                    ui.close();
                }
                for (label, offset) in [("Move up", -1), ("Move down", 1)] {
                    if ui.button(label).clicked() {
                        intents.push(Intent::MoveChapter {
                            id: chapter.id.clone(),
                            offset,
                        });
                        ui.close();
                    }
                }
                if ui.button("Add scene").clicked() {
                    intents.push(Intent::AddScene {
                        chapter: chapter.id.clone(),
                    });
                    ui.close();
                }
                // `trashChapter` asks first.
                ui.menu_button(RichText::new("Move to Trash").color(palette.danger), |ui| {
                    let count = project.scenes_in_chapter(&chapter.id).count();
                    let scenes = if count == 1 { "scene" } else { "scenes" };
                    ui.label(format!(
                        "Move “{title}” and its {count} {scenes} to Trash? You can restore them later."
                    ));
                    if ui
                        .button(RichText::new("Move to Trash").color(palette.danger))
                        .clicked()
                    {
                        intents.push(Intent::TrashChapter(chapter.id.clone()));
                        ui.close();
                    }
                });
                ui.separator();
                if ui.button("Manage labels…").clicked() {
                    *manage_labels = true;
                    ui.close();
                }
            });
        }
        for scene in project.scenes_in_chapter(&chapter.id).filter(shown) {
            let selected = selection == Some(&Selection::Document(scene.id.clone()));
            let title = if scene.title.is_empty() {
                "Untitled scene"
            } else {
                scene.title.as_str()
            };
            indent(ui, 1, |ui| {
                if let Some((id, draft)) = renaming.as_mut().filter(|(id, _)| *id == scene.id) {
                    // `saveSceneRename`: Enter or leaving saves a title that isn't blank;
                    // Escape cancels.
                    let field = ui.add(
                        egui::TextEdit::singleline(draft)
                            .char_limit(120)
                            .desired_width(180.0),
                    );
                    crate::views::name_field(ui, &field, &format!("Rename {title}"));
                    if !field.has_focus() && !field.lost_focus() {
                        field.request_focus();
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        *renaming = None;
                    } else if field.lost_focus() {
                        let next = draft.trim();
                        if !next.is_empty() {
                            intents.push(Intent::SetDocumentTitle {
                                document: id.clone(),
                                title: next.to_owned(),
                            });
                        }
                        *renaming = None;
                    }
                    return;
                }
                let row = tree_row(ui, palette, title, 2, selected);
                label_dot(ui, palette, project, scene.label_id.as_deref());
                if editable {
                    drag_row(ui, &row, Dragged::Scene(scene.id.clone()), &mut drag_rows);
                }
                if row.clicked() && ui.input(|i| i.modifiers.alt) {
                    intents.push(Intent::OpenBeside(prosecraft_core::split::doc_target(
                        &scene.id,
                    )));
                } else if row.clicked() {
                    chosen = Some(Selection::Document(scene.id.clone()));
                }
                if editable {
                    row_menu(ui, &row, &format!("Actions for {title}"), |ui| {
                        if ui.button("Open beside").clicked() {
                            intents.push(Intent::OpenBeside(prosecraft_core::split::doc_target(
                                &scene.id,
                            )));
                            ui.close();
                        }
                        if ui.button("Rename scene").clicked() {
                            *renaming = Some((scene.id.clone(), title.to_owned()));
                            ui.close();
                        }
                        for (label, offset) in [("Move up", -1), ("Move down", 1)] {
                            if ui.button(label).clicked() {
                                intents.push(Intent::MoveScene {
                                    id: scene.id.clone(),
                                    offset,
                                });
                                ui.close();
                            }
                        }
                        // `convertToScreenplay` (it asks first; a snapshot of the prose is kept).
                        if !scene.screenplay {
                            ui.menu_button("Convert this scene to screenplay", |ui| {
                                ui.label(format!(
                                    "Convert “{title}” to screenplay format? A snapshot of the prose version will be saved first."
                                ));
                                if ui.button(RichText::new("Convert").strong()).clicked() {
                                    intents.push(Intent::ConvertToScreenplay {
                                        document: scene.id.clone(),
                                        now: now_ms,
                                    });
                                    ui.close();
                                }
                            });
                        }
                        ui.menu_button("Move to chapter…", |ui| {
                            for target in project.live_chapters() {
                                let here = scene.chapter_id.as_deref() == Some(&target.id);
                                let name = crate::views::label_of(project, &target.id);
                                if ui.radio(here, name).clicked() {
                                    intents.push(Intent::MoveSceneToChapter {
                                        scene: scene.id.clone(),
                                        chapter: target.id.clone(),
                                    });
                                    ui.close();
                                }
                            }
                        });
                        ui.separator();
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
    drop_rows(ui, palette, &drag_rows, editable, intents);
    if !any {
        let empty = if filter.is_some() {
            "No scenes with this label"
        } else {
            "No chapters yet."
        };
        ui.label(RichText::new(empty).color(palette.text_muted));
    }
    chosen
}

use prosecraft_core::wiki::Kind;

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
                // ProseCraft's "+": Add page, Add folder, Add file, Add web link.
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
                        if ui.button("Add file").clicked() {
                            intents.push(Intent::AddWikiFiles(None));
                            ui.close();
                        }
                        if ui.button("Add web link").clicked() {
                            intents.push(Intent::AddWikiLink(None));
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
        ui.label(RichText::new("Add a page, file, or link").color(palette.text_muted));
    }
    let folders: Vec<(String, String, usize)> = items
        .iter()
        .filter(|i| i.kind == WikiItemKind::Folder)
        .map(|i| (i.id.clone(), i.title.clone(), i.depth))
        .collect();
    let mut drag_rows: Vec<(egui::Rect, WikiDragged)> = Vec::new();
    let top = ui.cursor().top();
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
                if editable {
                    wiki_drag_row(ui, &row, (Kind::Document, &item.id), &mut drag_rows);
                }
                if row.clicked() && ui.input(|i| i.modifiers.alt) {
                    intents.push(Intent::OpenBeside(prosecraft_core::split::doc_target(
                        &item.id,
                    )));
                } else if row.clicked() {
                    chosen = Some(Selection::Document(item.id.clone()));
                }
                if editable {
                    row_menu(ui, &row, &format!("Actions for {title}"), |ui| {
                        if ui.button("Open page").clicked() {
                            chosen = Some(Selection::Document(item.id.clone()));
                            ui.close();
                        }
                        if ui.button("Open beside").clicked() {
                            intents.push(Intent::OpenBeside(prosecraft_core::split::doc_target(
                                &item.id,
                            )));
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
                    wiki_drag_row(ui, &row, (Kind::Folder, &item.id), &mut drag_rows);
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
                            if ui.button("Add file").clicked() {
                                intents.push(Intent::AddWikiFiles(Some(item.id.clone())));
                                ui.close();
                            }
                            if ui.button("Add web link").clicked() {
                                intents.push(Intent::AddWikiLink(Some(item.id.clone())));
                                ui.close();
                            }
                            if ui.button("Rename folder").clicked() {
                                *renaming = Some((item.id.clone(), item.title.clone()));
                                ui.close();
                            }
                            for (label, offset) in [("Move up", -1), ("Move down", 1)] {
                                if ui.button(label).clicked() {
                                    intents.push(Intent::MoveWikiFolder {
                                        id: item.id.clone(),
                                        offset,
                                    });
                                    ui.close();
                                }
                            }
                            if ui
                                .add_enabled(item.depth > 0, egui::Button::new("Move to top level"))
                                .clicked()
                            {
                                intents.push(Intent::MoveWikiFolderToTop(item.id.clone()));
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
            WikiItemKind::File | WikiItemKind::Link => {
                let Some(found) = project.research_items.iter().find(|r| r.id == item.id) else {
                    return;
                };
                let row = research_row(
                    ui,
                    palette,
                    &found.item,
                    item.depth,
                    editable,
                    &folders,
                    intents,
                );
                if editable {
                    let kind = if item.kind == WikiItemKind::File {
                        Kind::File
                    } else {
                        Kind::Link
                    };
                    wiki_drag_row(ui, &row, (kind, &item.id), &mut drag_rows);
                }
            }
        });
    }
    // `handleWikiRootDrop`: the rest of the Wiki's area takes a row to the top level.
    let bottom = ui.cursor().top() + 16.0;
    let root = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=bottom);
    wiki_drop(ui, palette, (&drag_rows, root), editable, intents);
    chosen
}

/// A Wiki row being dragged (`startWikiDrag`).
type WikiDragged = (Kind, String);

fn wiki_drag_id() -> egui::Id {
    egui::Id::unique("wiki-drag")
}

/// A Wiki row's place this frame, and its drag starting.
fn wiki_drag_row(
    ui: &Ui,
    row: &egui::Response,
    (kind, id): (Kind, &str),
    rows: &mut Vec<(egui::Rect, WikiDragged)>,
) {
    let row = row.interact(egui::Sense::drag());
    if row.drag_started() {
        ui.ctx()
            .data_mut(|d| d.insert_temp(wiki_drag_id(), (kind, id.to_owned())));
    }
    rows.push((row.rect, (kind, id.to_owned())));
}

/// `handleWikiRowDragOver` and the drops: on a folder, into it (for a folder, only in its middle
/// third; above or below that, beside it); on another row, before or after it (half way); on the
/// rest of the Wiki, to the top level. Marked while dragging, made when the button is let go.
fn wiki_drop(
    ui: &Ui,
    palette: &Palette,
    (rows, root): (&[(egui::Rect, WikiDragged)], egui::Rect),
    editable: bool,
    intents: &mut Vec<Intent>,
) {
    let Some((kind, id)) = ui.ctx().data(|d| d.get_temp::<WikiDragged>(wiki_drag_id())) else {
        return;
    };
    let released = ui.input(|i| i.pointer.any_released() || !i.pointer.any_down());
    let painter = ui.painter();
    let stroke = egui::Stroke::new(2.0, palette.accent);
    let pos = ui.ctx().pointer_latest_pos();
    let hit = pos.and_then(|p| {
        rows.iter()
            .find(|(r, _)| r.y_range().contains(p.y))
            .map(|(r, target)| (p, *r, target.clone()))
    });
    let mut intent = None;
    match hit {
        Some((_, _, (_, target))) if target == id => {}
        Some((p, rect, (target_kind, target))) => {
            let ratio = (p.y - rect.top()) / rect.height().max(1.0);
            let into = target_kind == Kind::Folder
                && (kind != Kind::Folder || (1.0 / 3.0..2.0 / 3.0).contains(&ratio));
            if into {
                painter.rect_stroke(rect.expand(2.0), 4.0, stroke, egui::StrokeKind::Outside);
                intent = Some(Intent::WikiIntoFolder {
                    kind,
                    id: id.clone(),
                    folder: Some(target),
                });
            } else {
                let after = p.y >= rect.center().y;
                painter.hline(
                    rect.x_range(),
                    if after { rect.bottom() } else { rect.top() },
                    stroke,
                );
                intent = Some(Intent::WikiToRow {
                    kind,
                    id: id.clone(),
                    target_kind,
                    target,
                    after,
                });
            }
        }
        None if pos.is_some_and(|p| root.contains(p)) => {
            painter.rect_stroke(
                root,
                4.0,
                egui::Stroke::new(1.0, palette.accent),
                egui::StrokeKind::Inside,
            );
            intent = Some(Intent::WikiIntoFolder {
                kind,
                id: id.clone(),
                folder: None,
            });
        }
        None => {}
    }
    if released {
        ui.ctx()
            .data_mut(|d| d.remove::<WikiDragged>(wiki_drag_id()));
        if editable && let Some(intent) = intent {
            intents.push(intent);
        }
    } else {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
}

/// A Wiki file or link: its mark and title (the size and type, or the address, on hover);
/// a click opens it, and its menu opens, reveals, moves or removes it.
fn research_row(
    ui: &mut Ui,
    palette: &Palette,
    item: &prosecraft_core::research::Item,
    depth: usize,
    editable: bool,
    folders: &[(String, String, usize)],
    intents: &mut Vec<Intent>,
) -> egui::Response {
    use prosecraft_core::research::Item;
    let (id, mark, file) = match item {
        Item::File { id, mime_type, .. } if mime_type == "application/pdf" => (id, "PDF", true),
        Item::File { id, mime_type, .. } if mime_type.starts_with("image/") => (id, "IMG", true),
        Item::File { id, .. } => (id, "FILE", true),
        Item::Link { id, .. } => (id, "LINK", false),
    };
    let title = item.title();
    let row = ui
        .horizontal(|ui| {
            ui.label(
                RichText::new(mark)
                    .size(9.0)
                    .monospace()
                    .color(palette.text_muted),
            );
            tree_row(ui, palette, title, depth + 1, false)
        })
        .inner;
    let hover = if file {
        format!("{title} — {}", item.meta())
    } else {
        item.meta()
    };
    let row = row.on_hover_text(hover);
    if row.clicked() && file && ui.input(|i| i.modifiers.alt) {
        intents.push(Intent::OpenBeside(prosecraft_core::split::file_target(id)));
    } else if row.clicked() {
        intents.push(Intent::OpenWikiItem(id.clone()));
    }
    if !editable {
        return row;
    }
    row_menu(ui, &row, &format!("Actions for {title}"), |ui| {
        if ui
            .button(if file { "Open file" } else { "Open link" })
            .clicked()
        {
            intents.push(Intent::OpenWikiItem(id.clone()));
            ui.close();
        }
        if file && ui.button("Open beside").clicked() {
            intents.push(Intent::OpenBeside(prosecraft_core::split::file_target(id)));
            ui.close();
        }
        if file && ui.button("Reveal in folder").clicked() {
            intents.push(Intent::RevealWikiFile(id.clone()));
            ui.close();
        }
        if !folders.is_empty() {
            ui.menu_button("Move to folder…", |ui| {
                if ui.button("Unfiled").clicked() {
                    intents.push(Intent::MoveWikiItem {
                        id: id.clone(),
                        folder: None,
                    });
                    ui.close();
                }
                for (folder, name, depth) in folders {
                    if ui
                        .button(format!("{}{name}", "  ".repeat(*depth)))
                        .clicked()
                    {
                        intents.push(Intent::MoveWikiItem {
                            id: id.clone(),
                            folder: Some(folder.clone()),
                        });
                        ui.close();
                    }
                }
            });
        }
        // `confirmDestructiveAction`.
        let remove = if file {
            "Remove and delete file"
        } else {
            "Remove link"
        };
        ui.menu_button(RichText::new(remove).color(palette.danger), |ui| {
            let deleted = if file {
                " The file will be deleted from the project folder."
            } else {
                ""
            };
            ui.label(format!("Remove “{title}” from the Wiki?{deleted}"));
            if ui
                .button(RichText::new("Remove").color(palette.danger))
                .clicked()
            {
                intents.push(Intent::RemoveWikiItem(id.clone()));
                ui.close();
            }
        });
    });
    row
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
    projects: &[(String, String)],
    editable: bool,
    intents: &mut Vec<Intent>,
) {
    let (chapters, documents) = project.trash();
    let count = chapters.len() + documents.len() + projects.len();
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
        for (id, name) in projects {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Project: {name}")).color(palette.text_secondary));
                if editable && restore_button(ui, &format!("project {name}")).clicked() {
                    intents.push(Intent::RestoreProject(id.clone()));
                }
                if editable {
                    // `permanentlyDeleteProject` asks first.
                    let delete = ui.menu_button(RichText::new("Delete").color(palette.danger), |ui| {
                        ui.label(format!(
                            "Permanently delete “{name}” and all its contents? This cannot be undone."
                        ));
                        if ui.button(RichText::new("Delete").color(palette.danger)).clicked() {
                            intents.push(Intent::DeleteProject(id.clone()));
                            ui.close();
                        }
                    });
                    let label = format!("Permanently delete project {name}");
                    ui.ctx()
                        .accesskit_node_builder(delete.response.id, |node| node.set_label(label));
                }
            });
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
                if editable {
                    // `permanentlyDeleteChapter` asks first.
                    let count = project
                        .documents
                        .iter()
                        .filter(|d| d.chapter_id.as_deref() == Some(&chapter.id))
                        .count();
                    let delete = ui.menu_button(RichText::new("Delete").color(palette.danger), |ui| {
                        ui.label(format!(
                            "Permanently delete “{}” and its {count} scene(s)? This cannot be undone.",
                            chapter.title
                        ));
                        if ui.button(RichText::new("Delete").color(palette.danger)).clicked() {
                            intents.push(Intent::DeleteChapter(chapter.id.clone()));
                            ui.close();
                        }
                    });
                    let name = format!("Permanently delete {} and its scenes", chapter.title);
                    ui.ctx()
                        .accesskit_node_builder(delete.response.id, |node| node.set_label(name));
                }
            });
        }
        for document in documents {
            ui.horizontal(|ui| {
                ui.label(RichText::new(document.title.as_str()).color(palette.text_secondary));
                if editable && restore_button(ui, &document.title).clicked() {
                    intents.push(Intent::RestoreDocument(document.id.clone()));
                }
                // `permanentlyDelete`: at once, as ProseCraft does.
                if editable {
                    let delete = ui.small_button(RichText::new("Delete").color(palette.danger));
                    let name = format!("Permanently delete {}", document.title);
                    ui.ctx()
                        .accesskit_node_builder(delete.id, |node| node.set_label(name));
                    if delete.clicked() {
                        intents.push(Intent::DeleteDocument(document.id.clone()));
                    }
                }
            });
        }
    });
}
