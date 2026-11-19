//! The Wiki migration (ProseCraft's `src/lib/wiki-migration.ts` and `publish/sheets.ts`): the
//! four former Story Spaces (Characters, Places, Research, Notes) folded into one Wiki, Notes
//! folders merged into the Wiki's, character and place profiles turned into readable page
//! content, and storyboard cards pointed at the Wiki.

use crate::js::{JsObject, JsValue};

const LEGACY_SPACES: [&str; 4] = ["Characters", "Places", "Research", "Notes"];
/// `SHEET_FIELD_LIMIT`, in UTF-16 units.
const SHEET_FIELD_LIMIT: usize = 8000;

/// `CHARACTER_SECTIONS` and `PLACE_SECTIONS`: each field's key and label, in their order.
const CHARACTER_FIELDS: [(&str, &str); 14] = [
    ("role", "Role"),
    ("age", "Age"),
    ("gender", "Gender"),
    ("pronouns", "Pronouns"),
    ("ethnicity", "Ethnicity / heritage"),
    ("occupation", "Occupation"),
    ("appearance", "Appearance"),
    ("voice", "Voice & mannerisms"),
    ("personality", "Personality"),
    ("background", "Background"),
    ("motivations", "Motivations & goals"),
    ("flaws", "Flaws & fears"),
    ("hobbies", "Hobbies & interests"),
    ("relationships", "Relationships"),
];
const PLACE_FIELDS: [(&str, &str); 14] = [
    ("type", "Type"),
    ("region", "Region"),
    ("climate", "Climate & weather"),
    ("population", "Population"),
    ("government", "Government"),
    ("economy", "Economy"),
    ("appearance", "Appearance"),
    ("atmosphere", "Atmosphere"),
    ("landmarks", "Landmarks"),
    ("history", "History"),
    ("culture", "Culture & customs"),
    ("inhabitants", "Notable inhabitants"),
    ("significance", "Role in the story"),
    ("dangers", "Dangers & secrets"),
];

fn text(v: &JsValue, key: &str) -> Option<String> {
    v.get(key)
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
}

/// `value.slice(0, n)` in UTF-16 units.
fn slice_utf16(value: &str, n: usize) -> String {
    let units: Vec<u16> = value.encode_utf16().take(n).collect();
    String::from_utf16_lossy(&units)
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `sheetToHtml` (with `normalizeCharacterSheet` / `normalizePlaceSheet`): the filled fields as
/// paragraphs under a heading; empty when nothing is filled.
fn sheet_to_html(fields: &[(&str, &str)], values: &JsValue, heading: &str) -> String {
    let mut lines = Vec::new();
    for (key, label) in fields {
        let value = values
            .get(key)
            .and_then(JsValue::as_js_string)
            .map(|s| slice_utf16(&s.to_string_lossy(), SHEET_FIELD_LIMIT))
            .unwrap_or_default();
        let value = value.trim_matches(crate::html::is_js_whitespace);
        if value.is_empty() {
            continue;
        }
        let escaped = escape_html(value)
            .replace("\r\n", "<br>")
            .replace('\n', "<br>");
        lines.push(format!(
            "<p><strong>{}:</strong> {escaped}</p>",
            escape_html(label)
        ));
    }
    if lines.is_empty() {
        return String::new();
    }
    format!("<h2>{}</h2>\n{}", escape_html(heading), lines.join("\n"))
}

/// `hasMeaningfulContent`: text, or an image, table, rule or rich block.
fn has_meaningful_content(html: &str) -> bool {
    let mut stripped = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                stripped.push(' ');
            }
            _ if !in_tag => stripped.push(c),
            _ => {}
        }
    }
    let text = stripped.replace("&nbsp;", " ").replace("&NBSP;", " ");
    if !text.trim_matches(crate::html::is_js_whitespace).is_empty() {
        return true;
    }
    let lower = html.to_lowercase();
    ["<img", "<table", "<hr", "<math", "<mermaid"]
        .iter()
        .any(|tag| {
            lower.match_indices(tag).any(|(at, _)| {
                lower[at + tag.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_')
            })
        })
}

/// `mergeProfileContent`: the profiles above the page's own text, a rule between.
fn merge_profile_content(document: &JsValue) -> Option<String> {
    let mut additions = Vec::new();
    if let Some(character) = document.get("character") {
        let profile = sheet_to_html(&CHARACTER_FIELDS, character, "Character profile");
        if !profile.is_empty() {
            additions.push(profile);
        }
    }
    if let Some(place) = document.get("place") {
        let profile = sheet_to_html(&PLACE_FIELDS, place, "Place profile");
        if !profile.is_empty() {
            additions.push(profile);
        }
    }
    let content = text(document, "content");
    if additions.is_empty() {
        return content;
    }
    let existing = content.unwrap_or_default();
    let existing = existing.trim_matches(crate::html::is_js_whitespace);
    if existing.is_empty() || !has_meaningful_content(existing) {
        return Some(additions.join("\n<hr>\n"));
    }
    additions.push(existing.to_owned());
    Some(additions.join("\n<hr>\n"))
}

