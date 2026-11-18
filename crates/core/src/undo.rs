//! Undo and redo as ProseCraft keeps them (`src/lib/undo/manager.ts`, upstream `2d1cf18`): a
//! history per text field, of text differences grouped as typing goes on, and a history per page
//! (a scene, a chapter, the storyboard…) that orders the field steps with steps of the project's
//! state (storyboard, timeline, publish options, citations, chapters, documents), so Undo outside
//! a text field takes back the last change made on that page, whichever it was.

use std::collections::HashMap;

use crate::js::{JsObject, JsValue};

/// `UNDO_GROUP_MAX_MS`: typing joins one step for at most this long from its start.
pub const GROUP_MAX_MS: f64 = 2000.0;
const MAX_FIELD_STEPS: usize = 1000;
const MAX_PAGE_STEPS: usize = 1000;
/// `STATE_MERGE_MS`: the same change repeated this soon (a drag) is one step.
const STATE_MERGE_MS: f64 = 800.0;

/// `EditKind`: what an edit did, for grouping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    Type,
    Delete,
    Newline,
    Compose,
    Other,
}

/// `TextDiff`: the text from `from` (a byte offset on a character boundary) that was removed,
/// and what was put there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextDiff {
    pub from: usize,
    pub removed: String,
    pub inserted: String,
}

/// `diffText`.
pub fn diff_text(before: &str, after: &str) -> TextDiff {
    let (b, a) = (before.as_bytes(), after.as_bytes());
    let limit = b.len().min(a.len());
    let mut from = b.iter().zip(a).take_while(|(x, y)| x == y).count();
    while !before.is_char_boundary(from) || !after.is_char_boundary(from) {
        from -= 1;
    }
    let mut suffix = 0;
    while suffix < limit - from && b[b.len() - 1 - suffix] == a[a.len() - 1 - suffix] {
        suffix += 1;
    }
    while !before.is_char_boundary(b.len() - suffix) || !after.is_char_boundary(a.len() - suffix) {
        suffix -= 1;
    }
    TextDiff {
        from,
        removed: before[from..b.len() - suffix].to_owned(),
        inserted: after[from..a.len() - suffix].to_owned(),
    }
}

fn revert(text: &str, diff: &TextDiff) -> String {
    let end = (diff.from + diff.inserted.len()).min(text.len());
    format!("{}{}{}", &text[..diff.from], diff.removed, &text[end..])
}

fn reapply(text: &str, diff: &TextDiff) -> String {
    let end = (diff.from + diff.removed.len()).min(text.len());
    format!("{}{}{}", &text[..diff.from], diff.inserted, &text[end..])
}

#[derive(Clone, Debug)]
struct FieldEntry<S> {
    id: u64,
    page: Option<String>,
    diff: TextDiff,
    before: S,
    after: S,
}

#[derive(Clone, Debug)]
struct OpenGroup {
    kind: EditKind,
    start: f64,
    base: String,
}

#[derive(Clone, Debug)]
struct FieldHistory<S> {
    current: String,
    done: Vec<FieldEntry<S>>,
    undone: Vec<FieldEntry<S>>,
    open: Option<OpenGroup>,
}

/// One item of an array change: its state before and after (whole items when it was added or
/// removed, else only the props that changed).
#[derive(Clone, Debug, PartialEq)]
pub struct ItemChange {
    pub id: String,
    pub before: Option<JsObject>,
    pub after: Option<JsObject>,
    pub whole: bool,
}

/// `StateChange`.
#[derive(Clone, Debug, PartialEq)]
pub enum StateChange {
    Array {
        before_ids: Vec<String>,
        after_ids: Vec<String>,
        items: Vec<ItemChange>,
    },
    Object {
        before: JsObject,
        after: JsObject,
    },
}

/// `StateStep`: changes by collection, when they were made, and what they changed (for merging
/// a repeated change).
#[derive(Clone, Debug, PartialEq)]
pub struct StateStep {
    pub changes: Vec<(String, StateChange)>,
    pub time: f64,
    pub signature: String,
}

impl StateStep {
    fn change(&self, name: &str) -> Option<&StateChange> {
        self.changes.iter().find(|(n, _)| n == name).map(|(_, c)| c)
    }

    /// Whether the step changes collection `name`.
    pub fn touches(&self, name: &str) -> bool {
        self.change(name).is_some()
    }
}

#[derive(Clone, Debug)]
enum PageStep {
    Field { key: String, entry: u64 },
    State(Box<StateStep>),
    External { key: String },
}

#[derive(Clone, Debug, Default)]
struct PageHistory {
    done: Vec<PageStep>,
    undone: Vec<PageStep>,
}

/// `FieldResult`: a field's text after undo or redo, and the selection to put back.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldResult<S> {
    pub key: String,
    pub value: String,
    pub selection: S,
}

/// `PageResult`.
#[derive(Clone, Debug, PartialEq)]
pub enum PageResult<S> {
    Field(FieldResult<S>),
    State(StateStep),
    External { key: String },
}

/// `RecordOptions`.
#[derive(Clone, Debug)]
pub struct Edit<S> {
    pub kind: EditKind,
    pub time: f64,
    pub caret_moved: bool,
    pub before: S,
    pub after: S,
}

/// `UndoManager`: histories per text field and per page. A page's steps point to field entries
/// instead of holding text, so a field and its page agree on which edits are undone, whichever
/// of the two ran the undo.
#[derive(Clone, Debug)]
pub struct UndoManager<S> {
    fields: HashMap<String, FieldHistory<S>>,
    pages: HashMap<String, PageHistory>,
    next_entry: u64,
}

impl<S> Default for UndoManager<S> {
    fn default() -> Self {
        Self {
            fields: HashMap::new(),
            pages: HashMap::new(),
            next_entry: 1,
        }
    }
}

