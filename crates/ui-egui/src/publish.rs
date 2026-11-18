//! The Publish view, as ProseCraft's `PublishView.svelte` lays it out: settings on the left
//! (summary, Format, Style, Manuscript, Front matter, the Export button) and the manuscript's
//! preview on the right. Settings are saved in the project's `publishOptions`, as ProseCraft
//! saves them. EPUB, DOCX, HTML, Markdown, plain text and Fountain are the exporters ported so
//! far; the other formats can be chosen (the choice is kept) but say they aren't ported yet.

use egui::{CornerRadius, Frame, Margin, RichText, Stroke, Ui};
use prosecraft_core::js::JsValue;
use prosecraft_core::publish::{
    PublishOptions, SceneSeparator, compile_book, export_docx, export_epub, export_html,
    export_markdown, export_text, slugify_filename,
};

use crate::editing::Intent;
use crate::theme::Palette;
use crate::widgets::{card, eyebrow};
use crate::{HostRequest, fonts};

/// ProseCraft's formats: id, label, extension, description (`FORMAT_INFO`).
const FORMATS: [(&str, &str, &str, &str); 8] = [
    (
        "pdf",
        "PDF",
        "pdf",
        "Print-ready pages for reading or proofing.",
    ),
    (
        "epub",
        "EPUB",
        "epub",
        "Reflowable ebook for e-readers and tablets.",
    ),
    (
        "docx",
        "DOCX",
        "docx",
        "Editable manuscript for Word or Pages.",
    ),
    (
        "markdown",
        "Markdown",
        "md",
        "Plain structured text for writing tools.",
    ),
    ("html", "HTML", "html", "A styled, self-contained web page."),
    (
        "text",
        "Plain text",
        "txt",
        "Clean text with no formatting.",
    ),
    // `SCREENPLAY_FORMAT_ORDER`: only for projects with screenplay scenes.
    (
        "fountain",
        "Fountain",
        "fountain",
        "Plain-text screenplay format for screenwriting apps.",
    ),
    (
        "screenplay-pdf",
        "Screenplay PDF",
        "pdf",
        "US Letter pages in industry screenplay format.",
    ),
];

/// Formats that can be exported here.
const PORTED: [&str; 8] = [
    "pdf",
    "epub",
    "docx",
    "markdown",
    "html",
    "text",
    "fountain",
    "screenplay-pdf",
];

/// ProseCraft's built-in styles (`PRESETS`).
const STYLES: [(&str, &str, &str); 4] = [
    (
        "manuscript",
        "Manuscript",
        "12 pt · double-spaced · title page · page numbers",
    ),
    (
        "classic",
        "Classic novel",
        "11.5 pt · justified · drop caps · 6 × 9 in",
    ),
    (
        "modern",
        "Modern trade",
        "11 pt · left-aligned · generous spacing · 6 × 9 in",
    ),
    (
        "ebook",
        "Ebook",
        "Relative spacing · no fixed margins · compact headings",
    ),
];

const SEPARATORS: [(SceneSeparator, &str, &str); 5] = [
    (SceneSeparator::Hash, "hash", "#"),
    (SceneSeparator::Asterisks, "asterisks", "* * *"),
    (SceneSeparator::Line, "line", "A rule"),
    (SceneSeparator::Blank, "blank", "Blank space"),
    (SceneSeparator::None, "none", "No separator"),
];

const CONTROLS_WIDTH: f32 = 320.0;

/// What the Publish view changes.
pub(crate) struct PublishEdit<'a> {
    pub editable: bool,
    /// Unix milliseconds, for the EPUB's modification date.
    pub now_ms: f64,
    /// A fresh id for the next EPUB, taken by an export.
    pub book_id: &'a mut Option<String>,
    pub intents: &'a mut Vec<Intent>,
    pub requests: &'a mut Vec<HostRequest>,
    /// Why the last cover image couldn't be used.
    pub cover_error: &'a mut Option<String>,
    /// The cover tab as stored: ProseCraft keeps "image" chosen before there is an image, which
    /// the options as read (`normalizeCover`) turn into "none".
    pub cover_mode: &'static str,
    /// The generated cover's picture, once drawn.
    pub generated_cover: Option<String>,
}

/// `FRONT_MATTER_PRESETS` and `BACK_MATTER_PRESETS`: label, title, blank.
const FRONT_PRESETS: [(&str, &str, bool); 5] = [
    ("Epigraph", "Epigraph", false),
    ("Foreword", "Foreword", false),
    ("Preface", "Preface", false),
    ("Introduction", "Introduction", false),
    ("Blank page", "", true),
];
const BACK_PRESETS: [(&str, &str, bool); 7] = [
    ("Acknowledgments", "Acknowledgments", false),
    ("About the Author", "About the Author", false),
    ("Also By", "Also By", false),
    ("Afterword", "Afterword", false),
    ("Appendix", "Appendix", false),
    ("Glossary", "Glossary", false),
    ("Blank page", "", true),
];

fn help(ui: &mut Ui, palette: &Palette, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(palette.text_muted));
}

/// The pages as `publishOptions` keeps them.
fn matter_value(pages: &[prosecraft_core::publish::MatterPage]) -> JsValue {
    JsValue::Array(
        pages
            .iter()
            .map(|page| {
                let mut object = prosecraft_core::js::JsObject::new();
                object.insert("id", JsValue::str(&page.id));
                object.insert("title", JsValue::str(&page.title));
                object.insert("body", JsValue::str(&page.body));
                if page.blank {
                    object.insert("blank", JsValue::Bool(true));
                }
                JsValue::Object(object)
            })
            .collect(),
    )
}

