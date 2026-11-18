//! Zotero against ProseCraft's own `zotero.ts` (`fixtures/zotero.json`, written by the fork's
//! `tools/rust-fixtures-zotero` generator): search paths, error wording, item keys, mapping, and
//! the provider through a transport answering from a table.

use prosecraft_core::citations::zotero::{
    self, ErrorCode, ZoteroError, classify_error, classify_http_error, extract_key, map_csl_json,
    search_path,
};
use prosecraft_core::js::{self, JsValue};

fn fixture() -> JsValue {
    let path = format!("{}/tests/fixtures/zotero.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    js::parse(&text).unwrap_or_else(|e| panic!("{path}: {e:?}"))
}

fn text(value: Option<&JsValue>) -> String {
    value
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

fn cases<'a>(fixture: &'a JsValue, key: &str) -> &'a [JsValue] {
    fixture
        .get(key)
        .and_then(JsValue::as_array)
        .unwrap_or_default()
}

/// A state as the fixture writes it (`detail` only when there is one).
fn state(error: &ZoteroError) -> String {
    let mut out = format!("{}|{}", error.code.as_str(), error.message);
    if let Some(detail) = &error.detail {
        out.push('|');
        out.push_str(detail);
    }
    out
}

fn expected_state(value: Option<&JsValue>) -> String {
    let value = value.expect("a state");
    let mut out = format!("{}|{}", text(value.get("code")), text(value.get("message")));
    if let Some(detail) = value.get("detail") {
        out.push('|');
        out.push_str(&text(Some(detail)));
    }
    out
}

#[test]
fn constants_and_search_paths() {
    let fixture = fixture();
    let constants = fixture.get("constants").expect("constants");
    assert_eq!(text(constants.get("ping")), zotero::PING_PATH);
    assert_eq!(text(constants.get("cayw")), zotero::CAYW_PATH);
    assert_eq!(
        constants.get("limit").and_then(JsValue::as_number),
        Some(f64::from(zotero::SEARCH_LIMIT))
    );
    let paths = cases(&fixture, "paths");
    assert_eq!(paths.len(), 54);
    for case in paths {
        let limit = match case.get("limit") {
            Some(JsValue::Number(n)) => Some(*n),
            Some(JsValue::String(_)) => Some(f64::NAN),
            _ => None,
        };
        let query = text(case.get("query"));
        assert_eq!(
            search_path(&query, limit),
            text(case.get("path")),
            "{query:?} {limit:?}"
        );
    }
}

#[test]
fn errors_are_worded_as_prosecraft_words_them() {
    let fixture = fixture();
    for case in cases(&fixture, "http") {
        let status = case
            .get("status")
            .and_then(JsValue::as_number)
            .unwrap_or(0.0) as u16;
        let body = text(case.get("body"));
        assert_eq!(
            state(&classify_http_error(status, &body)),
            expected_state(case.get("state")),
            "{status} {body:?}"
        );
    }
    let errors = cases(&fixture, "errors");
    assert!(errors.len() > 20);
    for case in errors {
        let raw = text(case.get("raw"));
        if raw.starts_with("(ZoteroError") {
            continue;
        }
        assert_eq!(
            state(&classify_error(&raw)),
            expected_state(case.get("state")),
            "{raw:?}"
        );
    }
}

#[test]
fn keys_come_from_uris_fields_and_ids() {
    let fixture = fixture();
    for case in cases(&fixture, "keys") {
        let arg = |name: &str| match case.get(name) {
            Some(JsValue::String(s)) if s.eq_str("(undefined)") => None,
            Some(JsValue::String(s)) if s.eq_str("(Infinity)") => {
                Some(JsValue::Number(f64::INFINITY))
            }
            other => other.cloned(),
        };
        let (id, key) = (arg("id"), arg("key"));
        let expected = case
            .get("out")
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy());
        assert_eq!(
            extract_key(id.as_ref(), key.as_ref()),
            expected,
            "{id:?} {key:?}"
        );
    }
}

