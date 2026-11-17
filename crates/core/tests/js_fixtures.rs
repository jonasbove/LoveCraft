//! The JSON layer against output of the real JavaScript (see `fixtures/README.md`).

use prosecraft_core::js::{self, JsValue};

fn fixture(name: &str) -> JsValue {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    js::parse(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn text(value: &JsValue, key: &str) -> String {
    value
        .get(key)
        .and_then(JsValue::as_js_string)
        .and_then(|s| s.as_string())
        .unwrap_or_else(|| panic!("missing {key}"))
}

#[test]
fn parse_and_stringify_match_javascript() {
    let cases = fixture("json-cases.json");
    for case in cases.as_array().unwrap_or_default() {
        let input = text(case, "input");
        let value = js::parse(&input).unwrap_or_else(|e| panic!("{input}: {e}"));
        assert_eq!(
            js::stringify(&value),
            text(case, "compact"),
            "compact for {input}"
        );
        assert_eq!(
            js::stringify_pretty(&value),
            text(case, "pretty"),
            "pretty for {input}"
        );
        assert_eq!(
            js::stable_stringify(&value),
            text(case, "stable"),
            "stable for {input}"
        );
    }
}

#[test]
fn numbers_print_like_javascript_string() {
    let cases = fixture("numbers.json");
    let cases = cases.as_array().unwrap_or_default();
    assert!(cases.len() >= 3000);
    for case in cases {
        let bits = u64::from_str_radix(&text(case, "bits"), 16).unwrap_or_else(|e| panic!("{e}"));
        let value = f64::from_bits(bits);
        assert_eq!(
            js::number_to_string(value),
            text(case, "string"),
            "bits {bits:016x}"
        );
        // And JSON.parse of that text gives the same double back.
        assert_eq!(
            js::parse(&text(case, "string"))
                .ok()
                .and_then(|v| v.as_number())
                .map(f64::to_bits),
            Some(if value == 0.0 { 0.0f64.to_bits() } else { bits })
        );
    }
}
