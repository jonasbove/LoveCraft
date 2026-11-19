//! Text beside a wrapped picture: ProseCraft floats a picture whose layout is "wrap left" or
//! "wrap right" (`float: left` / `float: right`), and the lines that follow wrap beside it until
//! they pass its bottom edge.
//!
//! egui lays text out at one width per galley, so the rows beside a picture are laid out again:
//! a paragraph that starts beside one (or holds one) is laid out piece by piece, each piece at the
//! width left beside the pictures at its height, and its rows placed in one galley with the rest.
//! A paragraph clear of the pictures keeps the rows egui laid out for the whole text.

use std::ops::Range;
use std::sync::Arc;

use egui::epaint::text::{ByteIndex, Galley, LayoutJob, LayoutSection, PlacedRow};
use egui::{Rect, Vec2, pos2, vec2};

/// The room above a wrapped picture (`margin: 4px 18px 14px 0`).
pub(crate) const TOP: f32 = 4.0;
/// The room between a wrapped picture and the text beside it.
pub(crate) const GAP: f32 = 18.0;
/// The room below a wrapped picture.
pub(crate) const BOTTOM: f32 = 14.0;
/// Text narrower than this beside the pictures goes below them instead.
const MIN_BAND: f32 = 40.0;

/// A wrapped picture in the text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Float {
    /// Its character's index in the text.
    pub index: usize,
    /// Its atom's index.
    pub atom: usize,
    /// Wrap left (else right).
    pub left: bool,
    /// Its drawn size.
    pub size: Vec2,
}

/// A picture placed in the column: the space it keeps the text out of.
#[derive(Clone, Copy, Debug)]
struct Placed {
    left: bool,
    top: f32,
    bottom: f32,
    /// How far into the column it reaches, its gap to the text included.
    reach: f32,
}

impl Placed {
    fn new(float: &Float, row_top: f32, placed: &[Placed]) -> Self {
        // A picture on the same side still in the way stacks below it.
        let top = placed
            .iter()
            .filter(|p| p.left == float.left && p.bottom > row_top)
            .map(|p| p.bottom)
            .fold(row_top, f32::max);
        Self {
            left: float.left,
            top,
            bottom: top + TOP + float.size.y + BOTTOM,
            reach: float.size.x + GAP,
        }
    }

    fn image(&self, float: &Float, column: f32) -> Rect {
        let x = if self.left {
            0.0
        } else {
            column - float.size.x
        };
        Rect::from_min_size(pos2(x, self.top + TOP), float.size)
    }
}

/// Each wrapped picture's rectangle in `galley` (laid out by [`flow`]), in its coordinates.
pub(crate) fn placed(galley: &Galley, floats: &[Float]) -> Vec<Rect> {
    let column = galley.job.wrap.max_width;
    let mut placed: Vec<Placed> = Vec::new();
    floats
        .iter()
        .map(|float| {
            let top = row_top(galley, float.index);
            let p = Placed::new(float, top, &placed);
            placed.push(p);
            p.image(float, column)
        })
        .collect()
}

/// The top of the row holding character `index`.
fn row_top(galley: &Galley, index: usize) -> f32 {
    let mut start = 0;
    for placed in &galley.rows {
        let count = placed.char_count_including_newline().0;
        if index < start + count {
            return placed.min_y();
        }
        start += count;
    }
    galley.rows.last().map_or(0.0, PlacedRow::max_y)
}

/// The text's paragraphs, as egui splits them to lay them out (the `\n` between them left out;
/// a `\n` ending the text kept in the last).
fn paragraphs(text: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = text
            .get(start..)
            .and_then(|rest| rest.find('\n'))
            .map_or(text.len(), |i| start + i);
        if end + 1 == text.len() && text.ends_with('\n') {
            end += 1;
        }
        out.push(start..end);
        start = end + 1;
    }
    out
}

