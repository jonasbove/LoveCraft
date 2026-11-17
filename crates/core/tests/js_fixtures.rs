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

#[test]
fn plain_text_and_word_counts_match_prosecraft() {
    use prosecraft_core::html::{PlainTextOptions, RichText, html_to_plain_text, html_word_count};
    let cases = fixture("plain-text.json");
    for case in cases.as_array().unwrap_or_default() {
        let clean = text(case, "clean");
        let omit = html_to_plain_text(
            &clean,
            &PlainTextOptions {
                rich: RichText::Omit,
                ..Default::default()
            },
        );
        assert_eq!(omit, text(case, "omit"), "omit for {clean}");
        let source = html_to_plain_text(
            &clean,
            &PlainTextOptions {
                rich: RichText::Source,
                ..Default::default()
            },
        );
        assert_eq!(source, text(case, "source"), "source for {clean}");
        let words = case
            .get("words")
            .and_then(JsValue::as_number)
            .unwrap_or(-1.0);
        assert_eq!(html_word_count(&clean) as f64, words, "words for {clean}");
    }
}

#[test]
fn prosecraft_rich_html_round_trips_through_the_editor_model() {
    use prosecraft_core::richtext::RichDoc;
    let cases = fixture("rich-corpus.json");
    let mut editable = 0;
    for case in cases.as_array().unwrap_or_default() {
        let html = text(case, "html");
        match RichDoc::from_html(&html) {
            Some(doc) => {
                assert_eq!(doc.to_html(), html);
                editable += 1;
            }
            None => println!("read-only: {html:?}"),
        }
    }
    println!(
        "{editable} of {} editable",
        cases.as_array().map_or(0, <[JsValue]>::len)
    );
    assert!(editable >= 14, "only {editable} editable");
}

