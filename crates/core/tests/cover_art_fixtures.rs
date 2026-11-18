//! Generated covers against ProseCraft (`fixtures/cover-art.json`): every canvas call each
//! design makes for four books (long, wrapped, empty and spaced-out text) in the five palettes,
//! with the style's font or Inter. ProseCraft's canvas was replaced by a recorder whose
//! `measureText` is 0.5 of the font size per character (0.6 at weight 700 and up); the same
//! measure replays the layout here.

use prosecraft_core::js::{self, JsString, JsValue};
use prosecraft_core::publish::cover::palette_for;
use prosecraft_core::publish::cover_art::{Align, CoverText, Font, Measure, Op, Paint, draw};

struct Recorder;

impl Measure for Recorder {
    fn width(&self, text: &str, font: &Font) -> f64 {
        let per = if font.weight >= 700 { 0.6 } else { 0.5 };
        text.chars().count() as f64 * font.size * per
    }
}

fn num(n: f64) -> JsValue {
    JsValue::Number(n)
}

fn obj(pairs: Vec<(&str, JsValue)>) -> JsValue {
    let mut o = js::JsObject::new();
    for (k, v) in pairs {
        o.insert(k, v);
    }
    JsValue::Object(o)
}

/// An op as the recorder writes the call.
fn call(op: &Op) -> JsValue {
    match op {
        Op::FillRect {
            x,
            y,
            width,
            height,
            paint,
        } => {
            let fill = match paint {
                Paint::Solid(c) => JsValue::str(&c.css()),
                Paint::Vertical(a, b) => obj(vec![
                    (
                        "gradient",
                        JsValue::Array(vec![num(0.0), num(0.0), num(0.0), num(2400.0)]),
                    ),
                    (
                        "stops",
                        JsValue::Array(vec![
                            JsValue::Array(vec![num(0.0), JsValue::str(&a.css())]),
                            JsValue::Array(vec![num(1.0), JsValue::str(&b.css())]),
                        ]),
                    ),
                ]),
            };
            obj(vec![
                ("op", JsValue::str("fillRect")),
                (
                    "args",
                    JsValue::Array(vec![num(*x), num(*y), num(*width), num(*height)]),
                ),
                ("fill", fill),
            ])
        }
        Op::StrokeRect {
            x,
            y,
            width,
            height,
            color,
            line,
        } => obj(vec![
            ("op", JsValue::str("strokeRect")),
            (
                "args",
                JsValue::Array(vec![num(*x), num(*y), num(*width), num(*height)]),
            ),
            ("stroke", JsValue::str(&color.css())),
            ("lineWidth", num(*line)),
        ]),
        Op::Line {
            from,
            to,
            color,
            line,
        } => obj(vec![
            ("op", JsValue::str("stroke")),
            (
                "path",
                JsValue::Array(vec![
                    JsValue::Array(vec![JsValue::str("M"), num(from.0), num(from.1)]),
                    JsValue::Array(vec![JsValue::str("L"), num(to.0), num(to.1)]),
                ]),
            ),
            ("stroke", JsValue::str(&color.css())),
            ("lineWidth", num(*line)),
        ]),
        Op::Circle {
            x,
            y,
            radius,
            color,
        } => obj(vec![
            ("op", JsValue::str("fill")),
            (
                "path",
                JsValue::Array(vec![JsValue::Array(vec![
                    JsValue::str("A"),
                    num(*x),
                    num(*y),
                    num(*radius),
                    num(0.0),
                    num(std::f64::consts::TAU),
                ])]),
            ),
            ("fill", JsValue::str(&color.css())),
        ]),
        Op::Text {
            text,
            x,
            y,
            font,
            align,
            color,
        } => obj(vec![
            ("op", JsValue::str("fillText")),
            ("text", JsValue::str(text)),
            ("x", num(*x)),
            ("y", num(*y)),
            ("font", JsValue::str(&font.css())),
            (
                "align",
                JsValue::str(if *align == Align::Center {
                    "center"
                } else {
                    "left"
                }),
            ),
            ("baseline", JsValue::str("middle")),
            ("fill", JsValue::str(&color.css())),
        ]),
    }
}

/// Equal, numbers within a billionth.
fn same(a: &JsValue, b: &JsValue) -> bool {
    match (a, b) {
        (JsValue::Number(x), JsValue::Number(y)) => (x - y).abs() < 1e-9,
        (JsValue::Array(x), JsValue::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (JsValue::Object(x), JsValue::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(&k.to_string_lossy()).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

#[test]
fn generated_covers_draw_what_prosecraft_draws() {
    let path = format!(
        "{}/tests/fixtures/cover-art.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let fixture = js::parse(&std::fs::read_to_string(&path).expect("fixture")).expect("json");
    let text = |v: &JsValue, k: &str| {
        v.get(k)
            .and_then(JsValue::as_js_string)
            .map(JsString::to_string_lossy)
            .unwrap_or_default()
    };
    let cases = fixture
        .get("cases")
        .and_then(JsValue::as_array)
        .expect("cases");
    assert_eq!(cases.len(), 16);
    let mut drawn = 0;
    for case in cases {
        let book = case.get("book").expect("book");
        let (title, subtitle, author) = (
            text(book, "title"),
            text(book, "subtitle"),
            text(book, "author"),
        );
        let cover = CoverText {
            title: &title,
            subtitle: &subtitle,
            author: &author,
        };
        let design = text(case, "design");
        let palette = palette_for(&text(case, "palette"));
        let ops = draw(
            &cover,
            &design,
            palette,
            &text(case, "styleFont"),
            &Recorder,
        );
        let want = case
            .get("calls")
            .and_then(JsValue::as_array)
            .expect("calls");
        let got: Vec<JsValue> = ops.iter().map(call).collect();
        for (index, (g, w)) in got.iter().zip(want).enumerate() {
            assert!(
                same(g, w),
                "{design} {title:?} call {index}:\n  have {}\n  want {}",
                js::stringify(g),
                js::stringify(w)
            );
        }
        assert_eq!(got.len(), want.len(), "{design} {title:?}: number of calls");
        drawn += got.len();
        assert_eq!(
            text(case.get("cover").expect("cover"), "background"),
            palette.background
        );
    }
    eprintln!("{drawn} calls compared");
}
