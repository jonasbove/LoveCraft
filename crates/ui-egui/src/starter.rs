//! ProseCraft's "Start from" dialog for a new project: what you are writing (a novel or a
//! screenplay), the wiki template, and the world-building modules to include. The new project
//! is ProseCraft's own new project (`core::starters::new_project`).

use egui::{RichText, Ui};
use prosecraft_core::starters::{self, Blueprint, BlueprintFolder};

use crate::{AppState, Library, ProseApp, Selection, WorkspaceView};

/// The dialog's choices.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StarterDraft {
    pub screenplay: bool,
    /// The preset chosen ("blank", "fantasy"…).
    pub preset: String,
    /// The blueprint being customised, and the folder and page keys included.
    pub working: Blueprint,
    pub selected: Vec<String>,
}

impl StarterDraft {
    pub fn new() -> Self {
        let mut draft = Self {
            screenplay: false,
            preset: String::new(),
            working: Blueprint::default(),
            selected: Vec::new(),
        };
        draft.select_preset("blank");
        draft
    }

    /// `selectStarter`.
    fn select_preset(&mut self, id: &str) {
        self.preset = id.to_owned();
        self.working = if id == "blank" {
            Blueprint::default()
        } else {
            starters::starter_blueprint(id)
        };
        self.selected = starters::all_keys(&self.working);
    }

    /// `selectProjectKind`: a screenplay starts with Characters, Places and Items.
    fn select_kind(&mut self, screenplay: bool) {
        self.screenplay = screenplay;
        if screenplay {
            self.preset = "blank".into();
            self.working = starters::screenplay_blueprint();
            self.selected = starters::all_keys(&self.working);
        } else {
            self.select_preset("blank");
        }
    }

    fn folder_keys(folder: &BlueprintFolder) -> Vec<String> {
        std::iter::once(folder.key.clone())
            .chain(folder.pages.iter().map(|p| p.key.clone()))
            .collect()
    }

    fn included(&self, folder: &BlueprintFolder) -> bool {
        Self::folder_keys(folder)
            .iter()
            .all(|k| self.selected.contains(k))
    }

    fn set_keys(&mut self, keys: Vec<String>, included: bool) {
        if included {
            for key in keys {
                if !self.selected.contains(&key) {
                    self.selected.push(key);
                }
            }
        } else {
            self.selected.retain(|k| !keys.contains(k));
        }
    }

    /// `toggleStarterModule`: a module toggled off keeps its folder, so toggling it back
    /// restores it.
    fn toggle_module(&mut self, id: &str) {
        let key = starters::module_folder_key(id);
        match self.working.folders.iter().find(|f| f.key == key).cloned() {
            None => {
                self.working = starters::add_module(&self.working, id);
                if let Some(added) = self.working.folders.iter().find(|f| f.key == key).cloned() {
                    self.set_keys(Self::folder_keys(&added), true);
                }
            }
            Some(folder) => {
                let included = self.included(&folder);
                self.set_keys(Self::folder_keys(&folder), !included);
            }
        }
    }

    fn module_included(&self, id: &str) -> bool {
        let key = starters::module_folder_key(id);
        self.working
            .folders
            .iter()
            .find(|f| f.key == key)
            .is_some_and(|f| self.included(f))
    }

    /// `clearStarterModules`.
    fn clear_modules(&mut self) {
        let keys: Vec<String> = self
            .working
            .folders
            .iter()
            .filter(|f| f.key.starts_with("module:"))
            .flat_map(Self::folder_keys)
            .collect();
        self.set_keys(keys, false);
    }

    /// What the project will start with.
    pub fn blueprint(&self) -> Blueprint {
        starters::filter_blueprint(&self.working, &self.selected)
    }
}

/// `starterSummary`: "3 folders · 5 pages".
fn summary(blueprint: &Blueprint) -> String {
    let folders = blueprint.folders.len();
    let pages: usize = blueprint.folders.iter().map(|f| f.pages.len()).sum();
    format!(
        "{folders} {} · {pages} {}",
        if folders == 1 { "folder" } else { "folders" },
        if pages == 1 { "page" } else { "pages" }
    )
}

fn heading(ui: &mut Ui, palette: &crate::theme::Palette, title: &str, hint: &str) {
    ui.label(RichText::new(title).strong().color(palette.text_heading));
    ui.add(egui::Label::new(RichText::new(hint).size(12.0).color(palette.text_muted)).wrap());
    ui.add_space(6.0);
}

fn option(ui: &mut Ui, selected: bool, title: &str, text: &str) -> bool {
    let clicked = ui.radio(selected, RichText::new(title).strong()).clicked();
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        ui.add(egui::Label::new(RichText::new(text).size(12.0)).wrap());
    });
    ui.add_space(4.0);
    clicked
}