/// A text field saved when it is left (each keystroke isn't a revision).
#[allow(clippy::too_many_arguments)]
fn kept_field(
    ui: &mut Ui,
    editable: bool,
    key: &str,
    value: &str,
    multiline: bool,
    hint: &str,
    name: &str,
    limit: usize,
) -> Option<String> {
    let id = ui.make_persistent_id(("matter-field", key));
    let mut text = ui
        .data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| value.to_owned());
    let field = if multiline {
        egui::TextEdit::multiline(&mut text).desired_rows(3)
    } else {
        egui::TextEdit::singleline(&mut text)
    };
    let response = ui.add_enabled(
        editable,
        field
            .id(id)
            .char_limit(limit)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    );
    crate::views::name_field(ui, &response, name);
    if response.changed() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
    }
    if response.lost_focus() {
        ui.data_mut(|d| d.remove_temp::<String>(id));
        if text != value {
            return Some(text);
        }
    }
    None
}

/// "Additional pages": add from a preset (at most 12), each page's title and text, move up,
/// move down, remove (`addMatterPage`, `updateMatterPage`, `moveMatterPage`, `removeMatterPage`).
fn matter_editor(
    ui: &mut Ui,
    palette: &Palette,
    edit: &mut PublishEdit<'_>,
    front: bool,
    pages: &[prosecraft_core::publish::MatterPage],
) {
    let (key, side, presets): (&str, &str, &[(&str, &str, bool)]) = if front {
        ("frontMatter", "front matter", &FRONT_PRESETS)
    } else {
        ("backMatter", "back matter", &BACK_PRESETS)
    };
    let editable = edit.editable;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Additional pages").strong());
        ui.label(
            RichText::new(format!("{}/12", pages.len()))
                .size(12.0)
                .color(palette.text_muted),
        );
    });
    let preset_id = ui.make_persistent_id(("matter-preset", front));
    let mut chosen = ui.data(|d| d.get_temp::<usize>(preset_id)).unwrap_or(0);
    ui.horizontal(|ui| {
        let combo = craft_ui::ComboBox::from_id_salt(("matter-preset-combo", front))
            .selected_text(presets[chosen.min(presets.len() - 1)].0)
            .show_ui(ui, |ui| {
                for (index, (label, ..)) in presets.iter().enumerate() {
                    ui.selectable_value(&mut chosen, index, *label);
                }
            });
        let name = if front {
            "Front matter preset"
        } else {
            "Back matter preset"
        };
        ui.ctx()
            .accesskit_node_builder(combo.response.id, |node| node.set_label(name));
        ui.data_mut(|d| d.insert_temp(preset_id, chosen));
        let add = ui.add_enabled(editable && pages.len() < 12, egui::Button::new("Add page"));
        if add.clicked() {
            let (_, title, blank) = presets[chosen.min(presets.len() - 1)];
            edit.intents.push(Intent::AddMatterPage {
                front,
                title: title.to_owned(),
                blank,
            });
        }
    });
    let mut next: Option<Vec<prosecraft_core::publish::MatterPage>> = None;
    for (index, page) in pages.iter().enumerate() {
        let named = if page.title.is_empty() {
            format!("{side} page")
        } else {
            page.title.clone()
        };
        ui.horizontal(|ui| {
            let hint = if page.blank && page.title.is_empty() {
                "Blank page"
            } else {
                "Page title"
            };
            let label = if front {
                "Front matter page title"
            } else {
                "Back matter page title"
            };
            ui.allocate_ui(egui::vec2(ui.available_width() - 110.0, 24.0), |ui| {
                if let Some(title) = kept_field(
                    ui,
                    editable,
                    &format!("{}:title", page.id),
                    &page.title,
                    false,
                    hint,
                    label,
                    120,
                ) {
                    let mut all = pages.to_vec();
                    all[index].title = title;
                    next = Some(all);
                }
            });
            for (text, offset, name, enabled) in [
                ("↑", -1isize, format!("Move {named} up"), index > 0),
                (
                    "↓",
                    1,
                    format!("Move {named} down"),
                    index + 1 < pages.len(),
                ),
            ] {
                let button = ui.add_enabled(editable && enabled, egui::Button::new(text).small());
                crate::views::name_field(ui, &button, &name);
                if button.clicked() {
                    let mut all = pages.to_vec();
                    all.swap(index, index.saturating_add_signed(offset));
                    next = Some(all);
                }
            }
            let remove = ui.add_enabled(
                editable,
                egui::Button::new(RichText::new("Remove").color(palette.danger)).small(),
            );
            crate::views::name_field(ui, &remove, &format!("Remove {named}"));
            if remove.clicked() {
                next = Some(pages.iter().filter(|p| p.id != page.id).cloned().collect());
            }
        });
        let body_name = format!(
            "{} text",
            if page.title.is_empty() {
                if front { "Front matter" } else { "Back matter" }
            } else {
                page.title.as_str()
            }
        );
        if let Some(body) = kept_field(
            ui,
            editable,
            &format!("{}:body", page.id),
            &page.body,
            true,
            "Page text…",
            &body_name,
            20_000,
        ) {
            let mut all = next.clone().unwrap_or_else(|| pages.to_vec());
            if let Some(p) = all.iter_mut().find(|p| p.id == page.id) {
                p.body = body;
            }
            next = Some(all);
        }
        ui.add_space(6.0);
    }
    if let Some(pages) = next {
        set(edit, key, matter_value(&pages));
    }
}

/// The cover's picture, decoded once per image.
fn cover_texture(ctx: &egui::Context, data_url: &str) -> Option<egui::TextureHandle> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data_url.hash(&mut hasher);
    let id = egui::Id::unique(("cover-texture", hasher.finish()));
    if let Some(found) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(id)) {
        return found;
    }
    use base64::Engine;
    let texture = data_url
        .split_once(";base64,")
        .and_then(|(_, data)| base64::engine::general_purpose::STANDARD.decode(data).ok())
        .and_then(|bytes| craft_image_codecs::decode(&bytes).ok())
        .map(|image| {
            ctx.load_texture(
                "cover",
                crate::pictures::color_image(ctx, &image),
                egui::TextureOptions::LINEAR,
            )
        });
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    texture
}

