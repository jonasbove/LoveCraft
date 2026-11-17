//! Imports against ProseCraft's own importers' output (see `fixtures/README.md`).

use base64::Engine;
use prosecraft_core::import::epub::{build_epub_project, parse_epub};
use prosecraft_core::js::{self, JsValue};

fn fixture(name: &str) -> JsValue {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    js::parse(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn s(v: &JsValue, key: &str) -> String {
    v.get(key)
        .and_then(JsValue::as_js_string)
        .map(|t| t.to_string_lossy())
        .unwrap_or_default()
}

/// A manuscript as ProseCraft's JSON writes it.
fn manuscript_json(m: &prosecraft_core::import::epub::EpubManuscript) -> String {
    let mut o = js::JsObject::new();
    o.insert("title", JsValue::str(&m.title));
    o.insert("author", JsValue::str(&m.author));
    let chapters = m
        .chapters
        .iter()
        .map(|c| {
            let mut co = js::JsObject::new();
            co.insert("title", JsValue::str(&c.title));
            let scenes = c
                .scenes
                .iter()
                .map(|sc| {
                    let mut so = js::JsObject::new();
                    so.insert("title", JsValue::str(&sc.title));
                    so.insert("content", JsValue::str(&sc.content));
                    JsValue::Object(so)
                })
                .collect();
            co.insert("scenes", JsValue::Array(scenes));
            JsValue::Object(co)
        })
        .collect();
    o.insert("chapters", JsValue::Array(chapters));
    o.insert("coverImage", JsValue::str(&m.cover_image));
    o.insert("skippedImages", JsValue::Number(m.skipped_images as f64));
    js::stringify(&JsValue::Object(o))
}

#[test]
fn epub_import_matches_prosecraft() {
    let all = fixture("epub-import.json");
    let mut failures = Vec::new();
    let cases = all
        .get("cases")
        .and_then(JsValue::as_array)
        .unwrap_or_default();
    assert_eq!(cases.len(), 25, "every case is checked");
    for case in cases {
        let name = s(case, "name");
        let data = base64::engine::general_purpose::STANDARD
            .decode(s(case, "epub"))
            .expect("base64");
        let fallback = case
            .get("fallbackTitle")
            .and_then(JsValue::as_js_string)
            .map(|t| t.to_string_lossy());
        let got = parse_epub(&data, fallback.as_deref());
        match (case.get("error").and_then(JsValue::as_js_string), got) {
            (Some(error), Err(e)) if error.to_string_lossy() == e.0 => {}
            (Some(error), other) => failures.push(format!(
                "{name}: expected error {:?}, got {other:?}",
                error.to_string_lossy()
            )),
            (None, Ok(m)) => {
                let want = js::stringify(case.get("result").unwrap_or(&JsValue::Null));
                let have = manuscript_json(&m);
                if want != have {
                    failures.push(format!("{name}:\n  want {want}\n  have {have}"));
                }
            }
            (None, Err(e)) => failures.push(format!("{name}: unexpected error {e}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_imported_epub_becomes_prosecraft_s_project() {
    let all = fixture("epub-import.json");
    let case = all
        .get("cases")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .find(|c| s(c, "name") == "cover-meta")
        .cloned()
        .expect("case");
    let data = base64::engine::general_purpose::STANDARD
        .decode(s(&case, "epub"))
        .expect("base64");
    let manuscript = parse_epub(&data, None).expect("parse");
    let mut n = 0;
    let project = build_epub_project(&manuscript, &mut || {
        n += 1;
        format!("id-{n}")
    });
    assert_eq!(
        js::stringify(&project),
        js::stringify(all.get("project").unwrap_or(&JsValue::Null))
    );
}
