//! Changing projects: what the panes ask for, the scene being typed into, and saving.
//!
//! The panes push [`Intent`]s; the app applies them after the frame, each through
//! `prosecraft_core::edit`, then `stamp_project`, as ProseCraft's `saveVault` does, and saves the
//! library once nothing has changed for a moment. Typing goes into a [`Draft`] first and reaches
//! the project after a short pause, so a keystroke doesn't re-stamp a whole book.

use prosecraft_core::edit::{self, EditError};
use prosecraft_core::js::JsValue;
use prosecraft_core::storage::stamp::stamp_project;

/// A change the interface asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    AddChapter,
    AddScene {
        chapter: String,
    },
    TrashDocument(String),
    TrashChapter(String),
    RestoreDocument(String),
    RestoreChapter(String),
    SetChapterSummary {
        chapter: String,
        summary: String,
    },
    SetChapterTitle {
        chapter: String,
        title: String,
    },
    /// One of the project's `publishOptions`.
    SetPublishOption {
        key: String,
        value: JsValue,
    },
    /// Snapshots of these scenes (one, or a chapter's), at `now` (Unix milliseconds).
    TakeSnapshots {
        documents: Vec<String>,
        note: String,
        now: f64,
    },
    /// Restores a snapshot, keeping the current text as a snapshot first.
    RestoreSnapshot {
        id: String,
        now: f64,
    },
    DeleteSnapshot(String),
    /// A new comment on a passage of a scene.
    AddComment {
        document: String,
        anchor: prosecraft_core::comments::Anchor,
        body: String,
        author: String,
        now: f64,
    },
    ResolveComment {
        id: String,
        resolved: bool,
    },
    DeleteComment(String),
    /// Find and replace, confirmed: snapshots of the changed documents, then the replacement.
    ReplaceInProject {
        query: String,
        replacement: String,
        options: prosecraft_core::search::SearchOptions,
        only: Vec<String>,
        now: f64,
    },
    /// The Wiki: a new page (in a folder, or unfiled), a new folder, renaming and removing one,
    /// and moving a page to a folder (or unfiled).
    AddWikiPage {
        folder: Option<String>,
    },
    AddWikiFolder {
        parent: Option<String>,
    },
    RenameWikiFolder {
        id: String,
        title: String,
    },
    RemoveWikiFolder(String),
    MoveWikiPage {
        document: String,
        folder: Option<String>,
    },
    /// The Labels dialog's labels (new ones have `new:` ids), and one scene's or chapter's label.
    SaveLabels(Vec<prosecraft_core::project::Label>),
    SetLabel {
        chapter: bool,
        id: String,
        label: Option<String>,
    },
    /// The storyboard: a new card at a place (its title and note from its source), moving,
    /// removing, and a card's title or note (a chapter card's note is its summary).
    AddCard {
        kind: String,
        source: String,
        title: String,
        note: String,
        x: f64,
        y: f64,
    },
    MoveCard {
        id: String,
        x: f64,
        y: f64,
    },
    RemoveCard(String),
    /// A storyboard drawing: new (its id is made then), changed, removed.
    AddDrawing(prosecraft_core::storyboard::Drawing),
    SetDrawing(prosecraft_core::storyboard::Drawing),
    RemoveDrawing(String),
    SetCardTitle {
        id: String,
        title: String,
    },
    SetCardNote {
        id: String,
        note: String,
    },
    /// `convertToScreenplay`: a prose scene made a screenplay, a snapshot of the prose first.
    ConvertToScreenplay {
        document: String,
        now: f64,
    },
    /// Footnotes: a new one at a character of the open scene (handled with the editor's
    /// draft), a definition's text, removing one (marker and definition), and dropping
    /// definitions whose marker is gone.
    InsertFootnote {
        document: String,
        at: usize,
    },
    SetFootnote {
        document: String,
        id: String,
        html: String,
    },
    RemoveFootnote {
        document: String,
        id: String,
    },
    PruneFootnotes(String),
    /// The timeline: a new moment, one field of a moment, removing and moving one.
    AddMoment,
    SetMoment {
        id: String,
        field: prosecraft_core::timeline::Field,
        value: String,
    },
    RemoveMoment(String),
    MoveMoment {
        id: String,
        offset: isize,
    },
    /// The project's word count goal, and a chapter's.
    SetProjectGoal(f64),
    SetChapterGoal {
        chapter: String,
        goal: f64,
    },
    /// Project settings: name, author and type.
    SetProjectDetails {
        name: String,
        author: String,
        screenplay: bool,
    },
}

