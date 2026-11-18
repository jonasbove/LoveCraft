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
    let project = Project::from_stored(&JsValue::Object(stored)).expect("project view");
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