#[test]
fn answers_map_to_citation_items() {
    let fixture = fixture();
    let mapped = cases(&fixture, "mapped");
    assert_eq!(mapped.len(), 16);
    for case in mapped {
        let raw = text(case.get("text"));
        let items = map_csl_json(&raw).unwrap_or_else(|e| panic!("{raw:?}: {e:?}"));
        let expected = case.get("items").expect("items");
        assert_eq!(
            js::stringify(&JsValue::Array(items)),
            js::stringify(expected),
            "{raw}"
        );
    }
    for case in cases(&fixture, "invalid") {
        let raw = text(case.get("text"));
        let error = map_csl_json(&raw).expect_err("invalid JSON");
        let expected = case.get("error").expect("error");
        assert_eq!(error.code, ErrorCode::InvalidResponse);
        assert_eq!(error.message, text(expected.get("message")), "{raw:?}");
        // The detail is the JSON parser's own message, which differs between engines.
        assert!(error.detail.is_some());
    }
}

#[test]
fn the_provider_through_a_transport() {
    let fixture = fixture();
    let texts: Vec<String> = cases(&fixture, "mapped")
        .iter()
        .map(|c| text(c.get("text")))
        .collect();
    let search = search_path("  rosa ", None);
    let not_running = "Zotero is not running (connection refused on 127.0.0.1:23119).";
    let forbidden = "HTTP 403: Local API is not enabled";
    // The generator's answer tables, in its order.
    let tables: Vec<Vec<(&str, Result<String, String>)>> = vec![
        vec![
            (zotero::PING_PATH, Ok("Zotero is running".into())),
            (&search, Ok(texts[0].clone())),
            (zotero::CAYW_PATH, Ok(texts[1].clone())),
        ],
        vec![
            (zotero::PING_PATH, Ok("ok".into())),
            (&search, Ok("{broken".into())),
            (zotero::CAYW_PATH, Ok("  ".into())),
        ],
        vec![("*", Err(not_running.into()))],
        vec![
            (zotero::PING_PATH, Err(forbidden.into())),
            (&search, Err(forbidden.into())),
        ],
        vec![
            (zotero::PING_PATH, Ok("ok".into())),
            (&search, Err("Zotero request timed out after 5s.".into())),
            (zotero::CAYW_PATH, Err("HTTP 404".into())),
        ],
    ];
    let providers = cases(&fixture, "providers");
    assert_eq!(providers.len(), tables.len());
    for (table, expected) in tables.iter().zip(providers) {
        let calls = std::cell::RefCell::new(Vec::<String>::new());
        let transport = |path: &str| -> Result<String, String> {
            calls.borrow_mut().push(path.to_owned());
            table
                .iter()
                .find(|(p, _)| *p == path)
                .or_else(|| table.iter().find(|(p, _)| *p == "*"))
                .map_or_else(|| Err("HTTP 404".to_owned()), |(_, a)| a.clone())
        };
        let status = zotero::check_status(&transport);
        let found = zotero::search(&transport, "  rosa ");
        let pick = zotero::try_pick(&transport);
        let want_calls: Vec<String> = expected
            .get("calls")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .map(|c| text(Some(c)))
            .collect();
        assert_eq!(*calls.borrow(), want_calls);
        let want_status = expected.get("status").expect("status");
        assert_eq!(
            status.is_ok(),
            matches!(want_status.get("available"), Some(JsValue::Bool(true)))
        );
        if let Err(error) = &status {
            assert_eq!(state(error), expected_state(want_status.get("error")));
        }
        let want_search = expected.get("search").expect("search");
        match (&found, want_search.get("items"), want_search.get("error")) {
            (Ok(items), Some(want), _) => assert_eq!(
                js::stringify(&JsValue::Array(items.clone())),
                js::stringify(want)
            ),
            (Err(error), _, Some(want)) => assert_eq!(state(error), expected_state(Some(want))),
            other => panic!("search differs: {other:?}"),
        }
        let want_pick = expected.get("pick").expect("pick");
        let supported = matches!(want_pick.get("supported"), Some(JsValue::Bool(true)));
        assert_eq!(pick.is_some(), supported);
        if let Some(items) = pick {
            assert_eq!(
                js::stringify(&JsValue::Array(items)),
                js::stringify(want_pick.get("items").expect("items"))
            );
        }
    }
}
