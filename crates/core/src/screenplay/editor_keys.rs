//! The screenplay editor's keyboard rules (ProseCraft `src/lib/screenplay/editor-keys.ts`,
//! upstream `7dae544`): the element-type cycle, detection for freshly typed lines, and the
//! upper-casing rules.

use crate::html::is_js_whitespace;

/// The element types an editor paragraph can carry, in the Screenplay bar's order.
pub const EDITOR_ELEMENT_TYPES: [&str; 6] = [
    "scene-heading",
    "action",
    "character",
    "dialogue",
    "parenthetical",
    "transition",
];

/// The Screenplay bar's names for them.
pub fn element_label(kind: &str) -> &'static str {
    match kind {
        "scene-heading" => "Scene heading",
        "character" => "Character",
        "dialogue" => "Dialogue",
        "parenthetical" => "Parenthetical",
        "transition" => "Transition",
        _ => "Action",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorKey {
    Enter,
    Tab,
    ShiftTab,
}

/// `SCENE_HEADING` here (`^(?:INT\.?\/EXT\.?|I\/E\.?|INT\.?|EXT\.?|EST\.?)(?:\s|$)`, case-insensitive).
fn is_scene_heading(line: &str) -> bool {
    const STARTS: [&str; 12] = [
        "INT/EXT",
        "INT./EXT",
        "INT/EXT.",
        "INT./EXT.",
        "I/E",
        "I/E.",
        "INT",
        "INT.",
        "EXT",
        "EXT.",
        "EST",
        "EST.",
    ];
    STARTS.iter().any(|start| {
        line.get(..start.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(start))
            && line[start.len()..]
                .chars()
                .next()
                .is_none_or(is_js_whitespace)
    })
}

/// `isAllCaps`: a capital and no lower-case letter (a trailing parenthetical aside).
fn is_all_caps(line: &str) -> bool {
    let name = match line.strip_suffix(')') {
        Some(body) => {
            let from = body.rfind(')').map_or(0, |i| i + 1);
            match body[from..].find('(').map(|i| i + from) {
                Some(open) => line[..open].trim_end_matches(is_js_whitespace),
                None => line,
            }
        }
        None => line,
    };
    let name = name.trim_matches(is_js_whitespace);
    name.chars().any(|c| c.is_ascii_uppercase()) && !name.chars().any(|c| c.is_ascii_lowercase())
}

/// `nextElementType`: Enter starts what usually follows; Tab walks the character, transition
/// and parenthetical cycle, Shift+Tab back.
pub fn next_element_type(current: &str, key: EditorKey, empty: bool) -> &'static str {
    match key {
        EditorKey::Enter => match current {
            "scene-heading" => "action",
            "character" => {
                if empty {
                    "action"
                } else {
                    "dialogue"
                }
            }
            "dialogue" => "character",
            "parenthetical" => "dialogue",
            "transition" => "scene-heading",
            _ => "action",
        },
        EditorKey::Tab => match current {
            "action" => {
                if empty {
                    "character"
                } else {
                    "action"
                }
            }
            "character" => "transition",
            "dialogue" => "parenthetical",
            other => static_kind(other),
        },
        EditorKey::ShiftTab => match current {
            "character" => "action",
            "transition" => "character",
            "parenthetical" => "dialogue",
            other => static_kind(other),
        },
    }
}

fn static_kind(kind: &str) -> &'static str {
    EDITOR_ELEMENT_TYPES
        .iter()
        .find(|k| **k == kind)
        .copied()
        .unwrap_or("action")
}

/// `autoDetectType`: the type a typed line suggests, or `None` to keep the current one.
pub fn auto_detect_type(text: &str) -> Option<&'static str> {
    let line = text.trim_matches(is_js_whitespace);
    if line.is_empty() {
        return None;
    }
    if is_all_caps(line) && line.ends_with("TO:") {
        return Some("transition");
    }
    if is_scene_heading(line) {
        return Some("scene-heading");
    }
    is_all_caps(line).then_some("character")
}

/// `autoCase`: scene headings, character cues and transitions are upper-case.
pub fn auto_case(kind: &str, text: &str) -> String {
    if matches!(kind, "scene-heading" | "character" | "transition") {
        text.to_uppercase()
    } else {
        text.to_owned()
    }
}
