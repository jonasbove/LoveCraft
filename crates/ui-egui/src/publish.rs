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

    // Style.
    let style_label = STYLES
        .iter()
        .find(|s| s.0 == options.style)
        .map_or("Custom", |s| s.1);
    section_heading(ui, palette, "Style", style_label);
    ui.add_space(4.0);
    grid(ui, &STYLES, |ui, (id, label, detail), width| {
        if choice_card(ui, palette, options.style == id, label, "", detail, width)
            && editable
            && options.style != id
        {
            set(edit, "style", JsValue::str(id));
        }
    });
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
