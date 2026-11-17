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
        ..
    } = state;
    let entry = library.as_ref().and_then(|l| l.projects().get(*active));
    let Some(entry) = entry else {
        column(ui, |ui| welcome(ui, &palette));
        return;
    };
    let project = &entry.view;
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
    };
    let mut chosen = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            column(ui, |ui| match view {
                WorkspaceView::Outline => chosen = outline(ui, &palette, project, &mut edit),
                WorkspaceView::Write => write(ui, &palette, project, selection.as_ref(), &mut edit),
                WorkspaceView::Storyboard => {
                    let text = format!(
                        "The storyboard ({} cards) isn't ported yet.",
                        project.storyboard_cards
                    );
                    not_ported(ui, &palette, "Storyboard", &text);
                }
                WorkspaceView::Timeline => timeline(ui, &palette, project),
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
            "Open a ProseCraft backup to see your projects: drop a .prosecraft file on this window, or start prosecraft-rs with the file's path.",
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

fn not_ported(ui: &mut Ui, palette: &Palette, title: &str, text: &str) {
    ui.add_space(40.0);
    eyebrow(ui, palette, title);
    ui.label(
        RichText::new(text)
            .font(fonts::serif(22.0))
            .color(palette.text_heading),
    );
    ui.add_space(6.0);
    ui.label(
        RichText::new("It is on the roadmap; your data for it is kept as it is.")
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
                match RichDoc::from_html(&document.content).filter(|_| edit.editable) {
                    Some(doc) => editor(ui, palette, document, doc, edit),
                    None => {
                        ui.label(
                            RichText::new(document.title.as_str())
                                .font(fonts::serif(38.0))
                                .color(palette.text_heading),
                        );
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

/// What an atom shows: a footnote its number, a citation its own text, an equation its TeX.
fn atom_labels(doc: &RichDoc) -> Vec<(String, bool)> {
    let mut footnote = 0;
    doc.atoms
        .iter()
        .map(|html| {
            if html.contains("footnote-ref") {
                footnote += 1;
                (footnote.to_string(), true)
            } else if html.contains("math-inline") {
                let nodes = prosecraft_core::html::parse_fragment(html);
                let tex = nodes.first().and_then(|n| match n {
                    prosecraft_core::html::Node::Element(e) => {
                        e.attr("data-tex").map(str::to_owned)
                    }
                    prosecraft_core::html::Node::Text(_) => None,
                });
                (format!("${}$", tex.unwrap_or_default()), false)
            } else {
                let text = prosecraft_core::html::html_to_plain_text(html, &Default::default());
                (
                    if text.is_empty() {
                        "citation".to_owned()
                    } else {
                        text
                    },
                    false,
                )
            }
        })
        .collect()
}

const ATOM_FONT: f32 = 12.0;

/// The text formats the editor lays the scene out with.
fn layout_doc(doc: &RichDoc, text: &str, palette: &Palette, wrap: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap;
    let mut line = 0;
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
            format.font_id = egui::FontId::proportional(ATOM_FONT + 2.0);
            format.color = egui::Color32::TRANSPARENT;
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
    edit: &mut Edit<'_>,
) {
    // A new draft when another document opens, or when the stored one changed underneath an
    // untouched draft (an import of a newer copy).
    let stale = edit.draft.as_ref().is_none_or(|d| {
        d.document != document.id
            || (!d.is_dirty() && (d.title != document.title || d.doc.to_html() != document.content))
    });
    if stale {
        *edit.draft = Some(Draft {
            document: document.id.clone(),
            title: document.title.clone(),
            body: doc.text.clone(),
            doc,
            title_dirty: false,
            body_dirty: false,
            changed_at: edit.now,
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
        });
    });
    if changed_format {
        draft.body_dirty = true;
        draft.changed_at = now;
        ui.memory_mut(|m| m.request_focus(body_id));
    }
    ui.add_space(14.0);

    let doc_for_layout = draft.doc.clone();
    let layout_palette = *palette;
    let mut layouter = move |ui: &Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        let job = layout_doc(&doc_for_layout, text.as_str(), &layout_palette, wrap_width);
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let output = egui::TextEdit::multiline(&mut draft.body)
        .id(body_id)
        .frame(egui::Frame::NONE)
        .hint_text("Start writing…")
        .desired_width(f32::INFINITY)
        .desired_rows(12)
        .layouter(&mut layouter)
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
        draft.body_dirty = true;
        draft.changed_at = now;
    }
    paint_extras(ui, palette, &draft.doc, &output);
    let count = count_words(&draft.body.replace(ATOM, " "));
    footer(ui, palette, count, &save_text(edit.save_state));
}

/// What the text itself can't show: list bullets and numbers, the quote bar, and footnote
/// markers as their numbers. Painted over the laid-out text, so the caret, selection and the
/// accessibility tree still see the real text.
fn paint_extras(
    ui: &Ui,
    palette: &Palette,
    doc: &RichDoc,
    output: &egui::text_edit::TextEditOutput,
) {
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
    // Each atom's label, painted in its character's box.
    let atoms = doc
        .text
        .chars()
        .enumerate()
        .filter(|(_, c)| *c == ATOM)
        .map(|(i, _)| i);
    for (index, (label, footnote)) in atoms.zip(atom_labels(doc)) {
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
        if footnote {
            // A superscript number, as ProseCraft shows footnote references.
            let at = egui::pos2(center.x, before.min.y + before.height() * 0.3);
            painter.text(
                at,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(ATOM_FONT - 2.0),
                palette.link,
            );
        } else {
            let short = if label.starts_with('$') { "ƒ" } else { "cite" };
            let chip = egui::Rect::from_center_size(
                center,
                egui::vec2((right - before.min.x).max(16.0), ATOM_FONT + 4.0),
            );
            painter.rect_filled(chip, 4.0, palette.selected_bg);
            painter.text(
                chip.center(),
                egui::Align2::CENTER_CENTER,
                short,
                egui::FontId::proportional(ATOM_FONT - 2.0),
                palette.selected_text,
            );
        }
    }
}

fn timeline(ui: &mut Ui, palette: &Palette, project: &Project) {
    ui.add_space(28.0);
    eyebrow(ui, palette, "Timeline");
    ui.label(
        RichText::new("Moments")
            .font(fonts::serif(32.0))
            .color(palette.text_heading),
    );
    ui.add_space(14.0);
    if project.timeline.is_empty() {
        ui.label(RichText::new("No moments yet.").color(palette.text_muted));
    }
    for node in &project.timeline {
        card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let when = [node.date.as_str(), node.time.as_str()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            if !when.is_empty() {
                ui.label(RichText::new(when).size(12.0).color(palette.text_muted));
            }
            ui.label(
                RichText::new(node.title.as_str())
                    .font(fonts::serif(18.0))
                    .color(palette.text_heading),
            );
            if !node.description.is_empty() {
                ui.label(RichText::new(node.description.as_str()).color(palette.text_body));
            }
        });
        ui.add_space(10.0);
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
