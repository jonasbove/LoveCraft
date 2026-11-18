//! Equations and calculations in exports (`renderExport`): prepared, the book's PDF and DOCX
//! carry pictures of them and its HTML carries SVG.

use prosecraft_core::js::{self, JsValue};
use prosecraft_core::project::Project;
use prosecraft_core::publish::pdf::{Op, layout_pdf};
use prosecraft_core::publish::pdf_writer::DataUrlSizes;
use prosecraft_core::publish::{PublishOptions, compile_book, export_docx, export_html};
use prosecraft_core::rich::{RenderOptions, prepare_book};

fn book() -> prosecraft_core::publish::CompiledBook {
    let path = format!(
        "{}/tests/fixtures/export-html.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let fixture = js::parse(&std::fs::read_to_string(path).expect("fixture")).expect("json");
    let mut stored = js::JsObject::new();
    stored.insert("project", fixture.get("project").cloned().expect("project"));
    let mut project = Project::from_stored(&JsValue::Object(stored)).expect("project");
    project.citations.clear();
    // One scene gets an equation, an inline one, a calculation and a diagram.
    let scene = project
        .documents
        .iter_mut()
        .find(|d| d.chapter_id.is_some())
        .expect("a scene");
    scene.content = r#"<p>Area <span class="math-inline" data-tex="\pi r^2"></span>.</p><div class="math-block" data-tex="E = mc^2"></div><div class="calc-worksheet" data-source="m := 5 kg&#10;F := m g ="></div><div class="mermaid-diagram" data-source="graph TD; A--&gt;B"></div>"#.into();
    compile_book(&project, &PublishOptions::read(None))
}

#[test]
fn prepared_exports_carry_the_drawings() {
    let book = book();
    let options = PublishOptions::read(None);
    let png = RenderOptions {
        png: true,
        color: "#1d211d".into(),
        body_size_pt: 12.0,
    };
    let prepared = prepare_book(&book, &png);
    let images = |b| {
        layout_pdf(b, &options, &DataUrlSizes::default())
            .pages
            .iter()
            .flat_map(|p| &p.ops)
            .filter(|op| matches!(op, Op::Image { .. }))
            .count()
    };
    assert_eq!(images(&book), 0);
    assert_eq!(
        images(&prepared),
        4,
        "the inline equation, the equation, the calculation and the diagram"
    );
    let docx = export_docx(&prepared, &options).expect("docx");
    // Each picture's name is in the archive twice (its entry and the central directory).
    let names = docx
        .bytes
        .windows(11)
        .filter(|w| *w == b"word/media/")
        .count();
    assert_eq!(names / 2, 4, "four pictures in the DOCX");
    let html = prepare_book(&book, &RenderOptions::default());
    let page = String::from_utf8(export_html(&html, &options).bytes).expect("utf-8");
    assert_eq!(page.matches("<svg").count(), 4);
    assert!(
        page.contains(r#"class="mermaid-diagram rich-rendered""#),
        "the diagram is drawn"
    );
    assert!(page.contains("viewBox=") && !page.contains("viewbox="));
}
