//! The right sidebar: Progress, Comments, Snapshots and Footnotes (ProseCraft's right sidebar).

use egui::{Align2, CornerRadius, RichText, Sense, Ui, vec2};
use prosecraft_core::html::{PlainTextOptions, html_to_plain_text};
use prosecraft_core::project::Project;

use crate::theme::Palette;
use crate::views::grouped;
use crate::widgets::{card, eyebrow, goal_bar, segmented};
use crate::{AppState, Selection, SideTab, fonts};

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    ui.add_space(6.0);
    let labels: Vec<&str> = SideTab::ALL.iter().map(|t| t.label()).collect();
    let current = SideTab::ALL
        .iter()
        .position(|t| *t == state.side_tab)
        .unwrap_or(0);
    if let Some(index) = segmented(ui, &palette, "side-tab", &labels, current, 12.0) {
        state.side_tab = SideTab::ALL.get(index).copied().unwrap_or_default();
    }
    ui.add_space(14.0);
    let Some(project) = state
        .library
        .as_ref()
        .and_then(|l| l.projects().get(state.active))
        .map(|p| &p.view)
    else {
        ui.label(RichText::new("Nothing open.").color(palette.text_muted));
        return;
    };
    // Comments, snapshots and footnotes belong to the open scene or page.
    let document = match &state.selection {
        Some(Selection::Document(id)) => Some(id.as_str()),
        _ => None,
    };
    // Snapshots: the open scene, or every scene of the open chapter (chapter mode).
    let snapshot_scope: Option<(Vec<String>, bool)> = match &state.selection {
        Some(Selection::Document(id)) => Some((vec![id.clone()], false)),
        Some(Selection::Chapter(id)) => Some((
            project
                .scenes_in_chapter(id)
                .map(|d| d.id.clone())
                .collect(),
            true,
        )),
        None => None,
    };
    let open_draft = state
        .draft
        .as_ref()
        .map(|d| (d.document.clone(), d.body.clone()));
    let (editable, now_ms) = (state.editable, state.now_ms);
    let mut open = None;
    let month = &mut state.activity_month;
    egui::ScrollArea::vertical()
        .id_salt("sidebar-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| match state.side_tab {
            SideTab::Progress => progress(ui, &palette, project, month),
            SideTab::Comments => {
                let mut edit = CommentEdit {
                    pending: &mut state.pending_comment,
                    draft: &mut state.comment_draft,
                    author: &mut state.comment_author,
                    show_all: &mut state.show_all_comments,
                    show_resolved: &mut state.show_resolved_comments,
                    goto: &mut state.comment_goto,
                    open: None,
                    open_draft,
                    intents: &mut state.intents,
                    editable,
                    now_ms,
                };
                comments(ui, &palette, project, snapshot_scope, &mut edit);
                open = edit.open;
            }
            SideTab::Snapshots => {
                let mut edit = SnapshotEdit {
                    note: &mut state.snapshot_note,
                    preview: &mut state.snapshot_preview,
                    intents: &mut state.intents,
                    editable,
                    now_ms,
                };
                snapshots(ui, &palette, project, snapshot_scope, &mut edit);
            }
            SideTab::Footnotes => footnotes(ui, &palette, project, document),
        });
    if let Some(document) = open {
        state.select(Selection::Document(document));
    }
}

fn heading(ui: &mut Ui, palette: &Palette, eyebrow_text: &str, title: &str) {
    eyebrow(ui, palette, eyebrow_text);
    ui.label(
        RichText::new(title)
            .font(fonts::serif(24.0))
            .color(palette.text_heading),
    );
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(10.0);
}

fn section(ui: &mut Ui) {
    ui.add_space(14.0);
    ui.separator();
    ui.add_space(10.0);
}