/// The scene or page being typed into.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub document: String,
    pub title: String,
    /// The scene's text and formatting.
    pub doc: prosecraft_core::richtext::RichDoc,
    /// The editor's text buffer (the document's text).
    pub body: String,
    /// Changed since the last commit to the project.
    pub title_dirty: bool,
    pub body_dirty: bool,
    /// When (in egui's seconds) the last keystroke came.
    pub changed_at: f64,
    /// The text's word count, and the writing activity (edits, words added) not yet recorded
    /// in the project (ProseCraft's `trackActivity`, applied when the draft commits).
    pub words: usize,
    pub edits: u32,
    pub words_added: f64,
}

/// Seconds after the last keystroke before the draft reaches the project (ProseCraft: 250 ms).
pub const COMMIT_AFTER: f64 = 0.25;
/// Seconds after the last change before the library is written.
pub const SAVE_AFTER: f64 = 1.0;

/// Applies `change` to project `id` in `library` and stamps it. Nothing changes on an error.
pub fn apply(
    library: &mut prosecraft_core::library::Library,
    project_id: &str,
    now_ms: f64,
    change: impl FnOnce(&mut JsValue) -> Result<(), EditError>,
) -> Result<(), EditError> {
    let previous = library
        .project(project_id)
        .cloned()
        .ok_or(EditError::Shape)?;
    let mut data = previous.get("project").cloned().ok_or(EditError::Shape)?;
    change(&mut data)?;
    let device = library.device_id.clone();
    let stamped = stamp_project(Some(&previous), data, &device, now_ms);
    library.put(stamped);
    Ok(())
}

