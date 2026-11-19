//! The bits of JavaScript semantics the TypeScript server relies on when it reads files it does
//! not control: property reads that throw on `null` and `undefined`, `String(value)`, `+ 1`,
//! `Math.max`, object spread. A JavaScript `undefined` is `None` here.
//!
//! The messages are V8's, so a malformed header fails with the text the TypeScript server shows.

use prosecraft_core::js::{JsObject, JsString, JsValue, number_to_string, string_to_number};

/// Longest a `String(value)` of a nested array may recurse before it gives up.
const MAX_TO_STRING_DEPTH: usize = 64;

/// The name V8 gives a base it cannot read a property of.
fn nullish_name(value: Option<&JsValue>) -> &'static str {
    match value {
        Some(JsValue::Null) => "null",
        _ => "undefined",
    }
}

/// `base.key`: `Ok(None)` when the property is missing or the base is a primitive without it,
/// V8's `TypeError` when the base is `null` or `undefined`.
pub fn prop<'a>(base: Option<&'a JsValue>, key: &str) -> Result<Option<&'a JsValue>, String> {
    match base {
        None | Some(JsValue::Null) => Err(format!(
            "Cannot read properties of {} (reading '{key}')",
            nullish_name(base)
        )),
        Some(value) => Ok(get(value, key)),
    }
}

/// `base?.key`.
pub fn opt<'a>(base: Option<&'a JsValue>, key: &str) -> Option<&'a JsValue> {
    base.and_then(|value| get(value, key))
}

/// An own property of an object, or an element of an array by its index key; nothing for the
/// other values (the keys this server reads are none of a string's or a number's).
pub fn get<'a>(value: &'a JsValue, key: &str) -> Option<&'a JsValue> {
    match value {
        JsValue::Object(object) => object.get(key),
        JsValue::Array(items) => array_index(key).and_then(|index| items.get(index)),
        _ => None,
    }
}

fn array_index(key: &str) -> Option<usize> {
    if key.is_empty() || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    if !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse().ok()
}

/// `a ?? b` for a property that may be `undefined` or `null`.
pub fn nullish(value: Option<&JsValue>) -> Option<&JsValue> {
    value.filter(|v| !v.is_null())
}

/// `!value`: whether a value is falsy.
pub fn falsy(value: Option<&JsValue>) -> bool {
    match value {
        None | Some(JsValue::Null) => true,
        Some(JsValue::Bool(b)) => !b,
        Some(JsValue::Number(n)) => *n == 0.0 || n.is_nan(),
        Some(JsValue::String(s)) => s.is_empty(),
        Some(_) => false,
    }
}

/// `typeof value === "string" ? value : undefined`.
pub fn as_str(value: Option<&JsValue>) -> Option<&JsString> {
    value.and_then(JsValue::as_js_string)
}

/// `value === text`.
pub fn is_str(value: Option<&JsValue>, text: &str) -> bool {
    as_str(value).is_some_and(|s| s.eq_str(text))
}

/// `value === true`.
pub fn is_true(value: Option<&JsValue>) -> bool {
    matches!(value, Some(JsValue::Bool(true)))
}

/// `value === null`.
pub fn is_null(value: Option<&JsValue>) -> bool {
    matches!(value, Some(JsValue::Null))
}

/// A plain object (`typeof value === "object" && value !== null && !Array.isArray(value)`).
pub fn is_plain_object(value: Option<&JsValue>) -> bool {
    matches!(value, Some(JsValue::Object(_)))
}

/// `typeof value === "object" && value !== null`: objects and arrays.
pub fn is_object_like(value: Option<&JsValue>) -> bool {
    matches!(value, Some(JsValue::Object(_) | JsValue::Array(_)))
}

pub fn string(text: &str) -> JsValue {
    JsValue::str(text)
}

pub fn units(units: &[u16]) -> JsValue {
    JsValue::String(JsString::from_units(units.to_vec()))
}

/// An object from key/value pairs, in this order.
pub fn object(entries: Vec<(&str, JsValue)>) -> JsValue {
    let mut out = JsObject::new();
    for (key, value) in entries {
        out.insert(key, value);
    }
    JsValue::Object(out)
}

