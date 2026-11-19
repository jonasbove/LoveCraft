//! The story timeline (ProseCraft `src/lib/timeline.ts` and the page's timeline handlers,
//! upstream `7dae544`): dated moments in the project's `timelineNodes`, in order.

use crate::edit::EditError;
use crate::js::{JsObject, JsValue};

pub const TITLE_LIMIT: usize = 160;
pub const WHEN_LIMIT: usize = 80;
pub const DESCRIPTION_LIMIT: usize = 4000;

/// A moment's text fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Title,
    Date,
    Time,
    Description,
}

impl Field {
    fn key(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Date => "date",
            Self::Time => "time",
            Self::Description => "description",
        }
    }

    pub fn limit(self) -> usize {
        match self {
            Self::Title => TITLE_LIMIT,
            Self::Date | Self::Time => WHEN_LIMIT,
            Self::Description => DESCRIPTION_LIMIT,
        }
    }
}

fn nodes_mut(project: &mut JsValue) -> Result<&mut Vec<JsValue>, EditError> {
    let object = project.as_object_mut().ok_or(EditError::Shape)?;
    if !matches!(object.get("timelineNodes"), Some(JsValue::Array(_))) {
        object.insert("timelineNodes", JsValue::Array(Vec::new()));
    }
    match object.get_mut("timelineNodes") {
        Some(JsValue::Array(list)) => Ok(list),
        _ => Err(EditError::Shape),
    }
}

fn has_id(node: &JsValue, id: &str) -> bool {
    node.get("id")
        .and_then(JsValue::as_js_string)
        .is_some_and(|i| i.eq_str(id))
}

/// `createTimelineNode` added last.
pub fn add_node(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    let mut node = JsObject::new();
    node.insert("id", JsValue::str(id));
    for key in ["title", "date", "time", "description"] {
        node.insert(key, JsValue::str(""));
    }
    nodes_mut(project)?.push(JsValue::Object(node));
    Ok(())
}

/// `updateTimelineNode` for one field, kept within its limit (UTF-16 units).
pub fn set_field(
    project: &mut JsValue,
    id: &str,
    field: Field,
    value: &str,
) -> Result<(), EditError> {
    let units: Vec<u16> = value.encode_utf16().take(field.limit()).collect();
    let node = nodes_mut(project)?
        .iter_mut()
        .find(|n| has_id(n, id))
        .ok_or(EditError::Shape)?;
    node.as_object_mut().ok_or(EditError::Shape)?.insert(
        field.key(),
        JsValue::String(crate::js::JsString::from_units(units)),
    );
    Ok(())
}

/// `removeTimelineNode`.
pub fn remove_node(project: &mut JsValue, id: &str) -> Result<(), EditError> {
    nodes_mut(project)?.retain(|n| !has_id(n, id));
    Ok(())
}

/// `moveTimelineNode`: `offset` places earlier (negative) or later, kept within the list.
pub fn move_node(project: &mut JsValue, id: &str, offset: isize) -> Result<(), EditError> {
    let nodes = nodes_mut(project)?;
    let Some(index) = nodes.iter().position(|n| has_id(n, id)) else {
        return Ok(());
    };
    let target = (index as isize + offset).clamp(0, nodes.len() as isize - 1) as usize;
    if target != index {
        let node = nodes.remove(index);
        nodes.insert(target, node);
    }
    Ok(())
}

/// `reorderToTarget` and `reorderTimelineNodes` (a moment dragged onto another): before or
/// after `target`.
pub fn reorder(
    project: &mut JsValue,
    id: &str,
    target: &str,
    after: bool,
) -> Result<(), EditError> {
    let nodes = nodes_mut(project)?;
    let (Some(from), Some(to)) = (
        nodes.iter().position(|n| has_id(n, id)),
        nodes.iter().position(|n| has_id(n, target)),
    ) else {
        return Ok(());
    };
    let mut insert = if after { to + 1 } else { to };
    if insert > from {
        insert -= 1;
    }
    if insert != from {
        let node = nodes.remove(from);
        nodes.insert(insert, node);
    }
    Ok(())
}

#[cfg(test)]
mod reorder_tests {
    use super::reorder;
    use crate::js::{JsValue, parse};

    fn order(p: &JsValue) -> String {
        p.get("timelineNodes")
            .and_then(JsValue::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|n| {
                n.get("id")
                    .and_then(JsValue::as_js_string)
                    .map(|s| s.to_string_lossy())
            })
            .collect()
    }

    #[test]
    fn moments_go_before_or_after_the_one_dropped_on() {
        let mut p = parse(r#"{"timelineNodes":[{"id":"a"},{"id":"b"},{"id":"c"},{"id":"d"}]}"#)
            .expect("json");
        reorder(&mut p, "a", "c", true).expect("reorder");
        assert_eq!(order(&p), "bcad");
        reorder(&mut p, "d", "b", false).expect("reorder");
        assert_eq!(order(&p), "dbca");
        // Onto its own neighbour's near side: no change.
        reorder(&mut p, "b", "c", false).expect("reorder");
        assert_eq!(order(&p), "dbca");
        reorder(&mut p, "missing", "c", false).expect("reorder");
        assert_eq!(order(&p), "dbca");
    }
}

#[cfg(test)]
mod tests {
    use super::{Field, add_node, move_node, remove_node, set_field};
    use crate::js::{parse, stringify};

    #[test]
    fn moments_are_added_edited_moved_and_removed() {
        let mut p = parse(r#"{"timelineNodes":[{"id":"a","title":"A","date":"","time":"","description":"","extra":1}]}"#).expect("json");
        add_node(&mut p, "b").expect("add");
        set_field(&mut p, "b", Field::Title, "Battle").expect("set");
        set_field(&mut p, "b", Field::Date, &"x".repeat(100)).expect("set");
        move_node(&mut p, "b", -5).expect("move");
        assert_eq!(
            stringify(&p),
            format!(
                r#"{{"timelineNodes":[{{"id":"b","title":"Battle","date":"{}","time":"","description":""}},{{"id":"a","title":"A","date":"","time":"","description":"","extra":1}}]}}"#,
                "x".repeat(80)
            )
        );
        remove_node(&mut p, "b").expect("remove");
        assert_eq!(
            stringify(&p),
            r#"{"timelineNodes":[{"id":"a","title":"A","date":"","time":"","description":"","extra":1}]}"#
        );
    }
}
