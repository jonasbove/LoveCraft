//! ProseCraft's Project Settings dialog: the project's name, author and type, and Labels
//! ("Manage labels…").

use egui::{RichText, Ui};

use crate::AppState;

/// The dialog's fields while it is open.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProjectSettingsDraft {
    pub name: String,
    pub author: String,
    pub screenplay: bool,
    /// Focus the name field once.
    pub focus: bool,
}

impl ProjectSettingsDraft {
    /// `openProjectSettings` for the open project.
    pub fn for_project(project: &prosecraft_core::project::Project) -> Self {
        Self {
            name: project.name.clone(),
            author: project.author.clone(),
            screenplay: project.kind == prosecraft_core::project::ProjectKind::Screenplay,
            focus: true,
        }
    }
}

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let editable = state.editable;
    // Under the Labels dialog (which covers it) Project Settings is inert, as ProseCraft's
    // modal dialogs are: it isn't drawn, so only the Labels dialog's controls are there.
    if state.labels_dialog.is_some() {
        return;
    }
    let Some(draft) = state.project_settings.as_mut() else {
        return;
    };
    let mut open = true;
    let mut save = false;
    let mut manage = false;
    egui::Window::new("Project Settings")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(440.0, 390.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.label(
                RichText::new("Project details")
                    .strong()
                    .color(palette.text_heading),
            );
            ui.label(
                RichText::new("Set the title and author shown in your workspace")
                    .size(12.0)
                    .color(palette.text_muted),
            );
            ui.add_space(8.0);
            let label = ui.label(
                RichText::new("Project name")
                    .size(12.0)
                    .color(palette.text_secondary),
            );
            let name = ui
                .add(
                    egui::TextEdit::singleline(&mut draft.name)
                        .char_limit(100)
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id);
            if std::mem::take(&mut draft.focus) {
                name.request_focus();
            }
            ui.add_space(6.0);
            let label = ui.label(
                RichText::new("Author name")
                    .size(12.0)
                    .color(palette.text_secondary),
            );
            ui.add(
                egui::TextEdit::singleline(&mut draft.author)
                    .char_limit(100)
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(label.id);
            ui.add_space(6.0);
            ui.label(
                RichText::new("Project type")
                    .size(12.0)
                    .color(palette.text_secondary),
            );
            let current = if draft.screenplay {
                "Screenplay"
            } else {
                "Novel"
            };
            let mut screenplay = draft.screenplay;
            let combo = craft_ui::ComboBox::from_id_salt("project-type")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut screenplay, false, "Novel");
                    ui.selectable_value(&mut screenplay, true, "Screenplay");
                });
            ui.ctx().accesskit_node_builder(combo.response.id, |node| {
                node.set_label("Project type");
                node.set_value(current);
            });
            draft.screenplay = screenplay;
            ui.label(
                RichText::new("Existing scenes keep their format; new scenes use the new type.")
                    .size(12.0)
                    .color(palette.text_muted),
            );
            ui.add_space(10.0);
            ui.label(RichText::new("Labels").strong().color(palette.text_heading));
            ui.label(
                RichText::new("Rename, recolour or remove the labels used on scenes and chapters")
                    .size(12.0)
                    .color(palette.text_muted),
            );
            manage = ui.button("Manage labels…").clicked();
            ui.add_space(10.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Project details are saved on this device.")
                        .size(12.0)
                        .color(palette.text_muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = editable && !draft.name.trim().is_empty();
                    save = ui
                        .add_enabled(ready, egui::Button::new(RichText::new("Save").strong()))
                        .clicked()
                        || (ready
                            && name.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                });
            });
        });
    if manage {
        // As ProseCraft, the Labels dialog opens over Project Settings.
        state.labels_dialog = state
            .library
            .as_ref()
            .and_then(|l| l.projects().get(state.active))
            .map(|p| crate::labels::LabelsDraft::for_project(&p.view));
    }
    if save {
        let intent = crate::editing::Intent::SetProjectDetails {
            name: draft.name.clone(),
            author: draft.author.clone(),
            screenplay: draft.screenplay,
        };
        state.intents.push(intent);
        state.project_settings = None;
    } else if !open
        || (state.labels_dialog.is_none() && ui.input(|i| i.key_pressed(egui::Key::Escape)))
    {
        state.project_settings = None;
    }
}
