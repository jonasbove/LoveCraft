//! The main panel: the view switch and the Outline, Write, Storyboard, Timeline and Publish views.

use egui::{RichText, Ui};
use prosecraft_core::edit::chapter_label;
use prosecraft_core::html::{
    PlainTextOptions, RichText as RichMode, count_words, html_to_plain_text,
};
use prosecraft_core::project::Project;
use prosecraft_core::richtext::{ATOM, LineKind, Mark, RichDoc};

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

/// The parts of the state a view changes, borrowed apart from the project it shows.
struct Edit<'a> {
    editable: bool,
    draft: &'a mut Option<Draft>,
    intents: &'a mut Vec<Intent>,
    save_state: &'a SaveState,
    now: f64,
    /// The editor's Comment button: the passage chosen, and the sidebar tab to show.
    pending_comment: &'a mut Option<crate::PendingComment>,
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
}

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    ui.add_space(14.0);
    let labels: Vec<&str> = WorkspaceView::ALL.iter().map(|v| v.label()).collect();
    let current = WorkspaceView::ALL
        .iter()
        .position(|v| *v == state.view)
        .unwrap_or(0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        if let Some(index) = segmented(ui, &palette, "workspace-tab", &labels, current, 13.0) {
            state.view = WorkspaceView::ALL.get(index).copied().unwrap_or_default();
        }
        if state.library.is_some() {
            let search = ui.button("Search").on_hover_text("Search project (Ctrl+F)");
            name_field(ui, &search, "Search project");
            if search.clicked() {
                state.find = Some(crate::find::FindState::new());
            }
        }
    });
    if let Some(status) = &state.status {
        ui.horizontal(|ui| {
            ui.add_space(18.0);
            ui.label(RichText::new(status).size(12.0).color(palette.text_muted));
        });
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
    if *view == WorkspaceView::Publish {
        let mut edit = crate::publish::PublishEdit {
            editable: *editable,
            now_ms: *now_ms,
            book_id,
            intents,
            requests,
        };
        crate::publish::show(ui, &palette, &entry.stored, project, &mut edit);
        return;
    }
    let mut edit = Edit {
        editable: *editable,
        draft,
        intents,
        save_state,
        now: *now,
        pending_comment,
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
        cites: {
            let options = prosecraft_core::publish::PublishOptions::read(
                entry
                    .stored
                    .get("project")
                    .and_then(|p| p.get("publishOptions")),
            );
            CiteStyle {
                items: &project.citations,
                style: options.citation_style,
                locale: options.citation_locale,
                cache: cite_labels,
            }
        },
    };
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            column(ui, |ui| match view {
                WorkspaceView::Outline => chosen = outline(ui, &palette, project, &mut edit),
                WorkspaceView::Write => write(ui, &palette, project, selection.as_ref(), &mut edit),
                // Shown above, as a full-width canvas.
                WorkspaceView::Storyboard => {}
                WorkspaceView::Timeline => timeline(ui, &palette, project, &mut edit),
                // Shown above, in its own two-column layout.
                WorkspaceView::Publish => {}
            });
        });
    if let Some(selection) = chosen {
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
    ui.label(
        RichText::new(
            "Create a project with Create project, or open a ProseCraft backup: drop a .prosecraft file on this window, or start prosecraft-rs with the file's path.",
        )
        .size(14.0)
        .color(palette.text_body),
    );
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

/// The manuscript text, read-only: serif, ProseCraft's 1.85 line spacing, a gap between paragraphs.
fn manuscript(ui: &mut Ui, palette: &Palette, text: &str) {
    for paragraph in text.split("\n\n") {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = ui.available_width();
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
                    markdown_editor(ui, palette, document, edit);
                    return;
                }
                match RichDoc::from_html(&document.content).filter(|_| edit.editable) {
                    Some(doc) => {
                        editor(ui, palette, document, doc, screenplay, edit);
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
                // The chapter's title, kept when the field is left. An automatically numbered
                // chapter shows its number until it is given a name of its own.
                let key = ui.scope_id().with(("chapter-title", id));
                let stored = project
                    .chapters
                    .iter()
                    .find(|c| &c.id == id)
                    .map(|c| c.title.clone())
                    .unwrap_or_default();
                let mut title = ui
                    .data_mut(|d| d.get_temp::<String>(key))
                    .unwrap_or_else(|| label.clone());
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
                        edit.intents.push(Intent::SetChapterTitle {
                            chapter: id.clone(),
                            title,
                        });
                    }
                }
                ui.horizontal(|ui| {
                    if ui.button("Add scene").clicked() {
                        edit.intents.push(Intent::AddScene {
                            chapter: id.clone(),
                        });
                    }
                    if ui.button("Move chapter to Trash").clicked() {
                        edit.intents.push(Intent::TrashChapter(id.clone()));
                    }
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
    /// Anything else (none yet): nothing drawn, nothing to click.
    Other,
}

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
            if is_footnote(html) {
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

/// The font size each atom's (invisible) character is laid out in: a citation's is scaled so
/// the character is as wide as its label and padding, and wraps as one piece with it (0: the
/// usual size).
pub(crate) fn atom_sizes(
    fonts: &mut egui::epaint::FontsView<'_>,
    labels: &[AtomLabel],
) -> Vec<f32> {
    let glyph = fonts
        .layout_no_wrap(
            ATOM.to_string(),
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
                if glyph > 0.0 {
                    (ATOM_FONT + 2.0) * (width + 12.0) / glyph
                } else {
                    0.0
                }
            }
            AtomLabel::Footnote(_) | AtomLabel::Math(_) | AtomLabel::Other => 0.0,
        })
        .collect()
}

const ATOM_FONT: f32 = 12.0;

/// The text formats the editor lays the scene out with.
pub(crate) fn layout_doc(
    doc: &RichDoc,
    text: &str,
    palette: &Palette,
    wrap: f32,
    atom_sizes: &[f32],
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap;
    let mut line = 0;
    let mut atom = 0;
    for (i, c) in text.chars().enumerate() {
        if c == '\n' {
            line += 1;
        }
        let kind = doc.lines.get(line).copied().unwrap_or(LineKind::Paragraph);
        let marks = doc.marks.get(i).map_or(&[][..], Vec::as_slice);
        let (size, heading) = match kind {
            LineKind::Heading1 => (32.0, true),
            LineKind::Heading2 => (25.0, true),
            LineKind::Heading3 => (20.0, true),
            _ => (17.0, false),
        };
        let bold = heading || Mark::Bold.applies(marks);
        let italic = kind == LineKind::Quote || Mark::Italic.applies(marks);
        let font = match (bold, italic) {
            (_, true) => fonts::serif_italic(size),
            (true, false) => {
                egui::FontId::new(size, egui::FontFamily::Name(fonts::SERIF_SEMIBOLD.into()))
            }
            _ => fonts::serif(size),
        };
        let mut format = egui::TextFormat {
            font_id: font,
            line_height: Some(size * if heading { 1.3 } else { 1.85 }),
            ..Default::default()
        };
        format.color = match kind {
            LineKind::Quote => palette.quote,
            _ if heading => palette.text_heading,
            _ => palette.text_body,
        };
        if Mark::Underline.applies(marks) || marks.iter().any(|t| t.name == "a") {
            format.underline = egui::Stroke::new(1.0, format.color);
        }
        if Mark::Strike.applies(marks) {
            format.strikethrough = egui::Stroke::new(1.0, format.color);
        }
        if c == ATOM {
            // The character itself is invisible; its label is painted in the room reserved here.
            // The character's own box holds a short painted label.
            let size = atom_sizes
                .get(atom)
                .copied()
                .filter(|s| *s > 0.0)
                .unwrap_or(ATOM_FONT + 2.0);
            format.font_id = egui::FontId::proportional(size);
            format.color = egui::Color32::TRANSPARENT;
            atom += 1;
        }
        if c == '\n' {
            // ProseCraft's gap between paragraphs.
            format.line_height = Some(size * 1.85 + 10.0);
        }
        let mut buf = [0u8; 4];
        job.append(c.encode_utf8(&mut buf), 0.0, format);
    }
    job
}

/// The editor: the scene's title, ProseCraft's toolbar, and its formatted text.
fn editor(
    ui: &mut Ui,
    palette: &Palette,
    document: &prosecraft_core::project::Document,
    doc: RichDoc,
    screenplay: bool,
    edit: &mut Edit<'_>,
) {
    // A new draft when another document opens, or when the stored one changed underneath an
    // untouched draft (an import of a newer copy).
    let stale = edit.draft.as_ref().is_none_or(|d| {
        d.document != document.id
            || d.markdown.is_some()
            || (!d.is_dirty() && (d.title != document.title || d.doc.to_html() != document.content))
    });
    if stale {
        *edit.draft = Some(Draft {
            document: document.id.clone(),
            title: document.title.clone(),
            words: count_words(&doc.text.replace(ATOM, " ")),
            body: doc.text.clone(),
            doc,
            title_dirty: false,
            body_dirty: false,
            changed_at: edit.now,
            edits: 0,
            words_added: 0.0,
            markdown: None,
            markdown_base: String::new(),
        });
    }
    let now = edit.now;
    let Some(draft) = edit.draft.as_mut() else {
        return;
    };
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
    ui.add_space(10.0);

    let body_id = ui.make_persistent_id(("scene-body", &document.id));
    let selection = egui::TextEdit::load_state(ui.ctx(), body_id)
        .and_then(|s| s.cursor.char_range())
        .map(|r| {
            let (a, b) = (r.primary.index.0, r.secondary.index.0);
            a.min(b)..a.max(b)
        });
    let cursor = selection.clone().map_or(0, |r| r.start);
    let mut changed_format = false;
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
    // ProseCraft's formatting toolbar.
    card(palette).show(ui, |ui| {
        ui.horizontal(|ui| {
            let current = draft.doc.kind_at(cursor);
            let label = BLOCK_STYLES
                .iter()
                .find(|(k, _)| *k == current)
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
            // ProseCraft's comment button: a comment on the selected passage.
            let selected = selection.clone().filter(|r| !r.is_empty());
            let comment = ui
                .add_enabled(selected.is_some(), egui::Button::new("Comment"))
                .on_hover_text("Comment on the selected text");
            name_field(ui, &comment, "Comment");
            if comment.clicked()
                && let Some(range) = selected
            {
                use prosecraft_core::comments::{build_anchor, editor_text};
                let text = editor_text::comment_text(&draft.body);
                let (start, end) = (
                    editor_text::to_comment(&draft.body, range.start),
                    editor_text::to_comment(&draft.body, range.end),
                );
                // Whitespace alone has nothing to anchor to.
                if let Some(anchor) = build_anchor(&text, start, end) {
                    *edit.pending_comment = Some(crate::PendingComment {
                        document: document.id.clone(),
                        anchor,
                    });
                    *edit.side_tab = crate::SideTab::Comments;
                }
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
    let doc_for_layout = draft.doc.clone();
    let layout_labels = atom_labels(&draft.doc, &edit.cites);
    let layout_palette = *palette;
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
            let spacing = atom_sizes(f, &layout_labels);
            let job = layout_doc(
                &doc_for_layout,
                text.as_str(),
                &layout_palette,
                wrap_width,
                &spacing,
            );
            f.layout_job(job)
        })
    };
    let output = egui::TextEdit::multiline(&mut draft.body)
        .id(body_id)
        .frame(egui::Frame::NONE)
        .hint_text("Start writing…")
        .desired_width(f32::INFINITY)
        .desired_rows(12)
        .layouter(&mut layouter)
        // In a screenplay, Tab walks the element cycle instead of leaving the text.
        .lock_focus(screenplay)
        .margin(egui::Margin {
            left: 26,
            ..Default::default()
        })
        .show(ui);
    name_field(ui, &output.response, "Scene text");
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
        let words = count_words(&draft.body.replace(ATOM, " "));
        draft.edits += 1;
        draft.words_added += (words as f64 - draft.words as f64).max(0.0);
        draft.words = words;
    }
    let labels = atom_labels(&draft.doc, &edit.cites);
    let markers = paint_extras(ui, palette, &draft.doc, &labels, &output);
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
    let count = draft.words;
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
    edit: &mut Edit<'_>,
) {
    // A new draft when another document opens, or when the scene changed underneath an
    // untouched draft.
    let stale = edit.draft.as_ref().is_none_or(|d| {
        d.document != document.id
            || d.markdown.is_none()
            || (!d.is_dirty()
                && (d.title != document.title || d.markdown_base != markdown_base(document)))
    });
    if stale {
        let source = document.markdown();
        let doc = RichDoc::from_html(&document.content).unwrap_or_default();
        *edit.draft = Some(Draft {
            document: document.id.clone(),
            title: document.title.clone(),
            words: markdown_words(&source),
            body: doc.text.clone(),
            doc,
            title_dirty: false,
            body_dirty: false,
            changed_at: edit.now,
            edits: 0,
            words_added: 0.0,
            markdown: Some(source),
            markdown_base: markdown_base(document),
        });
    }
    let now = edit.now;
    let Some(draft) = edit.draft.as_mut() else {
        return;
    };
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
        .font(egui::FontId::monospace(15.0))
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
}

/// What the text itself can't show: list bullets and numbers, the quote bar, footnote
/// markers as their numbers, and citations as their labels. Painted over the laid-out text, so the caret, selection and the
/// accessibility tree still see the real text.
fn paint_extras(
    ui: &Ui,
    palette: &Palette,
    doc: &RichDoc,
    labels: &[AtomLabel],
    output: &egui::text_edit::TextEditOutput,
) -> Vec<(egui::Rect, Clicked)> {
    let mut markers = Vec::new();
    let painter = ui.painter_at(output.text_clip_rect.expand2(egui::vec2(30.0, 0.0)));
    let galley = &output.galley;
    let origin = output.galley_pos;
    let mut number = 0;
    let mut previous = None;
    for (line, (start, _end)) in doc.line_ranges().into_iter().enumerate() {
        let kind = doc.lines.get(line).copied().unwrap_or(LineKind::Paragraph);
        let rect = galley
            .pos_from_cursor(egui::text::CCursor::new(start))
            .translate(origin.to_vec2());
        number = if kind == LineKind::Numbered {
            if previous == Some(LineKind::Numbered) {
                number + 1
            } else {
                1
            }
        } else {
            0
        };
        let marker = match kind {
            LineKind::Bullet => Some("•".to_owned()),
            LineKind::Numbered => Some(format!("{number}.")),
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
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.min.x - 14.0, rect.min.y),
                egui::pos2(rect.min.x - 12.0, rect.max.y),
            );
            painter.rect_filled(bar, 1.0, palette.border_control);
        }
        previous = Some(kind);
    }
    markers.extend(paint_atoms(&painter, palette, doc, labels, output));
    markers
}

/// Each atom's label, painted in its character's box (footnote numbers, citation labels,
/// equations), and the markers that can be clicked.
pub(crate) fn paint_atoms(
    painter: &egui::Painter,
    palette: &Palette,
    doc: &RichDoc,
    labels: &[AtomLabel],
    output: &egui::text_edit::TextEditOutput,
) -> Vec<(egui::Rect, Clicked)> {
    let mut markers = Vec::new();
    let galley = &output.galley;
    let origin = output.galley_pos;
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
            AtomLabel::Math(_) => {
                let chip = egui::Rect::from_center_size(
                    center,
                    egui::vec2((right - before.min.x).max(16.0), ATOM_FONT + 4.0),
                );
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
fn timeline(ui: &mut Ui, palette: &Palette, project: &Project, edit: &mut Edit<'_>) {
    use prosecraft_core::timeline::Field;
    ui.add_space(28.0);
    let count = project.timeline.len();
    let mut add = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            eyebrow(ui, palette, "Follow your story");
            ui.label(RichText::new("Timeline").font(fonts::serif(32.0)).color(palette.text_heading));
            ui.label(RichText::new("Map the moments that move your story forward. Use the arrows to change their order.").color(palette.text_muted));
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
    for (index, node) in project.timeline.iter().enumerate() {
        let label = if node.title.trim().is_empty() {
            format!("timeline moment {}", index + 1)
        } else {
            node.title.trim().to_owned()
        };
        card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
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
        ui.add_space(8.0);
    }
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
    use super::grouped;

    #[test]
    fn groups_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(80_000), "80,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}