/// The dialog, while open.
pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let Some(draft) = state.starter.as_mut() else {
        return;
    };
    let mut open = true;
    let mut create = false;
    let mut cancel = false;
    egui::Window::new("Start from")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        // A fixed size: an auto-sized, centred window drifts while it grows, under the pointer.
        .fixed_size(egui::vec2(560.0, 640.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.set_width(540.0);
            egui::ScrollArea::vertical()
                .max_height(560.0)
                .show(ui, |ui| {
                    heading(
                        ui,
                        &palette,
                        "What are you writing?",
                        "Choose the format for new scenes in this project.",
                    );
                    let kinds = ui.scope(|ui| {
                        if option(
                            ui,
                            !draft.screenplay,
                            "Novel or story",
                            "Chapters and prose scenes, with genre templates.",
                        ) {
                            draft.select_kind(false);
                        }
                        if option(
                            ui,
                            draft.screenplay,
                            "Screenplay",
                            "Acts, industry script formatting, and Fountain/PDF export.",
                        ) {
                            draft.select_kind(true);
                        }
                    });
                    ui.ctx().accesskit_node_builder(kinds.response.id, |node| {
                        node.set_role(egui::accesskit::Role::RadioGroup);
                        node.set_label("What are you writing?");
                    });
                    ui.separator();
                    if !draft.screenplay {
                        heading(
                            ui,
                            &palette,
                            "Wiki template",
                            "Adds starter folders and template pages to the new project.",
                        );
                        let presets = ui.scope(|ui| {
                            for starter in starters::starters() {
                                if option(
                                    ui,
                                    draft.preset == starter.id,
                                    &starter.label,
                                    &starter.description,
                                ) {
                                    draft.select_preset(&starter.id);
                                }
                            }
                        });
                        ui.ctx()
                            .accesskit_node_builder(presets.response.id, |node| {
                                node.set_role(egui::accesskit::Role::RadioGroup);
                                node.set_label("Starter");
                            });
                        ui.separator();
                    }
                    heading(ui, &palette, "Customise", &summary(&draft.blueprint()));
                    let modules = starters::modules();
                    let included = modules
                        .iter()
                        .filter(|m| draft.module_included(&m.id))
                        .count();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Modules").strong());
                        ui.label(
                            RichText::new(format!("{included} of {} included", modules.len()))
                                .size(12.0)
                                .color(palette.text_muted),
                        );
                        if included > 0 && ui.small_button("Clear").clicked() {
                            draft.clear_modules();
                        }
                    });
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                "Choose the world-building modules to include in the new project.",
                            )
                            .size(12.0)
                            .color(palette.text_muted),
                        )
                        .wrap(),
                    );
                    ui.add_space(4.0);
                    egui::Grid::new("starter-modules")
                        .num_columns(2)
                        .spacing([8.0, 6.0])
                        .show(ui, |ui| {
                            for (index, module) in modules.iter().enumerate() {
                                let on = draft.module_included(&module.id);
                                let button = ui
                                    .add(
                                        egui::Button::selectable(
                                            on,
                                            RichText::new(&module.label).strong(),
                                        )
                                        .min_size(egui::vec2(260.0, 26.0)),
                                    )
                                    .on_hover_text(&module.description);
                                let (label, description) =
                                    (module.label.clone(), module.description.clone());
                                ui.ctx().accesskit_node_builder(button.id, move |node| {
                                    node.set_role(egui::accesskit::Role::Button);
                                    node.set_label(label);
                                    node.set_description(description);
                                    node.set_toggled(if on {
                                        egui::accesskit::Toggled::True
                                    } else {
                                        egui::accesskit::Toggled::False
                                    });
                                });
                                if button.clicked() {
                                    draft.toggle_module(&module.id);
                                }
                                if index % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                        });
                });
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    create = ui.button(RichText::new("Create").strong()).clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        });
    if create {
        state.create_project = Some((draft.screenplay, draft.blueprint()));
        state.starter = None;
    } else if cancel || !open || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.starter = None;
    }
}

impl ProseApp {
    /// `createProjectWithStarter`: the new project is added to the library, saved and opened at
    /// its first scene.
    pub(crate) fn create_project(&mut self, screenplay: bool, blueprint: &Blueprint) {
        self.commit_draft();
        self.save_now();
        let Some(home) = self.home.as_mut() else {
            return;
        };
        if home.library.is_read_only() {
            self.state.status = Some("This library has a project from a newer version of ProseCraft, so no project can be added.".into());
            return;
        }
        let existing = home
            .library
            .projects()
            .filter(|p| {
                !p.get("project")
                    .and_then(|d| d.get("trashed"))
                    .is_some_and(|t| matches!(t, prosecraft_core::js::JsValue::Bool(true)))
            })
            .count();
        let store = &mut home.store;
        let data = starters::new_project(screenplay, existing, blueprint, &mut || store.new_id());
        let now = home.store.now_ms();
        let stored = prosecraft_core::storage::stamp::create_stored_project(
            data,
            &home.library.device_id,
            now,
        );
        let id = stored
            .get("id")
            .and_then(|v| v.as_js_string())
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        let scene = stored
            .get("project")
            .and_then(|p| p.get("activeDocumentId"))
            .and_then(|v| v.as_js_string())
            .map(|s| s.to_string_lossy());
        home.library.put(stored);
        let text = home.library.to_text(now);
        let saved = home.store.save(&text);
        let views = Library::from_stored(home.library.projects().cloned().collect());
        self.state.active = views
            .projects()
            .iter()
            .position(|p| p.view.id == id)
            .unwrap_or(0);
        self.state.library = Some(views);
        self.state.draft = None;
        self.state.selection = scene.map(Selection::Document);
        self.state.view = WorkspaceView::Write;
        // As ProseCraft, Project Settings opens next, to name the project.
        if let Some(project) = self
            .state
            .library
            .as_ref()
            .and_then(|l| l.projects().get(self.state.active))
        {
            self.state.project_settings = Some(
                crate::project_settings::ProjectSettingsDraft::for_project(&project.view),
            );
        }
        self.state.status = Some(match saved {
            Ok(()) => "Created a new project.".into(),
            Err(error) => format!("Created a new project, but couldn't save your library: {error}"),
        });
    }
}
