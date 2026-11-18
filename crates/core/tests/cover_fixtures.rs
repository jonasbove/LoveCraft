//! Covers against ProseCraft (`fixtures/cover.json`): the cover options as `normalizeCover`
//! reads them, and a book with a cover exported as HTML and EPUB. (PDF: `pdf_export.rs`.)

use base64::Engine;
use prosecraft_core::js::{self, JsString, JsValue};
use prosecraft_core::project::Project;
use prosecraft_core::publish::cover::{CompiledCover, CoverOptions, media_type};
use prosecraft_core::publish::{PublishOptions, compile_book, epub_entries, export_html};

fn fixture() -> JsValue {
    let path = format!("{}/tests/fixtures/cover.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    js::parse(&text).unwrap_or_else(|e| panic!("{path}: {e:?}"))
}

fn text(value: &JsValue, key: &str) -> String {
    value
        .get(key)
        .and_then(JsValue::as_js_string)
        .map(JsString::to_string_lossy)
        .unwrap_or_default()
}

#[test]
fn cover_options_read_like_prosecraft() {
    let fixture = fixture();
    let raw = fixture
        .get("rawCovers")
        .and_then(JsValue::as_array)
        .expect("raw");
    let want = fixture
        .get("normalized")
        .and_then(JsValue::as_array)
        .expect("normalized");
    assert_eq!(raw.len(), want.len());
    for (raw, want) in raw.iter().zip(want) {
        let raw = (raw.as_js_string().and_then(JsString::as_string).as_deref()
            != Some("UNDEFINED"))
        .then_some(raw);
        let got = CoverOptions::read(raw);
        assert_eq!(
            js::stringify(&got.to_value()),
            js::stringify(want),
            "{raw:?}"
        );
    }
}

#[test]
fn a_cover_exports_like_prosecraft() {
    let fixture = fixture();
    let mut stored = js::JsObject::new();
    stored.insert("project", fixture.get("project").cloned().expect("project"));
    let mut project = Project::from_stored(&JsValue::Object(stored)).expect("project");
    project.citations.clear();
    for (name, case) in fixture
        .get("exports")
        .and_then(JsValue::as_object)
        .expect("exports")
        .iter()
    {
        let name = name.to_string_lossy();
        let options = PublishOptions::read(case.get("options"));
        let cover = case.get("cover").expect("cover");
        let data_url = text(cover, "dataUrl");
        let mut book = compile_book(&project, &options);
        book.cover = Some(CompiledCover {
            media_type: media_type(&data_url),
            data_url,
            width: 2,
            height: 3,
            background: text(cover, "background"),
        });
        let sections: Vec<String> = prosecraft_core::publish::sections::list(&book, &options)
            .into_iter()
            .map(|s| s.id)
            .collect();
        let want: Vec<String> = case
            .get("sections")
            .and_then(JsValue::as_array)
            .expect("sections")
            .iter()
            .map(|v| {
                v.as_js_string()
                    .map(JsString::to_string_lossy)
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(sections, want, "{name}: sections");
        let html = export_html(&book, &options);
        assert_eq!(
            String::from_utf8_lossy(&html.bytes),
            text(case, "html"),
            "{name}: html"
        );
        let got = epub_entries(&book, &options, "urn:uuid:ID", "MODIFIED").expect("entries");
        let want = case.get("epub").and_then(JsValue::as_array).expect("epub");
        assert_eq!(
            got.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
            want.iter().map(|e| text(e, "name")).collect::<Vec<_>>(),
            "{name}: entries"
        );
        for ((entry, bytes), want) in got.iter().zip(want) {
            let got = if entry.ends_with(".png") {
                base64::engine::general_purpose::STANDARD.encode(bytes)
            } else {
                String::from_utf8_lossy(bytes).into_owned()
            };
            assert_eq!(got, text(want, "text"), "{name}: {entry}");
        }
    }
}
