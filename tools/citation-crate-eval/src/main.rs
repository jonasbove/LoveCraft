//! Runs third-party BibTeX and RIS crates over the differential corpus (see
//! `docs/citation-compare.md`) and measures how far each one's import is from ProseCraft's.
//!
//! ```sh
//! cargo run --release -- /path/to/citation-corpus.json /path/to/crate-eval-report.json
//! # EVAL_TRACE=1 prints each case id and route to stderr before running it.
//! ```
//!
//! Two kinds of route are measured:
//!
//! - **tokenizer**: the crate reads the file (entries, fields, `@string` macros,
//!   concatenation); its field values are written back out as a plain BibTeX entry and handed
//!   to our `import_citations`, so the LaTeX decoding, names, dates and CSL mapping stay ours.
//!   A value the plain entry cannot carry (unbalanced braces, a trailing backslash, a type
//!   outside `[A-Za-z]+`) counts as a bridge failure, not as a crate difference.
//! - **full**: the crate's own model (its LaTeX decoding, names, dates, type names) mapped field
//!   by field onto ProseCraft's CSL items, then checked by our `normalize_csl_items_detailed`.

use prosecraft_core::citations::import::import_citations;
use prosecraft_core::csl::normalize_csl_items_detailed;
use prosecraft_core::js::{self, JsObject, JsValue};
use std::collections::{BTreeMap, HashMap};

/// What a route made of one input.
enum Outcome {
    /// Items and warnings.
    Done(Vec<JsValue>, Vec<String>),
    /// The crate rejected the whole file (with its error).
    Rejected(String),
    /// The crate read it, but the bridge to our mapping cannot express a value (which).
    Bridge(String),
    /// The crate panicked.
    Panic,
    /// The crate would overflow the stack (an abort, not a panic).
    Overflow,
}

type Entries = Vec<(String, String, Vec<(String, String)>)>;

// ------------------------------------------------------------------------------------------
// Shared helpers.

fn s(text: &str) -> JsValue {
    JsValue::str(text)
}

fn obj(pairs: Vec<(&str, JsValue)>) -> JsValue {
    let mut o = JsObject::new();
    for (k, v) in pairs {
        o.insert(k, v);
    }
    JsValue::Object(o)
}

/// ProseCraft's `BIBTEX_TYPE_MAP`.
fn bibtex_type(entry_type: &str) -> &'static str {
    match entry_type {
        "article" => "article-journal",
        "book" | "mvbook" | "collection" | "mvcollection" => "book",
        "inbook" | "incollection" | "bookinbook" => "chapter",
        "inproceedings" | "conference" | "proceedings" => "paper-conference",
        "phdthesis" | "mastersthesis" | "thesis" => "thesis",
        "techreport" | "report" => "report",
        "online" | "electronic" | "www" | "webpage" => "webpage",
        "unpublished" | "manuscript" => "manuscript",
        "dataset" => "dataset",
        "software" => "software",
        _ => "document",
    }
}

/// ProseCraft's `RIS_TYPE_MAP`.
fn ris_type(code: &str) -> &'static str {
    match code {
        "BOOK" | "WHOLE" | "EDBOOK" => "book",
        "JOUR" | "JFULL" | "EJOUR" => "article-journal",
        "MGZN" => "article-magazine",
        "NEWS" => "article-newspaper",
        "CHAP" | "SECT" => "chapter",
        "CONF" | "CPAPER" => "paper-conference",
        "THES" => "thesis",
        "RPRT" => "report",
        "ELEC" | "WEB" => "webpage",
        "ENCYC" => "entry-encyclopedia",
        "DICT" => "entry-dictionary",
        "MANSCPT" | "UNPB" => "manuscript",
        "DATA" => "dataset",
        "COMP" => "software",
        "PAT" => "patent",
        _ => "document",
    }
}

/// Whether `value` can sit inside `{…}` in a BibTeX entry unchanged.
fn braceable(value: &str) -> bool {
    let mut depth = 0i64;
    let mut escaped = false;
    for c in value.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0 && !escaped
}

