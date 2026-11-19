//! The main panel: the view switch and the Outline, Write, Storyboard, Timeline and Publish views.

use egui::{RichText, Ui};
use prosecraft_core::edit::chapter_label;
use prosecraft_core::html::{PlainTextOptions, RichText as RichMode, html_to_plain_text};
use prosecraft_core::project::Project;
use prosecraft_core::richtext::{ATOM, HARD_BREAK, LineKind, Mark, RichDoc};

use crate::editing::{Draft, Intent};
use crate::theme::Palette;
use crate::widgets::{card, eyebrow, segmented};
use crate::{AppState, SaveState, Selection, WorkspaceView, fonts};

/// The writing column's widest measure, as ProseCraft's editor.
const COLUMN: f32 = 720.0;

/// Thousands separators, as ProseCraft shows counts (`toLocaleString` in English).
pub(crate) fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub(crate) fn words(n: usize) -> String {
    format!("{} {}", grouped(n), if n == 1 { "word" } else { "words" })
}

/// A chapter's label as ProseCraft shows it ("Chapter N" renumbered by place).
pub(crate) fn label_of(project: &Project, chapter_id: &str) -> String {
    let live: Vec<&prosecraft_core::project::Chapter> = project.live_chapters().collect();
    let Some(index) = live.iter().position(|c| c.id == chapter_id) else {
        return project
            .chapters
            .iter()
            .find(|c| c.id == chapter_id)
            .map(|c| c.title.clone())
            .unwrap_or_default();
    };
    let titles: Vec<&str> = live
        .iter()
        .take(index + 1)
        .map(|c| c.title.as_str())
        .collect();
    let title = titles.last().copied().unwrap_or_default();
    let label = chapter_label(title, &titles);
    if label.is_empty() {
        "Untitled chapter".to_owned()
    } else {
        label
    }
}

/// Lays `add` out in a centred column at most `COLUMN` wide.
fn column(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    let inner = width.min(COLUMN);
    ui.horizontal(|ui| {
        ui.add_space(((width - inner) / 2.0).max(0.0));
        ui.vertical(|ui| {
            ui.set_width(inner);
            add(ui);
        });
    });
}

/// A paste with HTML and no text in this frame, and the desktop host's reader for it.
type HtmlPasteFor<'a> = (
    &'a crate::clipboard_keys::HtmlPaste,
    Option<&'a crate::ClipboardHtml>,
);

/// The parts of the state a view changes, borrowed apart from the project it shows.
struct Edit<'a> {
    /// The project as stored, for the story data panel.
    stored: &'a prosecraft_core::js::JsValue,
    /// The story data panel open, for every scene.
    story_open: &'a mut bool,
    editable: bool,
    /// The host's clipboard HTML, for rich paste, and its writer, for rich copy.
    clipboard_html: Option<&'a crate::ClipboardHtml>,
    clipboard_write: Option<&'a crate::ClipboardWriter>,
    /// This frame's paste with HTML and no text, and the desktop host's reader for it.
    html_paste: HtmlPasteFor<'a>,
    /// The comments and Wiki articles highlighted in the text.
    marks: crate::text_marks::Sources<'a>,
    draft: &'a mut Option<Draft>,
    intents: &'a mut Vec<Intent>,
    save_state: &'a SaveState,
    now: f64,
    /// The editor's Comment button: the passage chosen, and the sidebar tab to show.
    pending_comment: &'a mut Option<crate::PendingComment>,
    /// Undo and redo, and the project and page they are kept for.
    undo: &'a mut prosecraft_core::undo::UndoManager<(usize, usize)>,
    project_id: String,
    undo_page: String,
    /// The Writing settings (text size, typewriter, fade, Zen), and where the text was drawn.
    writing: crate::WritingPrefs,
    editor_rect: &'a mut Option<egui::Rect>,
    side_tab: &'a mut crate::SideTab,
    /// A passage to select ("Go to" in the Comments tab): document, editor start and end.
    comment_goto: &'a mut Option<(String, usize, usize)>,
    /// Now, in milliseconds since the epoch (for snapshots).
    now_ms: f64,
    /// A footnote to open in the footnote editor: document and id.
    footnote_open: &'a mut Option<(String, String)>,
    /// The Markdown editor is enabled, on, and a switch asked for.
    markdown_enabled: bool,
    markdown_mode: bool,
    markdown_switch: &'a mut Option<bool>,
    /// The Markdown editor opened for content the rich editor can't hold, with the
    /// preference off.
    markdown_forced: &'a mut bool,
    /// The Sources dialog (opened from the toolbar's "Sources…").
    sources: &'a mut Option<crate::sources::SourcesDialog>,
    /// What citation markers are labelled with.
    cites: CiteStyle<'a>,
    /// The citation picker (opened from the toolbar, Ctrl+Alt+C, or a citation).
    citation_picker: &'a mut Option<crate::citation_picker::CitationPicker>,
    /// What divides scenes in the chapter view (the publish settings' scene separator).
    separator: prosecraft_core::publish::SceneSeparator,
    /// Another scene of the chapter clicked into: open it in the editor.
    open: Option<Selection>,
    /// The open scene's proofreading.
    proofing: &'a mut crate::proofreading::Proofing,
    /// The equation, diagram or calculation editor.
    rich_dialog: &'a mut Option<crate::rich_dialog::RichDialog>,
}

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    // Set again below, in the Write view only.
    crate::split_view::set_drop_grid(ui.ctx(), None);
    ui.add_space(14.0);
    // The Timeline view only when it is turned on (Settings → Writing).
    let views: Vec<WorkspaceView> = WorkspaceView::ALL
        .into_iter()
        .filter(|v| *v != WorkspaceView::Timeline || state.writing.timeline)
        .collect();
    let labels: Vec<&str> = views.iter().map(|v| v.label()).collect();
    let current = views.iter().position(|v| *v == state.view).unwrap_or(0);
    // Zen and focus mode hide the workspace controls too.
    if !state.zen_hidden {
        // Wrapped, so a narrow window puts the toggles on a second row instead of widening
        // the page under the side panels.
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(ui.available_width());
            ui.add_space(16.0);
            if let Some(index) = segmented(ui, &palette, "workspace-tab", &labels, current, 13.0) {
                state.view = views.get(index).copied().unwrap_or_default();
            }
            if state.library.is_some() {
                let search = ui.button("Search").on_hover_text("Search project (Ctrl+F)");
                name_field(ui, &search, "Search project");
                if search.clicked() {
                    state.find = Some(crate::find::FindState::new());
                }
            }
            // `toggleLeftSidebar`, `toggleRightSidebar` (not on the Storyboard) and
            // `toggleShortcuts`.
            if state.library.is_some() && state.view != WorkspaceView::Storyboard {
                for (hidden, show, hide, text) in [
                    (
                        state.binder_hidden,
                        "Show chapter sidebar",
                        "Hide chapter sidebar",
                        "Chapters",
                    ),
                    (
                        state.sidebar_hidden,
                        "Show progress sidebar",
                        "Hide progress sidebar",
                        "Progress",
                    ),
                ] {
                    let name = if hidden { show } else { hide };
                    let toggle = ui.selectable_label(!hidden, text).on_hover_text(name);
                    name_field(ui, &toggle, name);
                    if toggle.clicked() {
                        if text == "Chapters" {
                            state.binder_hidden = !state.binder_hidden;
                        } else {
                            state.sidebar_hidden = !state.sidebar_hidden;
                        }
                    }
                }
            }
            let shortcuts = ui
                .selectable_label(state.shortcuts.is_some(), "Shortcuts")
                .on_hover_text("Keyboard shortcuts (Ctrl/Cmd+/)");
            name_field(ui, &shortcuts, "Keyboard shortcuts");
            if shortcuts.clicked() {
                state.shortcuts = match state.shortcuts {
                    Some(_) => None,
                    None => Some(cfg!(target_os = "macos")),
                };
            }
            // `toggleSplitView`, in the Write view.
            if state.library.is_some() && state.view == WorkspaceView::Write {
                let open = !state.split.is_empty();
                let name = if open {
                    "Close split view"
                } else {
                    "Open split view"
                };
                let toggle = ui.selectable_label(open, "Split").on_hover_text(name);
                name_field(ui, &toggle, name);
                if toggle.clicked() {
                    state.split = if open {
                        Vec::new()
                    } else {
                        prosecraft_core::split::add(&[], &crate::split_view::pane_id())
                    };
                }
            }
        });
        if let Some(status) = &state.status {
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                ui.label(RichText::new(status).size(12.0).color(palette.text_muted));
            });
        }
    }
    let AppState {
        library,
        active,
        view,
        selection,
        draft,
        intents,
        editable,
        save_state,
        now,
        now_ms,
        book_id,
        cover_error,
        requests,
        pending_comment,
        side_tab,
        comment_goto,
        footnote_open,
        markdown_enabled,
        markdown_mode,
        markdown_switch,
        markdown_forced,
        sources,
        citation_picker,
        cite_labels,
        undo,
        undo_page,
        writing,
        editor_rect,
        proofing,
        rich_dialog,
        split,
        wiki_store,
        wiki_root,
        clipboard_html,
        clipboard_write,
        clipboard_keys,
        html_paste,
        tiles,
        network,
        fresh_ids,
        story_data_open,
        ..
    } = state;
    let entry = library.as_ref().and_then(|l| l.projects().get(*active));
    let Some(entry) = entry else {
        column(ui, |ui| welcome(ui, &palette));
        return;
    };
    let project = &entry.view;
    if *view == WorkspaceView::Storyboard {
        let data = entry
            .stored
            .get("project")
            .unwrap_or(&prosecraft_core::js::JsValue::Null);
        crate::storyboard::show(ui, &palette, data, project, *editable, intents);
        return;
    }
    if *view == WorkspaceView::Network {
        let derived = crate::graph_tile::derived(ui.ctx(), &entry.stored);
        let data = entry
            .stored
            .get("project")
            .unwrap_or(&prosecraft_core::js::JsValue::Null);
        let mut out = crate::network_view::Out {
            editable: *editable,
            intents,
            ids: fresh_ids,
            open: None,
        };
        // Full width, as ProseCraft's network view (not the writing column).
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(24, 0))
                    .show(ui, |ui| {
                        crate::network_view::show(ui, &palette, data, &derived, network, &mut out);
                    });
            });
        if let Some(selection) = out.open {
            state.select(selection);
        }
        return;
    }
    if *view == WorkspaceView::Publish {
        let mut edit = crate::publish::PublishEdit {
            editable: *editable,
            now_ms: *now_ms,
            book_id,
            intents,
            requests,
            cover_error,
            cover_mode: "none",
            generated_cover: None,
        };
        crate::publish::show(ui, &palette, &entry.stored, project, &mut edit);
        return;
    }
    let mut from_split = None;
    let split_open = *view == WorkspaceView::Write && !split.is_empty();
    let options = prosecraft_core::publish::PublishOptions::read(
        entry
            .stored
            .get("project")
            .and_then(|p| p.get("publishOptions")),
    );
    let mut edit = Edit {
        stored: entry
            .stored
            .get("project")
            .unwrap_or(&prosecraft_core::js::JsValue::Null),
        story_open: story_data_open,
        editable: *editable,
        clipboard_html: clipboard_html.as_ref(),
        clipboard_write: clipboard_write.as_ref(),
        html_paste: (html_paste, clipboard_keys.as_ref()),
        marks: crate::text_marks::Sources {
            comments: &project.comments,
            documents: &project.documents,
        },
        draft,
        intents,
        save_state,
        now: *now,
        pending_comment,
        undo,
        project_id: project.id.clone(),
        undo_page: undo_page.clone(),
        writing: *writing,
        editor_rect,
        side_tab,
        comment_goto,
        footnote_open,
        now_ms: *now_ms,
        markdown_enabled: *markdown_enabled,
        markdown_mode: *markdown_mode,
        markdown_switch,
        markdown_forced,
        sources,
        citation_picker,
        cites: CiteStyle {
            items: &project.citations,
            style: options.citation_style,
            locale: options.citation_locale,
            cache: cite_labels,
        },
        separator: options.scene_separator,
        proofing,
        rich_dialog,
        open: None,
    };
    let mut chosen = None;
    if *view == WorkspaceView::Write {
        crate::split_view::set_drop_grid(ui.ctx(), Some(ui.available_rect_before_wrap()));
    }
    let total = ui.available_size();
    let split_width = if split_open {
        (total.x * 0.45).max(320.0)
    } else {
        0.0
    };
    ui.horizontal_top(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let editor_width = (total.x - split_width - if split_open { gap } else { 0.0 }).max(200.0);
        ui.allocate_ui_with_layout(
            egui::vec2(editor_width, total.y),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(editor_width);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        column(ui, |ui| match view {
                            WorkspaceView::Outline => {
                                chosen = outline(ui, &palette, project, &mut edit)
                            }
                            WorkspaceView::Write => {
                                write(ui, &palette, project, selection.as_ref(), &mut edit)
                            }
                            // Shown above, as a full-width canvas.
                            WorkspaceView::Storyboard => {}
                            WorkspaceView::Timeline => timeline(ui, &palette, project, &mut edit),
                            WorkspaceView::Network => {}
                            // Shown above, in its own two-column layout.
                            WorkspaceView::Publish => {}
                        });
                    });
            },
        );
        if split_open {
            ui.allocate_ui_with_layout(
                egui::vec2(split_width, total.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(split_width);
                    ui.set_max_width(split_width);
                    let options = prosecraft_core::publish::PublishOptions::read(
                        entry
                            .stored
                            .get("project")
                            .and_then(|p| p.get("publishOptions")),
                    );
                    let files = crate::split_view::Files {
                        store: wiki_store.as_deref(),
                        root: wiki_root.clone().filter(|r| !r.is_empty()),
                        folder: prosecraft_core::research::normalize_folder_name(
                            entry
                                .stored
                                .get("project")
                                .and_then(|p| p.get("researchFolderName")),
                        ),
                        citation_style: options.citation_style.clone(),
                        citation_locale: options.citation_locale.clone(),
                        stored: &entry.stored,
                    };
                    let active = edit.draft.as_ref().map(|d| d.document.clone());
                    let asked = crate::split_view::show(
                        ui,
                        &palette,
                        project,
                        split,
                        active.as_deref(),
                        options.scene_separator,
                        &files,
                        &mut crate::split_view::pane_id,
                        tiles,
                    );
                    from_split = asked.open;
                    if let (Some(page), Some(scene)) = (&asked.confirm, active.as_deref()) {
                        edit.intents.push(Intent::ConfirmCharacter {
                            scene: scene.to_owned(),
                            page: page.clone(),
                        });
                    }
                    if let Some(page) = &asked.beside {
                        edit.intents
                            .push(Intent::OpenBeside(prosecraft_core::split::doc_target(page)));
                    }
                    if let Some(file) = asked.open_file {
                        edit.intents.push(Intent::OpenWikiFileExternally(file));
                    }
                    if edit.editable {
                        for (document, title, content) in asked.patches {
                            edit.intents.push(Intent::PatchTile {
                                document,
                                title,
                                content,
                            });
                        }
                        for (document, content) in asked.scenes {
                            edit.intents
                                .push(Intent::SetSceneText { document, content });
                        }
                        for (chapter, title) in asked.chapter_titles {
                            edit.intents
                                .push(Intent::SetChapterTitle { chapter, title });
                        }
                    }
                },
            );
        }
    });
    if let Some(selection) = chosen.or(edit.open.take()).or(from_split) {
        state.select(selection);
    }
}

fn welcome(ui: &mut Ui, palette: &Palette) {
    ui.add_space(64.0);
    eyebrow(ui, palette, "prosecraft-rs");
    ui.label(
        RichText::new("Outline. Write. Publish.")
            .font(fonts::serif(38.0))
            .color(palette.text_heading),
    );
    ui.add_space(12.0);
    // The desktop app also opens a backup given on the command line.
    let how = if cfg!(target_arch = "wasm32") {
        "Create a project with Create project, or open a ProseCraft backup: drop a .prosecraft file on this page, or use Open backup."
    } else {
        "Create a project with Create project, or open a ProseCraft backup: drop a .prosecraft file on this window, or start prosecraft-rs with the file's path."
    };
    ui.label(RichText::new(how).size(14.0).color(palette.text_body));
    ui.add_space(18.0);
    ui.label(
        RichText::new(prosecraft_core::about::credit_line())
            .size(12.0)
            .color(palette.text_muted),
    );
}

/// Gives a text field an accessible name (its placeholder isn't one).
pub(crate) fn name_field(ui: &Ui, response: &egui::Response, name: &str) {
    let name = name.to_owned();
    ui.ctx()
        .accesskit_node_builder(response.id, move |node| node.set_label(name));
}

