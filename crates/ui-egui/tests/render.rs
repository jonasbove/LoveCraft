//! Renders the app's views offscreen to PNG files, to look at them next to ProseCraft's.
//!
//! Ignored by default (it needs a GPU or a software renderer). Run it with
//! `PROSECRAFT_RENDER_DIR=/some/dir CRAFT_FONTS_DIR=/path/to/craft-fonts \
//!  cargo test -p prosecraft-ui-egui --test render -- --ignored`.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use prosecraft_ui_egui::ProseApp;

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
#[ignore = "renders with wgpu; run on demand"]
fn render_views() {
    let Some(dir) = std::env::var_os("PROSECRAFT_RENDER_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let mut app = ProseApp::new();
    app.open_backup(
        "bundle-plain.prosecraft",
        &fixture("bundle-plain.prosecraft"),
    );
    let mut harness = Harness::builder()
        .with_size([1440.0, 900.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup(&harness.ctx);
    harness.run();
    let save = |harness: &mut Harness<'_, ProseApp>, name: &str| {
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render: {e}"));
        image
            .save(dir.join(format!("{name}.png")))
            .unwrap_or_else(|e| panic!("save: {e}"));
    };
    save(&mut harness, "outline");
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    save(&mut harness, "write-scene");
    harness
        .get_by_role_and_label(Role::TreeItem, "Chapter One")
        .click();
    save(&mut harness, "write-chapter");
}
