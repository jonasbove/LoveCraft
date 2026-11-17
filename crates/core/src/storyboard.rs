//! The freeform storyboard (ProseCraft `src/lib/storyboard-layout.ts` and the page's storyboard
//! handlers, upstream `7dae544`): cards for chapters, scenes, wiki pages or nothing in
//! particular, placed on a canvas, and diagram drawings (lines, arrows, boxes, text).

use crate::edit::EditError;
use crate::js::{JsObject, JsString, JsValue};

pub const MIN_ZOOM: f32 = 0.4;
pub const MAX_ZOOM: f32 = 2.5;
const LEGACY_CARD_STEP_X: f64 = 250.0;
const LEGACY_ROW_STEP_Y: f64 = 290.0;
const MAX_COORDINATE: f64 = 10_000_000.0;
/// How far an arrow key moves a card.
pub const NUDGE: f64 = 24.0;
pub const TITLE_LIMIT: usize = 120;
pub const SUMMARY_LIMIT: usize = 4000;
/// A card's size on the canvas (ProseCraft's `.storyboard-card` is 220 wide).
pub const CARD_WIDTH: f32 = 220.0;

/// JavaScript's `Math.round`.
pub fn js_round(x: f64) -> f64 {
    let r = (x + 0.5).floor();
    if r == 0.0 && x < 0.0 { -0.0 } else { r }
}

/// `clampBoardZoom`.
pub fn clamp_zoom(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    }
}

/// `boardPointAtViewportCenter`: where an added card goes (its top-left).
pub fn point_at_viewport_center(
    width: f64,
    height: f64,
    pan: (f64, f64),
    offset: f64,
    zoom: f64,
) -> (f64, f64) {
    let scale = if zoom.is_finite() && zoom > 0.0 {
        zoom
    } else {
        1.0
    };
    (
        js_round(pan.0 + width / (2.0 * scale) - 110.0 + offset),
        js_round(pan.1 + height / (2.0 * scale) - 80.0 + offset),
    )
}

/// A card on the board.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub id: String,
    pub source_id: String,
    pub row_id: String,
    pub x: f64,
    pub y: f64,
    /// "chapter", "scene", "blank" or "Wiki".
    pub kind: String,
    pub title: String,
    pub note: String,
}