fn push_capped(list: &mut Vec<PageStep>, step: PageStep) {
    list.push(step);
    if list.len() > MAX_PAGE_STEPS {
        list.remove(0);
    }
}

fn take_field_step(list: &mut Vec<PageStep>, key: &str, entry: u64) -> Option<PageStep> {
    let index = list.iter().rposition(
        |s| matches!(s, PageStep::Field { key: k, entry: e } if k == key && *e == entry),
    )?;
    Some(list.remove(index))
}

fn take_external_step(list: &mut Vec<PageStep>, key: &str) -> Option<PageStep> {
    let index = list
        .iter()
        .rposition(|s| matches!(s, PageStep::External { key: k } if k == key))?;
    Some(list.remove(index))
}

impl<S: Clone> UndoManager<S> {
    pub fn has(&self, key: &str) -> bool {
        self.fields.contains_key(key)
    }

    /// The text a field's history ends at.
    pub fn current(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(|f| f.current.as_str())
    }

    /// `sync`: starts a field's history, or discards it when its text changed outside the
    /// history (a sync, an import).
    pub fn sync(&mut self, key: &str, actual: &str) {
        let stale = match self.fields.get(key) {
            None => true,
            Some(field) => field.current != actual,
        };
        if !stale {
            return;
        }
        if let Some(field) = self.fields.remove(key) {
            self.drop_entries(key, &field.done);
            self.drop_entries(key, &field.undone);
        }
        self.fields.insert(
            key.to_owned(),
            FieldHistory {
                current: actual.to_owned(),
                done: Vec::new(),
                undone: Vec::new(),
                open: None,
            },
        );
    }

    /// `record`: the field's new text after an edit on `page`. Typing (or deleting) joins the
    /// last step while it goes on for under two seconds from its start without the caret moving
    /// elsewhere; a new line and anything else is a step of its own.
    pub fn record(&mut self, key: &str, page: Option<&str>, next: &str, edit: Edit<S>) {
        let Some(field) = self.fields.get(key) else {
            return;
        };
        if field.current == next {
            return;
        }
        let joins = match (&field.open, field.done.last()) {
            (Some(open), Some(top)) => {
                top.page.as_deref() == page
                    && !edit.caret_moved
                    && open.kind == edit.kind
                    && !matches!(edit.kind, EditKind::Newline | EditKind::Other)
                    && (edit.kind == EditKind::Compose || edit.time - open.start < GROUP_MAX_MS)
            }
            _ => false,
        };
        if joins {
            let Some(field) = self.fields.get_mut(key) else {
                return;
            };
            let base = field
                .open
                .as_ref()
                .map(|o| o.base.clone())
                .unwrap_or_default();
            if let Some(top) = field.done.last_mut() {
                top.diff = diff_text(&base, next);
                top.after = edit.after;
            }
            field.current = next.to_owned();
            return;
        }
        let id = self.next_entry;
        self.next_entry += 1;
        let undone = {
            let Some(field) = self.fields.get_mut(key) else {
                return;
            };
            let entry = FieldEntry {
                id,
                page: page.map(str::to_owned),
                diff: diff_text(&field.current, next),
                before: edit.before,
                after: edit.after,
            };
            field.open = Some(OpenGroup {
                kind: edit.kind,
                start: edit.time,
                base: field.current.clone(),
            });
            let undone = std::mem::take(&mut field.undone);
            field.done.push(entry);
            field.current = next.to_owned();
            undone
        };
        self.drop_entries(key, &undone);
        let dropped = self
            .fields
            .get_mut(key)
            .filter(|f| f.done.len() > MAX_FIELD_STEPS)
            .map(|f| vec![f.done.remove(0)]);
        if let Some(dropped) = dropped {
            self.drop_entries(key, &dropped);
        }
        if let Some(page) = page {
            let history = self.page(page);
            history.undone.clear();
            push_capped(
                &mut history.done,
                PageStep::Field {
                    key: key.to_owned(),
                    entry: id,
                },
            );
        }
    }

    /// `undoField`.
    pub fn undo_field(&mut self, key: &str) -> Option<FieldResult<S>> {
        let field = self.fields.get_mut(key)?;
        let entry = field.done.pop()?;
        field.open = None;
        field.current = revert(&field.current, &entry.diff);
        let value = field.current.clone();
        field.undone.push(entry.clone());
        if let Some(page) = &entry.page {
            let history = self.page(page);
            if let Some(step) = take_field_step(&mut history.done, key, entry.id) {
                history.undone.push(step);
            }
        }
        Some(FieldResult {
            key: key.to_owned(),
            value,
            selection: entry.before,
        })
    }

    /// `redoField`.
    pub fn redo_field(&mut self, key: &str) -> Option<FieldResult<S>> {
        let field = self.fields.get_mut(key)?;
        let entry = field.undone.pop()?;
        field.open = None;
        field.current = reapply(&field.current, &entry.diff);
        let value = field.current.clone();
        field.done.push(entry.clone());
        if let Some(page) = &entry.page {
            let history = self.page(page);
            let step =
                take_field_step(&mut history.undone, key, entry.id).unwrap_or(PageStep::Field {
                    key: key.to_owned(),
                    entry: entry.id,
                });
            push_capped(&mut history.done, step);
        }
        Some(FieldResult {
            key: key.to_owned(),
            value,
            selection: entry.after,
        })
    }

