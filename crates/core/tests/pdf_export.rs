//! PDF export against ProseCraft's own (`fixtures/export-pdf.json`): both files are read back
//! and what each page draws is compared, text by text (font, size, colour, position), line by
//! line and image by image. jsPDF writes each run of text as its own `BT … Td (…) Tj ET`; so
//! does the Rust writer.

use std::sync::Arc;

use base64::Engine;
use craft_pdf::cos::{Document, Object};
use prosecraft_core::js::{self, JsValue};
use prosecraft_core::project::Project;
use prosecraft_core::publish::pdf::layout_pdf;
use prosecraft_core::publish::pdf_text::win_ansi;
use prosecraft_core::publish::pdf_writer::{DataUrlSizes, write_pdf};
use prosecraft_core::publish::{PublishOptions, compile_book};

fn fixture(name: &str) -> JsValue {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    js::parse(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[derive(Clone, Debug)]
enum Draw {
    Text {
        x: f64,
        y: f64,
        font: String,
        size: f64,
        color: [f64; 3],
        bytes: Vec<u8>,
    },
    Line {
        from: (f64, f64),
        to: (f64, f64),
        color: [f64; 3],
    },
    Image {
        at: [f64; 4],
    },
}

/// WinAnsi bytes as text; or, as jsPDF writes a string the font can't encode, UTF-16 with what
/// WinAnsi lacks as "?".
fn decode(bytes: &[u8], utf16: bool) -> String {
    if utf16 {
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|p| u16::from_be_bytes([p[0], *p.get(1).unwrap_or(&0)]))
            .collect();
        return String::from_utf16_lossy(&units)
            .chars()
            .map(|c| if win_ansi(c).is_some() { c } else { '?' })
            .collect();
    }
    bytes
        .iter()
        .map(|b| {
            (0x20..=0xFFu32)
                .filter_map(char::from_u32)
                .find(|c| win_ansi(*c) == Some(*b))
                .unwrap_or('\u{FFFD}')
        })
        .collect()
}

fn pages(doc: &Document) -> Vec<Arc<Object>> {
    fn walk(doc: &Document, node: &Object, out: &mut Vec<Arc<Object>>) {
        let Some(dict) = node.as_dict() else { return };
        for kid in dict
            .get(b"Kids")
            .and_then(Object::as_array)
            .into_iter()
            .flatten()
        {
            let Some(r) = kid.as_ref() else { continue };
            let obj = doc.get(r);
            match obj.as_dict().and_then(|d| d.name(b"Type")) {
                Some(b"Pages") => walk(doc, &obj, out),
                _ => out.push(obj),
            }
        }
    }
    let root = doc.root().expect("root");
    let catalog = doc.get(root);
    let pages = doc.get(
        catalog
            .as_dict()
            .and_then(|d| d.reference(b"Pages"))
            .expect("pages"),
    );
    let mut out = Vec::new();
    walk(doc, &pages, &mut out);
    out
}

fn resolve<'a>(doc: &Document, obj: &'a Object) -> std::borrow::Cow<'a, Object> {
    match obj.as_ref() {
        Some(r) => std::borrow::Cow::Owned((*doc.get(r)).clone()),
        None => std::borrow::Cow::Borrowed(obj),
    }
}

