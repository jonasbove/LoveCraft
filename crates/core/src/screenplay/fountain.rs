//! Fountain 1.1 (ProseCraft `src/lib/screenplay/fountain.ts`, upstream `7dae544`): parsing
//! and writing screenplay documents.
//!
//! The model is structural only: emphasis marks (`*italic*`, `**bold**`, `***bold italic***`,
//! `_underline_`) stay as written in `text`, notes become their own elements so no content is
//! lost, and the boneyard is dropped. ProseCraft's regular expressions are matched here by
//! hand, with their exact semantics.

use crate::html::is_js_whitespace;

/// One element of a screenplay.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScreenplayElement {
    /// "scene-heading", "action", "character", "dialogue", "parenthetical", "transition",
    /// "centered", "section", "synopsis", "note" or "page-break".
    pub kind: String,
    pub text: String,
    /// A section's level: one per leading `#`.
    pub level: Option<usize>,
    /// The character cue starts a dual dialogue block (`^`).
    pub dual: bool,
    /// Written with a forcing marker (`.`, `!`, `@`, `>`).
    pub forced: bool,
    pub scene_number: Option<String>,
}

impl ScreenplayElement {
    pub fn new(kind: &str, text: impl Into<String>) -> Self {
        Self {
            kind: kind.to_owned(),
            text: text.into(),
            ..Self::default()
        }
    }

    fn forced(mut self) -> Self {
        self.forced = true;
        self
    }
}

/// The title page: keys as written, in order (a repeated key keeps its first place).
pub type TitlePage = Vec<(String, String)>;

const TITLE_PAGE_KEYS: [&str; 12] = [
    "title",
    "credit",
    "author",
    "author(s)",
    "authors",
    "source",
    "notes",
    "draft date",
    "date",
    "contact",
    "copyright",
    "revision",
];

fn trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

fn trim_end(s: &str) -> &str {
    s.trim_end_matches(is_js_whitespace)
}

/// `.` in a JavaScript pattern: anything but a line terminator.
fn no_line_terminator(s: &str) -> bool {
    !s.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}

fn set_title(title: &mut TitlePage, key: &str, value: String) {
    match title.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = value,
        None => title.push((key.to_owned(), value)),
    }
}

/// `TITLE_PAGE_LINE` (`^([A-Za-z][A-Za-z ()]*?)\s*:\s*(.*)$`): the key and the value.
fn title_page_line(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let key = trim_end(&line[..colon]);
    let mut chars = key.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        || !chars.all(|c| c.is_ascii_alphabetic() || matches!(c, ' ' | '(' | ')'))
    {
        return None;
    }
    // `\s*` may take line terminators; `(.*)` may not.
    let value = &line[colon + 1..];
    no_line_terminator(value.trim_start_matches(is_js_whitespace)).then_some((key, value))
}

/// `SCENE_HEADING` (`^(?:INT\.?\/EXT\.?|I\/E\.?|INT|EXT|EST)(?:\.|\s)`, case-insensitive).
pub fn is_scene_heading(text: &str) -> bool {
    const STARTS: [&str; 9] = [
        "INT/EXT",
        "INT./EXT",
        "INT/EXT.",
        "INT./EXT.",
        "I/E",
        "I/E.",
        "INT",
        "EXT",
        "EST",
    ];
    STARTS.iter().any(|start| {
        text.get(..start.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(start))
            && text[start.len()..]
                .chars()
                .next()
                .is_some_and(|c| c == '.' || is_js_whitespace(c))
    })
}

fn is_transition(text: &str) -> bool {
    text.ends_with("TO:") && text == text.to_uppercase()
}

/// `text.replace(/\s*\([^)]*\)$/, "")`: a trailing parenthetical (the leftmost that fits) and
/// the whitespace before it go.
fn without_trailing_parenthetical(text: &str) -> &str {
    let Some(body) = text.strip_suffix(')') else {
        return text;
    };
    let from = body.rfind(')').map_or(0, |i| i + 1);
    let Some(open) = body[from..].find('(').map(|i| i + from) else {
        return text;
    };
    trim_end(&text[..open])
}

