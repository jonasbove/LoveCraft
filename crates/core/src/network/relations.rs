//! Relations between Wiki pages and where pages appear in the story (ProseCraft
//! `src/lib/relations.ts`, upstream `bcb884d`).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::js::{JsObject, JsString, JsValue};
use crate::mentions::Matcher;

use super::meta::{Meta, Relation};

/// A document as the network reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub id: String,
    pub title: String,
    pub space: Option<String>,
    pub chapter_id: Option<String>,
    pub folder_id: Option<String>,
    pub trashed: bool,
    pub content: String,
    /// `meta.kind`.
    pub kind: Option<String>,
    pub aliases: Vec<String>,
    /// `meta.characters`: pages the author confirmed for a scene.
    pub characters: Vec<String>,
    pub relations: Vec<Relation>,
}

fn text(value: &JsValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(JsValue::as_js_string)
        .map(JsString::to_string_lossy)
}

impl Doc {
    pub fn from_js(value: &JsValue) -> Self {
        let meta = Meta::of(value);
        Self {
            id: text(value, "id").unwrap_or_default(),
            title: text(value, "title").unwrap_or_default(),
            space: text(value, "space"),
            chapter_id: text(value, "chapterId").filter(|c| !c.is_empty()),
            folder_id: text(value, "folderId").filter(|c| !c.is_empty()),
            trashed: matches!(value.get("trashed"), Some(JsValue::Bool(true))),
            content: text(value, "content").unwrap_or_default(),
            kind: meta.kind(),
            aliases: meta.aliases(),
            characters: meta.characters(),
            relations: meta.relations(),
        }
    }

    pub fn is_wiki(&self) -> bool {
        self.space.as_deref() == Some("Wiki")
    }

    /// A scene: in a chapter, not a Wiki page, not trashed.
    pub fn is_scene(&self) -> bool {
        !self.is_wiki() && !self.trashed && self.chapter_id.is_some()
    }
}

/// A Wiki folder: its id and title.
#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    pub id: String,
    pub title: String,
}

/// What the network reads from a stored project.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Story {
    pub docs: Vec<Doc>,
    /// Chapter ids in project order.
    pub chapters: Vec<String>,
    pub folders: Vec<Folder>,
}

impl Story {
    pub fn from_project(project: &JsValue) -> Self {
        let list = |key: &str| {
            project
                .get(key)
                .and_then(JsValue::as_array)
                .unwrap_or_default()
        };
        Self {
            docs: list("documents").iter().map(Doc::from_js).collect(),
            chapters: list("chapters")
                .iter()
                .filter_map(|c| text(c, "id"))
                .collect(),
            folders: list("researchFolders")
                .iter()
                .filter_map(|f| {
                    Some(Folder {
                        id: text(f, "id")?,
                        title: text(f, "title").unwrap_or_default(),
                    })
                })
                .collect(),
        }
    }

    pub fn doc(&self, id: &str) -> Option<&Doc> {
        self.docs.iter().find(|d| d.id == id)
    }

    /// Live Wiki pages.
    pub fn pages(&self) -> impl Iterator<Item = &Doc> {
        self.docs.iter().filter(|d| d.is_wiki() && !d.trashed)
    }

    /// `pageKind`: `meta.kind`, else from the title of the page's folder.
    pub fn page_kind(&self, doc: &Doc) -> &'static str {
        if let Some(kind) = doc.kind.as_deref()
            && let Some(k) = super::meta::PAGE_KINDS.iter().find(|k| **k == kind)
        {
            return k;
        }
        let Some(folder) = doc
            .folder_id
            .as_deref()
            .and_then(|id| self.folders.iter().find(|f| f.id == id))
        else {
            return "other";
        };
        let title = folder
            .title
            .trim_matches(crate::html::is_js_whitespace)
            .to_lowercase();
        match title.as_str() {
            "characters" => "character",
            "places" => "place",
            "organisations" | "organizations" => "organisation",
            "items" => "item",
            _ => "other",
        }
    }

    /// `characterPages`.
    pub fn character_pages(&self) -> Vec<&Doc> {
        self.pages()
            .filter(|d| self.page_kind(d) == "character")
            .collect()
    }
}