/// A diagram drawing.
#[derive(Clone, Debug, PartialEq)]
pub enum Drawing {
    Line {
        id: String,
        arrow: bool,
        start: (f64, f64),
        end: (f64, f64),
    },
    Box {
        id: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    Text {
        id: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        text: String,
        font_size: f64,
    },
}

fn string(v: &JsValue, key: &str) -> Option<String> {
    match v.get(key) {
        Some(JsValue::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    }
}

fn number(v: &JsValue, key: &str) -> Option<f64> {
    match v.get(key) {
        Some(JsValue::Number(n)) => Some(*n),
        _ => None,
    }
}

const KINDS: [&str; 4] = ["chapter", "scene", "blank", "Wiki"];
/// The former Story Spaces: a card of one of these is a Wiki card (`migrateStoryboardCard`,
/// which ProseCraft applies when it loads a project, before it filters the cards).
const LEGACY_SPACES: [&str; 4] = ["Characters", "Places", "Research", "Notes"];

/// A card's kind after ProseCraft's Wiki migration.
fn migrated_kind(card: &JsValue) -> Option<String> {
    string(card, "kind").map(|k| {
        if LEGACY_SPACES.contains(&k.as_str()) {
            "Wiki".to_owned()
        } else {
            k
        }
    })
}

fn valid_card(card: &JsValue) -> bool {
    matches!(card, JsValue::Object(_))
        && string(card, "id").is_some()
        && string(card, "sourceId").is_some()
        && migrated_kind(card).is_some_and(|k| KINDS.contains(&k.as_str()))
        && string(card, "title").is_some()
        && string(card, "note").is_some()
}

/// The saved rows' ids (`{ id, title }` strings), in order.
fn row_ids(project: &JsValue) -> Vec<String> {
    project
        .get("storyboardRows")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter(|r| string(r, "title").is_some())
        .filter_map(|r| string(r, "id"))
        .collect()
}

/// The cards as ProseCraft loads them: invalid ones dropped, unknown rows the first row, and
/// cards without a usable position laid out in their former row (`positionStoryboardCards`).
/// `default_row` stands in when no row is saved.
fn positioned(project: &JsValue, default_row: &str) -> Vec<(JsValue, f64, f64, String)> {
    let rows = row_ids(project);
    let first = rows.first().map_or(default_row, String::as_str).to_owned();
    let mut next_in_row: Vec<(String, f64)> = Vec::new();
    project
        .get("storyboardCards")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter(|c| valid_card(c))
        .map(|card| {
            let row = string(card, "rowId")
                .filter(|r| rows.contains(r))
                .unwrap_or_else(|| first.clone());
            let row_position = rows.iter().position(|r| *r == row).unwrap_or(0) as f64;
            let column = match next_in_row.iter_mut().find(|(r, _)| *r == row) {
                Some((_, n)) => {
                    *n += 1.0;
                    *n - 1.0
                }
                None => {
                    next_in_row.push((row.clone(), 1.0));
                    0.0
                }
            };
            let usable = |n: Option<f64>| n.filter(|n| n.is_finite() && n.abs() <= MAX_COORDINATE);
            let (x, y) = match (usable(number(card, "x")), usable(number(card, "y"))) {
                (Some(x), Some(y)) => (js_round(x), js_round(y)),
                _ => (
                    40.0 + column * LEGACY_CARD_STEP_X,
                    40.0 + row_position * LEGACY_ROW_STEP_Y,
                ),
            };
            (card.clone(), x, y, row)
        })
        .collect()
}

/// The board's cards.
pub fn cards(project: &JsValue) -> Vec<Card> {
    positioned(project, "")
        .into_iter()
        .map(|(c, x, y, row_id)| Card {
            id: string(&c, "id").unwrap_or_default(),
            source_id: string(&c, "sourceId").unwrap_or_default(),
            row_id,
            x,
            y,
            kind: migrated_kind(&c).unwrap_or_default(),
            title: string(&c, "title").unwrap_or_default(),
            note: string(&c, "note").unwrap_or_default(),
        })
        .collect()
}

/// `normalizeStoryboardTextFontSize`.
pub fn text_font_size(value: Option<f64>) -> f64 {
    match value {
        Some(n) if n.is_finite() => js_round(n.clamp(8.0, 72.0)),
        _ => 14.0,
    }
}

/// `normalizeStoryboardDrawings`: malformed drawings are left out.
pub fn drawings(project: &JsValue) -> Vec<Drawing> {
    let ok = |n: Option<f64>| n.filter(|n| n.is_finite() && n.abs() <= MAX_COORDINATE);
    project
        .get("storyboardDrawings")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|d| {
            let id = string(d, "id").filter(|i| !i.is_empty())?;
            let kind = string(d, "type")?;
            match kind.as_str() {
                "line" | "arrow" => {
                    let [x1, y1, x2, y2] =
                        ["startX", "startY", "endX", "endY"].map(|k| ok(number(d, k)));
                    Some(Drawing::Line {
                        id,
                        arrow: kind == "arrow",
                        start: (js_round(x1?), js_round(y1?)),
                        end: (js_round(x2?), js_round(y2?)),
                    })
                }
                "box" | "text" => {
                    let [x, y, w, h] = ["x", "y", "width", "height"].map(|k| ok(number(d, k)));
                    let (x, y, w, h) = (x?, y?, w?, h?);
                    if w <= 0.0 || h <= 0.0 {
                        return None;
                    }
                    let (x, y, width, height) =
                        (js_round(x), js_round(y), js_round(w), js_round(h));
                    if kind == "box" {
                        Some(Drawing::Box {
                            id,
                            x,
                            y,
                            width,
                            height,
                        })
                    } else {
                        Some(Drawing::Text {
                            id,
                            x,
                            y,
                            width,
                            height,
                            text: string(d, "text")?,
                            font_size: text_font_size(number(d, "fontSize")),
                        })
                    }
                }
                _ => None,
            }
        })
        .collect()
}

/// Rewrites the stored cards as ProseCraft saves them after loading (and saves a first row
/// when there is none, as its load creates one).
fn normalize(project: &mut JsValue, new_id: &mut dyn FnMut() -> String) -> Result<(), EditError> {
    let object = project.as_object_mut().ok_or(EditError::Shape)?;
    let has_row = matches!(object.get("storyboardRows"), Some(JsValue::Array(rows)) if rows.iter().any(|r| string(r, "id").is_some() && string(r, "title").is_some()));
    if !has_row {
        let mut row = JsObject::new();
        row.insert("id", JsValue::str(&new_id()));
        row.insert("title", JsValue::str("Timeline"));
        object.insert("storyboardRows", JsValue::Array(vec![JsValue::Object(row)]));
    }
    let cards = positioned(project, "")
        .into_iter()
        .map(|(mut card, x, y, row)| {
            let kind = migrated_kind(&card);
            if let Some(o) = card.as_object_mut() {
                if let Some(kind) = kind {
                    o.insert("kind", JsValue::str(&kind));
                }
                o.insert("rowId", JsValue::str(&row));
                o.insert("x", JsValue::Number(x));
                o.insert("y", JsValue::Number(y));
            }
            card
        })
        .collect();
    project
        .as_object_mut()
        .ok_or(EditError::Shape)?
        .insert("storyboardCards", JsValue::Array(cards));
    Ok(())
}

fn cards_mut(project: &mut JsValue) -> Result<&mut Vec<JsValue>, EditError> {
    match project
        .as_object_mut()
        .and_then(|o| o.get_mut("storyboardCards"))
    {
        Some(JsValue::Array(list)) => Ok(list),
        _ => Err(EditError::Shape),
    }
}

fn card_mut<'a>(project: &'a mut JsValue, id: &str) -> Result<&'a mut JsObject, EditError> {
    cards_mut(project)?
        .iter_mut()
        .find(|c| string(c, "id").as_deref() == Some(id))
        .and_then(JsValue::as_object_mut)
        .ok_or(EditError::Shape)
}

