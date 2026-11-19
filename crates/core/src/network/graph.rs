//! The relation network's view model (ProseCraft `relation-graph-model.ts`, `graph-connect.ts`
//! and `suggestion-accept.ts`, upstream `bcb884d`).

use std::collections::{HashMap, HashSet};

use super::meta::Relation;
use super::relations::{Appearances, PagePair, Sourced, relation_active_at};

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub kind: &'static str,
    pub initials: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Declared,
    Suggested,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub label: String,
    pub tension: Option<f64>,
    pub status: Status,
    /// Shared scenes behind a suggestion.
    pub count: Option<usize>,
    /// Offset among the edges of the same pair, centred on 0.
    pub lane: f64,
    pub lane_count: usize,
}

/// A Wiki page as the graph sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub kind: &'static str,
}

pub struct Input<'a> {
    pub pages: &'a [Page],
    pub relations: &'a [Sourced],
    pub appearances: &'a [Appearances],
    pub suggestions: &'a [PagePair],
    pub chapters: &'a [String],
    /// The chapter to focus, or the whole story.
    pub chapter_id: Option<&'a str>,
    /// Scene id → chapter id; without it a chapter filter uses the first and last appearance.
    pub scene_chapters: Option<&'a HashMap<String, String>>,
    pub kinds: Option<&'a [&'a str]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

fn js_upper(s: &str) -> String {
    s.to_uppercase()
}

/// The first `n` UTF-16 units of `s` (a lone surrogate half dropped).
fn units_prefix(s: &str, n: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().take(n).collect();
    String::from_utf16_lossy(&units)
}

/// `initialsOf`: first and last word's first letters, or a single word's first two.
pub fn initials_of(label: &str) -> String {
    let words: Vec<&str> = label
        .split(crate::html::is_js_whitespace)
        .filter(|w| !w.is_empty())
        .collect();
    match words.as_slice() {
        [] => "?".into(),
        [one] => js_upper(&units_prefix(one, 2)),
        [first, .., last] => js_upper(&(units_prefix(first, 1) + &units_prefix(last, 1))),
    }
}

fn pair_key(a: &str, b: &str) -> (String, String) {
    if a.encode_utf16().lt(b.encode_utf16()) {
        (a.into(), b.into())
    } else {
        (b.into(), a.into())
    }
}

fn appears_in_chapter(
    appearance: Option<&Appearances>,
    chapter_id: &str,
    chapter_index: Option<usize>,
    scene_chapters: Option<&HashMap<String, String>>,
) -> bool {
    let Some(appearance) = appearance else {
        return false;
    };
    if let Some(map) = scene_chapters {
        return appearance
            .scenes
            .iter()
            .any(|s| map.get(&s.scene_id).is_some_and(|c| c == chapter_id));
    }
    // `chapterIndex` is -1 for an unknown chapter, which no range holds.
    let (Some(first), Some(last), Some(index)) = (
        appearance.first_chapter.as_ref().map(|c| c.1),
        appearance.last_chapter.as_ref().map(|c| c.1),
        chapter_index,
    ) else {
        return false;
    };
    first <= index && index <= last
}

fn shared_scenes_in_chapter(
    a: &str,
    b: &str,
    chapter_id: &str,
    appearances: &[Appearances],
    scene_chapters: &HashMap<String, String>,
) -> usize {
    let of = |id: &str| appearances.iter().find(|x| x.page_id == id);
    let other: HashSet<&str> = of(b)
        .map(|x| x.scenes.iter().map(|s| s.scene_id.as_str()).collect())
        .unwrap_or_default();
    of(a).map_or(0, |x| {
        x.scenes
            .iter()
            .filter(|s| {
                other.contains(s.scene_id.as_str())
                    && scene_chapters
                        .get(&s.scene_id)
                        .is_some_and(|c| c == chapter_id)
            })
            .count()
    })
}

