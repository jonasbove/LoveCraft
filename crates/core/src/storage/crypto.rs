//! Passphrase vaults and encrypted records, compatible with ProseCraft's.
//!
//! Ported from ProseCraft `src/lib/storage/crypto.ts` (upstream `7dae544`). Every detail that
//! decides whether existing files can be decrypted is kept:
//!
//! - **Key derivation:** Argon2id v1.3 over the UTF-8 of the NFKC-normalized passphrase (a lone
//!   surrogate becomes U+FFFD, as `TextEncoder` makes it), 32-byte output.
//! - **Vault key:** a random 32-byte content key, sealed with XChaCha20-Poly1305 under the derived
//!   key, with the AAD `prosecraft-vault-key-v1`.
//! - **Records:** `JSON.stringify(payload)` sealed under the vault key. The AAD is the envelope's
//!   cleartext fields joined by NUL, with numbers written as JavaScript's `String()` writes them.
//! - Base64 is standard with padding; the header bounds stop a hostile file from asking for
//!   gigabytes of Argon2 memory.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use crate::js::{self, JsObject, JsString, JsValue, number_to_string};

/// `ENVELOPE_VERSION`: the cleartext header and KDF contract.
pub const ENVELOPE_VERSION: f64 = 1.0;

/// `ARGON2_DEFAULTS` (OWASP's recommendation: 19 MiB, 2 passes, one lane).
pub const DEFAULT_MEMORY_KIB: u32 = 19_456;
pub const DEFAULT_ITERATIONS: u32 = 2;
pub const DEFAULT_PARALLELISM: u32 = 1;

const KEY_LENGTH: usize = 32;
const SALT_LENGTH: usize = 16;
const NONCE_LENGTH: usize = 24;
const TAG_LENGTH: usize = 16;
const VAULT_KEY_AAD: &[u8] = b"prosecraft-vault-key-v1";

/// Upper bounds for KDF parameters read from a file, as ProseCraft enforces them. Without them a
/// hostile header could ask for an enormous Argon2 allocation and hang the app.
const MAX_KDF_MEMORY_KIB: u32 = 262_144;
const MAX_KDF_ITERATIONS: u32 = 16;
const MAX_KDF_PARALLELISM: u32 = 8;

/// What can go wrong. The messages are ProseCraft's, so people see the same text.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("A passphrase is required.")]
    PassphraseRequired,
    #[error("Unsupported key-derivation parameters.")]
    UnsupportedKdf,
    #[error("This is not a valid ProseCraft vault.")]
    NotAVault,
    #[error("Incorrect passphrase.")]
    IncorrectPassphrase,
    #[error("This record is not readable.")]
    RecordUnreadable,
    #[error("This record could not be decrypted.")]
    RecordNotDecrypted,
    #[error("This record could not be decoded.")]
    RecordNotDecoded,
    #[error("Random numbers are not available.")]
    NoRandomness,
}

/// Where salts, nonces and keys come from. The desktop uses [`OsRandom`]; the web build passes
/// the browser's `crypto.getRandomValues`; tests can pass a fixed stream.
pub trait RandomSource {
    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), CryptoError>;
}

/// The operating system's random numbers.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default, Clone, Copy)]
pub struct OsRandom;

#[cfg(not(target_arch = "wasm32"))]
impl RandomSource for OsRandom {
    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), CryptoError> {
        getrandom::fill(bytes).map_err(|_| CryptoError::NoRandomness)
    }
}

/// The content key that seals a vault's records. Wiped from memory when dropped.
#[derive(Clone)]
pub struct VaultKey(Zeroizing<[u8; KEY_LENGTH]>);

impl VaultKey {
    pub fn from_bytes(bytes: [u8; KEY_LENGTH]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; KEY_LENGTH] {
        &self.0
    }

    fn cipher(&self) -> Result<XChaCha20Poly1305, CryptoError> {
        XChaCha20Poly1305::new_from_slice(self.0.as_slice())
            .map_err(|_| CryptoError::RecordNotDecrypted)
    }
}

impl std::fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VaultKey(…)")
    }
}

fn engine() -> GeneralPurpose {
    // `atob` accepts non-zero trailing bits; the canonical-form check is `is_base64` below.
    GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_allow_trailing_bits(true)
            .with_decode_padding_mode(DecodePaddingMode::RequireCanonical),
    )
}

/// ProseCraft's `isBase64`: non-empty, a multiple of four, standard alphabet, padding only at
/// the end.
fn is_base64(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return false;
    }
    let padding = bytes.iter().rev().take_while(|b| **b == b'=').count();
    let body = bytes.get(..bytes.len() - padding).unwrap_or_default();
    padding <= 2
        && body
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/')
}

