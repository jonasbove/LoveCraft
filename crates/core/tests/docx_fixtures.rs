//! DOCX export against files ProseCraft's own `exportDocx` wrote (`fixtures/export-docx.json`,
//! see `fixtures/README.md`).
//!
//! The two files can't match byte for byte: ProseCraft writes with the `docx` npm package, this
//! port with craft-docx. So both are read back with `craft_docx::read` and compared as a
//! normalized view of what a word processor would lay out: every story (body, each section's
//! header and footer, each footnote, followed from its reference), each paragraph's own
//! properties and text, each run's own formatting, links, fields, content controls, pictures
//! (size, alt text, bytes), sections, the styles the text uses, the document defaults and the
//! title, author and description. The formatting is printed as the model's JSON, which leaves
//! out only what a file doesn't set, so a property neither side was expected to touch still
//! shows when it differs.
//!
//! What is left out, and why:
//! - Ids: note, part, relationship and picture ids, and media names. Notes and header and
//!   footer stories are followed from where they are used instead.
//! - The package's own bookkeeping: dates, revision, "last modified by", the application name
//!   and settings ProseCraft doesn't choose.
//! - Footnote separators: the npm package writes its own (with a stray reference mark in them);
//!   craft-docx writes Word's defaults. ProseCraft sets neither.
//! - Styles the text doesn't use: the npm package defines a set of its own (Title, Heading 1–6,
//!   Caption, …) whatever the document holds.
//! - An empty `w:pgNumType` and `w:docGrid w:linePitch="360"` with no grid type in every npm
//!   section: neither changes the layout, and ProseCraft doesn't ask for them.
//! - The creator "Un-named": what the npm package writes when ProseCraft gives no author.

use std::collections::BTreeSet;

use base64::Engine;
use craft_wordmodel::para::OBJ;
use craft_wordmodel::{Block, Blocks, Document, InlineObject, Paragraph, SectionProps};
use prosecraft_core::js::{self, JsString, JsValue};
use prosecraft_core::project::Project;
use prosecraft_core::publish::{PublishOptions, compile_book, export_docx};

fn fixture() -> JsValue {
    let path = format!(
        "{}/tests/fixtures/export-docx.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    js::parse(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn string(value: Option<&JsValue>) -> String {
    value
        .and_then(JsValue::as_js_string)
        .and_then(JsString::as_string)
        .unwrap_or_default()
}

/// The value as compact JSON: the model leaves out what isn't set.
macro_rules! json {
    ($value:expr) => {
        serde_json::to_string($value).unwrap_or_default()
    };
}

/// The view of one document, line by line.
struct View<'a> {
    doc: &'a Document,
    lines: Vec<String>,
    /// Footnote stories in the order the text first refers to them.
    notes: Vec<u32>,
    styles: BTreeSet<String>,
}

impl<'a> View<'a> {
    fn new(doc: &'a Document) -> Self {
        Self {
            doc,
            lines: Vec::new(),
            notes: Vec::new(),
            styles: BTreeSet::new(),
        }
    }

    fn note_number(&mut self, part: u32) -> usize {
        match self.notes.iter().position(|n| *n == part) {
            Some(i) => i + 1,
            None => {
                self.notes.push(part);
                self.notes.len()
            }
        }
    }

    fn object(&mut self, object: &InlineObject, own_note: Option<u32>) -> String {
        match object {
            InlineObject::NoteRef { id, custom, .. } if Some(*id) == own_note => {
                format!("[mark{custom}]")
            }
            InlineObject::NoteRef { id, custom, .. } => {
                format!("[note {}{custom}]", self.note_number(*id))
            }
            InlineObject::Image {
                media, w, h, alt, ..
            } => {
                let bytes = self.doc.media.get(media).map(|b| b.as_slice());
                let sum = bytes.map_or(0u64, |b| {
                    b.iter()
                        .fold(0u64, |s, &x| s.wrapping_mul(31).wrapping_add(u64::from(x)))
                });
                // Points back to pixels, two decimals: EMU rounding is below that.
                format!(
                    "[image {:.2}x{:.2}px alt={alt:?} bytes={} sum={sum:x}]",
                    f64::from(*w) / 0.75,
                    f64::from(*h) / 0.75,
                    bytes.map_or(0, <[u8]>::len)
                )
            }
            other => format!("[{}]", json!(other)),
        }
    }

