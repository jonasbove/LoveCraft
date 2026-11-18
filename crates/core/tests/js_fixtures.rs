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

/// `formatCitation`, `formatBibliography` and `computeDisambiguation` in all eight styles, in
/// English and German, on ProseCraft's own output: 22 sources (types, name forms, particles,
/// corporate and missing authors, editors, 23 and 7 authors, editions, DOIs, links, no date),
/// each cited eight ways (locators, labels, prefix and suffix, author left out), and clusters.
#[test]
fn citations_are_formatted_as_prosecraft_formats_them() {
    use prosecraft_core::citations::format::{
        CitationContext, CitationInput, compute_disambiguation, format_bibliography,
        format_citation,
    };
    use std::collections::HashMap;
    let fx = fixture("citations-format.json");
    let list = |v: &JsValue, k: &str| {
        v.get(k)
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .to_vec()
    };
    let text = |v: &JsValue, k: &str| {
        v.get(k)
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
    };
    let library = list(&fx, "library");
    let variants = list(&fx, "variants");
    let input = |item: &JsValue, v: &JsValue| CitationInput {
        item: item.clone(),
        locator: text(v, "locator"),
        label: text(v, "label"),
        prefix: text(v, "prefix"),
        suffix: text(v, "suffix"),
        suppress_author: v.get("suppressAuthor") == Some(&JsValue::Bool(true)),
    };
    let same =
        |have: &prosecraft_core::citations::format::Formatted, want: &JsValue, what: &str| {
            assert_eq!(
                have.html,
                text(want, "html").unwrap_or_default(),
                "{what} (html)"
            );
            assert_eq!(
                have.text,
                text(want, "text").unwrap_or_default(),
                "{what} (text)"
            );
        };
    let mut checked = 0;
    for case in list(&fx, "styles") {
        let style = text(&case, "style").unwrap_or_default();
        let locale = text(&case, "locale").unwrap_or_default();
        let label = format!("{style}/{locale}");
        let mut suffixes: Vec<(String, String)> = compute_disambiguation(&style, &library, &locale)
            .into_iter()
            .collect();
        suffixes.sort();
        let mut want_suffixes: Vec<(String, String)> = case
            .get("yearSuffixes")
            .and_then(JsValue::as_object)
            .map(|o| {
                o.iter()
                    .map(|(k, v)| {
                        (
                            k.to_string_lossy(),
                            text(
                                &JsValue::Object({
                                    let mut x = js::JsObject::new();
                                    x.insert("v", v.clone());
                                    x
                                }),
                                "v",
                            )
                            .unwrap_or_default(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        want_suffixes.sort();
        assert_eq!(suffixes, want_suffixes, "{label} suffixes");
        let year_suffixes: HashMap<String, String> = suffixes.into_iter().collect();
        let bibliography = format_bibliography(&style, &library, &locale);
        let want = list(&case, "bibliography");
        assert_eq!(bibliography.len(), want.len(), "{label}");
        for (have, want) in bibliography.iter().zip(&want) {
            assert_eq!(Some(have.id.clone()), text(want, "id"), "{label} order");
            assert_eq!(
                Some(have.html.clone()),
                text(want, "html"),
                "{label} {}",
                have.id
            );
            assert_eq!(
                Some(have.text.clone()),
                text(want, "text"),
                "{label} {}",
                have.id
            );
            checked += 1;
        }
        for (i, (item, wants)) in library.iter().zip(list(&case, "singles")).enumerate() {
            for (v, want) in variants.iter().zip(wants.as_array().unwrap_or_default()) {
                let context = CitationContext {
                    index: Some((i + 1) as f64),
                    locale: locale.clone(),
                    year_suffixes: Some(year_suffixes.clone()),
                    ..CitationContext::default()
                };
                let have = format_citation(&style, &[input(item, v)], &context);
                same(
                    &have,
                    want,
                    &format!(
                        "{label} {} {}",
                        text(item, "id").unwrap_or_default(),
                        js::stringify(v)
                    ),
                );
                checked += 1;
            }
        }
        let clusters = list(&case, "clusters");
        let none = JsValue::Object(js::JsObject::new());
        let with = |k: &str, v: &str| {
            let mut o = js::JsObject::new();
            o.insert(k, JsValue::str(v));
            JsValue::Object(o)
        };
        let first = format_citation(
            &style,
            &[
                input(&library[0], &none),
                input(&library[2], &with("locator", "105")),
                input(&library[1], &none),
            ],
            &CitationContext {
                index: Some(4.0),
                locale: locale.clone(),
                year_suffixes: Some(year_suffixes.clone()),
                ..CitationContext::default()
            },
        );
        same(&first, &clusters[0], &format!("{label} cluster 1"));
        let second = format_citation(
            &style,
            &[input(&library[2], &none), input(&library[17], &none)],
            &CitationContext {
                indices: Some(
                    [("doe-roe".to_owned(), 3.0), ("two-same".to_owned(), 9.0)]
                        .into_iter()
                        .collect(),
                ),
                locale: locale.clone(),
                library: Some(library.clone()),
                ..CitationContext::default()
            },
        );
        same(&second, &clusters[1], &format!("{label} cluster 2"));
        let mut suppressed = input(&library[4], &none);
        suppressed.suppress_author = true;
        let third = format_citation(
            &style,
            &[input(&library[0], &with("prefix", "cf.")), suppressed],
            &CitationContext {
                locale: locale.clone(),
                library: Some(library.clone()),
                ..CitationContext::default()
            },
        );
        same(&third, &clusters[2], &format!("{label} cluster 3"));
        let empty = format_citation(
            &style,
            &[],
            &CitationContext {
                locale: locale.clone(),
                ..CitationContext::default()
            },
        );
        same(&empty, &clusters[3], &format!("{label} empty"));
        checked += 4;
    }
    assert!(checked > 3000, "{checked}");
}

fn list(value: &JsValue, key: &str) -> Vec<JsValue> {
    value
        .get(key)
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .to_vec()
}

fn optional(value: &JsValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
}

fn want(value: &JsValue, key: &str) -> String {
    value.get(key).map(js::stringify).unwrap_or_default()
}

fn strings(list: &[String]) -> JsValue {
    JsValue::Array(list.iter().map(|w| JsValue::str(w)).collect())
}

/// `{ items, warnings }` as JSON, the shape of the parsers' results.
fn items_and_warnings(items: Vec<JsValue>, warnings: &[String]) -> String {
    let mut o = js::JsObject::new();
    o.insert("items", JsValue::Array(items));
    o.insert("warnings", strings(warnings));
    js::stringify(&JsValue::Object(o))
}

/// The `imports`, `bibtex`, `ris` and `latex` parts; returns how many cases were checked.
fn check_citation_parsers(fx: &JsValue) -> usize {
    use prosecraft_core::citations::bibtex::{decode_latex, parse_bibtex};
    use prosecraft_core::citations::import::import_citations;
    use prosecraft_core::citations::ris::parse_ris;
    let mut checked = 0;
    for case in list(fx, "imports") {
        let result = import_citations(&text(&case, "text"), &text(&case, "filename"));
        let mut o = js::JsObject::new();
        let format = result
            .format
            .map_or(JsValue::Null, |f| JsValue::str(f.as_str()));
        o.insert("format", format);
        o.insert("items", JsValue::Array(result.items));
        o.insert("warnings", strings(&result.warnings));
        let have = js::stringify(&JsValue::Object(o));
        assert_eq!(
            have,
            want(&case, "result"),
            "import {}",
            js::stringify(&case)
        );
        checked += 1;
    }
    for case in list(fx, "bibtex") {
        let result = parse_bibtex(&text(&case, "text"));
        let have = items_and_warnings(result.items, &result.warnings);
        assert_eq!(
            have,
            want(&case, "result"),
            "bibtex {}",
            text(&case, "text")
        );
        checked += 1;
    }
    for case in list(fx, "ris") {
        let result = parse_ris(&text(&case, "text"));
        let have = items_and_warnings(result.items, &result.warnings);
        assert_eq!(have, want(&case, "result"), "ris {}", text(&case, "text"));
        checked += 1;
    }
    for case in list(fx, "latex") {
        let pages = matches!(case.get("pages"), Some(JsValue::Bool(true)));
        let input = text(&case, "input");
        let have = decode_latex(&input, pages);
        assert_eq!(have, text(&case, "out"), "latex {input:?} {pages}");
        checked += 1;
    }
    checked
}

/// The `names` and `parsedNames` parts and the library's items, years, contributors, types and
/// manual kinds; returns how many cases were checked.
fn check_citation_names_and_types(fx: &JsValue) -> usize {
    use prosecraft_core::citations::library::{
        ManualSourceKind, csl_type_to_manual_kind, format_author_list, format_item_contributors,
        item_year_text, manual_kind_to_csl_type, parse_author_list, source_type_label,
    };
    use prosecraft_core::csl::normalize_csl_items;
    let mut checked = 0;
    for case in list(fx, "names") {
        assert_eq!(
            format_author_list(&list(&case, "names")),
            text(&case, "list")
        );
        checked += 1;
    }
    for case in list(fx, "parsedNames") {
        let have = parse_author_list(&text(&case, "raw")).map_or(JsValue::Null, JsValue::Array);
        assert_eq!(
            js::stringify(&have),
            want(&case, "names"),
            "{}",
            text(&case, "raw")
        );
        checked += 1;
    }
    let library = fx.get("library").cloned().unwrap_or(JsValue::Null);
    let items = list(&library, "items");
    let input = js::parse(
        r#"[
        {"id":"mueller2020","citeKey":"mueller2020","type":"book","title":"Ü","author":[{"family":"Müller"}],"issued":{"date-parts":[[2020]]}},
        {"id":"x","type":"article-journal","title":"Y","issued":{"literal":"Spring 1999"}},
        {"id":"z","type":"webpage","title":"Z","editor":[{"family":"Ed"}],"issued":{"raw":"2001-02"}},
        {"id":"n","type":"thesis","title":"N"}]"#,
    )
    .expect("library input");
    let normalized = JsValue::Array(normalize_csl_items(&input));
    assert_eq!(js::stringify(&normalized), want(&library, "items"));
    let years: Vec<String> = items.iter().map(item_year_text).collect();
    assert_eq!(js::stringify(&strings(&years)), want(&library, "years"));
    let contributors: Vec<String> = items.iter().map(format_item_contributors).collect();
    assert_eq!(
        js::stringify(&strings(&contributors)),
        want(&library, "contributors")
    );
    checked += 3;
    for case in list(&library, "types") {
        let ty = text(&case, "type");
        assert_eq!(source_type_label(&ty), text(&case, "label"), "{ty}");
        let manual = csl_type_to_manual_kind(&ty).map(|k| k.as_str().to_owned());
        assert_eq!(manual, optional(&case, "manual"), "{ty}");
        checked += 1;
    }
    for case in list(&library, "manual") {
        let kind = ManualSourceKind::from_name(&text(&case, "kind")).expect("kind");
        assert_eq!(manual_kind_to_csl_type(kind), text(&case, "type"));
        checked += 1;
    }
    checked
}

/// The library's citation keys and merges; returns how many cases were checked.
fn check_citation_keys_and_merges(library: &JsValue) -> usize {
    use prosecraft_core::citations::library::{
        CitationKeySource, generate_citation_key, merge_imported_citations,
    };
    let mut checked = 0;
    for case in list(library, "keys") {
        let params = case.get("params").cloned().unwrap_or(JsValue::Null);
        let title = optional(&params, "title");
        let year = optional(&params, "year");
        let source = CitationKeySource {
            author: params.get("author").and_then(JsValue::as_array),
            editor: params.get("editor").and_then(JsValue::as_array),
            title: title.as_deref(),
            year: year.as_deref(),
        };
        let exclude = optional(&case, "exclude");
        let key = generate_citation_key(&source, &list(&case, "existing"), exclude.as_deref());
        assert_eq!(key, text(&case, "key"), "{}", js::stringify(&case));
        checked += 1;
    }
    for case in list(library, "merges") {
        let merged = merge_imported_citations(&list(&case, "existing"), &list(&case, "imported"));
        let mut o = js::JsObject::new();
        o.insert("items", JsValue::Array(merged.items));
        o.insert("addedCount", JsValue::Number(merged.added_count as f64));
        o.insert("warnings", strings(&merged.warnings));
        assert_eq!(js::stringify(&JsValue::Object(o)), want(&case, "result"));
        checked += 1;
    }
    checked
}

fn fixture_document(d: &JsValue) -> prosecraft_core::project::Document {
    use prosecraft_core::project::{Document, Footnote};
    Document {
        id: text(d, "id"),
        title: text(d, "title"),
        chapter_id: None,
        space: None,
        folder_id: None,
        trashed: false,
        label_id: None,
        screenplay: false,
        content: text(d, "content"),
        footnotes: list(d, "footnotes")
            .iter()
            .map(|f| Footnote {
                id: text(f, "id"),
                html: text(f, "html"),
            })
            .collect(),
        markdown_source: None,
    }
}

/// The library's citation counts and searches; returns how many cases were checked.
fn check_citation_counts_and_queries(library: &JsValue) -> usize {
    use prosecraft_core::citations::library::{count_source_citations, matches_library_query};
    let mut checked = 0;
    for case in list(library, "counts") {
        let documents: Vec<_> = list(&case, "documents")
            .iter()
            .map(fixture_document)
            .collect();
        let item = case.get("item").cloned().unwrap_or(JsValue::Null);
        let count = count_source_citations(&item, &documents);
        let mut o = js::JsObject::new();
        o.insert(
            "documentCount",
            JsValue::Number(count.document_count as f64),
        );
        o.insert("docIds", strings(&count.doc_ids));
        let have = js::stringify(&JsValue::Object(o));
        assert_eq!(have, want(&case, "count"), "{}", js::stringify(&item));
        checked += 1;
    }
    let items = list(library, "items");
    for case in list(library, "queries") {
        let query = text(&case, "query");
        let matches: Vec<JsValue> = items
            .iter()
            .map(|item| JsValue::Bool(matches_library_query(item, &query)))
            .collect();
        let have = js::stringify(&JsValue::Array(matches));
        assert_eq!(have, want(&case, "matches"), "{query}");
        checked += 1;
    }
    checked
}

/// `importCitations`, `parseBibtex`, `parseRis`, `decodeLatex` and the library helpers
/// (`library.ts`) on ProseCraft's own output: its import test inputs and more (string macros,
/// accents, malformed and unclosed entries, missing `ER`, CSL-JSON shapes and syntax errors),
/// author lists, citation keys, merges, citation counts and the library search.
#[test]
fn citation_import_matches_prosecraft() {
    let fx = fixture("citations-import.json");
    let library = fx.get("library").cloned().unwrap_or(JsValue::Null);
    let checked = check_citation_parsers(&fx)
        + check_citation_names_and_types(&fx)
        + check_citation_keys_and_merges(&library)
        + check_citation_counts_and_queries(&library);
    let count = |v: &JsValue, key: &str| list(v, key).len();
    let top = ["imports", "bibtex", "ris", "latex", "names", "parsedNames"];
    let lib = [
        "items", "types", "manual", "keys", "merges", "counts", "queries",
    ];
    for part in top {
        assert!(count(&fx, part) > 0, "{part}");
    }
    for part in lib {
        assert!(count(&library, part) > 0, "library.{part}");
    }
    // Every case of every part, and the library's items, years and contributors (3).
    let expected: usize = top.iter().map(|p| count(&fx, p)).sum::<usize>()
        + lib[1..].iter().map(|p| count(&library, p)).sum::<usize>()
        + 3;
    assert_eq!(checked, expected);
    assert!(checked > 100, "{checked}");
}

#[test]
fn editor_citation_labels_match_prosecraft() {
    use prosecraft_core::citations::labels::citation_labels;
    let fx = fixture("citation-labels.json");
    let text = |v: &JsValue| v.as_js_string().map(|s| s.to_string_lossy());
    let library = fx
        .get("library")
        .and_then(JsValue::as_array)
        .expect("library");
    let markers: Vec<String> = fx
        .get("markers")
        .and_then(JsValue::as_array)
        .expect("markers")
        .iter()
        .filter_map(text)
        .collect();
    let markers: Vec<&str> = markers.iter().map(String::as_str).collect();
    // The first six were made by `citeMarkerHtml` from these sources.
    let cite = |id: &str| prosecraft_core::cite::CiteRefItem {
        id: id.into(),
        ..Default::default()
    };
    let located = |id: &str, locator: &str, label: &str| prosecraft_core::cite::CiteRefItem {
        locator: Some(locator.into()),
        label: Some(label.into()),
        ..cite(id)
    };
    let made = [
        vec![cite("doe-roe")],
        vec![located("smithLH", "42", "page")],
        vec![
            prosecraft_core::cite::CiteRefItem {
                prefix: Some("see ".into()),
                suffix: Some("for more".into()),
                ..cite("smith2020b")
            },
            located("corp", "3", "chapter"),
        ],
        vec![cite("missing")],
        vec![
            prosecraft_core::cite::CiteRefItem {
                suppress_author: true,
                ..cite("nodate")
            },
            cite("gone"),
        ],
        vec![located("doe-roe", "7-9", "section")],
    ];
    for (items, marker) in made.iter().zip(&markers) {
        assert_eq!(
            prosecraft_core::cite::cite_marker_html(items).as_deref(),
            Some(*marker)
        );
    }
    assert_eq!(prosecraft_core::cite::cite_marker_html(&[]), None);
    assert_eq!(
        prosecraft_core::cite::cite_marker_html(&[cite("bad id!")]),
        None
    );
    let cases = fx.get("cases").and_then(JsValue::as_array).expect("cases");
    assert!(cases.len() > 16);
    for case in cases {
        let style = case.get("style").and_then(text).expect("style");
        let locale = case.get("locale").and_then(text).expect("locale");
        let items = if case.get("empty").is_some() {
            &[][..]
        } else {
            library
        };
        let expected: Vec<Option<String>> = case
            .get("labels")
            .and_then(JsValue::as_array)
            .expect("labels")
            .iter()
            .map(text)
            .collect();
        assert_eq!(expected.len(), markers.len());
        assert_eq!(
            citation_labels(&markers, items, &style, &locale),
            expected,
            "{style} {locale}"
        );
    }
}

/// A value without its `false` and `undefined` fields: ProseCraft sets a style key only when it
/// applies, and a few only sometimes write `false`.
fn without_false(value: &JsValue) -> JsValue {
    match value {
        JsValue::Object(o) => {
            let mut out = js::JsObject::new();
            for (k, v) in o.iter() {
                if !matches!(v, JsValue::Bool(false)) {
                    out.insert(k.clone(), without_false(v));
                }
            }
            JsValue::Object(out)
        }
        JsValue::Array(items) => JsValue::Array(items.iter().map(without_false).collect()),
        other => other.clone(),
    }
}

fn rich_image(image: &prosecraft_core::publish::rich_text::InlineImage) -> JsValue {
    let mut o = js::JsObject::new();
    o.insert("src", JsValue::str(&image.src));
    o.insert("alt", JsValue::str(&image.alt));
    if let Some(w) = image.width {
        o.insert("width", JsValue::Number(w));
    }
    if let Some(h) = image.height {
        o.insert("height", JsValue::Number(h));
    }
    JsValue::Object(o)
}

fn rich_run(run: &prosecraft_core::publish::rich_text::InlineRun) -> JsValue {
    let mut o = js::JsObject::new();
    o.insert("text", JsValue::str(&run.text));
    for (key, on) in [
        ("bold", run.bold),
        ("italic", run.italic),
        ("underline", run.underline),
        ("strike", run.strike),
        ("lineBreak", run.line_break),
    ] {
        if on {
            o.insert(key, JsValue::Bool(true));
        }
    }
    if let Some(link) = &run.link {
        o.insert("link", JsValue::str(link));
    }
    if let Some(image) = &run.image {
        o.insert("image", rich_image(image));
    }
    if let Some(id) = &run.footnote_id {
        o.insert("footnoteId", JsValue::str(id));
    }
    JsValue::Object(o)
}

fn rich_block(block: &prosecraft_core::publish::rich_text::Block) -> JsValue {
    use prosecraft_core::publish::rich_text::BlockKind;
    let mut o = js::JsObject::new();
    let kind = match block.kind {
        BlockKind::Paragraph => "paragraph",
        BlockKind::Heading => "heading",
        BlockKind::Quote => "quote",
        BlockKind::List => "list",
        BlockKind::Image => "image",
    };
    o.insert("kind", JsValue::str(kind));
    o.insert(
        "runs",
        JsValue::Array(block.runs.iter().map(rich_run).collect()),
    );
    if let Some(image) = &block.image {
        o.insert("image", rich_image(image));
    }
    if let Some(level) = block.level {
        o.insert("level", JsValue::Number(f64::from(level)));
    }
    if let Some(marker) = &block.marker {
        o.insert("marker", JsValue::str(marker));
    }
    if block.eyebrow {
        o.insert("eyebrow", JsValue::Bool(true));
    }
    JsValue::Object(o)
}

#[test]
fn publish_rich_text_matches_prosecraft() {
    use prosecraft_core::publish::rich_text::{blocks_from_html, inline_runs_from_html};
    let cases = fixture("publish-rich-text.json");
    let mut failures = Vec::new();
    for case in cases
        .get("blocks")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        let input = text(case, "input");
        let want = js::stable_stringify(&without_false(case.get("blocks").expect("blocks")));
        let have = js::stable_stringify(&JsValue::Array(
            blocks_from_html(&input).iter().map(rich_block).collect(),
        ));
        if have != want {
            failures.push(format!("{input:?}\n  want {want}\n  have {have}"));
        }
    }
    for case in cases
        .get("inline")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        let input = text(case, "input");
        let want = js::stable_stringify(&without_false(case.get("runs").expect("runs")));
        let have = js::stable_stringify(&JsValue::Array(
            inline_runs_from_html(&input).iter().map(rich_run).collect(),
        ));
        if have != want {
            failures.push(format!("inline {input:?}\n  want {want}\n  have {have}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