    /// `undoPage`: the newest step on the page; a field step `can_apply` rejects is discarded.
    pub fn undo_page(
        &mut self,
        page: &str,
        can_apply: impl Fn(&str) -> bool,
    ) -> Option<PageResult<S>> {
        loop {
            let step = self.page(page).done.last().cloned()?;
            match step {
                PageStep::State(step) => {
                    let history = self.page(page);
                    if let Some(taken) = history.done.pop() {
                        history.undone.push(taken);
                    }
                    return Some(PageResult::State(*step));
                }
                PageStep::External { key } => {
                    if can_apply(&key) {
                        return Some(PageResult::External { key });
                    }
                    self.page(page).done.pop();
                }
                PageStep::Field { key, entry } => {
                    let newest = self
                        .fields
                        .get(&key)
                        .and_then(|f| f.done.last())
                        .is_some_and(|e| e.id == entry);
                    if newest && can_apply(&key) {
                        return self.undo_field(&key).map(PageResult::Field);
                    }
                    self.page(page).done.pop();
                }
            }
        }
    }

    /// `redoPage`.
    pub fn redo_page(
        &mut self,
        page: &str,
        can_apply: impl Fn(&str) -> bool,
    ) -> Option<PageResult<S>> {
        loop {
            let step = self.page(page).undone.last().cloned()?;
            match step {
                PageStep::State(step) => {
                    let history = self.page(page);
                    if let Some(taken) = history.undone.pop() {
                        history.done.push(taken);
                    }
                    return Some(PageResult::State(*step));
                }
                PageStep::External { key } => {
                    if can_apply(&key) {
                        return Some(PageResult::External { key });
                    }
                    self.page(page).undone.pop();
                }
                PageStep::Field { key, entry } => {
                    let newest = self
                        .fields
                        .get(&key)
                        .and_then(|f| f.undone.last())
                        .is_some_and(|e| e.id == entry);
                    if newest && can_apply(&key) {
                        return self.redo_field(&key).map(PageResult::Field);
                    }
                    self.page(page).undone.pop();
                }
            }
        }
    }

    /// `recordExternal`: a step in an editor that keeps its own history; the page only holds
    /// its place in the order.
    pub fn record_external(&mut self, page: &str, key: &str) {
        let history = self.page(page);
        history.undone.clear();
        push_capped(
            &mut history.done,
            PageStep::External {
                key: key.to_owned(),
            },
        );
    }

    /// `externalUndone`.
    pub fn external_undone(&mut self, page: &str, key: &str) {
        let history = self.page(page);
        if let Some(step) = take_external_step(&mut history.done, key) {
            history.undone.push(step);
        }
    }

    /// `externalRedone`.
    pub fn external_redone(&mut self, page: &str, key: &str) {
        let history = self.page(page);
        let step = take_external_step(&mut history.undone, key).unwrap_or(PageStep::External {
            key: key.to_owned(),
        });
        push_capped(&mut history.done, step);
    }

    /// `discardNewest`: the external editor had nothing to apply.
    pub fn discard_newest(&mut self, page: &str, undo: bool) {
        let history = self.page(page);
        if undo {
            history.done.pop();
        } else {
            history.undone.pop();
        }
    }

    /// `forgetExternal`.
    pub fn forget_external(&mut self, page: &str, key: &str) {
        let history = self.page(page);
        while take_external_step(&mut history.done, key).is_some() {}
        while take_external_step(&mut history.undone, key).is_some() {}
    }

    /// `pushState`: a change to the project's state; the same props changed again within 800 ms
    /// join it.
    pub fn push_state(&mut self, page: &str, step: StateStep) {
        let history = self.page(page);
        history.undone.clear();
        if let Some(PageStep::State(top)) = history.done.last_mut()
            && top.signature == step.signature
            && step.time - top.time < STATE_MERGE_MS
        {
            **top = merge_steps(top, &step);
            return;
        }
        push_capped(&mut history.done, PageStep::State(Box::new(step)));
    }

    /// `moveState`: keeps a state step reachable after applying it switched the page.
    pub fn move_state(&mut self, from: &str, to: &str, step: &StateStep) {
        if from == to {
            return;
        }
        for done in [true, false] {
            let source = self.page(from);
            let list = if done {
                &mut source.done
            } else {
                &mut source.undone
            };
            let Some(index) = list
                .iter()
                .position(|s| matches!(s, PageStep::State(s) if **s == *step))
            else {
                continue;
            };
            let moved = list.remove(index);
            let target = self.page(to);
            if done {
                target.done.push(moved);
            } else {
                target.undone.push(moved);
            }
        }
    }

    /// `forget`: an item removed from every step, so redo can't bring back what was
    /// permanently deleted.
    pub fn forget(&mut self, collection: &str, id: &str) {
        for history in self.pages.values_mut() {
            for list in [&mut history.done, &mut history.undone] {
                list.retain_mut(|step| match step {
                    PageStep::State(step) => !forget_from_step(step, collection, id),
                    _ => true,
                });
            }
        }
    }

    /// Puts `next` where `step` is, in whichever history holds it: applying a step keeps the
    /// text typed into an item it removes, for redo (ProseCraft changes the step in place).
    pub fn replace_state(&mut self, step: &StateStep, next: StateStep) {
        for history in self.pages.values_mut() {
            for list in [&mut history.done, &mut history.undone] {
                for entry in list.iter_mut() {
                    if let PageStep::State(s) = entry
                        && **s == *step
                    {
                        **s = next;
                        return;
                    }
                }
            }
        }
    }

    fn page(&mut self, id: &str) -> &mut PageHistory {
        self.pages.entry(id.to_owned()).or_default()
    }

    fn drop_entries(&mut self, key: &str, entries: &[FieldEntry<S>]) {
        for entry in entries {
            let Some(page) = &entry.page else {
                continue;
            };
            let history = self.page(page);
            take_field_step(&mut history.done, key, entry.id);
            take_field_step(&mut history.undone, key, entry.id);
        }
    }
}

