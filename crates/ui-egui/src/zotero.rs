//! The Sources dialog's Zotero section (ProseCraft `CitationLibraryDialog.svelte`): whether
//! Zotero answers, a search of its library, Better BibTeX's picker, and adding what is found.
//!
//! Requests go through the transport the host gave ([`crate::ProseApp::set_zotero`]), on a
//! thread of their own (Zotero may take up to five seconds), and their answers are picked up
//! by the dialog's next frame. Without a transport (the web app) there is no Zotero section,
//! as ProseCraft shows none outside its desktop app.

use std::sync::{Arc, Mutex};

use egui::{RichText, Ui};
use prosecraft_core::citations::library::{
    format_item_contributors, item_year_text, merge_imported_citations, source_type_label,
};
use prosecraft_core::citations::zotero::{self, ErrorCode, ZoteroError};
use prosecraft_core::js::JsValue;

/// A request to Zotero: a path in, the body or the error message out.
pub type Transport = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

/// What a request brought back.
#[derive(Debug)]
enum Reply {
    Status(Result<(), ZoteroError>),
    Search(Result<Vec<JsValue>, ZoteroError>),
    Pick(Option<Vec<JsValue>>),
}

/// The answer to the request in flight, once it is there.
#[derive(Clone, Default)]
struct Pending(Arc<Mutex<Option<Reply>>>);

impl std::fmt::Debug for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pending")
    }
}

impl PartialEq for Pending {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// The section's state, kept with the dialog.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ZoteroPanel {
    started: bool,
    checking: bool,
    available: Option<bool>,
    query: String,
    searching: bool,
    results: Vec<JsValue>,
    has_searched: bool,
    error: Option<ZoteroError>,
    notice: Option<String>,
    picker_supported: bool,
    pending: Vec<Pending>,
}

impl Default for ZoteroPanel {
    fn default() -> Self {
        Self {
            started: false,
            checking: false,
            available: None,
            query: String::new(),
            searching: false,
            results: Vec::new(),
            has_searched: false,
            error: None,
            notice: None,
            picker_supported: true,
            pending: Vec::new(),
        }
    }
}