/// Entries written back out as plain BibTeX: fields in order, each value braced.
fn emit(entries: &Entries) -> Result<String, String> {
    let mut out = String::new();
    for (kind, key, fields) in entries {
        if kind.is_empty() || !kind.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(format!("entry type {kind:?}"));
        }
        if key.is_empty()
            || key.contains([',', '{', '}', '='])
            || key.chars().any(char::is_whitespace)
        {
            return Err(format!("key {key:?}"));
        }
        out.push_str(&format!("@{kind}{{{key},\n"));
        for (name, value) in fields {
            if !braceable(value) {
                return Err(format!("value of {name}: {value:?}"));
            }
            out.push_str(&format!("  {name} = {{{value}}},\n"));
        }
        out.push_str("}\n");
    }
    Ok(out)
}

fn tokenized(entries: Entries) -> Outcome {
    match emit(&entries) {
        Ok(text) => {
            let result = import_citations(&text, "bridge.bib");
            Outcome::Done(result.items, result.warnings)
        }
        Err(e) => Outcome::Bridge(e),
    }
}

/// The items through `normalize_csl_items_detailed`, as `importCitations` finishes them.
fn normalized(items: Vec<JsValue>) -> Outcome {
    let n = normalize_csl_items_detailed(&JsValue::Array(items));
    Outcome::Done(n.items, n.warnings)
}

const MONTHS: [(&str, &str); 12] = [
    ("jan", "1"),
    ("feb", "2"),
    ("mar", "3"),
    ("apr", "4"),
    ("may", "5"),
    ("jun", "6"),
    ("jul", "7"),
    ("aug", "8"),
    ("sep", "9"),
    ("oct", "10"),
    ("nov", "11"),
    ("dec", "12"),
];

// ------------------------------------------------------------------------------------------
// biblatex (typst): RawBibliography as the tokenizer.

fn biblatex_raw(text: &str) -> Outcome {
    use biblatex::{RawBibliography, RawChunk};
    let raw = match RawBibliography::parse(text) {
        Ok(raw) => raw,
        Err(e) => return Outcome::Rejected(format!("{e}")),
    };
    // Macros apply from where they are defined on, as in ProseCraft.
    enum Item<'a, 's> {
        Macro(&'a biblatex::Pair<'s>),
        Entry(&'a biblatex::RawEntry<'s>),
    }
    let mut ordered: Vec<(usize, Item)> = raw
        .abbreviations
        .iter()
        .map(|p| (p.key.span.start, Item::Macro(p)))
        .chain(
            raw.entries
                .iter()
                .map(|e| (e.span.start, Item::Entry(&e.v))),
        )
        .collect();
    ordered.sort_by_key(|(at, _)| *at);
    let resolve = |field: &biblatex::Field, macros: &HashMap<String, String>| -> String {
        field
            .iter()
            .map(|chunk| match &chunk.v {
                RawChunk::Normal(t) => (*t).to_string(),
                RawChunk::Abbreviation(name) => {
                    let lower = name.to_lowercase();
                    macros
                        .get(&lower)
                        .cloned()
                        .or_else(|| {
                            MONTHS
                                .iter()
                                .find(|(m, _)| *m == lower)
                                .map(|(_, n)| (*n).to_string())
                        })
                        .unwrap_or_else(|| (*name).to_string())
                }
            })
            .collect()
    };
    let mut macros: HashMap<String, String> = HashMap::new();
    let mut entries = Vec::new();
    for (_, item) in ordered {
        match item {
            Item::Macro(pair) => {
                let value = resolve(&pair.value.v, &macros);
                macros.insert(pair.key.v.to_lowercase(), value);
            }
            Item::Entry(entry) => {
                let fields = entry
                    .fields
                    .iter()
                    .map(|p| (p.key.v.to_string(), resolve(&p.value.v, &macros)))
                    .collect();
                entries.push((entry.kind.v.to_string(), entry.key.v.to_string(), fields));
            }
        }
    }
    tokenized(entries)
}

// ------------------------------------------------------------------------------------------
// biblatex (typst): its own model, mapped onto ProseCraft's items.