/// A relation and the page that stores it.
#[derive(Clone, Debug, PartialEq)]
pub struct Sourced {
    pub source: String,
    pub relation: Relation,
}

/// `relationsOf`: every stored relation of the live documents.
pub fn relations_of(docs: &[Doc]) -> Vec<Sourced> {
    docs.iter()
        .filter(|d| !d.trashed)
        .flat_map(|d| {
            d.relations.iter().map(|r| Sourced {
                source: d.id.clone(),
                relation: r.clone(),
            })
        })
        .collect()
}

/// `reverseView`: the incoming direction of each relation, never stored.
pub fn reverse_view(relations: &[Sourced]) -> Vec<Sourced> {
    relations
        .iter()
        .map(|s| Sourced {
            source: s.relation.to.clone(),
            relation: Relation {
                id: format!("reverse:{}", s.relation.id),
                to: s.source.clone(),
                ..s.relation.clone()
            },
        })
        .collect()
}

/// `relationActiveAt`: whether a relation holds in a chapter (bounds inclusive; a bound that is
/// no longer a chapter is ignored).
pub fn relation_active_at(relation: &Relation, chapter_id: &str, chapters: &[String]) -> bool {
    let index = |id: &str| chapters.iter().position(|c| c == id);
    let Some(current) = index(chapter_id) else {
        return false;
    };
    if let Some(from) = relation.from.as_deref().and_then(index)
        && current < from
    {
        return false;
    }
    if let Some(until) = relation.until.as_deref().and_then(index)
        && current > until
    {
        return false;
    }
    true
}

/// `replaceRelations`: one page's relation list replaced in the stored documents; `meta` goes
/// when nothing is left in it.
fn replace_relations(
    documents: &mut [JsValue],
    source: &str,
    replace: impl FnOnce(Vec<JsValue>) -> Vec<JsValue>,
) {
    let Some(doc) = documents.iter_mut().find(|d| {
        d.get("id")
            .and_then(JsValue::as_js_string)
            .is_some_and(|i| i.eq_str(source))
    }) else {
        return;
    };
    let Some(object) = doc.as_object_mut() else {
        return;
    };
    let mut meta = object
        .get("meta")
        .and_then(JsValue::as_object)
        .cloned()
        .unwrap_or_default();
    let current = meta
        .get("relations")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default();
    let next = replace(current);
    if next.is_empty() {
        meta.remove("relations");
    } else {
        meta.insert("relations", JsValue::Array(next));
    }
    if meta.is_empty() {
        object.remove("meta");
    } else {
        object.insert("meta", JsValue::Object(meta));
    }
}

/// `addRelation`: `{ ...relation, id }`, so the id goes last.
pub fn add_relation(documents: &mut [JsValue], source: &str, relation: Relation) {
    let mut entry = relation.to_js();
    if let Some(o) = entry.as_object_mut()
        && let Some(id) = o.remove("id")
    {
        o.insert("id", id);
    }
    replace_relations(documents, source, |mut list| {
        list.push(entry);
        list
    });
}

/// `updateRelation`: the relation replaced by `patch(relation)`, keeping its id.
pub fn update_relation(
    documents: &mut [JsValue],
    source: &str,
    relation_id: &str,
    patch: impl Fn(&mut JsObject),
) {
    replace_relations(documents, source, |list| {
        list.into_iter()
            .map(|r| {
                let matches = r
                    .get("id")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|i| i.eq_str(relation_id));
                match (matches, r) {
                    (true, JsValue::Object(mut o)) => {
                        let id = o.get("id").cloned();
                        patch(&mut o);
                        if let Some(id) = id {
                            o.insert("id", id);
                        }
                        JsValue::Object(o)
                    }
                    (_, r) => r,
                }
            })
            .collect()
    });
}

