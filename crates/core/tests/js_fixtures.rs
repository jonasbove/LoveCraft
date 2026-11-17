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