fn capped(value: &str, limit: usize) -> JsValue {
    JsValue::String(JsString::from_units(
        value.encode_utf16().take(limit).collect(),
    ))
}

/// `addStoryboardCard` / a drop: a new card at `(x, y)` in the first row. `title` and `note` are
/// `createStoryboardCard`'s (the chapter's label and summary, the page's title, or
/// "Untitled card" for a blank one).
#[allow(clippy::too_many_arguments)]
pub fn add_card(
    project: &mut JsValue,
    new_id: &mut dyn FnMut() -> String,
    kind: &str,
    source_id: &str,
    title: &str,
    note: &str,
    x: f64,
    y: f64,
) -> Result<(), EditError> {
    normalize(project, new_id)?;
    let row = row_ids(project).into_iter().next().unwrap_or_default();
    let mut card = JsObject::new();
    card.insert("id", JsValue::str(&new_id()));
    card.insert(
        "sourceId",
        JsValue::str(if kind == "blank" { "" } else { source_id }),
    );
    card.insert("kind", JsValue::str(kind));
    card.insert("title", JsValue::str(title));
    card.insert("note", JsValue::str(note));
    card.insert("rowId", JsValue::str(&row));
    card.insert("x", JsValue::Number(js_round(x)));
    card.insert("y", JsValue::Number(js_round(y)));
    cards_mut(project)?.push(JsValue::Object(card));
    Ok(())
}

/// A card dropped at `(x, y)`, or nudged there.
pub fn move_card(
    project: &mut JsValue,
    new_id: &mut dyn FnMut() -> String,
    id: &str,
    x: f64,
    y: f64,
) -> Result<(), EditError> {
    normalize(project, new_id)?;
    let card = card_mut(project, id)?;
    card.insert("x", JsValue::Number(js_round(x)));
    card.insert("y", JsValue::Number(js_round(y)));
    Ok(())
}

