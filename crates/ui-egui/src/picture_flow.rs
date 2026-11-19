//! Text beside a wrapped picture: ProseCraft floats a picture whose layout is "wrap left" or
//! "wrap right" (`float: left` / `float: right`), and the lines that follow wrap beside it until
//! they pass its bottom edge.
//!
//! The pictures are placed by the CSS float rules (CSS 2.1 §9.5.1): a picture goes no higher
//! than its character's row or any picture before it, beside the pictures already on its side
//! when it fits there, and otherwise below the nearest of them in its way.
//!
//! egui lays text out at one width per galley, so the rows beside a picture are laid out again:
//! a paragraph that starts beside one (or holds one) is laid out piece by piece, each piece at the
//! width left beside the pictures at its height, and its rows placed in one galley with the rest.
//! A paragraph clear of the pictures keeps the rows egui laid out for the whole text.
//!
//! Right-to-left text: egui resolves each paragraph's direction (UAX #9), orders each row's
//! text, and sets a right-to-left paragraph's rows flush with the right (with its widest row).
//! A piece is laid out in the direction its paragraph has in the whole text (not guessed from
//! the piece), and never runs past a `<br>`, where the bidirectional algorithm starts a new
//! paragraph. Its right-to-left rows end where the room beside a picture on the right does, and
//! elsewhere where the other right-to-left rows end. The pictures stay where `float: left` /
//! `float: right` put them: those are sides of the page, not of the text.
//!
//! Scene text sets every paragraph left-to-right at its base, as ProseCraft does (see
//! `views::SCENE_DIRECTION`); a piece keeps a direction its job sets, and so starts on the left.

use std::ops::Range;
use std::sync::Arc;

use egui::epaint::text::{ByteIndex, Galley, LayoutJob, LayoutSection, PlacedRow, TextDirection};
use egui::{Rect, Vec2, pos2, vec2};

/// The room above a wrapped picture (`margin: 4px 18px 14px 0`).
pub(crate) const TOP: f32 = 4.0;
/// The room between a wrapped picture and the text beside it.
pub(crate) const GAP: f32 = 18.0;
/// The room below a wrapped picture.
pub(crate) const BOTTOM: f32 = 14.0;
/// Text narrower than this beside the pictures goes below them instead.
const MIN_BAND: f32 = 40.0;
/// `<br>`: it ends a paragraph for the bidirectional algorithm (class B), not for egui.
const HARD_BREAK: char = '\u{2029}';

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

/// A picture placed in the column: the space it keeps the text out of (its margin box).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placed {
    left: bool,
    top: f32,
    bottom: f32,
    /// How far its margin box is from its side of the column (past the pictures beside it).
    offset: f32,
    /// How far into the column it reaches from its side, its gap to the text included.
    reach: f32,
}

impl Placed {
    /// Where `float`, its character in the row `row` (its top and bottom), goes after the
    /// pictures `placed` (CSS 2.1 §9.5.1): not above that row or the top of any picture before
    /// it (rules 5 and 6); beside the pictures in its way when it fits between them (rules 2, 3
    /// and 7), else lower, below the first of them to end (rule 8). A picture that can't go
    /// beside its row goes below it, as browsers place a float that doesn't fit on its line.
    fn new(float: &Float, row: (f32, f32), placed: &[Placed], column: f32) -> Self {
        let height = TOP + float.size.y + BOTTOM;
        let width = float.size.x + GAP;
        let mut top = placed.iter().map(|p| p.top).fold(row.0, f32::max);
        let mut offset = 0.0;
        // Each round moves below a picture in the way (or the row), so all of them at most.
        for _ in 0..placed.len() + 2 {
            let (at, fits, below) = room(placed, top, height, float.left, width, column);
            if fits && (top <= row.0 || top >= row.1) {
                offset = at;
                break;
            }
            let next = if fits {
                row.1
            } else {
                below.unwrap_or(top).max(row.1)
            };
            if next <= top {
                break;
            }
            top = next;
        }
        Self {
            left: float.left,
            top,
            bottom: top + height,
            offset,
            reach: offset + width,
        }
    }

    fn image(&self, float: &Float, column: f32) -> Rect {
        let x = if self.left {
            self.offset
        } else {
            column - self.offset - float.size.x
        };
        Rect::from_min_size(pos2(x, self.top + TOP), float.size)
    }
}

