//! The relation network against ProseCraft (`fixtures/network.json`, from upstream `bcb884d`'s
//! `story-meta.ts`, `story-yaml.ts`, `relations.ts`, `relation-graph-model.ts`,
//! `graph-layout.ts`, `graph-connect.ts`, `graph-tile.ts`, `appearance-strip.ts`,
//! `network-timeline.ts`, `suggestion-accept.ts` and `sheet-templates.ts`), and the document
//! `meta`, `sheetTemplates` and `dismissedSuggestions` through the open sync folder.

use std::collections::HashMap;

use prosecraft_core::js::{self, JsObject, JsString, JsValue};
use prosecraft_core::network::graph::{self, Input, Point, Status};
use prosecraft_core::network::layout::{self, LayoutEdge, LayoutNode, Options};
use prosecraft_core::network::meta::{self, Relation};
use prosecraft_core::network::relations::{self, Appearances, Mode, PagePair, Sourced, Story};
use prosecraft_core::network::templates::{self, Template};
use prosecraft_core::network::tile;

fn fixture() -> JsValue {
    let path = format!("{}/tests/fixtures/network.json", env!("CARGO_MANIFEST_DIR"));
    js::parse(&std::fs::read_to_string(&path).expect("fixture")).expect("json")
}

fn list<'a>(v: &'a JsValue, key: &str) -> &'a [JsValue] {
    v.get(key).and_then(JsValue::as_array).unwrap_or_default()
}

fn text(v: &JsValue) -> String {
    v.as_js_string()
        .map(JsString::to_string_lossy)
        .unwrap_or_default()
}

fn s(v: &JsValue, key: &str) -> String {
    v.get(key).map(text).unwrap_or_default()
}

fn opt(v: &JsValue, key: &str) -> Option<String> {
    v.get(key).filter(|x| !x.is_null()).map(text)
}

fn num(v: &JsValue, key: &str) -> f64 {
    v.get(key).and_then(JsValue::as_number).unwrap_or(f64::NAN)
}

fn json(v: &JsValue) -> String {
    js::stringify(v)
}

fn obj(entries: Vec<(&str, JsValue)>) -> JsValue {
    let mut o = JsObject::new();
    for (k, v) in entries {
        o.insert(k, v);
    }
    JsValue::Object(o)
}

fn strs(items: impl IntoIterator<Item = impl AsRef<str>>) -> JsValue {
    JsValue::Array(
        items
            .into_iter()
            .map(|i| JsValue::str(i.as_ref()))
            .collect(),
    )
}

fn ostr(v: &Option<String>) -> JsValue {
    v.as_deref().map_or(JsValue::Null, JsValue::str)
}

fn story(f: &JsValue) -> Story {
    let mut project = JsObject::new();
    project.insert("documents", JsValue::Array(list(f, "documents").to_vec()));
    project.insert("chapters", JsValue::Array(list(f, "chapters").to_vec()));
    project.insert(
        "researchFolders",
        JsValue::Array(list(f, "folders").to_vec()),
    );
    Story::from_project(&JsValue::Object(project))
}

fn sourced_js(r: &Sourced) -> JsValue {
    obj(vec![
        ("source", JsValue::str(&r.source)),
        ("relation", r.relation.to_js()),
    ])
}

fn pair_js(p: &PagePair) -> JsValue {
    obj(vec![
        ("a", JsValue::str(&p.a)),
        ("b", JsValue::str(&p.b)),
        ("key", JsValue::str(&p.key)),
        ("count", JsValue::Number(p.count as f64)),
    ])
}

fn mode(m: Mode) -> &'static str {
    match m {
        Mode::Confirmed => "confirmed",
        Mode::Auto => "auto",
    }
}