/// Whether the `@string` macros refer to themselves (directly or through each other):
/// `Bibliography::parse` then recurses without bound and overflows the stack, which aborts the
/// process (no `catch_unwind` can stop it), so such files are counted here instead of run.
fn biblatex_macro_cycle(text: &str) -> bool {
    use biblatex::{RawBibliography, RawChunk};
    let Ok(raw) = RawBibliography::parse(text) else {
        return false;
    };
    let edges: HashMap<String, Vec<String>> = raw
        .abbreviations
        .iter()
        .map(|p| {
            let refs = p
                .value
                .v
                .iter()
                .filter_map(|c| match c.v {
                    RawChunk::Abbreviation(name) => Some(name.to_lowercase()),
                    RawChunk::Normal(_) => None,
                })
                .collect();
            (p.key.v.to_lowercase(), refs)
        })
        .collect();
    fn visits(node: &str, edges: &HashMap<String, Vec<String>>, path: &mut Vec<String>) -> bool {
        if path.iter().any(|p| p == node) {
            return true;
        }
        path.push(node.to_string());
        let found = edges
            .get(node)
            .is_some_and(|next| next.iter().any(|n| visits(n, edges, path)));
        path.pop();
        found
    }
    edges.keys().any(|k| visits(k, &edges, &mut Vec::new()))
}

fn biblatex_full(text: &str) -> Outcome {
    use biblatex::{Bibliography, ChunksExt, DateValue, PermissiveType};
    if biblatex_macro_cycle(text) {
        return Outcome::Overflow;
    }
    let bib = match Bibliography::parse(text) {
        Ok(bib) => bib,
        Err(e) => return Outcome::Rejected(format!("{e}")),
    };
    let people = |list: Vec<biblatex::Person>| {
        JsValue::Array(
            list.into_iter()
                .map(|p| {
                    let family = [p.prefix.as_str(), p.name.as_str()]
                        .iter()
                        .filter(|x| !x.is_empty())
                        .copied()
                        .collect::<Vec<_>>()
                        .join(" ");
                    let mut n = JsObject::new();
                    if !family.is_empty() {
                        n.insert("family", s(&family));
                    }
                    if !p.given_name.is_empty() {
                        n.insert("given", s(&p.given_name));
                    }
                    if !p.suffix.is_empty() {
                        n.insert("suffix", s(&p.suffix));
                    }
                    JsValue::Object(n)
                })
                .collect(),
        )
    };
    let mut items = Vec::new();
    for entry in bib.iter() {
        let kind = entry.entry_type.to_string().to_lowercase();
        let field = |k: &str| entry.get(k).map(|c| c.format_verbatim());
        let mut o = JsObject::new();
        o.insert("id", s(&entry.key));
        o.insert("type", s(bibtex_type(&kind)));
        o.insert("citeKey", s(&entry.key));
        o.insert("provider", s("import"));
        let put = |o: &mut JsObject, key: &str, value: Option<String>| {
            if let Some(v) = value.filter(|v| !v.trim().is_empty()) {
                o.insert(key, s(v.trim()));
            }
        };
        let title =
            field("title").or_else(|| (kind == "book").then(|| field("booktitle")).flatten());
        put(&mut o, "title", title);
        if let Some(a) = entry.author().ok().filter(|a| !a.is_empty()) {
            o.insert("author", people(a));
        }
        let editors: Vec<biblatex::Person> = entry
            .editors()
            .ok()
            .map(|e| e.into_iter().flat_map(|(people, _)| people).collect())
            .unwrap_or_default();
        if !editors.is_empty() {
            o.insert("editor", people(editors));
        }
        match entry.date() {
            Ok(PermissiveType::Typed(d)) => {
                let dt = match d.value {
                    DateValue::At(t) | DateValue::After(t) | DateValue::Before(t) => t,
                    DateValue::Between(t, _) => t,
                };
                let mut parts = vec![JsValue::Number(f64::from(dt.year))];
                if let Some(m) = dt.month {
                    parts.push(JsValue::Number(f64::from(m) + 1.0));
                    if let Some(d) = dt.day {
                        parts.push(JsValue::Number(f64::from(d) + 1.0));
                    }
                }
                o.insert(
                    "issued",
                    obj(vec![(
                        "date-parts",
                        JsValue::Array(vec![JsValue::Array(parts)]),
                    )]),
                );
            }
            Ok(PermissiveType::Chunks(c)) => {
                o.insert("issued", obj(vec![("literal", s(&c.format_verbatim()))]));
            }
            Err(_) => {}
        }
        let container = field("journal")
            .or_else(|| field("journaltitle"))
            .or_else(|| (kind != "book").then(|| field("booktitle")).flatten());
        put(&mut o, "container-title", container);
        let publisher = field("publisher")
            .or_else(|| field("school"))
            .or_else(|| field("institution"))
            .or_else(|| field("organization"));
        put(&mut o, "publisher", publisher);
        put(
            &mut o,
            "publisher-place",
            field("address").or_else(|| field("location")),
        );
        put(&mut o, "volume", field("volume"));
        put(&mut o, "issue", field("issue").or_else(|| field("number")));
        let page = field("pages").map(|p| p.replace(['–', '—'], "-").replace("--", "-"));
        put(&mut o, "page", page);
        put(&mut o, "DOI", field("doi"));
        put(&mut o, "URL", field("url"));
        put(&mut o, "ISBN", field("isbn"));
        put(&mut o, "edition", field("edition"));
        put(
            &mut o,
            "language",
            field("language").or_else(|| field("langid")),
        );
        put(&mut o, "abstract-free", field("abstract"));
        items.push(JsValue::Object(o));
    }
    normalized(items)
}

