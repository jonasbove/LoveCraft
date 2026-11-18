//! Split view (ProseCraft's `SplitPane.svelte`): reference panes beside the editor, each showing
//! a scene (its whole chapter), a wiki page or a wiki file: text and Markdown, an image, a PDF's
//! pages (drawn with craft-pdf), or a note with "Open externally".

use std::sync::Arc;

use egui::{RichText, Ui};
use prosecraft_core::project::Project;
use prosecraft_core::split::{self, Pane, Target};

use crate::Selection;
use crate::theme::Palette;
use crate::wiki_files::WikiFileStore;

/// A new pane's id.
pub(crate) fn pane_id() -> String {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
    format!(
        "pane-{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

/// `MAX_TEXT_BYTES`, `MAX_IMAGE_BYTES`, `MAX_PDF_BYTES`.
const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_PDF_BYTES: u64 = 50 * 1024 * 1024;

/// What a file pane shows.
#[derive(Clone)]
enum Content {
    Html(String),
    Image(egui::TextureHandle),
    Pages(Vec<egui::TextureHandle>),
    Note(String),
}

/// The wiki file store and where the project's files are.
pub(crate) struct Files<'a> {
    pub store: Option<&'a dyn WikiFileStore>,
    pub root: Option<String>,
    pub folder: String,
    /// The project's citation style and locale, for scenes' citations.
    pub citation_style: String,
    pub citation_locale: String,
}

/// `splitGroups`: each chapter's scenes, the wiki pages, the wiki files.
fn groups(project: &Project) -> Vec<(String, Vec<(String, String)>)> {
    let mut out = Vec::new();
    for chapter in project.chapters.iter().filter(|c| !c.trashed) {
        let options: Vec<(String, String)> = project
            .documents
            .iter()
            .filter(|d| d.chapter_id.as_deref() == Some(&chapter.id) && !d.trashed)
            .map(|d| (split::doc_target(&d.id), title_or(&d.title, "Untitled")))
            .collect();
        if !options.is_empty() {
            out.push((title_or(&chapter.title, "Untitled chapter"), options));
        }
    }
    let wiki: Vec<(String, String)> = project
        .documents
        .iter()
        .filter(|d| d.space.as_deref() == Some("Wiki") && !d.trashed)
        .map(|d| (split::doc_target(&d.id), title_or(&d.title, "Untitled")))
        .collect();
    if !wiki.is_empty() {
        out.push(("Wiki pages".into(), wiki));
    }
    let files: Vec<(String, String)> = project
        .research_items
        .iter()
        .filter(|r| !r.link)
        .map(|r| (split::file_target(&r.id), r.item.title().to_owned()))
        .collect();
    if !files.is_empty() {
        out.push(("Wiki files".into(), files));
    }
    out
}

fn title_or(title: &str, fallback: &str) -> String {
    if title.is_empty() {
        fallback.to_owned()
    } else {
        title.to_owned()
    }
}

/// A PDF's pages, `width` pixels wide.
fn pdf_pages(
    ctx: &egui::Context,
    bytes: Vec<u8>,
    width: f32,
) -> Result<Vec<egui::TextureHandle>, String> {
    let doc = craft_pdf::open(Arc::new(bytes), None)
        .map_err(|_| "Could not open that PDF.".to_owned())?;
    let cache = craft_pdf::raster::RenderCache::new();
    let settings = craft_pdf::interpret::InterpreterSettings::default();
    let mut out = Vec::new();
    for (index, page) in doc.pages().iter().enumerate().take(200) {
        let (w, _) = page.render_dimensions();
        let scale = if w > 0.0 { (width * 2.0) / w } else { 1.0 };
        let options = craft_pdf::raster::Options {
            scale: [scale, scale],
            background: [255, 255, 255, 255],
            ..Default::default()
        };
        let raster = craft_pdf::raster::render(page, &cache, &settings, &options)
            .map_err(|_| "Could not draw that PDF.".to_owned())?;
        let image = egui::ColorImage::from_rgba_premultiplied(
            [raster.width as usize, raster.height as usize],
            raster.bytes(),
        );
        out.push(ctx.load_texture(
            format!("pdf-page-{index}"),
            image,
            egui::TextureOptions::LINEAR,
        ));
    }
    Ok(out)
}

/// The file's content for its pane, loaded once.
fn load(
    ctx: &egui::Context,
    files: &Files<'_>,
    item: &prosecraft_core::research::Item,
    width: f32,
) -> Content {
    use prosecraft_core::research::Item;
    let Item::File {
        id,
        file_name,
        mime_type,
        size,
        storage_key,
        ..
    } = item
    else {
        return Content::Note("This file type cannot be shown inside ProseCraft yet.".into());
    };
    let key = egui::Id::unique(("split-file", id.as_str(), *size as u64));
    if let Some(found) = ctx.data(|d| d.get_temp::<Content>(key)) {
        return found;
    }
    let name = file_name.to_lowercase();
    let pdf = mime_type == "application/pdf" || name.ends_with(".pdf");
    let markdown =
        mime_type == "text/markdown" || name.ends_with(".md") || name.ends_with(".markdown");
    let text = markdown || mime_type == "text/plain" || name.ends_with(".txt");
    let image = mime_type.starts_with("image/");
    let read = |max: u64| -> Result<Vec<u8>, String> {
        let (Some(store), Some(root)) = (files.store, files.root.as_deref()) else {
            return Err("Wiki files are shown in the desktop app.".into());
        };
        store.read(root, &files.folder, storage_key, max)
    };
    let content = if pdf {
        if *size > MAX_PDF_BYTES as f64 {
            Content::Note("This PDF is too large to show here (over 50 MB).".into())
        } else {
            read(MAX_PDF_BYTES)
                .and_then(|bytes| pdf_pages(ctx, bytes, width))
                .map_or_else(Content::Note, Content::Pages)
        }
    } else if text && *size <= MAX_TEXT_BYTES as f64 {
        read(MAX_TEXT_BYTES).map_or_else(Content::Note, |bytes| {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            Content::Html(if markdown {
                prosecraft_core::markdown::markdown_to_html(&text)
            } else {
                format!(
                    "<pre>{}</pre>",
                    text.replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;")
                )
            })
        })
    } else if image && *size <= MAX_IMAGE_BYTES as f64 {
        read(MAX_IMAGE_BYTES)
            .and_then(|bytes| {
                craft_image_codecs::decode(&bytes)
                    .map_err(|_| "Could not open that file.".to_owned())
            })
            .map_or_else(Content::Note, |image| {
                let pixels = crate::pictures::color_image(ctx, &image);
                Content::Image(ctx.load_texture(
                    format!("split-image-{id}"),
                    pixels,
                    egui::TextureOptions::LINEAR,
                ))
            })
    } else {
        Content::Note("This file type cannot be shown inside ProseCraft yet.".into())
    };
    ctx.data_mut(|d| d.insert_temp(key, content.clone()));
    content
}

/// A pane's header button with ProseCraft's label and tip.
fn header_button(ui: &mut Ui, text: &str, label: &str, tip: &str, enabled: bool) -> bool {
    let button = ui
        .add_enabled(enabled, egui::Button::new(text).small())
        .on_hover_text(tip);
    crate::views::name_field(ui, &button, label);
    button.clicked()
}

/// What the panes asked for.
pub(crate) struct Asked {
    /// A scene to open in the editor (Swap).
    pub open: Option<Selection>,
    /// A wiki file to open with its app.
    pub open_file: Option<String>,
}

/// The panes, as a grid of `tileColumns` columns.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    panes: &mut Vec<Pane>,
    active: Option<&str>,
    separator: prosecraft_core::publish::SceneSeparator,
    files: &Files<'_>,
    new_id: &mut dyn FnMut() -> String,
) -> Asked {
    let mut asked = Asked {
        open: None,
        open_file: None,
    };
    let groups = groups(project);
    // `pruneSplitTargets`.
    let valid: std::collections::HashSet<String> = groups
        .iter()
        .flat_map(|(_, o)| o.iter().map(|(v, _)| v.clone()))
        .collect();
    *panes = split::prune(panes, &valid);
    let total = ui.max_rect().width();
    let columns = split::tile_columns(panes.len(), f64::from(total), 300.0);
    let gap = ui.spacing().item_spacing.x;
    let width = ((total - gap * (columns as f32 - 1.0)) / columns as f32 - 1.0).max(120.0);
    let body_height =
        (ui.available_height() / panes.len().div_ceil(columns).max(1) as f32 - 70.0).max(160.0);
    let snapshot = panes.clone();
    let mut next = None;
    egui::ScrollArea::vertical()
        .id_salt("split-panes")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for row in snapshot.chunks(columns) {
                ui.horizontal_top(|ui| {
                    for pane in row {
                        let index = snapshot.iter().position(|p| p.id == pane.id).unwrap_or(0);
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            egui::Frame::new()
                                .fill(palette.control_bg)
                                .stroke(egui::Stroke::new(1.0, palette.border))
                                .corner_radius(9)
                                .inner_margin(8)
                                .show(ui, |ui| {
                                    ui.set_width(width - 18.0);
                                    if let Some(changed) =
                                        header(ui, &groups, &snapshot, pane, index, active, new_id)
                                    {
                                        next = Some(changed);
                                    }
                                    ui.separator();
                                    body(
                                        ui,
                                        palette,
                                        project,
                                        pane,
                                        active,
                                        separator,
                                        files,
                                        (width - 30.0, body_height),
                                        &mut asked,
                                    );
                                });
                        });
                    }
                });
                ui.add_space(8.0);
            }
        });
    if let Some((changed, swap)) = next {
        *panes = changed;
        if swap.is_some() {
            asked.open = swap;
        }
    }
    asked
}