fn appearance_js(a: &Appearances) -> JsValue {
    let chapter =
        |c: &Option<(String, usize)>| c.as_ref().map_or(JsValue::Null, |c| JsValue::str(&c.0));
    let index =
        |c: &Option<(String, usize)>| JsValue::Number(c.as_ref().map_or(-1.0, |c| c.1 as f64));
    obj(vec![
        ("pageId", JsValue::str(&a.page_id)),
        ("kind", JsValue::str(a.kind)),
        (
            "scenes",
            JsValue::Array(
                a.scenes
                    .iter()
                    .map(|s| {
                        obj(vec![
                            ("sceneId", JsValue::str(&s.scene_id)),
                            ("mode", JsValue::str(mode(s.mode))),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("sceneCount", JsValue::Number(a.scene_count() as f64)),
        ("confirmedCount", JsValue::Number(a.confirmed as f64)),
        ("autoCount", JsValue::Number(a.auto as f64)),
        ("firstChapterId", chapter(&a.first_chapter)),
        ("lastChapterId", chapter(&a.last_chapter)),
        ("firstChapterIndex", index(&a.first_chapter)),
        ("lastChapterIndex", index(&a.last_chapter)),
    ])
}

/// The fixture's JSON with `undefined` dropped is what the Rust side builds.
fn same(have: JsValue, want: &JsValue, label: &str) {
    assert_eq!(json(&have), json(want), "{label}");
}

#[test]
fn metadata_normalizes_and_parses_as_prosecraft() {
    let f = fixture();
    for (n, case) in list(&f, "normalize").iter().enumerate() {
        let input = case.get("input").cloned().unwrap_or(JsValue::Null);
        same(
            meta::normalize_meta(&input).unwrap_or(JsValue::Null),
            case.get("output").unwrap_or(&JsValue::Null),
            &format!("normalize {n}"),
        );
    }
    for case in list(&f, "story") {
        let source = s(case, "text");
        match (meta::parse_story_meta(&source), case.get("error")) {
            (Ok(m), None) => {
                same(
                    m.clone().unwrap_or(JsValue::Null),
                    case.get("meta").unwrap_or(&JsValue::Null),
                    &source,
                );
                assert_eq!(
                    meta::serialize_story_meta(m.as_ref()),
                    s(case, "serialized"),
                    "{source}"
                );
            }
            (Err(e), Some(want)) => assert_eq!(e, text(want), "{source}"),
            (got, want) => panic!("{source}: {got:?} vs {want:?}"),
        }
    }
}

#[test]
fn appearances_relations_and_suggestions_match() {
    let f = fixture();
    let st = story(&f);
    let kinds: Vec<&str> = st.docs.iter().map(|d| st.page_kind(d)).collect();
    same(strs(kinds), f.get("kinds").expect("kinds"), "kinds");
    let apps = relations::appearances(&st);
    same(
        JsValue::Array(apps.iter().map(appearance_js).collect()),
        f.get("appearances").expect("a"),
        "appearances",
    );
    let rels = relations::relations_of(&st.docs);
    same(
        JsValue::Array(rels.iter().map(sourced_js).collect()),
        f.get("relations").expect("r"),
        "relations",
    );
    let pairs = relations::co_appearance_counts(&apps);
    same(
        JsValue::Array(pairs.iter().map(pair_js).collect()),
        f.get("pairs").expect("p"),
        "pairs",
    );
    let sugg = f.get("suggestions").expect("s");
    let defaults = relations::suggest_relations(&apps, &rels, &[], 3);
    same(
        JsValue::Array(defaults.iter().map(pair_js).collect()),
        sugg.get("defaults").expect("d"),
        "defaults",
    );
    let one = relations::suggest_relations(&apps, &rels, &["eli|jonah".into()], 1);
    same(
        JsValue::Array(one.iter().map(pair_js).collect()),
        sugg.get("one").expect("o"),
        "one",
    );
    for (r, want) in rels.iter().zip(list(&f, "active")) {
        let mut have: Vec<JsValue> = st
            .chapters
            .iter()
            .map(|c| JsValue::Bool(relations::relation_active_at(&r.relation, c, &st.chapters)))
            .collect();
        have.push(JsValue::Bool(relations::relation_active_at(
            &r.relation,
            "nope",
            &st.chapters,
        )));
        same(JsValue::Array(have), want, &r.relation.id);
    }
    let b = relations::relation_boundary_indexes(&rels, &st.chapters);
    same(
        JsValue::Array(b.iter().map(|i| JsValue::Number(*i as f64)).collect()),
        f.get("boundaries").expect("b"),
        "boundaries",
    );
}

fn edge_js(e: &graph::Edge) -> JsValue {
    let mut entries = vec![
        ("id", JsValue::str(&e.id)),
        ("from", JsValue::str(&e.from)),
        ("to", JsValue::str(&e.to)),
        ("type", JsValue::str(&e.kind)),
        ("label", JsValue::str(&e.label)),
    ];
    if let Some(t) = e.tension {
        entries.push(("tension", JsValue::Number(t)));
    }
    entries.push((
        "status",
        JsValue::str(if e.status == Status::Declared {
            "declared"
        } else {
            "suggested"
        }),
    ));
    if let Some(c) = e.count {
        entries.push(("count", JsValue::Number(c as f64)));
    }
    entries.push(("lane", JsValue::Number(e.lane)));
    entries.push(("laneCount", JsValue::Number(e.lane_count as f64)));
    obj(entries)
}

#[test]
fn the_graph_model_matches() {
    let f = fixture();
    let st = story(&f);
    let apps = relations::appearances(&st);
    let rels = relations::relations_of(&st.docs);
    let pairs = relations::co_appearance_counts(&apps);
    let pages: Vec<graph::Page> = st
        .pages()
        .map(|d| graph::Page {
            id: d.id.clone(),
            title: d.title.clone(),
            kind: st.page_kind(d),
        })
        .collect();
    let scene_chapters: HashMap<String, String> = st
        .docs
        .iter()
        .filter_map(|d| Some((d.id.clone(), d.chapter_id.clone()?)))
        .collect();
    for (n, case) in list(&f, "graphCases").iter().enumerate() {
        let chapter = opt(case, "chapterId");
        let kinds: Option<Vec<String>> = case
            .get("kinds")
            .and_then(JsValue::as_array)
            .map(|k| k.iter().map(text).collect());
        let kind_refs: Option<Vec<&str>> = kinds
            .as_ref()
            .map(|k| k.iter().map(String::as_str).collect());
        let exact = matches!(case.get("exact"), Some(JsValue::Bool(true)));
        let model = graph::build(&Input {
            pages: &pages,
            relations: &rels,
            appearances: &apps,
            suggestions: &pairs,
            chapters: &st.chapters,
            chapter_id: chapter.as_deref(),
            scene_chapters: exact.then_some(&scene_chapters),
            kinds: kind_refs.as_deref(),
        });
        let nodes = JsValue::Array(
            model
                .nodes
                .iter()
                .map(|n| {
                    obj(vec![
                        ("id", JsValue::str(&n.id)),
                        ("label", JsValue::str(&n.label)),
                        ("kind", JsValue::str(n.kind)),
                        ("initials", JsValue::str(&n.initials)),
                    ])
                })
                .collect(),
        );
        let want = case.get("model").expect("model");
        same(
            nodes,
            want.get("nodes").expect("nodes"),
            &format!("nodes {n}"),
        );
        same(
            JsValue::Array(model.edges.iter().map(edge_js).collect()),
            want.get("edges").expect("edges"),
            &format!("edges {n}"),
        );
    }
    for case in list(&f, "initials") {
        assert_eq!(graph::initials_of(&s(case, "label")), s(case, "initials"));
    }
}

#[test]
fn graph_helpers_match() {
    let f = fixture();
    let st = story(&f);
    let apps = relations::appearances(&st);
    let rels = relations::relations_of(&st.docs);
    let pairs = relations::co_appearance_counts(&apps);
    let pages: Vec<graph::Page> = st
        .pages()
        .map(|d| graph::Page {
            id: d.id.clone(),
            title: d.title.clone(),
            kind: st.page_kind(d),
        })
        .collect();
    let model = graph::build(&Input {
        pages: &pages,
        relations: &rels,
        appearances: &apps,
        suggestions: &pairs,
        chapters: &st.chapters,
        chapter_id: None,
        scene_chapters: None,
        kinds: None,
    });
    for case in list(&f, "queries") {
        same(
            strs(graph::matching_node_ids(&model.nodes, &s(case, "q"))),
            case.get("ids").expect("ids"),
            "query",
        );
    }
    for case in list(&f, "neighbours") {
        let id = s(case, "id");
        let mut n: Vec<String> = graph::neighbour_ids(&model.edges, &id)
            .into_iter()
            .collect();
        n.sort();
        let mut i: Vec<String> = graph::incident_edge_ids(&model.edges, &id)
            .into_iter()
            .collect();
        i.sort();
        same(strs(n), case.get("neighbours").expect("n"), &id);
        same(strs(i), case.get("incident").expect("i"), &id);
    }
    let point = |v: &JsValue| Point {
        x: num(v, "x"),
        y: num(v, "y"),
    };
    for case in list(&f, "bounds") {
        let (x, y, w, h) = graph::graph_bounds(list(case, "points").iter().map(point));
        let want = case.get("bounds").expect("bounds");
        assert_eq!(
            (x, y, w, h),
            (
                num(want, "x"),
                num(want, "y"),
                num(want, "width"),
                num(want, "height")
            )
        );
    }
    let nodes = [
        ("a", Point { x: 0.0, y: 0.0 }),
        ("b", Point { x: 10.0, y: 0.0 }),
        ("c", Point { x: 12.0, y: 0.0 }),
    ];
    let rels = [("a".to_owned(), "b".to_owned())];
    for case in list(&f, "connects") {
        let from = s(case, "from");
        let p = point(case.get("point").expect("p"));
        let r = num(case, "radius");
        let target = graph::connection_target(nodes.iter().copied(), &rels, &from, p, r);
        let want = case.get("target").expect("target");
        match target {
            None => assert!(want.is_null()),
            Some((id, valid)) => {
                assert_eq!(id, s(want, "id"));
                assert_eq!(Some(&JsValue::Bool(valid)), want.get("valid"));
            }
        }
        assert_eq!(
            graph::node_at_point(nodes.iter().copied(), p, r).map(str::to_owned),
            opt(case, "at")
        );
    }
    let w = layout::screen_to_world(
        Point { x: 100.0, y: 50.0 },
        layout::View {
            scale: 2.0,
            translate: Point { x: 10.0, y: -10.0 },
        },
    );
    let want = f.get("world").expect("world");
    assert_eq!((w.x, w.y), (num(want, "x"), num(want, "y")));
    let pair = PagePair {
        a: "jonah".into(),
        b: "mara".into(),
        key: "jonah|mara".into(),
        count: 3,
    };
    for case in list(&f, "accepted") {
        let choice = case.get("choice").expect("choice");
        let (source, r) = graph::relation_from_suggestion(
            &pair,
            &s(choice, "type"),
            &s(choice, "label"),
            s(choice, "direction") == "a-to-b",
        );
        let want = case.get("result").expect("result");
        assert_eq!(source, s(want, "sourceId"));
        let mut rel = want.get("relation").cloned().expect("relation");
        if let Some(o) = rel.as_object_mut() {
            let mut with_id = JsObject::new();
            with_id.insert("id", JsValue::str(""));
            for (k, v) in o.iter() {
                with_id.insert(k.clone(), v.clone());
            }
            *o = with_id;
        }
        same(r.to_js(), &rel, "accepted");
    }
}

#[test]
fn relations_are_added_updated_and_removed_as_prosecraft_does() {
    let f = fixture();
    let mut docs = list(&f, "documents").to_vec();
    relations::add_relation(
        &mut docs,
        "jonah",
        Relation {
            id: "new-1".into(),
            to: "harbor".into(),
            kind: "love".into(),
            label: Some("x".into()),
            tension: None,
            from: None,
            until: None,
            note: None,
        },
    );
    let find = |docs: &[JsValue], id: &str| {
        docs.iter()
            .find(|d| s(d, "id") == id)
            .cloned()
            .expect("doc")
    };
    let m = f.get("mutations").expect("mutations");
    same(
        find(&docs, "jonah"),
        m.get("added").expect("added"),
        "added",
    );
    relations::update_relation(&mut docs, "mara", "r1", |o| {
        o.insert("tension", JsValue::Number(5.0));
        o.insert("label", JsValue::str("best"));
        o.insert("id", JsValue::str("hijack"));
    });
    same(
        find(&docs, "mara"),
        m.get("updated").expect("updated"),
        "updated",
    );
    relations::remove_relation(&mut docs, "jonah", "r4");
    relations::remove_relation(&mut docs, "jonah", "new-1");
    same(
        find(&docs, "jonah"),
        m.get("removed").expect("removed"),
        "removed",
    );
}

fn character_js(c: &tile::TileCharacter) -> JsValue {
    obj(vec![
        ("id", JsValue::str(&c.id)),
        ("label", JsValue::str(&c.label)),
        ("kind", JsValue::str(c.kind)),
        ("mode", JsValue::str(mode(c.mode))),
    ])
}

#[test]
fn graph_tiles_and_appearance_strips_match() {
    let f = fixture();
    let st = story(&f);
    for case in list(&f, "tiles") {
        let scene = opt(case, "sceneId");
        let page = opt(case, "pageId");
        let m = tile::tile_model(&st, scene.as_deref(), page.as_deref());
        let have = obj(vec![
            ("sceneId", ostr(&m.scene_id)),
            ("chapterId", ostr(&m.chapter_id)),
            (
                "characters",
                JsValue::Array(m.characters.iter().map(character_js).collect()),
            ),
            (
                "suggestions",
                JsValue::Array(m.suggestions.iter().map(character_js).collect()),
            ),
            (
                "relations",
                JsValue::Array(m.relations.iter().map(sourced_js).collect()),
            ),
            (
                "pages",
                JsValue::Array(
                    m.pages
                        .iter()
                        .map(|p| {
                            obj(vec![
                                ("id", JsValue::str(&p.id)),
                                ("title", JsValue::str(&p.title)),
                                ("kind", JsValue::str(p.kind)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]);
        same(
            have,
            case.get("model").expect("model"),
            &format!("{scene:?} {page:?}"),
        );
    }
    let apps = relations::appearances(&st);
    for case in list(&f, "bars") {
        let id = s(case, "id");
        let bars = tile::chapter_bars(apps.iter().find(|a| a.page_id == id), &st);
        let have = JsValue::Array(
            bars.iter()
                .map(|b| {
                    obj(vec![
                        ("chapterId", JsValue::str(&b.chapter_id)),
                        ("total", JsValue::Number(b.total as f64)),
                        ("confirmed", JsValue::Number(b.confirmed as f64)),
                        ("auto", JsValue::Number(b.auto as f64)),
                        ("sceneTitles", strs(&b.scene_titles)),
                        ("firstSceneId", ostr(&b.first_scene_id)),
                        ("first", JsValue::Bool(b.first)),
                        ("last", JsValue::Bool(b.last)),
                    ])
                })
                .collect(),
        );
        same(have, case.get("bars").expect("bars"), &id);
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * (1.0 + b.abs())
}

#[test]
fn layouts_match_within_rounding() {
    let f = fixture();
    let point = |v: &JsValue| Point {
        x: num(v, "x"),
        y: num(v, "y"),
    };
    let maybe = |v: &JsValue, k: &str| v.get(k).and_then(JsValue::as_number);
    let mut compared = 0;
    for case in list(&f, "layouts") {
        let name = s(case, "name");
        let ids: Vec<String> = list(case, "nodes").iter().map(|n| s(n, "id")).collect();
        let nodes: Vec<LayoutNode> = list(case, "nodes")
            .iter()
            .zip(&ids)
            .map(|(n, id)| LayoutNode {
                id,
                at: n.get("x").map(|_| point(n)),
                pinned: matches!(n.get("pinned"), Some(JsValue::Bool(true))),
            })
            .collect();
        let ends: Vec<(String, String, Option<f64>)> = list(case, "edges")
            .iter()
            .map(|e| {
                (
                    s(e, "from"),
                    s(e, "to"),
                    e.get("weight").map(|w| w.as_number().unwrap_or(f64::NAN)),
                )
            })
            .collect();
        let edges: Vec<LayoutEdge> = ends
            .iter()
            .map(|(a, b, w)| LayoutEdge {
                from: a,
                to: b,
                weight: *w,
            })
            .collect();
        let o = case.get("options").cloned().unwrap_or(JsValue::Null);
        let options = Options {
            width: maybe(&o, "width"),
            height: maybe(&o, "height"),
            seed: maybe(&o, "seed"),
            iterations: maybe(&o, "iterations"),
            min_distance: maybe(&o, "minDistance"),
            previous: o
                .get("previous")
                .and_then(JsValue::as_object)
                .map(|p| {
                    p.iter()
                        .map(|(k, v)| (k.to_string_lossy(), point(v)))
                        .collect()
                })
                .unwrap_or_default(),
        };
        let have = layout::layout(&nodes, &edges, &options);
        let want = case
            .get("result")
            .and_then(JsValue::as_object)
            .expect("result");
        assert_eq!(have.len(), want.len(), "{name}");
        for (id, p) in want.iter() {
            let got = have[&id.to_string_lossy()];
            let p = point(p);
            assert!(
                close(got.x, p.x) && close(got.y, p.y),
                "{name} {id:?}: {got:?} vs {p:?}"
            );
            compared += 1;
        }
    }
    assert!(compared > 60);
    for case in list(&f, "fits") {
        let positions: Vec<Point> = case
            .get("positions")
            .and_then(JsValue::as_object)
            .map(|p| p.iter().map(|(_, v)| point(v)).collect())
            .unwrap_or_default();
        let vp = case.get("viewport").expect("viewport");
        let padding = case
            .get("padding")
            .map_or(40.0, |p| p.as_number().unwrap_or(f64::NAN));
        let view = layout::fit_view(&positions, num(vp, "width"), num(vp, "height"), padding);
        let want = case.get("view").expect("view");
        let t = point(want.get("translate").expect("t"));
        assert!(
            close(view.scale, num(want, "scale"))
                && close(view.translate.x, t.x)
                && close(view.translate.y, t.y),
            "{view:?} vs {want:?}"
        );
    }
    for case in list(&f, "clusters") {
        let nodes: Vec<(String, Option<String>)> = list(case, "nodes")
            .iter()
            .map(|n| (s(n, "id"), Some(s(n, "g"))))
            .collect();
        let o = case.get("options").cloned().unwrap_or(JsValue::Null);
        let have = layout::cluster_by_key(&nodes, maybe(&o, "radius"), maybe(&o, "spacing"));
        let want = case
            .get("result")
            .and_then(JsValue::as_object)
            .expect("result");
        assert_eq!(have.len(), want.len());
        for (id, p) in want.iter() {
            let got = have[&id.to_string_lossy()];
            let p = point(p);
            assert!(close(got.x, p.x) && close(got.y, p.y), "{id:?}");
        }
    }
}

#[test]
fn sheet_templates_match() {
    let f = fixture();
    let sheet_meta = js::parse(
        r#"{"kind":"character","sheet":{"secret":"s","custom":"c","role":"Lead","3":"three"}}"#,
    )
    .expect("meta");
    let project = js::parse(r#"{"sheetTemplates":[{"id":"mine","name":"Mine","fields":[{"id":"custom","label":"Custom","type":"text"}]}]}"#).expect("p");
    let mut all = templates::all_templates(&project);
    all.push(Template {
        id: "none".into(),
        name: String::new(),
        fields: Vec::new(),
    });
    for (t, case) in all.iter().zip(list(&f, "applied")) {
        let (m, orphans) = if t.id == "none" {
            templates::apply_template(None, &templates::built_in()[3])
        } else {
            templates::apply_template(Some(&sheet_meta), t)
        };
        same(
            obj(vec![("meta", m), ("orphans", strs(orphans))]),
            case.get("result").expect("r"),
            &t.id,
        );
    }
    for case in list(&f, "setValues") {
        let m = case.get("meta").filter(|m| !m.is_null());
        let have = templates::set_sheet_value(m, &s(case, "field"), &s(case, "value"));
        same(
            have.unwrap_or(JsValue::Null),
            case.get("result").expect("result"),
            "set",
        );
    }
    for case in list(&f, "validations") {
        let fj = case.get("field").expect("field");
        let field =
            templates::Template::from_js(&obj(vec![("fields", JsValue::Array(vec![fj.clone()]))]))
                .fields
                .remove(0);
        let error = templates::validate_sheet_value(&field, &s(case, "value")).map(|e| match e {
            templates::ValueError::Number => "number",
            templates::ValueError::Choice => "choice",
        });
        assert_eq!(
            error.map(str::to_owned),
            opt(case, "error"),
            "{:?} {:?}",
            field.kind,
            s(case, "value")
        );
    }
    let mut t: Vec<Template> = Vec::new();
    let mut n = 0;
    let mut id = || {
        n += 1;
        format!("t-{n}")
    };
    let steps = list(&f, "steps");
    let mut step = 0;
    let mut check = |t: &[Template]| {
        same(
            templates::to_js_list(t),
            &steps[step],
            &format!("step {step}"),
        );
        step += 1;
    };
    templates::add_template(&mut t, "  ", &mut id);
    check(&t);
    templates::add_template(&mut t, " Villains ", &mut id);
    check(&t);
    templates::rename_template(&mut t, "t-1", "  ");
    check(&t);
    templates::rename_template(&mut t, "t-1", " Heroes ");
    check(&t);
    let o = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    templates::add_field(
        &mut t,
        "t-1",
        " Mood ",
        "choice",
        Some(&o(&[" a ", "b", "a", "", " "])),
        &mut id,
    );
    check(&t);
    templates::add_field(&mut t, "t-1", "Age", "number", Some(&o(&["x"])), &mut id);
    check(&t);
    templates::add_field(&mut t, "t-1", "  ", "text", None, &mut id);
    check(&t);
    templates::add_field(&mut t, "t-1", "Tag", "choice", Some(&o(&[" "])), &mut id);
    check(&t);
    templates::rename_field(&mut t, "t-1", "t-3", " Feeling ");
    check(&t);
    templates::reorder_field(&mut t, "t-1", "t-5", -4.0);
    check(&t);
    templates::reorder_field(&mut t, "t-1", "t-5", 99.7);
    check(&t);
    templates::reorder_field(&mut t, "t-1", "t-5", f64::NAN);
    check(&t);
    templates::change_field_type(&mut t, "t-1", "t-3", "text");
    check(&t);
    templates::set_field_options(&mut t, "t-1", "t-4", &o(&[" q ", "q", "r"]));
    check(&t);
    templates::set_field_options(&mut t, "t-1", "t-4", &o(&[" "]));
    check(&t);
    templates::remove_field(&mut t, "t-1", "t-4");
    check(&t);
    templates::remove_template(&mut t, "t-2");
    check(&t);
    assert_eq!(step, steps.len());
    let mut taken = templates::built_in();
    taken.push(Template {
        id: "x".into(),
        name: "Heroes".into(),
        fields: Vec::new(),
    });
    taken.push(Template {
        id: "y".into(),
        name: "Heroes copy".into(),
        fields: Vec::new(),
    });
    for case in list(&f, "names") {
        assert_eq!(
            templates::unique_template_name(&taken, &s(case, "name")),
            s(case, "unique")
        );
    }
    let mut full = Vec::new();
    for i in 0..22 {
        templates::add_template(&mut full, &format!("T{i}"), || format!("id{i}"));
    }
    assert_eq!(full.len() as f64, num(&f, "fullCount"));
    let mut many = Vec::new();
    templates::add_template(&mut many, "Many", || "m".into());
    for i in 0..42 {
        templates::add_field(&mut many, "m", &format!("F{i}"), "text", None, || {
            format!("f{i}")
        });
    }
    assert_eq!(many[0].fields.len() as f64, num(&f, "manyFields"));
}

/// The document `meta`, `sheetTemplates` and `dismissedSuggestions` through the open folder:
/// written as ProseCraft writes them, read back, and a `meta` edited outside normalized.
#[test]
fn the_open_folder_keeps_story_data() {
    use prosecraft_core::storage::crypto::OsRandom;
    use prosecraft_core::storage::stamp::project_content_key;
    use prosecraft_core::sync::memory::MemoryFs;
    use prosecraft_core::sync::open_layout::read_open_projects;
    use prosecraft_core::sync::{OpenFolderProvider, SyncProvider};
    let f = fixture();
    let sync = f.get("sync").expect("sync");
    let now = num(sync, "now");
    let files = |v: &JsValue| {
        MemoryFs::with_files(
            v.as_object()
                .expect("files")
                .iter()
                .map(|(k, c)| (k.to_string_lossy(), text(c))),
        )
    };
    let project = sync.get("project").cloned().expect("project");
    assert_eq!(
        project_content_key(project.get("project").expect("p")),
        s(sync, "contentKey")
    );
    let mut fs = MemoryFs::new();
    {
        let mut provider = OpenFolderProvider::new(&mut fs, now);
        let normalized = provider.normalize(vec![project]);
        provider
            .push(&normalized, None, &mut OsRandom)
            .expect("push");
    }
    let want = sync
        .get("files")
        .and_then(JsValue::as_object)
        .expect("files");
    let marker = |p: &str| p.ends_with("prosecraft-folder.json");
    for (path, content) in want.iter() {
        let path = path.to_string_lossy();
        if marker(&path) {
            continue;
        }
        assert_eq!(
            fs.files().get(&path).map(String::as_str),
            Some(text(content).as_str()),
            "{path}"
        );
    }
    for (key, read) in [
        ("files", "read"),
        ("edited", "readEdited"),
        ("emptied", "readEmptied"),
    ] {
        let mut fs = files(sync.get(key).expect("files"));
        let projects = read_open_projects(&mut fs, now).expect("read");
        same(
            JsValue::Array(projects),
            sync.get(read).expect("read"),
            read,
        );
    }
}