fn outline(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    edit: &mut Edit<'_>,
) -> Option<Selection> {
    ui.add_space(28.0);
    eyebrow(ui, palette, "Outline");
    ui.label(
        RichText::new(project.name.as_str())
            .font(fonts::serif(32.0))
            .color(palette.text_heading),
    );
    let chapters = project.live_chapters().count();
    let noun = if chapters == 1 { "chapter" } else { "chapters" };
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "{} in {chapters} {noun}",
                words(project.word_count())
            ))
            .color(palette.text_muted),
        );
        if edit.editable && ui.button("Add chapter").clicked() {
            edit.intents.push(Intent::AddChapter);
        }
    });
    ui.add_space(18.0);
    let mut open = None;
    for chapter in project.live_chapters() {
        let label = label_of(project, &chapter.id);
        card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let text = RichText::new(label.as_str())
                    .font(fonts::serif(19.0))
                    .color(palette.text_heading);
                if ui.add(egui::Button::new(text).frame(false)).clicked() {
                    open = Some(chapter.id.clone());
                }
                if let Some(tag) = chapter.label_id.as_deref().and_then(|id| project.label(id)) {
                    ui.label(
                        RichText::new(tag.name.as_str())
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                }
            });
            let scenes = project.scenes_in_chapter(&chapter.id).count();
            let meta = format!(
                "{scenes} {} · {}",
                if scenes == 1 { "scene" } else { "scenes" },
                words(project.chapter_word_count(&chapter.id))
            );
            ui.label(RichText::new(meta).size(12.0).color(palette.text_muted));
            ui.add_space(4.0);
            if edit.editable {
                // ProseCraft's chapter summary field; the change is kept when the field is left.
                let id = ui.scope_id().with(("summary", &chapter.id));
                let mut summary = ui
                    .data_mut(|d| d.get_temp::<String>(id))
                    .unwrap_or_else(|| chapter.summary.clone().unwrap_or_default());
                let response = ui.add(
                    egui::TextEdit::multiline(&mut summary)
                        .hint_text("What happens in this chapter?")
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                name_field(ui, &response, &format!("Summary of {label}"));
                if response.changed() {
                    ui.data_mut(|d| d.insert_temp(id, summary.clone()));
                }
                if response.lost_focus() {
                    ui.data_mut(|d| d.remove::<String>(id));
                    if summary != chapter.summary.clone().unwrap_or_default() {
                        edit.intents.push(Intent::SetChapterSummary {
                            chapter: chapter.id.clone(),
                            summary,
                        });
                    }
                }
            } else if let Some(summary) = &chapter.summary {
                ui.label(RichText::new(summary.as_str()).color(palette.text_body));
            }
        });
        ui.add_space(10.0);
    }
    open.map(Selection::Chapter)
}

fn plain(html: &str) -> String {
    html_to_plain_text(
        html,
        &PlainTextOptions {
            rich: RichMode::Source,
            ..Default::default()
        },
    )
}

fn body_format(palette: &Palette) -> egui::TextFormat {
    egui::TextFormat {
        font_id: fonts::serif(17.0),
        color: palette.text_body,
        line_height: Some(17.0 * 1.85),
        ..Default::default()
    }
}

/// The base direction of every paragraph of scene text. ProseCraft's editor sets no `dir`, so a
/// browser gives each paragraph the page's left-to-right base direction and aligns it to the
/// left: Arabic or Hebrew words are ordered right-to-left within the line, and a line mixing
/// them with left-to-right words orders its runs left to right (UAX #9 with paragraph level 0).
pub(crate) const SCENE_DIRECTION: egui::epaint::text::TextDirection =
    egui::epaint::text::TextDirection::LeftToRight;

/// The manuscript text, read-only: serif, ProseCraft's 1.85 line spacing, a gap between paragraphs.
fn manuscript(ui: &mut Ui, palette: &Palette, text: &str) {
    for paragraph in text.split("\n\n") {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = ui.available_width();
        job.direction = SCENE_DIRECTION;
        job.append(paragraph, 0.0, body_format(palette));
        ui.label(job);
        ui.add_space(8.0);
    }
}

fn save_text(state: &SaveState) -> String {
    match state {
        SaveState::Saved => "All changes saved".to_owned(),
        SaveState::Pending => "Saving…".to_owned(),
        SaveState::Failed(error) => format!("Couldn't save: {error}"),
    }
}

fn footer(ui: &mut Ui, palette: &Palette, words_count: usize, right: &str) {
    ui.add_space(24.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(words(words_count))
                .size(12.0)
                .color(palette.text_muted),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(right).size(12.0).color(palette.text_muted));
        });
    });
}

fn write(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    selection: Option<&Selection>,
    edit: &mut Edit<'_>,
) {
    ui.add_space(22.0);
    match selection {
        Some(Selection::Document(id)) => match project.document(id) {
            Some(document) => {
                let place = document
                    .chapter_id
                    .as_deref()
                    .map_or_else(|| "Wiki".to_owned(), |c| label_of(project, c));
                let kind = if document.is_wiki() { "Page" } else { "Scene" };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(place).size(12.0).color(palette.text_muted));
                    ui.label(RichText::new("·").size(12.0).color(palette.text_muted));
                    eyebrow(ui, palette, kind);
                    if edit.editable && !document.is_wiki() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Move to Trash").clicked() {
                                edit.intents
                                    .push(Intent::TrashDocument(document.id.clone()));
                            }
                        });
                    }
                });
                ui.add_space(10.0);
                // `screenplayActive`: a screenplay scene, or any scene of a screenplay project.
                let screenplay = !document.is_wiki()
                    && (document.screenplay
                        || project.kind == prosecraft_core::project::ProjectKind::Screenplay);
                // The Markdown editor edits any scene's source, even one the rich editor can't
                // hold (a table, a code block); a screenplay keeps its own editor.
                if edit.markdown_mode && edit.editable && !screenplay {
                    // A screenplay scene can't go through Markdown (its elements would be lost),
                    // so a chapter holding one edits only the open scene's Markdown.
                    let chapter = chapter_view(project, document).filter(|c| {
                        !c.screenplay_project
                            && c.before.iter().chain(&c.after).all(|s| !s.screenplay)
                    });
                    markdown_editor(ui, palette, document, chapter, edit);
                    return;
                }
                // A scene the editor can hold only in a normalized shape opens in it too; that
                // shape is stored only once the text is edited (see `Draft::source`).
                let held = RichDoc::from_html(&document.content)
                    .or_else(|| RichDoc::from_html_normalized(&document.content));
                match held.filter(|_| edit.editable) {
                    Some(doc) => {
                        let chapter = chapter_view(project, document);
                        editor(ui, palette, document, doc, screenplay, chapter, edit);
                    }
                    None => {
                        ui.label(
                            RichText::new(document.title.as_str())
                                .font(fonts::serif(38.0))
                                .color(palette.text_heading),
                        );
                        // Content the rich editor can't hold yet (a table, a code block, a
                        // line break) can still be edited as Markdown.
                        if edit.editable {
                            let markdown = ui
                                .button("Edit as Markdown")
                                .on_hover_text("Open this scene in the Markdown editor");
                            if markdown.clicked() {
                                *edit.markdown_forced = true;
                                *edit.markdown_switch = Some(true);
                            }
                        }
                        ui.add_space(18.0);
                        manuscript(ui, palette, &plain(&document.content));
                        let why = if edit.editable {
                            "Read-only for now: this scene has content the editor can't change yet"
                        } else {
                            "Read-only preview"
                        };
                        footer(ui, palette, document.word_count(), why);
                    }
                }
            }
            None => {
                ui.label("That document is no longer in this project.");
            }
        },
        Some(Selection::Chapter(id)) => {
            eyebrow(ui, palette, "Chapter");
            let label = label_of(project, id);
            if edit.editable {
                let stored = project
                    .chapters
                    .iter()
                    .find(|c| &c.id == id)
                    .map(|c| c.title.clone())
                    .unwrap_or_default();
                chapter_title(ui, palette, id, &label, &stored, edit.intents);
                ui.horizontal(|ui| {
                    if ui.button("Add scene").clicked() {
                        edit.intents.push(Intent::AddScene {
                            chapter: id.clone(),
                        });
                    }
                    // `trashChapter` asks first, as from the binder.
                    ui.menu_button(
                        RichText::new("Move chapter to Trash").color(palette.danger),
                        |ui| {
                            let count = project.scenes_in_chapter(id).count();
                            let scenes = if count == 1 { "scene" } else { "scenes" };
                            ui.label(format!(
                                "Move “{label}” and its {count} {scenes} to Trash? You can restore them later."
                            ));
                            if ui
                                .button(RichText::new("Move to Trash").color(palette.danger))
                                .clicked()
                            {
                                edit.intents.push(Intent::TrashChapter(id.clone()));
                                ui.close();
                            }
                        },
                    );
                });
            } else {
                ui.label(
                    RichText::new(label)
                        .font(fonts::serif(38.0))
                        .color(palette.text_heading),
                );
            }
            ui.add_space(18.0);
            for (index, scene) in project.scenes_in_chapter(id).enumerate() {
                if index > 0 {
                    ui.add_space(6.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("⁂").size(16.0).color(palette.text_muted))
                    });
                    ui.add_space(6.0);
                }
                ui.label(
                    RichText::new(scene.title.as_str())
                        .font(fonts::serif(22.0))
                        .color(palette.text_heading),
                );
                ui.add_space(8.0);
                manuscript(ui, palette, &plain(&scene.content));
            }
            footer(
                ui,
                palette,
                project.chapter_word_count(id),
                "Open a scene to write in it",
            );
        }
        None => {
            ui.add_space(40.0);
            let hint = RichText::new("Choose a chapter, scene or Wiki page in the binder.")
                .font(fonts::serif(20.0))
                .color(palette.text_muted);
            ui.label(hint);
        }
    }
}

/// The chapter's title, kept when the field is left. An automatically numbered chapter shows
/// its number until it is given a name of its own.
fn chapter_title(
    ui: &mut Ui,
    palette: &Palette,
    id: &str,
    label: &str,
    stored: &str,
    intents: &mut Vec<Intent>,
) {
    let key = egui::Id::unique(("chapter-title", id));
    let mut title = ui
        .data_mut(|d| d.get_temp::<String>(key))
        .unwrap_or_else(|| label.to_owned());
    let response = ui.add(
        egui::TextEdit::singleline(&mut title)
            .font(fonts::serif(38.0))
            .text_color(palette.text_heading)
            .frame(egui::Frame::NONE)
            .desired_width(f32::INFINITY),
    );
    name_field(ui, &response, "Chapter title");
    if response.changed() {
        ui.data_mut(|d| d.insert_temp(key, title.clone()));
    }
    if response.lost_focus() {
        ui.data_mut(|d| d.remove::<String>(key));
        if title != label && title != stored {
            intents.push(Intent::SetChapterTitle {
                chapter: id.to_owned(),
                title,
            });
        }
    }
}

/// ProseCraft's chapter view: a scene of a chapter is written among the chapter's other
/// scenes, which show above and below it with a scene divider between each.
struct ChapterView<'p> {
    id: String,
    label: String,
    title: String,
    before: Vec<&'p prosecraft_core::project::Document>,
    after: Vec<&'p prosecraft_core::project::Document>,
    /// The other scenes are screenplay scenes (a screenplay project).
    screenplay_project: bool,
    /// The words of the other scenes.
    words: usize,
}

/// The chapter view for `document`, when it is a live scene of a live chapter.
fn chapter_view<'p>(
    project: &'p Project,
    document: &'p prosecraft_core::project::Document,
) -> Option<ChapterView<'p>> {
    let id = document
        .chapter_id
        .as_deref()
        .filter(|_| !document.is_wiki())?;
    let chapter = project.live_chapters().find(|c| c.id == id)?;
    let scenes: Vec<_> = project.scenes_in_chapter(id).collect();
    let at = scenes.iter().position(|s| s.id == document.id)?;
    let words = scenes
        .iter()
        .filter(|s| s.id != document.id)
        .map(|s| s.word_count())
        .sum();
    Some(ChapterView {
        id: id.to_owned(),
        label: label_of(project, id),
        title: chapter.title.clone(),
        before: scenes[..at].to_vec(),
        after: scenes[at + 1..].to_vec(),
        screenplay_project: project.kind == prosecraft_core::project::ProjectKind::Screenplay,
        words,
    })
}

/// The editor's text field for a scene (also where a click in the chapter view puts the caret).
fn body_id(document: &str) -> egui::Id {
    egui::Id::unique(("scene-body", document))
}

/// A scene divider, as `sceneBreakHtml` shows it for the publish separator.
fn scene_divider(
    ui: &mut Ui,
    palette: &Palette,
    separator: prosecraft_core::publish::SceneSeparator,
) {
    use prosecraft_core::publish::SceneSeparator;
    ui.add_space(14.0);
    match separator {
        SceneSeparator::Line => {
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 12.0), egui::Sense::hover());
            let middle = rect.center();
            ui.painter().hline(
                (middle.x - 48.0)..=(middle.x + 48.0),
                middle.y,
                egui::Stroke::new(1.0, palette.border_control),
            );
        }
        SceneSeparator::Blank | SceneSeparator::None => {
            ui.add_space(12.0);
        }
        _ => {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(prosecraft_core::chapter::scene_break_glyph(separator))
                        .font(fonts::serif(17.0))
                        .color(palette.text_muted),
                );
            });
        }
    }
    ui.add_space(14.0);
}

/// Proofreading over the editor: the underlines, a click on one opening its suggestions
/// (`openHarperPopover`), and a suggestion replacing the passage (`applyHarperSuggestion`).
fn proofreading_marks(
    ui: &mut Ui,
    palette: &Palette,
    (proofing, editable, now): (&mut crate::proofreading::Proofing, bool, f64),
    draft: &mut Draft,
    output: &egui::text_edit::TextEditOutput,
) {
    let lints = proofing.current(draft).to_vec();
    if lints.is_empty() {
        proofing.popover = None;
        return;
    }
    crate::proofreading::paint(
        ui,
        &lints,
        &output.galley,
        output.galley_pos,
        output.text_clip_rect,
    );
    if output.response.clicked()
        && let Some(caret) = output.cursor_range.map(|r| r.primary.index.0)
    {
        proofing.popover = lints
            .iter()
            .position(|l| l.range.start <= caret && caret <= l.range.end);
    }
    let Some(index) = proofing.popover else {
        return;
    };
    let Some(shown) = lints.get(index) else {
        proofing.popover = None;
        return;
    };
    let at = output
        .galley
        .pos_from_cursor(egui::text::CCursor::new(shown.range.start))
        .translate(output.galley_pos.to_vec2())
        .left_bottom()
        + egui::vec2(0.0, 8.0);
    match crate::proofreading::popover(ui.ctx(), palette, index, shown, at) {
        crate::proofreading::PopoverAction::None => {}
        crate::proofreading::PopoverAction::Close => proofing.popover = None,
        crate::proofreading::PopoverAction::Ignore(i) => proofing.ignore(i),
        crate::proofreading::PopoverAction::Apply(suggestion) => {
            proofing.popover = None;
            if !editable {
                return;
            }
            let chars: Vec<char> = draft.body.chars().collect();
            let range = shown.range.start.min(chars.len())..shown.range.end.min(chars.len());
            let mut text: String = chars[..range.start].iter().collect();
            text.push_str(&suggestion);
            text.extend(&chars[range.end..]);
            let after = range.start + suggestion.chars().count();
            draft.body = draft.doc.apply_text_at(&text, Some(after));
            draft.body_dirty = true;
            draft.changed_at = now;
            draft.caret = Some(after);
            draft.words = prosecraft_core::html::html_word_count(&draft.doc.to_html());
        }
    }
}

/// The pictures in the text: each one an image named by its alternative text, a click on one
/// selecting it (`selectEditorImage`) and anywhere else in the text letting go of it, and the
/// selected one's outline and resize handle.
fn picture_marks(
    ui: &mut Ui,
    palette: &Palette,
    document: &str,
    draft: &mut Draft,
    markers: &[(egui::Rect, Clicked)],
    output: &egui::text_edit::TextEditOutput,
    (body_id, editable, now): (egui::Id, bool, f64),
) {
    // The selected picture's outline and resize handle.
    if let Some(selected) = crate::picture_tools::selected(ui.ctx(), document)
        && let Some((rect, _)) = markers
            .iter()
            .find(|(_, c)| matches!(c, Clicked::Picture(a) if *a == selected))
        && editable
    {
        let column = output.galley.job.wrap.max_width;
        let column = if column.is_finite() {
            column
        } else {
            output.text_clip_rect.width()
        };
        crate::picture_tools::handle(ui, palette, document, draft, *rect, column, now);
    }
    // A click on a picture selects it (`selectEditorImage`); anywhere else in the text, it
    // lets go of it. Each picture is an image named by its alternative text, as its `<img>` is.
    if output.response.clicked() {
        let on = output.response.interact_pointer_pos().and_then(|pos| {
            markers.iter().find_map(|(rect, c)| match c {
                Clicked::Picture(atom) if rect.contains(pos) => Some(*atom),
                _ => None,
            })
        });
        crate::picture_tools::select(ui.ctx(), document, on.filter(|_| editable));
    }
    for (rect, clicked) in markers {
        let Clicked::Picture(atom) = clicked else {
            continue;
        };
        let response = ui.interact(
            rect.intersect(ui.clip_rect()),
            body_id.with(("picture", *atom)),
            egui::Sense::click(),
        );
        if response.clicked() && editable {
            crate::picture_tools::select(ui.ctx(), document, Some(*atom));
        }
        let alt = draft
            .doc
            .atoms
            .get(*atom)
            .and_then(|html| crate::pictures::info(html))
            .map(|p| p.alt)
            .unwrap_or_default();
        ui.ctx().accesskit_node_builder(response.id, move |node| {
            node.set_role(egui::accesskit::Role::Image);
            node.set_label(alt);
        });
    }
}

/// A passage to select: its document and characters.
type GoTo = Option<(String, usize, usize)>;