/// The cover thumbnail (`cover-thumb`): the picture at 2:3, or a word on an empty frame.
fn cover_thumb(ui: &mut Ui, palette: &Palette, picture: Option<&str>, empty: &str, alt: &str) {
    let size = egui::vec2(72.0, 108.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let texture = picture.and_then(|p| cover_texture(ui.ctx(), p));
    let painter = ui.painter();
    match &texture {
        Some(texture) => {
            painter.image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            painter.rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0, palette.border_control),
                egui::StrokeKind::Inside,
            );
        }
        None => {
            painter.rect(
                rect,
                3.0,
                palette.control_bg,
                Stroke::new(1.0, palette.border_control),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                empty,
                egui::FontId::proportional(10.0),
                palette.text_muted,
            );
        }
    }
    let label = if texture.is_some() { alt } else { empty };
    ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::Image);
        n.set_label(label);
    });
}

/// The Cover section: none, an uploaded image, or a generated design and palette.
fn cover_section(
    ui: &mut Ui,
    palette: &Palette,
    options: &PublishOptions,
    edit: &mut PublishEdit<'_>,
) {
    use prosecraft_core::publish::cover::{DESIGNS, PALETTES, design_for};
    let editable = edit.editable;
    let cover = &options.cover;
    let mode = edit.cover_mode;
    // `coverSummary`.
    let summary = match mode {
        "none" => "None",
        "image" => "Image",
        _ => design_for(cover.design).1,
    };
    section_heading(ui, palette, "Cover", summary);
    ui.add_space(4.0);
    // `updateCover`: the stored cover with some fields changed.
    let update =
        |edit: &mut PublishEdit<'_>, mode: &str, image: &str, design: &str, palette_id: &str| {
            let mut value = cover.to_value();
            if let Some(o) = value.as_object_mut() {
                o.insert("mode", JsValue::str(mode));
                o.insert("image", JsValue::str(image));
                o.insert("design", JsValue::str(design));
                o.insert("palette", JsValue::str(palette_id));
            }
            set(edit, "cover", value);
        };
    ui.horizontal(|ui| {
        for (id, label) in [
            ("none", "None"),
            ("image", "Image"),
            ("generated", "Generated"),
        ] {
            let tab = ui.add_enabled(editable, egui::Button::selectable(mode == id, label));
            ui.ctx().accesskit_node_builder(tab.id, |n| {
                n.set_label(format!("Cover: {label}"));
            });
            if tab.clicked() && mode != id {
                // `setCoverMode`: the image is kept only for the Image tab.
                *edit.cover_error = None;
                let image = if id == "image" {
                    cover.image.clone()
                } else {
                    String::new()
                };
                update(edit, id, &image, cover.design, cover.palette);
            }
        }
    });
    ui.add_space(6.0);
    match mode {
        "image" => {
            ui.horizontal_top(|ui| {
                let picture = (!cover.image.is_empty()).then_some(cover.image.as_str());
                cover_thumb(ui, palette, picture, "No image", "Cover preview");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let choose = if cover.image.is_empty() {
                            "Choose image"
                        } else {
                            "Replace"
                        };
                        if ui
                            .add_enabled(editable, egui::Button::new(choose))
                            .clicked()
                        {
                            edit.intents.push(Intent::ChooseCover);
                        }
                        if !cover.image.is_empty()
                            && ui
                                .add_enabled(editable, egui::Button::new("Remove"))
                                .clicked()
                        {
                            // `removeCoverImage`.
                            *edit.cover_error = None;
                            update(edit, "none", "", cover.design, cover.palette);
                        }
                    });
                    help(ui, palette, "PNG, JPG, or WebP. Cropped to a 2:3 cover.");
                });
            });
        }
        "generated" => {
            ui.horizontal_top(|ui| {
                let art = edit.generated_cover.as_deref();
                cover_thumb(ui, palette, art, "No preview", "Generated cover preview");
                ui.vertical(|ui| {
                    let width = ui.available_width();
                    for (id, label, detail) in DESIGNS {
                        let chosen = cover.design == id;
                        if choice_card(ui, palette, chosen, label, "", detail, width)
                            && editable
                            && !chosen
                        {
                            update(edit, mode, "", id, cover.palette);
                        }
                        ui.add_space(4.0);
                    }
                    ui.horizontal(|ui| {
                        for p in PALETTES {
                            let (rect, swatch) = ui
                                .allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
                            let chosen = cover.palette == p.id;
                            let color = |hex: &str| {
                                egui::Color32::from_hex(hex).unwrap_or(egui::Color32::GRAY)
                            };
                            ui.painter().circle(
                                rect.center(),
                                10.0,
                                color(p.background),
                                Stroke::new(
                                    if chosen { 2.5 } else { 1.0 },
                                    if chosen { palette.accent } else { color(p.ink) },
                                ),
                            );
                            let swatch = swatch.on_hover_text(p.label);
                            ui.ctx().accesskit_node_builder(swatch.id, |n| {
                                n.set_role(egui::accesskit::Role::Button);
                                n.set_label(format!("{} palette", p.label));
                            });
                            if editable && swatch.clicked() && !chosen {
                                update(edit, mode, "", cover.design, p.id);
                            }
                        }
                    });
                });
            });
        }
        _ => {}
    }
    if let Some(error) = edit.cover_error.as_deref() {
        ui.label(RichText::new(error).size(11.0).color(palette.danger));
    }
}

fn style_editor_id() -> egui::Id {
    egui::Id::unique("style-editor")
}

/// The Style editor's draft (`StyleEditorDialog`): the style as edited, what it opened on (for
/// Reset), whether it is new, and whether Delete is being confirmed.
#[derive(Clone, Debug)]
struct StyleDraft {
    draft: prosecraft_core::publish::styles::CustomStyle,
    original: prosecraft_core::publish::styles::CustomStyle,
    new: bool,
    deleting: bool,
}

