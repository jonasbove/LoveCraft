//! Proofreading in the editor (ProseCraft's Harper integration in `+page.svelte`): the
//! settings, the check run a moment after typing stops, the wavy underlines and the
//! suggestion popover.

use std::collections::HashSet;
use std::ops::Range;

use egui::{RichText, Ui};
use prosecraft_core::proofread::{Lint, Proofreader, editor_range, ignore_key};

use crate::theme::Palette;

/// `harperEnabledStorageKey`, `harperDialectStorageKey`.
pub const ENABLED_KEY: &str = "prosecraft-harper-enabled";
pub const DIALECT_KEY: &str = "prosecraft-harper-dialect";
/// Seconds after the last change before the scene is checked (ProseCraft: 1200 ms).
pub const DELAY: f64 = 1.2;
/// `HARPER_RENDER_LIMIT`.
const RENDER_LIMIT: usize = 200;

/// The Proofreading settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProofreadPrefs {
    pub enabled: bool,
    pub dialect: String,
}

impl Default for ProofreadPrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            dialect: "american".to_owned(),
        }
    }
}

/// A lint shown in the editor: its editor characters, and what Ignore remembers.
#[derive(Clone, Debug)]
pub struct Shown {
    pub range: Range<usize>,
    pub lint: Lint,
    pub key: String,
}

type Request = (String, String, String, Vec<char>, Vec<Option<usize>>);
type Answer = (String, String, Vec<char>, Vec<Option<usize>>, Vec<Lint>);

/// The checks, run away from the interface (on the desktop, a thread that keeps the linter).
pub struct Engine {
    #[cfg(not(target_arch = "wasm32"))]
    to: std::sync::mpsc::Sender<Request>,
    #[cfg(not(target_arch = "wasm32"))]
    from: std::sync::mpsc::Receiver<Answer>,
    #[cfg(target_arch = "wasm32")]
    linter: Option<Proofreader>,
    #[cfg(target_arch = "wasm32")]
    ready: Vec<Answer>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Engine")
    }
}

fn check(linter: &mut Option<Proofreader>, (dialect, document, body, text, at): Request) -> Answer {
    if linter.as_ref().is_none_or(|l| l.dialect() != dialect) {
        *linter = Some(Proofreader::new(&dialect));
    }
    let lints = linter
        .as_mut()
        .map(|l| l.lint(&text.iter().collect::<String>()))
        .unwrap_or_default();
    (document, body, text, at, lints)
}

impl Engine {
    pub fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (to, requests) = std::sync::mpsc::channel::<Request>();
            let (answers, from) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut linter = None;
                while let Ok(mut request) = requests.recv() {
                    // Only the newest request matters.
                    while let Ok(newer) = requests.try_recv() {
                        request = newer;
                    }
                    if answers.send(check(&mut linter, request)).is_err() {
                        break;
                    }
                }
            });
            Self { to, from }
        }
        #[cfg(target_arch = "wasm32")]
        Self {
            linter: None,
            ready: Vec::new(),
        }
    }

    fn ask(&mut self, request: Request) {
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.to.send(request);
        #[cfg(target_arch = "wasm32")]
        {
            let answer = check(&mut self.linter, request);
            self.ready.push(answer);
        }
    }

    fn answer(&mut self) -> Option<Answer> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut last = None;
            while let Ok(answer) = self.from.try_recv() {
                last = Some(answer);
            }
            last
        }
        #[cfg(target_arch = "wasm32")]
        self.ready.pop()
    }
}

/// The proofreading of the open scene.
#[derive(Debug, Default)]
pub struct Proofing {
    engine: Option<Engine>,
    /// What was last asked for: the document and its text.
    asked: Option<(String, String, String)>,
    /// The document and text the lints are for.
    pub document: String,
    pub body: String,
    pub lints: Vec<Shown>,
    pub checking: bool,
    /// `harperIgnored`.
    ignored: HashSet<String>,
    /// The lint whose popover is open.
    pub popover: Option<usize>,
}

impl Proofing {
    /// Clears what is shown (`clearHarperHighlights`).
    pub fn clear(&mut self) {
        self.lints.clear();
        self.asked = None;
        self.document.clear();
        self.body.clear();
        self.checking = false;
        self.popover = None;
    }

