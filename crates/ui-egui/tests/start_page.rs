//! The start page, driven by keyboard and checked through the accessibility tree.

use egui::Key;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use prosecraft_ui_egui::ProseApp;

fn harness() -> Harness<'static, ProseApp> {
    Harness::new_ui_state(|ui, app: &mut ProseApp| app.show(ui), ProseApp::new())
}

#[test]
fn start_page_names_the_app() {
    let harness = harness();
    harness.get_by_label("prosecraft-rs");
}

#[test]
fn about_opens_by_keyboard_and_credits_the_original() {
    let mut harness = harness();
    harness.key_press(Key::Tab);
    harness.run();
    let button = harness.get_by_role_and_label(Role::Button, "About");
    assert!(button.is_focused(), "Tab should focus the About button");

    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.state().about_open());
    harness.get_by_role_and_label(Role::Button, "Hide about");
    harness.get_by_label_contains("ProseCraft by Jennifer Roylance");
}
