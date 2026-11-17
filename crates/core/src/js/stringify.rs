//! `JSON.stringify` (compact and with an indent of two) and ProseCraft's `stableStringify`.

use super::{JsString, JsValue, number_to_string};

/// `JSON.stringify(value)`.
pub fn stringify(value: &JsValue) -> String {
    let mut out = String::new();
    write_value(&mut out, value, None, 0);
    out
}

/// `JSON.stringify(value, null, 2)`, as ProseCraft writes its backups and sync files.
pub fn stringify_pretty(value: &JsValue) -> String {
    let mut out = String::new();
    write_value(&mut out, value, Some(2), 0);
    out
}

/// ProseCraft's `stableStringify` (`src/lib/storage/schema.ts`): compact JSON with every
/// object's keys sorted by UTF-16 code units, used for change detection and the sync hashes.
pub fn stable_stringify(value: &JsValue) -> String {
    let mut out = String::new();
    write_stable(&mut out, value);
    out
}

fn write_stable(out: &mut String, value: &JsValue) {
    match value {
        JsValue::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_stable(out, item);
            }
            out.push(']');
        }
        JsValue::Object(object) => {
            let mut entries: Vec<(&JsString, &JsValue)> = object.iter().collect();
            // `Array.prototype.sort` is stable and `<` on strings compares UTF-16 code units,
            // which is what `Ord` on `JsString` does.
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                write_stable(out, item);
            }
            out.push('}');
        }
        other => write_value(out, other, None, 0),
    }
}

fn newline(out: &mut String, indent: Option<usize>, level: usize) {
    if let Some(width) = indent {
        out.push('\n');
        out.push_str(&" ".repeat(width.saturating_mul(level)));
    }
}

fn write_value(out: &mut String, value: &JsValue, indent: Option<usize>, level: usize) {
    match value {
        JsValue::Null => out.push_str("null"),
        JsValue::Bool(true) => out.push_str("true"),
        JsValue::Bool(false) => out.push_str("false"),
        JsValue::Number(n) if n.is_finite() => out.push_str(&number_to_string(*n)),
        JsValue::Number(_) => out.push_str("null"),
        JsValue::String(text) => write_string(out, text),
        JsValue::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                newline(out, indent, level + 1);
                write_value(out, item, indent, level + 1);
            }
            newline(out, indent, level);
            out.push(']');
        }
        JsValue::Object(object) => {
            if object.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (index, (key, item)) in object.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                newline(out, indent, level + 1);
                write_string(out, key);
                out.push_str(if indent.is_some() { ": " } else { ":" });
                write_value(out, item, indent, level + 1);
            }
            newline(out, indent, level);
            out.push('}');
        }
    }
}

/// `QuoteJSONString`: the usual escapes, other control characters as `\u00xx`, and lone
/// surrogates as `\udxxx` (lowercase hex), so the output is always valid UTF-8.
fn write_string(out: &mut String, text: &JsString) {
    out.push('"');
    for item in char::decode_utf16(text.units().iter().copied()) {
        match item {
            Ok('"') => out.push_str("\\\""),
            Ok('\\') => out.push_str("\\\\"),
            Ok('\u{8}') => out.push_str("\\b"),
            Ok('\u{c}') => out.push_str("\\f"),
            Ok('\n') => out.push_str("\\n"),
            Ok('\r') => out.push_str("\\r"),
            Ok('\t') => out.push_str("\\t"),
            Ok(c) if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            Ok(c) => out.push(c),
            Err(lone) => out.push_str(&format!("\\u{:04x}", lone.unpaired_surrogate())),
        }
    }
    out.push('"');
}
