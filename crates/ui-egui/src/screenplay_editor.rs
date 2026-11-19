//! Writing a screenplay scene (ProseCraft's screenplay editor): each paragraph is an element
//! (scene heading, action, character, dialogue, parenthetical, transition) set in Courier with
//! its column; Enter starts what usually follows, Tab and Shift+Tab walk the element cycle,
//! typed headings and transitions are recognized, and headings, cues and transitions are
//! upper-case. The Screenplay bar shows the element and the scene's pages and running time.

use std::sync::Arc;

use egui::text::{CCursor, CCursorRange, LayoutJob};
use egui::{FontFamily, FontId, Galley, TextFormat, Ui};
use prosecraft_core::richtext::{LineKind, Mark, RichDoc, ScreenplayKind};
use prosecraft_core::screenplay::editor_keys::{
    EDITOR_ELEMENT_TYPES, EditorKey, auto_case, auto_detect_type, element_label, next_element_type,
};

use crate::editing::Draft;
use crate::theme::Palette;

/// The type size: ProseCraft's 12 pt.
const SIZE: f32 = 16.0;

/// The line holding character `cursor`.
pub(crate) fn line_at(doc: &RichDoc, cursor: usize) -> usize {
    doc.line_ranges()
        .iter()
        .position(|(start, end)| cursor >= *start && cursor <= *end)
        .unwrap_or(0)
}

/// A line's element type ("action" for a paragraph that isn't one yet).
pub(crate) fn kind_of(doc: &RichDoc, line: usize) -> &'static str {
    match doc.lines.get(line) {
        Some(LineKind::Screenplay { kind, .. }) => kind.as_str(),
        _ => "action",
    }
}

fn line_text(doc: &RichDoc, line: usize) -> String {
    let (start, end) = doc.line_ranges().get(line).copied().unwrap_or((0, 0));
    doc.text.chars().skip(start).take(end - start).collect()
}

/// Gives line `line` the element type `kind`, upper-cased as that type is (`autoCase`). Returns
/// the cursor moved by any change in length.
fn set_line(draft: &mut Draft, line: usize, kind: &str, cursor: usize) -> usize {
    let Some(sp) = ScreenplayKind::from_name(kind) else {
        return cursor;
    };
    let (start, end) = draft.doc.line_ranges().get(line).copied().unwrap_or((0, 0));
    let bare = matches!(
        draft.doc.lines.get(line),
        Some(LineKind::Screenplay { bare: true, .. })
    ) && start == end;
    draft
        .doc
        .set_line_kind(start..start, LineKind::Screenplay { kind: sp, bare });
    let text = line_text(&draft.doc, line);
    let cased = auto_case(kind, &text);
    if cased == text {
        return cursor;
    }
    let grown = cased.chars().count() as isize - text.chars().count() as isize;
    let chars: Vec<char> = draft.doc.text.chars().collect();
    let new_text: String = chars[..start]
        .iter()
        .copied()
        .chain(cased.chars())
        .chain(chars[end..].iter().copied())
        .collect();
    let cursor = (cursor as isize + if cursor >= start { grown } else { 0 }).max(0) as usize;
    draft.body = draft.doc.apply_text_at(&new_text, Some(cursor));
    cursor
}

fn put_cursor(ui: &Ui, id: egui::Id, at: usize) {
    let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(at))));
    state.store(ui.ctx(), id);
}