fn is_character_text(text: &str) -> bool {
    if text.is_empty() || is_scene_heading(text) || is_transition(text) {
        return false;
    }
    if text.starts_with(['#', '=', '.', '>', '!', '[', '@']) {
        return false;
    }
    let name = without_trailing_parenthetical(text);
    name.chars().any(|c| c.is_ascii_uppercase()) && !name.chars().any(|c| c.is_ascii_lowercase())
}

/// Action needs a `!` when it would otherwise be read as another element.
fn needs_action_marker(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    if text.starts_with(['.', '!', '>', '#', '=']) || text.starts_with("[[") {
        return true;
    }
    is_scene_heading(text) || is_transition(text)
}

/// `SCENE_NUMBER` (`#([^#]+)#\s*$`): the text without it, and the number.
fn read_scene_number(text: &str) -> (String, Option<String>) {
    let body = trim_end(text);
    let Some(inner) = body.strip_suffix('#') else {
        return (text.to_owned(), None);
    };
    let Some(open) = inner.rfind('#') else {
        return (text.to_owned(), None);
    };
    let number = &inner[open + 1..];
    if number.is_empty() {
        return (text.to_owned(), None);
    }
    let number = trim(number);
    if number.is_empty() {
        return (text.to_owned(), None);
    }
    (trim_end(&text[..open]).to_owned(), Some(number.to_owned()))
}

fn read_character_cue(text: &str) -> Option<(String, bool, bool)> {
    let forced = text.starts_with('@');
    let mut cue = if forced { trim(&text[1..]) } else { text };
    let mut dual = false;
    if let Some(rest) = cue.strip_suffix('^') {
        cue = trim(rest);
        dual = true;
    }
    if cue.is_empty() || (!forced && !is_character_text(cue)) {
        return None;
    }
    Some((cue.to_owned(), dual, forced))
}

/// `extractNotes`: inline notes leave their line, taking only one space that followed them.
fn extract_notes(line: &str) -> (String, Vec<String>) {
    let mut notes = Vec::new();
    let mut text = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("[[") {
        let Some(end) = rest[start + 2..].find("]]").map(|e| e + start + 2) else {
            break;
        };
        text.push_str(&rest[..start]);
        notes.push(trim(&rest[start + 2..end]).to_owned());
        rest = &rest[end + 2..];
        rest = rest.strip_prefix(' ').unwrap_or(rest);
    }
    text.push_str(rest);
    (text, notes)
}

/// `readNote`: a note that starts a line and may span several.
fn read_note(lines: &[String], index: usize, line: &str) -> (String, String, usize) {
    let opening = line.find("[[").unwrap_or(0);
    let first = &line[opening + 2..];
    if let Some(closing) = first.find("]]") {
        return (
            trim(&first[..closing]).to_owned(),
            first[closing + 2..].to_owned(),
            index,
        );
    }
    let mut parts = vec![first.to_owned()];
    let mut next = index;
    while next < lines.len() {
        let current = &lines[next];
        next += 1;
        if let Some(end) = current.find("]]") {
            parts.push(current[..end].to_owned());
            return (
                trim(&parts.join("\n")).to_owned(),
                current[end + 2..].to_owned(),
                next,
            );
        }
        parts.push(current.clone());
    }
    (trim(&parts.join("\n")).to_owned(), String::new(), next)
}

fn read_title_page(lines: &[String]) -> (TitlePage, usize) {
    let mut title = TitlePage::new();
    let mut index = 0;
    let mut key: Option<String> = None;
    while index < lines.len() {
        let line = &lines[index];
        if trim(line).is_empty() {
            break;
        }
        if let Some((k, v)) = title_page_line(line)
            && TITLE_PAGE_KEYS.contains(&trim(k).to_lowercase().as_str())
        {
            let k = trim(k).to_owned();
            set_title(&mut title, &k, trim(v).to_owned());
            key = Some(k);
            index += 1;
            continue;
        }
        // Indented lines continue the value of the previous key.
        let Some(k) = key.clone() else { break };
        if !line.starts_with(is_js_whitespace) {
            break;
        }
        let previous = title
            .iter()
            .find(|(t, _)| *t == k)
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        set_title(&mut title, &k, format!("{previous}\n{}", trim(line)));
        index += 1;
    }
    (title, index)
}

