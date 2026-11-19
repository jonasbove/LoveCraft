//! The screenshots in `docs/screenshots/` (the README's tour), rendered from the real app offscreen.
//!
//! The project is `tests/showcase/the-glass-orchard.prosecraft`, a short invented novel written for
//! these pictures. Every shot of the app is the app as it runs, driven through its interface; nothing
//! is drawn for the picture. Ignored by default (it needs a GPU or a software Vulkan driver), and it
//! refuses to run without the app's own fonts, so the pictures never show fallback faces:
//!
//! ```sh
//! PROSECRAFT_SHOTS=docs/screenshots CRAFT_FONTS_DIR=$PWD/../craft-fonts \
//!   cargo test -p prosecraft-ui-egui --test shots -- --ignored shots
//! ```
//!
//! `shots` renders the app at 1440×900, `shots_tokens` the colour token sheet of every scheme.

use egui::accesskit::Role;
use egui::{Color32, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use prosecraft_ui_egui::theme::{Palette, SCHEMES, contrast};
use prosecraft_ui_egui::{LibraryStore, ProjectLibrary, ProseApp, fonts};

const SIZE: [f32; 2] = [1440.0, 900.0];

/// Keeps nothing, with a fixed clock: 15 January 2027, the last day of writing in the showcase's
/// activity.
struct Nowhere;
impl LibraryStore for Nowhere {
    fn save(&mut self, _text: &str) -> Result<(), String> {
        Ok(())
    }
    fn now_ms(&self) -> f64 {
        1_800_000_000_000.0
    }
    fn new_id(&mut self) -> String {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        format!(
            "new-{}",
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    }
}

fn out_dir() -> Option<std::path::PathBuf> {
    // Relative to the workspace root (cargo runs tests in the crate's folder).
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = root.join(std::env::var_os("PROSECRAFT_SHOTS")?);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    assert!(
        fonts::has_app_faces(),
        "build with CRAFT_FONTS_DIR pointing at a craft-fonts checkout: the shots show Inter and Source Serif 4"
    );
    Some(dir)
}

fn showcase() -> String {
    let path = format!(
        "{}/tests/showcase/the-glass-orchard.prosecraft",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

type H = Harness<'static, ProseApp>;

fn save(harness: &mut H, dir: &std::path::Path, name: &str) {
    // No hover highlight or tooltip where the last click left the pointer.
    harness.event(egui::Event::PointerGone);
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render: {e}"));
    image
        .save(dir.join(format!("{name}.png")))
        .unwrap_or_else(|e| panic!("save: {e}"));
}

fn open(harness: &mut H, title: &str) {
    harness.get_by_role_and_label(Role::TreeItem, title).click();
    harness.run();
}

fn tab(harness: &mut H, name: &str) {
    harness.get_by_role_and_label(Role::Tab, name).click();
    harness.run();
}

/// Settings → Appearance: a theme or colour scheme picked from its menu.
fn pick(harness: &mut H, combo: &str, option: &str) {
    harness
        .get_by_role_and_label(Role::Button, "Settings…")
        .click();
    harness.run();
    harness.get_by_label("General").click();
    harness.run();
    harness.get_by_role_and_label(Role::ComboBox, combo).click();
    harness.run();
    harness.get_by_role_and_label(Role::Button, option).click();
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
}

#[test]
#[ignore = "renders with wgpu; run on demand"]
fn shots() {
    let Some(dir) = out_dir() else {
        return;
    };
    // The showcase as the library file the app starts with (an import would leave its
    // "Added 1 project" note in the status line).
    let library = ProjectLibrary::from_text(&showcase()).unwrap_or_else(|e| panic!("{e}"));
    let app = ProseApp::with_library(library, Box::new(Nowhere));
    assert!(
        app.state()
            .library()
            .is_some_and(|l| !l.projects().is_empty()),
        "the showcase opens: {:?}",
        app.state().status()
    );
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(1.0)
        .build_ui_state(
            |ui, app: &mut ProseApp| {
                // The whole window, as eframe gives it (the harness adds a margin).
                let full = ui.ctx().content_rect();
                ui.scope_builder(egui::UiBuilder::new().max_rect(full), |ui| app.show(ui));
            },
            app,
        );
    // A steady caret, then the fonts the desktop app installs (the system's as fallbacks).
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    prosecraft_ui_egui::setup(&harness.ctx);
    harness.run();
    let h = &mut harness;

    // The hero: a scene in the Write view, with its comments. Every chapter starts open; the last
    // two are folded so the binder has room for the Wiki's places as well as its characters (the
    // fold lasts the session, so every later shot has it too).
    tab(h, "Write");
    open(h, "Fog on the Quay");
    for chapter in ["The Ledger", "Lantern Night"] {
        h.get_by_role_and_label(Role::Button, &format!("Scenes of {chapter}"))
            .click();
        h.run();
    }
    tab(h, "Comments");
    save(h, &dir, "write");

    tab(h, "Progress");
    tab(h, "Outline");
    save(h, &dir, "outline");

    // The storyboard, zoomed out (pinch) around the canvas's top-left so the whole board shows.
    tab(h, "Storyboard");
    let corner = h.get_by_label("Freeform storyboard").rect().min + egui::vec2(2.0, 2.0);
    h.event(egui::Event::PointerMoved(corner));
    h.run();
    h.event(egui::Event::Zoom(0.9));
    h.run_steps(4);
    save(h, &dir, "storyboard");

    // The relation network, with the room the progress sidebar takes.
    tab(h, "Network");
    h.get_by_label("Hide progress sidebar").click();
    h.run();
    // Re-layout steps the layout's seed and refits the view. In the first arrangement the
    // Orchard's edge runs through its name and Tobiah Marsh's is cut short; the third keeps every
    // name whole except Saltmarsh Quay's, which is too long for any of them.
    for _ in 0..2 {
        h.get_by_label("↺ Re-layout").click();
        h.run_steps(8);
    }
    save(h, &dir, "network");

    // Publish: the classic novel style, its preview turned to the first chapter.
    tab(h, "Publish");
    h.get_by_role_and_label(Role::RadioButton, "Classic novel")
        .click();
    h.run();
    h.get_all_by_label("Next section")
        .next()
        .expect("next section")
        .click();
    h.run_steps(4);
    save(h, &dir, "publish");

    // Split view: the scene beside the Wiki page of its point-of-view character.
    tab(h, "Write");
    open(h, "Fog on the Quay");
    h.get_by_label("Open split view").click();
    h.run();
    h.get_by_label("Show in this pane").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "  Ines Calder")
        .click_accesskit();
    h.run();
    h.key_press(Key::Escape);
    h.run_steps(4);
    save(h, &dir, "split");
    h.get_by_label("Close split view").click();
    h.run();
    h.get_by_label("Show progress sidebar").click();
    h.run();

    // Focus mode hides everything but the text; its hint fades after three seconds.
    open(h, "Fog on the Quay");
    h.get_by_label("Scene text").focus();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter);
    h.run_steps(16);
    save(h, &dir, "focus");
    h.key_press(Key::Escape);
    h.run();

    // Another colour scheme, with the settings that choose it.
    pick(h, "Color scheme", "Everforest");
    h.get_by_role_and_label(Role::Button, "Settings…").click();
    save(h, &dir, "settings");
    h.key_press(Key::Escape);
    h.run();
    pick(h, "Color scheme", "ProseCraft");

    // The light theme.
    pick(h, "Theme", "Light");
    open(h, "Fog on the Quay");
    tab(h, "Comments");
    save(h, &dir, "write-light");
}

/// A swatch row: the token's name, its colour, and its hex value.
fn swatch(ui: &mut egui::Ui, name: &str, color: Color32, ink: Color32) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 18.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 4.0, color);
        ui.painter().rect_stroke(
            rect,
            4.0,
            (1.0, ink.gamma_multiply(0.25)),
            egui::StrokeKind::Inside,
        );
        ui.label(
            egui::RichText::new(format!(
                "{name}  #{:02x}{:02x}{:02x}",
                color.r(),
                color.g(),
                color.b()
            ))
            .size(11.0)
            .color(ink),
        );
    });
}