fn decode(text: &JsString) -> Option<Vec<u8>> {
    let text = text.as_string()?;
    if !is_base64(&text) {
        return None;
    }
    engine().decode(text).ok()
}

fn encode(bytes: &[u8]) -> JsValue {
    JsValue::str(&base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn decode_array<const N: usize>(value: Option<&JsValue>) -> Option<[u8; N]> {
    decode(value?.as_js_string()?)?.try_into().ok()
}

/// `Number.isInteger(value)` within `min..=max`, as a `u32`.
fn integer_in(value: Option<&JsValue>, min: u32, max: u32) -> Option<u32> {
    let n = value?.as_number()?;
    if n.fract() != 0.0 || n < f64::from(min) || n > f64::from(max) {
        return None;
    }
    // In range and integral, so the conversion is exact.
    Some(n as u32)
}

/// Argon2id parameters read from (or written to) a vault header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
    pub salt: Vec<u8>,
}

impl KdfParams {
    /// `isKdfParams`.
    pub fn read(value: &JsValue) -> Option<Self> {
        let kdf = value.as_object()?;
        if !kdf.get("algorithm")?.as_js_string()?.eq_str("argon2id") {
            return None;
        }
        Some(Self {
            memory_kib: integer_in(kdf.get("memoryKiB"), 8, MAX_KDF_MEMORY_KIB)?,
            iterations: integer_in(kdf.get("iterations"), 1, MAX_KDF_ITERATIONS)?,
            parallelism: integer_in(kdf.get("parallelism"), 1, MAX_KDF_PARALLELISM)?,
            salt: decode(kdf.get("salt")?.as_js_string()?)?,
        })
    }

    fn to_js(&self) -> JsValue {
        let mut kdf = JsObject::new();
        kdf.insert("algorithm", JsValue::str("argon2id"));
        kdf.insert("memoryKiB", JsValue::Number(f64::from(self.memory_kib)));
        kdf.insert("iterations", JsValue::Number(f64::from(self.iterations)));
        kdf.insert("parallelism", JsValue::Number(f64::from(self.parallelism)));
        kdf.insert("salt", encode(&self.salt));
        JsValue::Object(kdf)
    }
}

/// `deriveKek`: the key-encryption key for a passphrase.
pub fn derive_kek(
    passphrase: &JsString,
    kdf: &KdfParams,
) -> Result<Zeroizing<[u8; KEY_LENGTH]>, CryptoError> {
    let normalized: String = passphrase.to_string_lossy().nfkc().collect();
    let normalized = Zeroizing::new(normalized);
    let params = Params::new(
        kdf.memory_kib,
        kdf.iterations,
        kdf.parallelism,
        Some(KEY_LENGTH),
    )
    .map_err(|_| CryptoError::UnsupportedKdf)?;
    let mut out = Zeroizing::new([0u8; KEY_LENGTH]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(normalized.as_bytes(), &kdf.salt, out.as_mut_slice())
        .map_err(|_| CryptoError::UnsupportedKdf)?;
    Ok(out)
}

/// The cleartext part of a vault (`vault.json`, or `header` in an encrypted backup): enough to
/// derive the key, nothing secret.
#[derive(Clone, Debug, PartialEq)]
pub struct VaultHeader {
    pub kdf: KdfParams,
    pub nonce: [u8; NONCE_LENGTH],
    pub ciphertext: [u8; KEY_LENGTH + TAG_LENGTH],
    /// The header as stored, so writing it back changes nothing.
    pub raw: JsValue,
}

impl VaultHeader {
    /// `isVaultHeader`.
    pub fn read(value: &JsValue) -> Option<Self> {
        let header = value.as_object()?;
        if header.get("version")?.as_number()? != ENVELOPE_VERSION {
            return None;
        }
        let kdf = KdfParams::read(header.get("kdf")?)?;
        let wrapped = header.get("wrappedKey")?;
        if !matches!(header.get("hint"), None | Some(JsValue::String(_))) {
            return None;
        }
        Some(Self {
            kdf,
            nonce: decode_array(wrapped.get("nonce"))?,
            ciphertext: decode_array(wrapped.get("ciphertext"))?,
            raw: value.clone(),
        })
    }

    /// The passphrase hint, if the header has one.
    pub fn hint(&self) -> Option<String> {
        self.raw
            .get("hint")
            .and_then(JsValue::as_js_string)
            .map(JsString::to_string_lossy)
    }
}

/// Options for [`create_vault`]; `None` means ProseCraft's default.
#[derive(Clone, Debug, Default)]
pub struct VaultOptions {
    pub hint: Option<String>,
    pub memory_kib: Option<u32>,
    pub iterations: Option<u32>,
    pub parallelism: Option<u32>,
}

/// `createVault`: a fresh random content key, sealed under a key derived from the passphrase.
pub fn create_vault(
    passphrase: &JsString,
    options: &VaultOptions,
    random: &mut dyn RandomSource,
) -> Result<(VaultHeader, VaultKey), CryptoError> {
    if passphrase.is_empty() {
        return Err(CryptoError::PassphraseRequired);
    }
    let mut key = [0u8; KEY_LENGTH];
    random.fill(&mut key)?;
    let vault_key = VaultKey::from_bytes(key);
    let mut salt = vec![0u8; SALT_LENGTH];
    random.fill(&mut salt)?;
    let kdf = KdfParams {
        memory_kib: options.memory_kib.unwrap_or(DEFAULT_MEMORY_KIB),
        iterations: options.iterations.unwrap_or(DEFAULT_ITERATIONS),
        parallelism: options.parallelism.unwrap_or(DEFAULT_PARALLELISM),
        salt,
    };
    let kek = derive_kek(passphrase, &kdf)?;
    let mut nonce = [0u8; NONCE_LENGTH];
    random.fill(&mut nonce)?;
    let sealed = XChaCha20Poly1305::new_from_slice(kek.as_slice())
        .map_err(|_| CryptoError::UnsupportedKdf)?
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: vault_key.as_bytes(),
                aad: VAULT_KEY_AAD,
            },
        )
        .map_err(|_| CryptoError::UnsupportedKdf)?;
    let ciphertext: [u8; KEY_LENGTH + TAG_LENGTH] =
        sealed.try_into().map_err(|_| CryptoError::UnsupportedKdf)?;

    let mut wrapped = JsObject::new();
    wrapped.insert("nonce", encode(&nonce));
    wrapped.insert("ciphertext", encode(&ciphertext));
    let mut header = JsObject::new();
    header.insert("version", JsValue::Number(ENVELOPE_VERSION));
    header.insert("kdf", kdf.to_js());
    header.insert("wrappedKey", JsValue::Object(wrapped));
    if let Some(hint) = &options.hint {
        header.insert("hint", JsValue::str(hint));
    }
    Ok((
        VaultHeader {
            kdf,
            nonce,
            ciphertext,
            raw: JsValue::Object(header),
        },
        vault_key,
    ))
}