/// For a picture `width` wide (with its gap) on side `left`, from `top` down `height`: how far
/// from its side the pictures in the way push it, whether it fits there (or nothing is in the
/// way), and where the first of those pictures ends.
fn room(
    placed: &[Placed],
    top: f32,
    height: f32,
    left: bool,
    width: f32,
    column: f32,
) -> (f32, bool, Option<f32>) {
    let (mut on_left, mut on_right) = (0.0_f32, 0.0_f32);
    let mut below: Option<f32> = None;
    for p in placed
        .iter()
        .filter(|p| p.top < top + height && top < p.bottom)
    {
        if p.left {
            on_left = on_left.max(p.reach);
        } else {
            on_right = on_right.max(p.reach);
        }
        below = Some(below.map_or(p.bottom, |b| b.min(p.bottom)));
    }
    let fits = below.is_none() || on_left + width + on_right <= column + 0.01;
    (if left { on_left } else { on_right }, fits, below)
}

/// Each wrapped picture's rectangle in `galley` (laid out by [`flow`]), in its coordinates.
pub(crate) fn placed(galley: &Galley, floats: &[Float]) -> Vec<Rect> {
    let column = galley.job.wrap.max_width;
    let mut placed: Vec<Placed> = Vec::new();
    floats
        .iter()
        .map(|float| {
            let p = Placed::new(float, row_of(galley, float.index), &placed, column);
            placed.push(p);
            p.image(float, column)
        })
        .collect()
}

/// The top and bottom of the row holding character `index`.
fn row_of(galley: &Galley, index: usize) -> (f32, f32) {
    let mut start = 0;
    for placed in &galley.rows {
        let count = placed.char_count_including_newline().0;
        if index < start + count {
            return (placed.min_y(), placed.max_y());
        }
        start += count;
    }
    let end = galley.rows.last().map_or(0.0, PlacedRow::max_y);
    (end, end)
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

/// The part `range` of `job`, laid out `width` wide, as egui lays out a paragraph, in the
/// direction its paragraph has in the whole text (`rtl`).
fn slice(job: &LayoutJob, range: Range<usize>, width: f32, rtl: bool) -> LayoutJob {
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
        // A piece starting in the middle of a paragraph would otherwise take the direction of
        // its own first letter. A direction the job sets for every paragraph stays.
        direction: match job.direction {
            TextDirection::Auto if rtl => TextDirection::RightToLeft,
            TextDirection::Auto => TextDirection::LeftToRight,
            fixed => fixed,
        },
        ..job.clone()
    }
}

/// The room left beside the pictures for a row.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Band {
    left: f32,
    right: f32,
    /// Where the first picture beside the row ends (the room grows there).
    ends: Option<f32>,
    /// Where the next picture below the row's top starts (no row may reach past it).
    starts: Option<f32>,
}

/// The room left beside the pictures for a row from `y` down `height`.
fn band(placed: &[Placed], y: f32, height: f32, column: f32) -> Band {
    let mut band = Band {
        left: 0.0,
        right: column,
        ends: None,
        starts: None,
    };
    let min = |a: Option<f32>, v: f32| Some(a.map_or(v, |c| c.min(v)));
    let height = height.max(0.01);
    for p in placed {
        if p.top < y + height && y < p.bottom {
            if p.left {
                band.left = band.left.max(p.reach);
            } else {
                band.right = band.right.min(column - p.reach);
            }
            band.ends = min(band.ends, p.bottom);
        } else if p.top >= y + height {
            band.starts = min(band.starts, p.top);
        }
    }
    band
}

/// Is `row` set flush with the right, its paragraph being right-to-left (see
/// `LayoutJob::halign_min_is_start`)?
fn starts_on_the_right(job: &LayoutJob, row: &PlacedRow) -> bool {
    job.halign == egui::Align::Min
        && job.halign_min_is_start
        && row.rtl_paragraph
        && !row.glyphs.is_empty()
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
    let mut flow = Flow::new(&full, floats);
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
    rows: Vec<PlacedRow>,
    /// Where the next row goes.
    y: f32,
    /// The first character of each row of the whole text, and whether its paragraph is
    /// right-to-left: set flush with the right. egui decides that once for each paragraph
    /// (between `\n`s), by where it starts.
    directions: Vec<(usize, bool)>,
    /// Each character's bidirectional embedding level in the whole text (a `\n`'s is 0).
    levels: Vec<u8>,
    /// Whether the rows being placed are set flush with the right (see `directions`).
    align_right: bool,
    /// Where the right-to-left rows of the whole text end (egui sets them flush with its
    /// widest row).
    rtl_end: f32,
    /// The rows set flush with the right without a picture on their right.
    flush: Vec<usize>,
}