/// The edit an intent stands for. New records take ids from `new_id`.
pub fn run(
    intent: &Intent,
    data: &mut JsValue,
    new_id: &mut dyn FnMut() -> String,
) -> Result<(), EditError> {
    match intent {
        Intent::AddChapter => edit::add_chapter(data, new_id(), new_id()).map(|_| ()),
        Intent::AddScene { chapter } => {
            let count = data
                .get("documents")
                .and_then(JsValue::as_array)
                .unwrap_or_default()
                .iter()
                .filter(|d| {
                    d.get("chapterId")
                        .and_then(JsValue::as_js_string)
                        .is_some_and(|c| c.eq_str(chapter))
                })
                .count();
            edit::add_scene(data, chapter, &format!("Scene {}", count + 1), new_id()).map(|_| ())
        }
        Intent::TrashDocument(id) => edit::trash_document(data, id),
        Intent::TrashChapter(id) => edit::trash_chapter(data, id),
        Intent::RestoreDocument(id) => edit::restore_document(data, id),
        Intent::RestoreChapter(id) => edit::restore_chapter(data, id, None),
        Intent::SetChapterSummary { chapter, summary } => {
            edit::set_chapter_summary(data, chapter, summary)
        }
        Intent::SetChapterTitle { chapter, title } => edit::set_chapter_title(data, chapter, title),
        Intent::SetPublishOption { key, value } => {
            edit::set_publish_option(data, key, value.clone())
        }
        Intent::TakeSnapshots {
            documents,
            note,
            now,
        } => prosecraft_core::snapshots::take_snapshots(data, documents, *now, note, new_id),
        Intent::RestoreSnapshot { id, now } => {
            prosecraft_core::snapshots::restore_snapshot(data, id, *now, new_id).map(|_| ())
        }
        Intent::DeleteSnapshot(id) => prosecraft_core::snapshots::delete_snapshot(data, id),
        Intent::AddComment {
            document,
            anchor,
            body,
            author,
            now,
        } => {
            let id = new_id();
            prosecraft_core::comments::add_comment(data, &id, document, anchor, body, author, *now)
        }
        Intent::ResolveComment { id, resolved } => {
            prosecraft_core::comments::set_resolved(data, id, *resolved)
        }
        Intent::DeleteComment(id) => prosecraft_core::comments::delete_comment(data, id),
        Intent::SetProjectDetails {
            name,
            author,
            screenplay,
        } => edit::set_project_details(data, name, author, *screenplay),
        Intent::AddWikiPage { folder } => {
            prosecraft_core::wiki::add_page(data, &new_id(), folder.as_deref())
        }
        Intent::AddWikiFolder { parent } => {
            prosecraft_core::wiki::add_folder(data, &new_id(), parent.as_deref())
        }
        Intent::RenameWikiFolder { id, title } => {
            prosecraft_core::wiki::rename_folder(data, id, title)
        }
        Intent::RemoveWikiFolder(id) => prosecraft_core::wiki::remove_folder(data, id),
        Intent::MoveWikiPage { document, folder } => {
            prosecraft_core::wiki::move_page(data, document, folder.as_deref())
        }
        Intent::SaveLabels(labels) => {
            let labels: Vec<_> = labels
                .iter()
                .map(|l| prosecraft_core::project::Label {
                    id: if l.id.starts_with("new:") {
                        format!("label-{}", new_id())
                    } else {
                        l.id.clone()
                    },
                    ..l.clone()
                })
                .collect();
            prosecraft_core::labels::save_labels(data, &labels)
        }
        Intent::SetLabel { chapter, id, label } => {
            prosecraft_core::labels::set_label(data, *chapter, id, label.as_deref())
        }
        Intent::AddCard {
            kind,
            source,
            title,
            note,
            x,
            y,
        } => prosecraft_core::storyboard::add_card(data, new_id, kind, source, title, note, *x, *y),
        Intent::MoveCard { id, x, y } => {
            prosecraft_core::storyboard::move_card(data, new_id, id, *x, *y)
        }
        Intent::RemoveCard(id) => prosecraft_core::storyboard::remove_card(data, new_id, id),
        Intent::AddDrawing(drawing) => {
            prosecraft_core::storyboard::add_drawing(data, new_id, drawing)
        }
        Intent::SetDrawing(drawing) => prosecraft_core::storyboard::set_drawing(data, drawing),
        Intent::RemoveDrawing(id) => prosecraft_core::storyboard::remove_drawing(data, id),
        Intent::SetCardTitle { id, title } => {
            prosecraft_core::storyboard::set_card_title(data, new_id, id, title)
        }
        Intent::SetCardNote { id, note } => {
            prosecraft_core::storyboard::set_card_note(data, new_id, id, note)
        }
        Intent::ConvertToScreenplay { document, now } => {
            prosecraft_core::snapshots::snapshot_before(
                data,
                document,
                &new_id(),
                *now,
                "before-restore",
                "Before screenplay conversion",
            )?;
            let docs = match data.as_object_mut().and_then(|o| o.get_mut("documents")) {
                Some(JsValue::Array(list)) => list,
                _ => return Err(prosecraft_core::edit::EditError::Shape),
            };
            let doc = docs
                .iter_mut()
                .find(|d| {
                    d.get("id")
                        .and_then(JsValue::as_js_string)
                        .is_some_and(|i| i.eq_str(document))
                })
                .and_then(JsValue::as_object_mut)
                .ok_or_else(|| prosecraft_core::edit::EditError::NoDocument(document.clone()))?;
            prosecraft_core::screenplay::convert::convert_document(doc);
            Ok(())
        }
        // Placed by the app, which holds the draft.
        Intent::InsertFootnote { .. } => Ok(()),
        Intent::SetFootnote { document, id, html } => {
            prosecraft_core::footnotes::update_definition(data, document, id, html)
        }
        Intent::RemoveFootnote { document, id } => {
            prosecraft_core::footnotes::remove(data, document, id)
        }
        Intent::PruneFootnotes(document) => {
            prosecraft_core::footnotes::prune_orphans(data, document)
        }
        Intent::AddMoment => prosecraft_core::timeline::add_node(data, &new_id()),
        Intent::SetMoment { id, field, value } => {
            prosecraft_core::timeline::set_field(data, id, *field, value)
        }
        Intent::RemoveMoment(id) => prosecraft_core::timeline::remove_node(data, id),
        Intent::MoveMoment { id, offset } => {
            prosecraft_core::timeline::move_node(data, id, *offset)
        }
        Intent::SetProjectGoal(goal) => edit::set_project_goal(data, *goal),
        Intent::SetChapterGoal { chapter, goal } => edit::set_chapter_goal(data, chapter, *goal),
        Intent::ReplaceInProject {
            query,
            replacement,
            options,
            only,
            now,
        } => prosecraft_core::search::replace_in_project(
            data,
            query,
            replacement,
            *options,
            only,
            *now,
            new_id,
        )
        .map(|_| ()),
    }
}

impl Draft {
    pub fn is_dirty(&self) -> bool {
        self.title_dirty || self.body_dirty
    }
}

/// The draft's edit: what changed of its title and its paragraphs (as ProseCraft's HTML). An
/// unchanged body is left alone, so a rename never rewrites the scene.
pub fn commit_draft(draft: &Draft, data: &mut JsValue, today: &str) -> Result<(), EditError> {
    if draft.edits > 0 && !today.is_empty() {
        edit::track_activity(data, today, f64::from(draft.edits), draft.words_added)?;
    }
    if draft.title_dirty {
        edit::set_document_title(data, &draft.document, &draft.title)?;
    }
    if draft.body_dirty {
        edit::set_document_content(data, &draft.document, &draft.doc.to_html())?;
    }
    Ok(())
}