fn progress(ui: &mut Ui, palette: &Palette, project: &Project, month: &mut Option<(i32, u32)>) {
    heading(ui, palette, "Your writing", "Progress");
    let total = project.word_count();
    ui.label(
        RichText::new("Project goal")
            .font(fonts::semibold(13.0))
            .color(palette.text_primary),
    );
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(grouped(total))
                .font(fonts::serif(26.0))
                .color(palette.text_heading),
        );
        if project.project_goal > 0.0 {
            ui.label(
                RichText::new(format!(
                    "of {} words",
                    grouped(project.project_goal as usize)
                ))
                .size(12.0)
                .color(palette.text_muted),
            );
        }
    });
    if project.project_goal > 0.0 {
        let fraction = (total as f64 / project.project_goal) as f32;
        goal_bar(ui, palette, fraction);
        let percent = (fraction * 100.0).clamp(0.0, 100.0);
        ui.label(
            RichText::new(format!("{percent:.0}% of project goal"))
                .size(11.5)
                .color(palette.text_muted),
        );
    }
    section(ui);
    ui.label(
        RichText::new("Chapter goals")
            .font(fonts::semibold(13.0))
            .color(palette.text_primary),
    );
    ui.add_space(4.0);
    for chapter in project.live_chapters() {
        let written = project.chapter_word_count(&chapter.id);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(chapter.title.as_str())
                    .size(12.5)
                    .color(palette.text_body),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let text = match project.chapter_goal(&chapter.id).filter(|g| *g > 0.0) {
                    Some(goal) => format!("{} / {}", grouped(written), grouped(goal as usize)),
                    None => grouped(written),
                };
                ui.label(RichText::new(text).size(12.0).color(palette.text_muted));
            });
        });
    }
    section(ui);
    activity(ui, palette, project, month);
}

/// ProseCraft's writing-activity calendar: one month, a cell per day, darker for more edits.
fn activity(ui: &mut Ui, palette: &Palette, project: &Project, month: &mut Option<(i32, u32)>) {
    use chrono::{Datelike, NaiveDate};
    let days: Vec<(NaiveDate, f64)> = project
        .activity
        .iter()
        .filter_map(|a| {
            NaiveDate::parse_from_str(&a.date, "%Y-%m-%d")
                .ok()
                .map(|d| (d, a.edits))
        })
        .collect();
    let active_days = days.iter().filter(|(_, e)| *e > 0.0).count();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Writing activity")
                .font(fonts::semibold(13.0))
                .color(palette.text_primary),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let noun = if active_days == 1 { "day" } else { "days" };
            ui.label(
                RichText::new(format!("{active_days} active {noun}"))
                    .size(12.0)
                    .color(palette.text_muted),
            );
        });
    });
    // The month shown: chosen with the arrows, else the latest month with activity.
    let latest = days.iter().map(|(d, _)| *d).max();
    let (year, number) = month
        .or_else(|| latest.map(|d| (d.year(), d.month())))
        .unwrap_or((2026, 1));
    let Some(first) = NaiveDate::from_ymd_opt(year, number, 1) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(first.format("%B %Y").to_string())
                .size(12.5)
                .color(palette.text_body),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("›").on_hover_text("Next month").clicked() {
                let next = first
                    .checked_add_months(chrono::Months::new(1))
                    .unwrap_or(first);
                *month = Some((next.year(), next.month()));
            }
            if ui
                .small_button("‹")
                .on_hover_text("Previous month")
                .clicked()
            {
                let previous = first
                    .checked_sub_months(chrono::Months::new(1))
                    .unwrap_or(first);
                *month = Some((previous.year(), previous.month()));
            }
        });
    });
    let max = days
        .iter()
        .map(|(_, e)| *e)
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let cell = ((ui.available_width() - 6.0 * 4.0) / 7.0).clamp(14.0, 30.0);
    let offset = first.weekday().num_days_from_sunday() as usize;
    let in_month = (1..=31)
        .filter_map(|d| NaiveDate::from_ymd_opt(year, number, d))
        .count();
    let rows = (offset + in_month).div_ceil(7);
    let size = vec2(cell * 7.0 + 24.0, rows as f32 * (cell + 4.0) + 18.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let summary = format!(
        "Writing activity, {}: {active_days} active days",
        first.format("%B %Y")
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Group, true, summary.clone()));
    let painter = ui.painter_at(rect);
    for (i, day) in ["S", "M", "T", "W", "T", "F", "S"].iter().enumerate() {
        let x = rect.min.x + i as f32 * (cell + 4.0) + cell / 2.0;
        painter.text(
            egui::pos2(x, rect.min.y + 6.0),
            Align2::CENTER_CENTER,
            *day,
            egui::FontId::proportional(10.0),
            palette.text_muted,
        );
    }
    for index in 0..in_month {
        let Some(date) = NaiveDate::from_ymd_opt(year, number, index as u32 + 1) else {
            continue;
        };
        let slot = offset + index;
        let (row, col) = (slot / 7, slot % 7);
        let min = egui::pos2(
            rect.min.x + col as f32 * (cell + 4.0),
            rect.min.y + 16.0 + row as f32 * (cell + 4.0),
        );
        let cell_rect = egui::Rect::from_min_size(min, vec2(cell, cell));
        let edits = days
            .iter()
            .filter(|(d, _)| *d == date)
            .map(|(_, e)| *e)
            .sum::<f64>();
        let level = if edits <= 0.0 {
            0
        } else {
            (1.0 + (edits / max) * 2.99) as usize
        };
        let fill = palette
            .heat
            .get(level.min(3))
            .copied()
            .unwrap_or(palette.control_bg);
        painter.rect_filled(cell_rect, CornerRadius::same(4), fill);
        let text_color = if level >= 2 {
            palette.page_bg
        } else {
            palette.text_muted
        };
        painter.text(
            cell_rect.center(),
            Align2::CENTER_CENTER,
            (index + 1).to_string(),
            egui::FontId::proportional(9.5),
            text_color,
        );
    }
}

