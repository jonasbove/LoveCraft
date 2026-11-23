//! LoveCraft in the browser. Build with `trunk build --release` from this folder.
//!
//! The library is kept in the browser (IndexedDB), files are chosen with the browser's file
//! picker and exports are downloads. Folders can't be opened from a web page, so the folder
//! imports and folder sync say so.
//!
//! Framed by a Craft host (CraftCloud, `?host=parent`), it edits the file the host sends instead
//! and saves it back there ([`hosted`]).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
mod clipboard;
#[cfg(any(target_arch = "wasm32", test))]
mod hosted;
#[cfg(any(target_arch = "wasm32", test))]
mod html_slot;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use eframe::wasm_bindgen::JsCast;
    use eframe::wasm_bindgen::closure::Closure;
    use prosecraft_core::library::{Library, new_device_id};
    use prosecraft_ui_egui::{HostRequest, ProseApp};

    use crate::browser::{self, BrowserRandom, BrowserStore, Inbox};
    use crate::hosted::{self, Bridge, Effect};

    /// What a chosen file is for.
    enum Then {
        Open,
        Epub,
        ScrivenerZip,
        Fountain,
        Citations,
        /// A picture for the open scene.
        Picture,
        /// The book's cover image.
        Cover,
        /// A file dropped on the page: a picture goes into the open scene, an EPUB is imported,
        /// anything else opened as a backup.
        Dropped,
    }

    /// The page's app, shared with the listeners that save when the page goes away.
    struct WebApp {
        page: Rc<RefCell<Page>>,
    }

    struct Page {
        app: ProseApp,
        inbox: Inbox<Then>,
        /// The Craft host framing the app, when there is one.
        hosted: Option<Hosted>,
    }

    /// What the host sent, read into memory.
    enum FromHost {
        Open {
            name: String,
            bytes: Vec<u8>,
        },
        /// A file that couldn't be read (too large, or the browser failed to read it).
        Refused(String),
        Saved(String),
    }

    /// The connection to the host and the hosted file's session.
    struct Hosted {
        host: craft_web_host::web::Host,
        bridge: Bridge,
        messages: Rc<RefCell<VecDeque<FromHost>>>,
        /// This browser's id in the changes it makes (its library's, or a new one).
        device_id: String,
    }

    impl Hosted {
        /// Hears the host, then tells it the app is ready for a file.
        fn start(
            host: craft_web_host::web::Host,
            device_id: String,
            ctx: &eframe::egui::Context,
        ) -> Self {
            let messages: Rc<RefCell<VecDeque<FromHost>>> = Rc::default();
            let queue = Rc::clone(&messages);
            let repaint = ctx.clone();
            host.listen(move |message| hear(message, &queue, &repaint));
            // Unsent, the host never sends a file; there is no one else to tell.
            let _ = host.post_ready(env!("CARGO_PKG_VERSION"), false);
            Self {
                host,
                bridge: Bridge::new(&session_id()),
                messages,
                device_id,
            }
        }

        /// The host's messages since the last frame. An opened file replaces the app.
        fn receive(&mut self, app: &mut ProseApp, ctx: &eframe::egui::Context) {
            let messages: Vec<_> = self.messages.borrow_mut().drain(..).collect();
            for message in messages {
                match message {
                    FromHost::Open { name, bytes } => {
                        let device = Box::new(BrowserStore::new());
                        match self
                            .bridge
                            .open(&name, &bytes, self.device_id.clone(), device)
                        {
                            Ok(opened) => *app = prepare(opened, ctx, true),
                            Err(error) => app.report(&error),
                        }
                    }
                    FromHost::Refused(error) => {
                        app.report(&error);
                        self.bridge.refuse(error);
                    }
                    FromHost::Saved(id) => self.bridge.saved(&id),
                }
            }
        }

        /// After the frame: saves, the unsaved state and failures go to the host.
        fn send(&mut self, app: &mut ProseApp, ctx: &eframe::egui::Context) {
            let now = ctx.input(|i| i.time);
            self.bridge.tick(now, app);
            self.post(app);
            if let Some(seconds) = self.bridge.wake_in(now) {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(seconds.max(0.1)));
            }
        }

        /// What the session asks for, posted to the host.
        fn post(&mut self, app: &mut ProseApp) {
            for effect in self.bridge.take_effects() {
                match effect {
                    Effect::Save { name, bytes, id } => {
                        if let Err(error) = self.host.post_save(&name, &bytes, Some(&id), false) {
                            self.bridge
                                .not_sent(&id, String::from_utf8_lossy(&bytes).into_owned());
                            app.report(&format!("{name} couldn't be sent to be saved: {error}"));
                        }
                    }
                    Effect::Dirty(dirty) => self.host.post_dirty(dirty),
                    Effect::Failed { error, id } => {
                        let _ = self.host.post_failed(&error, id.as_deref());
                    }
                }
            }
        }
    }

    /// A random name for this page's saves, so a `saved` meant for an earlier page (before a
    /// reload) never confirms one of this page's.
    fn session_id() -> String {
        let mut bytes = [0u8; 6];
        let _ =
            prosecraft_core::storage::crypto::RandomSource::fill(&mut BrowserRandom, &mut bytes);
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// When the page is hidden or goes away (`visibilitychange`, `pagehide`), what was typed is
    /// sent to the host at once: a hidden page may get no more frames.
    fn save_when_leaving(page: &Rc<RefCell<Page>>, ctx: &eframe::egui::Context) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Some(document) = window.document() else {
            return;
        };
        let (shared, ctx) = (Rc::clone(page), ctx.clone());
        let watched = document.clone();
        let leave = Closure::<dyn Fn(web_sys::Event)>::new(move |event: web_sys::Event| {
            if event.type_() == "visibilitychange" && !watched.hidden() {
                return;
            }
            if let Ok(mut page) = shared.try_borrow_mut() {
                page.leaving(&ctx);
            }
        });
        let listener = leave.as_ref().unchecked_ref();
        let heard = document
            .add_event_listener_with_callback("visibilitychange", listener)
            .is_ok()
            | window
                .add_event_listener_with_callback("pagehide", listener)
                .is_ok();
        if heard {
            // Listening for the page's life.
            leave.forget();
        }
    }

    /// A host message for the session, its file read into memory first (never one far too large
    /// to be a backup).
    fn hear(
        message: craft_web_host::web::Message,
        queue: &Rc<RefCell<VecDeque<FromHost>>>,
        ctx: &eframe::egui::Context,
    ) {
        use craft_web_host::web::{FileData, Message};
        let heard = match message {
            Message::Open { name, data } => {
                if let Some(why) = Bridge::too_large(&name, data.size()) {
                    FromHost::Refused(why)
                } else {
                    match data {
                        FileData::Bytes(array) => FromHost::Open {
                            name,
                            bytes: array.to_vec(),
                        },
                        FileData::Blob(blob) => {
                            let (queue, ctx) = (Rc::clone(queue), ctx.clone());
                            wasm_bindgen_futures::spawn_local(async move {
                                let heard =
                                    match wasm_bindgen_futures::JsFuture::from(blob.array_buffer())
                                        .await
                                    {
                                        Ok(buffer) => FromHost::Open {
                                            name,
                                            bytes: js_sys::Uint8Array::new(&buffer).to_vec(),
                                        },
                                        Err(_) => FromHost::Refused(format!(
                                            "Couldn't open {name}: the browser couldn't read it."
                                        )),
                                    };
                                queue.borrow_mut().push_back(heard);
                                ctx.request_repaint();
                            });
                            return;
                        }
                    }
                }
            }
            Message::Saved { id } => FromHost::Saved(id),
            // No PDFs to attach, and nothing is handed off.
            Message::Host { .. } | Message::AttachPdf { .. } | Message::HandoffDone { .. } => {
                return;
            }
        };
        queue.borrow_mut().push_back(heard);
        ctx.request_repaint();
    }

    /// The browser's parts of an app: randomness, the About window's platform, the clipboard.
    fn prepare(mut app: ProseApp, ctx: &eframe::egui::Context, clipboard: bool) -> ProseApp {
        // Keys and nonces for encrypted backups: the browser's `crypto.getRandomValues`.
        app.set_random(|| Box::new(BrowserRandom));
        // The About window's diagnostics: the browser's engine and system, never more of its
        // user agent.
        let agent = web_sys::window()
            .and_then(|w| w.navigator().user_agent().ok())
            .unwrap_or_default();
        app.set_platform(prosecraft_ui_egui::Platform {
            os: prosecraft_core::about::summarize_user_agent(&agent),
            storage: "IndexedDB".into(),
        });
        if clipboard {
            // Without it, paste and copy in the editor are plain text.
            let _ = crate::clipboard::install(&mut app, ctx);
        }
        app
    }

    const NO_FOLDERS: &str = "Folders can't be opened from a web page. Import a zipped Scrivener project, or use the desktop app.";

    /// A file's media type from its extension, for the download.
    fn media_type(extension: &str) -> &'static str {
        match extension {
            "pdf" => "application/pdf",
            "epub" => "application/epub+zip",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "html" => "text/html",
            "md" => "text/markdown",
            "txt" | "fountain" => "text/plain",
            _ => "application/octet-stream",
        }
    }

    impl Page {
        fn receive(&mut self) {
            let chosen: Vec<_> = self.inbox.borrow_mut().drain(..).collect();
            for file in chosen {
                // `file.text()`: UTF-8, a byte-order mark dropped, anything else as U+FFFD.
                let text = || {
                    let text = String::from_utf8_lossy(&file.bytes).into_owned();
                    text.strip_prefix('\u{feff}')
                        .map(str::to_owned)
                        .unwrap_or(text)
                };
                match file.then {
                    Then::Open => self.app.open_backup_bytes(&file.name, &file.bytes),
                    Then::Epub => self.app.import_epub(&file.name, &file.bytes),
                    Then::ScrivenerZip => self.app.import_scrivener_zip(&file.bytes),
                    Then::Fountain => self.app.import_fountain(&file.name, &text()),
                    Then::Citations => self.app.import_citation_file(&file.name, &text()),
                    Then::Picture => self.app.insert_picture(&file.name, &file.bytes),
                    Then::Cover => self.app.set_cover_image(&file.name, &file.bytes),
                    Then::Dropped
                        if prosecraft_core::picture::is_supported(&file.name)
                            && self.app.picture_wanted() =>
                    {
                        self.app.insert_picture(&file.name, &file.bytes);
                    }
                    Then::Dropped if file.name.to_lowercase().ends_with(".epub") => {
                        self.app.import_epub(&file.name, &file.bytes);
                    }
                    Then::Dropped => self.app.open_backup_bytes(&file.name, &file.bytes),
                }
            }
        }

        /// The page is hidden or going away: the hosted file is sent now.
        fn leaving(&mut self, ctx: &eframe::egui::Context) {
            if let Some(hosted) = self.hosted.as_mut() {
                let now = ctx.input(|i| i.time);
                hosted.bridge.leaving(now, &mut self.app);
                hosted.post(&mut self.app);
            }
        }

        fn answer(&mut self, ctx: &eframe::egui::Context) {
            for request in self.app.take_requests() {
                let inbox = &self.inbox;
                let choose = |accept: &str, then: Then| {
                    browser::choose_file(accept, then, inbox, ctx);
                    Ok(())
                };
                let saved = match request {
                    HostRequest::OpenBackup => choose(".prosecraft,.json", Then::Open),
                    HostRequest::ImportEpub => choose(".epub", Then::Epub),
                    HostRequest::ImportScrivenerZip => choose(".zip", Then::ScrivenerZip),
                    HostRequest::ImportFountain => choose(".fountain,.txt", Then::Fountain),
                    HostRequest::InsertPicture => {
                        choose(".png,.jpg,.jpeg,.webp,.gif,.bmp,.avif", Then::Picture)
                    }
                    HostRequest::ImportCitations => {
                        choose(".bib,.bibtex,.ris,.json,.csljson", Then::Citations)
                    }
                    HostRequest::ChooseCover => choose(".png,.jpg,.jpeg,.webp", Then::Cover),
                    HostRequest::OpenUrl(url) => {
                        ctx.open_url(eframe::egui::OpenUrl::new_tab(url));
                        Ok(())
                    }
                    // Wiki files need a folder on the device (`set_wiki_files` isn't called here).
                    HostRequest::ChooseWikiRoot { .. } | HostRequest::PickWikiFiles { .. } => {
                        Ok(())
                    }
                    // CraftCloud is the desktop's (`set_craftcloud` isn't called here).
                    HostRequest::OpenBackupPath(_) => Ok(()),
                    HostRequest::ImportScrivenerFolder
                    | HostRequest::ImportMarkdownFolder
                    | HostRequest::ChooseSyncFolder => {
                        self.app.report(NO_FOLDERS);
                        Ok(())
                    }
                    HostRequest::ExportBackup { file_name, text } => {
                        browser::download(&file_name, text.as_bytes(), "application/json")
                    }
                    HostRequest::ExportFile {
                        file_name,
                        bytes,
                        extension,
                        ..
                    } => browser::download(&file_name, &bytes, media_type(&extension)),
                };
                if saved.is_err() {
                    self.app.report("The browser didn't start the download.");
                }
            }
        }
    }

    impl eframe::App for WebApp {
        fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
            // Only a page-leave listener could hold it, and it never runs inside a frame.
            if let Ok(mut page) = self.page.try_borrow_mut() {
                page.frame(ui);
            }
        }
    }

    impl Page {
        fn frame(&mut self, ui: &mut eframe::egui::Ui) {
            let dropped = ui.ctx().input(|input| input.raw.dropped_files.clone());
            for file in dropped {
                browser::read_dropped(file, Then::Dropped, &self.inbox, ui.ctx());
            }
            self.receive();
            if let Some(hosted) = self.hosted.as_mut() {
                hosted.receive(&mut self.app, ui.ctx());
                hosted.bridge.keys(ui.ctx(), &mut self.app);
            }
            eframe::egui::CentralPanel::default().show(ui, |ui| self.app.show(ui));
            self.answer(ui.ctx());
            if let Some(hosted) = self.hosted.as_mut() {
                hosted.send(&mut self.app, ui.ctx());
            }
        }
    }

    /// The app with the library this browser keeps, or why it runs without one. A library that
    /// is there but can't be read is never written over.
    async fn start_app() -> ProseApp {
        let library = match browser::load_library().await {
            Ok(Some(text)) => Library::from_text(&text).map_err(|e| {
                format!(
                    "The library this browser keeps couldn't be read ({e}); it is left untouched."
                )
            }),
            Ok(None) => new_device_id(&mut BrowserRandom)
                .map(Library::new)
                .map_err(|e| format!("Couldn't start a library: {e}")),
            Err(e) => Err(format!(
                "This browser can't keep a library ({}); open a backup to work, and export one to keep your changes.",
                e.as_string()
                    .unwrap_or_else(|| "IndexedDB is unavailable".into())
            )),
        };
        match library {
            Ok(library) => ProseApp::with_library(library, Box::new(BrowserStore::new())),
            Err(why) => {
                let mut app = ProseApp::new();
                app.report(&why);
                app
            }
        }
    }

    /// This browser's id for the changes it makes to hosted files, kept in its settings
    /// (`localStorage`; the browser's library isn't opened while hosted).
    fn device_id() -> String {
        use prosecraft_ui_egui::LibraryStore as _;
        const KEY: &str = "hosted-device-id";
        let mut settings = BrowserStore::new();
        if let Some(id) = settings.load_setting(KEY).filter(|id| !id.is_empty()) {
            return id;
        }
        let id = new_device_id(&mut BrowserRandom)
            .unwrap_or_else(|_| format!("browser-{}", js_sys::Date::now()));
        // Unkept, the next session takes a new id; changes still merge by time and revision.
        let _ = settings.save_setting(KEY, Some(&id));
        id
    }

    pub fn start() {
        wasm_bindgen_futures::spawn_local(async {
            let Some(canvas) = web_sys::window()
                .and_then(|window| window.document())
                .and_then(|document| document.get_element_by_id("prosecraft_canvas"))
                .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            else {
                return;
            };
            // Framed by a Craft host, the app waits for the host's file and the browser's
            // library stays closed. Without one (no `?host=parent` or `data-host="page"`, or a
            // parent on another origin), nothing changes.
            let host = craft_web_host::web::Host::connect(hosted::APP_ID, &canvas);
            let (app, device_id) = match host {
                Some(_) => (ProseApp::new(), device_id()),
                None => (start_app().await, String::new()),
            };
            let _ = eframe::WebRunner::new()
                .start(
                    canvas,
                    eframe::WebOptions::default(),
                    Box::new(move |cc| {
                        prosecraft_ui_egui::setup(&cc.egui_ctx);
                        let hosted = host.map(|host| Hosted::start(host, device_id, &cc.egui_ctx));
                        // Hosted, the clipboard is installed for the opened file's app.
                        let app = prepare(app, &cc.egui_ctx, hosted.is_none());
                        let hosting = hosted.is_some();
                        let page = Rc::new(RefCell::new(Page {
                            app,
                            inbox: Default::default(),
                            hosted,
                        }));
                        if hosting {
                            save_when_leaving(&page, &cc.egui_ctx);
                        }
                        Ok(Box::new(WebApp { page }))
                    }),
                )
                .await;
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("LoveCraft's web app runs in the browser: build it with `trunk build --release`.");
}
