//! Custom export styles against ProseCraft's `presets.ts` (`fixtures/custom-styles.json`):
//! storing (`normalizeCustomStyles`), tokens (`customStyleTokens`), copies
//! (`createCustomStyle`), and an HTML export in a custom style.

use prosecraft_core::js::{self, JsString, JsValue};
use prosecraft_core::project::Project;
use prosecraft_core::publish::styles::{
    create_custom_style, custom_style_tokens, detail, normalize_custom_styles, to_value,
    words_per_page,
};
use prosecraft_core::publish::{PublishOptions, compile_book, export_html};

fn fixture() -> JsValue {
    let path = format!(
        "{}/tests/fixtures/custom-styles.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    js::parse(&text).unwrap_or_else(|e| panic!("{path}: {e:?}"))
}

fn raw(fixture: &JsValue) -> JsValue {
    // `NaN` was written as a string; ProseCraft read the number.
    let text = js::stringify(fixture.get("raw").expect("raw"))
        .replace(r#""lineHeight":"NaN""#, r#""lineHeight":null"#);
    js::parse(&text).expect("raw")
}

#[test]
fn styles_are_stored_clamped_and_made_into_tokens() {
    let fixture = fixture();
    let styles = normalize_custom_styles(Some(&raw(&fixture)));
    let got: Vec<String> = styles.iter().map(|s| js::stringify(&to_value(s))).collect();
    let want: Vec<String> = fixture
        .get("normalized")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .map(js::stringify)
        .collect();
    assert_eq!(got, want);
    let tokens = fixture
        .get("tokens")
        .and_then(JsValue::as_array)
        .unwrap_or_default();
    assert_eq!(tokens.len(), styles.len());
    let text = |v: &JsValue, k: &str| {
        v.get(k)
            .and_then(JsValue::as_js_string)
            .map(JsString::to_string_lossy)
            .unwrap_or_default()
    };
    let num = |v: &JsValue, k: &str| v.get(k).and_then(JsValue::as_number).unwrap_or(f64::NAN);
    for (style, want) in styles.iter().zip(tokens) {
        let t = custom_style_tokens(style);
        assert_eq!(detail(style), text(want, "detail"));
        assert_eq!(t.font, text(want, "cssFontFamily"));
        assert_eq!(t.docx_font, text(want, "docxFont"));
        assert_eq!(t.pdf_font, text(want, "pdfFont"));
        assert_eq!(t.chapter_align, text(want, "chapterHeadingAlign"));
        let page = want.get("page").expect("page");
        assert_eq!(
            t.page,
            [
                num(page, "widthPt"),
                num(page, "heightPt"),
                num(page, "marginTopPt"),
                num(page, "marginRightPt"),
                num(page, "marginBottomPt"),
                num(page, "marginLeftPt"),
            ],
            "{}",
            style.id
        );
        assert_eq!(
            f64::from(words_per_page(style)),
            num(want, "wordsPerPage"),
            "{}",
            style.id
        );
    }
    for case in fixture
        .get("created")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        let source = text(case, "source");
        let want = case.get("style").expect("style");
        let unique = text(want, "id").trim_start_matches("custom-").to_owned();
        let made = create_custom_style(&source, &styles, &unique);
        assert_eq!(
            js::stringify(&to_value(&made)),
            js::stringify(want),
            "from {source}"
        );
    }
}

#[test]
fn html_exports_in_a_custom_style() {
    let fixture = fixture();
    let mut stored = js::JsObject::new();
    stored.insert("project", fixture.get("project").cloned().expect("project"));
    let mut project = Project::from_stored(&JsValue::Object(stored)).expect("project");
    project.citations.clear();
    let html = fixture
        .get("html")
        .and_then(JsValue::as_object)
        .expect("html");
    for (style, want) in html.iter() {
        let style = style.to_string_lossy();
        let mut options = js::JsObject::new();
        options.insert("style", JsValue::str(&style));
        options.insert("customStyles", raw(&fixture));
        options.insert("dropCaps", JsValue::Bool(true));
        options.insert("includeTableOfContents", JsValue::Bool(true));
        let options = PublishOptions::read(Some(&JsValue::Object(options)));
        let export = export_html(&compile_book(&project, &options), &options);
        let got = String::from_utf8_lossy(&export.bytes);
        let want = want
            .as_js_string()
            .map(JsString::to_string_lossy)
            .unwrap_or_default();
        assert!(
            got == want,
            "{style}: differs at {}",
            got.chars()
                .zip(want.chars())
                .take_while(|(a, b)| a == b)
                .count()
        );
    }
}
