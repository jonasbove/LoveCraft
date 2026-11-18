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