type Change = (Vec<Pane>, Option<Selection>);

fn header(
    ui: &mut Ui,
    groups: &[(String, Vec<(String, String)>)],
    panes: &[Pane],
    pane: &Pane,
    index: usize,
    active: Option<&str>,
    new_id: &mut dyn FnMut() -> String,
) -> Option<Change> {
    let mut change = None;
    ui.horizontal(|ui| {
        let current = groups
            .iter()
            .flat_map(|(_, o)| o.iter())
            .find(|(v, _)| *v == pane.target)
            .map_or("Choose something to show…".to_owned(), |(_, l)| l.clone());
        let mut target = pane.target.clone();
        let combo = craft_ui::ComboBox::from_id_salt(("split-target", pane.id.as_str()))
            .selected_text(current)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut target, String::new(), "Choose something to show…");
                for (group, options) in groups {
                    ui.label(RichText::new(group).strong().size(11.0));
                    for (value, label) in options {
                        ui.selectable_value(&mut target, value.clone(), format!("  {label}"));
                    }
                }
            });
        ui.ctx()
            .accesskit_node_builder(combo.response.id, |n| n.set_label("Show in this pane"));
        if target != pane.target {
            change = Some((split::set_target(panes, &pane.id, &target), None));
        }
    });
    ui.horizontal(|ui| {
        let doc = match split::parse_target(&pane.target) {
            Some(Target::Doc(id)) => Some(id),
            _ => None,
        };
        let can_swap = doc.is_some() && active.is_some();
        if header_button(
            ui,
            "⇄",
            "Edit this in the main editor and show the current text here",
            "Swap with the editor",
            can_swap,
        ) && let (Some(doc), Some(active)) = (doc, active)
        {
            change = Some((
                split::set_target(panes, &pane.id, &split::doc_target(active)),
                Some(Selection::Document(doc)),
            ));
        }
        if header_button(ui, "←", "Move this pane left", "Move left", index > 0) {
            change = Some((split::move_pane(panes, &pane.id, -1), None));
        }
        if header_button(
            ui,
            "→",
            "Move this pane right",
            "Move right",
            index + 1 < panes.len(),
        ) {
            change = Some((split::move_pane(panes, &pane.id, 1), None));
        }
        let can_add = split::can_add(panes);
        let tip = if can_add {
            "Add another pane"
        } else {
            "Up to five reference panes"
        };
        if header_button(ui, "+", "Add another pane", tip, can_add) {
            change = Some((split::add(panes, &new_id()), None));
        }
        if header_button(ui, "×", "Close this pane", "Close this pane", true) {
            change = Some((split::remove(panes, &pane.id), None));
        }
    });
    change
}