#[test]
fn sanitizer_matches_prosecraft() {
    use prosecraft_core::html::{sanitize_html, sanitize_inline_html};
    let fixture = fixture("sanitize.json");
    let mut failures = Vec::new();
    for case in fixture
        .get("cases")
        .and_then(JsValue::as_array)
        .expect("cases")
    {
        let html = text(case, "html");
        // The one deliberate difference: script and style content is dropped wherever it is.
        if html.contains("<style>") {
            continue;
        }
        for (got, key) in [
            (sanitize_html(&html), "full"),
            (sanitize_inline_html(&html), "inline"),
        ] {
            let want = text(case, key);
            if got != want {
                failures.push(format!("{key}: {html:?}\n  want {want:?}\n  got  {got:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn markdown_matches_prosecraft() {
    use prosecraft_core::markdown::{FootnoteDef, html_to_inline_markdown, html_to_markdown};
    let fixture = fixture("markdown.json");
    let notes = [
        FootnoteDef {
            id: "n1".into(),
            html: "A <em>note</em>\nover lines.".into(),
        },
        FootnoteDef {
            id: "bad id".into(),
            html: "x".into(),
        },
        FootnoteDef {
            id: "n2".into(),
            html: String::new(),
        },
    ];
    let mut failures = Vec::new();
    for case in fixture
        .get("cases")
        .and_then(JsValue::as_array)
        .expect("cases")
    {
        let html = text(case, "html");
        for (got, key) in [
            (html_to_markdown(&html, &[]), "markdown"),
            (html_to_inline_markdown(&html), "inline"),
            (html_to_markdown(&html, &notes), "withNotes"),
        ] {
            let want = text(case, key);
            if got != want {
                failures.push(format!("{key}: {html:?}\n  want {want:?}\n  got  {got:?}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn comment_anchors_match_prosecraft() {
    use prosecraft_core::comments::{Anchor, build_anchor, comment_text, locate_anchor};
    let fixture = fixture("comments.json");
    let units = |v: &JsValue| {
        v.as_js_string()
            .map(|s| s.units().to_vec())
            .expect("string")
    };
    for case in fixture
        .get("texts")
        .and_then(JsValue::as_array)
        .expect("texts")
    {
        let html = text(case, "html");
        assert_eq!(
            comment_text(&html),
            units(case.get("text").expect("text")),
            "{html}"
        );
    }
    let sample = units(fixture.get("sample").expect("sample"));
    let read = |a: &JsValue| Anchor {
        quote: units(a.get("quote").expect("q")),
        prefix: units(a.get("prefix").expect("p")),
        suffix: units(a.get("suffix").expect("s")),
    };
    for case in fixture
        .get("anchors")
        .and_then(JsValue::as_array)
        .expect("anchors")
    {
        let n = |k: &str| case.get(k).and_then(JsValue::as_number).expect(k) as usize;
        let want = case.get("anchor").filter(|a| !a.is_null()).map(read);
        assert_eq!(
            build_anchor(&sample, n("start"), n("end")),
            want,
            "{} {}",
            n("start"),
            n("end")
        );
    }
    let edited = units(fixture.get("edited").expect("edited"));
    for case in fixture
        .get("located")
        .and_then(JsValue::as_array)
        .expect("located")
    {
        let found = case.get("found").filter(|f| !f.is_null()).map(|f| {
            (
                f.get("start").and_then(JsValue::as_number).expect("s") as usize,
                f.get("end").and_then(JsValue::as_number).expect("e") as usize,
            )
        });
        assert_eq!(
            locate_anchor(&edited, &read(case.get("anchor").expect("anchor"))),
            found
        );
    }
}

#[test]
fn starters_build_as_prosecraft_builds_them() {
    use prosecraft_core::starters::{
        add_module, all_keys, build_from_blueprint, screenplay_blueprint, starter_blueprint,
        starters,
    };
    let fixture = fixture("starters.json");
    let counter = || {
        let mut n = 0;
        move || {
            n += 1;
            format!("id-{n}")
        }
    };
    let pair = |(folders, documents): (Vec<JsValue>, Vec<JsValue>)| {
        let mut o = js::JsObject::new();
        o.insert("folders", JsValue::Array(folders));
        o.insert("documents", JsValue::Array(documents));
        js::stringify(&JsValue::Object(o))
    };
    assert_eq!(starters().len(), 8);
    for starter in starters() {
        let want = fixture
            .get("builds")
            .and_then(|b| b.get(&starter.id))
            .expect("build");
        assert_eq!(
            pair(build_from_blueprint(
                &starter_blueprint(&starter.id),
                &mut counter()
            )),
            js::stringify(want),
            "{}",
            starter.id
        );
    }
    let screenplay = fixture.get("screenplay").expect("screenplay");
    assert_eq!(
        pair(build_from_blueprint(
            &screenplay_blueprint(),
            &mut counter()
        )),
        js::stringify(screenplay.get("build").expect("build"))
    );
    let keys: Vec<String> = screenplay
        .get("keys")
        .and_then(JsValue::as_array)
        .expect("keys")
        .iter()
        .map(|k| k.as_js_string().expect("key").to_string_lossy())
        .collect();
    assert_eq!(all_keys(&screenplay_blueprint()), keys);
    // An unknown module adds nothing, as there.
    assert_eq!(
        add_module(&starter_blueprint("fantasy"), "factions"),
        starter_blueprint("fantasy")
    );
}

#[test]
fn search_and_replace_match_prosecraft() {
    use prosecraft_core::search::{
        SearchOptions, find_first_match, replace_in_documents, search_documents,
    };
    let fixture = fixture("search.json");
    let documents = fixture
        .get("documents")
        .and_then(JsValue::as_array)
        .expect("docs")
        .to_vec();
    let chapters = fixture
        .get("chapters")
        .and_then(JsValue::as_array)
        .expect("chapters")
        .to_vec();
    let flag = |o: &JsValue, k: &str| matches!(o.get(k), Some(JsValue::Bool(true)));
    let opts = |o: &JsValue| SearchOptions {
        case_sensitive: flag(o, "caseSensitive"),
        whole_word: flag(o, "wholeWord"),
        include_trashed: flag(o, "includeTrashed"),
    };
    let n = |v: &JsValue, k: &str| v.get(k).and_then(JsValue::as_number).expect(k) as usize;
    for case in fixture
        .get("searches")
        .and_then(JsValue::as_array)
        .expect("searches")
    {
        let query = text(case, "query");
        let got = search_documents(
            &documents,
            &chapters,
            &query,
            opts(case.get("options").expect("o")),
        );
        let want = case.get("hits").and_then(JsValue::as_array).expect("hits");
        assert_eq!(got.len(), want.len(), "{query:?}");
        for (g, w) in got.iter().zip(want) {
            let snippet = w
                .get("snippet")
                .and_then(JsValue::as_js_string)
                .map(|s| s.units().to_vec())
                .expect("snippet");
            let chapter = w
                .get("chapterTitle")
                .and_then(JsValue::as_js_string)
                .map(|s| s.to_string_lossy());
            assert_eq!(
                (
                    g.document_id.as_str(),
                    &g.snippet,
                    g.match_start,
                    g.match_length,
                    g.matches,
                    g.wiki,
                    &g.chapter_title
                ),
                (
                    text(w, "documentId").as_str(),
                    &snippet,
                    n(w, "matchStart"),
                    n(w, "matchLength"),
                    n(w, "matches"),
                    text(w, "kind") == "wiki",
                    &chapter
                ),
                "{query:?}"
            );
        }
    }
    for case in fixture
        .get("replaces")
        .and_then(JsValue::as_array)
        .expect("replaces")
    {
        let mut docs = documents.clone();
        let options = case.get("options").expect("o");
        let result = replace_in_documents(
            &mut docs,
            &text(case, "query"),
            &text(case, "replacement"),
            opts(options),
            flag(options, "includeTitles"),
            None,
        );
        let changed: Vec<(String, usize)> = case
            .get("changed")
            .and_then(JsValue::as_array)
            .expect("changed")
            .iter()
            .map(|c| (text(c, "id"), n(c, "count")))
            .collect();
        assert_eq!(result.changed, changed, "{}", text(case, "query"));
        assert_eq!(result.total, n(case, "total"));
        for want in case
            .get("documents")
            .and_then(JsValue::as_array)
            .expect("docs")
        {
            let got = docs
                .iter()
                .find(|d| d.get("id") == want.get("id"))
                .expect("doc");
            assert_eq!(js::stringify(got), js::stringify(want));
        }
    }
    let mut docs = documents.clone();
    let only = replace_in_documents(
        &mut docs,
        "dark",
        "light",
        SearchOptions::default(),
        false,
        Some(&["s2".to_owned(), "w1".to_owned()]),
    );
    assert_eq!(only.total, n(fixture.get("only").expect("only"), "total"));
    for case in fixture
        .get("firsts")
        .and_then(JsValue::as_array)
        .expect("firsts")
    {
        let want = case
            .get("match")
            .filter(|m| !m.is_null())
            .map(|m| (n(m, "start"), n(m, "end")));
        assert_eq!(
            find_first_match(
                &text(case, "text"),
                &text(case, "query"),
                opts(case.get("options").expect("o"))
            ),
            want,
            "{}",
            text(case, "text")
        );
    }
}

#[test]
fn custom_starters_match_prosecraft() {
    use prosecraft_core::starters::custom::{
        CUSTOM_STARTERS_KEY, add_custom_folder, add_custom_page, custom_starters_json,
        custom_starters_value, normalize_custom_starters, remove_custom_folder, remove_custom_page,
        rename_custom_folder, rename_custom_page,
    };
    use prosecraft_core::starters::{
        Blueprint, all_keys, filter_blueprint, read_blueprint, starter_blueprint,
    };
    let all = fixture("custom-starters.json");
    assert_eq!(text(&all, "key"), CUSTOM_STARTERS_KEY);
    let normalized = normalize_custom_starters(all.get("stored").expect("stored"));
    assert_eq!(
        js::stringify(&custom_starters_value(&normalized)),
        js::stringify(all.get("normalized").expect("normalized"))
    );
    assert_eq!(custom_starters_json(&normalized), text(&all, "saved"));
    // The same edits as ProseCraft's, step by step.
    let mut working = starter_blueprint("fantasy");
    working = add_custom_folder(&working, "  My folder ");
    let folder = working
        .folders
        .last()
        .map(|f| f.key.clone())
        .unwrap_or_default();
    working = add_custom_page(&working, &folder, " Page one ", "<p>Hi</p>");
    working = add_custom_page(&working, &folder, "", "");
    working = add_custom_folder(&working, "");
    working = rename_custom_folder(&working, &folder, " Renamed ");
    let first = working
        .folders
        .iter()
        .find(|f| f.key == folder)
        .map(|f| f.pages[0].key.clone())
        .unwrap_or_default();
    working = rename_custom_page(&working, &first, "   ");
    let second = working
        .folders
        .iter()
        .find(|f| f.key == folder)
        .map(|f| f.pages[1].key.clone())
        .unwrap_or_default();
    working = remove_custom_page(&working, &second);
    let head = working.folders[0].key.clone();
    working = remove_custom_folder(&working, &head);
    let as_value = |b: &Blueprint| {
        custom_starters_value(&[prosecraft_core::starters::custom::CustomStarter {
            id: "x".into(),
            name: "x".into(),
            blueprint: b.clone(),
        }])
    };
    let want = |key: &str| read_blueprint(all.get(key).expect(key));
    assert_eq!(
        js::stringify(&as_value(&working)),
        js::stringify(&as_value(&want("working")))
    );
    let keys: Vec<String> = all_keys(&working)
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i % 3 != 1)
        .map(|(_, k)| k)
        .collect();
    assert_eq!(filter_blueprint(&working, &keys), want("template"));
}

/// `normalizeCslItemsDetailed` and `sanitizeCitationId` on ProseCraft's own output: cleaning,
/// caps, fallback ids, duplicates, dates, names, links, and the 5,000-item cap.
#[test]
fn citation_items_are_normalized_as_prosecraft_normalizes_them() {
    use prosecraft_core::csl::{normalize_csl_items_detailed, sanitize_citation_id};
    let fx = fixture("csl.json");
    for case in fx
        .get("cases")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        let input = case.get("input").cloned().unwrap_or(JsValue::Null);
        let have = normalize_csl_items_detailed(&input);
        let want = case.get("output").unwrap_or(&JsValue::Null);
        assert_eq!(
            js::stringify(&JsValue::Array(have.items)),
            want.get("items").map(js::stringify).unwrap_or_default(),
            "{}",
            js::stringify(&input)
        );
        let warnings: Vec<JsValue> = have.warnings.iter().map(|w| JsValue::str(w)).collect();
        assert_eq!(
            js::stringify(&JsValue::Array(warnings)),
            want.get("warnings").map(js::stringify).unwrap_or_default()
        );
    }
    for case in fx
        .get("ids")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        let text = |k: &str| {
            case.get(k)
                .and_then(JsValue::as_js_string)
                .map(|s| s.to_string_lossy())
                .unwrap_or_default()
        };
        assert_eq!(
            sanitize_citation_id(&text("raw")),
            text("id"),
            "{:?}",
            text("raw")
        );
    }
    let many: Vec<JsValue> = (0..5003)
        .map(|i| {
            js::parse(&format!(r#"{{"id":"r{i}","type":"book","title":"T{i}"}}"#))
                .unwrap_or(JsValue::Null)
        })
        .collect();
    let capped = normalize_csl_items_detailed(&JsValue::Array(many));
    let want = fx.get("capped").unwrap_or(&JsValue::Null);
    assert_eq!(
        capped.items.len() as f64,
        want.get("count")
            .and_then(JsValue::as_number)
            .unwrap_or(-1.0)
    );
    assert_eq!(
        capped.items.last().map(js::stringify),
        want.get("last").map(js::stringify)
    );
    assert_eq!(capped.warnings.len(), 1);
}