/// The comment and Wiki-mention highlights, and what their cards asked for.
fn text_marks(
    ui: &mut Ui,
    palette: &Palette,
    document: &prosecraft_core::project::Document,
    draft: &Draft,
    output: &egui::text_edit::TextEditOutput,
    (marks, comment_goto, intents): (&crate::text_marks::Sources<'_>, &mut GoTo, &mut Vec<Intent>),
) {
    match crate::text_marks::show(
        ui,
        palette,
        marks,
        &document.id,
        &draft.doc,
        &draft.body,
        output,
        ui.input(|i| i.time),
    ) {
        crate::text_marks::Asked::None => {}
        crate::text_marks::Asked::GoTo(start, end) => {
            *comment_goto = Some((document.id.clone(), start, end));
        }
        crate::text_marks::Asked::Resolve(id) => {
            intents.push(Intent::ResolveComment { id, resolved: true });
        }
    }
}

/// `handleEditorClipboard`: a copy or cut of a selection in the rich editor puts its HTML (with
/// its formatting, footnote markers and citations) and its plain text on the clipboard, as a
/// browser copies from ProseCraft's editor; a cut then takes the selection out.
fn copy(
    ui: &mut Ui,
    body_id: egui::Id,
    draft: &mut Draft,
    write: Option<&crate::ClipboardWriter>,
    now: f64,
) {
    let Some(write) = write.filter(|_| ui.memory(|m| m.has_focus(body_id))) else {
        return;
    };
    let (copy, cut) = ui.input(|i| {
        (
            i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
            i.events.iter().any(|e| matches!(e, egui::Event::Cut)),
        )
    });
    if !copy && !cut {
        return;
    }
    let Some(range) = egui::TextEdit::load_state(ui.ctx(), body_id)
        .and_then(|s| s.cursor.char_range())
        .map(|r| {
            let (a, b) = (r.primary.index.0, r.secondary.index.0);
            a.min(b)..a.max(b)
        })
        .filter(|r| !r.is_empty())
    else {
        return;
    };
    let html = prosecraft_core::html::sanitize_html(&draft.doc.slice_html(range.clone()));
    write(&html, &clipboard_text(&html));
    ui.input_mut(|i| {
        i.events
            .retain(|e| !matches!(e, egui::Event::Copy | egui::Event::Cut))
    });
    if cut {
        let chars: Vec<char> = draft.body.chars().collect();
        let kept: String = chars[..range.start]
            .iter()
            .chain(&chars[range.end..])
            .collect();
        draft.body = draft.doc.apply_text_at(&kept, Some(range.start));
        draft.body_dirty = true;
        draft.changed_at = now;
        draft.caret = Some(range.start);
        draft.words = prosecraft_core::html::html_word_count(&draft.doc.to_html());
        let mut state = egui::TextEdit::load_state(ui.ctx(), body_id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(range.start),
            )));
        state.store(ui.ctx(), body_id);
    }
}

/// The plain text a copy puts beside its HTML, as the browser writes it for a selection in
/// ProseCraft's editor: the text, an equation's or a diagram's source; for atoms with no text
/// of their own, a picture's description (its `alt`), a newline for a rule, and nothing for a
/// footnote marker or a citation (whose label is drawn, not text).
fn clipboard_text(html: &str) -> String {
    let plain = prosecraft_core::html::html_to_plain_text(html, &Default::default());
    let plain = plain.trim_end_matches('\n');
    if !plain.trim().is_empty() {
        return plain.to_owned();
    }
    let nodes = prosecraft_core::html::parse_fragment(html);
    let mut alts = Vec::new();
    let mut rule = false;
    let mut stack: Vec<&prosecraft_core::html::Node> = nodes.iter().rev().collect();
    while let Some(node) = stack.pop() {
        if let prosecraft_core::html::Node::Element(e) = node {
            match e.name.as_str() {
                "img" => alts.extend(e.attr("alt").map(str::trim).filter(|a| !a.is_empty())),
                "hr" => rule = true,
                _ => {}
            }
            stack.extend(e.children.iter().rev());
        }
    }
    if !alts.is_empty() {
        alts.join(" ")
    } else if rule {
        "\n".to_owned()
    } else {
        String::new()
    }
}

/// Whether the clipboard's HTML is the passage whose text was pasted (not HTML left from an
/// earlier copy): the same words as [`clipboard_text`] gives it; for a text with no words (a
/// rule's newline, or the object characters earlier builds wrote for atoms), HTML of atoms
/// alone.
fn clipboard_holds(text: &str, html: &str) -> bool {
    let words = words_of(text);
    if !words.is_empty() {
        return words_of(&clipboard_text(html)) == words;
    }
    words_of(&clipboard_text(html)).is_empty()
        && prosecraft_core::richtext::RichDoc::paste_atom_count(html) > 0
}

/// The text earlier builds copied for atoms alone: an object character for each.
fn is_atom_text(text: &str) -> bool {
    text.contains(prosecraft_core::richtext::ATOM)
        && text
            .chars()
            .all(|c| c == prosecraft_core::richtext::ATOM || c.is_whitespace())
}

/// Outside the rich editor a copy of atoms alone made by an earlier build pastes nothing, as an
/// empty plain text does in ProseCraft; an object character in Markdown would leave the scene
/// read-only.
fn drop_atom_paste(ui: &Ui) {
    ui.input_mut(|i| {
        i.events
            .retain(|e| !matches!(e, egui::Event::Paste(text) if is_atom_text(text)))
    });
}

/// Whether pasting `html` puts something in the editor: words or atoms (HTML of a picture on
/// the web, which the editor can't keep, pastes nothing).
fn html_pastes(html: &str) -> bool {
    prosecraft_core::richtext::RichDoc::paste_atom_count(html) > 0
        || !words_of(&prosecraft_core::html::html_to_plain_text(
            &prosecraft_core::html::sanitize_html(html),
            &Default::default(),
        ))
        .is_empty()
}

/// The words of a text, without spaces and punctuation: a pasted text and the clipboard's
/// HTML hold the same passage when these match.
fn words_of(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Paste into the rich editor as ProseCraft's browser editor pastes: the clipboard's HTML,
/// cleaned, with its formatting (egui's own paste is the plain text), and an image as a picture
/// (`handleImagePaste`). A clipboard with HTML and no text (a footnote marker copied alone),
/// which no paste event comes for, pastes its HTML too.
fn paste(
    ui: &mut Ui,
    body_id: egui::Id,
    document: &str,
    draft: &mut Draft,
    (clipboard_html, html_paste, intents, now): (
        Option<&crate::ClipboardHtml>,
        HtmlPasteFor<'_>,
        &mut Vec<Intent>,
        f64,
    ),
    screenplay: bool,
) {
    if screenplay || !ui.memory(|m| m.has_focus(body_id)) {
        return;
    }
    let range = egui::TextEdit::load_state(ui.ctx(), body_id)
        .and_then(|s| s.cursor.char_range())
        .map(|r| {
            let (a, b) = (r.primary.index.0, r.secondary.index.0);
            a.min(b)..a.max(b)
        })
        .unwrap_or_else(|| {
            let end = draft.body.chars().count();
            end..end
        });
    let image = ui.input_mut(|i| {
        let found = i.events.iter().find_map(|e| match e {
            egui::Event::PasteImage(image) => Some(image.clone()),
            _ => None,
        });
        if found.is_some() {
            i.events
                .retain(|e| !matches!(e, egui::Event::PasteImage(_)));
        }
        found
    });
    if let Some(image) = image {
        let [w, h] = image.size;
        let pixels: Vec<u8> = image
            .pixels
            .iter()
            .flat_map(|p| p.to_srgba_unmultiplied())
            .collect();
        let png = craft_image_codecs::Image::from_u8(
            w as u32,
            h as u32,
            craft_image_codecs::ChannelLayout::Rgba,
            pixels,
        )
        .ok()
        .and_then(|i| {
            craft_image_codecs::encode(&i, craft_image_codecs::Format::Png, &Default::default())
                .ok()
        });
        if let Some(png) = png {
            intents.push(Intent::PastePicture {
                document: document.to_owned(),
                at: range.start,
                png,
            });
        }
        return;
    }
    let text = ui.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Paste(text) => Some(text.clone()),
            _ => None,
        })
    });
    let html = match text {
        Some(text) => {
            let Some(html) = clipboard_html
                .and_then(|read| read())
                .filter(|html| clipboard_holds(&text, html))
            else {
                return;
            };
            ui.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Paste(_))));
            html
        }
        // No text, so no paste event: the HTML, if the editor can hold something of it.
        None => {
            let (asked, read) = html_paste;
            match asked.html(read) {
                Some(html) if html_pastes(&html) => html,
                _ => return,
            }
        }
    };
    let (body, caret) = draft.doc.paste_html(range, &html);
    draft.body = body;
    draft.body_dirty = true;
    draft.changed_at = now;
    draft.caret = Some(caret);
    draft.words = prosecraft_core::html::html_word_count(&draft.doc.to_html());
    let mut state = egui::TextEdit::load_state(ui.ctx(), body_id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(
            egui::text::CCursor::new(caret),
        )));
    state.store(ui.ctx(), body_id);
}

/// A passage's comment anchor, in ProseCraft's comment text (the scene's text nodes, the newlines
/// a Markdown scene has between its blocks included); none for whitespace alone.
fn comment_anchor(
    draft: &Draft,
    range: std::ops::Range<usize>,
) -> Option<prosecraft_core::comments::Anchor> {
    let map = draft.doc.comment_map();
    let (start, end) = (map.to_comment(range.start), map.to_comment(range.end));
    prosecraft_core::comments::build_anchor(&map.text, start, end)
}

/// `updateCommentSelectionPopover`: the Comment button over a selection (above it when there is
/// room, else below), as the toolbar's Comment does.
fn comment_popover(
    ui: &Ui,
    (document, offered): (&str, bool),
    draft: &Draft,
    output: &egui::text_edit::TextEditOutput,
    (pending, side_tab): (&mut Option<crate::PendingComment>, &mut crate::SideTab),
) {
    // The last one shown, kept while the pointer is on it: pressing its button takes the
    // focus from the text (ProseCraft keeps it with `preventDefault`).
    if !offered {
        return;
    }
    let kept_id = egui::Id::unique("comment-selection-popover-kept");
    let kept = ui
        .data(|d| {
            d.get_temp::<(prosecraft_core::comments::Anchor, egui::Pos2, egui::Rect)>(kept_id)
        })
        .filter(|(_, _, rect)| {
            ui.ctx()
                .pointer_latest_pos()
                .is_some_and(|p| rect.expand(4.0).contains(p))
        });
    let fresh = output
        .cursor_range
        .filter(|r| !r.is_empty() && output.response.has_focus())
        .filter(|_| !ui.input(|i| i.pointer.any_down()) && pending.is_none())
        .and_then(|r| {
            let (a, b) = (r.primary.index.0, r.secondary.index.0);
            let range = a.min(b)..a.max(b);
            let anchor = comment_anchor(draft, range.clone())?;
            let origin = output.galley_pos.to_vec2();
            let first = output
                .galley
                .pos_from_cursor(egui::text::CCursor::new(range.start))
                .translate(origin);
            let last = output
                .galley
                .pos_from_cursor(egui::text::CCursor::new(range.end))
                .translate(origin);
            let width = 108.0;
            let above = first.top() - ui.clip_rect().top() > 52.0;
            let center = if (first.top() - last.top()).abs() < 1.0 {
                (first.left() + last.left()) / 2.0
            } else {
                first.left()
            };
            let at = egui::pos2(
                center - width / 2.0,
                if above {
                    first.top() - 40.0
                } else {
                    last.bottom() + 10.0
                },
            );
            Some((anchor, at))
        });
    let Some((anchor, at)) = fresh.or(kept.map(|(a, at, _)| (a, at))) else {
        ui.data_mut(|d| {
            d.remove::<(prosecraft_core::comments::Anchor, egui::Pos2, egui::Rect)>(kept_id)
        });
        return;
    };
    let area = egui::Area::new(egui::Id::unique("comment-selection-popover"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .show(ui, |ui| {
                    let button = ui.button("💬 Comment").on_hover_text("Add comment");
                    name_field(ui, &button, "Add comment");
                    button.clicked()
                })
                .inner
        });
    ui.ctx().accesskit_node_builder(area.response.id, |node| {
        node.set_role(egui::accesskit::Role::Toolbar);
        node.set_label("Comment on selection");
    });
    if area.inner {
        ui.data_mut(|d| {
            d.remove::<(prosecraft_core::comments::Anchor, egui::Pos2, egui::Rect)>(kept_id)
        });
        *pending = Some(crate::PendingComment {
            document: document.to_owned(),
            anchor,
        });
        *side_tab = crate::SideTab::Comments;
    } else {
        let rect = area.response.rect;
        ui.data_mut(|d| d.insert_temp(kept_id, (anchor, at, rect)));
    }
}

/// The equation, diagram or calculation editor over the scene, and what it changed.
fn rich_dialog(
    ui: &mut Ui,
    palette: &Palette,
    (slot, editable, now): (&mut Option<crate::rich_dialog::RichDialog>, bool, f64),
    draft: &mut Draft,
    document: &str,
) {
    let Some(dialog) = slot.as_mut().filter(|d| d.document == document) else {
        return;
    };
    let (open, caret) = crate::rich_dialog::show(ui.ctx(), palette, dialog, draft, editable);
    if let Some(caret) = caret {
        draft.body_dirty = true;
        draft.changed_at = now;
        draft.caret = Some(caret);
        draft.words = prosecraft_core::html::html_word_count(&draft.doc.to_html());
    }
    if !open {
        *slot = None;
    }
}

/// HTML shown read-only as the editor draws text (a Wiki Markdown file's preview), or as its
/// plain text when the editor can't hold it.
pub(crate) fn read_only_html(ui: &mut Ui, palette: &Palette, html: &str, name: &str) {
    read_only_html_cited(ui, palette, html, name, &[], ("", "en"));
}

/// [`read_only_html`] with the project's citations labelled in its style and locale.
pub(crate) fn read_only_html_cited(
    ui: &mut Ui,
    palette: &Palette,
    html: &str,
    name: &str,
    items: &[prosecraft_core::js::JsValue],
    (style, locale): (&str, &str),
) {
    let cleaned = prosecraft_core::html::sanitize::sanitize_html(html);
    let held = RichDoc::from_html(&cleaned).or_else(|| RichDoc::from_html_normalized(&cleaned));
    let Some(doc) = held else {
        let response = ui
            .scope(|ui| manuscript(ui, palette, &plain(&cleaned)))
            .response;
        name_field(ui, &response, name);
        return;
    };
    let cache = std::cell::RefCell::new(Default::default());
    let cites = CiteStyle {
        items,
        style: style.to_owned(),
        locale: locale.to_owned(),
        cache: &cache,
    };
    let labels = atom_labels(&doc, &cites);
    let wrap = ui.available_width().max(1.0);
    let look = Look::default();
    let galley = ui.fonts_mut(|f| doc_galley(f, (&doc, &doc.text), palette, wrap, &labels, &look));
    let (rect, response) = ui.allocate_exact_size(
        galley.size().max(egui::vec2(wrap, 24.0)),
        egui::Sense::hover(),
    );
    ui.painter().galley(rect.min, galley, palette.text_body);
    let text = plain(&cleaned);
    ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::Document);
        n.set_label(name.to_owned());
        n.set_value(text);
    });
}

/// A split view pane's text, editable (ProseCraft's `TileEditor` body): laid out and drawn as
/// the editor draws text, named "Document text". Whether it changed.
pub(crate) fn tile_body(
    ui: &mut Ui,
    palette: &Palette,
    id: egui::Id,
    (doc, body): (&mut RichDoc, &mut String),
    cites: &CiteStyle<'_>,
) -> bool {
    let labels = atom_labels(doc, cites);
    let layout_labels = labels.clone();
    let doc_for_layout = doc.clone();
    let layout_palette = *palette;
    let mut layouter = move |ui: &Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        ui.fonts_mut(|f| {
            doc_galley(
                f,
                (&doc_for_layout, text.as_str()),
                &layout_palette,
                wrap_width,
                &layout_labels,
                &Look::default(),
            )
        })
    };
    let output = egui::TextEdit::multiline(body)
        .id(id)
        .frame(egui::Frame::NONE)
        .desired_width(f32::INFINITY)
        .desired_rows(4)
        .layouter(&mut layouter)
        .show(ui);
    name_field(ui, &output.response, "Document text");
    let mut changed = false;
    if output.response.changed() {
        let cursor = output.cursor_range.map(|r| r.primary.index.0);
        let kept = doc.apply_text_at(body, cursor);
        if kept != *body {
            *body = kept;
        }
        changed = true;
    }
    paint_extras(
        ui,
        palette,
        doc,
        &labels,
        &output.galley,
        output.galley_pos,
        output.text_clip_rect,
    );
    changed
}

/// Another scene of the open chapter, drawn as the editor draws it. A click opens it in the
/// editor with the caret where the click was.
fn chapter_scene(
    ui: &mut Ui,
    palette: &Palette,
    scene: &prosecraft_core::project::Document,
    screenplay: bool,
    cites: &CiteStyle<'_>,
    open: &mut Option<Selection>,
    look: &Look,
) {
    let title = if scene.title.is_empty() {
        "Untitled scene"
    } else {
        scene.title.as_str()
    };
    let held = RichDoc::from_html(&scene.content)
        .or_else(|| RichDoc::from_html_normalized(&scene.content));
    let Some(doc) = held else {
        // Content the rich editor can't hold: its text, read-only until the scene is opened.
        let response = ui
            .scope(|ui| manuscript(ui, palette, &plain(&scene.content)))
            .response
            .interact(egui::Sense::click());
        name_field(ui, &response, &format!("Scene {title}"));
        if response.clicked() {
            *open = Some(Selection::Document(scene.id.clone()));
        }
        return;
    };
    // Where the editor's own text starts, so the chapter reads as one column.
    let left = 0.0;
    let width = ui.available_width();
    let wrap = (width - left).max(1.0);
    let labels = atom_labels(&doc, cites);
    let galley = if screenplay {
        crate::screenplay_editor::galley(ui, &doc, &doc.text, palette, wrap)
    } else {
        ui.fonts_mut(|f| doc_galley(f, (&doc, &doc.text), palette, wrap, &labels, look))
    };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, galley.size().y.max(24.0)),
        egui::Sense::click(),
    );
    let origin = rect.min + egui::vec2(left, 0.0);
    ui.painter()
        .galley(origin, galley.clone(), palette.text_body);
    paint_extras(ui, palette, &doc, &labels, &galley, origin, rect);
    name_field(ui, &response, &format!("Scene {title}"));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    if response.clicked() {
        let at = response
            .interact_pointer_pos()
            .map_or(0, |pos| galley.cursor_from_pos(pos - origin).index.0);
        let id = body_id(&scene.id);
        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(at),
            )));
        state.store(ui.ctx(), id);
        ui.memory_mut(|m| m.request_focus(id));
        *open = Some(Selection::Document(scene.id.clone()));
    }
}