impl<'a> Flow<'a> {
    fn new(full: &'a Galley, floats: &'a [Float]) -> Self {
        let mut at = 0;
        let directions = full
            .rows
            .iter()
            .map(|row| {
                let start = at;
                at += row.char_count_including_newline().0;
                (start, row.rtl_paragraph)
            })
            .collect();
        let levels = full
            .rows
            .iter()
            .flat_map(|row| {
                let newline = row.char_count_including_newline().0 > row.glyphs.len();
                row.glyphs
                    .iter()
                    .map(|g| g.bidi_level)
                    .chain(newline.then_some(0))
            })
            .collect();
        Self {
            job: &full.job,
            column: full.job.wrap.max_width,
            floats,
            placed: Vec::new(),
            next: 0,
            rows: Vec::new(),
            y: 0.0,
            directions,
            levels,
            align_right: false,
            flush: Vec::new(),
            rtl_end: full
                .rows
                .iter()
                .map(|r| r.pos.x + r.size.x)
                .fold(0.0, f32::max),
        }
    }

    /// Is a picture beside (or below) where the next row goes?
    fn beside(&self) -> bool {
        self.placed.iter().any(|p| p.bottom > self.y)
    }

    /// The next row starts below every picture.
    fn clear(&mut self) {
        let bottom = self.placed.iter().map(|p| p.bottom).fold(self.y, f32::max);
        self.y = bottom;
    }

    /// Is the paragraph holding character `index` set flush with the right?
    fn rtl_at(&self, index: usize) -> bool {
        let i = self
            .directions
            .partition_point(|(start, _)| *start <= index)
            .saturating_sub(1);
        self.directions.get(i).is_some_and(|(_, rtl)| *rtl)
    }

    /// Is the paragraph of the bidirectional algorithm (between `\n`s and `<br>`s) holding
    /// byte `start`, character `index`, right-to-left? Every character of a right-to-left
    /// paragraph has an odd level or one above it; a left-to-right one has a level 0.
    fn bidi_rtl(&self, start: usize, index: usize) -> bool {
        let text = &self.job.text;
        let ends = ['\n', HARD_BREAK];
        let from = text
            .get(..start)
            .and_then(|before| before.char_indices().rfind(|(_, c)| ends.contains(c)))
            .map_or(0, |(i, c)| i + c.len_utf8());
        let to = text
            .get(start..)
            .and_then(|rest| rest.find(ends))
            .map_or(text.len(), |i| start + i);
        let before = text.get(from..start).map_or(0, |t| t.chars().count());
        let count = text.get(from..to).map_or(0, |t| t.chars().count());
        let first = index.saturating_sub(before);
        self.levels
            .get(first..first + count)
            .and_then(|levels| levels.iter().min())
            .is_some_and(|min| *min >= 1)
    }

    /// Where the piece starting at byte `start` ends: after the next `<br>` (where a
    /// paragraph of the bidirectional algorithm ends, a row always does), else at `end`.
    fn piece_end(&self, start: usize, end: usize) -> usize {
        self.job
            .text
            .get(start..end)
            .and_then(|rest| rest.find(HARD_BREAK))
            .map_or(end, |i| start + i + HARD_BREAK.len_utf8())
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
            if starts_on_the_right(self.job, &row) {
                self.flush.push(self.rows.len());
            }
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
        let mut finished = false;
        // Each round places at least a row or a picture (a picture moves only when its row
        // does); the bound guards against a loop.
        for _ in 0..(range.len() + 4) * 2 + self.floats.len() * 8 {
            let end = self.piece_end(start, range.end);
            self.align_right = self.rtl_at(char_at);
            let rtl = self.bidi_rtl(start, char_at);
            let (band, piece) = self.lay_out(fonts, start..end, rtl);
            if band.right - band.left < MIN_BAND
                && let Some(below) = band.ends
            {
                self.y = below;
                continue;
            }
            let step = self.take(&piece, band, char_at);
            char_at += step.chars;
            start = self.advance(start, step.chars).min(range.end);
            // All of it placed (unless a `\n` ending the text leaves its empty row to come).
            let ended = start >= range.end
                && step.chars > 0
                && !self.rows.last().is_some_and(|r| r.ends_with_newline);
            if (step.done && end >= range.end) || ended {
                finished = true;
                break;
            }
        }
        if !finished {
            // Never left out: the rest at the column's width.
            let rest = fonts.layout_job(slice(
                self.job,
                start..range.end,
                self.column,
                self.bidi_rtl(start, char_at),
            ));
            self.align_right = self.rtl_at(char_at);
            let top = self.y;
            let band = band(&[], top, 0.0, self.column);
            for row in &rest.rows {
                self.place_row(row, band, top + row.min_y());
            }
        }
        if let Some(row) = self.rows.last_mut() {
            row.ends_with_newline = newline;
        }
    }