/// The part `range` of `job`, laid out `width` wide, as egui lays out a paragraph.
fn slice(job: &LayoutJob, range: Range<usize>, width: f32) -> LayoutJob {
    let (start, end) = (range.start, range.end);
    let sections = job
        .sections
        .iter()
        .filter(|s| s.byte_range.end > ByteIndex(start) && s.byte_range.start <= ByteIndex(end))
        .map(|s| LayoutSection {
            // A list item's indent: only where its section starts.
            leading_space: if ByteIndex(start) <= s.byte_range.start {
                s.leading_space
            } else {
                0.0
            },
            byte_range: ByteIndex(s.byte_range.start.0.max(start) - start)
                ..ByteIndex(s.byte_range.end.0.min(end) - start),
            format: s.format.clone(),
        })
        .collect();
    LayoutJob {
        text: job.text.get(range.clone()).unwrap_or_default().to_owned(),
        sections,
        wrap: egui::text::TextWrapping {
            max_width: width,
            ..job.wrap.clone()
        },
        first_row_min_height: if start == 0 {
            job.first_row_min_height
        } else {
            0.0
        },
        ..job.clone()
    }
}

/// The room left beside the pictures at height `y`: its left and right edges, and the height
/// where that changes.
fn band(placed: &[Placed], y: f32, column: f32) -> (f32, f32, Option<f32>) {
    let mut left = 0.0_f32;
    let mut right = column;
    let mut change: Option<f32> = None;
    let mut at = |v: f32| change = Some(change.map_or(v, |c| c.min(v)));
    for p in placed {
        if p.top <= y && y < p.bottom {
            if p.left {
                left = left.max(p.reach);
            } else {
                right = right.min(column - p.reach);
            }
            at(p.bottom);
        } else if p.top > y {
            at(p.top);
        }
    }
    (left, right, change)
}

/// Lays out `job` with its text beside the wrapped pictures `floats` (in text order). A line
/// whose `clears` is set starts below every picture (`clear: both`).
pub(crate) fn flow(
    fonts: &mut egui::epaint::FontsView<'_>,
    job: LayoutJob,
    floats: &[Float],
    clears: &[bool],
) -> Arc<Galley> {
    let column = job.wrap.max_width;
    let full = fonts.layout_job(job);
    if floats.is_empty() || !column.is_finite() || column <= 0.0 {
        return full;
    }
    let mut flow = Flow {
        job: &full.job,
        column,
        floats,
        placed: Vec::new(),
        next: 0,
        unsettled: false,
        rows: Vec::new(),
        y: 0.0,
    };
    let mut old = full.rows.iter();
    let ranges = paragraphs(&full.job.text);
    let mut char_at = 0;
    for (line, range) in ranges.iter().enumerate() {
        let last = line + 1 == ranges.len();
        // This paragraph's rows as laid out for the whole text.
        let mut mine = Vec::new();
        for row in old.by_ref() {
            mine.push(row);
            if row.ends_with_newline && !last {
                break;
            }
        }
        if clears.get(line).copied().unwrap_or(false) {
            flow.clear();
        }
        let chars = full
            .job
            .text
            .get(range.clone())
            .map_or(0, |t| t.chars().count());
        let holds_float = floats
            .get(flow.next)
            .is_some_and(|f| f.index < char_at + chars + 1);
        if holds_float || flow.beside() {
            flow.paragraph(fonts, range.clone(), char_at, !last);
        } else {
            flow.keep(&mine);
        }
        char_at += chars + 1;
    }
    Arc::new(flow.finish(&full))
}

/// The galley being put together by [`flow`].
struct Flow<'a> {
    job: &'a LayoutJob,
    column: f32,
    floats: &'a [Float],
    placed: Vec<Placed>,
    /// The next picture not yet placed.
    next: usize,
    /// The picture placed last may still move down to its character's row.
    unsettled: bool,
    rows: Vec<PlacedRow>,
    /// Where the next row goes.
    y: f32,
}