/// The chapter view's own keys, taken before the text field sees them: Enter on an empty
/// paragraph starts a new scene with the text after it (`handleChapterEnter`; never before the
/// chapter's first line), and Backspace at the start of a scene joins it to the one before (as
/// deleting the divider does).
fn chapter_keys(
    ui: &mut Ui,
    draft: &Draft,
    document: &str,
    chapter: &ChapterView<'_>,
    body_id: egui::Id,
    selection: Option<std::ops::Range<usize>>,
    intents: &mut Vec<Intent>,
) {
    if !ui.memory(|m| m.has_focus(body_id)) {
        return;
    }
    let Some(caret) = selection.filter(|r| r.is_empty()).map(|r| r.start) else {
        return;
    };
    let ranges = draft.doc.line_ranges();
    let Some(line) = ranges.iter().position(|(s, e)| (*s..=*e).contains(&caret)) else {
        return;
    };
    let (start, end) = ranges[line];
    let empty_paragraph =
        start == end && draft.doc.lines.get(line).map(|k| k.shown()) == Some(LineKind::Paragraph);
    let leads = chapter.before.is_empty() && line == 0;
    if empty_paragraph
        && !leads
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
    {
        let (before, after) = draft.doc.split_at_line(line);
        intents.push(Intent::SplitScene {
            document: document.to_owned(),
            before: before.to_html(),
            after: after.to_html(),
        });
        return;
    }
    if caret == 0
        && let Some(previous) = chapter.before.last()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace))
    {
        let joined_at =
            RichDoc::from_html(&previous.content).map_or(0, |d| d.text.chars().count() + 1);
        intents.push(Intent::JoinScene {
            document: document.to_owned(),
            caret: joined_at,
        });
    }
}

/// `recordFieldEdit` for the editor: the scene's HTML and footnotes in its history, as one step
/// per change, typing grouped as ProseCraft groups it. A history that doesn't end at this text
/// (another scene opened, a change from elsewhere) starts over.
/// Undo and redo in the text are the scene's history, taken before the text field's own.
fn history_keys(ui: &Ui, body_id: egui::Id, document: &str, intents: &mut Vec<Intent>) {
    if !ui.memory(|m| m.has_focus(body_id)) {
        return;
    }
    let (undo, redo) = ui.input_mut(|i| {
        // Ctrl+Shift+Z first: a Ctrl+Z pattern also matches it.
        let redo = i.consume_key(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::Z,
        ) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y);
        let undo = i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z);
        (undo, redo)
    });
    if undo || redo {
        intents.push(Intent::BodyHistory {
            document: document.to_owned(),
            undo,
        });
    }
}

/// What typing sets off: Zen mode hides the interface, typewriter mode keeps the caret at the
/// middle of the view (`scheduleRichTextTypewriter`); and where the text is, for Zen mode.
fn after_typing(
    ui: &Ui,
    writing: crate::WritingPrefs,
    editor_rect: &mut Option<egui::Rect>,
    intents: &mut Vec<Intent>,
    output: &egui::text_edit::TextEditOutput,
) {
    *editor_rect = Some(output.response.rect);
    if !output.response.changed() {
        return;
    }
    if writing.zen {
        intents.push(Intent::HideInterface);
    }
    if writing.typewriter
        && let Some(range) = output.cursor_range
    {
        let caret = output
            .galley
            .pos_from_cursor(range.primary)
            .translate(output.galley_pos.to_vec2());
        ui.scroll_to_rect(caret, Some(egui::Align::Center));
    }
}

/// `addLink`'s prompt ("Paste a link"), with ProseCraft's messages for what can't be a link.
fn link_prompt(ui: &mut Ui, draft: &mut Draft, now: f64) {
    let Some((range, entered, error)) = draft.link_prompt.as_mut() else {
        return;
    };
    let mut open = true;
    let mut add = false;
    let mut cancel = false;
    egui::Window::new("Add link")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            let field = ui.add(
                egui::TextEdit::singleline(entered)
                    .hint_text("Paste a link")
                    .desired_width(320.0),
            );
            name_field(ui, &field, "Paste a link");
            field.request_focus();
            if let Some(error) = error {
                ui.label(RichText::new(*error).color(ui.visuals().error_fg_color));
            }
            ui.horizontal(|ui| {
                cancel = ui.button("Cancel").clicked();
                add = ui.button(RichText::new("Add link").strong()).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Enter));
            });
        });
    if add && !entered.trim().is_empty() {
        match prosecraft_core::richtext::link_href(entered) {
            Ok(href) => {
                let range = range.clone();
                draft.body = draft.doc.set_link(range, &href);
                draft.body_dirty = true;
                draft.changed_at = now;
                draft.link_prompt = None;
            }
            Err(message) => *error = Some(message),
        }
    } else if !open || cancel || (add && entered.trim().is_empty()) {
        draft.link_prompt = None;
    }
}

/// The history and where an edit is recorded in it.
struct UndoAt<'a> {
    undo: &'a mut prosecraft_core::undo::UndoManager<(usize, usize)>,
    project: &'a str,
    page: &'a str,
    now_ms: f64,
}

/// The ids of a scene's footnote markers, in order.
fn footnote_ids(doc: &RichDoc) -> Vec<String> {
    doc.atoms
        .iter()
        .filter(|a| is_footnote(a))
        .flat_map(|a| prosecraft_core::footnotes::extract_ids(a))
        .collect()
}

/// `handleInput`: a footnote whose last marker an edit took away (a cut, Backspace or Delete,
/// typing or pasting over it) loses its definition in the scene, or the chapter, it belongs
/// to. Undo brings it back with the marker (the step before holds both). Returns those ids.
fn drop_removed_footnotes(
    before: &[String],
    draft: &Draft,
    (document, chapter): (
        &prosecraft_core::project::Document,
        Option<&ChapterView<'_>>,
    ),
    intents: &mut Vec<Intent>,
) -> Vec<String> {
    let after = footnote_ids(&draft.doc);
    if before == after.as_slice() {
        return Vec::new();
    }
    let others: Vec<&prosecraft_core::project::Document> = chapter
        .map(|c| c.before.iter().chain(&c.after).copied().collect())
        .unwrap_or_default();
    // In the chapter view the chapter's other scenes are in the editor too. Only the open
    // scene is ever being typed into: switching scenes saves the draft first, so the others'
    // stored text is their live text (a marker just pasted into one of them counts).
    let elsewhere: Vec<String> = others
        .iter()
        .flat_map(|s| prosecraft_core::footnotes::extract_ids(&s.content))
        .collect();
    let mut removed = prosecraft_core::footnotes::removed_ids(before, &after);
    removed.retain(|id| !elsewhere.contains(id));
    for id in &removed {
        for scene in std::iter::once(document).chain(others.iter().copied()) {
            if scene.footnotes.iter().any(|f| f.id == *id) {
                intents.push(Intent::RemoveFootnote {
                    document: scene.id.clone(),
                    id: id.clone(),
                });
            }
        }
    }
    removed
}

fn record_body_history(
    edit: UndoAt<'_>,
    (document, dropped): (&prosecraft_core::project::Document, &[String]),
    draft: &mut Draft,
    cursor: Option<egui::text::CCursorRange>,
) {
    use prosecraft_core::undo::{Edit as UndoEdit, EditKind, decode_body, encode_body};
    let key = format!("{}/body:{}", edit.project, document.id);
    // The definitions as they are after this edit (those it dropped are going).
    let notes: Vec<(String, String)> = document
        .footnotes
        .iter()
        .filter(|f| !dropped.contains(&f.id))
        .map(|f| (f.id.clone(), f.html.clone()))
        .collect();
    let value = encode_body(&draft.doc.to_html(), &document.id, &notes);
    let selection = cursor.map(|r| {
        let (a, b) = (r.primary.index.0, r.secondary.index.0);
        (a.min(b), a.max(b))
    });
    let Some(current) = edit.undo.current(&key).map(str::to_owned) else {
        edit.undo.sync(&key, &value);
        draft.undo_caret = selection;
        return;
    };
    if current == value {
        if let Some(selection) = selection.filter(|_| draft.undo_caret.is_none()) {
            draft.undo_caret = Some(selection);
        }
        return;
    }
    let html_unchanged = decode_body(&current).0 == draft.doc.to_html();
    if !draft.is_dirty() && !html_unchanged {
        // Not typed here: the stored text changed underneath (a sync, an import).
        edit.undo.sync(&key, &value);
        draft.undo_caret = selection;
        return;
    }
    // Only a footnote's text changed (`finishProgrammaticBodyEdit` when its popover closes):
    // one step of its own.
    let before_text = prosecraft_core::richtext::RichDoc::from_html(&decode_body(&current).0)
        .map(|d| d.text)
        .unwrap_or_default();
    let diff = prosecraft_core::undo::diff_text(&before_text, &draft.doc.text);
    let kind = if diff.inserted.contains('\n') {
        EditKind::Newline
    } else if diff.inserted.is_empty() && !diff.removed.is_empty() {
        EditKind::Delete
    } else if diff.removed.is_empty() && !diff.inserted.is_empty() {
        EditKind::Type
    } else {
        EditKind::Other
    };
    let after = selection.or(draft.undo_caret).unwrap_or_default();
    let start = before_text[..diff.from].chars().count();
    // Where this edit began: right after the last one, unless the caret went elsewhere.
    let at = match kind {
        EditKind::Type | EditKind::Newline => start,
        EditKind::Delete => start + diff.removed.chars().count(),
        _ => start,
    };
    let previous = draft.undo_caret.unwrap_or((at, at));
    let caret_moved = previous.1 != at && previous.0 != at;
    edit.undo.record(
        &key,
        Some(edit.page),
        &value,
        UndoEdit {
            kind,
            time: edit.now_ms,
            caret_moved,
            before: previous,
            after,
        },
    );
    draft.undo_caret = Some(after);
}

/// `chapterMarkdownSource` over the chapter's scenes: their own sources when every scene has
/// one, else Markdown from their HTML.
fn chapter_markdown(scenes: &[&prosecraft_core::project::Document]) -> String {
    use prosecraft_core::chapter::join_chapter_markdown;
    let sources: Option<Vec<String>> = scenes.iter().map(|s| s.markdown_source.clone()).collect();
    match sources {
        Some(sources) if !scenes.is_empty() => join_chapter_markdown(&sources),
        _ => join_chapter_markdown(
            &scenes
                .iter()
                .map(|s| {
                    let notes: Vec<prosecraft_core::markdown::FootnoteDef> = s
                        .footnotes
                        .iter()
                        .map(|f| prosecraft_core::markdown::FootnoteDef {
                            id: f.id.clone(),
                            html: f.html.clone(),
                        })
                        .collect();
                    prosecraft_core::markdown::html_to_markdown(&s.content, &notes)
                })
                .collect::<Vec<_>>(),
        ),
    }
}

/// The block styles of ProseCraft's toolbar.
const BLOCK_STYLES: [(LineKind, &str); 7] = [
    (LineKind::Paragraph, "Normal text"),
    (LineKind::Heading1, "Title"),
    (LineKind::Heading2, "Heading"),
    (LineKind::Heading3, "Subheading"),
    (LineKind::Quote, "Quote"),
    (LineKind::Bullet, "Bulleted list"),
    (LineKind::Numbered, "Numbered list"),
];

/// The citation library and the publish settings' style and locale, which the editor labels
/// citation markers in (`refreshEditorCitationLabels`).
pub(crate) struct CiteStyle<'a> {
    pub items: &'a [prosecraft_core::js::JsValue],
    pub style: String,
    pub locale: String,
    /// The labels as last worked out (asked for every frame).
    pub cache: &'a std::cell::RefCell<prosecraft_core::citations::labels::LabelCache>,
}

/// What an atom shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AtomLabel {
    /// A footnote reference: its number.
    Footnote(String),
    /// An equation: its TeX.
    Math(String),
    /// A citation: its sources formatted in the publish style (`renderCitationLabels`).
    Cite(String),
    /// A block kept whole (a rule, code, a table, a block equation, a diagram, a calculation):
    /// drawn as a panel across the column.
    Block(prosecraft_core::richtext::BlockPreview),
    /// A picture: drawn at its layout's size, on a row of its own.
    Picture(crate::pictures::PictureInfo),
    /// Anything else (none yet): nothing drawn, nothing to click.
    Other,
}

/// The height of a kept block's row.
fn block_height(preview: &prosecraft_core::richtext::BlockPreview, wrap: f32) -> f32 {
    if preview.kind == prosecraft_core::richtext::BlockKind::Rule {
        return 28.0;
    }
    // A drawn equation or calculation: its height (shrunk to the column), with its margins.
    if let Some(kind) = crate::rich_view::block_kind(preview)
        && let Some(size) = crate::rich_view::size(kind, &preview.source)
    {
        let fit = if wrap.is_finite() && size.x > wrap {
            wrap / size.x
        } else {
            1.0
        };
        return size.y * fit + 2.0 * crate::rich_view::BLOCK_MARGIN;
    }
    let lines = preview.lines.len().max(1) + usize::from(preview.more);
    8.0 + 18.0 + lines as f32 * BLOCK_LINE + 12.0
}

const BLOCK_LINE: f32 = 18.0;

fn is_footnote(html: &str) -> bool {
    html.contains("footnote-ref")
}

fn is_math(html: &str) -> bool {
    html.contains("math-inline")
}

fn is_cite(html: &str) -> bool {
    crate::citation_picker::is_cite_marker(html)
}

/// Each atom's label, in order.
pub(crate) fn atom_labels(doc: &RichDoc, cites: &CiteStyle) -> Vec<AtomLabel> {
    let cite_markers: Vec<&str> = doc
        .atoms
        .iter()
        .map(String::as_str)
        .filter(|h| is_cite(h))
        .collect();
    let mut cite_labels = cites
        .cache
        .borrow_mut()
        .labels(&cite_markers, cites.items, &cites.style, &cites.locale)
        .into_iter();
    // Footnotes are numbered by id in order (`footnoteNumbers`): a repeated marker keeps its number.
    let mut numbered: Vec<String> = Vec::new();
    doc.atoms
        .iter()
        .map(|html| {
            if let Some(preview) = prosecraft_core::richtext::is_block_atom(html)
                .then(|| prosecraft_core::richtext::block_preview(html))
                .flatten()
            {
                AtomLabel::Block(preview)
            } else if html.starts_with("<img") {
                crate::pictures::info(html).map_or(AtomLabel::Other, AtomLabel::Picture)
            } else if is_footnote(html) {
                let id = prosecraft_core::footnotes::extract_ids(html)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                let number = match numbered.iter().position(|i| *i == id) {
                    Some(i) => i + 1,
                    None => {
                        numbered.push(id);
                        numbered.len()
                    }
                };
                AtomLabel::Footnote(number.to_string())
            } else if is_math(html) {
                let nodes = prosecraft_core::html::parse_fragment(html);
                let tex = nodes.first().and_then(|n| match n {
                    prosecraft_core::html::Node::Element(e) => {
                        e.attr("data-tex").map(str::to_owned)
                    }
                    prosecraft_core::html::Node::Text(_) => None,
                });
                AtomLabel::Math(tex.unwrap_or_default())
            } else if !is_cite(html) {
                AtomLabel::Other
            } else {
                // A marker whose sources can't be read shows nothing in ProseCraft (no
                // `data-label`); a word keeps it visible and clickable here.
                AtomLabel::Cite(
                    cite_labels
                        .next()
                        .flatten()
                        .unwrap_or_else(|| "citation".to_owned()),
                )
            }
        })
        .collect()
}

/// A citation label's font: the body's at 0.92em, as ProseCraft's `span.cite-ref`.
fn cite_font() -> egui::FontId {
    fonts::serif(17.0 * 0.92)
}

/// How an atom's (invisible) character is laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct AtomBox {
    /// Its font size (0: the usual size): a citation's is scaled so the character is as wide
    /// as its label and padding, and wraps as one piece with it.
    pub font: f32,
    /// Room added after it: a picture's character is as wide as the column.
    pub spacing: f32,
    /// The height of its row: a kept block's, a picture's.
    pub row: Option<f32>,
    /// A picture, laid out as [`PICTURE_CHAR`].
    pub picture: bool,
}