fn pick_document(ui: &mut Ui, palette: &Palette) {
    ui.label(
        RichText::new("Open a scene or Wiki page to see its items.").color(palette.text_muted),
    );
}

/// What the Comments tab changes.
struct CommentEdit<'a> {
    pending: &'a mut Option<crate::PendingComment>,
    draft: &'a mut String,
    author: &'a mut String,
    show_all: &'a mut bool,
    show_resolved: &'a mut bool,
    goto: &'a mut Option<(String, usize, usize)>,
    /// A scene to open for "Go to".
    open: Option<String>,
    /// The open editor's document and text (newer than the stored text while typing).
    open_draft: Option<(String, String)>,
    intents: &'a mut Vec<crate::editing::Intent>,
    editable: bool,
    now_ms: f64,
}

fn shorten(text: &str, max: usize) -> String {
    // `quote.slice(0, max)…`, in UTF-16 units.
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.len() > max {
        format!("{}…", String::from_utf16_lossy(&units[..max]))
    } else {
        text.to_owned()
    }
}

/// ProseCraft's Comments tab: a new comment on the passage chosen in the editor, then the
/// comments of the open scene (or chapter), in text order.
fn comments(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    scope: Option<(Vec<String>, bool)>,
    edit: &mut CommentEdit<'_>,
) {
    use prosecraft_core::comments::{Anchor, comment_text, editor_text, locate_anchor};
    heading(ui, palette, "Feedback", "Comments");
    ui.label(
        RichText::new("New comment")
            .strong()
            .color(palette.text_heading),
    );
    ui.add(
        egui::Label::new(
            RichText::new(
                "Notes for yourself or a proofreader. Never part of the text or any export.",
            )
            .size(12.0)
            .color(palette.text_muted),
        )
        .wrap(),
    );
    ui.add_space(6.0);
    match edit.pending.clone() {
        Some(pending) => {
            let quote = String::from_utf16_lossy(&pending.anchor.quote);
            ui.label(
                RichText::new(shorten(&quote, 160))
                    .font(fonts::serif_italic(14.0))
                    .color(palette.quote),
            );
            let body = ui.add(
                egui::TextEdit::multiline(edit.draft)
                    .hint_text("Your comment")
                    .char_limit(4000)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            crate::views::name_field(ui, &body, "Comment text");
            let mut add = false;
            ui.horizontal(|ui| {
                let name = ui.add(
                    egui::TextEdit::singleline(edit.author)
                        .hint_text("Name")
                        .char_limit(60)
                        .desired_width(140.0),
                );
                crate::views::name_field(ui, &name, "Author name");
                let ready = edit.editable && !edit.draft.trim().is_empty();
                add = ui
                    .add_enabled(ready, egui::Button::new("Add comment"))
                    .clicked();
            });
            if add {
                let author = if edit.author.trim().is_empty() {
                    "Me".to_owned()
                } else {
                    edit.author.trim().to_owned()
                };
                edit.intents.push(crate::editing::Intent::AddComment {
                    document: pending.document,
                    anchor: pending.anchor,
                    body: std::mem::take(edit.draft),
                    author,
                    now: edit.now_ms,
                });
                *edit.pending = None;
            }
        }
        None => {
            ui.add(egui::Label::new(RichText::new("Select some text in the scene, then use the comment button in the toolbar.").size(12.0).color(palette.text_muted)).wrap());
        }
    }
    ui.add_space(12.0);

    let in_scope: Vec<String> = scope
        .as_ref()
        .map(|(docs, _)| docs.clone())
        .unwrap_or_default();
    // Positions in the scope's comment text (scenes one after another), or None when the
    // passage isn't there any more.
    let mut positions: std::collections::HashMap<&str, Option<usize>> =
        std::collections::HashMap::new();
    let mut offset = 0;
    for id in &in_scope {
        let Some(doc) = project.document(id) else {
            continue;
        };
        let text = comment_text(&doc.content);
        for c in project.comments.iter().filter(|c| c.document_id == *id) {
            let anchor = Anchor {
                quote: c.quote.encode_utf16().collect(),
                prefix: c.prefix.encode_utf16().collect(),
                suffix: c.suffix.encode_utf16().collect(),
            };
            positions.insert(
                c.id.as_str(),
                locate_anchor(&text, &anchor).map(|(s, _)| offset + s),
            );
        }
        offset += text.len();
    }
    let live = |c: &&prosecraft_core::project::Comment| {
        project.document(&c.document_id).is_some_and(|d| !d.trashed)
    };
    let pool: Vec<&prosecraft_core::project::Comment> = if *edit.show_all {
        project.comments.iter().filter(live).collect()
    } else {
        project
            .comments
            .iter()
            .filter(|c| in_scope.contains(&c.document_id))
            .collect()
    };
    let resolved = project
        .comments
        .iter()
        .filter(|c| in_scope.contains(&c.document_id) && c.resolved)
        .count();
    let mut list: Vec<_> = pool
        .into_iter()
        .filter(|c| *edit.show_resolved || !c.resolved)
        .collect();
    let pos = |c: &prosecraft_core::project::Comment| {
        positions
            .get(c.id.as_str())
            .copied()
            .flatten()
            .map_or(f64::INFINITY, |p| p as f64)
    };
    if *edit.show_all {
        let chapters: Vec<&str> = project.live_chapters().map(|c| c.id.as_str()).collect();
        let chapter_of = |c: &prosecraft_core::project::Comment| {
            project
                .document(&c.document_id)
                .and_then(|d| d.chapter_id.as_deref())
                .and_then(|ch| chapters.iter().position(|x| *x == ch))
                .unwrap_or(chapters.len())
        };
        let doc_order = |c: &prosecraft_core::project::Comment| {
            project
                .documents
                .iter()
                .position(|d| d.id == c.document_id)
                .unwrap_or(0)
        };
        list.sort_by(|a, b| {
            chapter_of(a)
                .cmp(&chapter_of(b))
                .then(doc_order(a).cmp(&doc_order(b)))
                .then(pos(a).total_cmp(&pos(b)))
                .then(a.created_at.total_cmp(&b.created_at))
        });
    } else {
        list.sort_by(|a, b| {
            pos(a)
                .total_cmp(&pos(b))
                .then(a.created_at.total_cmp(&b.created_at))
        });
    }
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(if *edit.show_all {
                "All comments"
            } else {
                "This chapter"
            })
            .strong()
            .color(palette.text_heading),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(list.len().to_string()).color(palette.text_muted))
        });
    });
    ui.checkbox(edit.show_all, "Show all project comments");
    if resolved > 0 {
        ui.checkbox(edit.show_resolved, format!("Show resolved ({resolved})"));
    }
    ui.add_space(6.0);
    if list.is_empty() {
        ui.label(
            RichText::new("No comments yet. Select text in the editor, then add one above.")
                .color(palette.text_muted),
        );
    }
    for comment in list {
        card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if *edit.show_all
                && let Some(doc) = project.document(&comment.document_id)
            {
                let scene = if doc.title.is_empty() {
                    "Untitled scene".to_owned()
                } else {
                    doc.title.clone()
                };
                let location = match (
                    &doc.space,
                    doc.chapter_id
                        .as_deref()
                        .and_then(|c| project.chapters.iter().find(|x| x.id == c)),
                ) {
                    (Some(space), _) if !space.is_empty() => space.clone(),
                    (_, Some(chapter)) => format!("{} · {scene}", chapter.title),
                    _ => scene,
                };
                ui.label(RichText::new(location).size(11.5).color(palette.eyebrow));
            }
            let quote_color = if comment.resolved {
                palette.text_muted
            } else {
                palette.quote
            };
            ui.label(
                RichText::new(shorten(&comment.quote, 120))
                    .font(fonts::serif_italic(14.0))
                    .color(quote_color),
            );
            ui.add(
                egui::Label::new(RichText::new(comment.body.as_str()).color(palette.text_body))
                    .wrap(),
            );
            let date = chrono::DateTime::from_timestamp_millis(comment.created_at as i64)
                .map(|t| t.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            let missing = matches!(positions.get(comment.id.as_str()), Some(None));
            let meta = format!(
                "{} · {date}{}",
                comment.author,
                if missing {
                    " · text not found any more"
                } else {
                    ""
                }
            );
            ui.label(RichText::new(meta).size(11.5).color(palette.text_muted));
            ui.horizontal(|ui| {
                let short = shorten(&comment.quote, 40);
                // Named by the comment and its passage, so each button is told apart.
                let which = format!("“{}” on “{short}”", shorten(&comment.body, 40));
                let go = ui.add_enabled(!missing, egui::Button::new("Go to"));
                crate::views::name_field(ui, &go, &format!("Go to comment {which}"));
                if go.clicked() {
                    // Find the passage in the scene's editor text, then select it there.
                    let content = project
                        .document(&comment.document_id)
                        .map(|d| d.content.clone())
                        .unwrap_or_default();
                    let body = match &edit.open_draft {
                        Some((doc, body)) if *doc == comment.document_id => body.clone(),
                        _ => prosecraft_core::richtext::RichDoc::from_html(&content)
                            .map(|d| d.text)
                            .unwrap_or_default(),
                    };
                    let anchor = Anchor {
                        quote: comment.quote.encode_utf16().collect(),
                        prefix: comment.prefix.encode_utf16().collect(),
                        suffix: comment.suffix.encode_utf16().collect(),
                    };
                    if let Some((s, e)) = locate_anchor(&editor_text::comment_text(&body), &anchor)
                    {
                        *edit.goto = Some((
                            comment.document_id.clone(),
                            editor_text::to_editor(&body, s),
                            editor_text::to_editor(&body, e),
                        ));
                        edit.open = Some(comment.document_id.clone());
                    }
                }
                ui.add_enabled_ui(edit.editable, |ui| {
                    let label = if comment.resolved {
                        "Reopen"
                    } else {
                        "Resolve"
                    };
                    let toggle = ui.button(label);
                    crate::views::name_field(ui, &toggle, &format!("{label} comment {which}"));
                    if toggle.clicked() {
                        edit.intents.push(crate::editing::Intent::ResolveComment {
                            id: comment.id.clone(),
                            resolved: !comment.resolved,
                        });
                    }
                    let delete = ui.button(RichText::new("Delete").color(palette.danger));
                    crate::views::name_field(ui, &delete, &format!("Delete comment {which}"));
                    if delete.clicked() {
                        edit.intents
                            .push(crate::editing::Intent::DeleteComment(comment.id.clone()));
                    }
                });
            });
        });
        ui.add_space(8.0);
    }
}