impl Flow<'_> {
    /// Is a picture beside (or below) where the next row goes?
    fn beside(&self) -> bool {
        self.placed.iter().any(|p| p.bottom > self.y)
    }

    /// The next row starts below every picture.
    fn clear(&mut self) {
        let bottom = self.placed.iter().map(|p| p.bottom).fold(self.y, f32::max);
        self.y = bottom;
    }

    /// A paragraph's rows as egui laid them out, moved down to where it now starts.
    fn keep(&mut self, rows: &[&PlacedRow]) {
        let Some(first) = rows.first() else {
            return;
        };
        let dy = self.y - first.min_y();
        for row in rows {
            let mut row = (*row).clone();
            row.pos.y += dy;
            self.y = row.max_y();
            self.rows.push(row);
        }
    }

    /// Lays out the paragraph at bytes `range` (its first character `first_char`) beside the
    /// pictures, a piece at a time.
    fn paragraph(
        &mut self,
        fonts: &mut egui::epaint::FontsView<'_>,
        range: Range<usize>,
        first_char: usize,
        newline: bool,
    ) {
        let mut start = range.start;
        let mut char_at = first_char;
        // Each round places at least a row or a picture; the bound guards against a loop.
        for _ in 0..(range.len() + self.floats.len() + 4) * 2 {
            let (left, right, change) = band(&self.placed, self.y, self.column);
            if right - left < MIN_BAND
                && let Some(below) = change
            {
                self.y = below;
                continue;
            }
            let width = (right - left).max(MIN_BAND);
            let piece = fonts.layout_job(slice(self.job, start..range.end, width));
            let step = self.take(&piece, (left, change), char_at);
            char_at += step.chars;
            start = self
                .job
                .text
                .get(start..)
                .and_then(|rest| rest.char_indices().nth(step.chars).map(|(i, _)| start + i))
                .unwrap_or(range.end)
                .min(range.end);
            // All of it placed (unless a `\n` ending the text leaves its empty row to come).
            let ended = start >= range.end
                && step.chars > 0
                && !self.rows.last().is_some_and(|r| r.ends_with_newline);
            if step.done || ended {
                break;
            }
        }
        if let Some(row) = self.rows.last_mut() {
            row.ends_with_newline = newline;
        }
    }

    /// Takes the rows of `piece` (laid out from character `char_at`) that fit where they go:
    /// up to a picture not yet placed (which is placed then), or the height where the room
    /// beside the pictures changes.
    fn take(&mut self, piece: &Galley, (left, change): (f32, Option<f32>), char_at: usize) -> Step {
        let mut taken = 0;
        let origin = piece.rows.first().map_or(0.0, PlacedRow::min_y);
        let base = self.y;
        for (i, row) in piece.rows.iter().enumerate() {
            let top = base + row.min_y() - origin;
            if i > 0 && change.is_some_and(|c| top >= c) {
                return Step {
                    chars: taken,
                    done: false,
                };
            }
            let count = row.char_count_including_newline().0;
            let start = char_at + taken;
            if self.moved(start..start + count.max(1), top) {
                return Step {
                    chars: taken,
                    done: false,
                };
            }
            if let Some(float) = self.floats.get(self.next).copied()
                && float.index < start + count.max(1)
            {
                if i > 0 {
                    return Step {
                        chars: taken,
                        done: false,
                    };
                }
                let p = Placed::new(&float, top, &self.placed);
                self.placed.push(p);
                self.next += 1;
                self.unsettled = true;
                // The room beside this row changed: lay it out again.
                if p.top <= top {
                    return Step {
                        chars: 0,
                        done: false,
                    };
                }
                // Stacked below another: the room changes where it starts.
                self.push(row, left, top);
                return Step {
                    chars: taken + count,
                    done: false,
                };
            }
            self.push(row, left, top);
            taken += count;
        }
        Step {
            chars: taken,
            done: true,
        }
    }

    /// Whether the picture placed last, its character in the row at `chars` now going at `top`
    /// (laid out again beside it, its character went to a lower row), moved down to that row:
    /// a picture's top is its character's row's.
    fn moved(&mut self, chars: Range<usize>, top: f32) -> bool {
        let Some(float) = self.next.checked_sub(1).and_then(|i| self.floats.get(i)) else {
            return false;
        };
        if !self.unsettled || !chars.contains(&float.index) {
            return false;
        }
        let Some((last, others)) = self.placed.split_last_mut() else {
            return false;
        };
        let p = Placed::new(float, top, others);
        if (p.top - last.top).abs() < 0.5 {
            self.unsettled = false;
            return false;
        }
        *last = p;
        true
    }

    fn push(&mut self, row: &PlacedRow, left: f32, top: f32) {
        let mut row = row.clone();
        row.pos = pos2(row.pos.x + left, top);
        self.y = row.max_y();
        self.rows.push(row);
    }

    /// The galley: `full`'s, with these rows, as tall as the pictures in it.
    fn finish(self, full: &Galley) -> Galley {
        let mut galley = full.clone();
        let mut rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(0.0, 0.0));
        let mut mesh_bounds = Rect::NOTHING;
        let (mut vertices, mut indices) = (0, 0);
        for row in &self.rows {
            rect |= row.rect();
            mesh_bounds |= row.visuals.mesh_bounds.translate(row.pos.to_vec2());
            vertices += row.visuals.mesh.vertices.len();
            indices += row.visuals.mesh.indices.len();
        }
        for p in &self.placed {
            rect |= Rect::from_x_y_ranges(0.0..=0.0, p.top..=p.bottom);
        }
        rect.max.x = rect.max.x.min(self.column).max(rect.min.x);
        galley.rows = self.rows;
        galley.rect = rect;
        galley.mesh_bounds = mesh_bounds;
        galley.num_vertices = vertices;
        galley.num_indices = indices;
        galley
    }
}