/// How each atom's character is laid out (see [`AtomBox`]).
pub(crate) fn atom_sizes(
    fonts: &mut egui::epaint::FontsView<'_>,
    labels: &[AtomLabel],
    wrap: f32,
) -> Vec<AtomBox> {
    let glyph = fonts
        .layout_no_wrap(
            ATOM.to_string(),
            egui::FontId::proportional(ATOM_FONT + 2.0),
            egui::Color32::TRANSPARENT,
        )
        .size()
        .x;
    let picture_glyph = fonts
        .layout_no_wrap(
            PICTURE_CHAR.to_string(),
            egui::FontId::proportional(ATOM_FONT + 2.0),
            egui::Color32::TRANSPARENT,
        )
        .size()
        .x;
    labels
        .iter()
        .map(|label| match label {
            AtomLabel::Cite(text) => {
                let width = fonts
                    .layout_no_wrap(text.clone(), cite_font(), egui::Color32::TRANSPARENT)
                    .size()
                    .x;
                // ProseCraft's 5px padding each side and 1px margin.
                let font = if glyph > 0.0 {
                    (ATOM_FONT + 2.0) * (width + 12.0) / glyph
                } else {
                    0.0
                };
                AtomBox {
                    font,
                    ..AtomBox::default()
                }
            }
            AtomLabel::Block(preview) => AtomBox {
                row: Some(block_height(preview, wrap)),
                ..AtomBox::default()
            },
            // An equation in the line: as wide as its drawing.
            AtomLabel::Math(tex) => {
                let width =
                    crate::rich_view::size(prosecraft_core::rich::RichKind::MathInline, tex)
                        .map_or(14.0, |s| s.x + 4.0);
                AtomBox {
                    font: if glyph > 0.0 {
                        (ATOM_FONT + 2.0) * width / glyph
                    } else {
                        0.0
                    },
                    ..AtomBox::default()
                }
            }
            // A wrapped picture: the text flows beside it (see `picture_flow`), its character
            // all but invisible where it stands.
            AtomLabel::Picture(picture) if picture.layout.wrap != "block" && wrap.is_finite() => {
                AtomBox {
                    font: 1.0,
                    picture: true,
                    ..AtomBox::default()
                }
            }
            // As wide as the column, so the picture has its row to itself.
            AtomLabel::Picture(picture) => AtomBox {
                spacing: if wrap.is_finite() {
                    (wrap - 4.0 - picture_glyph).max(0.0)
                } else {
                    0.0
                },
                row: Some(picture.drawn(wrap).y + 2.0 * crate::pictures::MARGIN),
                picture: true,
                ..AtomBox::default()
            },
            AtomLabel::Footnote(_) | AtomLabel::Other => AtomBox::default(),
        })
        .collect()
}

const ATOM_FONT: f32 = 12.0;

/// The atom character's glyph, the same in every build: an invisible box half an em wide. No
/// installed font has U+FFFC, so egui asks its fallbacks, and on the web eframe's asks the
/// browser, which draws nothing for it and measures it as nothing; the character then had no
/// width to scale (see [`atom_sizes`]), and a citation's label lay over the text after it.
pub(crate) fn atom_glyph() -> egui::GlyphRasterizer {
    egui::GlyphRasterizer::new("prosecraft::atom_glyph", |request| {
        let mut chars = request.cluster.chars();
        if chars.next() != Some(ATOM) || chars.next().is_some() {
            return None;
        }
        Some(egui::RasterizedGlyph {
            bitmap: egui::GlyphBitmap {
                image: egui::ColorImage::filled([1, 1], egui::Color32::TRANSPARENT),
                offset_px: egui::Vec2::ZERO,
                is_color: false,
            },
            advance_px: request.font_size_px * 0.5,
        })
    })
    .with_priority(egui::FontPriority::Highest)
}

/// The text formats the editor lays the scene out with.
/// How the editor's text looks beyond its formatting: the text size (`--editor-scale`), and
/// which lines Fade other paragraphs dims.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Look {
    pub scale: f32,
    pub faded: Vec<bool>,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            scale: 1.0,
            faded: Vec::new(),
        }
    }
}

impl Look {
    fn scaled(writing: &crate::WritingPrefs) -> Self {
        Self {
            scale: writing.scale as f32 / 100.0,
            faded: Vec::new(),
        }
    }
}

/// `applyRichTextFade`: every line but those the selection is in, and the list items they sit
/// inside, faded.
fn faded_lines(doc: &RichDoc, selection: std::ops::Range<usize>) -> Vec<bool> {
    let ranges = doc.line_ranges();
    let line_of = |i: usize| {
        ranges
            .iter()
            .position(|(s, e)| (*s..=*e).contains(&i))
            .unwrap_or(0)
    };
    let (first, last) = (line_of(selection.start), line_of(selection.end));
    let mut faded = vec![true; ranges.len()];
    for line in first..=last.min(ranges.len().saturating_sub(1)) {
        faded[line] = false;
        // The items a nested item sits inside are its ancestors: kept.
        let mut depth = doc.lines.get(line).and_then(|k| k.list_depth());
        let mut at = line;
        while let Some(d) = depth.filter(|d| *d > 0) {
            let Some(parent) = (0..at).rev().find(|l| {
                doc.lines
                    .get(*l)
                    .and_then(|k| k.list_depth())
                    .is_none_or(|ld| ld < d)
            }) else {
                break;
            };
            if doc.lines.get(parent).and_then(|k| k.list_depth()).is_none() {
                break;
            }
            faded[parent] = false;
            depth = doc.lines.get(parent).and_then(|k| k.list_depth());
            at = parent;
        }
    }
    faded
}

pub(crate) fn layout_doc(
    doc: &RichDoc,
    text: &str,
    palette: &Palette,
    wrap: f32,
    atom_sizes: &[AtomBox],
    look: &Look,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap;
    job.direction = SCENE_DIRECTION;
    let mut line = 0;
    let mut atom = 0;
    let mut previous = None;
    for (i, c) in text.chars().enumerate() {
        let mut laid_out = c;
        if c == '\n' {
            line += 1;
        }
        let kind = doc
            .lines
            .get(line)
            .copied()
            .unwrap_or(LineKind::Paragraph)
            .shown();
        let marks = doc.marks.get(i).map_or(&[][..], Vec::as_slice);
        let (size, heading) = match kind {
            LineKind::Heading1 => (32.0, true),
            LineKind::Heading2 => (25.0, true),
            LineKind::Heading3 => (20.0, true),
            // ProseCraft's `.eyebrow`: small, spaced capitals.
            LineKind::Eyebrow => (12.0, false),
            _ => (17.0, false),
        };
        let size = size * look.scale;
        let bold = heading || Mark::Bold.applies(marks);
        let italic = kind == LineKind::Quote || Mark::Italic.applies(marks);
        let font = match (bold, italic) {
            (_, true) => fonts::writing_italic(size),
            (true, false) => fonts::writing_bold(size),
            _ => fonts::writing(size),
        };
        let mut format = egui::TextFormat {
            font_id: font,
            line_height: Some(size * if heading { 1.3 } else { 1.85 }),
            ..Default::default()
        };
        if kind == LineKind::Eyebrow {
            format.extra_letter_spacing = 1.2;
        }
        format.color = match kind {
            LineKind::Quote => palette.quote,
            LineKind::Eyebrow => palette.eyebrow,
            _ if heading => palette.text_heading,
            _ => palette.text_body,
        };
        if look.faded.get(line).copied().unwrap_or(false) {
            // `.faded-writing-block { opacity: .38 }`.
            format.color = format.color.gamma_multiply(0.38);
        }
        if Mark::Underline.applies(marks) || marks.iter().any(|t| t.name == "a") {
            format.underline = egui::Stroke::new(1.0, format.color);
        }
        if Mark::Strike.applies(marks) {
            format.strikethrough = egui::Stroke::new(1.0, format.color);
        }
        if c == ATOM {
            // The character itself is invisible; its label is painted in the room reserved here.
            // The character's own box holds a short painted label.
            let laid = atom_sizes.get(atom).copied().unwrap_or_default();
            // A picture: laid out as a character a row may end before and after (see
            // `RowBreaks`), as a browser breaks around an image.
            if laid.picture {
                laid_out = PICTURE_CHAR;
                format.extra_letter_spacing = laid.spacing;
            }
            // A line separator in the text itself: it ends the row, as in ProseCraft.
            if let Some(own @ (prosecraft_core::richtext::SOFT_BREAK | HARD_BREAK)) = doc
                .atoms
                .get(atom)
                .and_then(|html| prosecraft_core::richtext::text_atom(html))
            {
                laid_out = own;
            }
            let size = if laid.font > 0.0 {
                laid.font
            } else {
                ATOM_FONT + 2.0
            };
            format.font_id = egui::FontId::proportional(size);
            if laid.row.is_some() {
                format.line_height = laid.row;
            }
            format.color = egui::Color32::TRANSPARENT;
            atom += 1;
        }
        if matches!(laid_out, prosecraft_core::richtext::SOFT_BREAK | HARD_BREAK) {
            // A line break: it ends the row (see `RowBreaks`) and shows nothing.
            format.color = egui::Color32::TRANSPARENT;
            format.font_id.size = 1.0;
        }
        if c == '\n' {
            // ProseCraft's gap between paragraphs.
            format.line_height = Some(size * 1.85 + 10.0);
        }
        // ProseCraft's padding: a list item is indented a step per list it sits in, a quote's
        // text past its bar. (Rows a line wraps onto aren't.)
        let starts_line = c != '\n' && (i == 0 || previous == Some('\n'));
        let indent = match kind.list_depth() {
            _ if !starts_line => 0.0,
            Some(depth) => (depth + 1) as f32 * LIST_INDENT,
            None if kind == LineKind::Quote => QUOTE_INDENT,
            None => 0.0,
        };
        previous = Some(c);
        let mut buf = [0u8; 4];
        job.append(laid_out.encode_utf8(&mut buf), indent, format);
    }
    job
}

/// The scene's text laid out as the editor shows it: [`layout_doc`], with the text beside its
/// wrapped pictures.
pub(crate) fn doc_galley(
    fonts: &mut egui::epaint::FontsView<'_>,
    (doc, text): (&RichDoc, &str),
    palette: &Palette,
    wrap: f32,
    labels: &[AtomLabel],
    look: &Look,
) -> std::sync::Arc<egui::Galley> {
    let sizes = atom_sizes(fonts, labels, wrap);
    let job = layout_doc(doc, text, palette, wrap, &sizes, look);
    let floats = floats(text, labels, wrap);
    if floats.is_empty() {
        return fonts.layout_job(job);
    }
    crate::picture_flow::flow(fonts, job, &floats, &clears(doc, text, labels))
}

/// The wrapped pictures in `text`, drawn in a column `column` wide.
pub(crate) fn floats(
    text: &str,
    labels: &[AtomLabel],
    column: f32,
) -> Vec<crate::picture_flow::Float> {
    // Without a column to wrap in, a picture keeps its row (see `atom_sizes`).
    if !column.is_finite() {
        return Vec::new();
    }
    text.chars()
        .enumerate()
        .filter(|(_, c)| *c == ATOM)
        .zip(labels.iter().enumerate())
        .filter_map(|((index, _), (atom, label))| match label {
            AtomLabel::Picture(picture) if picture.layout.wrap != "block" => {
                Some(crate::picture_flow::Float {
                    index,
                    atom,
                    left: picture.layout.wrap == "wrap-left",
                    size: picture.drawn(column),
                })
            }
            _ => None,
        })
        .collect()
}

/// Which lines start below the wrapped pictures (ProseCraft's `clear: both`): headings, quotes
/// and kept blocks, and here a line holding a picture on its own row.
fn clears(doc: &RichDoc, text: &str, labels: &[AtomLabel]) -> Vec<bool> {
    let mut out: Vec<bool> = doc
        .lines
        .iter()
        .map(|kind| {
            matches!(
                kind.shown(),
                LineKind::Heading1
                    | LineKind::Heading2
                    | LineKind::Heading3
                    | LineKind::Quote
                    | LineKind::Block
            )
        })
        .collect();
    let mut line = 0;
    let mut atoms = labels.iter();
    for c in text.chars() {
        if c == '\n' {
            line += 1;
        } else if c == ATOM
            && let Some(AtomLabel::Picture(picture)) = atoms.next()
            && picture.layout.wrap == "block"
            && let Some(clear) = out.get_mut(line)
        {
            *clear = true;
        }
    }
    out
}

/// `.editor-content ul, ol { padding-left: 24px }`.
const LIST_INDENT: f32 = 24.0;
/// `.editor-content blockquote { padding: 2px 0 2px 17px }`.
const QUOTE_INDENT: f32 = 17.0;

/// What a picture's character is laid out as: a character no font has (drawn as an empty box,
/// as wide as its font size, like the atom character), which rows may end before and after.
pub(crate) const PICTURE_CHAR: char = '\u{E000}';

/// For a scene the editor holds only in a normalized shape, still untouched: said, with the
/// Markdown editor that edits its source as it is stored (not for a screenplay, which keeps its
/// own editor).
fn normalized_note(
    ui: &mut Ui,
    palette: &Palette,
    (draft, document, screenplay): (&Draft, &prosecraft_core::project::Document, bool),
    (forced, switch): (&mut bool, &mut Option<bool>),
) {
    if screenplay
        || draft.body_dirty
        || draft.source != document.content
        || draft.doc.to_html() == draft.source
    {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new("Some of this scene's formatting is simplified once you edit it.")
                .size(12.0)
                .color(palette.text_muted),
        );
        let markdown = ui
            .button("Edit as Markdown")
            .on_hover_text("Open this scene in the Markdown editor");
        if markdown.clicked() {
            *forced = true;
            *switch = Some(true);
        }
    });
    ui.add_space(6.0);
}

/// A new draft when another document opens, or when the stored one changed underneath an
/// untouched draft (an import of a newer copy). A draft held in a normalized shape writes other
/// HTML than it was read from, so it is compared with that too.
fn is_stale(draft: &Draft, document: &prosecraft_core::project::Document) -> bool {
    draft.document != document.id
        || draft.markdown.is_some()
        || (!draft.is_dirty()
            && (draft.title != document.title
                || (draft.source != document.content && draft.doc.to_html() != document.content)))
}