// ------------------------------------------------------------------------------------------
// nom-bibtex.

fn nom_bibtex(text: &str) -> Outcome {
    // nom-bibtex expands macros recursively too (same stack overflow).
    if biblatex_macro_cycle(text) {
        return Outcome::Overflow;
    }
    let bib = match nom_bibtex::Bibtex::parse(text) {
        Ok(bib) => bib,
        Err(e) => return Outcome::Rejected(format!("{e}")),
    };
    let entries = bib
        .bibliographies()
        .iter()
        .map(|b| {
            let mut fields: Vec<(String, String)> = b
                .tags()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            fields.sort();
            (
                b.entry_type().to_string(),
                b.citation_key().to_string(),
                fields,
            )
        })
        .collect();
    tokenized(entries)
}

// ------------------------------------------------------------------------------------------
// serde_bibtex.

#[derive(serde::Deserialize)]
struct SerdeEntry {
    entry_type: String,
    entry_key: String,
    fields: BTreeMap<String, String>,
}

fn serde_bibtex(text: &str) -> Outcome {
    // The month macros (`jan` = "1", ...) are opt-in; ProseCraft knows them.
    let mut macros = serde_bibtex::MacroDictionary::default();
    macros.set_month_macros();
    let de = serde_bibtex::de::Deserializer::from_str_with_macros(text, macros);
    let mut entries = Vec::new();
    for entry in de.into_iter_regular_entry::<SerdeEntry>() {
        match entry {
            Ok(e) => entries.push((e.entry_type, e.entry_key, e.fields.into_iter().collect())),
            Err(e) => return Outcome::Rejected(format!("{e}")),
        }
    }
    tokenized(entries)
}

// ------------------------------------------------------------------------------------------
// biblib: its BibTeX and RIS readers (only the normalised `Citation` is public).

fn biblib_type_code(display: &str) -> &str {
    match display {
        "Book" => "BOOK",
        "Book Chapter" => "CHAP",
        "Computer Program" => "COMP",
        "Conference Proceeding" => "CONF",
        "Data File" => "DATA",
        "Electronic Citation" => "ELEC",
        "Generic" => "GEN",
        "Journal/Periodical (Full)" => "JFULL",
        "Journal Article" => "JOUR",
        "Magazine Article" => "MGZN",
        "Newspaper" => "NEWS",
        "Patent" => "PAT",
        "Report" => "RPRT",
        "Thesis/Dissertation" => "THES",
        "Unpublished Work" => "UNPB",
        other => other,
    }
}