/// Enter, Tab and Shift+Tab, taken before the text field sees them (`handleScreenplayKey`).
/// Returns whether the draft changed.
pub(crate) fn handle_keys(
    ui: &Ui,
    draft: &mut Draft,
    body_id: egui::Id,
    selection: Option<std::ops::Range<usize>>,
) -> bool {
    if !ui.memory(|m| m.has_focus(body_id)) {
        return false;
    }
    // (Tab stays in the field: it is a `lock_focus` text field in a screenplay.)
    // As ProseCraft: not with Ctrl, Cmd or Alt, and Shift+Enter is the field's own. (egui
    // matches a key ignoring extra Shift, so Shift+Tab is taken first.)
    let mods = ui.input(|i| i.modifiers);
    let plain = !mods.alt && !mods.command && !mods.ctrl && !mods.mac_cmd;
    let shift_tab = plain
        && mods.shift
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
    let tab = plain
        && !mods.shift
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
    let enter = plain
        && !mods.shift
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));

    let Some(range) = selection else { return false };
    if tab || shift_tab {
        let line = line_at(&draft.doc, range.start);
        let empty = line_text(&draft.doc, line).trim().is_empty();
        let next = next_element_type(
            kind_of(&draft.doc, line),
            if shift_tab {
                EditorKey::ShiftTab
            } else {
                EditorKey::Tab
            },
            empty,
        );
        let at = set_line(draft, line, next, range.start);
        put_cursor(ui, body_id, at);
        return true;
    }
    if !enter {
        return false;
    }
    let mut cursor = range.start;
    if !range.is_empty() {
        let chars: Vec<char> = draft.doc.text.chars().collect();
        let without: String = chars[..range.start]
            .iter()
            .chain(&chars[range.end..])
            .collect();
        draft.body = draft.doc.apply_text_at(&without, Some(range.start));
    }
    let line = line_at(&draft.doc, cursor);
    let text = line_text(&draft.doc, line);
    let empty = text.trim().is_empty();
    let mut current = kind_of(&draft.doc, line);
    if current == "action"
        && !empty
        && let Some(detected) = auto_detect_type(&text)
    {
        current = detected;
    }
    cursor = set_line(draft, line, current, cursor);
    let next = next_element_type(current, EditorKey::Enter, empty);
    if empty && next != current {
        // An empty element changes type instead of starting another.
        cursor = set_line(draft, line, next, cursor);
        put_cursor(ui, body_id, cursor);
        return true;
    }
    let chars: Vec<char> = draft.doc.text.chars().collect();
    let split: String = chars[..cursor]
        .iter()
        .copied()
        .chain(std::iter::once('\n'))
        .chain(chars[cursor..].iter().copied())
        .collect();
    draft.body = draft.doc.apply_text_at(&split, Some(cursor + 1));
    let new_line = line + 1;
    let start = draft
        .doc
        .line_ranges()
        .get(new_line)
        .map_or(cursor + 1, |r| r.0);
    if let Some(sp) = ScreenplayKind::from_name(next) {
        draft.doc.set_line_kind(
            start..start,
            LineKind::Screenplay {
                kind: sp,
                bare: false,
            },
        );
    }
    let at = set_line(draft, new_line, next, cursor + 1);
    put_cursor(ui, body_id, at);
    true
}

/// After typing (`syncScreenplayTyping`): the line becomes an element; a typed heading or
/// transition is recognized; and it is upper-cased as its type is.
pub(crate) fn sync_typing(ui: &Ui, draft: &mut Draft, body_id: egui::Id, cursor: usize) {
    let line = line_at(&draft.doc, cursor);
    let mut current = kind_of(&draft.doc, line);
    if let Some(detected @ ("scene-heading" | "transition")) =
        auto_detect_type(&line_text(&draft.doc, line))
    {
        current = detected;
    }
    let at = set_line(draft, line, current, cursor);
    if at != cursor {
        put_cursor(ui, body_id, at);
    }
}

/// The Screenplay bar: the element under the caret (to change it), and the scene's length.
pub(crate) fn bar(
    ui: &mut Ui,
    palette: &Palette,
    draft: &mut Draft,
    body_id: egui::Id,
    cursor: usize,
) -> bool {
    let line = line_at(&draft.doc, cursor);
    let current = kind_of(&draft.doc, line);
    let mut chosen = current;
    let mut changed = false;
    ui.horizontal(|ui| {
        let combo = craft_ui::ComboBox::from_id_salt("screenplay-element")
            .selected_text(element_label(current))
            .show_ui(ui, |ui| {
                for kind in EDITOR_ELEMENT_TYPES {
                    ui.selectable_value(&mut chosen, kind, element_label(kind));
                }
            });
        ui.ctx().accesskit_node_builder(combo.response.id, |node| {
            node.set_label("Element type");
            node.set_value(element_label(current));
        });
        let elements = prosecraft_core::screenplay::layout::html_to_elements(&draft.doc.to_html());
        let pages = prosecraft_core::screenplay::layout::estimate_pages(&elements).pages;
        let minutes = prosecraft_core::screenplay::layout::estimate_runtime(pages);
        ui.label(
            egui::RichText::new(format!(
                "{pages} {} · ~{minutes} min",
                if pages == 1 { "page" } else { "pages" }
            ))
            .size(12.0)
            .color(palette.text_muted),
        );
    });
    if chosen != current {
        let at = set_line(draft, line, chosen, cursor);
        put_cursor(ui, body_id, at);
        ui.memory_mut(|m| m.request_focus(body_id));
        changed = true;
    }
    changed
}