    /// The piece at bytes `range` laid out in the room beside the pictures for its first row,
    /// and that room: the row's height decides which pictures it is beside.
    fn lay_out(
        &self,
        fonts: &mut egui::epaint::FontsView<'_>,
        range: Range<usize>,
        rtl: bool,
    ) -> (Band, Arc<Galley>) {
        let mut height = 0.0;
        let mut band = band(&self.placed, self.y, height, self.column);
        let width = |band: Band| (band.right - band.left).max(MIN_BAND);
        let mut piece = fonts.layout_job(slice(self.job, range.clone(), width(band), rtl));
        // The row grows or the room shrinks: settled within a few rounds.
        for _ in 0..4 {
            let first = piece.rows.first().map_or(0.0, |r| r.max_y() - r.min_y());
            if first <= height {
                break;
            }
            height = first;
            let wider = band;
            band = self::band(&self.placed, self.y, height, self.column);
            if band.left == wider.left && band.right == wider.right {
                break;
            }
            piece = fonts.layout_job(slice(self.job, range.clone(), width(band), rtl));
        }
        (band, piece)
    }

    /// The byte `chars` characters after byte `start`.
    fn advance(&self, start: usize, chars: usize) -> usize {
        self.job
            .text
            .get(start..)
            .and_then(|rest| rest.char_indices().nth(chars).map(|(i, _)| start + i))
            .unwrap_or(self.job.text.len())
    }

    /// Takes the rows of `piece` (laid out from character `char_at`) that fit where they go:
    /// up to a picture not yet placed (which is placed then), or the height where the room
    /// beside the pictures changes.
    fn take(&mut self, piece: &Galley, band: Band, char_at: usize) -> Step {
        let mut taken = 0;
        let origin = piece.rows.first().map_or(0.0, PlacedRow::min_y);
        let base = self.y;
        let stop = |taken| Step {
            chars: taken,
            done: false,
        };
        for (i, row) in piece.rows.iter().enumerate() {
            let top = base + row.min_y() - origin;
            let count = row.char_count_including_newline().0;
            let start = char_at + taken;
            let chars = start..start + count.max(1);
            let unplaced = self
                .floats
                .get(self.next)
                .is_some_and(|f| f.index < chars.end);
            let bottom = base + row.max_y() - origin;
            let room_changes = band.ends.is_some_and(|c| top >= c)
                || band.starts.is_some_and(|c| bottom > c + 0.01);
            if (i > 0 && (room_changes || unplaced)) || self.moved(chars.clone(), (top, bottom)) {
                return stop(taken);
            }
            match self.place(&chars, (top, bottom)) {
                // The room beside this row changed: lay it out again.
                Some(true) => return stop(taken),
                // Placed lower: the room changes where it starts.
                Some(false) => {
                    self.place_row(row, band, top);
                    return stop(taken + count);
                }
                None => {}
            }
            self.place_row(row, band, top);
            taken += count;
        }
        Step {
            chars: taken,
            done: true,
        }
    }

    /// Places the pictures whose characters are `chars`, in the row at `top`: `Some(true)` when
    /// one goes beside the row, `Some(false)` when they all go lower, `None` without any.
    fn place(&mut self, chars: &Range<usize>, row: (f32, f32)) -> Option<bool> {
        let mut any = None;
        while let Some(float) = self.floats.get(self.next).copied()
            && float.index < chars.end
        {
            let p = Placed::new(&float, row, &self.placed, self.column);
            self.placed.push(p);
            self.next += 1;
            if p.top <= row.0 {
                return Some(true);
            }
            any = Some(false);
        }
        any
    }

    /// Whether a picture placed already, its character in the row at `chars` now going at
    /// `top` (laid out again beside a picture, its character went to a lower row), moved down
    /// to that row: a picture's top is its character's row's. The pictures after it are placed
    /// again.
    fn moved(&mut self, chars: Range<usize>, row: (f32, f32)) -> bool {
        for i in 0..self.next {
            let Some(float) = self.floats.get(i) else {
                return false;
            };
            if !chars.contains(&float.index) {
                continue;
            }
            let p = Placed::new(
                float,
                row,
                self.placed.get(..i).unwrap_or_default(),
                self.column,
            );
            let same = self
                .placed
                .get(i)
                .is_some_and(|q| (q.top - p.top).abs() < 0.5 && (q.offset - p.offset).abs() < 0.5);
            if same {
                continue;
            }
            self.placed.truncate(i);
            self.placed.push(p);
            self.next = i + 1;
            return true;
        }
        false
    }