impl StyleDraft {
    fn new(
        style: prosecraft_core::publish::styles::CustomStyle,
        new: bool,
        deleting: bool,
    ) -> Self {
        Self {
            original: style.clone(),
            draft: style,
            new,
            deleting,
        }
    }
}

/// A number field with ProseCraft's bounds and step.
fn number(
    ui: &mut Ui,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    step: f64,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.0));
        let field = ui.add(
            egui::DragValue::new(value)
                .range(range)
                .speed(step)
                .max_decimals(2),
        );
        crate::views::name_field(ui, &field, label);
    });
}

/// `StyleEditorDialog`.
fn style_editor(
    ui: &mut Ui,
    palette: &Palette,
    edit: &mut PublishEdit<'_>,
    entries: &[(String, String, String, bool)],
) {
    use prosecraft_core::publish::styles::{FONTS, TRIMS};
    let Some(mut state) = ui.data(|d| d.get_temp::<StyleDraft>(style_editor_id())) else {
        return;
    };
    // `styleExistingLabels`: every other style's name.
    let label = state.draft.label.trim().to_lowercase();
    let name_error = if label.is_empty() {
        Some("Give this style a name.")
    } else if entries
        .iter()
        .any(|(id, l, ..)| *id != state.draft.id && l.trim().to_lowercase() == label)
    {
        Some("Another style already uses that name.")
    } else {
        None
    };
    let mut open = true;
    let mut close = false;
    let d = &mut state.draft;
    egui::Window::new(if state.new { "New style" } else { "Edit style" })
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            egui::ScrollArea::vertical()
                .max_height(520.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("EXPORT STYLE")
                            .size(10.0)
                            .color(palette.text_muted),
                    );
                    ui.label(RichText::new("Name").strong());
                    help(ui, palette, "Shown in the style picker for this project.");
                    let name = ui.add(egui::TextEdit::singleline(&mut d.label).char_limit(60));
                    crate::views::name_field(ui, &name, "Style name");
                    if let Some(error) = name_error {
                        ui.label(RichText::new(error).color(palette.danger));
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("Body typography").strong());
                    help(ui, palette, "How the manuscript text is set.");
                    let font = craft_ui::ComboBox::from_id_salt("style-font")
                        .selected_text(FONTS.iter().find(|f| f.0 == d.font_id).map_or("", |f| f.1))
                        .show_ui(ui, |ui| {
                            for f in FONTS {
                                ui.selectable_value(&mut d.font_id, f.0, f.1);
                            }
                        });
                    ui.ctx()
                        .accesskit_node_builder(font.response.id, |n| n.set_label("Font"));
                    number(ui, "Body size (pt)", &mut d.body_size, 6.0..=36.0, 0.5);
                    number(ui, "Line height", &mut d.line_height, 1.0..=3.0, 0.05);
                    number(ui, "First-line indent (em)", &mut d.indent, 0.0..=4.0, 0.1);
                    number(
                        ui,
                        "Paragraph spacing (em)",
                        &mut d.spacing,
                        0.0..=3.0,
                        0.05,
                    );
                    ui.checkbox(&mut d.justify, "Justify body text");
                    ui.add_space(8.0);
                    ui.label(RichText::new("Chapter headings").strong());
                    help(ui, palette, "Size and placement of chapter titles.");
                    number(
                        ui,
                        "Heading size (pt)",
                        &mut d.chapter_size,
                        8.0..=48.0,
                        0.5,
                    );
                    let align = craft_ui::ComboBox::from_id_salt("style-align")
                        .selected_text(if d.chapter_align == "center" {
                            "Centered"
                        } else {
                            "Left"
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.chapter_align, "left", "Left");
                            ui.selectable_value(&mut d.chapter_align, "center", "Centered");
                        });
                    ui.ctx()
                        .accesskit_node_builder(align.response.id, |n| n.set_label("Alignment"));
                    number(
                        ui,
                        "Space before (em)",
                        &mut d.chapter_before,
                        0.0..=6.0,
                        0.1,
                    );
                    number(ui, "Space after (em)", &mut d.chapter_after, 0.0..=6.0, 0.1);
                    ui.checkbox(&mut d.chapter_uppercase, "Uppercase chapter headings");
                    ui.add_space(8.0);
                    ui.label(RichText::new("Page layout").strong());
                    help(
                        ui,
                        palette,
                        "Trim size and margins for print and paginated previews.",
                    );
                    let trim = craft_ui::ComboBox::from_id_salt("style-trim")
                        .selected_text(TRIMS.iter().find(|t| t.0 == d.trim_id).map_or("", |t| t.1))
                        .show_ui(ui, |ui| {
                            for t in TRIMS {
                                ui.selectable_value(&mut d.trim_id, t.0, t.1);
                            }
                        });
                    ui.ctx()
                        .accesskit_node_builder(trim.response.id, |n| n.set_label("Trim size"));
                    for (index, label) in [
                        "Top margin (in)",
                        "Right margin (in)",
                        "Bottom margin (in)",
                        "Left margin (in)",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        number(ui, label, &mut d.margins[index], 0.25..=1.5, 0.01);
                    }
                    ui.checkbox(&mut d.running_header, "Running header on body pages");
                    ui.checkbox(&mut d.page_numbers, "Page numbers");
                    ui.add_space(8.0);
                    ui.label(RichText::new("Details").strong());
                    help(ui, palette, "Scene breaks and decorative touches.");
                    number(
                        ui,
                        "Scene break spacing (em)",
                        &mut d.scene_break,
                        0.0..=3.0,
                        0.05,
                    );
                    ui.checkbox(&mut d.drop_cap, "Use a drop cap when Drop caps is enabled");
                });
            ui.separator();
            ui.horizontal(|ui| {
                if !state.new {
                    if state.deleting {
                        ui.label("Delete this style?");
                        if ui
                            .button(RichText::new("Delete").color(palette.danger))
                            .clicked()
                        {
                            edit.intents.push(Intent::DeleteCustomStyle(d.id.clone()));
                            close = true;
                        }
                        if ui.button("Keep").clicked() {
                            state.deleting = false;
                        }
                    } else if ui
                        .button(RichText::new("Delete style").color(palette.danger))
                        .clicked()
                    {
                        state.deleting = true;
                    }
                }
                if ui.button("Reset").clicked() {
                    *d = state.original.clone();
                    state.deleting = false;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                let save = if state.new {
                    "Create style"
                } else {
                    "Save changes"
                };
                if ui
                    .add_enabled(
                        name_error.is_none(),
                        egui::Button::new(RichText::new(save).strong()),
                    )
                    .clicked()
                {
                    edit.intents
                        .push(Intent::SaveCustomStyle(Box::new(d.clone())));
                    close = true;
                }
            });
        });
    if close || !open {
        ui.data_mut(|data| data.remove::<StyleDraft>(style_editor_id()));
    } else {
        ui.data_mut(|data| data.insert_temp(style_editor_id(), state));
    }
}

