//! Split view (ProseCraft's `src/lib/split.ts`): up to five reference panes beside the editor,
//! each showing a scene, a wiki page or a wiki file, and how many columns they take.

/// `MAX_SPLIT_PANES`.
pub const MAX_PANES: usize = 5;

/// `SplitPaneState`: a pane and what it shows (`doc:<id>`, `file:<id>`, or "").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    pub target: String,
}

impl Pane {
    pub fn new(id: &str, target: &str) -> Self {
        Self {
            id: id.to_owned(),
            target: target.to_owned(),
        }
    }
}

/// `SplitTarget`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Doc(String),
    File(String),
    /// The graph tile (ProseCraft#31): of the scene being written (`None`) or of one Wiki page.
    Graph(Option<String>),
}

/// `docTarget`, `fileTarget`.
pub fn doc_target(id: &str) -> String {
    format!("doc:{id}")
}

pub fn file_target(id: &str) -> String {
    format!("file:{id}")
}

/// `graphTarget`: `graph:scene`, or `graph:page:<id>` for one Wiki page.
pub fn graph_target(page: Option<&str>) -> String {
    match page {
        Some(id) => format!("graph:page:{id}"),
        None => "graph:scene".to_owned(),
    }
}

/// `parseSplitTarget`.
pub fn parse_target(target: &str) -> Option<Target> {
    if target == "graph:scene" {
        return Some(Target::Graph(None));
    }
    if let Some(id) = target
        .strip_prefix("graph:page:")
        .filter(|id| !id.is_empty())
    {
        return Some(Target::Graph(Some(id.to_owned())));
    }
    if let Some(id) = target.strip_prefix("doc:").filter(|id| !id.is_empty()) {
        return Some(Target::Doc(id.to_owned()));
    }
    target
        .strip_prefix("file:")
        .filter(|id| !id.is_empty())
        .map(|id| Target::File(id.to_owned()))
}

/// `canAddSplitPane`.
pub fn can_add(panes: &[Pane]) -> bool {
    panes.len() < MAX_PANES
}

/// `addSplitPane`: an empty pane, until the limit.
pub fn add(panes: &[Pane], id: &str) -> Vec<Pane> {
    let mut next = panes.to_vec();
    if can_add(panes) {
        next.push(Pane::new(id, ""));
    }
    next
}

/// `removeSplitPane`.
pub fn remove(panes: &[Pane], id: &str) -> Vec<Pane> {
    panes.iter().filter(|p| p.id != id).cloned().collect()
}

/// `openBeside`: a pane already showing it, else the first empty pane, else a new pane, else
/// the last pane.
pub fn open_beside(panes: &[Pane], target: &str, new_id: &str) -> Vec<Pane> {
    let mut next = panes.to_vec();
    if panes.iter().any(|p| p.target == target) {
        return next;
    }
    if let Some(free) = next.iter_mut().find(|p| p.target.is_empty()) {
        free.target = target.to_owned();
        return next;
    }
    if can_add(panes) {
        next.push(Pane::new(new_id, target));
        return next;
    }
    if let Some(last) = next.last_mut() {
        last.target = target.to_owned();
    }
    next
}

/// `moveSplitPane`: swapped with its neighbour (`delta` −1 left, 1 right).
pub fn move_pane(panes: &[Pane], id: &str, delta: isize) -> Vec<Pane> {
    let mut next = panes.to_vec();
    if let Some(index) = panes.iter().position(|p| p.id == id) {
        let neighbour = index as isize + delta;
        if neighbour >= 0 && (neighbour as usize) < panes.len() {
            next.swap(index, neighbour as usize);
        }
    }
    next
}

/// `setSplitTarget`.
pub fn set_target(panes: &[Pane], id: &str, target: &str) -> Vec<Pane> {
    panes
        .iter()
        .map(|p| {
            if p.id == id {
                Pane::new(&p.id, target)
            } else {
                p.clone()
            }
        })
        .collect()
}

/// `pruneSplitTargets`: targets that no longer exist cleared, the panes kept.
pub fn prune(panes: &[Pane], valid: &std::collections::HashSet<String>) -> Vec<Pane> {
    panes
        .iter()
        .map(|p| {
            if !p.target.is_empty() && !valid.contains(&p.target) {
                Pane::new(&p.id, "")
            } else {
                p.clone()
            }
        })
        .collect()
}

