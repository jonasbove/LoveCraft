//! ProseCraft's Labels dialog ("Manage labels…"): rename a label, pick its colour, add or
//! delete one, or reset to the defaults. Changes apply to the project on Save.

use egui::{Color32, RichText, Ui};
use prosecraft_core::labels::{self, LABEL_COLORS, MAX_LABELS, NAME_LIMIT};
use prosecraft_core::project::Label;

use crate::AppState;

/// A pending confirmation in the dialog.
#[derive(Clone, Debug, PartialEq)]
enum Confirm {
    Delete(String),
    Reset,
}

/// The dialog's labels while it is open.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LabelsDraft {
    labels: Vec<Label>,
    confirm: Option<Confirm>,
    /// The label whose name takes the focus next frame (a new one).
    focus: Option<String>,
    /// New labels are `new:N` until saved, when they get their `label-…` id.
    added: usize,
}

impl LabelsDraft {
    /// `openLabelsDialog`.
    pub fn for_project(project: &prosecraft_core::project::Project) -> Self {
        Self {
            labels: project.labels.clone(),
            confirm: None,
            focus: None,
            added: 0,
        }
    }
}

/// A label's colour on screen (`labelColorCss`; unknown is faint text).
pub(crate) fn label_color(color: &str, faint: Color32) -> Color32 {
    labels::color_rgb(color).map_or(faint, |[r, g, b]| Color32::from_rgb(r, g, b))
}

/// A small coloured dot, as ProseCraft's label dots.
pub(crate) fn dot(ui: &mut Ui, color: Color32, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
    response
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let editable = state.editable;
    let Some(draft) = state.labels_dialog.as_mut() else {
        return;
    };
    let project = state
        .library
        .as_ref()
        .and_then(|l| l.projects().get(state.active))
        .map(|p| &p.view);
    let mut open = true;
    let mut save = false;
    egui::Window::new("Labels")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(600.0, 540.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.label(RichText::new("Scene and chapter labels").strong().color(palette.text_heading));
            ui.label(RichText::new("Rename a label, pick its colour or remove it. Changes apply to this project when you save.").size(12.0).color(palette.text_muted));
            ui.add_space(8.0);
            // Fixed heights, so the centred window keeps its size (and the rows stay put) as
            // labels and confirmations come and go.
            egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(340.0).min_scrolled_height(340.0).show(ui, |ui| {
                let mut delete = None;
                for (index, label) in draft.labels.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        dot(ui, label_color(&label.color, palette.text_muted), 10.0);
                        let name = ui.add(egui::TextEdit::singleline(&mut label.name).char_limit(NAME_LIMIT).desired_width(170.0));
                        crate::views::name_field(ui, &name, &format!("Name for label {}", index + 1));
                        if draft.focus.as_deref() == Some(label.id.as_str()) {
                            name.request_focus();
                            draft.focus = None;
                        }
                        let group = ui.scope(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for (key, [r, g, b]) in LABEL_COLORS {
                                let checked = label.color == key;
                                let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                                let swatch = ui.interact(rect, ui.scope_id().with(("label-swatch", &label.id, key)), egui::Sense::click());
                                let color = Color32::from_rgb(r, g, b);
                                ui.painter().circle_filled(rect.center(), 6.0, color);
                                if checked || swatch.has_focus() {
                                    ui.painter().circle_stroke(rect.center(), 8.5, egui::Stroke::new(1.5, palette.text_primary));
                                }
                                ui.ctx().accesskit_node_builder(swatch.id, move |node| {
                                    node.set_role(egui::accesskit::Role::RadioButton);
                                    node.set_label(key);
                                    node.set_toggled(if checked { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False });
                                });
                                if swatch.on_hover_text(key).clicked() {
                                    label.color = key.to_owned();
                                }
                            }
                        });
                        let group_name = format!("Colour for {}", if label.name.is_empty() { "label" } else { label.name.as_str() });
                        ui.ctx().accesskit_node_builder(group.response.id, move |node| {
                            node.set_role(egui::accesskit::Role::RadioGroup);
                            node.set_label(group_name);
                        });
                        let remove = ui.button(RichText::new("Delete").color(palette.danger));
                        let name = if label.name.is_empty() { "unnamed" } else { label.name.as_str() };
                        crate::views::name_field(ui, &remove, &format!("Delete label {name}"));
                        if remove.clicked() {
                            delete = Some(label.id.clone());
                        }
                    });
                }
                if let Some(id) = delete {
                    draft.confirm = Some(Confirm::Delete(id));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(draft.labels.len() < MAX_LABELS, egui::Button::new("Add label")).clicked() {
                        // `addLabelDraft`: a grey "New label", its name focused.
                        draft.added += 1;
                        let id = format!("new:{}", draft.added);
                        draft.labels.push(Label { id: id.clone(), name: "New label".into(), color: "grey".into() });
                        draft.focus = Some(id);
                    }
                    if ui.button("Reset to defaults").clicked() {
                        draft.confirm = Some(Confirm::Reset);
                    }
                });
            });
            // Confirmations, with ProseCraft's messages.
            ui.separator();
            let confirm_area = egui::vec2(ui.available_width(), 64.0);
            ui.allocate_ui_with_layout(confirm_area, egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_min_size(confirm_area);
            if let Some(confirm) = draft.confirm.clone() {
                let (message, action) = match &confirm {
                    Confirm::Delete(id) => {
                        let name = draft.labels.iter().find(|l| &l.id == id).map(|l| l.name.clone()).unwrap_or_default();
                        let scenes = project.map_or(0, |p| p.documents.iter().filter(|d| d.label_id.as_deref() == Some(id) && !d.trashed).count());
                        let chapters = project.map_or(0, |p| p.chapters.iter().filter(|c| c.label_id.as_deref() == Some(id) && !c.trashed).count());
                        (format!("Delete label “{name}”? {} and {} will lose it.", plural(scenes, "scene", "scenes"), plural(chapters, "chapter", "chapters")), "Delete label")
                    }
                    Confirm::Reset => ("Reset labels to the defaults? Custom labels are removed and the default names and colours are restored.".to_owned(), "Reset labels"),
                };
                ui.add(egui::Label::new(message).wrap());
                ui.horizontal(|ui| {
                    if ui.button(RichText::new(action).color(palette.danger)).clicked() {
                        match confirm {
                            Confirm::Delete(id) => draft.labels.retain(|l| l.id != id),
                            Confirm::Reset => draft.labels = labels::default_labels(),
                        }
                        draft.confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
                        draft.confirm = None;
                    }
                });
            }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Labels are saved with this project.").size(12.0).color(palette.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = editable && draft.confirm.is_none() && draft.labels.iter().all(|l| !l.name.trim().is_empty());
                    save = ui.add_enabled(ready, egui::Button::new(RichText::new("Save").strong())).clicked();
                });
            });
        });
    if save {
        state
            .intents
            .push(crate::editing::Intent::SaveLabels(draft.labels.clone()));
        state.labels_dialog = None;
    } else if !open || (draft.confirm.is_none() && ui.input(|i| i.key_pressed(egui::Key::Escape))) {
        state.labels_dialog = None;
    } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        draft.confirm = None;
    }
}
