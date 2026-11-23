//! prosecraft-rs in the browser. Build with `trunk build --release` from this folder.
//!
//! The library is kept in the browser (IndexedDB), files are chosen with the browser's file
//! picker and exports are downloads. Folders can't be opened from a web page, so the folder
//! imports and folder sync say so.
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
mod html_slot;

#[cfg(target_arch = "wasm32")]
mod web {
    use eframe::wasm_bindgen::JsCast;
    use prosecraft_core::library::{Library, new_device_id};
    use prosecraft_ui_egui::{HostRequest, ProseApp};

    use crate::browser::{self, BrowserRandom, BrowserStore, Inbox};

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

    struct WebApp {
        app: ProseApp,
        inbox: Inbox<Then>,
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

    impl WebApp {
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
            let dropped = ui.ctx().input(|input| input.raw.dropped_files.clone());
            for file in dropped {
                browser::read_dropped(file, Then::Dropped, &self.inbox, ui.ctx());
            }
            self.receive();
            eframe::egui::CentralPanel::default().show(ui, |ui| self.app.show(ui));
            self.answer(ui.ctx());
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
        let mut app = match library {
            Ok(library) => ProseApp::with_library(library, Box::new(BrowserStore::new())),
            Err(why) => {
                let mut app = ProseApp::new();
                app.report(&why);
                app
            }
        };
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
        app
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
            let app = start_app().await;
            let _ = eframe::WebRunner::new()
                .start(
                    canvas,
                    eframe::WebOptions::default(),
                    Box::new(|cc| {
                        prosecraft_ui_egui::setup(&cc.egui_ctx);
                        let mut app = app;
                        // Without it, paste and copy in the editor are plain text.
                        let _ = crate::clipboard::install(&mut app, &cc.egui_ctx);
                        Ok(Box::new(WebApp {
                            app,
                            inbox: Default::default(),
                        }))
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
    eprintln!("prosecraft-rs-web runs in the browser: build it with `trunk build --release`.");
}