/// `{ ...value }`: an object's own properties, an array's or a string's elements by index,
/// nothing for the other values.
pub fn spread(value: Option<&JsValue>) -> JsObject {
    let mut out = JsObject::new();
    match value {
        Some(JsValue::Object(object)) => {
            for (key, item) in object.iter() {
                out.insert(key.clone(), item.clone());
            }
        }
        Some(JsValue::Array(items)) => {
            for (index, item) in items.iter().enumerate() {
                out.insert(index.to_string(), item.clone());
            }
        }
        Some(JsValue::String(text)) => {
            for (index, unit) in text.units().iter().enumerate() {
                out.insert(index.to_string(), units(&[*unit]));
            }
        }
        _ => {}
    }
    out
}

/// `String(value)`, as a template literal writes it.
pub fn to_js_string(value: Option<&JsValue>) -> String {
    to_js_string_at(value, 0)
}

fn to_js_string_at(value: Option<&JsValue>, depth: usize) -> String {
    match value {
        None => "undefined".into(),
        Some(JsValue::Null) => "null".into(),
        Some(JsValue::Bool(b)) => b.to_string(),
        Some(JsValue::Number(n)) => number_to_string(*n),
        Some(JsValue::String(s)) => s.to_string_lossy(),
        Some(JsValue::Object(_)) => "[object Object]".into(),
        Some(JsValue::Array(items)) => {
            if depth >= MAX_TO_STRING_DEPTH {
                return String::new();
            }
            // `Array.prototype.join`: `null` and `undefined` elements are empty.
            items
                .iter()
                .map(|item| match item {
                    JsValue::Null => String::new(),
                    other => to_js_string_at(Some(other), depth + 1),
                })
                .collect::<Vec<_>>()
                .join(",")
        }
    }
}

/// `Number(value)` (`ToNumber`).
pub fn to_number(value: Option<&JsValue>) -> f64 {
    match value {
        None => f64::NAN,
        Some(JsValue::Null) => 0.0,
        Some(JsValue::Bool(b)) => f64::from(u8::from(*b)),
        Some(JsValue::Number(n)) => *n,
        Some(JsValue::String(s)) => string_to_number(&s.to_string_lossy()),
        Some(other) => string_to_number(&to_js_string(Some(other))),
    }
}

/// `value + 1`: a number for numbers, `null`, booleans and `undefined` (`NaN`), the text with
/// "1" appended for strings, objects and arrays.
pub fn add_one(value: Option<&JsValue>) -> JsValue {
    match value {
        Some(JsValue::String(_) | JsValue::Object(_) | JsValue::Array(_)) => {
            JsValue::str(&format!("{}1", to_js_string(value)))
        }
        other => JsValue::Number(to_number(other) + 1.0),
    }
}

/// `Math.max(a, b)` of a number and a value that may not be one: `NaN` wins.
pub fn math_max(a: f64, b: &JsValue) -> JsValue {
    let b = to_number(Some(b));
    if a.is_nan() || b.is_nan() {
        return JsValue::Number(f64::NAN);
    }
    // `Math.max(0, -0)` is 0; either way JSON writes "0".
    JsValue::Number(a.max(b))
}

/// `base.key = value` in strict mode: sets a property of an object, does nothing visible on an
/// array, and throws V8's `TypeError` for the other values.
pub fn set_prop(base: Option<&mut JsValue>, key: &str, value: JsValue) -> Result<(), String> {
    match base {
        Some(JsValue::Object(object)) => {
            object.insert(key, value);
            Ok(())
        }
        // A named property on an array is dropped by `JSON.stringify`.
        Some(JsValue::Array(_)) => Ok(()),
        None => Err(format!(
            "Cannot set properties of undefined (setting '{key}')"
        )),
        Some(JsValue::Null) => Err(format!("Cannot set properties of null (setting '{key}')")),
        Some(other) => {
            let kind = match other {
                JsValue::Bool(_) => "boolean",
                JsValue::Number(_) => "number",
                _ => "string",
            };
            Err(format!(
                "Cannot create property '{key}' on {kind} '{}'",
                to_js_string(Some(other))
            ))
        }
    }
}

/// A mutable property of an object, `None` when it is missing or the base is not an object.
pub fn get_mut<'a>(base: &'a mut JsValue, key: &str) -> Option<&'a mut JsValue> {
    match base {
        JsValue::Object(object) => object.get_mut(key),
        JsValue::Array(items) => array_index(key).and_then(|index| items.get_mut(index)),
        _ => None,
    }
}