fn biblib_items(citations: Vec<biblib::Citation>, ris: bool) -> Vec<JsValue> {
    citations
        .into_iter()
        .map(|c| {
            let extra = |tag: &str| c.extra_fields.get(tag).and_then(|v| v.first()).cloned();
            let ty = c.citation_type.first().cloned().unwrap_or_default();
            let csl_type = if ris {
                ris_type(&biblib_type_code(&ty).to_uppercase())
            } else {
                bibtex_type(&ty.to_lowercase())
            };
            let id = extra("ID").unwrap_or_default();
            let mut o = JsObject::new();
            o.insert("id", s(&id));
            o.insert("type", s(csl_type));
            if !id.is_empty() {
                o.insert("citeKey", s(&id));
            }
            o.insert("provider", s("import"));
            let mut put = |key: &str, value: Option<String>| {
                if let Some(v) = value.filter(|v| !v.trim().is_empty()) {
                    o.insert(key, s(v.trim()));
                }
            };
            put("title", Some(c.title.clone()));
            put(
                "container-title",
                c.journal.clone().or(c.journal_abbr.clone()),
            );
            put("publisher", c.publisher.clone());
            put("publisher-place", extra("CY"));
            put("volume", c.volume.clone());
            put("issue", c.issue.clone());
            put("page", c.pages.clone());
            put("DOI", c.doi.clone());
            put("URL", c.urls.first().cloned());
            put("ISBN", c.issn.first().cloned());
            put("edition", extra("ET"));
            put("language", c.language.clone());
            put("abstract-free", c.abstract_text.clone());
            if !c.authors.is_empty() {
                let names = c
                    .authors
                    .iter()
                    .map(|a| {
                        let given = [a.given_name.clone(), a.middle_name.clone()]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(" ");
                        let mut n = JsObject::new();
                        n.insert("family", s(&a.name));
                        if !given.is_empty() {
                            n.insert("given", s(&given));
                        }
                        JsValue::Object(n)
                    })
                    .collect();
                o.insert("author", JsValue::Array(names));
            }
            if let Some(d) = &c.date {
                let mut parts = vec![JsValue::Number(f64::from(d.year))];
                if let Some(m) = d.month {
                    parts.push(JsValue::Number(f64::from(m)));
                    if let Some(day) = d.day {
                        parts.push(JsValue::Number(f64::from(day)));
                    }
                }
                o.insert(
                    "issued",
                    obj(vec![(
                        "date-parts",
                        JsValue::Array(vec![JsValue::Array(parts)]),
                    )]),
                );
            }
            JsValue::Object(o)
        })
        .collect()
}

fn biblib_ris(text: &str) -> Outcome {
    use biblib::CitationParser;
    match biblib::RisParser::new().parse(text) {
        Ok(c) => normalized(biblib_items(c, true)),
        Err(e) => Outcome::Rejected(format!("{e}")),
    }
}

fn biblib_bib(text: &str) -> Outcome {
    use biblib::CitationParser;
    match biblib::BibParser::new().parse(text) {
        Ok(c) => normalized(biblib_items(c, false)),
        Err(e) => Outcome::Rejected(format!("{e}")),
    }
}

// ------------------------------------------------------------------------------------------
// The run.

type Route = (&'static str, &'static str, fn(&str) -> Outcome);

const ROUTES: [Route; 6] = [
    ("biblatex (tokenizer)", "bibtex", biblatex_raw),
    ("biblatex (full)", "bibtex", biblatex_full),
    ("nom-bibtex (tokenizer)", "bibtex", nom_bibtex),
    ("serde_bibtex (tokenizer)", "bibtex", serde_bibtex),
    ("biblib (full)", "bibtex", biblib_bib),
    ("biblib (full)", "ris", biblib_ris),
];

#[derive(Default)]
struct Count {
    cases: usize,
    rejected: usize,
    bridge: usize,
    panics: usize,
    overflows: usize,
    items_differ: usize,
    warnings_only_differ: usize,
    same: usize,
}

/// `realistic` (what reference managers export), `generated` (grammar-built, awkward but
/// mostly well-formed: odd keys and types, undefined macros, LaTeX, Unicode) or `malformed`.
fn input_class(category: &str) -> &'static str {
    if category.ends_with("/realistic") {
        "realistic"
    } else if category.ends_with("/malformed") || category == "noise" || category == "detect" {
        "malformed"
    } else {
        "generated"
    }
}

fn id_of(case: &JsValue) -> usize {
    case.get("id").and_then(JsValue::as_number).unwrap_or(-1.0) as usize
}