    /// `scheduleHarperLint` / `refreshHarperLints`: once the text has rested, checks it, and
    /// takes the answer when it comes. Returns whether a check is under way (to repaint).
    pub fn tick(
        &mut self,
        prefs: &ProofreadPrefs,
        draft: Option<&crate::editing::Draft>,
        now: f64,
    ) -> bool {
        if !prefs.enabled {
            if self.engine.is_some() || !self.lints.is_empty() {
                self.clear();
                self.engine = None;
            }
            return false;
        }
        let engine = self.engine.get_or_insert_with(Engine::new);
        if let Some((document, body, text, at, lints)) = engine.answer() {
            self.checking = false;
            self.lints = lints
                .into_iter()
                .filter_map(|lint| {
                    let key = ignore_key(&lint, &text);
                    let range = editor_range(&lint, &at)?;
                    (!self.ignored.contains(&key)).then_some(Shown { range, lint, key })
                })
                .collect();
            self.document = document;
            self.body = body;
            self.popover = None;
        }
        let Some(draft) = draft.filter(|d| d.markdown.is_none()) else {
            return self.checking;
        };
        let wanted = (
            prefs.dialect.clone(),
            draft.document.clone(),
            draft.body.clone(),
        );
        if self.asked.as_ref() != Some(&wanted) && now - draft.changed_at >= DELAY {
            let (text, at) = draft.doc.proofread_map();
            engine.ask((
                prefs.dialect.clone(),
                draft.document.clone(),
                draft.body.clone(),
                text,
                at,
            ));
            self.asked = Some(wanted);
            self.checking = true;
        }
        self.checking
            || self
                .asked
                .as_ref()
                .is_some_and(|(_, d, b)| *d != draft.document || *b != draft.body)
    }

    /// The lints to draw over this draft (none while its text differs from what was checked).
    pub fn current(&self, draft: &crate::editing::Draft) -> &[Shown] {
        if self.document == draft.document && self.body == draft.body {
            &self.lints[..self.lints.len().min(RENDER_LIMIT)]
        } else {
            &[]
        }
    }

    /// `ignoreHarperLint`.
    pub fn ignore(&mut self, index: usize) {
        if index < self.lints.len() {
            let key = self.lints.remove(index).key;
            self.ignored.insert(key);
        }
        self.popover = None;
    }
}

/// A wavy line under a stretch of text (`underline wavy`).
fn wave(painter: &egui::Painter, left: f32, right: f32, y: f32, color: egui::Color32) {
    let step = 2.0;
    let mut x = left;
    let mut up = true;
    let mut points = vec![egui::pos2(x, y)];
    while x < right {
        x = (x + step).min(right);
        points.push(egui::pos2(x, if up { y - 1.5 } else { y }));
        up = !up;
    }
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.0, color)));
}

/// Draws the lints under the text, row by row.
pub fn paint(
    ui: &Ui,
    lints: &[Shown],
    galley: &egui::Galley,
    origin: egui::Pos2,
    clip: egui::Rect,
) {
    let color = egui::Color32::from_rgba_unmultiplied(0xb4, 0x23, 0x18, 190);
    let painter = ui.painter().with_clip_rect(clip);
    for shown in lints {
        let start = galley.pos_from_cursor(egui::text::CCursor::new(shown.range.start));
        let end = galley.pos_from_cursor(egui::text::CCursor::new(shown.range.end));
        for row in &galley.rows {
            let top = row.rect().top();
            if top + 0.5 < start.top() || top > end.top() + 0.5 {
                continue;
            }
            let left = if (top - start.top()).abs() < 0.5 {
                start.left()
            } else {
                row.rect().left()
            };
            let right = if (top - end.top()).abs() < 0.5 {
                end.left()
            } else {
                row.rect().right()
            };
            if right > left {
                wave(
                    &painter,
                    origin.x + left,
                    origin.x + right,
                    origin.y + row.rect().bottom() - 1.0,
                    color,
                );
            }
        }
    }
}

/// What the popover asks for.
pub enum PopoverAction {
    None,
    Apply(String),
    Ignore(usize),
    Close,
}

/// The suggestion popover (`harper-popover`) under the lint at `at`.
pub fn popover(
    ctx: &egui::Context,
    palette: &Palette,
    index: usize,
    shown: &Shown,
    at: egui::Pos2,
) -> PopoverAction {
    let mut action = PopoverAction::None;
    let area = egui::Area::new(egui::Id::unique("proofreading-popover"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(300.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&shown.lint.kind_pretty).strong());
                    ui.label(format!("· {}", shown.lint.message));
                });
                if shown.lint.suggestions.is_empty() {
                    ui.label(
                        RichText::new("No automatic fix — edit the passage directly.")
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                } else {
                    ui.horizontal_wrapped(|ui| {
                        for suggestion in &shown.lint.suggestions {
                            let label = if suggestion.is_empty() {
                                "(remove)"
                            } else {
                                suggestion
                            };
                            if ui.button(label).clicked() {
                                action = PopoverAction::Apply(suggestion.clone());
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    if ui.button("Ignore").clicked() {
                        action = PopoverAction::Ignore(index);
                    }
                    if ui.button("Close").clicked() {
                        action = PopoverAction::Close;
                    }
                });
            });
        });
    ui_name(ctx, area.response.id);
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        action = PopoverAction::Close;
    }
    action
}

fn ui_name(ctx: &egui::Context, id: egui::Id) {
    ctx.accesskit_node_builder(id, |n| {
        n.set_role(egui::accesskit::Role::Dialog);
        n.set_label("Proofreading suggestion");
    });
}
