//! ProseCraft's Scrivener import: a `.scriv` project read into a preview (its name and what the
//! import will create), then added as a new project.

use egui::{RichText, Ui};
use prosecraft_core::import::scrivener::{
    ScrivenerFiles, ScrivenerReport, build_scrivener_project, files_from_entries, files_from_zip,
};
use prosecraft_core::js::JsValue;

use crate::{AppState, ProseApp};

/// The project the import will add, before it is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScrivenerPreview {
    project: JsValue,
    report: ScrivenerReport,
    name: String,
}

impl ProseApp {
    /// A zipped `.scriv` project (`handleScrivenerZipImport`): read, then previewed.
    pub fn import_scrivener_zip(&mut self, bytes: &[u8]) {
        match files_from_zip(bytes) {
            Ok(files) => self.preview_scrivener(&files, ""),
            Err(error) => self.state.error(error.0),
        }
    }

    /// A `.scriv` folder's files, by their paths inside it (`handleScrivenerFolderImport`).
    pub fn import_scrivener_folder(&mut self, folder_name: &str, entries: Vec<(String, Vec<u8>)>) {
        match files_from_entries(entries) {
            Ok(files) => self.preview_scrivener(&files, folder_name),
            Err(error) => self.state.error(error.0),
        }
    }

    /// `openScrivenerPreview`.
    fn preview_scrivener(&mut self, files: &ScrivenerFiles, project_name: &str) {
        let Some(home) = self.home.as_mut() else {
            return;
        };
        let store = &mut home.store;
        let name = (!project_name.is_empty()).then_some(project_name);
        match build_scrivener_project(files, &mut || store.new_id(), name) {
            Ok((project, report)) => {
                let name = project
                    .get("name")
                    .and_then(|n| n.as_js_string())
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                self.state.scrivener = Some(ScrivenerPreview {
                    project,
                    report,
                    name,
                });
                self.state.clear_status();
            }
            Err(error) => self.state.error(error.0),
        }
    }

    /// `submitScrivenerImport`: the project, named as chosen, added and opened.
    pub(crate) fn finish_scrivener_import(&mut self, preview: ScrivenerPreview) {
        let mut project = preview.project;
        let name = preview.name.trim().to_owned();
        if !name.is_empty()
            && let Some(o) = project.as_object_mut()
        {
            o.insert("name", JsValue::str(&name));
        }
        let name = project
            .get("name")
            .and_then(|n| n.as_js_string())
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        let Some(saved) = self.add_and_open(|_, _| project) else {
            return;
        };
        let r = preview.report;
        let failed = saved.is_err();
        self.state.say(
            match saved {
                Ok(()) => format!(
                    "Imported “{name}”: {} chapter(s), {} scene(s), {} wiki page(s).",
                    r.chapters, r.scenes, r.wiki_pages
                ),
                Err(_) => "Could not save the imported project.".into(),
            },
            failed,
        );
    }
}

/// The preview dialog.
pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let Some(preview) = state.scrivener.as_mut() else {
        return;
    };
    let mut open = true;
    let mut import = false;
    egui::Window::new("Import project")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(440.0, 380.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            crate::widgets::eyebrow(ui, &palette, "Scrivener");
            ui.label(
                RichText::new("Project details")
                    .strong()
                    .color(palette.text_heading),
            );
            ui.label(
                RichText::new("Review what the import will create")
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
                    egui::TextEdit::singleline(&mut preview.name)
                        .char_limit(100)
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(label.id);
            let r = &preview.report;
            let mut lines = vec![
                format!("{} chapter(s)", r.chapters),
                format!("{} scene(s)", r.scenes),
                format!("{} wiki page(s)", r.wiki_pages),
                format!("{} wiki folder(s)", r.wiki_folders),
            ];
            if r.skipped_files > 0 {
                lines.push(format!(
                    "{} file(s) such as PDFs and images were skipped",
                    r.skipped_files
                ));
            }
            if r.items_with_notes > 0 {
                lines.push(format!(
                    "{} item(s) have notes that are not imported",
                    r.items_with_notes
                ));
            }
            if r.flattened_folders > 0 {
                lines.push(format!("{} folder(s) were flattened", r.flattened_folders));
            }
            if r.missing_files > 0 {
                lines.push(format!("{} file(s) could not be read", r.missing_files));
            }
            ui.add_space(6.0);
            for line in lines {
                ui.label(format!("• {line}"));
            }
            ui.add_space(10.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Only the manuscript and research are imported.")
                        .size(12.0)
                        .color(palette.text_muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = !preview.name.trim().is_empty();
                    import = ui
                        .add_enabled(ready, egui::Button::new(RichText::new("Import").strong()))
                        .clicked()
                        || (ready
                            && name.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                });
            });
        });
    if import {
        state.scrivener_done = state.scrivener.take();
    } else if !open || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.scrivener = None;
    }
}