fn needs_migration(document: &JsValue) -> bool {
    let legacy_space =
        matches!(document.get("space"), Some(JsValue::String(s)) if s.to_string_lossy() != "Wiki");
    legacy_space || document.get("character").is_some() || document.get("place").is_some()
}

/// `migrateDocument`.
fn migrate_document(document: &JsValue, note_folders: &[(String, String)]) -> JsValue {
    if !needs_migration(document) {
        return document.clone();
    }
    let original = text(document, "space");
    let Some(mut next) = document.as_object().cloned() else {
        return document.clone();
    };
    next.insert("space", JsValue::str("Wiki"));
    // An undefined content is left out when written.
    match merge_profile_content(document) {
        Some(content) => next.insert("content", JsValue::str(&content)),
        None => {
            next.remove("content");
        }
    }
    let folder = text(document, "folderId")
        .filter(|f| !f.is_empty())
        .map(|f| {
            if original.as_deref() == Some("Notes") {
                note_folders
                    .iter()
                    .find(|(old, _)| *old == f)
                    .map_or(f.clone(), |(_, new)| new.clone())
            } else {
                f
            }
        });
    match folder {
        Some(folder) => next.insert("folderId", JsValue::str(&folder)),
        None => {
            next.remove("folderId");
        }
    }
    next.remove("character");
    next.remove("place");
    JsValue::Object(next)
}

/// `normalizeFolders`: well-formed folders, titled, their parents real and without loops.
fn normalize_folders(value: Option<&JsValue>) -> Vec<(String, String, Option<String>)> {
    let mut folders: Vec<(String, String, Option<String>)> = Vec::new();
    for entry in value.and_then(JsValue::as_array).unwrap_or_default() {
        let Some(id) = text(entry, "id").filter(|i| !i.is_empty()) else {
            continue;
        };
        let Some(title) = text(entry, "title") else {
            continue;
        };
        let parent = match entry.get("parentId") {
            Some(JsValue::Null) => None,
            Some(JsValue::String(s)) => Some(s.to_string_lossy()),
            _ => continue,
        };
        if folders.iter().any(|(seen, ..)| *seen == id) {
            continue;
        }
        let title = slice_utf16(title.trim_matches(crate::html::is_js_whitespace), 60);
        let title = if title.is_empty() {
            "Untitled folder".to_owned()
        } else {
            title
        };
        folders.push((id, title, parent));
    }
    let snapshot = folders.clone();
    let parent_of = |id: &str| {
        snapshot
            .iter()
            .find(|(i, ..)| i == id)
            .and_then(|(_, _, p)| p.clone())
    };
    for folder in &mut folders {
        let fine = folder
            .2
            .as_ref()
            .is_some_and(|p| *p != folder.0 && snapshot.iter().any(|(i, ..)| i == p));
        if !fine {
            folder.2 = None;
            continue;
        }
        // A loop: back to the top level.
        let mut path = vec![folder.0.clone()];
        let mut current = folder.2.clone();
        while let Some(id) = current {
            if path.contains(&id) {
                folder.2 = None;
                break;
            }
            path.push(id.clone());
            current = parent_of(&id);
        }
    }
    folders
}

fn folder_js(id: &str, title: &str, parent: Option<&str>) -> JsValue {
    let mut o = JsObject::new();
    o.insert("id", JsValue::str(id));
    o.insert("title", JsValue::str(title));
    o.insert("parentId", parent.map_or(JsValue::Null, JsValue::str));
    JsValue::Object(o)
}

/// `migrateProjectToWiki`: the project (its `project` data) with the legacy spaces folded into
/// the Wiki; `None` when there is nothing to migrate (ProseCraft returns the same object).
pub fn migrate_project_to_wiki(project: &JsValue) -> Option<JsValue> {
    let research = normalize_folders(project.get("researchFolders"));
    let notes = normalize_folders(project.get("noteFolders"));
    let documents = project
        .get("documents")
        .and_then(JsValue::as_array)
        .unwrap_or_default();
    let cards = project.get("storyboardCards").and_then(JsValue::as_array);
    let legacy_card =
        |card: &JsValue| text(card, "kind").is_some_and(|k| LEGACY_SPACES.contains(&k.as_str()));
    let legacy_document = documents.iter().any(needs_migration);
    let legacy_cards = cards.unwrap_or_default().iter().any(legacy_card);
    if !legacy_document && !legacy_cards && notes.is_empty() {
        return None;
    }
    let mut used: Vec<String> = research.iter().map(|(id, ..)| id.clone()).collect();
    let mut note_ids: Vec<(String, String)> = Vec::new();
    for (id, ..) in &notes {
        let mut new = id.clone();
        if used.contains(&new) {
            let mut suffix = 1;
            while used.contains(&format!("{id}-note-{suffix}")) {
                suffix += 1;
            }
            new = format!("{id}-note-{suffix}");
        }
        used.push(new.clone());
        note_ids.push((id.clone(), new));
    }
    let renamed = |id: &str| {
        note_ids
            .iter()
            .find(|(old, _)| old == id)
            .map_or(id.to_owned(), |(_, new)| new.clone())
    };
    let mut folders: Vec<JsValue> = research
        .iter()
        .map(|(id, title, parent)| folder_js(id, title, parent.as_deref()))
        .collect();
    folders.extend(notes.iter().map(|(id, title, parent)| {
        folder_js(
            &renamed(id),
            title,
            parent.as_deref().map(&renamed).as_deref(),
        )
    }));
    let mut migrated = project.as_object()?.clone();
    migrated.insert(
        "documents",
        JsValue::Array(
            documents
                .iter()
                .map(|d| migrate_document(d, &note_ids))
                .collect(),
        ),
    );
    migrated.insert("researchFolders", JsValue::Array(folders));
    if let Some(cards) = cards {
        let cards = cards
            .iter()
            .map(|card| {
                let mut card = card.clone();
                if legacy_card(&card)
                    && let Some(o) = card.as_object_mut()
                {
                    o.insert("kind", JsValue::str("Wiki"));
                }
                card
            })
            .collect();
        migrated.insert("storyboardCards", JsValue::Array(cards));
    }
    migrated.remove("noteFolders");
    Some(JsValue::Object(migrated))
}