fn set(edit: &mut PublishEdit<'_>, key: &str, value: JsValue) {
    edit.intents.push(Intent::SetPublishOption {
        key: key.to_owned(),
        value,
    });
}

fn section_heading(ui: &mut Ui, palette: &Palette, title: &str, aside: &str) {
    ui.horizontal(|ui| {
        eyebrow(ui, palette, title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(aside.to_uppercase())
                    .size(10.0)
                    .color(palette.text_muted),
            );
        });
    });
}

/// A selectable card in a two-column grid (format and style cards). Returns whether it was clicked.
fn choice_card(
    ui: &mut Ui,
    palette: &Palette,
    selected: bool,
    title: &str,
    tag: &str,
    detail: &str,
    width: f32,
) -> bool {
    let stroke = if selected {
        Stroke::new(1.5, palette.accent)
    } else {
        Stroke::new(1.0, palette.border_control)
    };
    let response = Frame::new()
        .fill(if selected {
            palette.selected_bg
        } else {
            palette.control_bg
        })
        .stroke(stroke)
        .corner_radius(CornerRadius::same(7))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(width - 22.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(title)
                            .font(fonts::semibold(13.0))
                            .color(palette.text_heading),
                    );
                    if !tag.is_empty() {
                        ui.label(
                            RichText::new(tag)
                                .monospace()
                                .size(10.0)
                                .color(palette.eyebrow),
                        );
                    }
                });
                ui.add(
                    egui::Label::new(RichText::new(detail).size(11.0).color(palette.text_muted))
                        .wrap(),
                );
            });
        })
        .response;
    let response = response.interact(egui::Sense::click());
    let name = if tag.is_empty() {
        title.to_owned()
    } else {
        format!("{title} ({tag})")
    };
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, &name));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
}

fn grid<T: Copy>(ui: &mut Ui, items: &[T], mut cell: impl FnMut(&mut Ui, T, f32)) {
    let width = (ui.available_width() - 8.0) / 2.0;
    for pair in items.chunks(2) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for item in pair {
                cell(ui, *item, width);
            }
        });
        ui.add_space(4.0);
    }
}

/// An option row: a checkbox with a title and a hint below. Returns the new value when changed.
fn option_row(
    ui: &mut Ui,
    palette: &Palette,
    enabled: bool,
    value: bool,
    title: &str,
    hint: &str,
) -> Option<bool> {
    let mut checked = value;
    let changed = ui
        .add_enabled(
            enabled,
            egui::Checkbox::new(
                &mut checked,
                RichText::new(title).color(palette.text_primary),
            ),
        )
        .changed();
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        ui.add(egui::Label::new(RichText::new(hint).size(11.0).color(palette.text_muted)).wrap());
    });
    ui.add_space(6.0);
    changed.then_some(checked)
}

pub(crate) fn show(
    ui: &mut Ui,
    palette: &Palette,
    stored: &JsValue,
    project: &prosecraft_core::project::Project,
    edit: &mut PublishEdit<'_>,
) {
    let options = PublishOptions::read(stored.get("project").and_then(|p| p.get("publishOptions")));
    edit.cover_mode = stored
        .get("project")
        .and_then(|p| p.get("publishOptions"))
        .and_then(|o| o.get("cover"))
        .and_then(|c| c.get("mode"))
        .and_then(JsValue::as_js_string)
        .map(|m| m.to_string_lossy())
        .and_then(|m| {
            prosecraft_core::publish::cover::MODES
                .into_iter()
                .find(|x| *x == m)
        })
        .unwrap_or(options.cover.mode);
    let book = compile_book(project, &options);
    let preview = String::from_utf8_lossy(&export_text(&book, &options).bytes).into_owned();
    ui.add_space(18.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(CONTROLS_WIDTH);
            egui::ScrollArea::vertical()
                .id_salt("publish-controls")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(CONTROLS_WIDTH - 12.0);
                    // `hasScreenplayDocuments`: the screenplay formats join the list.
                    let screenplay = project.documents.iter().any(|d| d.screenplay);
                    controls(
                        ui,
                        palette,
                        &options,
                        &book,
                        screenplay,
                        project.citations.len(),
                        edit,
                    );
                });
        });
        ui.add_space(18.0);
        ui.vertical(|ui| {
            preview_pane(ui, palette, &book.title, &preview);
        });
    });
}