#[allow(clippy::too_many_arguments)]
fn body(
    ui: &mut Ui,
    palette: &Palette,
    project: &Project,
    pane: &Pane,
    active: Option<&str>,
    separator: prosecraft_core::publish::SceneSeparator,
    files: &Files<'_>,
    (width, height): (f32, f32),
    asked: &mut Asked,
) {
    let note = |ui: &mut Ui, text: &str| {
        ui.label(RichText::new(text).size(11.0).color(palette.text_muted));
    };
    egui::ScrollArea::vertical()
        .id_salt(("split-body", pane.id.as_str()))
        .max_height(height)
        .auto_shrink([false, true])
        .show(ui, |ui| match split::parse_target(&pane.target) {
            Some(Target::Doc(id)) => {
                let Some(doc) = project.documents.iter().find(|d| d.id == id && !d.trashed) else {
                    note(
                        ui,
                        "Pick a scene, wiki page or file above to read it next to your text.",
                    );
                    return;
                };
                // A scene shows its whole chapter.
                let chapter = doc
                    .chapter_id
                    .as_ref()
                    .and_then(|c| project.chapters.iter().find(|ch| ch.id == *c));
                let (title, html) = match chapter {
                    Some(chapter) => {
                        let scenes: Vec<String> = project
                            .documents
                            .iter()
                            .filter(|d| d.chapter_id.as_deref() == Some(&chapter.id) && !d.trashed)
                            .map(|d| d.content.clone())
                            .collect();
                        (
                            title_or(&chapter.title, "Untitled chapter"),
                            prosecraft_core::chapter::build_chapter_html(&scenes, separator),
                        )
                    }
                    None => (title_or(&doc.title, "Untitled"), doc.content.clone()),
                };
                ui.label(
                    RichText::new(title)
                        .strong()
                        .size(15.0)
                        .color(palette.text_heading),
                );
                if active == Some(doc.id.as_str()) {
                    note(ui, "Open in the editor");
                }
                crate::views::read_only_html_cited(
                    ui,
                    palette,
                    &html,
                    "Pane text",
                    &project.citations,
                    (&files.citation_style, &files.citation_locale),
                );
            }
            Some(Target::File(id)) => {
                let Some(item) = project.research_items.iter().find(|r| r.id == id) else {
                    note(
                        ui,
                        "Pick a scene, wiki page or file above to read it next to your text.",
                    );
                    return;
                };
                ui.label(
                    RichText::new(item.item.title())
                        .strong()
                        .size(15.0)
                        .color(palette.text_heading),
                );
                match load(ui.ctx(), files, &item.item, width) {
                    Content::Html(html) => {
                        crate::views::read_only_html(ui, palette, &html, "Pane text")
                    }
                    Content::Image(texture) => {
                        let size = texture.size_vec2();
                        let fit = (width / size.x).min(1.0);
                        let image = ui.add(egui::Image::new((texture.id(), size * fit)));
                        crate::views::name_field(ui, &image, item.item.title());
                    }
                    Content::Pages(pages) => {
                        for (number, page) in pages.iter().enumerate() {
                            let size = page.size_vec2();
                            let fit = width / size.x;
                            let image = ui.add(egui::Image::new((page.id(), size * fit)));
                            crate::views::name_field(ui, &image, &format!("Page {}", number + 1));
                            ui.add_space(8.0);
                        }
                    }
                    Content::Note(text) => {
                        note(ui, &text);
                        if ui.button("Open externally").clicked() {
                            asked.open_file = Some(id.clone());
                        }
                    }
                }
            }
            None => note(
                ui,
                "Pick a scene, wiki page or file above to read it next to your text.",
            ),
        });
}