/// `removeStoryboardCard`.
pub fn remove_card(
    project: &mut JsValue,
    new_id: &mut dyn FnMut() -> String,
    id: &str,
) -> Result<(), EditError> {
    normalize(project, new_id)?;
    cards_mut(project)?.retain(|c| string(c, "id").as_deref() != Some(id));
    Ok(())
}

/// `updateStoryboardCard` for the title (120 at most).
pub fn set_card_title(
    project: &mut JsValue,
    new_id: &mut dyn FnMut() -> String,
    id: &str,
    title: &str,
) -> Result<(), EditError> {
    normalize(project, new_id)?;
    card_mut(project, id)?.insert("title", capped(title, TITLE_LIMIT));
    Ok(())
}

/// `updateStoryboardCard` for the note: a chapter card's note is its chapter's summary
/// (`syncChapterSummary`).
pub fn set_card_note(
    project: &mut JsValue,
    new_id: &mut dyn FnMut() -> String,
    id: &str,
    note: &str,
) -> Result<(), EditError> {
    normalize(project, new_id)?;
    let card = card_mut(project, id)?;
    let chapter = (string(&JsValue::Object(card.clone()), "kind").as_deref() == Some("chapter"))
        .then(|| string(&JsValue::Object(card.clone()), "sourceId"))
        .flatten();
    match chapter {
        Some(chapter) => sync_chapter_summary(project, &chapter, note),
        None => {
            card.insert("note", JsValue::str(note));
            Ok(())
        }
    }
}

/// `syncChapterSummary`: the summary (4000 at most) on the chapter and on its storyboard cards.
pub fn sync_chapter_summary(
    project: &mut JsValue,
    chapter: &str,
    summary: &str,
) -> Result<(), EditError> {
    let summary = capped(summary, SUMMARY_LIMIT);
    let object = project.as_object_mut().ok_or(EditError::Shape)?;
    let Some(JsValue::Array(chapters)) = object.get_mut("chapters") else {
        return Err(EditError::NoChapter(chapter.to_owned()));
    };
    let target = chapters
        .iter_mut()
        .find(|c| string(c, "id").as_deref() == Some(chapter))
        .and_then(JsValue::as_object_mut)
        .ok_or_else(|| EditError::NoChapter(chapter.to_owned()))?;
    target.insert("summary", summary.clone());
    if let Some(JsValue::Array(cards)) = object.get_mut("storyboardCards") {
        for card in cards.iter_mut().filter(|c| {
            string(c, "kind").as_deref() == Some("chapter")
                && string(c, "sourceId").as_deref() == Some(chapter)
        }) {
            if let Some(o) = card.as_object_mut() {
                o.insert("note", summary.clone());
            }
        }
    }
    Ok(())
}

impl Drawing {
    pub fn id(&self) -> &str {
        match self {
            Self::Line { id, .. } | Self::Box { id, .. } | Self::Text { id, .. } => id,
        }
    }

    fn with_id(&self, new: String) -> Self {
        let mut d = self.clone();
        match &mut d {
            Self::Line { id, .. } | Self::Box { id, .. } | Self::Text { id, .. } => *id = new,
        }
        d
    }

    /// The stored record (ProseCraft's field order).
    fn to_js(&self) -> JsObject {
        let mut o = JsObject::new();
        o.insert("id", JsValue::str(self.id()));
        match self {
            Self::Line {
                arrow, start, end, ..
            } => {
                o.insert("type", JsValue::str(if *arrow { "arrow" } else { "line" }));
                o.insert("startX", JsValue::Number(start.0));
                o.insert("startY", JsValue::Number(start.1));
                o.insert("endX", JsValue::Number(end.0));
                o.insert("endY", JsValue::Number(end.1));
            }
            Self::Box {
                x,
                y,
                width,
                height,
                ..
            } => {
                o.insert("type", JsValue::str("box"));
                for (k, v) in [("x", x), ("y", y), ("width", width), ("height", height)] {
                    o.insert(k, JsValue::Number(*v));
                }
            }
            Self::Text {
                x,
                y,
                width,
                height,
                text,
                font_size,
                ..
            } => {
                o.insert("type", JsValue::str("text"));
                for (k, v) in [("x", x), ("y", y), ("width", width), ("height", height)] {
                    o.insert(k, JsValue::Number(*v));
                }
                o.insert("text", JsValue::str(text));
                o.insert("fontSize", JsValue::Number(*font_size));
            }
        }
        o
    }
}