/// A labelled choice, as ProseCraft's `option-row-select`: the title and hint, then the select.
#[allow(clippy::too_many_arguments)]
fn select_row(
    ui: &mut Ui,
    palette: &Palette,
    editable: bool,
    (id, name): (&str, &str),
    hint: &str,
    current_label: &str,
    choices: &[(String, String)],
    current: &str,
    on_choose: &mut dyn FnMut(&str),
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(name).color(palette.text_primary));
            ui.label(RichText::new(hint).size(11.0).color(palette.text_muted));
        });
        let mut chosen = current.to_owned();
        ui.add_enabled_ui(editable, |ui| {
            let combo = craft_ui::ComboBox::from_id_salt(id)
                .selected_text(current_label)
                .show_ui(ui, |ui| {
                    for (value, label) in choices {
                        ui.selectable_value(&mut chosen, value.clone(), label);
                    }
                });
            ui.ctx().accesskit_node_builder(combo.response.id, |node| {
                node.set_label(name);
                node.set_value(current_label);
            });
        });
        if chosen != current {
            on_choose(&chosen);
        }
    });
    ui.add_space(4.0);
}

fn controls(
    ui: &mut Ui,
    palette: &Palette,
    options: &PublishOptions,
    book: &prosecraft_core::publish::CompiledBook,
    screenplay: bool,
    sources: usize,
    edit: &mut PublishEdit<'_>,
) {
    let editable = edit.editable;
    eyebrow(ui, palette, "Publish");
    ui.label(
        RichText::new("Publish your manuscript")
            .font(fonts::serif(24.0))
            .color(palette.text_heading),
    );
    ui.add_space(10.0);

    // The summary: chapters, scenes, words, pages.
    let pages = book.page_estimate(options);
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            one.to_owned()
        } else {
            many.to_owned()
        }
    };
    let stats = [
        (
            book.chapters.len().to_string(),
            plural(book.chapters.len(), "chapter", "chapters"),
        ),
        (
            book.scenes().to_string(),
            plural(book.scenes(), "scene", "scenes"),
        ),
        (crate::views::grouped(book.words), "words".to_owned()),
        match pages {
            Some(p) => (p.to_string(), "pages".to_owned()),
            None => (
                "—".to_owned(),
                if options.words_per_page == Some(0) {
                    "reflowable"
                } else {
                    "pages"
                }
                .to_owned(),
            ),
        },
    ];
    ui.columns(4, |cols| {
        for (col, (value, label)) in cols.iter_mut().zip(&stats) {
            col.label(
                RichText::new(value)
                    .font(fonts::semibold(15.0))
                    .color(palette.text_heading),
            );
            col.label(
                RichText::new(label.to_uppercase())
                    .size(9.0)
                    .color(palette.text_muted),
            );
        }
    });
    ui.separator();

    // Format.
    let offered = &FORMATS[..if screenplay { 8 } else { 6 }];
    let format = offered
        .iter()
        .find(|f| f.0 == options.format)
        .copied()
        .unwrap_or(offered[0]);
    section_heading(ui, palette, "Format", format.2);
    ui.add_space(4.0);
    grid(ui, offered, |ui, (id, label, ext, description), width| {
        if choice_card(
            ui,
            palette,
            options.format == id,
            label,
            ext,
            description,
            width,
        ) && editable
            && options.format != id
        {
            set(edit, "format", JsValue::str(id));
        }
    });
    ui.separator();

    // Style: the built-in ones and the project's own (`styleSummaries`).
    let style_label =
        prosecraft_core::publish::styles::label_of(&options.style, &options.custom_styles);
    section_heading(ui, palette, "Style", &style_label);
    ui.add_space(4.0);
    let entries: Vec<(String, String, String, bool)> = STYLES
        .iter()
        .map(|(id, label, detail)| {
            (
                (*id).to_owned(),
                (*label).to_owned(),
                (*detail).to_owned(),
                false,
            )
        })
        .chain(options.custom_styles.iter().map(|s| {
            (
                s.id.clone(),
                s.label.clone(),
                prosecraft_core::publish::styles::detail(s),
                true,
            )
        }))
        .collect();
    let mut editing: Option<(String, bool)> = None;
    grid(
        ui,
        &entries.iter().collect::<Vec<_>>(),
        |ui, (id, label, detail, custom), width| {
            ui.vertical(|ui| {
                let tag = if *custom { "Custom" } else { "" };
                if choice_card(ui, palette, options.style == *id, label, tag, detail, width)
                    && editable
                    && options.style != *id
                {
                    set(edit, "style", JsValue::str(id));
                }
                if *custom && editable {
                    ui.horizontal(|ui| {
                        let e = ui.small_button("Edit").on_hover_text("Edit style");
                        crate::views::name_field(ui, &e, &format!("Edit {label}"));
                        if e.clicked() {
                            editing = Some((id.clone(), false));
                        }
                        let d = ui
                            .small_button(RichText::new("Delete").color(palette.danger))
                            .on_hover_text("Delete style");
                        crate::views::name_field(ui, &d, &format!("Delete {label}"));
                        if d.clicked() {
                            editing = Some((id.clone(), true));
                        }
                    });
                }
            });
        },
    );
    let room = options.custom_styles.len() < prosecraft_core::publish::styles::MAX_STYLES;
    if ui
        .add_enabled(editable && room, egui::Button::new("+ New style"))
        .clicked()
    {
        // `startNewStyle`: a copy of the chosen style; its id comes when it is saved.
        let draft = prosecraft_core::publish::styles::create_custom_style(
            &options.style,
            &options.custom_styles,
            "",
        );
        let draft = prosecraft_core::publish::styles::CustomStyle {
            id: String::new(),
            ..draft
        };
        ui.data_mut(|d| d.insert_temp(style_editor_id(), StyleDraft::new(draft, true, false)));
    }
    help(
        ui,
        palette,
        &if room {
            "Custom styles are saved with this project.".to_owned()
        } else {
            format!(
                "You’ve reached the limit of {} custom styles.",
                prosecraft_core::publish::styles::MAX_STYLES
            )
        },
    );
    if let Some((id, delete)) = editing
        && let Some(style) = options.custom_styles.iter().find(|s| s.id == id)
    {
        ui.data_mut(|d| {
            d.insert_temp(
                style_editor_id(),
                StyleDraft::new(style.clone(), false, delete),
            );
        });
    }
    style_editor(ui, palette, edit, &entries);
    ui.separator();

    cover_section(ui, palette, options, edit);
    ui.separator();

    // Manuscript.
    section_heading(ui, palette, "Manuscript", "");
    ui.add_space(4.0);
    let rows: [(&str, bool, &str, &str); 4] = [
        (
            "includeChapterTitles",
            options.include_chapter_titles,
            "Chapter headings",
            "Use the same chapter names and numbering as the sidebar.",
        ),
        (
            "includeSceneTitles",
            options.include_scene_titles,
            "Scene titles",
            "Include each scene's title above its text.",
        ),
        (
            "pageBreakBeforeChapters",
            options.page_break_before_chapters,
            "Start chapters on a new page",
            "Give every chapter a clean opening page.",
        ),
        (
            "dropCaps",
            options.drop_caps,
            "Drop caps",
            "Use a large initial letter when the style supports it.",
        ),
    ];
    for (key, value, title, hint) in rows {
        if let Some(value) = option_row(ui, palette, editable, value, title, hint) {
            set(edit, key, JsValue::Bool(value));
        }
    }
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new("Scene separator").color(palette.text_primary));
            ui.label(
                RichText::new("Shown between scenes inside a chapter.")
                    .size(11.0)
                    .color(palette.text_muted),
            );
        });
        let current = SEPARATORS
            .iter()
            .find(|s| s.0 == options.scene_separator)
            .copied()
            .unwrap_or(SEPARATORS[0]);
        let mut chosen = current.1;
        ui.add_enabled_ui(editable, |ui| {
            let combo = craft_ui::ComboBox::from_id_salt("scene-separator")
                .selected_text(current.2)
                .show_ui(ui, |ui| {
                    for (_, id, label) in SEPARATORS {
                        ui.selectable_value(&mut chosen, id, label);
                    }
                });
            ui.ctx().accesskit_node_builder(combo.response.id, |node| {
                node.set_label("Scene separator");
                node.set_value(current.2);
            });
        });
        if chosen != current.1 {
            set(edit, "sceneSeparator", JsValue::str(chosen));
        }
    });
    ui.add_space(6.0);
    if let Some(value) = option_row(
        ui,
        palette,
        editable,
        options.footnotes_restart_per_chapter,
        "Restart footnotes each chapter",
        "Number footnotes from 1 in every chapter, with notes after it.",
    ) {
        set(edit, "footnotesRestartPerChapter", JsValue::Bool(value));
    }
    ui.separator();

    // ProseCraft's Citations section: the style and language citations are written in, and
    // whether a bibliography ends the book.
    section_heading(
        ui,
        palette,
        "Citations",
        &format!(
            "{sources} {}",
            if sources == 1 { "source" } else { "sources" }
        ),
    );
    ui.add_space(4.0);
    let styles = prosecraft_core::citations::styles::styles();
    let style_label = styles
        .iter()
        .find(|s| s.id == options.citation_style)
        .map_or("APA 7th Edition".to_owned(), |s| s.label.clone());
    select_row(
        ui,
        palette,
        editable,
        ("citation-style", "Citation style"),
        "Applied to inline citations and the bibliography.",
        &style_label,
        &styles
            .iter()
            .map(|s| (s.id.clone(), s.label.clone()))
            .collect::<Vec<_>>(),
        &options.citation_style,
        &mut |id| set(edit, "citationStyle", JsValue::str(id)),
    );
    let languages = [
        ("en".to_owned(), "English".to_owned()),
        ("de".to_owned(), "Deutsch".to_owned()),
    ];
    let language = if options.citation_locale == "de" {
        "Deutsch"
    } else {
        "English"
    };
    select_row(
        ui,
        palette,
        editable,
        ("citation-language", "Citation language"),
        "Used for terms like ed., pp., and n.d.",
        language,
        &languages,
        &options.citation_locale,
        &mut |id| set(edit, "citationLocale", JsValue::str(id)),
    );
    if let Some(value) = option_row(
        ui,
        palette,
        editable,
        options.include_bibliography,
        "Include bibliography",
        "Append a formatted reference list when citations are used.",
    ) {
        set(edit, "includeBibliography", JsValue::Bool(value));
    }
    ui.separator();

    // Front matter.
    egui::CollapsingHeader::new(RichText::new("Front matter").color(palette.text_primary))
        .id_salt("front-matter")
        .show(ui, |ui| {
            if let Some(v) = option_row(
                ui,
                palette,
                editable,
                options.include_title_page,
                "Title page",
                "Project title, subtitle, author, and copyright.",
            ) {
                set(edit, "includeTitlePage", JsValue::Bool(v));
            }
            for (key, label, value, limit) in [
                ("subtitle", "Subtitle", &options.subtitle, 200),
                ("copyright", "Copyright", &options.copyright, 500),
                ("dedication", "Dedication", &options.dedication, 1000),
            ] {
                ui.label(
                    RichText::new(label)
                        .size(12.0)
                        .color(palette.text_secondary),
                );
                // Typed text is kept here and saved when the field is left, so each keystroke
                // isn't a revision.
                let id = ui.make_persistent_id(("publish-field", key));
                let mut text = ui
                    .data(|d| d.get_temp::<String>(id))
                    .unwrap_or_else(|| value.clone());
                let response = ui.add_enabled(
                    editable,
                    egui::TextEdit::singleline(&mut text)
                        .id(id)
                        .char_limit(limit)
                        .desired_width(f32::INFINITY),
                );
                crate::views::name_field(ui, &response, label);
                if response.changed() {
                    ui.data_mut(|d| d.insert_temp(id, text.clone()));
                }
                if response.lost_focus() {
                    ui.data_mut(|d| d.remove_temp::<String>(id));
                    if text != *value {
                        set(edit, key, JsValue::str(&text));
                    }
                }
                ui.add_space(4.0);
            }
            if let Some(v) = option_row(
                ui,
                palette,
                editable,
                options.include_copyright_page,
                "Copyright on its own page",
                "Move the copyright text off the title page.",
            ) {
                set(edit, "includeCopyrightPage", JsValue::Bool(v));
            }
            if let Some(v) = option_row(
                ui,
                palette,
                editable,
                options.include_table_of_contents,
                "Table of contents",
                "List chapter titles at the front of the book.",
            ) {
                set(edit, "includeTableOfContents", JsValue::Bool(v));
            }
            matter_editor(ui, palette, edit, true, &options.front_matter);
        });
    ui.separator();
    egui::CollapsingHeader::new(RichText::new("Back matter").color(palette.text_primary))
        .id_salt("back-matter")
        .show(ui, |ui| {
            help(ui, palette, "Acknowledgments, author notes, appendices");
            matter_editor(ui, palette, edit, false, &options.back_matter);
        });
    ui.separator();

    // Export.
    let file_name = format!("{}.{}", slugify_filename(&book.title), format.2);
    ui.horizontal(|ui| {
        ui.label(RichText::new("FILE").size(10.0).color(palette.text_muted));
        ui.label(
            RichText::new(&file_name)
                .monospace()
                .size(12.0)
                .color(palette.text_secondary),
        );
    });
    ui.add_space(4.0);
    let ported = PORTED.contains(&format.0);
    let can_export = ported && book.scenes() > 0;
    let button = egui::Button::new(
        RichText::new(format!("Export {}", format.1))
            .color(palette.page_bg)
            .strong(),
    )
    .fill(palette.accent_strong)
    .min_size(egui::vec2(ui.available_width(), 34.0));
    if ui.add_enabled(can_export, button).clicked() {
        // `resolveCoverImage`: the cover joins the book for the export.
        let mut with_cover;
        let book = if options.cover.mode == "image" {
            with_cover = book.clone();
            with_cover.cover = prosecraft_core::publish::cover::resolve_image(&options.cover);
            &with_cover
        } else {
            book
        };
        let export = match format.0 {
            "epub" => {
                // Without a library store there is no id source: a fixed id, so the export works.
                let uuid = edit
                    .book_id
                    .take()
                    .unwrap_or_else(|| "00000000-0000-4000-8000-000000000000".to_owned());
                export_epub(book, options, &uuid, edit.now_ms).map(|e| (e, 0))
            }
            .map_err(|e| e.to_string()),
            "pdf" => prosecraft_core::publish::pdf_writer::export_pdf(book, options)
                .map_err(|e| e.to_string()),
            "docx" => export_docx(book, options)
                .map(|e| (e, 0))
                .map_err(|e| e.to_string()),
            "screenplay-pdf" => prosecraft_core::screenplay::pdf::export_screenplay_pdf(book)
                .map_err(|e| e.to_string()),
            "markdown" => Ok((export_markdown(book, options), 0)),
            "fountain" => Ok((
                prosecraft_core::screenplay::export::export_fountain(book),
                0,
            )),
            "html" => Ok((export_html(book, options), 0)),
            _ => Ok((export_text(book, options), 0)),
        };
        let error_id = ui.make_persistent_id("publish-export-error");
        match export {
            Ok((export, unshown)) => {
                // PDF's standard fonts show Western European text only (ProseCraft's too; see
                // jennraeross/ProseCraft#53): say what came out as "?".
                ui.data_mut(|d| {
                    if unshown > 0 {
                        d.insert_temp(
                            error_id,
                            format!(
                                "{unshown} character{} the PDF fonts can't show came out as “?”.",
                                if unshown == 1 { "" } else { "s" }
                            ),
                        );
                    } else {
                        d.remove_temp::<String>(error_id);
                    }
                });
                edit.requests.push(HostRequest::ExportFile {
                    file_name: export.file_name,
                    bytes: export.bytes,
                    kind: format.1.to_owned(),
                    extension: format.2.to_owned(),
                });
            }
            Err(error) => {
                ui.data_mut(|d| {
                    d.insert_temp(error_id, format!("Could not export: {error}."));
                });
            }
        }
    }
    if let Some(error) =
        ui.data(|d| d.get_temp::<String>(ui.make_persistent_id("publish-export-error")))
    {
        let response = ui.label(RichText::new(&error).size(11.0).color(palette.danger));
        let text = error.clone();
        ui.ctx().accesskit_node_builder(response.id, move |node| {
            node.set_role(egui::accesskit::Role::Alert);
            node.set_label(text);
        });
    }
    let note = if !ported {
        format!(
            "{} export isn't ported yet; PDF, EPUB, DOCX, HTML, Markdown, plain text, Fountain and Screenplay PDF are.",
            format.1
        )
    } else if book.scenes() == 0 {
        "Write a scene to export the manuscript.".to_owned()
    } else {
        String::new()
    };
    if !note.is_empty() {
        ui.label(RichText::new(note).size(11.0).color(palette.text_muted));
    }
    ui.add_space(24.0);
}

fn preview_pane(ui: &mut Ui, palette: &Palette, title: &str, text: &str) {
    eyebrow(ui, palette, "Preview");
    ui.label(
        RichText::new(title)
            .font(fonts::serif(20.0))
            .color(palette.text_heading),
    );
    ui.add_space(8.0);
    card(palette).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical()
            .id_salt("publish-preview")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let response = ui.add(
                    egui::Label::new(
                        RichText::new(text)
                            .font(fonts::serif(15.0))
                            .color(palette.text_body),
                    )
                    .wrap()
                    .selectable(true),
                );
                let label = "Manuscript preview".to_owned();
                ui.ctx()
                    .accesskit_node_builder(response.id, move |node| node.set_label(label));
            });
    });
}
