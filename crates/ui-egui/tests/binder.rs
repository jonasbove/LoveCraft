//! The binder's rows: a long title keeps its actions on its row, and chapters fold away, driven
//! by keyboard and checked through the accessibility tree.

use egui::Key;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use prosecraft_ui_egui::{LibraryStore, ProjectLibrary, ProseApp};

/// Keeps nothing.
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

const LONG: &str = "The Night the Lighthouse Keeper Finally Climbed Down to the Village";

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The plain bundle as the app's own library (so it can be edited), changed by `edit`.
fn app(edit: impl Fn(String) -> String) -> Harness<'static, ProseApp> {
    let text = edit(fixture("bundle-plain.prosecraft"));
    let library = ProjectLibrary::from_text(&text).unwrap_or_else(|e| panic!("{e}"));
    let app = ProseApp::with_library(library, Box::new(Nowhere));
    let mut harness = Harness::builder()
        .with_max_steps(8)
        .with_size([1280.0, 850.0])
        .build_ui_state(|ui, app: &mut ProseApp| app.show(ui), app);
    prosecraft_ui_egui::setup_for_tests(&harness.ctx);
    harness.run();
    harness
}

/// The right edge of the binder's contents: where its "Add chapter" button ends (the Outline
/// has one too, further right).
fn binder_right(harness: &Harness<'static, ProseApp>) -> f32 {
    harness
        .get_all_by_role_and_label(Role::Button, "Add chapter")
        .map(|n| n.rect().right())
        .fold(f32::INFINITY, f32::min)
}

#[test]
fn a_long_title_is_shortened_and_its_actions_stay_on_its_row() {
    let harness = app(|text| {
        text.replacen(
            r#""title": "Chapter One""#,
            &format!(r#""title": "{LONG}""#),
            1,
        )
        .replacen(
            r#""title": "Opening Scene""#,
            &format!(r#""title": "{LONG}, scene""#),
            1,
        )
    });
    for title in [LONG.to_owned(), format!("{LONG}, scene")] {
        // The row keeps its whole title for screen readers, though it shows less of it.
        let row = harness.get_by_role_and_label(Role::TreeItem, &title).rect();
        let actions = harness
            .get_by_role_and_label(Role::Button, &format!("Actions for {title}"))
            .rect();
        // On the same line as the title, after it, and inside the binder.
        assert!(
            row.y_range().contains(actions.center().y),
            "{title}: actions {actions:?} not beside the row {row:?}"
        );
        assert!(
            actions.left() >= row.right(),
            "{title}: {actions:?} {row:?}"
        );
        assert!(
            actions.right() <= binder_right(&harness) + 0.5,
            "{title}: actions end at {}",
            actions.right()
        );
    }
}

#[test]
fn every_row_has_its_actions_at_the_binders_right_edge() {
    let harness = app(|text| text);
    for title in ["Chapter One", "Opening Scene", "INT. LIGHTHOUSE - NIGHT"] {
        let row = harness.get_by_role_and_label(Role::TreeItem, title).rect();
        let actions = harness
            .get_by_role_and_label(Role::Button, &format!("Actions for {title}"))
            .rect();
        assert!(row.y_range().contains(actions.center().y), "{title}");
        assert!(
            (actions.right() - binder_right(&harness)).abs() <= 1.0,
            "{title}: actions end at {}",
            actions.right()
        );
    }
}

#[test]
fn a_chapter_folds_its_scenes_away_and_back() {
    let mut harness = app(|text| text);
    let chevron = harness.get_by_role_and_label(Role::Button, "Scenes of Chapter One");
    assert_eq!(chevron.accesskit_node().data().is_expanded(), Some(true));
    harness.get_by_role_and_label(Role::TreeItem, "Opening Scene");

    // Enter on the chevron hides the scenes; the Wiki below moves up.
    let wiki = harness
        .get_by_role_and_label(Role::TreeItem, "Mara")
        .rect()
        .top();
    harness
        .get_by_role_and_label(Role::Button, "Scenes of Chapter One")
        .focus();
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(
        harness
            .query_by_role_and_label(Role::TreeItem, "Opening Scene")
            .is_none()
    );
    let chapter = harness.get_by_role_and_label(Role::TreeItem, "Chapter One");
    assert_eq!(chapter.accesskit_node().data().is_expanded(), Some(false));
    assert!(
        harness
            .get_by_role_and_label(Role::TreeItem, "Mara")
            .rect()
            .top()
            < wiki
    );

    // Right on the chapter shows them again, Left hides them, as in a tree.
    harness
        .get_by_role_and_label(Role::TreeItem, "Chapter One")
        .focus();
    harness.run();
    harness.key_press(Key::ArrowRight);
    harness.run();
    harness.get_by_role_and_label(Role::TreeItem, "Opening Scene");
    harness.key_press(Key::ArrowLeft);
    harness.run();
    assert!(
        harness
            .query_by_role_and_label(Role::TreeItem, "Opening Scene")
            .is_none()
    );
}

#[test]
fn a_chapter_stored_as_folded_starts_folded_unless_its_scene_is_open() {
    let mut harness = app(|text| text.replace(r#""expanded": true"#, r#""expanded": false"#));
    assert!(
        harness
            .query_by_role_and_label(Role::TreeItem, "Opening Scene")
            .is_none()
    );
    let chevron = harness.get_by_role_and_label(Role::Button, "Scenes of Chapter One");
    assert_eq!(chevron.accesskit_node().data().is_expanded(), Some(false));
    harness
        .get_by_role_and_label(Role::Button, "Scenes of Chapter One")
        .focus();
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    // Open a scene: its chapter stays open, and its chevron can't fold it while it is.
    harness
        .get_by_role_and_label(Role::TreeItem, "Opening Scene")
        .click();
    harness.run();
    let chevron = harness.get_by_role_and_label(Role::Button, "Scenes of Chapter One");
    assert_eq!(chevron.accesskit_node().data().is_expanded(), Some(true));
    assert!(chevron.accesskit_node().is_disabled());
}