fn text_of(value: Option<&JsValue>) -> String {
    value
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(corpus_path) = args.get(1) else {
        eprintln!("usage: citation-crate-eval <corpus.json> [report.json]");
        std::process::exit(2);
    };
    let report_path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| format!("{corpus_path}.crates.json"));
    let source = std::fs::read_to_string(corpus_path).expect("read corpus");
    let corpus = js::parse(&source).expect("parse corpus");
    let cases = corpus
        .get("cases")
        .and_then(JsValue::as_array)
        .expect("cases");
    std::panic::set_hook(Box::new(|_| {}));
    let trace = std::env::var_os("EVAL_TRACE").is_some();

    let mut counts: BTreeMap<(String, String, &str), Count> = BTreeMap::new();
    // Per route and first differing field: count and the smallest example.
    let mut fields: BTreeMap<(String, String), (usize, usize, JsValue)> = BTreeMap::new();
    let mut reasons: BTreeMap<(String, String), (usize, usize, JsValue)> = BTreeMap::new();
    for case in cases {
        let category = text_of(case.get("category"));
        if category == "js-prototype" || category == "cap" {
            continue;
        }
        let Some(result) = case.get("result") else {
            continue;
        };
        let format = text_of(result.get("format"));
        let text = text_of(case.get("text"));
        let want_items = js::stringify(result.get("items").unwrap_or(&JsValue::Null));
        let want_warnings = js::stringify(result.get("warnings").unwrap_or(&JsValue::Null));
        for (name, route_format, route) in ROUTES {
            if format != route_format {
                continue;
            }
            let c = counts
                .entry((name.to_string(), format.clone(), input_class(&category)))
                .or_default();
            c.cases += 1;
            if trace {
                eprintln!("{} {name}", id_of(case));
            }
            let outcome = std::panic::catch_unwind(|| route(&text)).unwrap_or(Outcome::Panic);
            let (items, warnings) = match outcome {
                Outcome::Rejected(why) => {
                    c.rejected += 1;
                    let class = input_class(&category);
                    let slot = reasons
                        .entry((format!("{name} [{format}] {class}"), reason(&why)))
                        .or_insert((0, usize::MAX, JsValue::Null));
                    slot.0 += 1;
                    if text.len() < slot.1 {
                        slot.1 = text.len();
                        slot.2 = obj(vec![("text", s(&text)), ("error", s(&why))]);
                    }
                    continue;
                }
                Outcome::Bridge(why) => {
                    c.bridge += 1;
                    let slot = reasons
                        .entry((format!("{name} [{format}] bridge"), reason(&why)))
                        .or_insert((0, usize::MAX, JsValue::Null));
                    slot.0 += 1;
                    if text.len() < slot.1 {
                        slot.1 = text.len();
                        slot.2 = obj(vec![("text", s(&text)), ("error", s(&why))]);
                    }
                    continue;
                }
                Outcome::Panic => {
                    c.panics += 1;
                    continue;
                }
                Outcome::Overflow => {
                    c.overflows += 1;
                    continue;
                }
                Outcome::Done(items, warnings) => (items, warnings),
            };
            let have_items = js::stringify(&JsValue::Array(items.clone()));
            let have_warnings =
                js::stringify(&JsValue::Array(warnings.iter().map(|w| s(w)).collect()));
            if have_items != want_items {
                c.items_differ += 1;
                let want = result
                    .get("items")
                    .and_then(JsValue::as_array)
                    .unwrap_or_default();
                let class = input_class(&category);
                for (key, want_field, have_field) in differing_fields(want, &items) {
                    let slot = fields
                        .entry((format!("{name} [{format}] {class}"), key))
                        .or_insert((0, usize::MAX, JsValue::Null));
                    slot.0 += 1;
                    if text.len() < slot.1 {
                        slot.1 = text.len();
                        slot.2 = obj(vec![
                            ("category", s(&category)),
                            ("text", s(&text)),
                            ("wantField", s(&want_field)),
                            ("haveField", s(&have_field)),
                            ("want", s(&want_items)),
                            ("have", s(&have_items)),
                        ]);
                    }
                }
            } else if have_warnings != want_warnings {
                c.warnings_only_differ += 1;
            } else {
                c.same += 1;
            }
        }
    }

    println!(
        "{:<26} {:<7} {:<11} {:>6} {:>9} {:>7} {:>6} {:>9} {:>12} {:>14} {:>9}",
        "route",
        "format",
        "input",
        "cases",
        "rejected",
        "bridge",
        "panic",
        "overflow",
        "items differ",
        "warnings only",
        "identical"
    );
    let mut summary = Vec::new();
    for ((name, format, input), c) in &counts {
        println!(
            "{name:<26} {format:<7} {input:<11} {:>6} {:>9} {:>7} {:>6} {:>9} {:>12} {:>14} {:>9}",
            c.cases,
            c.rejected,
            c.bridge,
            c.panics,
            c.overflows,
            c.items_differ,
            c.warnings_only_differ,
            c.same
        );
        let n = |v: usize| JsValue::Number(v as f64);
        summary.push(obj(vec![
            ("route", s(name)),
            ("format", s(format)),
            ("input", s(input)),
            ("cases", n(c.cases)),
            ("rejected", n(c.rejected)),
            ("bridge", n(c.bridge)),
            ("panics", n(c.panics)),
            ("stackOverflows", n(c.overflows)),
            ("itemsDiffer", n(c.items_differ)),
            ("warningsOnlyDiffer", n(c.warnings_only_differ)),
            ("identical", n(c.same)),
        ]));
    }
    println!("\nfirst differing field (count):");
    let mut examples = Vec::new();
    for ((route, key), (count, _, example)) in &fields {
        println!("  {route:<36} {key:<28} {count}");
        examples.push(obj(vec![
            ("route", s(route)),
            ("field", s(key)),
            ("count", JsValue::Number(*count as f64)),
            ("smallest", example.clone()),
        ]));
    }
    println!("\nrejections and bridge failures (count):");
    let mut rejections = Vec::new();
    for ((route, why), (count, _, example)) in &reasons {
        println!("  {route:<44} {why:<50} {count}");
        rejections.push(obj(vec![
            ("route", s(route)),
            ("reason", s(why)),
            ("count", JsValue::Number(*count as f64)),
            ("smallest", example.clone()),
        ]));
    }
    let report = obj(vec![
        ("rejections", JsValue::Array(rejections)),
        ("meta", corpus.get("meta").cloned().unwrap_or(JsValue::Null)),
        ("summary", JsValue::Array(summary)),
        ("differences", JsValue::Array(examples)),
    ]);
    std::fs::write(&report_path, js::stringify_pretty(&report)).expect("write report");
    println!("report: {report_path}");
}