/// The editor: the scene's title, ProseCraft's toolbar, and its formatted text.
fn editor(
    ui: &mut Ui,
    palette: &Palette,
    document: &prosecraft_core::project::Document,
    doc: RichDoc,
    screenplay: bool,
    chapter: Option<ChapterView<'_>>,
    edit: &mut Edit<'_>,
) {
    if edit.draft.as_ref().is_none_or(|d| is_stale(d, document)) {
        *edit.draft = Some(Draft {
            document: document.id.clone(),
            title: document.title.clone(),
            words: prosecraft_core::html::html_word_count(&doc.to_html()),
            body: doc.text.clone(),
            doc,
            source: document.content.clone(),
            title_dirty: false,
            body_dirty: false,
            changed_at: edit.now,
            edits: 0,
            words_added: 0.0,
            markdown: None,
            markdown_base: String::new(),
            chapter: None,
            caret: None,
            undo_caret: None,
            link_prompt: None,
        });
    }
    let now = edit.now;
    let Some(draft) = edit.draft.as_mut() else {
        return;
    };
    // In the chapter view the title is the chapter's (scenes are renamed in the binder).
    if let Some(chapter) = &chapter {
        chapter_title(
            ui,
            palette,
            &chapter.id,
            &chapter.label,
            &chapter.title,
            edit.intents,
        );
    } else {
        let title = ui.add(
            egui::TextEdit::singleline(&mut draft.title)
                .font(fonts::serif(38.0))
                .text_color(palette.text_heading)
                .frame(egui::Frame::NONE)
                .hint_text("Untitled scene")
                .desired_width(f32::INFINITY),
        );
        name_field(ui, &title, "Scene title");
        if title.changed() {
            draft.title_dirty = true;
            draft.changed_at = now;
        }
    }
    ui.add_space(10.0);
    crate::story_data::show(
        ui,
        palette,
        edit.stored,
        &document.id,
        edit.story_open,
        edit.editable,
        edit.intents,
    );

    let body_id = body_id(&document.id);
    let selection = egui::TextEdit::load_state(ui.ctx(), body_id)
        .and_then(|s| s.cursor.char_range())
        .map(|r| {
            let (a, b) = (r.primary.index.0, r.secondary.index.0);
            a.min(b)..a.max(b)
        });
    let cursor = selection.clone().map_or(0, |r| r.start);
    let mut changed_format = false;
    if let Some(chapter) = chapter.as_ref().filter(|_| !screenplay) {
        chapter_keys(
            ui,
            draft,
            &document.id,
            chapter,
            body_id,
            selection.clone(),
            edit.intents,
        );
    }
    // The screenplay keys (Enter, Tab, Shift+Tab) are the editor's, taken before anything is
    // drawn so the Screenplay bar shows the element they leave the caret on.
    let mut cursor = cursor;
    if screenplay && crate::screenplay_editor::handle_keys(ui, draft, body_id, selection.clone()) {
        draft.body_dirty = true;
        draft.changed_at = now;
        cursor = egui::TextEdit::load_state(ui.ctx(), body_id)
            .and_then(|s| s.cursor.char_range())
            .map_or(cursor, |r| r.primary.index.0);
    }
    // Shift+Enter: a line break inside the block, the `<br>` ProseCraft's editor writes.
    if ui.memory(|m| m.has_focus(body_id))
        && let Some(range) =
            egui::TextEdit::load_state(ui.ctx(), body_id).and_then(|s| s.cursor.char_range())
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter))
    {
        let (a, b) = (range.primary.index.0, range.secondary.index.0);
        let chars: Vec<char> = draft.body.chars().collect();
        let (start, end) = (a.min(b).min(chars.len()), a.max(b).min(chars.len()));
        let text: String = chars[..start]
            .iter()
            .chain(&[HARD_BREAK])
            .chain(&chars[end..])
            .collect();
        draft.body = draft.doc.apply_text_at(&text, Some(start + 1));
        let mut state = egui::TextEdit::load_state(ui.ctx(), body_id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(start + 1),
            )));
        state.store(ui.ctx(), body_id);
        draft.body_dirty = true;
        draft.changed_at = now;
        cursor = start + 1;
    }
    // ProseCraft's Screenplay bar, above the formatting toolbar; for a prose scene of a
    // screenplay project, with "Convert this scene to screenplay" (asked first).
    if screenplay {
        changed_format |= crate::screenplay_editor::bar(ui, palette, draft, body_id, cursor);
        if !document.screenplay {
            let asking = ui.make_persistent_id(("convert-to-screenplay", &document.id));
            if ui.data(|d| d.get_temp::<bool>(asking)).unwrap_or(false) {
                let name = if document.title.is_empty() {
                    "Untitled scene"
                } else {
                    document.title.as_str()
                };
                ui.label(format!("Convert “{name}” to screenplay format? A snapshot of the prose version will be saved first."));
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Convert").strong()).clicked() {
                        edit.intents.push(Intent::ConvertToScreenplay {
                            document: document.id.clone(),
                            now: edit.now_ms,
                        });
                        ui.data_mut(|d| d.remove_temp::<bool>(asking));
                    }
                    if ui.button("Cancel").clicked() {
                        ui.data_mut(|d| d.remove_temp::<bool>(asking));
                    }
                });
            } else if ui.button("Convert this scene to screenplay").clicked() {
                ui.data_mut(|d| d.insert_temp(asking, true));
            }
        }
        ui.add_space(6.0);
    }
    normalized_note(
        ui,
        palette,
        (draft, document, screenplay),
        (edit.markdown_forced, edit.markdown_switch),
    );
    // ProseCraft's formatting toolbar; it wraps onto more rows in a narrow column rather than
    // widening the column past the window.
    card(palette).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            let current = draft.doc.kind_at(cursor);
            // A nested list item shows as its list's style.
            let shown = match current.list_depth() {
                Some(_) if current.numbered() => LineKind::Numbered,
                Some(_) => LineKind::Bullet,
                None => current,
            };
            let label = BLOCK_STYLES
                .iter()
                .find(|(k, _)| *k == shown)
                .map_or("Normal text", |(_, l)| l);
            let mut chosen = current;
            let style = craft_ui::ComboBox::from_id_salt(("block-style", &document.id))
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (kind, name) in BLOCK_STYLES {
                        ui.selectable_value(&mut chosen, kind, name);
                    }
                });
            // Named for what it sets; the current style is its value.
            ui.ctx().accesskit_node_builder(style.response.id, |node| {
                node.set_label("Block style");
                node.set_value(label);
            });
            if chosen != current {
                draft
                    .doc
                    .set_line_kind(selection.clone().unwrap_or(cursor..cursor), chosen);
                changed_format = true;
            }
            ui.separator();
            let body_focused = ui.memory(|m| m.has_focus(body_id));
            for (mark, text, name, key) in [
                (
                    Mark::Bold,
                    RichText::new("B").strong(),
                    "Bold",
                    Some(egui::Key::B),
                ),
                (
                    Mark::Italic,
                    RichText::new("I").italics(),
                    "Italic",
                    Some(egui::Key::I),
                ),
                (
                    Mark::Underline,
                    RichText::new("U").underline(),
                    "Underline",
                    Some(egui::Key::U),
                ),
                (
                    Mark::Strike,
                    RichText::new("S").strikethrough(),
                    "Strikethrough",
                    None,
                ),
            ] {
                let button = ui.button(text).on_hover_text(name);
                ui.ctx()
                    .accesskit_node_builder(button.id, |node| node.set_label(name));
                let keyed = key.is_some_and(|k| {
                    body_focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, k))
                });
                if (button.clicked() || keyed)
                    && let Some(range) = selection.clone().filter(|r| !r.is_empty())
                {
                    draft.doc.toggle_mark(range, mark);
                    changed_format = true;
                }
            }
            ui.separator();
            // ProseCraft's Add link: an address for the selection (or put in as its own text).
            let link = ui.button("Link").on_hover_text("Add link");
            name_field(ui, &link, "Add link");
            if link.clicked() {
                let range = selection.clone().unwrap_or(cursor..cursor);
                draft.link_prompt = Some((range, String::new(), None));
            }
            ui.separator();
            // ProseCraft's Undo and Redo: the scene's own history.
            for (label, undo) in [("Undo", true), ("Redo", false)] {
                let button = ui.button(label).on_hover_text(label);
                name_field(ui, &button, label);
                if button.clicked() {
                    edit.intents.push(Intent::BodyHistory {
                        document: document.id.clone(),
                        undo,
                    });
                }
            }
            ui.separator();
            // ProseCraft's Insert image: a picture at the caret (or the end of the text).
            let picture = ui.button("Image").on_hover_text("Insert image");
            name_field(ui, &picture, "Insert image");
            if picture.clicked() {
                let at = selection
                    .clone()
                    .map_or(draft.doc.text.chars().count(), |r| r.start);
                edit.intents.push(Intent::InsertPicture {
                    document: document.id.clone(),
                    at,
                });
            }
            // ProseCraft's comment button: a comment on the selected passage.
            let selected = selection.clone().filter(|r| !r.is_empty());
            let comment = ui
                .add_enabled(selected.is_some(), egui::Button::new("Comment"))
                .on_hover_text("Comment on the selected text");
            name_field(ui, &comment, "Comment");
            if comment.clicked()
                && let Some(range) = selected
                && let Some(anchor) = comment_anchor(draft, range)
            {
                *edit.pending_comment = Some(crate::PendingComment {
                    document: document.id.clone(),
                    anchor,
                });
                *edit.side_tab = crate::SideTab::Comments;
            }
            // ProseCraft's Insert footnote (Ctrl+Alt+F): a marker after the selection, or at
            // the end of the text.
            let footnote = ui
                .button("Footnote")
                .on_hover_text("Insert footnote (Ctrl+Alt+F)");
            name_field(ui, &footnote, "Insert footnote");
            let keyed = ui.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::ALT,
                    egui::Key::F,
                )
            });
            if footnote.clicked() || keyed {
                let at = selection
                    .clone()
                    .map_or(draft.doc.text.chars().count(), |r| r.end);
                edit.intents.push(Intent::InsertFootnote {
                    document: document.id.clone(),
                    at,
                });
            }
            // ProseCraft's Insert citation (Ctrl+Alt+C): the picker for a citation at the caret,
            // or at the end of the text.
            let cite = ui
                .button("Cite")
                .on_hover_text("Insert citation (Ctrl+Alt+C)");
            name_field(ui, &cite, "Insert citation");
            let keyed = ui.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::ALT,
                    egui::Key::C,
                )
            });
            if cite.clicked() || keyed {
                let at = selection
                    .clone()
                    .map_or(draft.doc.text.chars().count(), |r| r.start);
                *edit.citation_picker = Some(crate::citation_picker::CitationPicker::new(
                    &document.id,
                    &draft.body,
                    crate::citation_picker::Target::Insert { at },
                    &[],
                ));
            }
            // ProseCraft's "Sources…": the project's citation library.
            let sources = ui.button("Sources…").on_hover_text("Sources…");
            name_field(ui, &sources, "Sources");
            if sources.clicked() {
                *edit.sources = Some(crate::sources::SourcesDialog::default());
            }
            // Insert equation, diagram, calculation (`openRichDialog`).
            for (label, name, kind) in [
                ("Σ", "Insert equation", crate::rich_dialog::DialogKind::Math),
                (
                    "Diagram",
                    "Insert diagram",
                    crate::rich_dialog::DialogKind::Diagram,
                ),
                (
                    "Calc",
                    "Insert calculation",
                    crate::rich_dialog::DialogKind::Calc,
                ),
            ] {
                let button = ui.button(label).on_hover_text(name);
                name_field(ui, &button, name);
                if button.clicked() {
                    let caret = draft.caret.unwrap_or_else(|| draft.body.chars().count());
                    *edit.rich_dialog = Some(crate::rich_dialog::RichDialog::insert(
                        &document.id,
                        kind,
                        caret,
                    ));
                }
            }
            // ProseCraft's "MD" button, when the Markdown editor is enabled.
            if edit.markdown_enabled {
                ui.separator();
                let markdown = ui.button("MD").on_hover_text("Markdown editor");
                name_field(ui, &markdown, "Markdown editor");
                if markdown.clicked() {
                    *edit.markdown_switch = Some(true);
                }
            }
        });
    });
    if changed_format {
        draft.body_dirty = true;
        draft.changed_at = now;
        ui.memory_mut(|m| m.request_focus(body_id));
    }
    if edit.editable && !screenplay {
        let column = ui.available_width();
        crate::picture_tools::toolbar(ui, palette, &document.id, draft, column, now);
    }
    ui.add_space(14.0);

    // "Go to" from the Comments tab: select the passage.
    // Applied once the click that asked for it is over, so the click doesn't take the focus.
    let settled = ui.input(|i| !i.pointer.any_down() && !i.pointer.any_released());
    if let Some((_, start, end)) = edit
        .comment_goto
        .take_if(|(d, ..)| settled && *d == document.id)
    {
        let mut state = egui::TextEdit::load_state(ui.ctx(), body_id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(start),
                egui::text::CCursor::new(end),
            )));
        state.store(ui.ctx(), body_id);
        ui.memory_mut(|m| m.request_focus(body_id));
    }
    if let Some(chapter) = &chapter {
        for scene in &chapter.before {
            let screenplay = scene.screenplay || chapter.screenplay_project;
            chapter_scene(
                ui,
                palette,
                scene,
                screenplay,
                &edit.cites,
                &mut edit.open,
                &Look::scaled(&edit.writing),
            );
            scene_divider(ui, palette, edit.separator);
        }
    }
    let doc_for_layout = draft.doc.clone();
    let layout_labels = atom_labels(&draft.doc, &edit.cites);
    let layout_palette = *palette;
    let mut look = Look::scaled(&edit.writing);
    // Fade other paragraphs while the editor has the focus.
    if edit.writing.fade
        && !screenplay
        && ui.memory(|m| m.has_focus(body_id))
        && let Some(range) =
            egui::TextEdit::load_state(ui.ctx(), body_id).and_then(|s| s.cursor.char_range())
    {
        let (a, b) = (range.primary.index.0, range.secondary.index.0);
        look.faded = faded_lines(&draft.doc, a.min(b)..a.max(b));
    }
    let mut layouter = move |ui: &Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        if screenplay {
            return crate::screenplay_editor::galley(
                ui,
                &doc_for_layout,
                text.as_str(),
                &layout_palette,
                wrap_width,
            );
        }
        ui.fonts_mut(|f| {
            doc_galley(
                f,
                (&doc_for_layout, text.as_str()),
                &layout_palette,
                wrap_width,
                &layout_labels,
                &look,
            )
        })
    };
    link_prompt(ui, draft, now);
    let footnotes_before = footnote_ids(&draft.doc);
    copy(
        ui,
        body_id,
        draft,
        edit.clipboard_write.filter(|_| !screenplay),
        now,
    );
    paste(
        ui,
        body_id,
        &document.id,
        draft,
        (edit.clipboard_html, edit.html_paste, edit.intents, now),
        screenplay,
    );
    history_keys(ui, body_id, &document.id, edit.intents);
    let output = egui::TextEdit::multiline(&mut draft.body)
        .id(body_id)
        .frame(egui::Frame::NONE)
        .hint_text("Start writing…")
        .desired_width(f32::INFINITY)
        .desired_rows(if chapter.is_some() { 1 } else { 12 })
        .layouter(&mut layouter)
        // In a screenplay, Tab walks the element cycle instead of leaving the text.
        .lock_focus(screenplay)
        .margin(egui::Margin {
            left: 26,
            ..Default::default()
        })
        .show(ui);
    name_field(ui, &output.response, "Scene text");
    if let Some(range) = output.cursor_range {
        draft.caret = Some(range.primary.index.0);
    }
    if output.response.changed() {
        let cursor = output.cursor_range.map(|r| r.primary.index.0);
        let kept = draft.doc.apply_text_at(&draft.body, cursor);
        if kept != draft.body {
            draft.body = kept;
        }
        if screenplay && let Some(cursor) = cursor {
            crate::screenplay_editor::sync_typing(ui, draft, body_id, cursor);
        }
        draft.body_dirty = true;
        draft.changed_at = now;
        // Writing activity, as ProseCraft's `trackActivity` on each input: one more edit, and
        // the words gained.
        // `refreshCounts`: the words of the scene's HTML, equations and diagrams left out.
        let words = prosecraft_core::html::html_word_count(&draft.doc.to_html());
        draft.edits += 1;
        draft.words_added += (words as f64 - draft.words as f64).max(0.0);
        draft.words = words;
    }
    after_typing(ui, edit.writing, edit.editor_rect, edit.intents, &output);
    let dropped = drop_removed_footnotes(
        &footnotes_before,
        draft,
        (document, chapter.as_ref()),
        edit.intents,
    );
    record_body_history(
        UndoAt {
            undo: edit.undo,
            project: &edit.project_id,
            page: &edit.undo_page,
            now_ms: edit.now_ms,
        },
        (document, &dropped),
        draft,
        output.cursor_range,
    );
    let labels = atom_labels(&draft.doc, &edit.cites);
    let markers = paint_extras(
        ui,
        palette,
        &draft.doc,
        &labels,
        &output.galley,
        output.galley_pos,
        output.text_clip_rect,
    );
    picture_marks(
        ui,
        palette,
        &document.id,
        draft,
        &markers,
        &output,
        (body_id, edit.editable, now),
    );
    // A click on a footnote marker opens its editor (`openFootnotePopoverForMarker`); on a
    // citation, the picker to change it (`openCitationPickerForMarker`).
    if output.response.clicked()
        && let Some(pos) = output.response.interact_pointer_pos()
        && let Some((rect, clicked)) = markers
            .iter()
            .find(|(rect, _)| rect.expand(2.0).contains(pos))
    {
        match clicked {
            Clicked::Footnote(id) => {
                *edit.footnote_open = Some((document.id.clone(), id.clone()));
            }
            Clicked::Picture(_) => {}
            Clicked::Rich(atom) => {
                if let Some(html) = draft.doc.atoms.get(*atom) {
                    *edit.rich_dialog =
                        crate::rich_dialog::RichDialog::edit(&document.id, *atom, html);
                }
            }
            Clicked::Cite(atom) => {
                let cites = draft
                    .doc
                    .atoms
                    .get(*atom)
                    .and_then(|html| cite_data(html))
                    .unwrap_or_default();
                let mut picker = crate::citation_picker::CitationPicker::new(
                    &document.id,
                    &draft.body,
                    crate::citation_picker::Target::Edit { atom: *atom },
                    &cites,
                );
                picker.pos = Some(rect.left_bottom() + egui::vec2(0.0, 6.0));
                *edit.citation_picker = Some(picker);
            }
        }
    }
    comment_popover(
        ui,
        (&document.id, edit.editable && !screenplay),
        draft,
        &output,
        (edit.pending_comment, edit.side_tab),
    );
    text_marks(
        ui,
        palette,
        document,
        draft,
        &output,
        (&edit.marks, edit.comment_goto, edit.intents),
    );
    proofreading_marks(
        ui,
        palette,
        (edit.proofing, edit.editable, edit.now),
        draft,
        &output,
    );
    rich_dialog(
        ui,
        palette,
        (edit.rich_dialog, edit.editable, edit.now),
        draft,
        &document.id,
    );
    // A new citation's picker opens beside the caret.
    if let Some(picker) = edit
        .citation_picker
        .as_mut()
        .filter(|p| p.pos.is_none() && p.document == document.id)
    {
        let at = match picker.target {
            crate::citation_picker::Target::Insert { at } => at,
            crate::citation_picker::Target::Edit { .. } => 0,
        };
        let caret = output
            .galley
            .pos_from_cursor(egui::text::CCursor::new(at))
            .translate(output.galley_pos.to_vec2());
        picker.pos = Some(caret.left_bottom() + egui::vec2(0.0, 6.0));
    }
    let mut count = draft.words;
    if let Some(chapter) = &chapter {
        for scene in &chapter.after {
            scene_divider(ui, palette, edit.separator);
            let screenplay = scene.screenplay || chapter.screenplay_project;
            chapter_scene(
                ui,
                palette,
                scene,
                screenplay,
                &edit.cites,
                &mut edit.open,
                &Look::scaled(&edit.writing),
            );
        }
        count += chapter.words;
    }
    footer(ui, palette, count, &save_text(edit.save_state));
}

/// The words of Markdown, counted as ProseCraft counts them after `handleMarkdownInput` (the
/// rendered text, equations and diagrams left out).
fn markdown_words(source: &str) -> usize {
    prosecraft_core::html::html_word_count(&prosecraft_core::html::sanitize_html(
        &prosecraft_core::markdown::markdown_to_html(source),
    ))
}

/// What the Markdown editor's draft was read from: the content, the Markdown source and the
/// footnotes. When any of them changes underneath an untouched draft (a footnote edited in the
/// sidebar, a sync), the draft is read again, so the next keystroke can't write the old one back.
fn markdown_base(document: &prosecraft_core::project::Document) -> String {
    let notes: Vec<String> = document
        .footnotes
        .iter()
        .map(|f| format!("{}\u{1}{}", f.id, f.html))
        .collect();
    format!(
        "{}\u{0}{:?}\u{0}{}",
        document.content,
        document.markdown_source,
        notes.join("\u{2}")
    )
}