fn tokens(p: &Palette) -> Vec<(&'static str, Color32)> {
    vec![
        ("page_bg", p.page_bg),
        ("sidebar_bg", p.sidebar_bg),
        ("surface", p.surface),
        ("surface_raised", p.surface_raised),
        ("control_bg", p.control_bg),
        ("hover_bg", p.hover_bg),
        ("selected_bg", p.selected_bg),
        ("border", p.border),
        ("border_control", p.border_control),
        ("text_heading", p.text_heading),
        ("text_primary", p.text_primary),
        ("text_body", p.text_body),
        ("text_secondary", p.text_secondary),
        ("text_muted", p.text_muted),
        ("text_faint", p.text_faint),
        ("selected_text", p.selected_text),
        ("eyebrow", p.eyebrow),
        ("link", p.link),
        ("accent", p.accent),
        ("accent_strong", p.accent_strong),
        ("danger", p.danger),
        ("warning", p.warning),
        ("success", p.success),
    ]
}

/// One scheme in one mode: its name, a sample of the text colours on the page, and its tokens.
fn scheme_card(ui: &mut egui::Ui, label: &str, p: &Palette) {
    egui::Frame::new()
        .fill(p.page_bg)
        .stroke((1.0, p.border))
        .corner_radius(8.0)
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(196.0);
                ui.label(
                    egui::RichText::new(label)
                        .font(fonts::semibold(13.0))
                        .color(p.text_heading),
                );
                ui.label(
                    egui::RichText::new("The fog came in sideways.")
                        .font(fonts::writing(14.0))
                        .color(p.text_body),
                );
                // The rule the theme test enforces: every text colour on every surface.
                let texts = [
                    p.text_primary,
                    p.text_heading,
                    p.text_body,
                    p.text_secondary,
                    p.text_muted,
                    p.selected_text,
                    p.eyebrow,
                    p.quote,
                    p.link,
                    p.danger,
                    p.warning,
                ];
                let surfaces = [
                    p.page_bg,
                    p.sidebar_bg,
                    p.surface,
                    p.surface_raised,
                    p.control_bg,
                    p.hover_bg,
                    p.selected_bg,
                ];
                let worst = texts
                    .into_iter()
                    .flat_map(|t| surfaces.map(move |s| contrast(t, s)))
                    .fold(f64::INFINITY, f64::min);
                ui.label(
                    egui::RichText::new(format!("lowest text contrast {worst:.1}:1"))
                        .size(10.0)
                        .color(p.text_muted),
                );
                ui.add_space(4.0);
                for (name, color) in tokens(p) {
                    swatch(ui, name, color, p.text_primary);
                }
            });
        });
}