/// `drawingBoundsFromPoints`: a rectangle from any corner to any corner, at least `minimum`.
pub fn bounds_from_points(
    start: (f64, f64),
    end: (f64, f64),
    minimum: f64,
) -> (f64, f64, f64, f64) {
    (
        js_round(start.0.min(end.0)),
        js_round(start.1.min(end.1)),
        js_round((end.0 - start.0).abs()).max(minimum),
        js_round((end.1 - start.1).abs()).max(minimum),
    )
}

/// `moveStoryboardDrawing`.
pub fn move_drawing(drawing: &Drawing, dx: f64, dy: f64) -> Drawing {
    let mut d = drawing.clone();
    match &mut d {
        Drawing::Line { start, end, .. } => {
            *start = (js_round(start.0 + dx), js_round(start.1 + dy));
            *end = (js_round(end.0 + dx), js_round(end.1 + dy));
        }
        Drawing::Box { x, y, .. } | Drawing::Text { x, y, .. } => {
            *x = js_round(*x + dx);
            *y = js_round(*y + dy);
        }
    }
    d
}

/// `resizeStoryboardDrawing` (boxes and text only).
pub fn resize_drawing(
    drawing: &Drawing,
    dx: f64,
    dy: f64,
    min_width: f64,
    min_height: f64,
) -> Drawing {
    let mut d = drawing.clone();
    if let Drawing::Box { width, height, .. } | Drawing::Text { width, height, .. } = &mut d {
        *width = js_round(*width + dx).max(min_width);
        *height = js_round(*height + dy).max(min_height);
    }
    d
}

/// The stored drawings as ProseCraft saves them after loading (`normalizeStoryboardDrawings`):
/// malformed ones dropped, the rest rounded, fields it doesn't know kept.
fn normalize_drawings(project: &mut JsValue) -> Result<(), EditError> {
    let valid = drawings(project);
    let stored: Vec<JsValue> = project
        .get("storyboardDrawings")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default();
    let mut next = Vec::new();
    let mut used = vec![false; stored.len()];
    for drawing in &valid {
        // The first stored record of each valid drawing, in order (ids may repeat).
        let found = stored.iter().enumerate().find(|(i, s)| {
            !used[*i]
                && string(s, "id").as_deref() == Some(drawing.id())
                && drawings(&wrap(s)).first() == Some(drawing)
        });
        let mut record = match found {
            Some((i, s)) => {
                used[i] = true;
                s.as_object().cloned().unwrap_or_default()
            }
            None => JsObject::new(),
        };
        for (k, v) in drawing.to_js().iter() {
            record.insert(k.clone(), v.clone());
        }
        next.push(JsValue::Object(record));
    }
    project
        .as_object_mut()
        .ok_or(EditError::Shape)?
        .insert("storyboardDrawings", JsValue::Array(next));
    Ok(())
}

fn wrap(drawing: &JsValue) -> JsValue {
    let mut o = JsObject::new();
    o.insert("storyboardDrawings", JsValue::Array(vec![drawing.clone()]));
    JsValue::Object(o)
}

fn drawings_mut(project: &mut JsValue) -> Result<&mut Vec<JsValue>, EditError> {
    normalize_drawings(project)?;
    match project
        .as_object_mut()
        .and_then(|o| o.get_mut("storyboardDrawings"))
    {
        Some(JsValue::Array(list)) => Ok(list),
        _ => Err(EditError::Shape),
    }
}

/// A new drawing (its id is made here), last.
pub fn add_drawing(
    project: &mut JsValue,
    new_id: &mut dyn FnMut() -> String,
    drawing: &Drawing,
) -> Result<(), EditError> {
    let drawing = drawing.with_id(new_id());
    drawings_mut(project)?.push(JsValue::Object(drawing.to_js()));
    Ok(())
}