    fn paragraph(&mut self, p: &Paragraph, own_note: Option<u32>) -> String {
        if let Some(style) = &p.props.style {
            self.styles.insert(style.clone());
        }
        let mut props = p.props.clone();
        props.style = None;
        let mut out = format!(
            "P {} {}{}",
            p.props.style.as_deref().unwrap_or("-"),
            json!(&props),
            if p.section.is_some() { " §" } else { "" }
        );
        let mut objects = p.objects.iter();
        let mut last_format = None;
        for (range, chr) in p.run_ranges() {
            if let Some(style) = &chr.style {
                self.styles.insert(style.clone());
            }
            let format = json!(chr);
            let text = p.text.get(range).unwrap_or_default();
            for c in text.chars() {
                if last_format.as_ref() != Some(&format) {
                    out.push_str(&format!(" {format}:"));
                    last_format = Some(format.clone());
                }
                match c {
                    OBJ => {
                        let object = objects.next().map_or_else(
                            || "[missing object]".to_owned(),
                            |o| self.object(o, own_note),
                        );
                        out.push_str(&object);
                    }
                    '\n' => out.push('⏎'),
                    '\u{c}' => out.push('⇟'),
                    '\t' => out.push('⇥'),
                    c => out.push(c),
                }
            }
        }
        out
    }

    fn story(&mut self, name: &str, blocks: &Blocks, own_note: Option<u32>) {
        self.lines.push(format!("== {name}"));
        for block in blocks {
            let line = match &**block {
                Block::Para(p) => self.paragraph(p, own_note),
                Block::Table(_) => "TABLE".to_owned(),
            };
            self.lines.push(line);
        }
    }

    fn section(&mut self, index: usize, section: &SectionProps) {
        let mut own = section.clone();
        own.page_num_set = false;
        own.doc_grid = None;
        own.headers = Default::default();
        own.footers = Default::default();
        self.lines
            .push(format!("== section {index} {}", json!(&own)));
        for (kind, set) in [("header", &section.headers), ("footer", &section.footers)] {
            for (which, part) in [
                ("default", set.default),
                ("first", set.first),
                ("even", set.even),
            ] {
                if let Some(part) = part {
                    let blocks = self.doc.parts.get(&part).map(|p| p.blocks.clone());
                    self.story(
                        &format!("section {index} {kind} {which}"),
                        &blocks.unwrap_or_default(),
                        None,
                    );
                }
            }
        }
    }

    fn render(mut self) -> Vec<String> {
        let doc = self.doc;
        let creator = if doc.core.creator == "Un-named" {
            ""
        } else {
            &doc.core.creator
        };
        self.lines.push(format!(
            "core title={:?} creator={creator:?} description={:?}",
            doc.core.title, doc.core.description
        ));
        self.lines.push(format!(
            "defaults run={} paragraph={}",
            json!(&doc.styles.default_chr),
            json!(&doc.styles.default_para)
        ));
        self.lines
            .push(format!("skipped parts {:?}", doc.skipped_parts));
        self.story("body", &doc.body, None);
        for (index, (_, section)) in doc.sections().into_iter().enumerate() {
            self.section(index, section);
        }
        let mut k = 0;
        while let Some(&part) = self.notes.get(k) {
            let blocks = doc.parts.get(&part).map(|p| p.blocks.clone());
            self.story(
                &format!("footnote {}", k + 1),
                &blocks.unwrap_or_default(),
                Some(part),
            );
            k += 1;
        }
        // The styles the text uses and the styles they are based on, by id.
        let mut used: Vec<String> = self.styles.iter().cloned().collect();
        let mut i = 0;
        while let Some(id) = used.get(i).cloned() {
            if let Some(style) = doc.styles.styles.iter().find(|s| s.id == id) {
                for other in [&style.based_on, &style.next].into_iter().flatten() {
                    if !used.contains(other) {
                        used.push(other.clone());
                    }
                }
            }
            i += 1;
        }
        used.sort();
        for id in used {
            let line = match doc.styles.styles.iter().find(|s| s.id == id) {
                Some(style) => format!("style {}", json!(style)),
                None => format!("style {id} undefined"),
            };
            self.lines.push(line);
        }
        self.lines
    }
}