/// `encodeBodyUndoValue`: the editor's value in its history, the scene's footnotes kept with
/// its HTML (as a comment after it) so undo brings back a footnote's text with its marker.
pub fn encode_body(html: &str, document: &str, notes: &[(String, String)]) -> String {
    if notes.is_empty() {
        return html.to_owned();
    }
    let footnotes = notes
        .iter()
        .map(|(id, text)| {
            let mut note = JsObject::new();
            note.insert("id", JsValue::str(id));
            note.insert("html", JsValue::str(text));
            JsValue::Object(note)
        })
        .collect();
    let mut entry = JsObject::new();
    entry.insert("docId", JsValue::str(document));
    entry.insert("footnotes", JsValue::Array(footnotes));
    let json = crate::js::stringify(&JsValue::Array(vec![JsValue::Object(entry)]));
    format!("{html}\n<!--prosecraft-footnotes:{json}-->")
}

/// A scene's footnotes in an editor value: the scene's id, and each footnote's id and HTML.
pub type ScopeNotes = (String, Vec<(String, String)>);

/// `decodeBodyUndoValue`: the HTML, and each scene's footnotes.
pub fn decode_body(value: &str) -> (String, Vec<ScopeNotes>) {
    const MARK: &str = "\n<!--prosecraft-footnotes:";
    let Some(at) = value.rfind(MARK).filter(|_| value.ends_with("-->")) else {
        return (value.to_owned(), Vec::new());
    };
    let json = &value[at + MARK.len()..value.len() - 3];
    let Ok(JsValue::Array(entries)) = crate::js::parse(json) else {
        return (value.to_owned(), Vec::new());
    };
    let text = |v: Option<&JsValue>| {
        v.and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    let scopes = entries
        .iter()
        .map(|entry| {
            let notes = entry
                .get("footnotes")
                .and_then(JsValue::as_array)
                .unwrap_or_default()
                .iter()
                .map(|n| (text(n.get("id")), text(n.get("html"))))
                .collect();
            (text(entry.get("docId")), notes)
        })
        .collect();
    (value[..at].to_owned(), scopes)
}

/// `isItemArray`: an array of objects that each have a string `id`.
fn item_array(value: Option<&JsValue>) -> Option<Vec<(String, &JsObject)>> {
    value?
        .as_array()?
        .iter()
        .map(|item| {
            let object = item.as_object()?;
            let id = object.get("id")?.as_js_string()?.to_string_lossy();
            Some((id, object))
        })
        .collect()
}

fn same(left: Option<&JsValue>, right: Option<&JsValue>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(l), Some(r)) => crate::js::stringify(l) == crate::js::stringify(r),
        _ => false,
    }
}

/// `changedProps`.
fn changed_props(
    before: &JsObject,
    after: &JsObject,
    ignored: &[&str],
) -> Option<(JsObject, JsObject)> {
    let mut props: Vec<String> = before.iter().map(|(k, _)| k.to_string_lossy()).collect();
    for (key, _) in after.iter() {
        let key = key.to_string_lossy();
        if !props.contains(&key) {
            props.push(key);
        }
    }
    let mut b = JsObject::new();
    let mut a = JsObject::new();
    let mut changed = false;
    for prop in props {
        if ignored.contains(&prop.as_str()) || same(before.get(&prop), after.get(&prop)) {
            continue;
        }
        // An absent prop is `undefined`, which JSON leaves out.
        if let Some(value) = before.get(&prop) {
            b.insert(prop.as_str(), value.clone());
        }
        if let Some(value) = after.get(&prop) {
            a.insert(prop.as_str(), value.clone());
        }
        changed = true;
    }
    changed.then_some((b, a))
}

fn diff_array(
    before: &[(String, &JsObject)],
    after: &[(String, &JsObject)],
    ignored: &[&str],
) -> Option<StateChange> {
    // A `Map` keeps the first place of a repeated id and its last value.
    fn by_id<'a>(list: &[(String, &'a JsObject)]) -> Vec<(String, &'a JsObject)> {
        let mut out: Vec<(String, &JsObject)> = Vec::new();
        for (id, item) in list {
            match out.iter_mut().find(|(i, _)| i == id) {
                Some(slot) => slot.1 = item,
                None => out.push((id.clone(), item)),
            }
        }
        out
    }
    let before = by_id(before);
    let after = by_id(after);
    let mut items = Vec::new();
    for (id, item) in &before {
        match after.iter().find(|(i, _)| i == id) {
            None => items.push(ItemChange {
                id: id.clone(),
                before: Some((*item).clone()),
                after: None,
                whole: true,
            }),
            Some((_, next)) => {
                if let Some((b, a)) = changed_props(item, next, ignored) {
                    items.push(ItemChange {
                        id: id.clone(),
                        before: Some(b),
                        after: Some(a),
                        whole: false,
                    });
                }
            }
        }
    }
    for (id, item) in &after {
        if !before.iter().any(|(i, _)| i == id) {
            items.push(ItemChange {
                id: id.clone(),
                before: None,
                after: Some((*item).clone()),
                whole: true,
            });
        }
    }
    let before_ids: Vec<String> = before.into_iter().map(|(i, _)| i).collect();
    let after_ids: Vec<String> = after.into_iter().map(|(i, _)| i).collect();
    if items.is_empty() && before_ids == after_ids {
        return None;
    }
    Some(StateChange::Array {
        before_ids,
        after_ids,
        items,
    })
}

