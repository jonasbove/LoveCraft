//! Encryption and backups against files ProseCraft itself wrote (see `fixtures/README.md`).

use prosecraft_core::js::{self, JsString, JsValue};
use prosecraft_core::storage::bundle::{
    BundleError, EncryptedBundle, PlainBundle, assemble_encrypted, reseal_with_nonces,
    write_plain_bundle,
};
use prosecraft_core::storage::crypto::{
    self, CryptoError, EntityEnvelope, EntityMeta, RandomSource, VaultHeader, VaultKey,
    VaultOptions,
};
use prosecraft_core::storage::schema::is_stored_project;

fn read(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn fixture(name: &str) -> JsValue {
    js::parse(&read(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn hex(value: Option<&JsValue>) -> Vec<u8> {
    let text = value
        .and_then(JsValue::as_js_string)
        .and_then(JsString::as_string)
        .unwrap_or_default();
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap_or_default())
        .collect()
}

fn js_string(value: Option<&JsValue>) -> JsString {
    value
        .and_then(JsValue::as_js_string)
        .cloned()
        .unwrap_or_default()
}

/// A counter, so tests that only need *some* randomness are reproducible.
struct Counter(u8);
impl RandomSource for Counter {
    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), CryptoError> {
        for byte in bytes {
            self.0 = self.0.wrapping_add(1);
            *byte = self.0;
        }
        Ok(())
    }
}

#[test]
fn derives_unlocks_and_seals_exactly_like_prosecraft() {
    let vaults = fixture("crypto.json");
    let vaults = vaults.as_array().unwrap_or_default();
    assert_eq!(vaults.len(), 3);
    for vault in vaults {
        let passphrase = js_string(vault.get("passphrase"));
        let header = vault
            .get("header")
            .and_then(VaultHeader::read)
            .expect("valid header");

        let kek = crypto::derive_kek(&passphrase, &header.kdf).expect("kek");
        assert_eq!(
            kek.as_slice(),
            hex(vault.get("kek")).as_slice(),
            "kek for {passphrase:?}"
        );

        let key = crypto::unlock_vault(&header, &passphrase).expect("unlock");
        assert_eq!(
            key.as_bytes().as_slice(),
            hex(vault.get("vaultKey")).as_slice()
        );

        let wrong = JsString::from("not the passphrase");
        assert_eq!(
            crypto::unlock_vault(&header, &wrong).err(),
            Some(CryptoError::IncorrectPassphrase)
        );

        // Decrypt ProseCraft's record, then seal the same payload with the same nonce: the
        // envelope must come out byte for byte the same, AAD and all.
        let stored = vault.get("envelope").expect("envelope");
        let envelope = EntityEnvelope::read(stored).expect("valid envelope");
        let payload = crypto::decrypt_entity(&key, &envelope).expect("decrypt");
        assert_eq!(Some(&payload), vault.get("payload"));
        let resealed = crypto::encrypt_entity_with_nonce(
            &key,
            envelope.meta.clone(),
            &payload,
            envelope.nonce,
        )
        .expect("seal");
        assert_eq!(js::stringify(&resealed.to_js()), js::stringify(stored));
    }
}

#[test]
fn tampered_metadata_fails_to_decrypt() {
    let vaults = fixture("crypto.json");
    let vault = vaults.as_array().and_then(|v| v.get(1)).expect("vault");
    let key = VaultKey::from_bytes(hex(vault.get("vaultKey")).try_into().expect("32 bytes"));
    let mut envelope =
        EntityEnvelope::read(vault.get("envelope").expect("envelope")).expect("valid");
    envelope.meta.revision += 1.0;
    assert_eq!(
        crypto::decrypt_entity(&key, &envelope).err(),
        Some(CryptoError::RecordNotDecrypted)
    );
}

#[test]
fn a_new_vault_round_trips() {
    let passphrase = JsString::from("ﬁnal pass");
    let options = VaultOptions {
        hint: Some("hint".into()),
        memory_kib: Some(64),
        iterations: Some(1),
        parallelism: Some(1),
    };
    let (header, key) =
        crypto::create_vault(&passphrase, &options, &mut Counter(0)).expect("vault");
    // What we write reads back through ProseCraft's checks, and unlocks with the NFKC twin.
    let reread = VaultHeader::read(&header.raw).expect("valid header");
    let unlocked = crypto::unlock_vault(&reread, &JsString::from("final pass")).expect("unlock");
    assert_eq!(unlocked.as_bytes(), key.as_bytes());
    let meta = EntityMeta {
        kind: "document".into(),
        id: JsString::from_units(vec![0x64, 0xd83d]),
        project_id: "p".into(),
        device_id: "d".into(),
        updated_at: 1.5,
        revision: 0.0,
    };
    let payload = js::parse(r#"{"lone":"\udfff","n":[1e21,-0]}"#).expect("payload");
    let envelope = crypto::encrypt_entity(&key, meta, &payload, &mut Counter(9)).expect("seal");
    let reread = EntityEnvelope::read(&envelope.to_js()).expect("valid envelope");
    let opened = crypto::decrypt_entity(&key, &reread).expect("open");
    assert_eq!(js::stringify(&opened), js::stringify(&payload));
    assert_eq!(
        crypto::create_vault(&JsString::default(), &options, &mut Counter(0)).err(),
        Some(CryptoError::PassphraseRequired)
    );
}

#[test]
fn plain_backup_reads_and_writes_byte_for_byte() {
    let text = read("bundle-plain.prosecraft");
    let bundle = PlainBundle::parse(&text).expect("valid backup");
    assert_eq!(bundle.projects().len(), 2);
    // Nothing lost or reordered: lone surrogates, integer-like keys, -0, 1e21, future fields.
    assert_eq!(bundle.to_text(), text);
    let projects: Vec<JsValue> = bundle.projects().into_iter().cloned().collect();
    let exported_at = bundle
        .raw
        .get("exportedAt")
        .and_then(JsValue::as_number)
        .expect("exportedAt");
    assert_eq!(write_plain_bundle(&projects, exported_at, "device-a"), text);
    let future = projects
        .first()
        .and_then(|p| p.get("project"))
        .and_then(|p| p.get("futureField"));
    assert!(
        future.is_some(),
        "a field from a newer release must survive"
    );
}

#[test]
fn encrypted_backup_decrypts_and_reseals_byte_for_byte() {
    let text = read("bundle-encrypted.prosecraft");
    let secrets = fixture("bundle-encrypted.key.json");
    let bundle = EncryptedBundle::parse(&text).expect("valid encrypted backup");
    assert_eq!(bundle.hint().as_deref(), Some("the usual"));

    let (key, projects) = bundle
        .open(&js_string(secrets.get("passphrase")))
        .expect("open");
    assert_eq!(
        key.as_bytes().as_slice(),
        hex(secrets.get("vaultKey")).as_slice()
    );
    assert_eq!(projects.len(), 2);
    assert!(projects.iter().all(is_stored_project));
    // The decrypted projects are exactly the ones in the plain backup.
    let plain = PlainBundle::parse(&read("bundle-plain.prosecraft")).expect("plain");
    let expected: Vec<String> = plain.projects().into_iter().map(js::stringify).collect();
    assert_eq!(
        projects.iter().map(js::stringify).collect::<Vec<_>>(),
        expected
    );

    let envelopes: Vec<EntityEnvelope> = bundle
        .raw
        .get("entities")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(EntityEnvelope::read)
        .collect();
    let resealed = reseal_with_nonces(&projects, &envelopes, &key, "device-a").expect("reseal");
    let created_at = bundle
        .raw
        .get("createdAt")
        .and_then(JsValue::as_number)
        .expect("createdAt");
    assert_eq!(
        assemble_encrypted(&resealed, &bundle.header, created_at, Some("the usual")),
        text
    );

    assert!(matches!(
        bundle.open(&JsString::from("wrong")),
        Err(BundleError::Crypto(CryptoError::IncorrectPassphrase))
    ));
}

#[test]
fn hostile_or_foreign_files_are_refused_not_crashed_on() {
    for text in [
        "",
        "null",
        "[]",
        "{}",
        r#"{"format":"prosecraft.bundle"}"#,
        r#"{"format":"prosecraft.bundle","projects":[{}]}"#,
        "{\"format\":",
        &"[".repeat(100_000),
    ] {
        assert!(PlainBundle::parse(text).is_err(), "{text:.40}");
        assert!(EncryptedBundle::parse(text).is_err(), "{text:.40}");
    }
    // A header asking for a gigabyte of Argon2 memory is not a vault.
    let text =
        read("bundle-encrypted.prosecraft").replace("\"memoryKiB\": 64", "\"memoryKiB\": 1048576");
    assert_eq!(
        EncryptedBundle::parse(&text).err(),
        Some(BundleError::NotEncrypted)
    );
}

#[test]
fn stamping_matches_prosecraft_step_by_step() {
    use prosecraft_core::storage::stamp::{
        create_stored_project, project_content_key, stamp_project,
    };
    let fixture = fixture("stamping.json");
    let created = fixture.get("created").expect("created");
    let project = created.get("project").cloned().expect("project");
    assert_eq!(
        js::stable_stringify(&create_stored_project(project.clone(), "device-a", 1_000.0)),
        js::stable_stringify(created)
    );
    let key = fixture
        .get("contentKey")
        .and_then(JsValue::as_js_string)
        .and_then(JsString::as_string)
        .expect("key");
    assert_eq!(project_content_key(&project), key);
    for step in fixture
        .get("steps")
        .and_then(JsValue::as_array)
        .unwrap_or_default()
    {
        let name = step
            .get("name")
            .and_then(JsValue::as_js_string)
            .map(JsString::to_string_lossy)
            .unwrap_or_default();
        let previous = step.get("previous").filter(|p| !p.is_null());
        let device = step
            .get("device")
            .and_then(JsValue::as_js_string)
            .map(JsString::to_string_lossy)
            .unwrap_or_default();
        let now = step
            .get("now")
            .and_then(JsValue::as_number)
            .unwrap_or_default();
        let stamped = stamp_project(
            previous,
            step.get("project").cloned().expect("project"),
            &device,
            now,
        );
        assert_eq!(
            js::stringify(&stamped),
            js::stringify(step.get("result").expect("result")),
            "{name}"
        );
    }
}

#[test]
fn plain_text_export_matches_prosecraft_byte_for_byte() {
    use prosecraft_core::project::Project;
    use prosecraft_core::publish::{PublishOptions, compile_book, export_text};
    let fixture = fixture("export-text.json");
    // The fixture's project is project data; wrap it as a stored project for the view.
    let mut stored = prosecraft_core::js::JsObject::new();
    stored.insert("project", fixture.get("project").cloned().expect("project"));
    let mut project = Project::from_stored(&JsValue::Object(stored)).expect("project view");
    // These fixtures compiled the book without the citation library (`compileBook`'s
    // `citations` left out), so every citation stays `[@key]`; see `export-citations.json`.
    project.citations.clear();
    for (name, case) in fixture
        .get("exports")
        .and_then(JsValue::as_object)
        .expect("exports")
        .iter()
    {
        let options = PublishOptions::read(case.get("options"));
        let book = compile_book(&project, &options);
        let export = export_text(&book, &options);
        let expected = case
            .get("text")
            .and_then(JsValue::as_js_string)
            .and_then(JsString::as_string)
            .expect("text");
        assert_eq!(
            String::from_utf8_lossy(&export.bytes),
            expected,
            "{}",
            name.to_string_lossy()
        );
        assert_eq!(
            Some(export.file_name.as_str()),
            fixture
                .get("filename")
                .and_then(JsValue::as_js_string)
                .and_then(JsString::as_string)
                .as_deref()
        );
    }
}

#[test]
fn markdown_export_matches_prosecraft_byte_for_byte() {
    use prosecraft_core::project::Project;
    use prosecraft_core::publish::{PublishOptions, compile_book, export_markdown};
    let fixture = fixture("export-markdown.json");
    let mut stored = prosecraft_core::js::JsObject::new();
    stored.insert("project", fixture.get("project").cloned().expect("project"));
    let mut project = Project::from_stored(&JsValue::Object(stored)).expect("project view");
    // These fixtures compiled the book without the citation library (`compileBook`'s
    // `citations` left out), so every citation stays `[@key]`; see `export-citations.json`.
    project.citations.clear();
    for (name, case) in fixture
        .get("exports")
        .and_then(JsValue::as_object)
        .expect("exports")
        .iter()
    {
        let options = PublishOptions::read(case.get("options"));
        let export = export_markdown(&compile_book(&project, &options), &options);
        let expected = case
            .get("markdown")
            .and_then(JsValue::as_js_string)
            .and_then(JsString::as_string)
            .expect("markdown");
        assert_eq!(
            String::from_utf8_lossy(&export.bytes),
            expected,
            "{}",
            name.to_string_lossy()
        );
        assert_eq!(export.file_name, "the-lighthouse-keeper.md");
    }
}

#[test]
fn html_export_matches_prosecraft_byte_for_byte() {
    use prosecraft_core::project::Project;
    use prosecraft_core::publish::{PublishOptions, compile_book, export_html};
    let fixture = fixture("export-html.json");
    let mut stored = prosecraft_core::js::JsObject::new();
    stored.insert("project", fixture.get("project").cloned().expect("project"));
    let mut project = Project::from_stored(&JsValue::Object(stored)).expect("project view");
    // These fixtures compiled the book without the citation library (`compileBook`'s
    // `citations` left out), so every citation stays `[@key]`; see `export-citations.json`.
    project.citations.clear();
    for (name, case) in fixture
        .get("exports")
        .and_then(JsValue::as_object)
        .expect("exports")
        .iter()
    {
        let options = PublishOptions::read(case.get("options"));
        let export = export_html(&compile_book(&project, &options), &options);
        let expected = case
            .get("html")
            .and_then(JsValue::as_js_string)
            .and_then(JsString::as_string)
            .expect("html");
        let got = String::from_utf8_lossy(&export.bytes);
        if got != expected {
            let at = got
                .chars()
                .zip(expected.chars())
                .take_while(|(a, b)| a == b)
                .count();
            let start = at.saturating_sub(120);
            let window = |s: &str| s.chars().skip(start).take(260).collect::<String>();
            panic!(
                "{}: differs at char {at}\n--- want\n{}\n--- got\n{}",
                name.to_string_lossy(),
                window(&expected),
                window(&got)
            );
        }
        assert_eq!(export.file_name, "the-lighthouse-keeper.html");
    }
}

#[test]
fn epub_export_matches_prosecraft_entry_for_entry() {
    use prosecraft_core::project::Project;
    use prosecraft_core::publish::{PublishOptions, compile_book, epub_entries, export_epub_with};
    let fixture = fixture("export-epub.json");
    let mut stored = prosecraft_core::js::JsObject::new();
    stored.insert("project", fixture.get("project").cloned().expect("project"));
    let mut project = Project::from_stored(&JsValue::Object(stored)).expect("project view");
    // These fixtures compiled the book without the citation library (`compileBook`'s
    // `citations` left out), so every citation stays `[@key]`; see `export-citations.json`.
    project.citations.clear();
    for (name, case) in fixture
        .get("exports")
        .and_then(JsValue::as_object)
        .expect("exports")
        .iter()
    {
        let name = name.to_string_lossy();
        let options = PublishOptions::read(case.get("options"));
        let book = compile_book(&project, &options);
        let got = epub_entries(&book, &options, "urn:uuid:ID", "MODIFIED").expect("entries");
        let want: Vec<(String, String)> = case
            .get("entries")
            .and_then(JsValue::as_array)
            .expect("entries")
            .iter()
            .map(|e| {
                let text = |k: &str| {
                    e.get(k)
                        .and_then(JsValue::as_js_string)
                        .and_then(JsString::as_string)
                        .expect("text")
                };
                (text("name"), text("text"))
            })
            .collect();
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            want.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            "{name}: entry order"
        );
        for ((entry, bytes), (_, text)) in got.iter().zip(&want) {
            assert_eq!(&String::from_utf8_lossy(bytes), text, "{name}: {entry}");
        }
        // The package: the stored mimetype first, as OCF requires, and every entry inside.
        let epub = export_epub_with(&book, &options, "urn:uuid:ID", "MODIFIED").expect("epub");
        assert!(
            epub.bytes.starts_with(b"PK\x03\x04") && epub.bytes[30..38] == *b"mimetype",
            "{name}"
        );
        assert_eq!(
            &epub.bytes[38..58],
            b"application/epub+zip",
            "{name}: mimetype is stored first"
        );
    }
}

/// Exports with citations, as ProseCraft writes them: each style's citations in the text (in
/// the notes style, as footnotes), sources not in the library as `[@key]`, and the bibliography
/// in Markdown, HTML and EPUB (none when it is turned off), in English and German.
#[test]
fn exports_with_citations_match_prosecraft() {
    use prosecraft_core::project::Project;
    use prosecraft_core::publish::{
        PublishOptions, compile_book, epub_entries, export_html, export_markdown, export_text,
    };
    let fixture = fixture("export-citations.json");
    let mut stored = prosecraft_core::js::JsObject::new();
    stored.insert("project", fixture.get("project").cloned().expect("project"));
    let project = Project::from_stored(&JsValue::Object(stored)).expect("project view");
    assert_eq!(project.citations.len(), 5);
    let text = |v: &JsValue, k: &str| {
        v.get(k)
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    };
    let mut checked = 0;
    for (name, case) in fixture
        .get("exports")
        .and_then(JsValue::as_object)
        .expect("exports")
        .iter()
    {
        let name = name.to_string_lossy();
        let options = PublishOptions::read(case.get("options"));
        let book = compile_book(&project, &options);
        let utf8 = |bytes: Vec<u8>| String::from_utf8(bytes).expect("utf-8");
        assert_eq!(
            utf8(export_text(&book, &options).bytes),
            text(case, "text"),
            "{name}: text"
        );
        assert_eq!(
            utf8(export_markdown(&book, &options).bytes),
            text(case, "markdown"),
            "{name}: markdown"
        );
        assert_eq!(
            utf8(export_html(&book, &options).bytes),
            text(case, "html"),
            "{name}: html"
        );
        let entries = epub_entries(&book, &options, "urn:uuid:ID", "MODIFIED").expect("epub");
        let want = case.get("epub").and_then(JsValue::as_array).expect("epub");
        let names: Vec<String> = entries.iter().map(|(n, _)| n.clone()).collect();
        let want_names: Vec<String> = want.iter().map(|e| text(e, "name")).collect();
        assert_eq!(names, want_names, "{name}: epub entries");
        for ((entry, bytes), w) in entries.iter().zip(want) {
            assert_eq!(utf8(bytes.clone()), text(w, "text"), "{name}: {entry}");
        }
        checked += 1;
    }
    assert_eq!(checked, 6);
}

/// `sanitizeStoredProject`: a backup from outside can't bring in HTML the editor or an export
/// would carry: each document's body is sanitized and its footnotes normalized on import.
#[test]
fn imported_backups_are_sanitized_as_prosecraft_imports_them() {
    use prosecraft_core::library::Library;
    let path = format!(
        "{}/tests/fixtures/bundle-plain.prosecraft",
        env!("CARGO_MANIFEST_DIR")
    );
    let bundle = js::parse(&std::fs::read_to_string(path).expect("fixture")).expect("json");
    let mut stored = bundle
        .get("projects")
        .and_then(JsValue::as_array)
        .and_then(|p| p.first())
        .cloned()
        .expect("project");
    let doc = js::parse(r#"{"id":"d","title":"D","content":"<p onclick=\"x()\">Hi<script>bad()</script></p><img src=\"https://example.org/t.png\">","footnotes":[{"id":"a","html":"<b onclick=\"x()\">Note</b>"},{"id":"a","html":"dup"},{"id":"bad id!","html":"x"}]}"#).expect("doc");
    if let Some(JsValue::Array(docs)) = stored
        .as_object_mut()
        .and_then(|s| s.get_mut("project"))
        .and_then(JsValue::as_object_mut)
        .and_then(|p| p.get_mut("documents"))
    {
        docs.insert(0, doc);
    }
    let mut library = Library::new("d".into());
    library.merge(vec![stored]);
    let project = library.projects().next().expect("project");
    let doc = project
        .get("project")
        .and_then(|p| p.get("documents"))
        .and_then(JsValue::as_array)
        .and_then(|d| d.first())
        .cloned()
        .expect("doc");
    let content = doc
        .get("content")
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    assert_eq!(
        content,
        prosecraft_core::html::sanitize_html(
            r#"<p onclick="x()">Hi<script>bad()</script></p><img src="https://example.org/t.png">"#
        )
    );
    assert!(
        !content.contains("onclick")
            && !content.contains("script")
            && !content.contains("example.org"),
        "{content}"
    );
    assert_eq!(
        js::stringify(doc.get("footnotes").expect("notes")),
        r#"[{"id":"a","html":"<b>Note</b>"}]"#
    );
}