#[cfg(test)]
mod tests {
    use super::migrate_project_to_wiki;
    use crate::js::{JsValue, parse};

    #[test]
    fn legacy_spaces_profiles_folders_and_cards_fold_into_the_wiki() {
        let project = parse(
            r#"{"id":"project-1","name":"Test Novel","documents":[
              {"id":"char-1","title":"Mara","content":"<p>Free-form notes.</p>","space":"Characters","character":{"role":"Protagonist","age":"32","gender":"","pronouns":"she/her","occupation":"Cartographer","appearance":"Tall"}},
              {"id":"note-1","title":"Old note","content":"<p><br></p>","space":"Notes","folderId":"folder-1"},
              {"id":"research-1","title":"Old research","content":"<p>Research text.</p>","space":"Research","folderId":"folder-1"}],
              "chapters":[],
              "researchFolders":[{"id":"folder-1","title":"Research folder","parentId":null}],
              "noteFolders":[{"id":"folder-1","title":"Notes folder","parentId":null}],
              "researchItems":[{"id":"link-1","kind":"link","folderId":"folder-1","title":"Reference","url":"https://example.com","addedAt":1}],
              "storyboardCards":[{"id":"card-1","sourceId":"char-1","rowId":"row-1","x":10,"y":20,"kind":"Characters","title":"Mara","note":""}]}"#,
        )
        .expect("json");
        let migrated = migrate_project_to_wiki(&project).expect("migrated");
        let docs = migrated
            .get("documents")
            .and_then(JsValue::as_array)
            .unwrap_or_default();
        let field = |id: &str, key: &str| {
            docs.iter()
                .find(|d| d.get("id") == Some(&JsValue::str(id)))
                .and_then(|d| d.get(key).cloned())
        };
        assert_eq!(field("char-1", "space"), Some(JsValue::str("Wiki")));
        assert_eq!(
            field("char-1", "content"),
            Some(JsValue::str(
                "<h2>Character profile</h2>\n<p><strong>Role:</strong> Protagonist</p>\n<p><strong>Age:</strong> 32</p>\n<p><strong>Pronouns:</strong> she/her</p>\n<p><strong>Occupation:</strong> Cartographer</p>\n<p><strong>Appearance:</strong> Tall</p>\n<hr>\n<p>Free-form notes.</p>"
            ))
        );
        assert_eq!(field("char-1", "character"), None);
        assert_eq!(
            field("note-1", "folderId"),
            Some(JsValue::str("folder-1-note-1"))
        );
        assert_eq!(
            field("research-1", "folderId"),
            Some(JsValue::str("folder-1"))
        );
        assert!(migrated.get("noteFolders").is_none());
        let folder_ids: Vec<JsValue> = migrated
            .get("researchFolders")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|f| f.get("id").cloned())
            .collect();
        assert_eq!(
            folder_ids,
            [JsValue::str("folder-1"), JsValue::str("folder-1-note-1")]
        );
        let card_kind = migrated
            .get("storyboardCards")
            .and_then(JsValue::as_array)
            .and_then(|c| c.first().and_then(|c| c.get("kind").cloned()));
        assert_eq!(card_kind, Some(JsValue::str("Wiki")));
        // Once migrated, nothing is left to do.
        assert!(migrate_project_to_wiki(&migrated).is_none());
    }

    #[test]
    fn a_project_without_legacy_spaces_is_left_alone() {
        let project = parse(r#"{"documents":[{"id":"page-1","title":"Page","content":"<p>Text</p>","space":"Wiki"}]}"#).expect("json");
        assert!(migrate_project_to_wiki(&project).is_none());
    }
}
