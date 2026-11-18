//! Wiki files and links against ProseCraft's `research.ts` (`fixtures/research.json`).

use prosecraft_core::js::{self, JsString, JsValue};
use prosecraft_core::research::{
    Item, create_folder_name, file_name_from_path, format_size, is_safe_storage_key, mime_type,
    normalize_folder_name, normalize_items, normalize_url, safe_file_name, title_from_url,
};

fn fixture() -> JsValue {
    let path = format!(
        "{}/tests/fixtures/research.json",
        env!("CARGO_MANIFEST_DIR")
    );
    js::parse(&std::fs::read_to_string(&path).expect("fixture")).expect("json")
}

fn cases<'a>(f: &'a JsValue, key: &str) -> &'a [JsValue] {
    f.get(key).and_then(JsValue::as_array).expect(key)
}

fn text(v: &JsValue, k: &str) -> Option<String> {
    v.get(k)
        .and_then(JsValue::as_js_string)
        .map(JsString::to_string_lossy)
}

#[test]
fn names_addresses_and_sizes_match_prosecraft() {
    let f = fixture();
    for c in cases(&f, "urls") {
        let raw = text(c, "raw").unwrap_or_default();
        assert_eq!(normalize_url(&raw), text(c, "normalized"), "url {raw:?}");
        assert_eq!(
            Some(title_from_url(&raw)),
            text(c, "title"),
            "title {raw:?}"
        );
    }
    for c in cases(&f, "names") {
        let raw = text(c, "raw").unwrap_or_default();
        let safe = safe_file_name(&raw, 1.0);
        let want = text(c, "safe").unwrap_or_default();
        if want.starts_with("research-file-") {
            assert_eq!(safe, "research-file-1", "{raw:?}");
        } else {
            assert_eq!(safe, want, "{raw:?}");
        }
        assert_eq!(Some(mime_type(&raw).to_owned()), text(c, "mime"), "{raw:?}");
    }
    for c in cases(&f, "paths") {
        let raw = text(c, "raw").unwrap_or_default();
        assert_eq!(
            Some(file_name_from_path(&raw, 1.0)),
            text(c, "name"),
            "{raw:?}"
        );
    }
    for c in cases(&f, "sizes") {
        let bytes = match c.get("bytes") {
            Some(JsValue::Number(n)) => *n,
            other => text(
                &JsValue::Object({
                    let mut o = js::JsObject::new();
                    o.insert("v", other.cloned().unwrap_or(JsValue::Null));
                    o
                }),
                "v",
            )
            .and_then(|s| s.parse().ok())
            .unwrap_or(f64::NAN),
        };
        assert_eq!(Some(format_size(bytes)), text(c, "text"), "{bytes}");
    }
    for c in cases(&f, "folders") {
        let (name, id) = (
            text(c, "name").unwrap_or_default(),
            text(c, "id").unwrap_or_default(),
        );
        assert_eq!(
            Some(create_folder_name(&name, &id)),
            text(c, "folder"),
            "{name:?}"
        );
    }
    for c in cases(&f, "folderValues") {
        assert_eq!(
            Some(normalize_folder_name(c.get("value"))),
            text(c, "normalized"),
            "{:?}",
            c.get("value")
        );
    }
    for c in cases(&f, "keys") {
        let key = text(c, "key").unwrap_or_default();
        let kept = c.get("items").and_then(JsValue::as_number) == Some(1.0);
        assert_eq!(is_safe_storage_key(&key), kept, "{key:?}");
    }
}

#[test]
fn stored_items_are_kept_as_prosecraft_keeps_them() {
    let f = fixture();
    let items = f.get("items").expect("items");
    let got = normalize_items(items.get("raw"), &["f".to_owned()]);
    let want = items
        .get("normalized")
        .and_then(JsValue::as_array)
        .expect("normalized");
    assert_eq!(got.len(), want.len());
    for ((item, folder, added), w) in got.iter().zip(want) {
        assert_eq!(*folder, text(w, "folderId"));
        assert_eq!(Some(*added), w.get("addedAt").and_then(JsValue::as_number));
        assert_eq!(Some(item.title().to_owned()), text(w, "title"));
        match item {
            Item::File {
                id,
                file_name,
                mime_type,
                size,
                storage_key,
                ..
            } => {
                assert_eq!(Some(id.clone()), text(w, "id"));
                assert_eq!(Some(file_name.clone()), text(w, "fileName"));
                assert_eq!(Some(mime_type.clone()), text(w, "mimeType"));
                assert_eq!(Some(*size), w.get("size").and_then(JsValue::as_number));
                assert_eq!(Some(storage_key.clone()), text(w, "storageKey"));
            }
            Item::Link {
                id,
                url,
                description,
                ..
            } => {
                assert_eq!(Some(id.clone()), text(w, "id"));
                assert_eq!(Some(url.clone()), text(w, "url"));
                assert_eq!(
                    description.clone(),
                    text(w, "description").unwrap_or_default()
                );
            }
        }
    }
}
