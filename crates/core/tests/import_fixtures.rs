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
    assert_eq!(cases.len(), 25);
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

#[test]
fn screenplay_estimates_and_editor_rules_match_prosecraft() {
    use prosecraft_core::screenplay::editor_keys::{
        EditorKey, auto_case, auto_detect_type, next_element_type,
    };
    use prosecraft_core::screenplay::fountain::parse_fountain;
    use prosecraft_core::screenplay::layout::{
        estimate_pages, estimate_runtime, wrap_screenplay_text,
    };
    let all = fixture("fountain.json");
    let num = |v: &JsValue, k: &str| match v.get(k) {
        Some(JsValue::Number(n)) => *n as usize,
        _ => usize::MAX,
    };
    for case in all
        .get("cases")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        let (_, elements) = parse_fountain(&s(case, "input"));
        let have = estimate_pages(&elements);
        let want = case.get("estimate").cloned().unwrap_or(JsValue::Null);
        let scenes: Vec<(String, usize, usize)> = want
            .get("byScene")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|sc| (s(sc, "heading"), num(sc, "startPage"), num(sc, "pages")))
            .collect();
        assert_eq!(
            (have.pages, have.lines),
            (num(&want, "pages"), num(&want, "lines")),
            "{:?}",
            s(case, "input")
        );
        assert_eq!(
            have.by_scene
                .iter()
                .map(|sc| (sc.heading.clone(), sc.start_page, sc.pages))
                .collect::<Vec<_>>(),
            scenes
        );
        assert_eq!(estimate_runtime(have.pages), num(case, "runtime"));
    }
    for k in all
        .get("keys")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        let key = match s(k, "key").as_str() {
            "Enter" => EditorKey::Enter,
            "Tab" => EditorKey::Tab,
            _ => EditorKey::ShiftTab,
        };
        let empty = matches!(k.get("empty"), Some(JsValue::Bool(true)));
        assert_eq!(
            next_element_type(&s(k, "type"), key, empty),
            s(k, "next"),
            "{} {} {empty}",
            s(k, "type"),
            s(k, "key")
        );
    }
    for d in all
        .get("detect")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        let want = d
            .get("type")
            .and_then(JsValue::as_js_string)
            .map(|t| t.to_string_lossy());
        assert_eq!(
            auto_detect_type(&s(d, "line")).map(str::to_owned),
            want,
            "{:?}",
            s(d, "line")
        );
    }
    for c in all
        .get("cased")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        assert_eq!(auto_case(&s(c, "type"), "Mara's straße – ok"), s(c, "text"));
    }
    for w in all
        .get("wraps")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        let lines: Vec<String> = w
            .get("lines")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|l| l.as_js_string().map(|t| t.to_string_lossy()))
            .collect();
        assert_eq!(wrap_screenplay_text(&s(w, "text"), num(w, "width")), lines);
    }
}

#[test]
fn screenplay_scenes_are_editable_and_write_back_exactly() {
    use prosecraft_core::richtext::RichDoc;
    let all = fixture("fountain.json");
    let mut read_only = Vec::new();
    for case in all
        .get("cases")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        let html = s(case, "html");
        match RichDoc::from_html(&html) {
            Some(doc) => assert_eq!(doc.to_html(), html),
            None => read_only.push(html),
        }
    }
    assert!(read_only.is_empty(), "not editable: {read_only:?}");
    let new_scene = r#"<p data-sp="scene-heading"></p>"#;
    assert_eq!(
        RichDoc::from_html(new_scene)
            .map(|d| d.to_html())
            .as_deref(),
        Some(new_scene)
    );
    let typed = r#"<p data-sp="action"><br></p><p data-sp="character">MARA</p>"#;
    assert_eq!(
        RichDoc::from_html(typed).map(|d| d.to_html()).as_deref(),
        Some(typed)
    );
    assert!(RichDoc::from_html(r#"<p data-sp="unknown">x</p>"#).is_none());
}

#[test]
fn a_book_exports_to_fountain_as_prosecraft_exports_it() {
    use prosecraft_core::project::Project;
    use prosecraft_core::publish::{PublishOptions, compile_book};
    use prosecraft_core::screenplay::export::book_to_fountain;
    let all = fixture("fountain.json");
    for book in all
        .get("books")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        let mut data = js::JsObject::new();
        data.insert("name", JsValue::str(&s(book, "title")));
        data.insert("author", JsValue::str(&s(book, "author")));
        data.insert(
            "documents",
            all.get("documents").cloned().unwrap_or(JsValue::Null),
        );
        data.insert(
            "chapters",
            all.get("chapters").cloned().unwrap_or(JsValue::Null),
        );
        let mut stored = js::JsObject::new();
        stored.insert("project", JsValue::Object(data));
        let project = Project::from_stored(&JsValue::Object(stored)).expect("project");
        let compiled = compile_book(&project, &PublishOptions::read(None));
        assert_eq!(
            book_to_fountain(&compiled),
            s(book, "fountain"),
            "{}",
            s(book, "title")
        );
    }
}

#[test]
fn prose_converts_to_a_screenplay_as_prosecraft_converts_it() {
    use prosecraft_core::screenplay::convert::{convert_document, prose_to_screenplay_elements};
    let all = fixture("fountain.json");
    let conversions = all
        .get("conversions")
        .and_then(JsValue::as_array)
        .unwrap_or_default();
    assert_eq!(conversions.len(), 11);
    for case in conversions {
        let have = js::stringify(&JsValue::Array(
            prose_to_screenplay_elements(&s(case, "html"))
                .iter()
                .map(element_json)
                .collect(),
        ));
        assert_eq!(
            have,
            js::stringify(case.get("elements").unwrap_or(&JsValue::Null)),
            "{:?}",
            s(case, "html")
        );
    }
    let first = s(&conversions[0], "html");
    let mut doc = js::parse(&format!(r#"{{"id":"d1","title":"Prose","content":{},"chapterId":"c1","markdownSource":"Mara: x","labelId":"l1"}}"#, js::stringify(&JsValue::str(&first)))).expect("json");
    if let Some(o) = doc.as_object_mut() {
        convert_document(o);
    }
    assert_eq!(
        js::stringify(&doc),
        js::stringify(all.get("converted").unwrap_or(&JsValue::Null))
    );
}