/// An element's column (ProseCraft's `p[data-sp]` styles): left edge and width as shares of
/// the page, space above and below in lines, and whether it sits against the right margin.
fn column(kind: &str) -> (f32, f32, f32, f32, bool) {
    match kind {
        "scene-heading" => (0.0, 1.0, 1.5, 1.0, false),
        "character" => (0.37, 0.63, 1.0, 0.0, false),
        "dialogue" => (0.25, 0.55, 0.0, 1.0, false),
        "parenthetical" => (0.31, 0.35, 0.0, 0.0, false),
        "transition" => (0.0, 1.0, 1.0, 1.0, true),
        _ => (0.0, 1.0, 0.0, 1.0, false),
    }
}

fn format_for(doc: &RichDoc, index: usize, kind: &str, palette: &Palette) -> TextFormat {
    let marks = doc.marks.get(index).map_or(&[][..], Vec::as_slice);
    let heading = matches!(kind, "scene-heading" | "character" | "transition");
    let mut format = TextFormat {
        font_id: FontId::new(SIZE, FontFamily::Monospace),
        color: if heading {
            palette.text_heading
        } else {
            palette.text_body
        },
        italics: Mark::Italic.applies(marks),
        ..TextFormat::default()
    };
    if Mark::Underline.applies(marks) {
        format.underline = egui::Stroke::new(1.0, format.color);
    }
    if Mark::Strike.applies(marks) {
        format.strikethrough = egui::Stroke::new(1.0, format.color);
    }
    if Mark::Bold.applies(marks) || kind == "scene-heading" {
        // Monospace has no bold face here: a heavier colour stands in.
        format.color = palette.text_heading;
    }
    format
}

/// The scene laid out element by element, each in its column, joined into one galley so the
/// text field's caret, selection and accessibility work across it.
pub(crate) fn galley(
    ui: &Ui,
    doc: &RichDoc,
    text: &str,
    palette: &Palette,
    wrap: f32,
) -> Arc<Galley> {
    let line_height = ui.fonts_mut(|f| f.row_height(&FontId::new(SIZE, FontFamily::Monospace)));
    let mut galleys = Vec::new();
    let mut previous_bottom: Option<f32> = None;
    let mut index = 0;
    for (line, part) in text.split('\n').enumerate() {
        let kind = match doc.lines.get(line) {
            Some(LineKind::Screenplay { kind, .. }) => kind.as_str(),
            _ => "action",
        };
        let (left, share, top, bottom, right) = column(kind);
        let width = (wrap * share).max(40.0);
        let mut job = LayoutJob::default();
        job.wrap.max_width = width;
        job.direction = crate::views::SCENE_DIRECTION;
        for c in part.chars() {
            let format = format_for(doc, index, kind, palette);
            let mut buf = [0u8; 4];
            job.append(c.encode_utf8(&mut buf), 0.0, format);
            index += 1;
        }
        if part.is_empty() {
            job.append("", 0.0, format_for(doc, index, kind, palette));
        }
        index += 1; // the line break
        let laid = ui.fonts_mut(|f| f.layout_job(job));
        let mut placed: Galley = (*laid).clone();
        // Collapsing margins: the larger of the space below the last and above this one.
        let gap = previous_bottom.map_or(0.0, |b| b.max(top)) * line_height;
        for row in &mut placed.rows {
            let shift = if right {
                width - row.rect().width()
            } else {
                0.0
            };
            row.pos.x += wrap * left + shift.max(0.0);
            row.pos.y += gap;
        }
        galleys.push(Arc::new(placed));
        previous_bottom = Some(bottom);
    }
    let mut whole = LayoutJob::simple(
        text.to_owned(),
        FontId::new(SIZE, FontFamily::Monospace),
        palette.text_body,
        wrap,
    );
    whole.wrap.max_width = wrap;
    let pixels_per_point = ui.ctx().pixels_per_point();
    Arc::new(Galley::concat(Arc::new(whole), &galleys, pixels_per_point))
}