/// A drawing moved, resized or retyped (`replaceStoryboardDrawing`).
pub fn set_drawing(project: &mut JsValue, drawing: &Drawing) -> Result<(), EditError> {
    let list = drawings_mut(project)?;
    let record = list
        .iter_mut()
        .find(|d| string(d, "id").as_deref() == Some(drawing.id()))
        .and_then(JsValue::as_object_mut)
        .ok_or(EditError::Shape)?;
    for (k, v) in drawing.to_js().iter() {
        record.insert(k.clone(), v.clone());
    }
    Ok(())
}

/// `removeStoryboardDrawing`.
pub fn remove_drawing(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    drawings_mut(project)?.retain(|d| string(d, "id").as_deref() != Some(id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Drawing, add_card, cards, drawings, js_round, move_card, point_at_viewport_center,
        remove_card, set_card_note, set_card_title,
    };
    use crate::js::{parse, stringify};

    #[test]
    fn legacy_cards_are_laid_out_by_row_and_order() {
        let p = parse(r#"{"storyboardRows":[{"id":"r1","title":"A"},{"id":"r2","title":"B"}],"storyboardCards":[
            {"id":"a","sourceId":"","kind":"blank","title":"A","note":"","rowId":"r2"},
            {"id":"b","sourceId":"","kind":"blank","title":"B","note":"","rowId":"r2"},
            {"id":"c","sourceId":"","kind":"blank","title":"C","note":"","rowId":"gone","x":10.6,"y":-3.5},
            {"id":"d","sourceId":"","kind":"Research","title":"D","note":""}]}"#).expect("json");
        let got: Vec<_> = cards(&p)
            .into_iter()
            .map(|c| (c.id, c.row_id, c.x, c.y))
            .collect();
        assert_eq!(
            got,
            [
                ("a".into(), "r2".into(), 40.0, 330.0),
                ("b".into(), "r2".into(), 290.0, 330.0),
                ("c".into(), "r1".into(), 11.0, -3.0),
                // A card of a former Story Space is a Wiki card, laid out after "c" in the first row.
                ("d".into(), "r1".into(), 290.0, 40.0)
            ]
        );
        assert_eq!(js_round(-3.5), -3.0);
        assert_eq!(
            point_at_viewport_center(700.0, 600.0, (0.0, 0.0), 24.0, 1.0),
            (264.0, 244.0)
        );
    }

    #[test]
    fn cards_of_the_former_story_spaces_are_wiki_cards() {
        let mut p = parse(r#"{"storyboardRows":[{"id":"r","title":"T"}],"storyboardCards":[{"id":"c","sourceId":"d","kind":"Characters","title":"Mara","note":"","rowId":"r","x":1,"y":2},{"id":"n","sourceId":"","kind":"Mystery","title":"?","note":""}]}"#).expect("json");
        assert_eq!(
            cards(&p)
                .iter()
                .map(|c| (c.id.as_str(), c.kind.as_str()))
                .collect::<Vec<_>>(),
            [("c", "Wiki")]
        );
        let mut n = 0;
        let mut new_id = || {
            n += 1;
            format!("id{n}")
        };
        move_card(&mut p, &mut new_id, "c", 5.0, 6.0).expect("move");
        assert_eq!(
            stringify(&p),
            r#"{"storyboardRows":[{"id":"r","title":"T"}],"storyboardCards":[{"id":"c","sourceId":"d","kind":"Wiki","title":"Mara","note":"","rowId":"r","x":5,"y":6}]}"#
        );
    }

    #[test]
    fn cards_are_added_moved_edited_and_removed() {
        let mut p = parse(
            r#"{"chapters":[{"id":"ch","title":"One","summary":"old"}],"storyboardCards":[]}"#,
        )
        .expect("json");
        let mut n = 0;
        let mut new_id = || {
            n += 1;
            format!("id{n}")
        };
        add_card(
            &mut p,
            &mut new_id,
            "chapter",
            "ch",
            "Chapter 1",
            "old",
            10.2,
            20.7,
        )
        .expect("add");
        add_card(
            &mut p,
            &mut new_id,
            "blank",
            "ignored",
            "Untitled card",
            "",
            0.0,
            0.0,
        )
        .expect("add");
        move_card(&mut p, &mut new_id, "id3", 48.0, -24.0).expect("move");
        set_card_note(&mut p, &mut new_id, "id2", "new summary").expect("note");
        set_card_title(&mut p, &mut new_id, "id3", &"t".repeat(130)).expect("title");
        let s = stringify(&p);
        assert!(s.starts_with(r#"{"chapters":[{"id":"ch","title":"One","summary":"new summary"}],"storyboardCards":[{"id":"id2","sourceId":"ch","kind":"chapter","title":"Chapter 1","note":"new summary","rowId":"id1","x":10,"y":21},{"id":"id3","sourceId":"","kind":"blank","title":""#), "{s}");
        assert!(
            s.contains(&format!(
                r#""title":"{}","note":"","rowId":"id1","x":48,"y":-24}}]"#,
                "t".repeat(120)
            )),
            "{s}"
        );
        assert!(
            s.ends_with(r#""storyboardRows":[{"id":"id1","title":"Timeline"}]}"#),
            "{s}"
        );
        remove_card(&mut p, &mut new_id, "id2").expect("remove");
        assert_eq!(cards(&p).len(), 1);
    }

    #[test]
    fn drawings_are_added_changed_and_removed() {
        use super::{
            add_drawing, bounds_from_points, move_drawing, remove_drawing, resize_drawing,
            set_drawing,
        };
        let mut p = parse(r#"{"storyboardDrawings":[{"id":"old","type":"box","x":1.6,"y":0,"width":10,"height":10,"color":"red"},{"id":"bad","type":"blob"}]}"#).expect("json");
        let mut n = 0;
        let mut new_id = || {
            n += 1;
            format!("d{n}")
        };
        let (x, y, w, h) = bounds_from_points((50.0, 40.0), (10.0, 38.0), 24.0);
        assert_eq!((x, y, w, h), (10.0, 38.0, 40.0, 24.0));
        add_drawing(
            &mut p,
            &mut new_id,
            &Drawing::Line {
                id: String::new(),
                arrow: true,
                start: (0.0, 0.0),
                end: (5.0, 5.0),
            },
        )
        .expect("add");
        let boxed = drawings(&p)[0].clone();
        set_drawing(
            &mut p,
            &resize_drawing(&move_drawing(&boxed, 10.0, -2.0), -100.0, 6.4, 24.0, 24.0),
        )
        .expect("set");
        assert_eq!(
            stringify(&p),
            r#"{"storyboardDrawings":[{"id":"old","type":"box","x":12,"y":-2,"width":24,"height":24,"color":"red"},{"id":"d1","type":"arrow","startX":0,"startY":0,"endX":5,"endY":5}]}"#
        );
        remove_drawing(&mut p, "old").expect("remove");
        assert_eq!(drawings(&p).len(), 1);
    }

    #[test]
    fn malformed_drawings_are_left_out() {
        let p = parse(r#"{"storyboardDrawings":[{"id":"l","type":"arrow","startX":1.4,"startY":2,"endX":3,"endY":4},{"id":"b","type":"box","x":0,"y":0,"width":0,"height":5},{"id":"t","type":"text","x":0,"y":0,"width":10,"height":5,"text":"Hi","fontSize":100},{"id":"","type":"line","startX":1,"startY":2,"endX":3,"endY":4},{"id":"x","type":"text","x":0,"y":0,"width":10,"height":5}]}"#).expect("json");
        assert_eq!(
            drawings(&p),
            [
                Drawing::Line {
                    id: "l".into(),
                    arrow: true,
                    start: (1.0, 2.0),
                    end: (3.0, 4.0)
                },
                Drawing::Text {
                    id: "t".into(),
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 5.0,
                    text: "Hi".into(),
                    font_size: 72.0
                }
            ]
        );
    }
}