/// ProseCraft's Markdown editor: the scene's Markdown source as plain text, with "Rich text
/// editor" to go back. What is typed reaches the scene as it does from the rich editor, as
/// Markdown (`applySingleDocumentMarkdown`).
fn markdown_editor(
    ui: &mut Ui,
    palette: &Palette,
    document: &prosecraft_core::project::Document,
    chapter: Option<ChapterView<'_>>,
    edit: &mut Edit<'_>,
) {
    // In the chapter view, the whole chapter's Markdown (`chapterMarkdownSource`).
    let chapter_source = chapter.as_ref().map(|c| {
        let scenes: Vec<&prosecraft_core::project::Document> = c
            .before
            .iter()
            .copied()
            .chain(std::iter::once(document))
            .chain(c.after.iter().copied())
            .collect();
        chapter_markdown(&scenes)
    });
    // A new draft when another document opens, or when the scene changed underneath an
    // untouched draft. A chapter's source is compared as its sections, as it is written back.
    let stale = edit.draft.as_ref().is_none_or(|d| {
        d.document != document.id
            || d.markdown.is_none()
            || d.chapter != chapter.as_ref().map(|c| c.id.clone())
            || (!d.is_dirty()
                && match &chapter_source {
                    Some(source) => {
                        d.markdown.as_deref().map(|m| {
                            prosecraft_core::chapter::join_chapter_markdown(
                                &prosecraft_core::chapter::split_chapter_markdown(m),
                            )
                        }) != Some(source.clone())
                    }
                    None => d.title != document.title || d.markdown_base != markdown_base(document),
                })
    });
    if stale {
        let source = chapter_source
            .clone()
            .unwrap_or_else(|| document.markdown());
        let doc = RichDoc::from_html(&document.content).unwrap_or_default();
        *edit.draft = Some(Draft {
            document: document.id.clone(),
            title: document.title.clone(),
            words: markdown_words(&source),
            body: doc.text.clone(),
            doc,
            source: document.content.clone(),
            title_dirty: false,
            body_dirty: false,
            changed_at: edit.now,
            edits: 0,
            words_added: 0.0,
            markdown: Some(source),
            markdown_base: markdown_base(document),
            chapter: chapter.as_ref().map(|c| c.id.clone()),
            caret: None,
            undo_caret: None,
            link_prompt: None,
        });
    }
    let now = edit.now;
    let Some(draft) = edit.draft.as_mut() else {
        return;
    };
    drop_atom_paste(ui);
    if let Some(chapter) = &chapter {
        chapter_title(
            ui,
            palette,
            &chapter.id,
            &chapter.label,
            &chapter.title,
            edit.intents,
        );
    } else {
        let title = ui.add(
            egui::TextEdit::singleline(&mut draft.title)
                .font(fonts::serif(38.0))
                .text_color(palette.text_heading)
                .frame(egui::Frame::NONE)
                .hint_text("Untitled scene")
                .desired_width(f32::INFINITY),
        );
        name_field(ui, &title, "Scene title");
        if title.changed() {
            draft.title_dirty = true;
            draft.changed_at = now;
        }
    }
    ui.add_space(10.0);
    // ProseCraft's Markdown utilities: back to the rich text editor.
    ui.horizontal(|ui| {
        let rich = ui.button("Rich text editor");
        ui.ctx().accesskit_node_builder(rich.id, |node| {
            node.set_label("Rich text editor");
            node.set_toggled(egui::accesskit::Toggled::True);
        });
        if rich.clicked() {
            *edit.markdown_switch = Some(false);
        }
    });
    ui.add_space(10.0);
    let Some(source) = draft.markdown.as_mut() else {
        return;
    };
    let body_id = ui.make_persistent_id(("markdown-source", &document.id));
    let output = egui::TextEdit::multiline(source)
        .id(body_id)
        .font(fonts::markdown(15.0))
        .text_color(palette.text_body)
        .frame(egui::Frame::NONE)
        .hint_text("Start writing…")
        .desired_width(f32::INFINITY)
        .desired_rows(16)
        .show(ui);
    name_field(ui, &output.response, "Markdown source");
    if output.response.changed() {
        draft.body_dirty = true;
        draft.changed_at = now;
        let words = markdown_words(source);
        draft.edits += 1;
        draft.words_added += (words as f64 - draft.words as f64).max(0.0);
        draft.words = words;
    }
    let count = draft.words;
    footer(ui, palette, count, &save_text(edit.save_state));
}

/// The box of character `index`'s glyph (its advance across, its row's height), from the laid
/// out rows themselves: a cursor at the start of a row can report the end of the row before.
fn glyph_rect(galley: &egui::Galley, index: usize) -> Option<egui::Rect> {
    let mut start = 0;
    for placed in &galley.rows {
        let count = placed.char_count_including_newline().0;
        if index < start + count {
            let glyph = placed.row.glyphs.get(index - start)?;
            let row = placed.rect();
            let left = row.min.x + glyph.pos.x;
            return Some(egui::Rect::from_min_max(
                egui::pos2(left, row.min.y),
                egui::pos2(left + glyph.advance_width, row.max.y),
            ));
        }
        start += count;
    }
    None
}

/// A citation marker's sources, from its `data-cites`.
pub(crate) fn cite_data(html: &str) -> Option<Vec<prosecraft_core::cite::CiteRefItem>> {
    let raw = prosecraft_core::html::parse_fragment(html)
        .into_iter()
        .find_map(|n| match n {
            prosecraft_core::html::Node::Element(e) => e.attr("data-cites").map(str::to_owned),
            prosecraft_core::html::Node::Text(_) => None,
        })?;
    prosecraft_core::cite::parse_cite_data(Some(&raw))
}

/// A marker the writer can click.
pub(crate) enum Clicked {
    /// A footnote reference: its id.
    Footnote(String),
    /// A citation: its atom's index.
    Cite(usize),
    /// A drawn equation, diagram or calculation: its atom's index.
    Rich(usize),
    /// A picture: its atom's index.
    Picture(usize),
}

/// What the text itself can't show: list bullets and numbers, the quote bar, footnote
/// markers as their numbers, and citations as their labels. Painted over the laid-out text, so the caret, selection and the
/// accessibility tree still see the real text.
pub(crate) fn paint_extras(
    ui: &Ui,
    palette: &Palette,
    doc: &RichDoc,
    labels: &[AtomLabel],
    galley: &egui::Galley,
    origin: egui::Pos2,
    clip: egui::Rect,
) -> Vec<(egui::Rect, Clicked)> {
    let mut markers = Vec::new();
    // The text area is as wide as its text; kept blocks span the column.
    let column = ui.max_rect().max.x.max(clip.max.x);
    let painter = ui.painter_at(egui::Rect::from_min_max(
        clip.min - egui::vec2(30.0, 0.0),
        egui::pos2(column, clip.max.y),
    ));
    // The lists open (as the HTML nests them) and how many items each has had.
    let mut lists: Vec<(&str, u32)> = Vec::new();
    for (line, (start, end)) in doc.line_ranges().into_iter().enumerate() {
        let kind = doc
            .lines
            .get(line)
            .copied()
            .unwrap_or(LineKind::Paragraph)
            .shown();
        let rect = galley
            .pos_from_cursor(egui::text::CCursor::new(start))
            .translate(origin.to_vec2());
        let last_row = galley
            .pos_from_cursor(egui::text::CCursor::new(end))
            .translate(origin.to_vec2());
        let wanted = kind.lists();
        let common = lists
            .iter()
            .zip(&wanted)
            .take_while(|((open, _), want)| open == *want)
            .count();
        lists.truncate(common);
        // A second block in one item, or a paragraph straight in a list, has no marker and
        // isn't counted.
        let starts = doc.starts_item(line);
        if !wanted.is_empty() {
            if lists.len() == wanted.len() {
                if let Some(last) = lists.last_mut() {
                    last.1 += u32::from(starts);
                }
            } else {
                lists.extend(wanted[common..].iter().map(|l| (*l, 1)));
                if let Some(last) = lists.last_mut() {
                    last.1 = u32::from(starts);
                }
            }
        }
        // The browser's markers: numbers, or a disc, circle and square by depth.
        let marker = match (kind.list_depth(), lists.last()) {
            _ if !starts => None,
            (Some(_), Some(("ol", n))) => Some(format!("{n}.")),
            (Some(depth), Some(_)) => {
                let at = egui::pos2(rect.min.x - 11.0, rect.center().y);
                match depth {
                    0 => {
                        painter.circle_filled(at, 2.5, palette.text_muted);
                    }
                    1 => {
                        painter.circle_stroke(at, 2.5, egui::Stroke::new(1.0, palette.text_muted));
                    }
                    _ => {
                        painter.rect_filled(
                            egui::Rect::from_center_size(at, egui::vec2(4.5, 4.5)),
                            0.0,
                            palette.text_muted,
                        );
                    }
                }
                None
            }
            _ => None,
        };
        if let Some(marker) = marker {
            painter.text(
                egui::pos2(rect.min.x - 8.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                marker,
                fonts::serif(16.0),
                palette.text_muted,
            );
        }
        if kind == LineKind::Quote {
            // `border-left: 2px`, the padding's width before the text.
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.min.x - QUOTE_INDENT, rect.min.y),
                egui::pos2(rect.min.x - QUOTE_INDENT + 2.0, last_row.max.y),
            );
            painter.rect_filled(bar, 1.0, palette.border_control);
        }
    }
    markers.extend(paint_atoms(
        &painter, palette, doc, labels, galley, origin, column,
    ));
    markers
}

/// A kept block across the column, in its row: a rule as a line; anything else as a panel
/// with its kind and its text (code, a table's rows, an equation's or diagram's source).
fn paint_block(
    painter: &egui::Painter,
    palette: &Palette,
    preview: &prosecraft_core::richtext::BlockPreview,
    row: egui::Rect,
    right_edge: f32,
) {
    let left = row.min.x;
    let right = right_edge.max(left + 40.0);
    // An equation or calculation drawn, centred (`.scene .math-block { text-align: center }`).
    if let Some(kind) = crate::rich_view::block_kind(preview)
        && let Ok((size, pixels)) =
            crate::rich_view::drawn(kind, &preview.source, palette.text_body)
    {
        let width = right - left;
        let fit = if size.x > width { width / size.x } else { 1.0 };
        let shown = size * fit;
        let rect = egui::Rect::from_center_size(
            egui::pos2(
                (left + right) / 2.0,
                row.min.y + crate::rich_view::BLOCK_MARGIN + shown.y / 2.0,
            ),
            shown,
        );
        let texture = crate::rich_view::texture(painter.ctx(), &pixels);
        painter.image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        return;
    }
    if preview.kind == prosecraft_core::richtext::BlockKind::Rule {
        let y = row.center().y;
        painter.line_segment(
            [egui::pos2(left, y), egui::pos2(right, y)],
            egui::Stroke::new(1.0, palette.border_control),
        );
        return;
    }
    let panel = egui::Rect::from_min_max(
        egui::pos2(left, row.min.y + 4.0),
        egui::pos2(right, row.max.y - 4.0),
    );
    painter.rect_filled(panel, 6.0, palette.control_bg);
    painter.rect_stroke(
        panel,
        6.0,
        egui::Stroke::new(1.0, palette.border),
        egui::StrokeKind::Inside,
    );
    let inner = painter.with_clip_rect(panel.shrink(1.0).intersect(painter.clip_rect()));
    inner.text(
        egui::pos2(panel.min.x + 10.0, panel.min.y + 6.0),
        egui::Align2::LEFT_TOP,
        preview.kind.name().to_uppercase(),
        egui::FontId::proportional(10.0),
        palette.text_muted,
    );
    let mut y = panel.min.y + 6.0 + 18.0;
    let lines = preview
        .lines
        .iter()
        .map(String::as_str)
        .chain(preview.more.then_some("…"));
    for line in lines {
        inner.text(
            egui::pos2(panel.min.x + 10.0, y),
            egui::Align2::LEFT_TOP,
            line,
            egui::FontId::monospace(13.0),
            palette.text_body,
        );
        y += BLOCK_LINE;
    }
}

/// A picture in its row, at its layout's size and alignment; its alternative text in a frame
/// when the image can't be drawn.
fn paint_picture(
    painter: &egui::Painter,
    palette: &Palette,
    html: &str,
    picture: &crate::pictures::PictureInfo,
    row: egui::Rect,
    right_edge: f32,
) -> egui::Rect {
    let left = row.min.x;
    let width = (right_edge - left).max(40.0);
    let size = picture.drawn(width);
    let x = match (picture.layout.wrap, picture.layout.align) {
        ("wrap-right", _) | ("block", "right") => right_edge - size.x,
        ("block", "center") => left + (width - size.x) / 2.0,
        _ => left,
    };
    let rect = egui::Rect::from_min_size(egui::pos2(x, row.min.y + crate::pictures::MARGIN), size);
    draw_picture(painter, palette, html, picture, rect);
    rect
}

/// A picture's image in `rect`, or its alternative text in a frame when it can't be drawn.
fn draw_picture(
    painter: &egui::Painter,
    palette: &Palette,
    html: &str,
    picture: &crate::pictures::PictureInfo,
    rect: egui::Rect,
) {
    match crate::pictures::texture(painter.ctx(), html, picture) {
        Some(texture) => {
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        None => {
            painter.rect_stroke(
                rect,
                4.0,
                egui::Stroke::new(1.0, palette.border_control),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                &picture.alt,
                egui::FontId::proportional(13.0),
                palette.text_muted,
            );
        }
    }
}

/// Each atom's label, painted in its character's box (footnote numbers, citation labels,
/// equations), and the markers that can be clicked.
pub(crate) fn paint_atoms(
    painter: &egui::Painter,
    palette: &Palette,
    doc: &RichDoc,
    labels: &[AtomLabel],
    galley: &egui::Galley,
    origin: egui::Pos2,
    right_edge: f32,
) -> Vec<(egui::Rect, Clicked)> {
    let mut markers = Vec::new();
    // The wrapped pictures, where the layout put them (see `picture_flow`).
    let floats = floats(&doc.text, labels, galley.job.wrap.max_width);
    let wrapped: Vec<(usize, egui::Rect)> = floats
        .iter()
        .map(|f| f.atom)
        .zip(crate::picture_flow::placed(galley, &floats))
        .collect();
    let atoms = doc
        .text
        .chars()
        .enumerate()
        .filter(|(_, c)| *c == ATOM)
        .map(|(i, _)| i);
    for (atom, ((index, label), html)) in atoms.zip(labels).zip(&doc.atoms).enumerate() {
        let before = galley
            .pos_from_cursor(egui::text::CCursor::new(index))
            .translate(origin.to_vec2());
        let after = galley
            .pos_from_cursor(egui::text::CCursor::new(index + 1))
            .translate(origin.to_vec2());
        let right = if (after.min.y - before.min.y).abs() < 1.0 {
            after.min.x
        } else {
            before.min.x + 14.0
        };
        let center = egui::pos2((before.min.x + right) / 2.0, before.center().y);
        match label {
            AtomLabel::Footnote(number) => {
                // A superscript number, as ProseCraft shows footnote references.
                let at = egui::pos2(center.x, before.min.y + before.height() * 0.3);
                if let Some(id) = prosecraft_core::footnotes::extract_ids(html)
                    .into_iter()
                    .next()
                {
                    markers.push((
                        egui::Rect::from_min_max(
                            before.min,
                            egui::pos2(right.max(before.min.x + 8.0), before.max.y),
                        ),
                        Clicked::Footnote(id),
                    ));
                }
                painter.text(
                    at,
                    egui::Align2::CENTER_CENTER,
                    number,
                    egui::FontId::proportional(ATOM_FONT - 2.0),
                    palette.link,
                );
            }
            AtomLabel::Math(tex)
                if let Ok((size, pixels)) = crate::rich_view::drawn(
                    prosecraft_core::rich::RichKind::MathInline,
                    tex,
                    palette.text_body,
                ) =>
            {
                let rect =
                    egui::Rect::from_center_size(egui::pos2(center.x, before.center().y), size);
                markers.push((rect, Clicked::Rich(atom)));
                let texture = crate::rich_view::texture(painter.ctx(), &pixels);
                painter.image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            AtomLabel::Math(_) => {
                let chip = egui::Rect::from_center_size(
                    center,
                    egui::vec2((right - before.min.x).max(16.0), ATOM_FONT + 4.0),
                );
                markers.push((chip, Clicked::Rich(atom)));
                painter.rect_filled(chip, 4.0, palette.selected_bg);
                painter.text(
                    chip.center(),
                    egui::Align2::CENTER_CENTER,
                    "ƒ",
                    egui::FontId::proportional(ATOM_FONT - 2.0),
                    palette.selected_text,
                );
            }
            AtomLabel::Other => {}
            AtomLabel::Block(preview) => {
                paint_block(painter, palette, preview, before, right_edge);
                // A click on an equation, diagram or calculation edits it (`openRichDialog`).
                if crate::rich_view::block_kind(preview).is_some()
                    && let Some(cell) = glyph_rect(galley, index)
                {
                    let row = cell.translate(origin.to_vec2());
                    markers.push((
                        egui::Rect::from_min_max(row.min, egui::pos2(right_edge, row.max.y)),
                        Clicked::Rich(atom),
                    ));
                }
            }
            AtomLabel::Picture(picture)
                if let Some((_, rect)) = wrapped.iter().find(|(a, _)| *a == atom) =>
            {
                let rect = rect.translate(origin.to_vec2());
                draw_picture(painter, palette, html, picture, rect);
                markers.push((rect, Clicked::Picture(atom)));
            }
            AtomLabel::Picture(picture) => {
                // Its own glyph's cell: at a row break the caret before it is at the end of the
                // row above.
                if let Some(cell) = glyph_rect(galley, index) {
                    let cell = cell.translate(origin.to_vec2());
                    // The column it was laid out in (see `atom_sizes`).
                    let wrap = galley.job.wrap.max_width;
                    let right = if wrap.is_finite() {
                        (origin.x + wrap).min(right_edge)
                    } else {
                        right_edge
                    };
                    let rect = paint_picture(painter, palette, html, picture, cell, right);
                    markers.push((rect, Clicked::Picture(atom)));
                }
            }
            AtomLabel::Cite(text) => {
                // ProseCraft's `span.cite-ref`: the label on a faint accent tint, over the atom's
                // own character, which the layout made as wide as the label.
                let font = cite_font();
                let Some(cell) = glyph_rect(galley, index) else {
                    continue;
                };
                let cell = cell.translate(origin.to_vec2());
                let chip = egui::Rect::from_min_max(
                    egui::pos2(cell.min.x + 1.0, cell.center().y - font.size * 0.62),
                    egui::pos2(cell.max.x - 1.0, cell.center().y + font.size * 0.62),
                );
                painter.rect_filled(chip, 4.0, palette.accent.gamma_multiply(0.11));
                markers.push((chip, Clicked::Cite(atom)));
                painter.text(
                    egui::pos2(chip.min.x + 5.0, chip.center().y),
                    egui::Align2::LEFT_CENTER,
                    text,
                    font,
                    palette.accent_strong,
                );
            }
        }
    }
    markers
}

/// A text field that saves when it is left (or Enter, for a single line), so typing isn't a
/// change per keystroke. Returns the new text then.
#[allow(clippy::too_many_arguments)]
fn saved_field(
    ui: &mut Ui,
    salt: (&str, &str),
    value: &str,
    hint: &str,
    name: &str,
    multiline: bool,
    limit: usize,
    editable: bool,
    focus: bool,
) -> Option<String> {
    let (saved, response) =
        saved_field_response(ui, salt, value, hint, name, multiline, limit, editable);
    if focus {
        response.request_focus();
    }
    saved
}

/// [`saved_field`] with the field's response.
#[allow(clippy::too_many_arguments)]
pub(crate) fn saved_field_response(
    ui: &mut Ui,
    salt: (&str, &str),
    value: &str,
    hint: &str,
    name: &str,
    multiline: bool,
    limit: usize,
    editable: bool,
) -> (Option<String>, egui::Response) {
    let id = ui.make_persistent_id(("saved-field", salt.0, salt.1));
    let mut text = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| value.to_owned());
    let field = if multiline {
        egui::TextEdit::multiline(&mut text).desired_rows(2)
    } else {
        egui::TextEdit::singleline(&mut text)
    };
    let response = ui.add_enabled(
        editable,
        field
            .id(id.with("edit"))
            .hint_text(hint)
            .char_limit(limit)
            .desired_width(f32::INFINITY),
    );
    name_field(ui, &response, name);
    if response.changed() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    }
    let mut saved = None;
    if response.lost_focus() {
        ui.data_mut(|d| d.remove_temp::<String>(id));
        if text != value {
            saved = Some(text);
        }
    }
    (saved, response)
}