/// What each page draws.
fn draws(bytes: &[u8]) -> Vec<Vec<Draw>> {
    let doc = Document::open(Arc::new(bytes.to_vec())).expect("a PDF");
    let mut out = Vec::new();
    for page in pages(&doc) {
        let dict = page.as_dict().expect("page");
        let resources = resolve(&doc, dict.get(b"Resources").expect("resources"));
        let fonts = resources
            .as_dict()
            .and_then(|r| r.get(b"Font"))
            .map(|f| resolve(&doc, f).into_owned());
        let font_name = |key: &[u8]| -> String {
            fonts
                .as_ref()
                .and_then(|f| f.as_dict())
                .and_then(|f| f.get(key))
                .map(|f| resolve(&doc, f).into_owned())
                .and_then(|f| {
                    f.as_dict()
                        .and_then(|d| d.name(b"BaseFont"))
                        .map(|n| String::from_utf8_lossy(n).into_owned())
                })
                .unwrap_or_default()
        };
        let mut content = Vec::new();
        let contents = dict.get(b"Contents").expect("contents");
        let streams: Vec<Object> = match &*resolve(&doc, contents) {
            Object::Array(items) => items
                .iter()
                .map(|i| resolve(&doc, i).into_owned())
                .collect(),
            other => vec![other.clone()],
        };
        for s in streams {
            if let Object::Stream(stream) = s {
                content.extend(stream.decoded().expect("decoded"));
                content.push(b'\n');
            }
        }
        let mut draws = Vec::new();
        let (mut font, mut size, mut fill, mut stroke) = (String::new(), 0.0, [0.0; 3], [0.0; 3]);
        let (mut x, mut y) = (0.0, 0.0);
        let mut path_start = (0.0, 0.0);
        let mut matrix = [0.0; 6];
        for op in craft_pdf::cos::content::parse(&content).ops {
            let n = |i: usize| op.num(i).unwrap_or(0.0);
            match op.op.as_slice() {
                b"Tf" => {
                    font = font_name(op.name(0).unwrap_or_default());
                    size = n(1);
                }
                b"rg" => fill = [n(0), n(1), n(2)],
                b"g" => fill = [n(0); 3],
                b"RG" => stroke = [n(0), n(1), n(2)],
                b"G" => stroke = [n(0); 3],
                b"BT" => (x, y) = (0.0, 0.0),
                b"Td" => (x, y) = (x + n(0), y + n(1)),
                b"Tj" => {
                    let bytes = match op.operands.first() {
                        Some(Object::String(s)) => s.bytes.clone(),
                        _ => Vec::new(),
                    };
                    draws.push(Draw::Text {
                        x,
                        y,
                        font: font.clone(),
                        size,
                        color: fill,
                        bytes,
                    });
                }
                b"m" => path_start = (n(0), n(1)),
                b"l" => draws.push(Draw::Line {
                    from: path_start,
                    to: (n(0), n(1)),
                    color: stroke,
                }),
                b"cm" => matrix = [n(0), n(1), n(2), n(3), n(4), n(5)],
                b"Do" => draws.push(Draw::Image {
                    at: [matrix[4], matrix[5], matrix[0], matrix[3]],
                }),
                _ => {}
            }
        }
        out.push(draws);
    }
    out
}

fn close(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance
}

/// Where two draws differ, if they do.
fn differ(want: &Draw, have: &Draw) -> Option<String> {
    match (want, have) {
        (
            Draw::Text {
                x: wx,
                y: wy,
                font: wf,
                size: ws,
                color: wc,
                bytes: wb,
            },
            Draw::Text {
                x: hx,
                y: hy,
                font: hf,
                size: hs,
                color: hc,
                bytes: hb,
            },
        ) => {
            let have_text = decode(hb, false);
            let want_text = if decode(wb, false) == have_text {
                have_text.clone()
            } else {
                decode(wb, true)
            };
            let same = close(*wx, *hx, 0.01)
                && close(*wy, *hy, 0.01)
                && wf == hf
                && close(*ws, *hs, 0.0001)
                && wc.iter().zip(hc).all(|(a, b)| close(*a, *b, 0.003))
                && want_text == have_text;
            (!same).then(|| {
                format!("want {want_text:?} {wf} {ws} at ({wx:.3}, {wy:.3}) {wc:?}\n    have {have_text:?} {hf} {hs} at ({hx:.3}, {hy:.3}) {hc:?}")
            })
        }
        (
            Draw::Line {
                from: wa,
                to: wb,
                color: wc,
            },
            Draw::Line {
                from: ha,
                to: hb,
                color: hc,
            },
        ) => {
            let same = close(wa.0, ha.0, 0.01)
                && close(wa.1, ha.1, 0.01)
                && close(wb.0, hb.0, 0.01)
                && close(wb.1, hb.1, 0.01)
                && wc.iter().zip(hc).all(|(a, b)| close(*a, *b, 0.003));
            (!same).then(|| {
                format!("want line {wa:?}-{wb:?} {wc:?}\n    have line {ha:?}-{hb:?} {hc:?}")
            })
        }
        (Draw::Image { at: w }, Draw::Image { at: h }) => {
            let same = w.iter().zip(h).all(|(a, b)| close(*a, *b, 0.01));
            (!same).then(|| format!("want image {w:?}\n    have image {h:?}"))
        }
        _ => Some(format!("want {want:?}\n    have {have:?}")),
    }
}

#[test]
fn pdf_export_draws_what_prosecraft_draws() {
    let fixture = fixture("export-pdf.json");
    let projects = fixture.get("projects").expect("projects");
    let mut report = Vec::new();
    let mut compared = 0;
    for (name, case) in fixture
        .get("exports")
        .and_then(JsValue::as_object)
        .expect("exports")
        .iter()
    {
        let project_id = case
            .get("project")
            .and_then(JsValue::as_js_string)
            .expect("project")
            .to_string_lossy();
        let mut stored = js::JsObject::new();
        stored.insert(
            "project",
            projects.get(&project_id).cloned().expect("project data"),
        );
        let mut project = Project::from_stored(&JsValue::Object(stored)).expect("project view");
        project.citations.clear();
        let options = PublishOptions::read(case.get("options"));
        let layout = layout_pdf(
            &compile_book(&project, &options),
            &options,
            &DataUrlSizes::default(),
        );
        let ours = write_pdf(&layout).expect("written");
        compare(
            &name.to_string_lossy(),
            &ours,
            case.get("pdf"),
            &mut report,
            &mut compared,
        );
    }
    eprintln!("compared {compared} draws");
    assert!(report.is_empty(), "{}", report.join("\n"));
}