fn view(doc: &Document) -> Vec<String> {
    View::new(doc).render()
}

/// The first lines that differ, with a little context, or `None` when the views agree.
fn difference(want: &[String], got: &[String]) -> Option<String> {
    let first = (0..want.len().max(got.len())).find(|&i| want.get(i) != got.get(i))?;
    let mut out = format!("first difference at line {first}\n");
    for i in first.saturating_sub(2)..(first + 4).min(want.len().max(got.len())) {
        out.push_str(&format!(
            "  {i:>3} ProseCraft: {}\n  {i:>3} ours:       {}\n",
            want.get(i).map_or("<none>", String::as_str),
            got.get(i).map_or("<none>", String::as_str)
        ));
    }
    Some(out)
}

/// A fixture case: ProseCraft's file and ours, made from the same compiled book.
struct Case {
    name: String,
    theirs: Vec<u8>,
    ours: Vec<u8>,
    their_file_name: String,
    our_file_name: String,
}

/// Every fixture case.
fn cases() -> Vec<Case> {
    let fixture = fixture();
    let projects = fixture
        .get("projects")
        .and_then(JsValue::as_array)
        .expect("projects");
    let prepared: Vec<(String, String)> = fixture
        .get("prepared")
        .and_then(JsValue::as_array)
        .expect("prepared")
        .iter()
        .map(|pair| {
            let pair = pair.as_array().expect("pair");
            (string(pair.first()), string(pair.get(1)))
        })
        .collect();
    let cases = fixture
        .get("cases")
        .and_then(JsValue::as_array)
        .expect("cases");
    cases
        .iter()
        .map(|case| {
            let name = string(case.get("name"));
            let index = case
                .get("project")
                .and_then(JsValue::as_number)
                .expect("project") as usize;
            let mut stored = js::JsObject::new();
            stored.insert("project", projects[index].clone());
            let project = Project::from_stored(&JsValue::Object(stored)).expect("project");
            let options = PublishOptions::read(case.get("options"));
            let mut book = compile_book(&project, &options);
            // As `renderExport` prepares the book: the cover attached, the equations drawn.
            for chapter in &mut book.chapters {
                for scene in &mut chapter.scenes {
                    for (from, to) in &prepared {
                        scene.html = scene.html.replace(from.as_str(), to);
                    }
                }
            }
            book.cover = case
                .get("cover")
                .and_then(JsValue::as_js_string)
                .and_then(JsString::as_string);
            let theirs = base64::engine::general_purpose::STANDARD
                .decode(string(case.get("docx")))
                .expect("base64");
            let ours = export_docx(&book, &options).expect("export");
            Case {
                name,
                theirs,
                ours: ours.bytes,
                their_file_name: string(case.get("filename")),
                our_file_name: ours.file_name,
            }
        })
        .collect()
}

#[test]
fn docx_export_matches_prosecraft() {
    let cases = cases();
    assert!(cases.len() >= 7, "every case was generated");
    let mut failures = Vec::new();
    let mut lines = 0;
    for case in &cases {
        let name = &case.name;
        assert_eq!(case.our_file_name, case.their_file_name, "{name}");
        let theirs = craft_docx::read(&case.theirs).expect("ProseCraft's file reads");
        let ours = craft_docx::read(&case.ours).expect("our file reads");
        let (want, got) = (view(&theirs), view(&ours));
        lines += want.len();
        if std::env::var_os("DOCX_VIEW").is_some() {
            println!(
                "### {name} ProseCraft\n{}\n### {name} ours\n{}",
                want.join("\n"),
                got.join("\n")
            );
        }
        if let Some(diff) = difference(&want, &got) {
            failures.push(format!("{name}: {diff}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(lines > 400, "the views hold the documents ({lines} lines)");
}

/// Writes both files of each case to `$DOCX_OUT`, for opening in a word processor.
#[test]
fn write_both_files_when_asked() {
    let Some(dir) = std::env::var_os("DOCX_OUT") else {
        return;
    };
    let dir = std::path::Path::new(&dir);
    for case in cases() {
        let name = &case.name;
        std::fs::write(dir.join(format!("{name}-prosecraft.docx")), case.theirs).expect("write");
        std::fs::write(dir.join(format!("{name}-ours.docx")), case.ours).expect("write");
    }
}