/// `unlockVault`: the content key, or [`CryptoError::IncorrectPassphrase`].
pub fn unlock_vault(header: &VaultHeader, passphrase: &JsString) -> Result<VaultKey, CryptoError> {
    let kek = derive_kek(passphrase, &header.kdf)?;
    let key = XChaCha20Poly1305::new_from_slice(kek.as_slice())
        .map_err(|_| CryptoError::NotAVault)?
        .decrypt(
            &XNonce::from(header.nonce),
            Payload {
                msg: &header.ciphertext,
                aad: VAULT_KEY_AAD,
            },
        )
        .map_err(|_| CryptoError::IncorrectPassphrase)?;
    let key: [u8; KEY_LENGTH] = key
        .try_into()
        .map_err(|_| CryptoError::IncorrectPassphrase)?;
    Ok(VaultKey::from_bytes(key))
}

/// The cleartext routing and merge fields of an encrypted record (`EntityMetaPlain`).
#[derive(Clone, Debug, PartialEq)]
pub struct EntityMeta {
    pub kind: JsString,
    pub id: JsString,
    pub project_id: JsString,
    pub device_id: JsString,
    pub updated_at: f64,
    pub revision: f64,
}

impl EntityMeta {
    /// The authenticated data: the cleartext fields joined by NUL, numbers as `String()` writes
    /// them, strings as `TextEncoder` encodes them.
    fn aad(&self) -> Vec<u8> {
        [
            "prosecraft-entity".to_owned(),
            number_to_string(ENVELOPE_VERSION),
            self.kind.to_string_lossy(),
            self.id.to_string_lossy(),
            self.project_id.to_string_lossy(),
            self.device_id.to_string_lossy(),
            number_to_string(self.updated_at),
            number_to_string(self.revision),
        ]
        .join("\0")
        .into_bytes()
    }
}

/// An encrypted record as stored: its cleartext fields, nonce and sealed body.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityEnvelope {
    pub meta: EntityMeta,
    pub nonce: [u8; NONCE_LENGTH],
    pub body: Vec<u8>,
}

