//! The graph tile beside the text and a page's appearance strip (ProseCraft `graph-tile.ts`,
//! `appearance-strip.ts`, `story-data-visibility.ts`, upstream `bcb884d`).

use std::collections::HashSet;

use super::meta::Relation;
use super::relations::{
    Appearances, Doc, Mode, Sourced, Story, appearances, relation_active_at, relations_of,
    reverse_view,
};

#[derive(Clone, Debug, PartialEq)]
pub struct TileCharacter {
    pub id: String,
    pub label: String,
    pub kind: &'static str,
    pub mode: Mode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TilePage {
    pub id: String,
    pub title: String,
    pub kind: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TileModel {
    /// The scene the tile follows; `None` for a page tile.
    pub scene_id: Option<String>,
    pub chapter_id: Option<String>,
    /// Characters in the scene: confirmed first, then detected.
    pub characters: Vec<TileCharacter>,
    /// Detected characters the author hasn't confirmed.
    pub suggestions: Vec<TileCharacter>,
    pub relations: Vec<Sourced>,
    /// The characters, then the other ends of their relations.
    pub pages: Vec<TilePage>,
}

fn base_id(id: &str) -> &str {
    id.strip_prefix("reverse:").unwrap_or(id)
}

/// `graphTileModel`: for a page (`page_id`, which wins) or for a scene.
pub fn tile_model(story: &Story, scene_id: Option<&str>, page_id: Option<&str>) -> TileModel {
    let wiki: Vec<&Doc> = story.pages().collect();
    let page = |id: &str| wiki.iter().copied().find(|d| d.id == id);
    let entry = |doc: &Doc, mode: Mode| TileCharacter {
        id: doc.id.clone(),
        label: doc.title.clone(),
        kind: story.page_kind(doc),
        mode,
    };
    let relations_for = |ids: &HashSet<String>, chapter: Option<&str>| {
        let stored = relations_of(&story.docs);
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for s in stored.iter().cloned().chain(reverse_view(&stored)) {
            let base = base_id(&s.relation.id).to_owned();
            if seen.contains(&base) {
                continue;
            }
            if page(&s.source).is_none() || page(&s.relation.to).is_none() {
                continue;
            }
            if !ids.contains(&s.source) && !ids.contains(&s.relation.to) {
                continue;
            }
            if let Some(chapter) = chapter
                && !relation_active_at(&s.relation, chapter, &story.chapters)
            {
                continue;
            }
            seen.insert(base);
            out.push(s);
        }
        out
    };
    let pages_of = |characters: &[TileCharacter], relations: &[Sourced]| {
        let mut pages = Vec::new();
        let mut seen = HashSet::new();
        for c in characters {
            pages.push(TilePage {
                id: c.id.clone(),
                title: c.label.clone(),
                kind: c.kind,
            });
            seen.insert(c.id.clone());
        }
        for s in relations {
            for id in [&s.source, &s.relation.to] {
                if seen.contains(id) {
                    continue;
                }
                if let Some(doc) = page(id) {
                    seen.insert(id.clone());
                    pages.push(TilePage {
                        id: doc.id.clone(),
                        title: doc.title.clone(),
                        kind: story.page_kind(doc),
                    });
                }
            }
        }
        pages
    };

    if let Some(centre) = page_id.and_then(page) {
        let characters = vec![entry(centre, Mode::Confirmed)];
        let ids = HashSet::from([centre.id.clone()]);
        let relations: Vec<Sourced> = relations_for(&ids, None)
            .into_iter()
            .map(|s| {
                if s.source == centre.id {
                    s
                } else {
                    Sourced {
                        source: centre.id.clone(),
                        relation: Relation {
                            id: format!("reverse:{}", base_id(&s.relation.id)),
                            to: s.source.clone(),
                            ..s.relation
                        },
                    }
                }
            })
            .collect();
        let pages = pages_of(&characters, &relations);
        return TileModel {
            scene_id: None,
            chapter_id: None,
            characters,
            suggestions: Vec::new(),
            relations,
            pages,
        };
    }

    let Some(scene) = scene_id.and_then(|id| story.docs.iter().find(|d| d.id == id && !d.trashed))
    else {
        return TileModel::default();
    };
    if scene.is_wiki() {
        return TileModel::default();
    }
    let chapter = scene.chapter_id.clone();
    let mut confirmed = Vec::new();
    for id in &scene.characters {
        if let Some(doc) = page(id)
            && story.page_kind(doc) == "character"
        {
            confirmed.push(entry(doc, Mode::Confirmed));
        }
    }
    let confirmed_ids: HashSet<&str> = confirmed.iter().map(|c| c.id.as_str()).collect();
    let mut detected = Vec::new();
    for a in appearances(story) {
        if a.kind != "character" || confirmed_ids.contains(a.page_id.as_str()) {
            continue;
        }
        if let Some(doc) = page(&a.page_id)
            && a.scenes.iter().any(|s| s.scene_id == scene.id)
        {
            detected.push(entry(doc, Mode::Auto));
        }
    }
    let mut characters = confirmed;
    characters.extend(detected.iter().cloned());
    let ids: HashSet<String> = characters.iter().map(|c| c.id.clone()).collect();
    let relations = relations_for(&ids, chapter.as_deref());
    let pages = pages_of(&characters, &relations);
    TileModel {
        scene_id: Some(scene.id.clone()),
        chapter_id: chapter,
        characters,
        suggestions: detected,
        relations,
        pages,
    }
}

/// One chapter's column in a page's appearance strip (`AppearanceBar`).
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub chapter_id: String,
    pub total: usize,
    pub confirmed: usize,
    pub auto: usize,
    pub scene_titles: Vec<String>,
    pub first_scene_id: Option<String>,
    pub first: bool,
    pub last: bool,
}

/// `chapterBars`: one bar per chapter, in project order.
pub fn chapter_bars(entry: Option<&Appearances>, story: &Story) -> Vec<Bar> {
    let scenes: Vec<&Doc> = story.docs.iter().filter(|d| d.is_scene()).collect();
    // A Map keyed by id: a later duplicate replaces the value, keeping the first's place.
    let scene = |id: &str| scenes.iter().rev().find(|d| d.id == id).copied();
    let mut per: Vec<(String, usize, usize, Vec<String>)> = Vec::new();
    for a in entry.map(|e| e.scenes.as_slice()).unwrap_or_default() {
        let Some(s) = scene(&a.scene_id) else {
            continue;
        };
        let Some(chapter) = s.chapter_id.clone() else {
            continue;
        };
        let slot = match per.iter().position(|p| p.0 == chapter) {
            Some(i) => i,
            None => {
                per.push((chapter, 0, 0, Vec::new()));
                per.len() - 1
            }
        };
        match a.mode {
            Mode::Confirmed => per[slot].1 += 1,
            Mode::Auto => per[slot].2 += 1,
        }
        per[slot].3.push(s.title.clone());
    }
    let mut first_scene: Vec<(String, String)> = Vec::new();
    let mut ids_seen = HashSet::new();
    for s in &scenes {
        if !ids_seen.insert(s.id.as_str()) {
            continue;
        }
        let s = scene(&s.id).unwrap_or(s);
        if let Some(c) = &s.chapter_id
            && !first_scene.iter().any(|(fc, _)| fc == c)
        {
            first_scene.push((c.clone(), s.id.clone()));
        }
    }
    story
        .chapters
        .iter()
        .map(|chapter| {
            let counts = per.iter().find(|p| &p.0 == chapter);
            let confirmed = counts.map_or(0, |c| c.1);
            let auto = counts.map_or(0, |c| c.2);
            Bar {
                chapter_id: chapter.clone(),
                total: confirmed + auto,
                confirmed,
                auto,
                scene_titles: counts.map(|c| c.3.clone()).unwrap_or_default(),
                first_scene_id: first_scene
                    .iter()
                    .find(|(c, _)| c == chapter)
                    .map(|(_, s)| s.clone()),
                first: entry
                    .and_then(|e| e.first_chapter.as_ref())
                    .is_some_and(|(c, _)| c == chapter),
                last: entry
                    .and_then(|e| e.last_chapter.as_ref())
                    .is_some_and(|(c, _)| c == chapter),
            }
        })
        .collect()
}

/// `shouldShowStoryData`: the story data panel belongs to live scenes.
pub fn shows_story_data(doc: &Doc) -> bool {
    doc.is_scene()
}