#[test]
#[ignore = "renders with wgpu; run on demand"]
fn shots_tokens() {
    let Some(dir) = out_dir() else {
        return;
    };
    for dark in [true, false] {
        let mut harness = Harness::builder()
            .with_size([1880.0, 800.0])
            .with_pixels_per_point(1.0)
            .build_ui(move |ui| {
                // The app's fonts first: the sheet is drawn from the next frame on.
                let ready = egui::Id::unique("shots-fonts");
                if ui.ctx().data(|d| d.get_temp::<bool>(ready)).is_none() {
                    prosecraft_ui_egui::setup_for_tests(ui.ctx());
                    ui.ctx().data_mut(|d| d.insert_temp(ready, true));
                    return;
                }
                let back = if dark {
                    Color32::from_rgb(0x10, 0x11, 0x10)
                } else {
                    Color32::from_rgb(0xe4, 0xe4, 0xe0)
                };
                ui.painter().rect_filled(ui.max_rect(), 0.0, back);
                ui.spacing_mut().item_spacing = egui::vec2(10.0, 4.0);
                ui.horizontal_top(|ui| {
                    for (value, label) in SCHEMES {
                        scheme_card(ui, label, &Palette::for_scheme(value, dark));
                    }
                });
            });
        harness.run();
        save_plain(
            &mut harness,
            &dir,
            if dark { "tokens-dark" } else { "tokens-light" },
        );
    }
}

fn save_plain(harness: &mut Harness<'_>, dir: &std::path::Path, name: &str) {
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render: {e}"));
    image
        .save(dir.join(format!("{name}.png")))
        .unwrap_or_else(|e| panic!("save: {e}"));
}
