//! ProseCraft's Fountain import (`handleFountainImport`): a Fountain or plain-text screenplay
//! added to the open project as a screenplay scene.

use prosecraft_core::screenplay::fountain::parse_fountain;
use prosecraft_core::screenplay::layout::elements_to_html;

use crate::{ProseApp, Selection, WorkspaceView};

/// The file name without `.fountain` or `.txt`, trimmed.
fn fallback_title(name: &str) -> String {
    let lower = name.to_lowercase();
    let stem = [".fountain", ".txt"]
        .iter()
        .find(|e| lower.ends_with(*e))
        .map_or(name, |e| name.get(..name.len() - e.len()).unwrap_or(name));
    stem.trim_matches(prosecraft_core::html::is_js_whitespace)
        .to_owned()
}

impl ProseApp {
    /// The screenplay goes into the open scene's chapter, else the first chapter, else a new
    /// one; titled by its title page, else the file name, else "Screenplay".
    pub fn import_fountain(&mut self, name: &str, text: &str) {
        if self.state.library.is_none() {
            return;
        }
        self.commit_draft();
        let (title_page, elements) = parse_fountain(text);
        let page_title = title_page
            .iter()
            .find(|(k, _)| k == "Title")
            .map(|(_, v)| {
                v.trim_matches(prosecraft_core::html::is_js_whitespace)
                    .to_owned()
            })
            .unwrap_or_default();
        let fallback = fallback_title(name);
        let title = [page_title, fallback]
            .into_iter()
            .find(|t| !t.is_empty())
            .unwrap_or_else(|| "Screenplay".into());
        let content = elements_to_html(&elements);
        let chapter = self
            .state
            .library
            .as_ref()
            .and_then(|l| l.projects().get(self.state.active))
            .and_then(|p| {
                let open = match &self.state.selection {
                    Some(Selection::Document(id)) => {
                        p.view.document(id).and_then(|d| d.chapter_id.clone())
                    }
                    _ => None,
                };
                open.or_else(|| p.view.live_chapters().next().map(|c| c.id.clone()))
            });
        let mut scene = None;
        self.change(|data, new_id| {
            let chapter = match chapter {
                Some(c) => c,
                None => prosecraft_core::edit::add_chapter(data, new_id(), new_id())?.0,
            };
            scene = Some(prosecraft_core::edit::add_scene_with(
                data,
                &chapter,
                &title,
                new_id(),
                Some(&content),
                Some("screenplay"),
            )?);
            Ok(())
        });
        if let Some(scene) = scene {
            self.state.draft = None;
            self.state.selection = Some(Selection::Document(scene));
            self.state.view = WorkspaceView::Write;
            self.state.settings_open = false;
            self.state
                .info(format!("Imported “{title}” as a screenplay scene."));
        }
    }
}