impl EntityEnvelope {
    /// `isEntityEnvelope`.
    pub fn read(value: &JsValue) -> Option<Self> {
        let envelope = value.as_object()?;
        if envelope.get("version")?.as_number()? != ENVELOPE_VERSION {
            return None;
        }
        let text = |key: &str| envelope.get(key).and_then(JsValue::as_js_string).cloned();
        let updated_at = envelope
            .get("updatedAt")?
            .as_number()
            .filter(|n| n.is_finite())?;
        let revision = envelope
            .get("revision")?
            .as_number()
            .filter(|n| n.fract() == 0.0 && *n >= 0.0)?;
        Some(Self {
            meta: EntityMeta {
                kind: text("kind")?,
                id: text("id")?,
                project_id: text("projectId")?,
                device_id: text("deviceId")?,
                updated_at,
                revision,
            },
            nonce: decode_array(envelope.get("nonce"))?,
            body: decode(envelope.get("body")?.as_js_string()?)?,
        })
    }

    /// The envelope as ProseCraft writes it, fields in its order.
    pub fn to_js(&self) -> JsValue {
        let mut envelope = JsObject::new();
        envelope.insert("version", JsValue::Number(ENVELOPE_VERSION));
        envelope.insert("kind", JsValue::String(self.meta.kind.clone()));
        envelope.insert("id", JsValue::String(self.meta.id.clone()));
        envelope.insert("projectId", JsValue::String(self.meta.project_id.clone()));
        envelope.insert("deviceId", JsValue::String(self.meta.device_id.clone()));
        envelope.insert("updatedAt", JsValue::Number(self.meta.updated_at));
        envelope.insert("revision", JsValue::Number(self.meta.revision));
        envelope.insert("nonce", encode(&self.nonce));
        envelope.insert("body", encode(&self.body));
        JsValue::Object(envelope)
    }
}

/// `encryptEntity`, with a fresh nonce.
pub fn encrypt_entity(
    key: &VaultKey,
    meta: EntityMeta,
    payload: &JsValue,
    random: &mut dyn RandomSource,
) -> Result<EntityEnvelope, CryptoError> {
    let mut nonce = [0u8; NONCE_LENGTH];
    random.fill(&mut nonce)?;
    encrypt_entity_with_nonce(key, meta, payload, nonce)
}

/// `encryptEntity` with a given nonce. A nonce must never be reused with the same key; this
/// exists so tests can reproduce ProseCraft's output byte for byte.
pub fn encrypt_entity_with_nonce(
    key: &VaultKey,
    meta: EntityMeta,
    payload: &JsValue,
    nonce: [u8; NONCE_LENGTH],
) -> Result<EntityEnvelope, CryptoError> {
    let plaintext = Zeroizing::new(js::stringify(payload));
    let body = key
        .cipher()?
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plaintext.as_bytes(),
                aad: &meta.aad(),
            },
        )
        .map_err(|_| CryptoError::RecordNotDecrypted)?;
    Ok(EntityEnvelope { meta, nonce, body })
}

/// `decryptEntity`. The cleartext fields are authenticated, so a changed `id` or `revision`
/// fails here instead of returning tampered metadata.
pub fn decrypt_entity(key: &VaultKey, envelope: &EntityEnvelope) -> Result<JsValue, CryptoError> {
    let plaintext = key
        .cipher()?
        .decrypt(
            &XNonce::from(envelope.nonce),
            Payload {
                msg: &envelope.body,
                aad: &envelope.meta.aad(),
            },
        )
        .map_err(|_| CryptoError::RecordNotDecrypted)?;
    let plaintext = Zeroizing::new(plaintext);
    // `TextDecoder`: invalid UTF-8 becomes U+FFFD, and a leading byte-order mark is dropped.
    let text = Zeroizing::new(String::from_utf8_lossy(&plaintext).into_owned());
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    js::parse(text).map_err(|_| CryptoError::RecordNotDecoded)
}

#[cfg(test)]
mod tests {
    use super::{integer_in, is_base64};
    use crate::js::JsValue;

    #[test]
    fn base64_check_matches_prosecraft() {
        for good in ["AAAA", "AA==", "AAA=", "ab+/Zz09"] {
            assert!(is_base64(good), "{good}");
        }
        for bad in ["", "A", "AAA", "A===", "AA=A", "AA-_", "AAAA\n", "===="] {
            assert!(!is_base64(bad), "{bad}");
        }
    }

    #[test]
    fn kdf_bounds_refuse_hostile_headers() {
        assert_eq!(
            integer_in(Some(&JsValue::Number(262_144.0)), 8, 262_144),
            Some(262_144)
        );
        assert_eq!(
            integer_in(Some(&JsValue::Number(262_145.0)), 8, 262_144),
            None
        );
        assert_eq!(integer_in(Some(&JsValue::Number(2.5)), 1, 16), None);
        assert_eq!(integer_in(Some(&JsValue::Number(f64::NAN)), 1, 16), None);
        assert_eq!(integer_in(Some(&JsValue::str("2")), 1, 16), None);
    }
}