/// `tileColumns`: columns for `tiles` tiles in `width`, slanted toward balanced rows.
pub fn tile_columns(tiles: usize, width: f64, min_tile_width: f64) -> usize {
    if tiles <= 1 {
        return 1;
    }
    let fitting = (width / min_tile_width).floor();
    let fitting = if fitting.is_finite() && fitting >= 0.0 {
        fitting as usize
    } else {
        1
    };
    let max = tiles.min(fitting);
    let mut columns = max;
    while columns > 1 {
        let remainder = tiles % columns;
        if remainder == 0 || remainder >= columns - 1 {
            return columns;
        }
        columns -= 1;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panes(list: &[(&str, &str)]) -> Vec<Pane> {
        list.iter().map(|(id, t)| Pane::new(id, t)).collect()
    }

    #[test]
    fn targets_and_limits() {
        assert_eq!(
            parse_target(&doc_target("a-1")),
            Some(Target::Doc("a-1".into()))
        );
        assert_eq!(
            parse_target(&file_target("f:2")),
            Some(Target::File("f:2".into()))
        );
        assert_eq!(parse_target(""), None);
        assert_eq!(parse_target("doc:"), None);
        assert_eq!(parse_target("graph:scene"), Some(Target::Graph(None)));
        assert_eq!(
            parse_target(&graph_target(Some("p"))),
            Some(Target::Graph(Some("p".into())))
        );
        assert_eq!(parse_target("graph:page:"), None);
        assert_eq!(parse_target("other:1"), None);
        let mut list = Vec::new();
        for i in 0..MAX_PANES + 2 {
            list = add(&list, &format!("p{i}"));
        }
        assert_eq!(list.len(), MAX_PANES);
        assert!(!can_add(&list) && can_add(&list[1..]));
        let targeted = set_target(&add(&add(&[], "a"), "b"), "b", &doc_target("x"));
        assert_eq!(targeted, panes(&[("a", ""), ("b", "doc:x")]));
        assert_eq!(remove(&targeted, "a"), panes(&[("b", "doc:x")]));
        let valid: std::collections::HashSet<String> = ["file:kept".to_owned()].into();
        assert_eq!(
            prune(
                &panes(&[("a", "doc:gone"), ("b", "file:kept"), ("c", "")]),
                &valid
            ),
            panes(&[("a", ""), ("b", "file:kept"), ("c", "")])
        );
    }

    #[test]
    fn open_beside_reuses_fills_appends_or_replaces() {
        let one = panes(&[("a", "doc:x")]);
        assert_eq!(open_beside(&one, "doc:x", "new"), one);
        assert_eq!(
            open_beside(
                &panes(&[("a", "doc:x"), ("b", ""), ("c", "")]),
                "file:y",
                "new"
            ),
            panes(&[("a", "doc:x"), ("b", "file:y"), ("c", "")])
        );
        assert_eq!(
            open_beside(&one, "file:y", "b"),
            panes(&[("a", "doc:x"), ("b", "file:y")])
        );
        assert_eq!(open_beside(&[], "doc:x", "a"), panes(&[("a", "doc:x")]));
        let full: Vec<Pane> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|id| Pane::new(id, &doc_target(id)))
            .collect();
        let next = open_beside(&full, "file:fresh", "new");
        assert_eq!(next.len(), MAX_PANES);
        assert_eq!(next[4], Pane::new("e", "file:fresh"));
        assert_eq!(&next[..4], &full[..4]);
    }

    #[test]
    fn panes_move_and_tiles_balance() {
        let list = panes(&[("a", ""), ("b", ""), ("c", "")]);
        let ids = |l: Vec<Pane>| l.into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(move_pane(&list, "b", -1)), ["b", "a", "c"]);
        assert_eq!(ids(move_pane(&list, "b", 1)), ["a", "c", "b"]);
        assert_eq!(ids(move_pane(&list, "a", -1)), ["a", "b", "c"]);
        assert_eq!(ids(move_pane(&list, "c", 1)), ["a", "b", "c"]);
        assert_eq!(ids(move_pane(&list, "missing", 1)), ["a", "b", "c"]);
        for (tiles, width, min, want) in [
            (0, 1600.0, 340.0, 1),
            (1, 1600.0, 340.0, 1),
            (6, 200.0, 340.0, 1),
            (2, 5000.0, 340.0, 2),
            (3, 5000.0, 340.0, 3),
            (4, 680.0, 340.0, 2),
            (5, 1020.0, 340.0, 3),
            (6, 1020.0, 340.0, 3),
            (4, 1020.0, 340.0, 2),
            (6, 1360.0, 340.0, 3),
            (4, 1000.0, 250.0, 4),
            (4, 1000.0, 400.0, 2),
        ] {
            assert_eq!(tile_columns(tiles, width, min), want, "{tiles} in {width}");
        }
    }
}