/// What the Snapshots tab changes.
struct SnapshotEdit<'a> {
    note: &'a mut String,
    preview: &'a mut Option<String>,
    intents: &'a mut Vec<crate::editing::Intent>,
    editable: bool,
    now_ms: f64,
}

/// `snapshotLabel`: when it was taken, and whether before a restore.
fn snapshot_label(created_at: f64, before_restore: bool) -> String {
    let when = chrono::DateTime::from_timestamp_millis(created_at as i64)
        .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_default();
    if before_restore {
        format!("{when} (before a restore)")
    } else {
        when
    }
}

/// ProseCraft's Snapshots tab: "Save this version" (with an optional note), then "Earlier
/// versions" with Preview, Restore and Delete. In chapter mode it covers the chapter's scenes.
fn snapshots(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    scope: Option<(Vec<String>, bool)>,
    edit: &mut SnapshotEdit<'_>,
) {
    heading(ui, palette, "Versions", "Snapshots");
    let Some((documents, chapter_mode)) = scope else {
        return pick_document(ui, palette);
    };
    let title = match (
        chapter_mode,
        documents.first().and_then(|id| project.document(id)),
    ) {
        (false, Some(doc)) if !doc.title.is_empty() => doc.title.clone(),
        (false, _) => "Untitled".to_owned(),
        (true, _) => "this chapter".to_owned(),
    };
    ui.label(
        RichText::new("Save this version")
            .strong()
            .color(palette.text_heading),
    );
    let what = if chapter_mode {
        "the scenes of this chapter".to_owned()
    } else {
        format!("“{title}”")
    };
    ui.add(
        egui::Label::new(
            RichText::new(format!(
                "Keeps a copy of {what} as it is now. Up to ten are kept per scene."
            ))
            .size(12.0)
            .color(palette.text_muted),
        )
        .wrap(),
    );
    ui.add_space(6.0);
    let mut take = false;
    ui.add_enabled_ui(edit.editable && !documents.is_empty(), |ui| {
        let field = ui.add(
            egui::TextEdit::singleline(edit.note)
                .hint_text("Optional note")
                .char_limit(120)
                .desired_width(f32::INFINITY),
        );
        crate::views::name_field(ui, &field, "Snapshot note");
        take |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        take |= ui.button("Take snapshot").clicked();
    });
    if take && edit.editable && !documents.is_empty() {
        edit.intents.push(crate::editing::Intent::TakeSnapshots {
            documents: documents.clone(),
            note: std::mem::take(edit.note),
            now: edit.now_ms,
        });
    }
    ui.add_space(12.0);
    let mut list: Vec<_> = project
        .snapshots
        .iter()
        .filter(|s| documents.contains(&s.document_id))
        .collect();
    // Newest first, as ProseCraft lists them.
    list.sort_by(|a, b| b.created_at.total_cmp(&a.created_at));
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Earlier versions")
                .strong()
                .color(palette.text_heading),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(list.len().to_string()).color(palette.text_muted))
        });
    });
    ui.add(
        egui::Label::new(
            RichText::new("Restoring first saves the current text, so you can go back.")
                .size(12.0)
                .color(palette.text_muted),
        )
        .wrap(),
    );
    ui.add_space(6.0);
    if list.is_empty() {
        ui.label(
            RichText::new("No snapshots yet. Save one above to freeze this moment.")
                .color(palette.text_muted),
        );
    }
    for snapshot in list {
        card(palette).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let label = snapshot_label(snapshot.created_at, snapshot.before_restore);
            ui.label(RichText::new(&label).strong().color(palette.text_primary));
            let text = html_to_plain_text(&snapshot.content, &PlainTextOptions::default());
            let mut meta = Vec::new();
            if chapter_mode {
                let scene = project
                    .document(&snapshot.document_id)
                    .map(|d| d.title.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "Untitled scene".into());
                meta.push(scene);
            }
            meta.push(format!(
                "{} words",
                prosecraft_core::html::count_words(&text)
            ));
            if let Some(note) = &snapshot.note {
                meta.push(note.clone());
            }
            ui.label(
                RichText::new(meta.join(" · "))
                    .size(12.0)
                    .color(palette.text_muted),
            );
            ui.horizontal(|ui| {
                let previewing = edit.preview.as_deref() == Some(snapshot.id.as_str());
                let toggle = ui.button(if previewing { "Hide" } else { "Preview" });
                crate::views::name_field(
                    ui,
                    &toggle,
                    &format!(
                        "{} snapshot from {label}",
                        if previewing { "Hide" } else { "Preview" }
                    ),
                );
                if toggle.clicked() {
                    *edit.preview = if previewing {
                        None
                    } else {
                        Some(snapshot.id.clone())
                    };
                }
                ui.add_enabled_ui(edit.editable, |ui| {
                    let restore = ui.button("Restore");
                    crate::views::name_field(
                        ui,
                        &restore,
                        &format!("Restore snapshot from {label}"),
                    );
                    if restore.clicked() {
                        edit.intents.push(crate::editing::Intent::RestoreSnapshot {
                            id: snapshot.id.clone(),
                            now: edit.now_ms,
                        });
                        *edit.preview = None;
                    }
                    let delete = ui.button(RichText::new("Delete").color(palette.danger));
                    crate::views::name_field(ui, &delete, &format!("Delete snapshot from {label}"));
                    if delete.clicked() {
                        edit.intents
                            .push(crate::editing::Intent::DeleteSnapshot(snapshot.id.clone()));
                    }
                });
            });
            if edit.preview.as_deref() == Some(snapshot.id.as_str()) {
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(&text)
                            .font(fonts::serif(14.0))
                            .color(palette.text_body),
                    )
                    .wrap(),
                );
            }
        });
        ui.add_space(8.0);
    }
}

fn footnotes(ui: &mut Ui, palette: &Palette, project: &Project, document: Option<&str>) {
    heading(ui, palette, "This scene", "Footnotes");
    let Some(document) = document.and_then(|id| project.document(id)) else {
        return pick_document(ui, palette);
    };
    if document.footnotes.is_empty() {
        ui.label(RichText::new("No footnotes in this scene.").color(palette.text_muted));
    }
    for (index, footnote) in document.footnotes.iter().enumerate() {
        let text = html_to_plain_text(&footnote.html, &PlainTextOptions::default());
        ui.label(RichText::new(format!("{}. {}", index + 1, text)).color(palette.text_body));
    }
}