/// What [`Flow::take`] did: the characters it placed, and whether the piece is all placed.
struct Step {
    chars: usize,
    done: bool,
}

#[cfg(test)]
mod tests {
    use super::{BOTTOM, Float, GAP, TOP, flow, paragraphs, placed};
    use egui::epaint::text::{Galley, LayoutJob, TextFormat};
    use egui::text::CCursor;
    use egui::{Rect, pos2};
    use std::sync::Arc;

    // No "fl" or "fi": a caret can't stand inside a ligature.
    const WORDS: &str = "The tide came in over the mud while the lamps went on along the \
        harbour wall, one by one, until the whole town was lit and the boats lay still.";

    /// Lays out with fonts, as the editor would.
    fn with_fonts<R>(f: impl FnOnce(&mut egui::epaint::FontsView<'_>) -> R) -> R {
        let ctx = egui::Context::default();
        crate::setup_for_tests(&ctx);
        ctx.begin_pass(egui::RawInput::default());
        let out = ctx.fonts_mut(f);
        let mut output = ctx.end_pass();
        output.textures_delta.clear();
        out
    }

    fn job(text: &str, width: f32) -> LayoutJob {
        let mut job = LayoutJob::default();
        job.wrap.max_width = width;
        let format = TextFormat {
            font_id: egui::FontId::proportional(17.0),
            line_height: Some(31.0),
            ..Default::default()
        };
        for c in text.chars() {
            let mut buf = [0u8; 4];
            job.append(c.encode_utf8(&mut buf), 0.0, format.clone());
        }
        job
    }

    fn text() -> String {
        format!("{WORDS}\nX\u{2029}{WORDS} {WORDS}\n{WORDS}\n{WORDS} {WORDS} {WORDS} {WORDS}")
    }

    fn picture(text: &str, left: bool) -> Float {
        Float {
            index: text.chars().position(|c| c == 'X').expect("X"),
            atom: 0,
            left,
            size: egui::vec2(150.0, 120.0),
        }
    }

    fn flowed(left: bool) -> (Arc<Galley>, Float, egui::Rect) {
        let text = text();
        let float = picture(&text, left);
        let galley = with_fonts(|f| flow(f, job(&text, 400.0), &[float], &[]));
        let rect = placed(&galley, &[float])[0];
        (galley, float, rect)
    }

    /// Each row with the index of its first character.
    fn rows(galley: &Galley) -> Vec<(usize, egui::Rect)> {
        let mut at = 0;
        galley
            .rows
            .iter()
            .map(|r| {
                let start = at;
                at += r.char_count_including_newline().0;
                (start, r.rect())
            })
            .collect()
    }

    #[test]
    fn text_after_a_left_picture_starts_beside_it_then_at_the_margin() {
        let (galley, float, rect) = flowed(true);
        assert_eq!(rect.left(), 0.0);
        let bottom = rect.bottom() + BOTTOM;
        let after: Vec<_> = rows(&galley)
            .into_iter()
            .filter(|(start, _)| *start > float.index)
            .collect();
        let beside: Vec<_> = after.iter().filter(|(_, r)| r.top() < bottom).collect();
        let below: Vec<_> = after.iter().filter(|(_, r)| r.top() >= bottom).collect();
        assert!(beside.len() >= 3, "{beside:?}");
        assert!(!below.is_empty());
        for (_, row) in &beside {
            assert!(row.left() >= rect.right() + GAP - 0.5, "{row:?} {rect:?}");
            assert!(row.right() <= 400.5, "{row:?}");
        }
        for (_, row) in &below {
            assert!(row.left().abs() < 0.5, "{row:?}");
        }
        // The rows before the picture are where egui put them.
        let plain = with_fonts(|f| f.layout_job(job(&text(), 400.0)));
        assert_eq!(galley.rows[0], plain.rows[0]);
        // The picture's top is its row's.
        let (_, row) = rows(&galley)
            .into_iter()
            .rfind(|(start, _)| *start <= float.index)
            .expect("its row");
        assert!((rect.top() - TOP - row.top()).abs() < 0.5);
    }

    #[test]
    fn text_after_a_right_picture_ends_before_it() {
        let (galley, float, rect) = flowed(false);
        assert!((rect.right() - 400.0).abs() < 0.5);
        let bottom = rect.bottom() + BOTTOM;
        let after: Vec<_> = rows(&galley)
            .into_iter()
            .filter(|(start, _)| *start > float.index)
            .collect();
        let beside: Vec<_> = after.iter().filter(|(_, r)| r.top() < bottom).collect();
        assert!(beside.len() >= 3);
        for (_, row) in &beside {
            assert!(row.left().abs() < 0.5);
            assert!(row.right() <= rect.left() - GAP + 0.5, "{row:?} {rect:?}");
        }
        let below = after
            .iter()
            .find(|(_, r)| r.top() >= bottom)
            .expect("below");
        assert!(below.1.right() > rect.left());
    }

    #[test]
    fn every_caret_position_round_trips() {
        for left in [true, false] {
            let (galley, _, _) = flowed(left);
            let count = text().chars().count();
            let total: usize = galley
                .rows
                .iter()
                .map(|r| r.char_count_including_newline().0)
                .sum();
            assert_eq!(total, count);
            assert_eq!(galley.text(), text());
            for i in 0..=count {
                let rect = galley.pos_from_cursor(CCursor::new(i));
                let back = galley.cursor_from_pos(rect.center().to_vec2());
                assert_eq!(back.index.0, i, "left {left}: {rect:?}");
            }
        }
    }

    /// No row runs into a picture or the room kept around it.
    fn clear_of(galley: &Galley, floats: &[Float]) {
        for (rect, float) in placed(galley, floats).iter().zip(floats) {
            let kept = Rect::from_min_max(
                pos2(rect.left() - GAP + 0.5, rect.top() - TOP + 0.5),
                pos2(rect.right() + GAP - 0.5, rect.bottom() + BOTTOM - 0.5),
            );
            for (start, row) in rows(galley) {
                // The picture's own character is all but invisible.
                if start == float.index && row.width() < 2.0 {
                    continue;
                }
                assert!(
                    !row.intersects(kept) || row.width() < 0.5,
                    "row at {start} {row:?} runs into {kept:?}"
                );
            }
        }
    }

    #[test]
    fn a_picture_after_words_in_a_row_goes_beside_its_own_row() {
        for left in [true, false] {
            let text =
                format!("{WORDS}\nThe lamps along the harbour wall X\u{2029}{WORDS} {WORDS}");
            let float = picture(&text, left);
            let galley = with_fonts(|f| flow(f, job(&text, 400.0), &[float], &[]));
            clear_of(&galley, &[float]);
            let count = text.chars().count();
            for i in 0..=count {
                let rect = galley.pos_from_cursor(CCursor::new(i));
                assert_eq!(galley.cursor_from_pos(rect.center().to_vec2()).index.0, i);
            }
        }
    }

    #[test]
    fn pictures_on_both_sides_too_wide_together_stack() {
        let text = format!("{WORDS}\nX\u{2029}Y\u{2029}{WORDS} {WORDS} {WORDS}");
        let at = |c| text.chars().position(|x| x == c).expect("char");
        let floats = [
            Float {
                index: at('X'),
                atom: 0,
                left: true,
                size: egui::vec2(200.0, 80.0),
            },
            Float {
                index: at('Y'),
                atom: 1,
                left: false,
                size: egui::vec2(200.0, 80.0),
            },
        ];
        let galley = with_fonts(|f| flow(f, job(&text, 400.0), &floats, &[]));
        clear_of(&galley, &floats);
        let rects = placed(&galley, &floats);
        assert!(rects[1].top() >= rects[0].bottom(), "{rects:?}");
    }

    #[test]
    fn a_highlight_beside_the_picture_stays_beside_it() {
        let (galley, float, rect) = flowed(true);
        // Comments and mentions are painted over `row_rects`.
        let range = float.index + 2..float.index + 120;
        let rects = crate::text_marks::row_rects(&galley, &range);
        assert!(rects.len() >= 2, "{rects:?}");
        for r in rects.iter().filter(|r| r.top() < rect.bottom()) {
            assert!(r.left() >= rect.right() + GAP - 0.5, "{r:?}");
        }
    }

    #[test]
    fn a_paragraph_laid_out_in_pieces_matches_egui() {
        let text = format!("{WORDS}\n\n{WORDS} {WORDS}\n");
        let (plain, mine) = with_fonts(|f| {
            let job = job(&text, 300.0);
            let plain = f.layout_job(job.clone());
            let mut flow = super::Flow {
                job: &plain.job,
                column: 300.0,
                floats: &[],
                placed: Vec::new(),
                next: 0,
                unsettled: false,
                rows: Vec::new(),
                y: 0.0,
            };
            let ranges = paragraphs(&text);
            let mut at = 0;
            for (i, range) in ranges.iter().enumerate() {
                flow.paragraph(f, range.clone(), at, i + 1 < ranges.len());
                at += text[range.clone()].chars().count() + 1;
            }
            let rows = flow.rows.clone();
            (plain, rows)
        });
        assert_eq!(plain.rows.len(), mine.len());
        for (a, b) in plain.rows.iter().zip(&mine) {
            assert_eq!(a.pos, b.pos);
            assert_eq!(
                a.char_count_including_newline(),
                b.char_count_including_newline()
            );
            assert_eq!(a.ends_with_newline, b.ends_with_newline);
        }
    }

    #[test]
    fn a_short_text_is_as_tall_as_its_picture() {
        let text = "X\u{2029}Short.";
        let float = picture(text, true);
        let galley = with_fonts(|f| flow(f, job(text, 400.0), &[float], &[]));
        let rect = placed(&galley, &[float])[0];
        assert!(galley.rect.bottom() >= rect.bottom() + BOTTOM - 0.5);
    }

    #[test]
    fn a_cleared_line_starts_below_the_picture() {
        let text = text();
        let float = picture(&text, true);
        let galley = with_fonts(|f| flow(f, job(&text, 400.0), &[float], &[false, false, true]));
        let rect = placed(&galley, &[float])[0];
        // The third paragraph's first row.
        let start = text
            .split('\n')
            .take(2)
            .map(|p| p.chars().count() + 1)
            .sum::<usize>();
        let row = rows(&galley)
            .into_iter()
            .find(|(s, _)| *s == start)
            .expect("row");
        assert!(row.1.top() >= rect.bottom() + BOTTOM - 0.5);
        assert!(row.1.left().abs() < 0.5);
    }
}
