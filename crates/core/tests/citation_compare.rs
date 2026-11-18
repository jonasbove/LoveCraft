//! Differential comparison of the citation importers with ProseCraft's own on a large generated
//! corpus (see `docs/citation-compare.md`). The corpus is written by ProseCraft's TypeScript
//! (`src/citation-compare.gen.test.ts` on the fork's `tools/rust-fixtures` branch) and is not
//! kept in this repository, so the tests here are ignored and run on demand:
//!
//! ```sh
//! PROSECRAFT_CITATION_CORPUS=/tmp/citation-corpus.json \
//!   cargo test --release -p prosecraft-core --test citation_compare -- --ignored --nocapture
//! ```
//!
//! Every case's `import_citations(text, filename)` is compared with `importCitations` as JSON
//! text. Mismatches, panics and slow cases are written to a report next to the corpus
//! (`<corpus>.report.json`, or `PROSECRAFT_CITATION_REPORT`). Cases in the `js-prototype`
//! category (`constructor` and `__proto__` looked up in plain JavaScript objects) are counted
//! but do not fail the run.

use prosecraft_core::citations::import::import_citations;
use prosecraft_core::js::{self, JsObject, JsValue};
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

/// Categories whose differences are reported but expected (see the module comment).
const REPORT_ONLY: &[&str] = &["js-prototype"];
/// A case slower than this is a finding: the importers must stay linear on hostile input.
const SLOW: Duration = Duration::from_secs(2);

