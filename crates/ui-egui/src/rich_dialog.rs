//! The equation, diagram and calculation editor (ProseCraft's `richDialog`): its source, a
//! live preview, Insert / Update, Remove and Cancel.

use egui::RichText;
use prosecraft_core::rich::{RichKind, placeholder_html, source_limit};

use crate::editing::Draft;
use crate::theme::Palette;

/// `RichDialogKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    Math,
    Diagram,
    Calc,
}

/// The open editor.
#[derive(Clone, Debug, PartialEq)]
pub struct RichDialog {
    pub document: String,
    pub kind: DialogKind,
    /// A block equation (else inline).
    pub block: bool,
    pub source: String,
    pub error: String,
    /// The atom being edited, else where to insert.
    pub target: Option<usize>,
    pub caret: usize,
}

impl RichDialog {
    /// `openRichDialog` for a new one at the caret.
    pub fn insert(document: &str, kind: DialogKind, caret: usize) -> Self {
        Self {
            document: document.to_owned(),
            kind,
            block: false,
            source: String::new(),
            error: String::new(),
            target: None,
            caret,
        }
    }

    /// `openRichDialog` on a placeholder: atom `index`, from its HTML.
    pub fn edit(document: &str, index: usize, html: &str) -> Option<Self> {
        let nodes = prosecraft_core::html::parse_fragment(html);
        let Some(prosecraft_core::html::Node::Element(element)) = nodes.first() else {
            return None;
        };
        let kind = RichKind::of(element)?;
        let source = prosecraft_core::rich::read_placeholder(element)
            .map(|p| p.1)
            .unwrap_or_default();
        Some(Self {
            document: document.to_owned(),
            kind: match kind {
                RichKind::Diagram => DialogKind::Diagram,
                RichKind::Calc => DialogKind::Calc,
                _ => DialogKind::Math,
            },
            block: kind == RichKind::MathBlock,
            source,
            error: String::new(),
            target: Some(index),
            caret: 0,
        })
    }

    /// `richKindForDialog`.
    fn rich_kind(&self) -> RichKind {
        match self.kind {
            DialogKind::Diagram => RichKind::Diagram,
            DialogKind::Calc => RichKind::Calc,
            DialogKind::Math if self.block => RichKind::MathBlock,
            DialogKind::Math => RichKind::MathInline,
        }
    }

    fn noun(&self) -> &'static str {
        match self.kind {
            DialogKind::Diagram => "diagram",
            DialogKind::Calc => "calculation",
            DialogKind::Math => "equation",
        }
    }
}

/// What the editor asks for.
enum Action {
    None,
    Save,
    Remove,
    Close,
}

/// `saveRichDialog`, `removeRichTarget`: the draft changed. Returns the caret after it.
fn apply(dialog: &mut RichDialog, draft: &mut Draft, action: &Action) -> Option<usize> {
    if let Action::Remove = action {
        let target = dialog.target?;
        let (_, at) = draft.doc.remove_atom(target);
        draft.body = draft.doc.text.clone();
        return at;
    }
    let source = dialog.source.trim().to_owned();
    if source.is_empty() {
        dialog.error = match dialog.kind {
            DialogKind::Diagram => "Add a Mermaid definition first.",
            DialogKind::Calc => "Add a calculation first.",
            DialogKind::Math => "Add some LaTeX first.",
        }
        .to_owned();
        return None;
    }
    let kind = dialog.rich_kind();
    let limit = source_limit(kind);
    if source.encode_utf16().count() > limit {
        dialog.error = format!("Keep the source to {} characters or fewer.", group(limit));
        return None;
    }
    dialog.error.clear();
    let html = placeholder_html(kind, &source);
    if let Some(target) = dialog.target {
        draft.body = draft.doc.replace_atom(target, &html);
        return draft.caret;
    }
    if kind == RichKind::MathInline {
        // The equation, then a space, the caret after it.
        draft.doc.insert_atom(dialog.caret, &html);
        let chars: Vec<char> = draft.doc.text.chars().collect();
        let at = (dialog.caret + 1).min(chars.len());
        let text: String = chars[..at]
            .iter()
            .chain([' '].iter())
            .chain(&chars[at..])
            .collect();
        draft.body = draft.doc.apply_text_at(&text, Some(at + 1));
        return Some(at + 1);
    }
    let (body, caret) = draft.doc.insert_block(dialog.caret, &html);
    draft.body = body;
    Some(caret)
}

