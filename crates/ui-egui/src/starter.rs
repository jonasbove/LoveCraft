//! ProseCraft's "Start from" dialog for a new project: what you are writing (a novel or a
//! screenplay), the wiki template, and the world-building modules to include. The new project
//! is ProseCraft's own new project (`core::starters::new_project`).

use egui::{RichText, Ui};
use prosecraft_core::starters::custom::CustomStarter;
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
    /// "Save as my template": the name being typed, and what the last save or delete said.
    template_name: String,
    notice: String,
    /// A template whose deletion is being asked about.
    confirm_delete: Option<String>,
}

impl StarterDraft {
    pub fn new() -> Self {
        let mut draft = Self {
            screenplay: false,
            preset: String::new(),
            working: Blueprint::default(),
            selected: Vec::new(),
            template_name: String::new(),
            notice: String::new(),
            confirm_delete: None,
        };
        draft.select_preset("blank", &[]);
        draft
    }

    /// `selectStarter`: a preset, or one of the user's own templates (`custom:<id>`).
    fn select_preset(&mut self, id: &str, templates: &[CustomStarter]) {
        self.preset = id.to_owned();
        self.notice.clear();
        self.working = if let Some(custom) = id.strip_prefix("custom:") {
            templates
                .iter()
                .find(|t| t.id == custom)
                .map(|t| t.blueprint.clone())
                .unwrap_or_default()
        } else if id == "blank" {
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
            self.select_preset("blank", &[]);
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

    /// `starterExtraFolders`: the template's folders that aren't modules.
    fn extra_folders(&self) -> Vec<BlueprintFolder> {
        self.working
            .folders
            .iter()
            .filter(|f| {
                !starters::modules()
                    .iter()
                    .any(|m| starters::module_folder_key(&m.id) == f.key)
            })
            .cloned()
            .collect()
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

    /// The last save didn't work.
    pub fn failed(&mut self, notice: &str) {
        self.notice = notice.to_owned();
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
    let templates = &state.custom_starters;
    let mut delete_template: Option<String> = None;
    let mut save_template: Option<CustomStarter> = None;
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
                                    draft.select_preset(&starter.id, templates);
                                }
                            }
                        });
                        ui.ctx()
                            .accesskit_node_builder(presets.response.id, |node| {
                                node.set_role(egui::accesskit::Role::RadioGroup);
                                node.set_label("Starter");
                            });
                        // "My templates": the user's own, saved on this device.
                        if !templates.is_empty() {
                            ui.add_space(6.0);
                            heading(ui, &palette, "My templates", "Saved on this device.");
                            let mine = ui.scope(|ui| {
                                for template in templates {
                                    let key = format!("custom:{}", template.id);
                                    ui.horizontal(|ui| {
                                        let picked = ui.vertical(|ui| option(ui, draft.preset == key, &template.name, &summary(&template.blueprint))).inner;
                                        if picked {
                                            draft.select_preset(&key, templates);
                                        }
                                        let delete = ui.small_button("🗑");
                                        crate::views::name_field(ui, &delete, &format!("Delete {}", template.name));
                                        if delete.clicked() {
                                            draft.confirm_delete = Some(template.id.clone());
                                        }
                                    });
                                    if draft.confirm_delete.as_deref() == Some(template.id.as_str()) {
                                        ui.label(format!("Delete the template “{}”? Projects created from it keep their pages.", template.name));
                                        ui.horizontal(|ui| {
                                            if ui.button(RichText::new("Delete").color(palette.danger)).clicked() {
                                                delete_template = Some(template.id.clone());
                                                draft.confirm_delete = None;
                                            }
                                            if ui.button("Cancel").clicked() {
                                                draft.confirm_delete = None;
                                            }
                                        });
                                    }
                                }
                            });
                            ui.ctx().accesskit_node_builder(mine.response.id, |node| {
                                node.set_role(egui::accesskit::Role::RadioGroup);
                                node.set_label("My templates");
                            });
                        }
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
                    // "Template content": the selected template's folders that aren't modules.
                    let extras = draft.extra_folders();
                    if !extras.is_empty() {
                        ui.add_space(8.0);
                        let included = extras.iter().filter(|f| draft.included(f)).count();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Template content").strong());
                            ui.label(RichText::new(format!("{included} of {} included", extras.len())).size(12.0).color(palette.text_muted));
                        });
                        ui.add(egui::Label::new(RichText::new("Folders from the selected template that aren't modules.").size(12.0).color(palette.text_muted)).wrap());
                        for folder in &extras {
                            let on = draft.included(folder);
                            let pages = if folder.pages.is_empty() { "Empty folder".to_owned() } else { folder.pages.iter().map(|p| p.title.as_str()).collect::<Vec<_>>().join(" · ") };
                            let button = ui.add(egui::Button::selectable(on, RichText::new(&folder.name).strong()).min_size(egui::vec2(260.0, 26.0))).on_hover_text(&pages);
                            let (label, description) = (folder.name.clone(), pages.clone());
                            ui.ctx().accesskit_node_builder(button.id, move |node| {
                                node.set_role(egui::accesskit::Role::Button);
                                node.set_label(label);
                                node.set_description(description);
                                node.set_toggled(if on { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False });
                            });
                            if button.clicked() {
                                draft.set_keys(StarterDraft::folder_keys(folder), !on);
                            }
                        }
                    }
                    // "Save as my template".
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let name = ui.add(egui::TextEdit::singleline(&mut draft.template_name).hint_text("Template name").desired_width(220.0));
                        crate::views::name_field(ui, &name, "Template name");
                        let ready = !draft.template_name.trim().is_empty();
                        let enter = name.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ui.add_enabled(ready, egui::Button::new("Save as my template")).clicked() || (ready && enter) {
                            // The app gives it its id when it saves it.
                            let id = String::new();
                            let name = draft.template_name.trim().to_owned();
                            save_template = Some(CustomStarter { id, name: name.clone(), blueprint: draft.blueprint() });
                            draft.template_name.clear();
                            draft.notice = format!("Saved as \"{name}\".");
                        }
                    });
                    if !draft.notice.is_empty() {
                        let notice = ui.label(RichText::new(&draft.notice).size(12.0).color(palette.text_muted));
                        let text = draft.notice.clone();
                        ui.ctx().accesskit_node_builder(notice.id, move |node| {
                            node.set_role(egui::accesskit::Role::Status);
                            node.set_label(text);
                        });
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    create = ui.button(RichText::new("Create").strong()).clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        });
    if let Some(template) = save_template {
        let mut next = state.custom_starters.clone();
        next.push(template);
        state.custom_starters_save = Some(next);
    }
    if let Some(id) = delete_template {
        let next: Vec<CustomStarter> = state
            .custom_starters
            .iter()
            .filter(|t| t.id != id)
            .cloned()
            .collect();
        if draft.preset == format!("custom:{id}") {
            draft.select_preset("blank", &next);
        }
        state.custom_starters_save = Some(next);
    }
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
        let Some(saved) = self.add_and_open(|store, existing| {
            starters::new_project(screenplay, existing, blueprint, &mut || store.new_id())
        }) else {
            return;
        };
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

    /// Adds the project `make` builds (from the store's ids and how many live projects there
    /// are) to the library, saves it and opens it at its active scene. `None` when there is no
    /// library to add to; else whether saving worked.
    pub(crate) fn add_and_open(
        &mut self,
        make: impl FnOnce(&mut Box<dyn crate::LibraryStore>, usize) -> prosecraft_core::js::JsValue,
    ) -> Option<Result<(), String>> {
        self.commit_draft();
        self.save_now();
        let home = self.home.as_mut()?;
        if home.library.is_read_only() {
            self.state.status = Some("This library has a project from a newer version of ProseCraft, so no project can be added.".into());
            return None;
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
        let data = make(&mut home.store, existing);
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
            .map(|s| s.to_string_lossy())
            .filter(|s| !s.is_empty());
        home.library.put(stored);
        let text = home.library.to_text(now);
        let saved = home.store.save(&text).map_err(|e| e.to_string());
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
        Some(saved)
    }

    /// `importEpubData`: the book becomes a new project, opened.
    pub fn import_epub(&mut self, name: &str, bytes: &[u8]) {
        use prosecraft_core::import::epub::{build_epub_project, parse_epub};
        let manuscript = match parse_epub(bytes, Some(name)) {
            Ok(m) => m,
            Err(error) => {
                self.state.status = Some(error.0);
                return;
            }
        };
        let Some(saved) =
            self.add_and_open(|store, _| build_epub_project(&manuscript, &mut || store.new_id()))
        else {
            return;
        };
        let scenes: usize = manuscript.chapters.iter().map(|c| c.scenes.len()).sum();
        let images = match manuscript.skipped_images {
            0 => String::new(),
            n => format!(", {n} image(s) skipped"),
        };
        let mut status = format!(
            "Imported “{}” — {} chapter(s), {scenes} scene(s){images}.",
            manuscript.title,
            manuscript.chapters.len()
        );
        if let Err(error) = saved {
            status.push_str(&format!(" Couldn't save your library: {error}"));
        }
        self.state.status = Some(status);
    }
}