    /// Puts `row` of a piece laid out in `band` at `top`, as part of a paragraph set as
    /// [`Self::align_right`] says. A row set flush with the right ends where the room does
    /// beside a picture on the right; elsewhere where every such row ends, as egui sets them
    /// (see [`Self::finish`]). The others start where the room does.
    fn place_row(&mut self, row: &PlacedRow, band: Band, top: f32) {
        let job = self.job;
        let right = job.halign == egui::Align::Min
            && job.halign_min_is_start
            && self.align_right
            && !row.glyphs.is_empty();
        let free = right && band.right >= self.column - 0.5;
        let x = if free {
            self.rtl_end.max(band.left + row.size.x) - row.size.x
        } else if right {
            band.right - row.size.x
        } else if starts_on_the_right(job, row) {
            // Laid out right-to-left but set on the left, as egui sets the whole paragraph.
            band.left
        } else {
            row.pos.x + band.left
        };
        let mut row = row.clone();
        row.pos = pos2(x, top);
        if row.rtl_paragraph != self.align_right {
            Arc::make_mut(&mut row.row).rtl_paragraph = self.align_right;
        }
        if free {
            self.flush.push(self.rows.len());
        }
        self.y = row.max_y();
        self.rows.push(row);
    }

    /// The galley: `full`'s, with these rows, as tall as the pictures in it. The rows set flush
    /// with the right without a picture there end together, at the widest of them (or of the
    /// whole text's), as egui sets right-to-left rows.
    fn finish(mut self, full: &Galley) -> Galley {
        let end = self
            .flush
            .iter()
            .filter_map(|i| self.rows.get(*i))
            .map(|r| r.pos.x + r.size.x)
            .fold(self.rtl_end, f32::max)
            .min(self.column);
        for i in &self.flush {
            if let Some(row) = self.rows.get_mut(*i) {
                row.pos.x = end - row.size.x;
            }
        }
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
    /// Right-to-left words (Hebrew), as long as a few rows.
    const HEBREW: &str = "הגאות עלתה על הבוץ בזמן שהפנסים נדלקו לאורך חומת הנמל, אחד אחד, \
        עד שכל העיר הוארה והסירות שכבו בשקט.";

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
        for text in [
            format!("{WORDS}\n\n{WORDS} {WORDS}\n"),
            // Right-to-left paragraphs, one beginning with a left-to-right word, and a `<br>`
            // between paragraphs of either direction.
            format!(
                "{HEBREW}\nharbour {HEBREW} {HEBREW}\u{2029}{WORDS}\u{2029}{HEBREW}\n\n{WORDS}"
            ),
        ] {
            let (plain, mine) = with_fonts(|f| {
                let job = job(&text, 300.0);
                let plain = f.layout_job(job.clone());
                let mut flow = super::Flow::new(&plain, &[]);
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
                assert!((a.pos - b.pos).length() < 0.5, "{:?} {:?}", a.pos, b.pos);
                assert_eq!(a.rtl_paragraph, b.rtl_paragraph);
                assert_eq!(
                    a.char_count_including_newline(),
                    b.char_count_including_newline()
                );
                assert_eq!(a.ends_with_newline, b.ends_with_newline);
            }
            assert!(plain.rows.iter().any(|r| r.rtl_paragraph) == text.contains(HEBREW));
        }
    }

    /// Pictures at the characters `marks` of `text` (in that order), each wrap left or right
    /// and its size.
    fn floats_at(text: &str, marks: &[(char, bool, (f32, f32))]) -> Vec<Float> {
        marks
            .iter()
            .enumerate()
            .map(|(atom, (mark, left, (w, h)))| Float {
                index: text.chars().position(|c| c == *mark).expect("mark"),
                atom,
                left: *left,
                size: egui::vec2(*w, *h),
            })
            .collect()
    }

    /// Lays `text` out 400 wide beside `floats`, checks no row runs into them and every caret
    /// position maps back, and returns the galley and the pictures' rectangles.
    fn checked(text: &str, floats: &[Float]) -> (Arc<Galley>, Vec<Rect>) {
        let galley = with_fonts(|f| flow(f, job(text, 400.0), floats, &[]));
        clear_of(&galley, floats);
        let count = text.chars().count();
        let total: usize = galley
            .rows
            .iter()
            .map(|r| r.char_count_including_newline().0)
            .sum();
        assert_eq!(total, count);
        for i in 0..=count {
            let rect = galley.pos_from_cursor(CCursor::new(i));
            assert_eq!(galley.cursor_from_pos(rect.center().to_vec2()).index.0, i);
        }
        let rects = placed(&galley, floats);
        (galley, rects)
    }

    /// The rows after character `after` whose tops are above `bottom`.
    fn rows_beside(galley: &Galley, after: usize, bottom: f32) -> Vec<Rect> {
        rows(galley)
            .into_iter()
            .filter(|(start, r)| *start > after && r.top() < bottom && r.width() > 2.0)
            .map(|(_, r)| r)
            .collect()
    }

    #[test]
    fn small_pictures_on_one_side_sit_side_by_side() {
        // As the editor inserts two pictures (each followed by a `<br>`), and both in one row.
        for text in [
            format!("{WORDS}\nX\u{2029}Y\u{2029}{WORDS} {WORDS} {WORDS}"),
            format!("{WORDS}\nXY\u{2029}{WORDS} {WORDS} {WORDS}"),
        ] {
            for left in [true, false] {
                let floats = floats_at(
                    &text,
                    &[('X', left, (80.0, 90.0)), ('Y', left, (80.0, 60.0))],
                );
                let (galley, rects) = checked(&text, &floats);
                let (x, y) = (rects[0], rects[1]);
                if left {
                    assert!((x.left()).abs() < 0.5, "{rects:?}");
                    assert!((y.left() - (x.right() + GAP)).abs() < 0.5, "{rects:?}");
                } else {
                    assert!((x.right() - 400.0).abs() < 0.5, "{rects:?}");
                    assert!((y.right() - (x.left() - GAP)).abs() < 0.5, "{rects:?}");
                }
                // Level with it, or a row lower where its character is.
                assert!(
                    y.top() >= x.top() - 0.5 && y.top() < x.bottom(),
                    "{rects:?}"
                );
                let after = floats[1].index;
                let beside = rows_beside(&galley, after, y.bottom());
                assert!(!beside.is_empty());
                for row in beside {
                    if left {
                        assert!(row.left() >= y.right() + GAP - 0.5, "{row:?} {rects:?}");
                    } else {
                        assert!(row.right() <= y.left() - GAP + 0.5, "{row:?} {rects:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn half_width_pictures_on_one_side_stack() {
        // Two 50% pictures with their 18px margins don't fit in the column together.
        let text = format!("{WORDS}\nXY\u{2029}{WORDS} {WORDS} {WORDS} {WORDS}");
        for left in [true, false] {
            let floats = floats_at(
                &text,
                &[('X', left, (200.0, 80.0)), ('Y', left, (200.0, 80.0))],
            );
            let (_, rects) = checked(&text, &floats);
            assert!(
                (rects[1].top() - (rects[0].bottom() + BOTTOM + TOP)).abs() < 0.5,
                "{rects:?}"
            );
            assert!((rects[1].left() - rects[0].left()).abs() < 0.5, "{rects:?}");
        }
    }

    #[test]
    fn small_pictures_on_both_sides_sit_level() {
        let text = format!("{WORDS}\nXY\u{2029}{WORDS} {WORDS} {WORDS}");
        let floats = floats_at(
            &text,
            &[('X', true, (100.0, 90.0)), ('Y', false, (100.0, 90.0))],
        );
        let (galley, rects) = checked(&text, &floats);
        assert!((rects[0].top() - rects[1].top()).abs() < 0.5, "{rects:?}");
        assert!(rects[0].left().abs() < 0.5 && (rects[1].right() - 400.0).abs() < 0.5);
        let beside = rows_beside(&galley, floats[1].index, rects[0].bottom());
        assert!(beside.len() >= 2, "{beside:?}");
        for row in beside {
            assert!(row.left() >= rects[0].right() + GAP - 0.5, "{row:?}");
            assert!(row.right() <= rects[1].left() - GAP + 0.5, "{row:?}");
        }
    }

    #[test]
    fn a_picture_that_no_longer_fits_goes_below_the_first_to_end() {
        // A tall picture on the left, a short one on the right, and a wide one on the left
        // that fits beside the tall one only once the short one has ended.
        let text = format!("{WORDS}\nXYZ\u{2029}{WORDS} {WORDS} {WORDS} {WORDS} {WORDS}");
        let floats = floats_at(
            &text,
            &[
                ('X', true, (100.0, 200.0)),
                ('Y', false, (100.0, 40.0)),
                ('Z', true, (220.0, 50.0)),
            ],
        );
        let (_, rects) = checked(&text, &floats);
        let (x, y, z) = (rects[0], rects[1], rects[2]);
        assert!((x.top() - y.top()).abs() < 0.5, "{rects:?}");
        assert!(
            (z.top() - (y.bottom() + BOTTOM + TOP)).abs() < 0.5,
            "{rects:?}"
        );
        assert!((z.left() - (x.right() + GAP)).abs() < 0.5, "{rects:?}");
        assert!(z.top() < x.bottom());
    }

    #[test]
    fn a_picture_between_words_keeps_the_ones_before_it_in_place() {
        // Pictures in the middle of a row of words: each where its character's row is.
        for left in [true, false] {
            let text =
                format!("{WORDS}\nThe lamps X along the harbour Y wall went on. {WORDS} {WORDS}");
            let floats = floats_at(
                &text,
                &[('X', left, (70.0, 60.0)), ('Y', left, (70.0, 60.0))],
            );
            let (galley, rects) = checked(&text, &floats);
            for (rect, float) in rects.iter().zip(&floats) {
                let (_, row) = rows(&galley)
                    .into_iter()
                    .rfind(|(start, _)| *start <= float.index)
                    .expect("its row");
                assert!(rect.top() - TOP >= row.top() - 0.5, "{rect:?} {row:?}");
            }
        }
    }

    #[test]
    fn right_to_left_text_beside_a_picture_ends_at_the_room_right_edge() {
        for left in [true, false] {
            // The picture in a paragraph of its own; right-to-left ones after it.
            let text = format!("{WORDS}\nX\n{HEBREW} {HEBREW} {HEBREW}\n{HEBREW}");
            let floats = floats_at(&text, &[('X', left, (150.0, 120.0))]);
            let (galley, rects) = checked(&text, &floats);
            let rect = rects[0];
            let bottom = rect.bottom() + BOTTOM;
            let hebrew: Vec<_> = galley
                .rows
                .iter()
                .filter(|r| r.rtl_paragraph && r.rect().width() > 2.0)
                .map(|r| r.rect())
                .collect();
            let beside: Vec<_> = hebrew.iter().filter(|r| r.top() < bottom).collect();
            let below: Vec<_> = hebrew.iter().filter(|r| r.top() >= bottom).collect();
            assert!(beside.len() >= 3 && !below.is_empty(), "{hebrew:?}");
            // Below the picture, the rows end where egui ends right-to-left rows.
            let end = below[0].right();
            assert!(end > 380.0, "{below:?}");
            for row in &below {
                assert!((row.right() - end).abs() < 1.0, "{row:?}");
            }
            for row in &beside {
                if left {
                    // Flush right, clear of the picture on the left.
                    assert!((row.right() - end).abs() < 1.0, "{row:?}");
                    assert!(row.left() >= rect.right() + GAP - 0.5, "{row:?}");
                } else {
                    // Flush with the picture's gap.
                    assert!(
                        (row.right() - (rect.left() - GAP)).abs() < 1.0,
                        "{row:?} {rect:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn short_right_to_left_rows_beside_a_picture_end_together() {
        // egui sets right-to-left rows flush with the widest of them: the rows beside the
        // picture and the one below it end together, clear of the picture.
        let long = "הגאות עלתה על הבוץ בזמן שהפנסים נדלקו לאורך";
        let text = format!("X\nשלום לכולם\nשורה שנייה\n{long}\n{long}\n{long}\nסוף");
        let floats = floats_at(&text, &[('X', true, (150.0, 100.0))]);
        let (galley, rects) = checked(&text, &floats);
        let hebrew: Vec<Rect> = galley
            .rows
            .iter()
            .filter(|r| r.rtl_paragraph && r.rect().width() > 2.0)
            .map(|r| r.rect())
            .collect();
        let end = hebrew.iter().map(|r| r.right()).fold(0.0, f32::max);
        assert!(hebrew.len() >= 5, "{hebrew:?}");
        for row in &hebrew {
            assert!((row.right() - end).abs() < 1.0, "{row:?} {hebrew:?}");
            if row.top() < rects[0].bottom() + BOTTOM {
                assert!(row.left() >= rects[0].right() + GAP - 0.5, "{row:?}");
            }
        }
        assert!(hebrew.iter().any(|r| r.top() >= rects[0].bottom() + BOTTOM));
    }

    #[test]
    fn right_to_left_words_after_a_picture_in_their_paragraph_are_set_as_egui_sets_them() {
        // A paragraph starting with a picture (a left-to-right character to egui) is set on
        // the left, its right-to-left words after the `<br>` ordered right-to-left all the same.
        let text = format!("X\u{2029}{HEBREW} {HEBREW}\n{WORDS}");
        let plain = with_fonts(|f| f.layout_job(job(&text, 400.0)));
        for left in [true, false] {
            let floats = floats_at(&text, &[('X', left, (150.0, 120.0))]);
            let (galley, rects) = checked(&text, &floats);
            let beside = rows_beside(&galley, 1, rects[0].bottom());
            assert!(beside.len() >= 3, "{beside:?}");
            for row in &beside {
                let start = if left { rects[0].right() + GAP } else { 0.0 };
                assert!((row.left() - start).abs() < 0.5, "{row:?} {rects:?}");
            }
            for (a, b) in plain.rows.iter().zip(&galley.rows) {
                assert_eq!(a.rtl_paragraph, b.rtl_paragraph);
            }
            // The words' own order: the first word on the right of its row.
            let row = galley
                .rows
                .iter()
                .find(|r| r.glyphs.len() > 10)
                .expect("a row of words");
            let first = row.glyphs.first().expect("a glyph");
            let last = row
                .glyphs
                .iter()
                .rfind(|g| !g.chr.is_whitespace())
                .expect("a glyph");
            assert!(first.pos.x > last.pos.x, "{} {}", first.pos.x, last.pos.x);
        }
    }

    #[test]
    fn a_left_to_right_base_keeps_right_to_left_text_on_the_left_beside_a_picture() {
        // The editor lays every paragraph out left-to-right at its base (as ProseCraft, which
        // sets no `dir`): rows beside a picture start at the room's left edge, and the space
        // where a row of Hebrew wraps stays at the row's right end (UAX #9 L1 at that base).
        let only_letters = ["הגאות עלתה על הבוץ בזמן שהפנסים נדלקו לאורך חומת הנמל"; 6].join(" ");
        for left in [true, false] {
            let text = format!("{WORDS}\nX\n{only_letters}\n{HEBREW} {HEBREW}");
            let floats = floats_at(&text, &[('X', left, (150.0, 120.0))]);
            let mut ltr = job(&text, 400.0);
            ltr.direction = egui::epaint::text::TextDirection::LeftToRight;
            let galley = with_fonts(|f| flow(f, ltr, &floats, &[]));
            clear_of(&galley, &floats);
            let rect = placed(&galley, &floats)[0];
            let start = if left { rect.right() + GAP } else { 0.0 };
            let bottom = rect.bottom() + BOTTOM;
            let (mut beside, mut spaces) = (0, 0);
            let mut at = 0;
            for r in &galley.rows {
                let after = at > floats[0].index + 1;
                at += r.char_count_including_newline().0;
                assert!(!r.rtl_paragraph);
                let row = r.rect();
                if !after || row.width() < 2.0 {
                    continue;
                }
                let edge = if row.top() < bottom { start } else { 0.0 };
                beside += usize::from(row.top() < bottom);
                assert!((row.left() - edge).abs() < 0.5, "{row:?} {rect:?}");
                if let Some(space) = r.glyphs.last().filter(|g| g.chr == ' ') {
                    let rightmost = r.glyphs.iter().map(|g| g.pos.x).fold(f32::MIN, f32::max);
                    assert!(
                        (space.pos.x - rightmost).abs() < 0.5,
                        "{} {rightmost}",
                        space.pos.x
                    );
                    spaces += 1;
                }
            }
            assert!(beside >= 3 && spaces >= 3, "{beside} {spaces}");
            let count = text.chars().count();
            for i in 0..=count {
                let rect = galley.pos_from_cursor(CCursor::new(i));
                assert!(galley.cursor_from_pos(rect.center().to_vec2()).index.0 <= count);
            }
        }
    }

    #[test]
    fn mixed_direction_text_beside_pictures_does_not_panic() {
        // Left-to-right words in right-to-left rows, and the other way, beside pictures on
        // both sides: every caret position stays in the text.
        let text = format!(
            "{HEBREW} harbour wall {HEBREW}\nXY\u{2029}lamps {HEBREW} on {WORDS} {HEBREW}\n{HEBREW}"
        );
        let floats = floats_at(
            &text,
            &[('X', true, (90.0, 140.0)), ('Y', false, (90.0, 70.0))],
        );
        let galley = with_fonts(|f| flow(f, job(&text, 400.0), &floats, &[]));
        clear_of(&galley, &floats);
        let count = text.chars().count();
        assert_eq!(galley.text(), text);
        for i in 0..=count {
            let rect = galley.pos_from_cursor(CCursor::new(i));
            assert!(rect.is_finite());
            assert!(galley.cursor_from_pos(rect.center().to_vec2()).index.0 <= count);
        }
        for y in (0..galley.rect.height() as i32).step_by(7) {
            for x in (0..400).step_by(13) {
                let c = galley.cursor_from_pos(egui::vec2(x as f32, y as f32));
                assert!(c.index.0 <= count);
            }
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