/// `toLocaleString` for the limits (8,000).
fn group(n: usize) -> String {
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

/// Draws the editor; returns whether it stays open, and the caret when the draft changed.
pub fn show(
    ctx: &egui::Context,
    palette: &Palette,
    dialog: &mut RichDialog,
    draft: &mut Draft,
    editable: bool,
) -> (bool, Option<usize>) {
    let noun = dialog.noun();
    let title = format!(
        "{} {noun}",
        if dialog.target.is_some() {
            "Edit"
        } else {
            "Insert"
        }
    );
    let mut action = Action::None;
    let mut open = true;
    egui::Window::new(&title)
        .id(egui::Id::unique("rich-dialog"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(560.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(RichText::new(noun.to_uppercase()).size(10.0).color(palette.text_muted));
            let (label, hint, help, rows) = match dialog.kind {
                DialogKind::Math => {
                    ui.horizontal(|ui| {
                        for (block, name) in [(false, "Inline"), (true, "Block")] {
                            if ui.selectable_label(dialog.block == block, name).clicked() {
                                dialog.block = block;
                            }
                        }
                    });
                    ("LaTeX", "E = mc^2", "Inline equations sit in a sentence; block equations centre on their own line.", 5)
                }
                DialogKind::Diagram => (
                    "Mermaid",
                    "graph TD\n  A[Start] --> B[Finish]",
                    "Use any Mermaid diagram type, such as flowchart, sequence, class, state, or gantt.",
                    9,
                ),
                DialogKind::Calc => (
                    "Calculation",
                    "m := 5 kg\na := 9.81 m/s^2\nF := m a =\nF = kN",
                    "MathCAD-style worksheet, evaluated top to bottom. x := 5 m defines, a trailing = shows the value, and = kN picks the unit. Define functions with f(x) := x^2; lines starting with # are text.",
                    9,
                ),
            };
            ui.label(label);
            let limit = source_limit(dialog.rich_kind());
            let source = ui.add(
                egui::TextEdit::multiline(&mut dialog.source)
                    .code_editor()
                    .hint_text(hint)
                    .desired_rows(rows)
                    .desired_width(f32::INFINITY)
                    .char_limit(limit),
            );
            crate::views::name_field(ui, &source, label);
            ui.add(egui::Label::new(RichText::new(help).size(11.0).color(palette.text_muted)).wrap());
            ui.add_space(6.0);
            // The preview (`rich-preview-frame`).
            egui::Frame::new()
                .fill(palette.control_bg)
                .stroke(egui::Stroke::new(1.0, palette.border))
                .corner_radius(6)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.set_min_height(80.0);
                    let source = dialog.source.trim();
                    if source.is_empty() {
                        ui.label(RichText::new("Preview appears here.").color(palette.text_muted));
                        return;
                    }
                    match crate::rich_view::drawn(dialog.rich_kind(), source, palette.text_body) {
                        Ok((size, pixels)) => {
                            let fit = (ui.available_width() / size.x).min(1.0);
                            let texture = crate::rich_view::texture(ui.ctx(), &pixels);
                            let image = ui.add(egui::Image::new((texture.id(), size * fit)));
                            crate::views::name_field(ui, &image, "Preview");
                        }
                        Err(message) => {
                            ui.label(RichText::new(message).color(palette.danger));
                        }
                    }
                });
            if !dialog.error.is_empty() {
                ui.label(RichText::new(&dialog.error).color(palette.danger));
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Saved as editable source.").size(11.0).color(palette.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let save = if dialog.target.is_some() { "Update" } else { "Insert" };
                    if ui.add_enabled(editable, egui::Button::new(RichText::new(save).strong())).clicked() {
                        action = Action::Save;
                    }
                    if ui.button("Cancel").clicked() {
                        action = Action::Close;
                    }
                    if dialog.target.is_some() && ui.add_enabled(editable, egui::Button::new("Remove")).clicked() {
                        action = Action::Remove;
                    }
                });
            });
        });
    if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return (false, None);
    }
    match action {
        Action::None => (true, None),
        Action::Close => (false, None),
        Action::Save | Action::Remove => {
            let caret = apply(dialog, draft, &action);
            let done = dialog.error.is_empty();
            (
                !done,
                if done {
                    caret.or(Some(dialog.caret))
                } else {
                    None
                },
            )
        }
    }
}
