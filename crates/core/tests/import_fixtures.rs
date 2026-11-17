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

#[test]
fn rtf_reads_as_prosecraft_reads_it() {
    let all = fixture("scrivener-import.json");
    let cases = all
        .get("rtf")
        .and_then(JsValue::as_array)
        .unwrap_or_default();
    assert!(cases.len() >= 40);
    let mut failures = Vec::new();
    for case in cases {
        let input = base64::engine::general_purpose::STANDARD
            .decode(s(case, "input"))
            .expect("base64");
        let have = prosecraft_core::import::rtf::rtf_to_html(&input);
        let want = s(case, "html");
        if have != want {
            failures.push(format!(
                "{:?}\n  want {want:?}\n  have {have:?}",
                String::from_utf8_lossy(&input)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn report_json(r: &prosecraft_core::import::scrivener::ScrivenerReport) -> String {
    format!(
        r#"{{"chapters":{},"scenes":{},"wikiPages":{},"wikiFolders":{},"flattenedFolders":{},"skippedFiles":{},"itemsWithNotes":{},"missingFiles":{},"layout":"{}"}}"#,
        r.chapters,
        r.scenes,
        r.wiki_pages,
        r.wiki_folders,
        r.flattened_folders,
        r.skipped_files,
        r.items_with_notes,
        r.missing_files,
        r.layout
    )
}

#[test]
fn scrivener_import_matches_prosecraft() {
    use prosecraft_core::import::scrivener::{
        build_scrivener_project, files_from_entries, files_from_zip,
    };
    let all = fixture("scrivener-import.json");
    let mut failures = Vec::new();
    let mut check =
        |name: &str,
         got: Result<(JsValue, prosecraft_core::import::scrivener::ScrivenerReport), String>,
         case: &JsValue| {
            match (case.get("error").and_then(JsValue::as_js_string), got) {
                (Some(e), Err(g)) if e.to_string_lossy() == g => {}
                (None, Ok((project, report))) => {
                    let want = case.get("result").cloned().unwrap_or(JsValue::Null);
                    let want_project = js::stringify(want.get("project").unwrap_or(&JsValue::Null));
                    let want_report = js::stringify(want.get("report").unwrap_or(&JsValue::Null));
                    if js::stringify(&project) != want_project {
                        failures.push(format!(
                            "{name} project:\n  want {want_project}\n  have {}",
                            js::stringify(&project)
                        ));
                    }
                    if report_json(&report) != want_report {
                        failures.push(format!(
                            "{name} report:\n  want {want_report}\n  have {}",
                            report_json(&report)
                        ));
                    }
                }
                (want, got) => failures.push(format!(
                    "{name}: want error {:?}, got {:?}",
                    want.map(|e| e.to_string_lossy()),
                    got.map(|(_, r)| r)
                )),
            }
        };
    let projects = all
        .get("scrivener")
        .and_then(JsValue::as_array)
        .unwrap_or_default();
    assert_eq!(projects.len(), 14);
    for case in projects {
        let name = s(case, "name");
        let entries = case
            .get("files")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|f| {
                (
                    s(f, "path"),
                    base64::engine::general_purpose::STANDARD
                        .decode(s(f, "bytes"))
                        .expect("base64"),
                )
            })
            .collect();
        let project_name = case
            .get("projectName")
            .and_then(JsValue::as_js_string)
            .map(|t| t.to_string_lossy());
        let mut n = 0;
        let got = files_from_entries(entries).and_then(|files| {
            build_scrivener_project(
                &files,
                &mut || {
                    n += 1;
                    format!("id-{n}")
                },
                project_name.as_deref(),
            )
        });
        check(&name, got.map_err(|e| e.0), case);
    }
    let zips = all
        .get("zips")
        .and_then(JsValue::as_array)
        .unwrap_or_default();
    assert_eq!(zips.len(), 4);
    for case in zips {
        let name = s(case, "name");
        let data = base64::engine::general_purpose::STANDARD
            .decode(s(case, "zip"))
            .expect("base64");
        let mut n = 0;
        let got = files_from_zip(&data).and_then(|files| {
            build_scrivener_project(
                &files,
                &mut || {
                    n += 1;
                    format!("id-{n}")
                },
                None,
            )
        });
        check(&name, got.map_err(|e| e.0), case);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn element_json(e: &prosecraft_core::screenplay::fountain::ScreenplayElement) -> JsValue {
    let mut o = js::JsObject::new();
    o.insert("type", JsValue::str(&e.kind));
    o.insert("text", JsValue::str(&e.text));
    if let Some(level) = e.level {
        o.insert("level", JsValue::Number(level as f64));
    }
    if e.dual {
        o.insert("dual", JsValue::Bool(true));
    }
    if e.forced {
        o.insert("forced", JsValue::Bool(true));
    }
    if let Some(n) = &e.scene_number {
        o.insert("sceneNumber", JsValue::str(n));
    }
    JsValue::Object(o)
}

#[test]
fn fountain_reads_and_writes_as_prosecraft_does() {
    use prosecraft_core::screenplay::fountain::{parse_fountain, serialize_fountain};
    use prosecraft_core::screenplay::layout::{elements_to_html, html_to_elements};
    let all = fixture("fountain.json");
    let cases = all
        .get("cases")
        .and_then(JsValue::as_array)
        .unwrap_or_default();
    assert_eq!(cases.len(), 24);
    let mut failures = Vec::new();
    for case in cases {
        let input = s(case, "input");
        let (title, elements) = parse_fountain(&input);
        let mut t = js::JsObject::new();
        for (k, v) in &title {
            t.insert(k.as_str(), JsValue::str(v));
        }
        let have_title = js::stringify(&JsValue::Object(t));
        let have_elements =
            js::stringify(&JsValue::Array(elements.iter().map(element_json).collect()));
        let html = elements_to_html(&elements);
        let back = js::stringify(&JsValue::Array(
            html_to_elements(&html).iter().map(element_json).collect(),
        ));
        let fountain = serialize_fountain(&title, &elements);
        for (what, have, want) in [
            (
                "title",
                have_title,
                js::stringify(case.get("title").unwrap_or(&JsValue::Null)),
            ),
            (
                "elements",
                have_elements,
                js::stringify(case.get("elements").unwrap_or(&JsValue::Null)),
            ),
            ("html", html, s(case, "html")),
            (
                "back",
                back,
                js::stringify(case.get("back").unwrap_or(&JsValue::Null)),
            ),
            ("fountain", fountain, s(case, "fountain")),
        ] {
            if have != want {
                failures.push(format!("{input:?} {what}:\n  want {want}\n  have {have}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