/// `removeRelation`.
pub fn remove_relation(documents: &mut [JsValue], source: &str, relation_id: &str) {
    replace_relations(documents, source, |list| {
        list.into_iter()
            .filter(|r| {
                !r.get("id")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|i| i.eq_str(relation_id))
            })
            .collect()
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Confirmed,
    Auto,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneAppearance {
    pub scene_id: String,
    pub mode: Mode,
}

/// `PageAppearances`: one page and the scenes it appears in, in story order.
#[derive(Clone, Debug, PartialEq)]
pub struct Appearances {
    pub page_id: String,
    pub kind: &'static str,
    pub scenes: Vec<SceneAppearance>,
    pub confirmed: usize,
    pub auto: usize,
    pub first_chapter: Option<(String, usize)>,
    pub last_chapter: Option<(String, usize)>,
}

impl Appearances {
    pub fn scene_count(&self) -> usize {
        self.scenes.len()
    }
}

/// `appearances`: for each live Wiki page (in document order), the scenes it is confirmed in
/// (`meta.characters`) or mentioned in by title or alias. The matching runs over the scene's
/// stored HTML, as ProseCraft's does.
pub fn appearances(story: &Story) -> Vec<Appearances> {
    let pages: Vec<&Doc> = story.pages().collect();
    let mut result: Vec<Appearances> = pages
        .iter()
        .map(|p| Appearances {
            page_id: p.id.clone(),
            kind: story.page_kind(p),
            scenes: Vec::new(),
            confirmed: 0,
            auto: 0,
            first_chapter: None,
            last_chapter: None,
        })
        .collect();
    let index_of: HashMap<&str, usize> = pages
        .iter()
        .enumerate()
        .rev()
        .map(|(i, p)| (p.id.as_str(), i))
        .collect();
    let mut articles: Vec<(&str, &str)> = Vec::new();
    for page in &pages {
        articles.push((page.title.as_str(), page.id.as_str()));
        for alias in &page.aliases {
            articles.push((alias.as_str(), page.id.as_str()));
        }
    }
    let matcher = Matcher::new(articles.iter().map(|(t, _)| *t));

    let mut scenes_by_chapter: HashMap<&str, Vec<&Doc>> = HashMap::new();
    for doc in &story.docs {
        if doc.trashed || doc.is_wiki() {
            continue;
        }
        if let Some(chapter) = doc.chapter_id.as_deref() {
            scenes_by_chapter.entry(chapter).or_default().push(doc);
        }
    }

    for (chapter_index, chapter) in story.chapters.iter().enumerate() {
        for scene in scenes_by_chapter
            .get(chapter.as_str())
            .into_iter()
            .flatten()
        {
            let confirmed: HashSet<&str> = scene.characters.iter().map(String::as_str).collect();
            // A Set keeps insertion order: the confirmed ids, then the mentions.
            let mut appearing: Vec<&str> = Vec::new();
            for id in &scene.characters {
                if !appearing.contains(&id.as_str()) {
                    appearing.push(id);
                }
            }
            let chars: Vec<char> = scene.content.chars().collect();
            for (_, article) in matcher.find(&chars) {
                let id = articles[article].1;
                if !appearing.contains(&id) {
                    appearing.push(id);
                }
            }
            for id in appearing {
                let Some(&slot) = index_of.get(id) else {
                    continue;
                };
                let entry = &mut result[slot];
                let mode = if confirmed.contains(id) {
                    entry.confirmed += 1;
                    Mode::Confirmed
                } else {
                    entry.auto += 1;
                    Mode::Auto
                };
                entry.scenes.push(SceneAppearance {
                    scene_id: scene.id.clone(),
                    mode,
                });
                if entry.first_chapter.is_none() {
                    entry.first_chapter = Some((chapter.clone(), chapter_index));
                }
                entry.last_chapter = Some((chapter.clone(), chapter_index));
            }
        }
    }
    // Duplicate page ids share the first entry (a Map keyed by id).
    let mut seen = HashSet::new();
    result.retain(|a| seen.insert(a.page_id.clone()));
    result
}

/// Two pages and the number of scenes they share; `key` is `a|b` with `a <= b`.
#[derive(Clone, Debug, PartialEq)]
pub struct PagePair {
    pub a: String,
    pub b: String,
    pub key: String,
    pub count: usize,
}

fn utf16_le(a: &str, b: &str) -> bool {
    a.encode_utf16().cmp(b.encode_utf16()) != std::cmp::Ordering::Greater
}

pub(crate) fn page_pair(a: &str, b: &str) -> (String, String, String) {
    if utf16_le(a, b) {
        (a.into(), b.into(), format!("{a}|{b}"))
    } else {
        (b.into(), a.into(), format!("{b}|{a}"))
    }
}

/// `coAppearanceCounts`: shared scenes per pair, most first, then by key.
pub fn co_appearance_counts(appearances: &[Appearances]) -> Vec<PagePair> {
    // Map insertion order: scenes in the order they are first seen.
    let mut scene_order: Vec<String> = Vec::new();
    let mut pages_by_scene: HashMap<String, Vec<String>> = HashMap::new();
    for entry in appearances {
        for scene in &entry.scenes {
            let pages = pages_by_scene
                .entry(scene.scene_id.clone())
                .or_insert_with(|| {
                    scene_order.push(scene.scene_id.clone());
                    Vec::new()
                });
            if !pages.contains(&entry.page_id) {
                pages.push(entry.page_id.clone());
            }
        }
    }
    let mut order: Vec<String> = Vec::new();
    let mut counts: BTreeMap<String, PagePair> = BTreeMap::new();
    for scene in &scene_order {
        let ids = &pages_by_scene[scene];
        for first in 0..ids.len() {
            for second in first + 1..ids.len() {
                let (a, b, key) = page_pair(&ids[first], &ids[second]);
                counts
                    .entry(key.clone())
                    .and_modify(|p| p.count += 1)
                    .or_insert_with(|| {
                        order.push(key.clone());
                        PagePair {
                            a,
                            b,
                            key,
                            count: 1,
                        }
                    });
            }
        }
    }
    let mut pairs: Vec<PagePair> = order.iter().filter_map(|k| counts.remove(k)).collect();
    pairs.sort_by(|l, r| {
        r.count
            .cmp(&l.count)
            .then_with(|| l.key.encode_utf16().cmp(r.key.encode_utf16()))
    });
    pairs
}

/// `suggestRelations`: pairs sharing at least `min_scenes` scenes, not linked either way and
/// not dismissed.
pub fn suggest_relations(
    appearances: &[Appearances],
    relations: &[Sourced],
    dismissed: &[String],
    min_scenes: usize,
) -> Vec<PagePair> {
    let linked: HashSet<String> = relations
        .iter()
        .map(|s| page_pair(&s.source, &s.relation.to).2)
        .collect();
    co_appearance_counts(appearances)
        .into_iter()
        .filter(|p| {
            p.count >= min_scenes && !dismissed.contains(&p.key) && !linked.contains(&p.key)
        })
        .collect()
}

/// `relationBoundaryIndexes`: chapter indexes where the set of active relations changes.
pub fn relation_boundary_indexes(relations: &[Sourced], chapters: &[String]) -> Vec<usize> {
    if relations.is_empty() {
        return Vec::new();
    }
    let mut boundaries = Vec::new();
    let mut previous: Vec<Vec<u16>> = Vec::new();
    for (index, chapter) in chapters.iter().enumerate() {
        let mut signature: Vec<Vec<u16>> = relations
            .iter()
            .filter(|s| relation_active_at(&s.relation, chapter, chapters))
            .map(|s| s.relation.id.encode_utf16().collect())
            .collect();
        signature.sort();
        // `previous` starts as "", which equals an empty signature.
        if signature != previous {
            boundaries.push(index);
        }
        previous = signature;
    }
    boundaries
}