/// An error message with digits and quoted parts blanked, to group by kind.
fn reason(why: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    for c in why.chars() {
        if c == '"' || c == '`' {
            quoted = !quoted;
            out.push('"');
        } else if !quoted {
            out.push(if c.is_ascii_digit() { '#' } else { c });
        }
    }
    let mut out: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    out.truncate(out.char_indices().nth(50).map_or(out.len(), |(i, _)| i));
    out
}

/// Each field that differs between ProseCraft's items and the route's (once, with the first
/// pair of values), or the item count when that differs.
fn differing_fields(want: &[JsValue], have: &[JsValue]) -> Vec<(String, String, String)> {
    if want.len() != have.len() {
        let n = |l: &[JsValue]| l.len().min(9);
        return vec![(
            format!("item count ({} vs {})", n(want), n(have)),
            String::new(),
            String::new(),
        )];
    }
    let show = |v: Option<&JsValue>| v.map(js::stringify).unwrap_or_default();
    let mut out: Vec<(String, String, String)> = Vec::new();
    for (w, h) in want.iter().zip(have) {
        let (Some(w), Some(h)) = (w.as_object(), h.as_object()) else {
            continue;
        };
        let mut keys: Vec<String> = w
            .iter()
            .chain(h.iter())
            .map(|(k, _)| k.to_string_lossy())
            .collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            if w.get(&k) != h.get(&k) && !out.iter().any(|(seen, _, _)| *seen == k) {
                out.push((k.clone(), show(w.get(&k)), show(h.get(&k))));
            }
        }
    }
    out
}