/// `buildRelationGraph`.
pub fn build(input: &Input) -> Model {
    let chapter_index = input
        .chapter_id
        .and_then(|id| input.chapters.iter().position(|c| c == id));
    let pages: Vec<&Page> = input
        .pages
        .iter()
        .filter(|p| input.kinds.is_none_or(|k| k.contains(&p.kind)))
        .collect();
    let visible: HashSet<&str> = pages
        .iter()
        .filter(|p| match input.chapter_id {
            None => true,
            Some(chapter) => appears_in_chapter(
                input.appearances.iter().find(|a| a.page_id == p.id),
                chapter,
                chapter_index,
                input.scene_chapters,
            ),
        })
        .map(|p| p.id.as_str())
        .collect();
    let nodes: Vec<Node> = pages
        .iter()
        .filter(|p| visible.contains(p.id.as_str()))
        .map(|p| Node {
            id: p.id.clone(),
            label: p.title.clone(),
            kind: p.kind,
            initials: initials_of(&p.title),
        })
        .collect();

    let mut edges: Vec<Edge> = Vec::new();
    let mut edge_ids: HashSet<String> = HashSet::new();
    let mut declared: HashSet<(String, String)> = HashSet::new();
    for Sourced { source, relation } in input.relations {
        if !visible.contains(source.as_str())
            || !visible.contains(relation.to.as_str())
            || *source == relation.to
        {
            continue;
        }
        declared.insert(pair_key(source, &relation.to));
        if let Some(chapter) = input.chapter_id
            && !relation_active_at(relation, chapter, input.chapters)
        {
            continue;
        }
        let id = format!("declared:{source}:{}", relation.id);
        if !edge_ids.insert(id.clone()) {
            continue;
        }
        let label = relation
            .label
            .as_deref()
            .map(|l| l.trim_matches(crate::html::is_js_whitespace))
            .filter(|l| !l.is_empty())
            .unwrap_or(&relation.kind);
        edges.push(Edge {
            id,
            from: source.clone(),
            to: relation.to.clone(),
            kind: relation.kind.clone(),
            label: label.into(),
            tension: relation.tension,
            status: Status::Declared,
            count: None,
            lane: 0.0,
            lane_count: 1,
        });
    }

    let mut suggestions: Vec<&PagePair> = input
        .suggestions
        .iter()
        .filter(|p| {
            p.a != p.b
                && !declared.contains(&pair_key(&p.a, &p.b))
                && visible.contains(p.a.as_str())
                && visible.contains(p.b.as_str())
        })
        .collect();
    // `localeCompare` on keys of ids; ids are ASCII in practice, where it orders as below.
    suggestions.sort_by(|l, r| {
        r.count
            .cmp(&l.count)
            .then_with(|| crate::network::locale_compare(&l.key, &r.key))
    });
    for pair in suggestions {
        let mut count = pair.count;
        if let (Some(chapter), Some(map)) = (input.chapter_id, input.scene_chapters) {
            count = shared_scenes_in_chapter(&pair.a, &pair.b, chapter, input.appearances, map);
            if count == 0 {
                continue;
            }
        }
        let id = format!("suggested:{}", pair.key);
        if !edge_ids.insert(id.clone()) {
            continue;
        }
        edges.push(Edge {
            id,
            from: pair.a.clone(),
            to: pair.b.clone(),
            kind: "other".into(),
            label: String::new(),
            tension: None,
            status: Status::Suggested,
            count: Some(count),
            lane: 0.0,
            lane_count: 1,
        });
    }

    let mut totals: HashMap<(String, String), usize> = HashMap::new();
    for e in &edges {
        *totals.entry(pair_key(&e.from, &e.to)).or_default() += 1;
    }
    let mut seen: HashMap<(String, String), usize> = HashMap::new();
    for e in &mut edges {
        let key = pair_key(&e.from, &e.to);
        let total = totals.get(&key).copied().unwrap_or(1);
        let index = seen.entry(key).or_default();
        e.lane = *index as f64 - (total as f64 - 1.0) / 2.0;
        e.lane_count = total;
        *index += 1;
    }
    Model { nodes, edges }
}

/// `neighbourIds`.
pub fn neighbour_ids(edges: &[Edge], node: &str) -> HashSet<String> {
    edges
        .iter()
        .filter_map(|e| {
            if e.from == node {
                Some(e.to.clone())
            } else if e.to == node {
                Some(e.from.clone())
            } else {
                None
            }
        })
        .collect()
}

/// `incidentEdgeIds`.
pub fn incident_edge_ids(edges: &[Edge], node: &str) -> HashSet<String> {
    edges
        .iter()
        .filter(|e| e.from == node || e.to == node)
        .map(|e| e.id.clone())
        .collect()
}

/// `matchingNodeIds`: labels containing the query, ignoring case; an empty query matches
/// nothing.
pub fn matching_node_ids(nodes: &[Node], query: &str) -> Vec<String> {
    let needle = query
        .trim_matches(crate::html::is_js_whitespace)
        .to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    nodes
        .iter()
        .filter(|n| n.label.to_lowercase().contains(&needle))
        .map(|n| n.id.clone())
        .collect()
}

/// A point in world coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// `graphBounds`: x, y, width, height; zero for no points.
pub fn graph_bounds(points: impl IntoIterator<Item = Point>) -> (f64, f64, f64, f64) {
    let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in points {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    if min_x == f64::INFINITY {
        return (0.0, 0.0, 0.0, 0.0);
    }
    (min_x, min_y, max_x - min_x, max_y - min_y)
}

/// `nodeAtPoint`: the nearest centre within `radius`.
pub fn node_at_point<'a>(
    nodes: impl IntoIterator<Item = (&'a str, Point)>,
    point: Point,
    radius: f64,
) -> Option<&'a str> {
    let mut found = None;
    let mut nearest = radius;
    for (id, at) in nodes {
        let distance = (at.x - point.x).hypot(at.y - point.y);
        if distance > nearest {
            continue;
        }
        found = Some(id);
        nearest = distance;
    }
    found
}

/// `canConnect`: two different pages, not already linked in this direction.
pub fn can_connect(relations: &[(String, String)], from: &str, to: &str) -> bool {
    from != to && !relations.iter().any(|(f, t)| f == from && t == to)
}

/// `connectionTarget`: the page a dragged connection snaps to (never its source) and whether
/// it may connect.
pub fn connection_target<'a>(
    nodes: impl IntoIterator<Item = (&'a str, Point)>,
    relations: &[(String, String)],
    from: &str,
    point: Point,
    radius: f64,
) -> Option<(&'a str, bool)> {
    let id = node_at_point(
        nodes.into_iter().filter(|(id, _)| *id != from),
        point,
        radius,
    )?;
    Some((id, can_connect(relations, from, id)))
}

/// `relationFromSuggestion`: which page stores the relation (`a_to_b` keeps `a` as the source)
/// and the relation, without an id; a blank label is left out.
pub fn relation_from_suggestion(
    pair: &PagePair,
    kind: &str,
    label: &str,
    a_to_b: bool,
) -> (String, Relation) {
    let (source, target) = if a_to_b {
        (&pair.a, &pair.b)
    } else {
        (&pair.b, &pair.a)
    };
    let label = label.trim_matches(crate::html::is_js_whitespace);
    (
        source.clone(),
        Relation {
            id: String::new(),
            to: target.clone(),
            kind: kind.into(),
            label: (!label.is_empty()).then(|| label.into()),
            tension: None,
            from: None,
            until: None,
            note: None,
        },
    )
}