/// `Object.entries(value)` for the values a header map can be: an object's own properties, an
/// array's elements and a string's characters by index, nothing for the other values.
pub fn entries(value: Option<&JsValue>) -> Vec<(String, JsValue)> {
    match value {
        Some(JsValue::Object(object)) => object
            .iter()
            .map(|(key, item)| (key.to_string_lossy(), item.clone()))
            .collect(),
        Some(JsValue::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(index, item)| (index.to_string(), item.clone()))
            .collect(),
        Some(JsValue::String(text)) => text
            .units()
            .iter()
            .enumerate()
            .map(|(index, unit)| (index.to_string(), units(&[*unit])))
            .collect(),
        _ => Vec::new(),
    }
}

/// `list.indexOf(id)` for an `order` list: the position in an array, `-1` when absent.
pub fn index_of(list: &[JsValue], id: &str) -> i64 {
    list.iter()
        .position(|item| is_str(Some(item), id))
        .and_then(|i| i64::try_from(i).ok())
        .unwrap_or(-1)
}

/// The UTF-16 units of a string value.
pub fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `text.indexOf(needle, from)` over UTF-16 units.
pub fn index_of_units(text: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    let from = from.min(text.len());
    if needle.is_empty() {
        return Some(from);
    }
    if needle.len() > text.len() {
        return None;
    }
    (from..=text.len() - needle.len()).find(|&i| text.get(i..i + needle.len()) == Some(needle))
}

/// `text.slice(start, end)` over UTF-16 units, with the bounds clamped.
pub fn slice_units(text: &[u16], start: usize, end: usize) -> &[u16] {
    let end = end.min(text.len());
    let start = start.min(end);
    text.get(start..end).unwrap_or_default()
}

/// `text.trim()`: JavaScript's whitespace and line terminators off both ends.
pub fn trim_units(text: &[u16]) -> &[u16] {
    let is_space = |unit: &u16| {
        char::from_u32(u32::from(*unit)).is_some_and(prosecraft_core::html::is_js_whitespace)
    };
    let start = text.iter().position(|u| !is_space(u)).unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|u| !is_space(u))
        .map_or(start, |i| i + 1);
    text.get(start..end).unwrap_or_default()
}

/// `text.trim()` for a Rust string.
pub fn trim(text: &str) -> &str {
    text.trim_matches(prosecraft_core::html::is_js_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_reads_throw_like_v8() {
        assert_eq!(
            prop(None, "documents"),
            Err("Cannot read properties of undefined (reading 'documents')".into())
        );
        assert_eq!(
            prop(Some(&JsValue::Null), "deletedAt"),
            Err("Cannot read properties of null (reading 'deletedAt')".into())
        );
        assert_eq!(prop(Some(&JsValue::Number(5.0)), "x"), Ok(None));
    }

    #[test]
    fn arithmetic_follows_javascript() {
        assert_eq!(add_one(Some(&JsValue::Number(4.0))), JsValue::Number(5.0));
        assert_eq!(add_one(Some(&JsValue::str("5"))), JsValue::str("51"));
        assert_eq!(add_one(Some(&JsValue::Null)), JsValue::Number(1.0));
        assert!(matches!(add_one(None), JsValue::Number(n) if n.is_nan()));
        assert_eq!(math_max(10.0, &JsValue::str("51")), JsValue::Number(51.0));
        assert!(matches!(math_max(10.0, &JsValue::str("x")), JsValue::Number(n) if n.is_nan()));
    }

    #[test]
    fn assignments_on_primitives_throw_like_v8() {
        let mut five = JsValue::Number(5.0);
        assert_eq!(
            set_prop(Some(&mut five), "title", JsValue::Null),
            Err("Cannot create property 'title' on number '5'".into())
        );
        let mut text = JsValue::str("ab");
        assert_eq!(
            set_prop(Some(&mut text), "title", JsValue::Null),
            Err("Cannot create property 'title' on string 'ab'".into())
        );
        assert_eq!(
            set_prop(None, "title", JsValue::Null),
            Err("Cannot set properties of undefined (setting 'title')".into())
        );
    }

    #[test]
    fn utf16_helpers_count_units() {
        let text = utf16("a😀b");
        assert_eq!(text.len(), 4);
        assert_eq!(index_of_units(&text, &utf16("b"), 0), Some(3));
        assert_eq!(slice_units(&text, 1, 2).len(), 1);
        assert_eq!(trim_units(&utf16("\u{a0} x \n")), utf16("x").as_slice());
    }
}