/// The same book in two of ProseCraft's custom styles (`custom-styles.json`): Courier on
/// Letter and IBM Plex Sans (Helvetica in PDF) on A5, measured with those fonts' widths.
#[test]
fn pdf_export_in_a_custom_style_draws_what_prosecraft_draws() {
    let fixture = fixture("custom-styles.json");
    let mut stored = js::JsObject::new();
    stored.insert("project", fixture.get("project").cloned().expect("project"));
    let mut project = Project::from_stored(&JsValue::Object(stored)).expect("project view");
    project.citations.clear();
    let raw = js::stringify(fixture.get("raw").expect("raw"))
        .replace(r#""lineHeight":"NaN""#, r#""lineHeight":null"#);
    let mut report = Vec::new();
    let mut compared = 0;
    for (style, pdf) in fixture
        .get("pdf")
        .and_then(JsValue::as_object)
        .expect("pdf")
        .iter()
    {
        let style = style.to_string_lossy();
        let mut options = js::JsObject::new();
        options.insert("style", JsValue::str(&style));
        options.insert("customStyles", js::parse(&raw).expect("raw"));
        options.insert("dropCaps", JsValue::Bool(true));
        options.insert("includeTableOfContents", JsValue::Bool(true));
        let options = PublishOptions::read(Some(&JsValue::Object(options)));
        let layout = layout_pdf(
            &compile_book(&project, &options),
            &options,
            &DataUrlSizes::default(),
        );
        let ours = write_pdf(&layout).expect("written");
        compare(&style, &ours, Some(pdf), &mut report, &mut compared);
    }
    eprintln!("compared {compared} draws");
    assert!(compared > 0);
    assert!(report.is_empty(), "{}", report.join("\n"));
}

fn compare(
    name: &str,
    ours: &[u8],
    pdf: Option<&JsValue>,
    report: &mut Vec<String>,
    compared: &mut usize,
) {
    {
        let theirs = base64::engine::general_purpose::STANDARD
            .decode(
                pdf.and_then(JsValue::as_js_string)
                    .expect("pdf")
                    .to_string_lossy(),
            )
            .expect("base64");
        let (want, have) = (draws(&theirs), draws(ours));
        if want.len() != have.len() {
            report.push(format!("{name}: {} pages, want {}", have.len(), want.len()));
        }
        for (page, (w, h)) in want.iter().zip(&have).enumerate() {
            if w.len() != h.len() {
                report.push(format!(
                    "{name} page {}: {} draws, want {}",
                    page + 1,
                    h.len(),
                    w.len()
                ));
            }
            for (a, b) in w.iter().zip(h) {
                *compared += 1;
                if let Some(d) = differ(a, b) {
                    report.push(format!("{name} page {}: {d}", page + 1));
                    break;
                }
            }
        }
    }
}

#[test]
fn screenplay_pdf_draws_what_prosecraft_draws() {
    use prosecraft_core::screenplay::fountain::ScreenplayElement;
    use prosecraft_core::screenplay::pdf::layout_screenplay_pdf;
    let fixture = fixture("export-screenplay-pdf.json");
    let text = |v: &JsValue, key: &str| {
        v.get(key)
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    let mut report = Vec::new();
    let mut compared = 0;
    for (name, case) in fixture.as_object().expect("cases").iter() {
        let elements: Vec<ScreenplayElement> = case
            .get("elements")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|e| ScreenplayElement::new(&text(e, "type"), text(e, "text")))
            .collect();
        let layout = layout_screenplay_pdf(&text(case, "title"), &text(case, "author"), &elements);
        let ours = write_pdf(&layout).expect("written");
        let theirs = base64::engine::general_purpose::STANDARD
            .decode(text(case, "pdf"))
            .expect("base64");
        let (want, have) = (draws(&theirs), draws(&ours));
        let name = name.to_string_lossy();
        if want.len() != have.len() {
            report.push(format!("{name}: {} pages, want {}", have.len(), want.len()));
        }
        for (page, (w, h)) in want.iter().zip(&have).enumerate() {
            if w.len() != h.len() {
                report.push(format!(
                    "{name} page {}: {} draws, want {}",
                    page + 1,
                    h.len(),
                    w.len()
                ));
            }
            for (a, b) in w.iter().zip(h) {
                compared += 1;
                if let Some(d) = differ(a, b) {
                    report.push(format!("{name} page {}: {d}", page + 1));
                    break;
                }
            }
        }
    }
    eprintln!("compared {compared} draws");
    assert_eq!(compared, 343);
    assert!(report.is_empty(), "{}", report.join("\n"));
}