/// ProseCraft's Timeline view: the story's moments in order, each with a title, a date, a
/// time and a description, moved with the arrows (ProseCraft also drags them; that comes
/// later) and removed with ×.
/// `updateDrag` and `finishDrag`: while a moment is dragged, a line before or after the moment
/// under the pointer (half way down it); letting go puts it there.
fn timeline_drop(
    ui: &Ui,
    palette: &Palette,
    drag_id: egui::Id,
    cards: &[(egui::Rect, String)],
    edit: &mut Edit<'_>,
) {
    let Some(dragging) = ui.data(|d| d.get_temp::<String>(drag_id)) else {
        return;
    };
    let pos = ui.ctx().pointer_latest_pos();
    let target = pos.and_then(|p| {
        cards
            .iter()
            .find(|(r, id)| *id != dragging && r.y_range().contains(p.y))
            .map(|(r, id)| (id.clone(), p.y > r.center().y, *r))
    });
    if let Some((_, after, rect)) = &target {
        let y = if *after {
            rect.bottom() + 4.0
        } else {
            rect.top() - 4.0
        };
        ui.painter()
            .hline(rect.x_range(), y, egui::Stroke::new(2.0, palette.accent));
    }
    if ui.input(|i| i.pointer.any_released() || !i.pointer.any_down()) {
        ui.data_mut(|d| d.remove::<String>(drag_id));
        if let Some((target, after, _)) = target
            && edit.editable
        {
            edit.intents.push(Intent::ReorderMoment {
                id: dragging,
                target,
                after,
            });
        }
    } else {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
}

fn timeline(ui: &mut Ui, palette: &Palette, project: &Project, edit: &mut Edit<'_>) {
    use prosecraft_core::timeline::Field;
    ui.add_space(28.0);
    let count = project.timeline.len();
    let mut add = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            eyebrow(ui, palette, "Follow your story");
            ui.label(RichText::new("Timeline").font(fonts::serif(32.0)).color(palette.text_heading));
            ui.label(RichText::new("Map the moments that move your story forward. Drag nodes to change their order.").color(palette.text_muted));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::BOTTOM), |ui| {
            add |= ui.add_enabled(edit.editable, egui::Button::new("+ Add moment")).clicked();
            ui.label(RichText::new(format!("{count} {}", if count == 1 { "moment" } else { "moments" })).size(12.0).color(palette.text_muted));
        });
    });
    ui.separator();
    ui.add_space(8.0);
    let focus_id = ui.make_persistent_id("timeline-focus-new");
    // After "Add moment", the new (last) moment's title takes the focus, as in ProseCraft.
    let focus_new = ui
        .data_mut(|d| d.remove_temp::<usize>(focus_id))
        .is_some_and(|n| n < count);
    if count == 0 {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("No moments yet")
                    .font(fonts::serif(22.0))
                    .color(palette.text_heading),
            );
            ui.label(
                RichText::new("Add your first event to begin charting the sequence of your story.")
                    .color(palette.text_muted),
            );
        });
    }
    // `startDrag`: the moment whose handle is being dragged.
    let drag_id = ui.make_persistent_id("timeline-drag");
    let mut cards = Vec::new();
    for (index, node) in project.timeline.iter().enumerate() {
        let label = if node.title.trim().is_empty() {
            format!("timeline moment {}", index + 1)
        } else {
            node.title.trim().to_owned()
        };
        let shown = card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if edit.editable {
                    let handle = ui
                        .add(
                            egui::Button::new("⠿")
                                .frame(false)
                                .sense(egui::Sense::drag()),
                        )
                        .on_hover_text("Drag to reorder");
                    name_field(ui, &handle, &format!("Drag {label} to reorder"));
                    if handle.drag_started() {
                        ui.data_mut(|d| d.insert_temp(drag_id, node.id.clone()));
                    }
                    if handle.hovered() || handle.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    }
                }
                ui.label(
                    RichText::new(format!("{:02}", index + 1))
                        .font(fonts::semibold(12.0))
                        .color(palette.eyebrow),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(edit.editable, |ui| {
                        let remove = ui.button("×").on_hover_text("Remove moment");
                        name_field(ui, &remove, &format!("Remove {label}"));
                        if remove.clicked() {
                            edit.intents.push(Intent::RemoveMoment(node.id.clone()));
                        }
                        let later = ui
                            .add_enabled(index + 1 < count, egui::Button::new("↓"))
                            .on_hover_text("Move later");
                        name_field(ui, &later, &format!("Move {label} later"));
                        if later.clicked() {
                            edit.intents.push(Intent::MoveMoment {
                                id: node.id.clone(),
                                offset: 1,
                            });
                        }
                        let earlier = ui
                            .add_enabled(index > 0, egui::Button::new("↑"))
                            .on_hover_text("Move earlier");
                        name_field(ui, &earlier, &format!("Move {label} earlier"));
                        if earlier.clicked() {
                            edit.intents.push(Intent::MoveMoment {
                                id: node.id.clone(),
                                offset: -1,
                            });
                        }
                    });
                    let focus = focus_new && index + 1 == count;
                    if let Some(title) = saved_field(
                        ui,
                        ("title", &node.id),
                        &node.title,
                        "What happens here?",
                        &format!("Title for timeline moment {}", index + 1),
                        false,
                        Field::Title.limit(),
                        edit.editable,
                        focus,
                    ) {
                        edit.intents.push(Intent::SetMoment {
                            id: node.id.clone(),
                            field: Field::Title,
                            value: title,
                        });
                    }
                });
            });
            ui.columns(2, |cols| {
                cols[0].label(
                    RichText::new("Date")
                        .size(11.5)
                        .color(palette.text_secondary),
                );
                if let Some(date) = saved_field(
                    &mut cols[0],
                    ("date", &node.id),
                    &node.date,
                    "Day 12, Third Age, 14 May 1842…",
                    &format!("Date for {label}"),
                    false,
                    Field::Date.limit(),
                    edit.editable,
                    false,
                ) {
                    edit.intents.push(Intent::SetMoment {
                        id: node.id.clone(),
                        field: Field::Date,
                        value: date,
                    });
                }
                cols[1].label(
                    RichText::new("Time")
                        .size(11.5)
                        .color(palette.text_secondary),
                );
                if let Some(time) = saved_field(
                    &mut cols[1],
                    ("time", &node.id),
                    &node.time,
                    "4:30 PM, dawn, midnight…",
                    &format!("Time for {label}"),
                    false,
                    Field::Time.limit(),
                    edit.editable,
                    false,
                ) {
                    edit.intents.push(Intent::SetMoment {
                        id: node.id.clone(),
                        field: Field::Time,
                        value: time,
                    });
                }
            });
            if let Some(description) = saved_field(
                ui,
                ("description", &node.id),
                &node.description,
                "Describe the moment, who is involved, and what changes…",
                &format!("Description for {label}"),
                true,
                Field::Description.limit(),
                edit.editable,
                false,
            ) {
                edit.intents.push(Intent::SetMoment {
                    id: node.id.clone(),
                    field: Field::Description,
                    value: description,
                });
            }
        });
        cards.push((shown.response.rect, node.id.clone()));
        ui.add_space(8.0);
    }
    timeline_drop(ui, palette, drag_id, &cards, edit);
    if count > 0 {
        add |= ui
            .add_enabled(
                edit.editable,
                egui::Button::new("+ Add moment — Place another event on your timeline."),
            )
            .clicked();
    }
    if add {
        edit.intents.push(Intent::AddMoment);
        ui.data_mut(|d| d.insert_temp(focus_id, count));
    }
}

#[cfg(test)]
mod tests {
    use super::{Look, doc_galley, grouped};
    use crate::theme::Palette;
    use egui::text::CCursor;
    use prosecraft_core::richtext::RichDoc;

    const HEBREW: &str = "הגאות עלתה על הבוץ בזמן שהפנסים נדלקו לאורך חומת הנמל, אחד אחד, \
        עד שכל העיר הוארה והסירות שכבו בשקט.";

    #[test]
    fn a_citation_label_has_its_room_where_the_browser_measures_the_atom_as_nothing() {
        // eframe's web fallback asks the browser for glyphs no font has; Chromium draws
        // nothing for U+FFFC and measures it as nothing. Here a fallback that does the same.
        use super::{AtomLabel, glyph_rect};
        let browser = egui::GlyphRasterizer::new("test::browser", |request| {
            (request.cluster == "\u{FFFC}").then(|| egui::RasterizedGlyph {
                bitmap: egui::GlyphBitmap {
                    image: egui::ColorImage::filled([1, 1], egui::Color32::TRANSPARENT),
                    offset_px: egui::Vec2::ZERO,
                    is_color: false,
                },
                advance_px: 0.0,
            })
        });
        let doc = RichDoc::from_html(
            r#"<p>Then <span class="cite-ref" data-cites="[{&quot;id&quot;:&quot;smith2020&quot;}]">(Smith 2020)</span>.</p>"#,
        )
        .expect("doc");
        let labels = [AtomLabel::Cite("(Smith, 2020)".into())];
        let ctx = egui::Context::default();
        crate::setup_for_tests(&ctx);
        ctx.add_glyph_rasterizer(browser);
        ctx.begin_pass(egui::RawInput::default());
        let (galley, label) = ctx.fonts_mut(|f| {
            let galley = doc_galley(
                f,
                (&doc, &doc.text),
                &Palette::LIGHT,
                600.0,
                &labels,
                &Look::default(),
            );
            let label = f
                .layout_no_wrap(
                    "(Smith, 2020)".into(),
                    super::cite_font(),
                    egui::Color32::WHITE,
                )
                .size()
                .x;
            (galley, label)
        });
        ctx.end_pass().textures_delta.clear();
        let at = doc
            .text
            .chars()
            .position(|c| c == super::ATOM)
            .expect("the citation");
        let cell = glyph_rect(&galley, at).expect("its cell");
        let then = glyph_rect(&galley, at - 1).expect("the space before it");
        let stop = glyph_rect(&galley, at + 1).expect("the full stop");
        // As wide as its label and padding, after "Then " and before the full stop.
        assert!(
            cell.width() >= label + 11.0,
            "{cell:?} for a label {label} wide"
        );
        assert!(then.max.x <= cell.min.x + 1.0, "{then:?} {cell:?}");
        assert!(cell.max.x <= stop.min.x + 1.0, "{cell:?} {stop:?}");
    }

    #[test]
    fn right_to_left_paragraphs_are_set_as_prosecraft_shows_them() {
        // ProseCraft sets no `dir`: every paragraph is left-to-right at its base and set on the
        // left, its Arabic or Hebrew words still ordered right-to-left.
        let html = format!("<p>{HEBREW} {HEBREW}</p><p>שלום abc def אבג</p><p>Plain words.</p>");
        let doc = RichDoc::from_html(&html).expect("doc");
        let ctx = egui::Context::default();
        crate::setup_for_tests(&ctx);
        ctx.begin_pass(egui::RawInput::default());
        let galley = ctx.fonts_mut(|f| {
            doc_galley(
                f,
                (&doc, &doc.text),
                &Palette::LIGHT,
                400.0,
                &[],
                &Look::default(),
            )
        });
        ctx.end_pass().textures_delta.clear();
        assert!(galley.rows.len() >= 4);
        for row in &galley.rows {
            assert!(!row.rtl_paragraph);
            assert!(row.rect().left().abs() < 0.5, "{:?}", row.rect());
        }
        // Within a row of Hebrew, the first letter is on the right.
        let first = &galley.rows[0];
        let x = |i: usize| first.glyphs[i].pos.x;
        assert!(x(0) > x(4), "{} {}", x(0), x(4));
        // A mixed line orders its runs left to right: "שלום" first, "abc def", then "אבג".
        let mixed = galley
            .rows
            .iter()
            .find(|r| r.glyphs.first().is_some_and(|g| g.chr == 'ש') && r.glyphs.len() < 20)
            .expect("the mixed line");
        let at = |c: char| {
            mixed
                .glyphs
                .iter()
                .find(|g| g.chr == c)
                .map_or(f32::NAN, |g| g.pos.x)
        };
        assert!(at('ש') < at('a') && at('a') < at('f') && at('f') < at('א'));
        // Every caret position maps back from where it is drawn.
        let count = doc.text.chars().count();
        for i in 0..=count {
            let rect = galley.pos_from_cursor(CCursor::new(i));
            let back = galley.cursor_from_pos(rect.center().to_vec2()).index.0;
            assert!(back <= count);
        }
    }

    #[test]
    fn groups_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(80_000), "80,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }

    #[test]
    fn atoms_alone_copy_as_the_browser_writes_them_and_paste_back() {
        use super::{clipboard_holds, clipboard_text, is_atom_text};
        let note = r#"<p><sup class="footnote-ref" data-footnote-id="fn-abc123" contenteditable="false"></sup></p>"#;
        let cite = r#"<p><span class="cite-ref" data-cites="[{&quot;id&quot;:&quot;smith2020&quot;}]" contenteditable="false"></span></p>"#;
        let picture = r#"<p><img src="data:image/png;base64,AA==" alt="A goat"></p>"#;
        let bare = r#"<p><img src="data:image/png;base64,AA=="></p>"#;
        // What Chromium writes as text/plain for these alone in ProseCraft's editor.
        assert_eq!(clipboard_text(note), "");
        assert_eq!(clipboard_text(cite), "");
        assert_eq!(clipboard_text(picture), "A goat");
        assert_eq!(clipboard_text(bare), "");
        assert_eq!(clipboard_text("<hr>"), "\n");
        assert_eq!(clipboard_text("<p><em>dark</em></p>"), "dark");
        assert_eq!(
            clipboard_text(r#"<p><span class="math-inline" data-tex="x^2"></span></p>"#),
            "$x^2$"
        );
        // A text pasted with them is theirs: a picture's description, a rule's newline.
        assert!(clipboard_holds("A goat", picture));
        assert!(!clipboard_holds("A sheep", picture));
        assert!(clipboard_holds("\n", "<hr>"));
        assert!(!clipboard_holds("\n", "<p>Something else</p>"));
        assert!(clipboard_holds("dark", "<p><em>dark</em></p>"));
        assert!(!clipboard_holds("light", "<p><em>dark</em></p>"));
        // The object characters earlier builds wrote still paste their atoms back.
        assert!(is_atom_text("\u{FFFC}\u{FFFC}\n"));
        assert!(!is_atom_text("\u{FFFC} and words"));
        assert!(!is_atom_text(" "));
        assert!(clipboard_holds("\u{FFFC}", note));
        assert!(!clipboard_holds("\u{FFFC}", "<p>Something else</p>"));
    }
}
