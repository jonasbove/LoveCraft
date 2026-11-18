//! Proofreading against ProseCraft's (`fixtures/proofread.json`): harper.js 3.0.0 through
//! `ensureHarperLinter` and `lintPlainText`, on seven texts in each of the four dialects.

use prosecraft_core::js::{self, JsString, JsValue};
use prosecraft_core::proofread::{DIALECTS, Lint, Proofreader};

fn text(v: &JsValue, k: &str) -> String {
    v.get(k)
        .and_then(JsValue::as_js_string)
        .map(JsString::to_string_lossy)
        .unwrap_or_default()
}

#[test]
fn lints_match_harper_js() {
    let path = format!(
        "{}/tests/fixtures/proofread.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let fixture = js::parse(&std::fs::read_to_string(&path).expect("fixture")).expect("json");
    let mut compared = 0;
    for (dialect, _) in DIALECTS {
        let mut linter = Proofreader::new(dialect);
        for case in fixture
            .get(dialect)
            .and_then(JsValue::as_array)
            .expect(dialect)
        {
            let source = text(case, "text");
            // The fixture's offsets are UTF-16 (JavaScript's); the samples have no characters
            // outside the BMP, so they are characters too.
            assert!(source.chars().all(|c| c.len_utf16() == 1));
            let want: Vec<Lint> = case
                .get("lints")
                .and_then(JsValue::as_array)
                .unwrap_or_default()
                .iter()
                .map(|l| Lint {
                    start: l.get("start").and_then(JsValue::as_number).unwrap_or(-1.0) as usize,
                    end: l.get("end").and_then(JsValue::as_number).unwrap_or(-1.0) as usize,
                    message: text(l, "message"),
                    kind: text(l, "kind"),
                    kind_pretty: text(l, "kindPretty"),
                    suggestions: l
                        .get("suggestions")
                        .and_then(JsValue::as_array)
                        .unwrap_or_default()
                        .iter()
                        .map(|s| {
                            s.as_js_string()
                                .map(JsString::to_string_lossy)
                                .unwrap_or_default()
                        })
                        .collect(),
                })
                .collect();
            let got = linter.lint(&source);
            assert_eq!(got, want, "{dialect}: {source:?}");
            compared += got.len();
        }
    }
    assert!(compared > 30, "{compared} lints");
}