/// The boneyard (`/* … */`, or to the end when unclosed) removed.
fn drop_boneyard(source: &str) -> String {
    let mut out = String::new();
    let mut rest = source;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// `SECTION` (`^(#{1,6})(?:\s+(.*))?$`): the level and the text.
fn section(text: &str) -> Option<(usize, String)> {
    let hashes = text.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &text[hashes..];
    if rest.is_empty() {
        return Some((hashes, String::new()));
    }
    if !rest.starts_with(is_js_whitespace)
        || !no_line_terminator(rest.trim_start_matches(is_js_whitespace))
    {
        return None;
    }
    Some((hashes, trim(rest).to_owned()))
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `parseFountain`: the title page and the elements.
pub fn parse_fountain(source: &str) -> (TitlePage, Vec<ScreenplayElement>) {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<String> = drop_boneyard(&normalized)
        .split('\n')
        .map(str::to_owned)
        .collect();
    let (title, body_start) = read_title_page(&lines);
    let mut elements = Vec::new();
    let mut index = body_start;
    let mut previous_blank = true;
    let mut pending: Option<String> = None;
    while pending.is_some() || index < lines.len() {
        let line = match pending.take() {
            Some(p) => p,
            None => {
                index += 1;
                lines[index - 1].clone()
            }
        };
        let trimmed = trim(&line);
        if trimmed.is_empty() {
            previous_blank = true;
            continue;
        }
        // A note that starts a line may continue over several lines.
        if trimmed.starts_with("[[") {
            let (text, rest, next) = read_note(&lines, index, &line);
            elements.push(ScreenplayElement::new("note", text));
            index = next;
            let rest = trim(&rest);
            if !rest.is_empty() {
                pending = Some(rest.to_owned());
            }
            continue;
        }
        let (inline, notes) = extract_notes(&line);
        let text = trim(&inline).to_owned();
        let note_elements = |notes: &[String]| {
            notes
                .iter()
                .map(|n| ScreenplayElement::new("note", n.clone()))
                .collect::<Vec<_>>()
        };

        if text.starts_with('.') && !text.starts_with("..") && utf16_len(&text) > 1 {
            let (heading, number) = read_scene_number(trim(&text[1..]));
            let mut element = ScreenplayElement::new("scene-heading", heading).forced();
            element.scene_number = number;
            elements.push(element);
            previous_blank = false;
            continue;
        }
        if is_scene_heading(&text) && previous_blank {
            let (heading, number) = read_scene_number(&text);
            let mut element = ScreenplayElement::new("scene-heading", heading);
            element.scene_number = number;
            elements.push(element);
            previous_blank = false;
            continue;
        }
        if previous_blank
            && index < lines.len()
            && !trim(&lines[index]).is_empty()
            && let Some((cue, dual, forced)) = read_character_cue(&text)
        {
            let mut character = ScreenplayElement::new("character", cue);
            character.dual = dual;
            character.forced = forced;
            elements.push(character);
            elements.extend(note_elements(&notes));
            while index < lines.len() {
                let raw = &lines[index];
                if trim(raw).is_empty() {
                    break;
                }
                index += 1;
                let (current, current_notes) = extract_notes(raw);
                let dialogue = trim(&current);
                if !dialogue.is_empty() {
                    let parenthetical = dialogue.starts_with('(')
                        && dialogue.ends_with(')')
                        && utf16_len(dialogue) >= 2
                        && no_line_terminator(dialogue);
                    elements.push(ScreenplayElement::new(
                        if parenthetical {
                            "parenthetical"
                        } else {
                            "dialogue"
                        },
                        dialogue,
                    ));
                }
                elements.extend(note_elements(&current_notes));
            }
            previous_blank = false;
            continue;
        }

        let length = utf16_len(&text);
        let element = if text.starts_with('>') && text.ends_with('<') && length > 2 {
            ScreenplayElement::new("centered", trim(&text[1..text.len() - 1]))
        } else if text.starts_with('>') && length > 1 {
            ScreenplayElement::new("transition", trim(&text[1..])).forced()
        } else if is_transition(&text) {
            ScreenplayElement::new("transition", text.clone())
        } else if length >= 3 && text.chars().all(|c| c == '=') {
            ScreenplayElement::new("page-break", "")
        } else if let Some((level, title)) = section(&text) {
            let mut element = ScreenplayElement::new("section", title);
            element.level = Some(level);
            element
        } else if let Some(rest) = text.strip_prefix('=') {
            ScreenplayElement::new("synopsis", trim(rest))
        } else if text.starts_with('!') && length > 1 {
            ScreenplayElement::new("action", trim(&text[1..])).forced()
        } else {
            ScreenplayElement::new("action", text.clone())
        };
        let outline = element.kind == "section" || element.kind == "synopsis";
        elements.push(element);
        elements.extend(note_elements(&notes));
        // Sections and synopses belong to the outline: a scene heading after them needs no
        // blank line of its own.
        if !outline {
            previous_blank = false;
        }
    }
    (title, elements)
}

fn is_parenthetical(text: &str) -> bool {
    text.starts_with('(') && text.ends_with(')') && utf16_len(text) >= 2 && no_line_terminator(text)
}

fn serialize_element(element: &ScreenplayElement) -> String {
    let text = &element.text;
    match element.kind.as_str() {
        "scene-heading" => {
            let marker = if element.forced || !is_scene_heading(text) {
                "."
            } else {
                ""
            };
            let number = element
                .scene_number
                .as_ref()
                .map(|n| format!(" #{n}#"))
                .unwrap_or_default();
            format!("{marker}{text}{number}")
        }
        "action" => format!(
            "{}{text}",
            if element.forced || needs_action_marker(text) {
                "!"
            } else {
                ""
            }
        ),
        "character" => {
            let marker = if element.forced || !is_character_text(text) {
                "@"
            } else {
                ""
            };
            format!("{marker}{text}{}", if element.dual { " ^" } else { "" })
        }
        "dialogue" => text.clone(),
        "parenthetical" => {
            if is_parenthetical(text) {
                text.clone()
            } else {
                format!("({text})")
            }
        }
        "transition" => format!(
            "{}{text}",
            if element.forced || !is_transition(text) {
                ">"
            } else {
                ""
            }
        ),
        "centered" => format!("> {text} <"),
        "section" => format!(
            "{}{}",
            "#".repeat(element.level.unwrap_or(1)),
            if text.is_empty() {
                String::new()
            } else {
                format!(" {text}")
            }
        ),
        "synopsis" => trim_end(&format!("= {text}")).to_owned(),
        "note" => format!("[[{text}]]"),
        "page-break" => "===".into(),
        _ => text.clone(),
    }
}

/// `serializeFountain`.
pub fn serialize_fountain(title: &TitlePage, elements: &[ScreenplayElement]) -> String {
    let mut blocks: Vec<String> = Vec::new();
    let mut title_lines = Vec::new();
    for (key, value) in title {
        let mut parts = value.split('\n');
        title_lines
            .push(trim_end(&format!("{key}: {}", parts.next().unwrap_or_default())).to_owned());
        for line in parts {
            title_lines.push(trim_end(&format!("    {line}")).to_owned());
        }
    }
    if !title_lines.is_empty() {
        blocks.push(title_lines.join("\n"));
    }
    // A cue, its parentheticals and its dialogue form one block: a blank line would end it.
    let mut block: Vec<String> = Vec::new();
    for element in elements {
        let line = serialize_element(element);
        if line.is_empty() {
            continue;
        }
        if element.kind == "dialogue" || element.kind == "parenthetical" {
            block.push(line);
            continue;
        }
        if !block.is_empty() {
            blocks.push(std::mem::take(&mut block).join("\n"));
        }
        block.push(line);
    }
    if !block.is_empty() {
        blocks.push(block.join("\n"));
    }
    if blocks.is_empty() {
        String::new()
    } else {
        format!("{}\n", blocks.join("\n\n"))
    }
}