fn string(value: &JsValue, key: &str) -> String {
    value
        .get(key)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

/// `{ format, items, warnings }` as `JSON.stringify` writes ProseCraft's result.
fn run(text: &str, filename: &str) -> String {
    let result = import_citations(text, filename);
    let mut o = JsObject::new();
    let format = result
        .format
        .map_or(JsValue::Null, |f| JsValue::str(f.as_str()));
    o.insert("format", format);
    o.insert("items", JsValue::Array(result.items));
    o.insert(
        "warnings",
        JsValue::Array(result.warnings.iter().map(|w| JsValue::str(w)).collect()),
    );
    js::stringify(&JsValue::Object(o))
}

/// Where two JSON texts first differ, as the last object key before that point (a rough path,
/// enough to group mismatches) and a short excerpt of each side.
fn first_difference(want: &str, have: &str) -> (String, String, String) {
    let at = want
        .char_indices()
        .zip(have.chars())
        .find(|((_, a), b)| a != b)
        .map_or(want.len().min(have.len()), |((i, _), _)| i);
    let at = (0..=at)
        .rev()
        .find(|&i| have.is_char_boundary(i))
        .unwrap_or(0);
    let key = want[..at]
        .rfind("\":")
        .and_then(|end| {
            want[..end]
                .rfind('"')
                .map(|start| want[start + 1..end].to_string())
        })
        .unwrap_or_else(|| "(start)".into());
    let excerpt = |s: &str| {
        let start = (0..=at.saturating_sub(40))
            .rev()
            .find(|&i| s.is_char_boundary(i))
            .unwrap_or(0);
        s[start.min(s.len())..]
            .chars()
            .take(120)
            .collect::<String>()
    };
    (key, excerpt(want), excerpt(have))
}

/// ProseCraft's result with each warning's lone surrogates replaced by U+FFFD. A warning can end
/// up holding half a surrogate pair (`JSON.parse` names the unit it stopped at, and cuts its
/// excerpt by units); Rust's `String` warnings cannot hold one, and both show the same text.
fn lossy_warnings(result: &JsValue) -> String {
    let mut result = result.clone();
    if let Some(JsValue::Array(warnings)) =
        result.as_object_mut().and_then(|o| o.get_mut("warnings"))
    {
        for warning in warnings.iter_mut() {
            if let JsValue::String(text) = warning {
                *warning = JsValue::str(&text.to_string_lossy());
            }
        }
    }
    js::stringify(&result)
}

#[derive(Default)]
struct Tally {
    total: usize,
    mismatched: usize,
    lossy: usize,
}

#[test]
#[ignore = "needs a generated corpus; see docs/citation-compare.md"]
fn citation_import_matches_prosecraft_on_generated_corpus() {
    let Ok(path) = std::env::var("PROSECRAFT_CITATION_CORPUS") else {
        panic!("set PROSECRAFT_CITATION_CORPUS to a corpus file (docs/citation-compare.md)");
    };
    let report_path = std::env::var("PROSECRAFT_CITATION_REPORT")
        .unwrap_or_else(|_| format!("{path}.report.json"));
    let source = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let corpus = js::parse(&source).unwrap_or_else(|e| panic!("{path}: {e}"));
    drop(source);
    let meta = corpus.get("meta").cloned().unwrap_or(JsValue::Null);
    let cases = corpus
        .get("cases")
        .and_then(JsValue::as_array)
        .unwrap_or_else(|| panic!("{path}: no cases"));

    let mut tally: BTreeMap<String, Tally> = BTreeMap::new();
    let mut groups: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut findings = Vec::new();
    let mut slowest = (Duration::ZERO, 0.0);
    let started = Instant::now();
    for case in cases {
        let id = case.get("id").and_then(JsValue::as_number).unwrap_or(-1.0);
        let category = string(case, "category");
        let text = string(case, "text");
        let filename = string(case, "filename");
        let result = case.get("result").cloned().unwrap_or(JsValue::Null);
        let want = js::stringify(&result);
        let clock = Instant::now();
        let have = catch_unwind(AssertUnwindSafe(|| run(&text, &filename)));
        let took = clock.elapsed();
        if took > slowest.0 {
            slowest = (took, id);
        }
        let entry = tally.entry(category.clone()).or_default();
        entry.total += 1;
        let (problem, have) = match have {
            Ok(have) if have == want && took <= SLOW => continue,
            Ok(have) if have == want => (format!("slow ({took:?})"), have),
            Ok(have) if took <= SLOW && have == lossy_warnings(&result) => {
                entry.lossy += 1;
                continue;
            }
            Ok(have) => ("mismatch".to_string(), have),
            Err(_) => ("panic".to_string(), String::new()),
        };
        entry.mismatched += 1;
        let (key, want_at, have_at) = first_difference(&want, &have);
        *groups.entry((category.clone(), key.clone())).or_default() += 1;
        let mut o = JsObject::new();
        o.insert("id", JsValue::Number(id));
        o.insert("category", JsValue::str(&category));
        o.insert("problem", JsValue::str(&problem));
        o.insert("key", JsValue::str(&key));
        o.insert("filename", JsValue::str(&filename));
        o.insert("text", JsValue::str(&text));
        o.insert("wantAt", JsValue::str(&want_at));
        o.insert("haveAt", JsValue::str(&have_at));
        o.insert("want", JsValue::str(&want));
        o.insert("have", JsValue::str(&have));
        findings.push((text.len(), JsValue::Object(o)));
    }
    let elapsed = started.elapsed();
    // Smallest inputs first, so each group's first entry is its shortest reproduction.
    findings.sort_by_key(|(len, _)| *len);

    println!("corpus: {path}");
    println!("meta: {}", js::stringify(&meta));
    println!(
        "{:<26} {:>7} {:>10} {:>14}",
        "category", "cases", "different", "lossy-warning"
    );
    let mut total = 0;
    let mut failing = 0;
    let mut lossy = 0;
    for (category, t) in &tally {
        println!(
            "{category:<26} {:>7} {:>10} {:>14}",
            t.total, t.mismatched, t.lossy
        );
        total += t.total;
        lossy += t.lossy;
        if !REPORT_ONLY.contains(&category.as_str()) {
            failing += t.mismatched;
        }
    }
    println!(
        "{:<26} {total:>7} {failing:>10} {lossy:>14} (different: not counting report-only categories)",
        "all"
    );
    println!(
        "time: {elapsed:?}; slowest case {} took {:?}",
        slowest.1, slowest.0
    );
    for ((category, key), n) in &groups {
        println!("  group {category} / {key}: {n}");
    }

    let mut summary = JsObject::new();
    for (category, t) in &tally {
        let mut c = JsObject::new();
        c.insert("cases", JsValue::Number(t.total as f64));
        c.insert("different", JsValue::Number(t.mismatched as f64));
        c.insert("lossyWarning", JsValue::Number(t.lossy as f64));
        summary.insert(category.as_str(), JsValue::Object(c));
    }
    let mut report = JsObject::new();
    report.insert("meta", meta);
    report.insert("summary", JsValue::Object(summary));
    report.insert(
        "findings",
        JsValue::Array(findings.into_iter().map(|(_, f)| f).collect()),
    );
    std::fs::write(&report_path, js::stringify_pretty(&JsValue::Object(report)))
        .unwrap_or_else(|e| panic!("{report_path}: {e}"));
    println!("report: {report_path}");

    assert!(total > 0, "the corpus has no cases");
    assert_eq!(
        failing, 0,
        "{failing} cases differ from ProseCraft; see {report_path}"
    );
}

/// Runs `[{ text, filename }]` from one file and writes the results to another, for checking a
/// hand-shrunk input: `PROSECRAFT_CITATION_REPLAY=in.json,out.json` (the TypeScript generator
/// has the same mode for ProseCraft's side).
#[test]
#[ignore = "replay mode; see docs/citation-compare.md"]
fn citation_import_replay() {
    let Ok(spec) = std::env::var("PROSECRAFT_CITATION_REPLAY") else {
        panic!("set PROSECRAFT_CITATION_REPLAY=in.json,out.json");
    };
    let (input, output) = spec.split_once(',').unwrap_or((&spec, "/dev/stdout"));
    let source = std::fs::read_to_string(input).unwrap_or_else(|e| panic!("{input}: {e}"));
    let cases = js::parse(&source).unwrap_or_else(|e| panic!("{input}: {e}"));
    let results: Vec<JsValue> = cases
        .as_array()
        .unwrap_or_default()
        .iter()
        .map(|case| {
            let have = run(&string(case, "text"), &string(case, "filename"));
            js::parse(&have).unwrap_or(JsValue::Null)
        })
        .collect();
    std::fs::write(output, js::stringify(&JsValue::Array(results)))
        .unwrap_or_else(|e| panic!("{output}: {e}"));
}
