//! Calculation worksheets against ProseCraft's `calc.ts` (`fixtures/calc.json`): every line of
//! 16 worksheets (units, display units, functions, unit chains, number formatting and its
//! rounding, errors, limits), the LaTeX and the plain text.

use prosecraft_core::calc::{
    CalcLine, evaluate_worksheet, worksheet_to_latex, worksheet_to_plain_text,
};
use prosecraft_core::js::{self, JsString, JsValue};

fn text(v: &JsValue, k: &str) -> String {
    v.get(k)
        .and_then(JsValue::as_js_string)
        .map(JsString::to_string_lossy)
        .unwrap_or_default()
}

fn line_of(v: &JsValue) -> CalcLine {
    match text(v, "kind").as_str() {
        "blank" => CalcLine::Blank,
        "text" => CalcLine::Text(text(v, "text")),
        "math" => CalcLine::Math {
            source: text(v, "source"),
            latex: text(v, "latex"),
            plain: text(v, "plain"),
        },
        _ => CalcLine::Error {
            source: text(v, "source"),
            message: text(v, "message"),
        },
    }
}

#[test]
fn worksheets_evaluate_like_prosecraft() {
    let path = format!("{}/tests/fixtures/calc.json", env!("CARGO_MANIFEST_DIR"));
    let fixture = js::parse(&std::fs::read_to_string(&path).expect("fixture")).expect("json");
    let cases = fixture
        .get("cases")
        .and_then(JsValue::as_array)
        .expect("cases");
    let mut lines_compared = 0;
    for (index, case) in cases.iter().enumerate() {
        let source = text(case, "source");
        let got = evaluate_worksheet(&source);
        match case.get("lines").and_then(JsValue::as_array) {
            Some(want) => {
                let got = got.unwrap_or_else(|e| panic!("case {index}: {e:?}"));
                assert_eq!(got.len(), want.len(), "case {index}");
                for (n, (g, w)) in got.iter().zip(want).enumerate() {
                    assert_eq!(*g, line_of(w), "case {index} line {n}");
                    lines_compared += 1;
                }
                let latex = worksheet_to_latex(&got).unwrap_or_else(|e| format!("ERROR: {}", e.0));
                assert_eq!(latex, text(case, "latex"), "case {index}: latex");
            }
            None => assert_eq!(
                got.map_err(|e| e.0),
                Err(text(case, "error")),
                "case {index}"
            ),
        }
        assert_eq!(
            worksheet_to_plain_text(&source),
            text(case, "plain"),
            "case {index}: plain"
        );
    }
    assert!(lines_compared > 100, "{lines_compared}");
}