/// `diffState`: the difference between two state trees, or `None` when they match apart from
/// the ignored props (by collection).
pub fn diff_state(
    before: &JsObject,
    after: &JsObject,
    ignored: &[(&str, &[&str])],
    time: f64,
) -> Option<StateStep> {
    let mut changes = Vec::new();
    let mut signature = Vec::new();
    for (name, right) in after.iter() {
        let name = name.to_string_lossy();
        let left = before.get(&name);
        let skip = ignored
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(&[][..], |(_, s)| *s);
        if let (Some(l), Some(r)) = (item_array(left), item_array(Some(right))) {
            let Some(change) = diff_array(&l, &r, skip) else {
                continue;
            };
            if let StateChange::Array {
                before_ids,
                after_ids,
                items,
            } = &change
            {
                let props: Vec<String> = items
                    .iter()
                    .map(|item| {
                        let changed = if item.whole {
                            "*".to_owned()
                        } else {
                            item.before
                                .as_ref()
                                .map(|b| {
                                    b.iter()
                                        .map(|(k, _)| k.to_string_lossy())
                                        .collect::<Vec<_>>()
                                        .join(",")
                                })
                                .unwrap_or_default()
                        };
                        format!("{}:{changed}", item.id)
                    })
                    .collect();
                let order = if before_ids == after_ids {
                    ""
                } else {
                    "#order"
                };
                signature.push(format!("{name}[{}]{order}", props.join("|")));
            }
            changes.push((name, change));
        } else if let (Some(JsValue::Object(l)), JsValue::Object(r)) = (left, right) {
            let Some((b, a)) = changed_props(l, r, skip) else {
                continue;
            };
            let keys: Vec<String> = b.iter().map(|(k, _)| k.to_string_lossy()).collect();
            signature.push(format!("{name}{{{}}}", keys.join(",")));
            changes.push((
                name,
                StateChange::Object {
                    before: b,
                    after: a,
                },
            ));
        }
    }
    (!signature.is_empty()).then(|| StateStep {
        changes,
        time,
        signature: signature.join(";"),
    })
}

fn merge_into(base: &JsObject, patch: &JsObject) -> JsObject {
    let mut out = base.clone();
    for (key, value) in patch.iter() {
        out.insert(key.clone(), value.clone());
    }
    out
}

/// `applyStateStep`: the step applied to the current state, undone or redone. Items that no
/// longer exist are skipped. `step` is updated with text typed into an item since it was added,
/// so redo brings that back too.
pub fn apply_state_step(current: &JsObject, step: &mut StateStep, undo: bool) -> JsObject {
    let mut next = current.clone();
    for (name, change) in step.changes.iter_mut() {
        match change {
            StateChange::Object { before, after } => {
                let base = current
                    .get(name)
                    .and_then(JsValue::as_object)
                    .cloned()
                    .unwrap_or_default();
                let patch = if undo { &*before } else { &*after };
                next.insert(name.as_str(), JsValue::Object(merge_into(&base, patch)));
            }
            StateChange::Array {
                before_ids,
                after_ids,
                items,
            } => {
                let Some(list) = item_array(current.get(name)) else {
                    continue;
                };
                let mut by_id: Vec<(String, JsObject)> = Vec::new();
                for (id, item) in list {
                    match by_id.iter_mut().find(|(i, _)| *i == id) {
                        Some(slot) => slot.1 = item.clone(),
                        None => by_id.push((id, item.clone())),
                    }
                }
                for entry in items.iter_mut() {
                    let target = if undo {
                        entry.before.clone()
                    } else {
                        entry.after.clone()
                    };
                    let existing = by_id.iter().position(|(i, _)| *i == entry.id);
                    match (entry.whole, target, existing) {
                        (true, Some(target), Some(i)) => by_id[i].1 = target,
                        (true, Some(target), None) => by_id.push((entry.id.clone(), target)),
                        (true, None, Some(i)) => {
                            let (_, kept) = by_id.remove(i);
                            if undo {
                                entry.after = Some(kept);
                            } else {
                                entry.before = Some(kept);
                            }
                        }
                        (false, Some(target), Some(i)) => {
                            by_id[i].1 = merge_into(&by_id[i].1, &target);
                        }
                        _ => {}
                    }
                }
                let target_ids = if undo { &*before_ids } else { &*after_ids };
                let mut placed: Vec<JsValue> = target_ids
                    .iter()
                    .filter_map(|id| by_id.iter().find(|(i, _)| i == id))
                    .map(|(_, item)| JsValue::Object(item.clone()))
                    .collect();
                placed.extend(
                    by_id
                        .iter()
                        .filter(|(i, _)| !target_ids.contains(i))
                        .map(|(_, item)| JsValue::Object(item.clone())),
                );
                next.insert(name.as_str(), JsValue::Array(placed));
            }
        }
    }
    next
}

/// `mergeSteps`: the second change, from where the first started.
fn merge_steps(first: &StateStep, second: &StateStep) -> StateStep {
    let changes = second
        .changes
        .iter()
        .map(|(name, change)| {
            let merged = match (change, first.change(name)) {
                (StateChange::Object { after, .. }, Some(StateChange::Object { before, .. })) => {
                    StateChange::Object {
                        before: before.clone(),
                        after: after.clone(),
                    }
                }
                (
                    StateChange::Array {
                        after_ids, items, ..
                    },
                    Some(StateChange::Array {
                        before_ids,
                        items: earlier,
                        ..
                    }),
                ) => StateChange::Array {
                    before_ids: before_ids.clone(),
                    after_ids: after_ids.clone(),
                    items: items
                        .iter()
                        .map(|item| ItemChange {
                            before: earlier
                                .iter()
                                .find(|e| e.id == item.id)
                                .and_then(|e| e.before.clone())
                                .or_else(|| item.before.clone()),
                            ..item.clone()
                        })
                        .collect(),
                },
                (change, _) => change.clone(),
            };
            (name.clone(), merged)
        })
        .collect();
    StateStep {
        changes,
        time: second.time,
        signature: second.signature.clone(),
    }
}