impl ZoteroPanel {
    fn ask(
        &mut self,
        ctx: &egui::Context,
        transport: &Transport,
        work: impl FnOnce(&dyn Fn(&str) -> Result<String, String>) -> Reply + Send + 'static,
    ) {
        let pending = Pending::default();
        self.pending.push(pending.clone());
        let transport = Arc::clone(transport);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let reply = work(&*transport);
            if let Ok(mut slot) = pending.0.lock() {
                *slot = Some(reply);
            }
            ctx.request_repaint();
        });
    }

    /// `checkZoteroConnection`.
    fn check(&mut self, ctx: &egui::Context, transport: &Transport) {
        self.checking = true;
        self.ask(ctx, transport, |t| Reply::Status(zotero::check_status(t)));
    }

    /// `searchZotero`.
    fn search(&mut self, ctx: &egui::Context, transport: &Transport) {
        self.searching = true;
        self.error = None;
        self.notice = None;
        self.has_searched = true;
        let query = self.query.clone();
        self.ask(ctx, transport, move |t| {
            Reply::Search(zotero::search(t, &query))
        });
    }

    /// `pickFromZotero`.
    fn pick(&mut self, ctx: &egui::Context, transport: &Transport) {
        self.searching = true;
        self.error = None;
        self.notice = None;
        self.ask(ctx, transport, |t| Reply::Pick(zotero::try_pick(t)));
    }

    /// Answers that arrived since the last frame.
    fn receive(&mut self) {
        let mut replies = Vec::new();
        self.pending.retain(|pending| {
            match pending.0.lock().ok().and_then(|mut slot| slot.take()) {
                Some(reply) => {
                    replies.push(reply);
                    false
                }
                None => true,
            }
        });
        for reply in replies {
            match reply {
                Reply::Status(status) => {
                    self.checking = false;
                    self.available = Some(status.is_ok());
                    match status {
                        Ok(()) => {
                            if self
                                .error
                                .as_ref()
                                .is_some_and(|e| e.code == ErrorCode::NotRunning)
                            {
                                self.error = None;
                            }
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Reply::Search(found) => {
                    self.searching = false;
                    match found {
                        Ok(items) => {
                            self.results = items;
                            self.available = Some(true);
                        }
                        Err(error) => {
                            if error.code == ErrorCode::NotRunning {
                                self.available = Some(false);
                            }
                            self.error = Some(error);
                            self.results.clear();
                        }
                    }
                }
                Reply::Pick(picked) => {
                    self.searching = false;
                    match picked {
                        None => self.picker_supported = false,
                        Some(items) => {
                            self.available = Some(true);
                            if !items.is_empty() {
                                self.results = items;
                                self.has_searched = true;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn text(item: &JsValue, key: &str) -> String {
    item.get(key)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

/// The section. Returns the library with an item added, when one was.
pub(crate) fn show(
    ui: &mut Ui,
    palette: &crate::theme::Palette,
    panel: &mut ZoteroPanel,
    transport: &Transport,
    items: &[JsValue],
    editable: bool,
) -> Option<Vec<JsValue>> {
    panel.receive();
    if !panel.started {
        panel.started = true;
        panel.check(ui.ctx(), transport);
    }
    let mut change = None;
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Zotero").strong().size(15.0));
        let status = if panel.checking {
            Some(("Checking…", palette.text_muted))
        } else {
            match panel.available {
                Some(true) => Some(("Connected", palette.accent_strong)),
                Some(false) => Some(("Not reachable", palette.danger)),
                None => None,
            }
        };
        if let Some((label, color)) = status {
            ui.label(RichText::new(label).size(11.0).color(color));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!panel.checking, egui::Button::new("Check again"))
                .clicked()
            {
                panel.check(ui.ctx(), transport);
            }
        });
    });
    if panel.available == Some(false) {
        ui.add(
            egui::Label::new(
                RichText::new("Zotero is not reachable on this computer. Start Zotero, and make sure “Allow other applications on this computer to communicate with Zotero” is enabled in Zotero Settings → Advanced → Files and Folders.")
                    .size(12.0)
                    .color(palette.text_muted),
            )
            .wrap(),
        );
    }
    ui.horizontal(|ui| {
        let field = ui.add(
            egui::TextEdit::singleline(&mut panel.query)
                .hint_text("Search Zotero library…")
                .desired_width(280.0),
        );
        crate::views::name_field(ui, &field, "Search Zotero");
        let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let label = if panel.searching {
            "Searching…"
        } else {
            "Search"
        };
        let search = ui.add_enabled(!panel.searching, egui::Button::new(label));
        if search.clicked() || (submitted && !panel.searching) {
            panel.search(ui.ctx(), transport);
        }
        if panel.picker_supported
            && ui
                .add_enabled(!panel.searching, egui::Button::new("Pick in Zotero…"))
                .clicked()
        {
            panel.pick(ui.ctx(), transport);
        }
    });
    if let Some(error) = &panel.error
        && (panel.available != Some(false)
            || matches!(error.code, ErrorCode::Forbidden | ErrorCode::Timeout))
    {
        ui.add(egui::Label::new(RichText::new(&error.message).color(palette.danger)).wrap());
    }
    if let Some(notice) = &panel.notice {
        ui.label(RichText::new(notice).color(palette.accent_strong));
    }
    if !panel.results.is_empty() {
        let in_project: Vec<String> = items.iter().map(|i| text(i, "id")).collect();
        for item in &panel.results {
            let id = text(item, "id");
            let title = text(item, "title");
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let shown = if title.is_empty() {
                            "(Untitled)"
                        } else {
                            &title
                        };
                        ui.label(RichText::new(shown).strong());
                        ui.label(
                            RichText::new(source_type_label(&text(item, "type")))
                                .size(10.0)
                                .color(palette.text_muted),
                        );
                    });
                    let contributors = format_item_contributors(item);
                    let year = item_year_text(item);
                    let key = item
                        .get("citeKey")
                        .and_then(JsValue::as_js_string)
                        .map_or_else(|| id.clone(), |k| k.to_string_lossy());
                    let meta = [
                        if contributors.is_empty() {
                            "Unknown author".to_owned()
                        } else {
                            contributors
                        },
                        if year.is_empty() {
                            "n.d.".to_owned()
                        } else {
                            year
                        },
                        format!("@{key}"),
                    ];
                    ui.label(
                        RichText::new(meta.join(" · "))
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if in_project.contains(&id) {
                        ui.label(
                            RichText::new("In project")
                                .size(11.0)
                                .color(palette.text_muted),
                        );
                    } else if ui
                        .add_enabled(editable, egui::Button::new("Add to this project"))
                        .clicked()
                    {
                        // `addZoteroItem`.
                        let merged = merge_imported_citations(items, std::slice::from_ref(item));
                        if merged.added_count > 0 {
                            change = Some(merged.items);
                            let name = if title.is_empty() { &id } else { &title };
                            panel.notice = Some(format!("Added “{name}” to this project."));
                        }
                    }
                });
            });
            ui.separator();
        }
    } else if panel.has_searched && !panel.searching && panel.error.is_none() {
        ui.label(
            RichText::new(format!("No Zotero items matched “{}”.", panel.query))
                .color(palette.text_muted),
        );
    }
    change
}