/// `forgetFromStep`: whether the step is left empty.
fn forget_from_step(step: &mut StateStep, collection: &str, id: &str) -> bool {
    let Some(index) = step.changes.iter().position(|(n, _)| n == collection) else {
        return false;
    };
    let StateChange::Array {
        before_ids,
        after_ids,
        items,
    } = &mut step.changes[index].1
    else {
        return false;
    };
    items.retain(|item| item.id != id);
    before_ids.retain(|i| i != id);
    after_ids.retain(|i| i != id);
    if !items.is_empty() || before_ids != after_ids {
        return false;
    }
    step.changes.remove(index);
    step.changes.is_empty()
}

/// `removedIds`: ids in `before` and missing from `after`, by collection.
pub fn removed_ids(before: &JsObject, after: &JsObject) -> Vec<(String, String)> {
    let mut removed = Vec::new();
    for (collection, list) in before.iter() {
        let collection = collection.to_string_lossy();
        let (Some(list), Some(next)) = (item_array(Some(list)), item_array(after.get(&collection)))
        else {
            continue;
        };
        for (id, _) in list {
            if !next.iter().any(|(i, _)| *i == id) {
                removed.push((collection.clone(), id));
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::{parse, stringify};

    fn edit(kind: EditKind, time: f64, caret_moved: bool) -> Edit<()> {
        Edit {
            kind,
            time,
            caret_moved,
            before: (),
            after: (),
        }
    }
    use EditKind::{Delete, Newline, Type};

    fn field(value: &str) -> UndoManager<()> {
        let mut undo = UndoManager::default();
        undo.sync("a", value);
        undo
    }

    fn tree(json: &str) -> JsObject {
        parse(json)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    }

    fn json(object: &JsObject) -> String {
        stringify(&JsValue::Object(object.clone()))
    }

    fn value(result: Option<FieldResult<()>>) -> Option<String> {
        result.map(|r| r.value)
    }

    #[test]
    fn diff_text_describes_the_change() {
        for (before, after) in [
            ("abc", "abXc"),
            ("abc", "ac"),
            ("", "x"),
            ("x", ""),
            ("aaa", "aaaa"),
            ("same", "same"),
            ("é", "è"),
            ("a🌊b", "a🌍b"),
        ] {
            let diff = diff_text(before, after);
            assert_eq!(reapply(before, &diff), after, "{before} -> {after}");
            assert_eq!(revert(after, &diff), before, "{before} <- {after}");
        }
    }

    #[test]
    fn joins_typing_within_two_seconds() {
        let mut undo = field("");
        undo.record("a", Some("p"), "o", edit(Type, 0.0, false));
        undo.record("a", Some("p"), "on", edit(Type, 1999.0, false));
        assert_eq!(value(undo.undo_field("a")).as_deref(), Some(""));
    }

    #[test]
    fn splits_typing_at_two_seconds_from_the_start_of_the_group() {
        let mut undo = field("");
        undo.record("a", Some("p"), "o", edit(Type, 0.0, false));
        undo.record("a", Some("p"), "on", edit(Type, 1000.0, false));
        undo.record("a", Some("p"), "one", edit(Type, 2000.0, false));
        assert_eq!(value(undo.undo_field("a")).as_deref(), Some("on"));
        assert_eq!(value(undo.undo_field("a")).as_deref(), Some(""));
    }

    #[test]
    fn splits_between_typing_and_deleting_around_newlines_and_after_a_caret_move() {
        let mut undo = field("");
        undo.record("a", Some("p"), "xx", edit(Type, 0.0, false));
        undo.record("a", Some("p"), "x", edit(Delete, 1.0, false));
        undo.record("a", Some("p"), "x\n", edit(Newline, 2.0, false));
        undo.record("a", Some("p"), "x\ny", edit(Type, 3.0, false));
        undo.record("a", Some("p"), "zx\ny", edit(Type, 4.0, true));
        let values: Vec<Option<String>> = (0..5).map(|_| value(undo.undo_field("a"))).collect();
        let want = ["x\ny", "x\n", "x", "xx", ""].map(|s| Some(s.to_owned()));
        assert_eq!(values, want);
    }

    #[test]
    fn does_not_join_an_edit_that_follows_an_undo() {
        let mut undo = field("");
        undo.record("a", Some("p"), "ab", edit(Type, 0.0, false));
        undo.record("a", Some("p"), "ab c", edit(Newline, 1.0, false));
        undo.undo_field("a");
        undo.record("a", Some("p"), "abd", edit(Type, 2.0, false));
        assert_eq!(value(undo.undo_field("a")).as_deref(), Some("ab"));
    }

    #[test]
    fn clears_redo_after_a_new_edit() {
        let mut undo = field("");
        undo.record("a", Some("p"), "a", edit(Type, 0.0, false));
        undo.undo_field("a");
        undo.record("a", Some("p"), "b", edit(Type, 1.0, false));
        assert!(undo.redo_field("a").is_none());
    }

    #[test]
    fn discards_history_when_the_text_changed_elsewhere() {
        let mut undo = field("");
        undo.record("a", Some("p"), "a", edit(Type, 0.0, false));
        undo.sync("a", "from sync");
        assert!(undo.undo_field("a").is_none());
        assert!(undo.undo_page("p", |_| true).is_none());
    }

    #[test]
    fn only_undoes_the_requested_field() {
        let mut undo = UndoManager::default();
        undo.sync("a", "");
        undo.sync("b", "");
        undo.record("a", Some("p"), "a", edit(Type, 0.0, false));
        undo.record("b", Some("p"), "b", edit(Type, 1.0, false));
        assert_eq!(
            undo.undo_field("a").map(|r| (r.key, r.value)),
            Some(("a".into(), String::new()))
        );
        assert!(undo.undo_field("a").is_none());
        assert_eq!(
            undo.undo_field("b").map(|r| (r.key, r.value)),
            Some(("b".into(), String::new()))
        );
    }

    fn board(x: u32, note: &str) -> JsObject {
        tree(&format!(
            r#"{{"cards":[{{"id":"c","x":{x},"note":"{note}"}}]}}"#
        ))
    }

    fn step(before: &JsObject, after: &JsObject, time: f64) -> StateStep {
        diff_state(before, after, &[], time).unwrap_or_else(|| panic!("no change"))
    }

    #[test]
    fn page_history_undoes_across_fields_and_state_in_order_then_redoes() {
        let mut undo = UndoManager::default();
        undo.sync("a", "");
        undo.record("a", Some("p"), "text", edit(Type, 0.0, false));
        undo.push_state("p", step(&board(0, ""), &board(5, ""), 1.0));
        assert!(matches!(
            undo.undo_page("p", |_| true),
            Some(PageResult::State(_))
        ));
        assert!(
            matches!(undo.undo_page("p", |_| true), Some(PageResult::Field(r)) if r.value.is_empty())
        );
        assert!(undo.undo_page("p", |_| true).is_none());
        assert!(
            matches!(undo.redo_page("p", |_| true), Some(PageResult::Field(r)) if r.value == "text")
        );
        assert!(matches!(
            undo.redo_page("p", |_| true),
            Some(PageResult::State(_))
        ));
    }

    #[test]
    fn keeps_page_and_field_in_step_when_the_field_undoes_alone() {
        let mut undo = UndoManager::default();
        undo.sync("a", "");
        undo.record("a", Some("p"), "x", edit(Type, 0.0, false));
        undo.undo_field("a");
        assert!(undo.undo_page("p", |_| true).is_none());
        assert!(
            matches!(undo.redo_page("p", |_| true), Some(PageResult::Field(r)) if r.value == "x")
        );
    }

    #[test]
    fn keeps_separate_pages_apart() {
        let mut undo: UndoManager<()> = UndoManager::default();
        undo.push_state("p", step(&board(0, ""), &board(1, ""), 0.0));
        assert!(undo.undo_page("q", |_| true).is_none());
        assert!(undo.undo_page("p", |_| true).is_some());
    }

    #[test]
    fn merges_repeated_changes_to_the_same_props_made_close_together() {
        let mut undo: UndoManager<()> = UndoManager::default();
        undo.push_state("p", step(&board(0, ""), &board(1, ""), 0.0));
        undo.push_state("p", step(&board(1, ""), &board(2, ""), 100.0));
        let Some(PageResult::State(mut result)) = undo.undo_page("p", |_| true) else {
            panic!("expected a state step");
        };
        assert_eq!(
            json(&apply_state_step(&board(2, ""), &mut result, true)),
            json(&board(0, ""))
        );
        assert!(undo.undo_page("p", |_| true).is_none());
    }

    #[test]
    fn does_not_merge_changes_further_apart() {
        let mut undo: UndoManager<()> = UndoManager::default();
        undo.push_state("p", step(&board(0, ""), &board(1, ""), 0.0));
        undo.push_state("p", step(&board(1, ""), &board(2, ""), 5000.0));
        undo.undo_page("p", |_| true);
        assert!(undo.undo_page("p", |_| true).is_some());
    }

    #[test]
    fn discards_a_field_step_that_cannot_be_applied() {
        let mut undo = UndoManager::default();
        undo.sync("a", "");
        undo.record("a", Some("p"), "x", edit(Type, 0.0, false));
        assert!(undo.undo_page("p", |_| false).is_none());
    }

    fn page_with_field_then_external() -> UndoManager<()> {
        let mut undo = UndoManager::default();
        undo.sync("title", "");
        undo.record("title", Some("p"), "t", edit(Type, 0.0, false));
        undo.record_external("p", "md");
        undo
    }

    fn is_field(result: Option<PageResult<()>>, key: &str) -> bool {
        matches!(result, Some(PageResult::Field(r)) if r.key == key)
    }

    fn is_external(result: Option<PageResult<()>>, key: &str) -> bool {
        matches!(result, Some(PageResult::External { key: k }) if k == key)
    }

    #[test]
    fn external_steps_keep_their_position_among_field_steps() {
        let mut undo = page_with_field_then_external();
        assert!(is_external(undo.undo_page("p", |_| true), "md"));
        undo.external_undone("p", "md");
        assert!(is_field(undo.undo_page("p", |_| true), "title"));
    }

    #[test]
    fn external_steps_come_after_a_field_step_recorded_later() {
        let mut undo = UndoManager::default();
        undo.record_external("p", "md");
        undo.sync("title", "");
        undo.record("title", Some("p"), "t", edit(Type, 0.0, false));
        assert!(is_field(undo.undo_page("p", |_| true), "title"));
        assert!(is_external(undo.undo_page("p", |_| true), "md"));
    }

    #[test]
    fn external_steps_stay_in_step_when_the_editor_undoes_by_itself() {
        let mut undo = page_with_field_then_external();
        undo.external_undone("p", "md");
        assert!(is_field(undo.undo_page("p", |_| true), "title"));
        assert!(is_field(undo.redo_page("p", |_| true), "title"));
        assert!(is_external(undo.redo_page("p", |_| true), "md"));
        undo.external_redone("p", "md");
        assert!(undo.redo_page("p", |_| true).is_none());
    }

    #[test]
    fn external_steps_are_skipped_for_an_editor_that_is_not_available() {
        let mut undo = page_with_field_then_external();
        assert!(is_field(undo.undo_page("p", |k| k != "md"), "title"));
    }

    #[test]
    fn external_steps_the_editor_could_not_apply_are_discarded() {
        let mut undo = page_with_field_then_external();
        undo.undo_page("p", |_| true);
        undo.discard_newest("p", true);
        assert!(is_field(undo.undo_page("p", |_| true), "title"));
    }

    #[test]
    fn external_steps_are_forgotten_when_the_editor_history_is_cleared() {
        let mut undo: UndoManager<()> = UndoManager::default();
        undo.record_external("p", "md");
        undo.record_external("p", "md");
        undo.forget_external("p", "md");
        assert!(undo.undo_page("p", |_| true).is_none());
    }

    #[test]
    fn state_steps_restore_only_changed_props_leaving_newer_text_alone() {
        let mut s = step(
            &tree(r#"{"cards":[{"id":"c","x":0,"note":"old"}]}"#),
            &tree(r#"{"cards":[{"id":"c","x":9,"note":"old"}]}"#),
            0.0,
        );
        let undone = apply_state_step(
            &tree(r#"{"cards":[{"id":"c","x":9,"note":"new"}]}"#),
            &mut s,
            true,
        );
        assert_eq!(
            json(&undone),
            r#"{"cards":[{"id":"c","x":0,"note":"new"}]}"#
        );
    }

    #[test]
    fn state_steps_are_none_when_only_ignored_props_changed() {
        assert!(
            diff_state(
                &tree(r#"{"docs":[{"id":"d","content":"a"}]}"#),
                &tree(r#"{"docs":[{"id":"d","content":"b"}]}"#),
                &[("docs", &["content"])],
                0.0
            )
            .is_none()
        );
    }

    #[test]
    fn state_steps_restore_a_removed_item_and_its_position() {
        let before = tree(r#"{"nodes":[{"id":"a"},{"id":"b"},{"id":"c"}]}"#);
        let after = tree(r#"{"nodes":[{"id":"a"},{"id":"c"}]}"#);
        let mut s = step(&before, &after, 0.0);
        assert_eq!(json(&apply_state_step(&after, &mut s, true)), json(&before));
    }

    #[test]
    fn state_steps_restore_order() {
        let before = tree(r#"{"nodes":[{"id":"a"},{"id":"b"}]}"#);
        let after = tree(r#"{"nodes":[{"id":"b"},{"id":"a"}]}"#);
        let mut s = step(&before, &after, 0.0);
        assert_eq!(json(&apply_state_step(&after, &mut s, true)), json(&before));
        assert_eq!(
            json(&apply_state_step(&before, &mut s, false)),
            json(&after)
        );
    }

    #[test]
    fn state_steps_keep_text_added_to_an_item_when_undo_removes_it_and_redo_restores_it() {
        let mut s = step(
            &tree(r#"{"nodes":[]}"#),
            &tree(r#"{"nodes":[{"id":"a","text":""}]}"#),
            0.0,
        );
        let removed = apply_state_step(
            &tree(r#"{"nodes":[{"id":"a","text":"typed"}]}"#),
            &mut s,
            true,
        );
        assert_eq!(json(&removed), r#"{"nodes":[]}"#);
        assert_eq!(
            json(&apply_state_step(&removed, &mut s, false)),
            r#"{"nodes":[{"id":"a","text":"typed"}]}"#
        );
    }

    #[test]
    fn state_steps_skip_items_that_no_longer_exist() {
        let mut s = step(
            &tree(r#"{"nodes":[{"id":"a","x":0}]}"#),
            &tree(r#"{"nodes":[{"id":"a","x":1}]}"#),
            0.0,
        );
        assert_eq!(
            json(&apply_state_step(&tree(r#"{"nodes":[]}"#), &mut s, true)),
            r#"{"nodes":[]}"#
        );
    }

    #[test]
    fn state_steps_patch_plain_objects_by_top_level_prop() {
        let mut s = step(
            &tree(r#"{"options":{"a":1,"b":1}}"#),
            &tree(r#"{"options":{"a":2,"b":1}}"#),
            0.0,
        );
        assert_eq!(
            json(&apply_state_step(
                &tree(r#"{"options":{"a":2,"b":5}}"#),
                &mut s,
                true
            )),
            r#"{"options":{"a":1,"b":5}}"#
        );
    }

    #[test]
    fn forgets_a_permanently_deleted_item_so_redo_cannot_restore_it() {
        let mut undo: UndoManager<()> = UndoManager::default();
        undo.push_state(
            "p",
            step(
                &tree(r#"{"docs":[]}"#),
                &tree(r#"{"docs":[{"id":"d"}]}"#),
                0.0,
            ),
        );
        undo.forget("docs", "d");
        assert!(undo.undo_page("p", |_| true).is_none());
    }

    #[test]
    fn body_values_keep_the_footnotes() {
        let notes = vec![("n1".to_owned(), "A <em>note</em>.".to_owned())];
        let value = encode_body("<p>x</p>", "d", &notes);
        assert_eq!(
            value,
            r#"<p>x</p>
<!--prosecraft-footnotes:[{"docId":"d","footnotes":[{"id":"n1","html":"A <em>note</em>."}]}]-->"#
        );
        assert_eq!(
            decode_body(&value),
            ("<p>x</p>".to_owned(), vec![("d".to_owned(), notes)])
        );
        assert_eq!(encode_body("<p>x</p>", "d", &[]), "<p>x</p>");
        assert_eq!(decode_body("<p>x</p>"), ("<p>x</p>".to_owned(), Vec::new()));
    }

    #[test]
    fn lists_removed_ids() {
        assert_eq!(
            removed_ids(
                &tree(r#"{"docs":[{"id":"a"},{"id":"b"}]}"#),
                &tree(r#"{"docs":[{"id":"b"}]}"#)
            ),
            [("docs".to_owned(), "a".to_owned())]
        );
    }
}
