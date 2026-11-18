//! Zotero: search a running Zotero 7's library, or pick with Better BibTeX's Cite-As-You-Write,
//! and map what comes back into citation items (ProseCraft `src/lib/citations/zotero.ts`, with
//! the fix for type names like `constructor` (`Object.hasOwn`), and the local HTTP request from
//! `src-tauri/src/lib.rs`, upstream `2d1cf18`).
//!
//! Zotero answers on `127.0.0.1:23119` when "Allow other applications on this computer to
//! communicate with Zotero" is on. Requests go through a transport (a path in, the body or an
//! error message out): [`request`] on the desktop, anything in tests.

use std::sync::OnceLock;

use crate::csl::{normalize_csl_items_detailed, sanitize_citation_id};
use crate::html::is_js_whitespace;
use crate::js::regexp::Re;
use crate::js::{JsObject, JsValue};

/// `ZOTERO_PING_PATH`.
pub const PING_PATH: &str = "/connector/ping";
/// `ZOTERO_BBT_CAYW_PATH`: Better BibTeX's picker, answering with the chosen items.
pub const CAYW_PATH: &str = "/better-bibtex/cayw?format=json";
/// `ZOTERO_SEARCH_LIMIT`.
pub const SEARCH_LIMIT: u32 = 25;

/// `ZoteroErrorCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    NotRunning,
    Forbidden,
    Timeout,
    NotDesktop,
    InvalidResponse,
    Unknown,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRunning => "not-running",
            Self::Forbidden => "forbidden",
            Self::Timeout => "timeout",
            Self::NotDesktop => "not-desktop",
            Self::InvalidResponse => "invalid-response",
            Self::Unknown => "unknown",
        }
    }
}

/// `ZoteroErrorState` (and `ZoteroError`): what went wrong, for the person, and the raw detail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZoteroError {
    pub code: ErrorCode,
    pub message: String,
    pub detail: Option<String>,
}

impl ZoteroError {
    fn new(code: ErrorCode, message: &str, detail: &str) -> Self {
        Self {
            code,
            message: message.to_owned(),
            detail: (!detail.is_empty()).then(|| detail.to_owned()),
        }
    }
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `encodeURIComponent`.
fn encode_uri_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `buildZoteroSearchPath`: the local API search for CSL-JSON items; a limit that isn't a
/// positive integer is the default, and none is over 100.
pub fn search_path(query: &str, limit: Option<f64>) -> String {
    let limit = match limit {
        Some(l) if l.fract() == 0.0 && l > 0.0 && l.is_finite() => l.min(100.0) as u32,
        _ => SEARCH_LIMIT,
    };
    format!(
        "/api/users/0/items?q={}&format=csljson&limit={limit}",
        encode_uri_component(js_trim(query))
    )
}

const FORBIDDEN_MESSAGE: &str = "Zotero blocked local API access (HTTP 403). Enable \"Allow other applications on this computer to communicate with Zotero\" in Zotero Settings → Advanced → Files and Folders.";

/// `classifyZoteroHttpError`.
pub fn classify_http_error(status: u16, body: &str) -> ZoteroError {
    let detail = js_trim(body);
    match status {
        403 => ZoteroError::new(ErrorCode::Forbidden, FORBIDDEN_MESSAGE, detail),
        408 | 504 => ZoteroError::new(
            ErrorCode::Timeout,
            "Zotero did not respond in time (5 s timeout). Check that Zotero is responsive and try again.",
            detail,
        ),
        _ => ZoteroError::new(
            ErrorCode::Unknown,
            &format!("Zotero returned HTTP {status}."),
            detail,
        ),
    }
}

macro_rules! rule {
    ($name:ident, $source:expr, $flags:expr) => {
        fn $name() -> &'static Re {
            static RULE: OnceLock<Re> = OnceLock::new();
            RULE.get_or_init(|| Re::new($source, $flags))
        }
    };
}

rule!(http_status, r"^HTTP\s+(\d{3})(?:\s*:\s*([\s\S]*))?$", "i");
rule!(
    forbidden,
    r"local api is not enabled|allow other applications on this computer to communicate with zotero|\b403\b.*forbidden",
    "i"
);
rule!(timed_out, r"timed?\s*out|timeout|wouldblock|etimedout", "i");
rule!(
    not_running,
    r"not running|connection refused|econnrefused|actively refused|connect.*failed|os error 111|os error 10061",
    "i"
);
rule!(not_desktop, r"only available in the desktop", "i");
rule!(item_uri, r"\/items\/([A-Za-z0-9]{4,32})(?:[/?#]|$)", "i");
rule!(web_url, r"^https?:\/\/", "i");

/// `classifyZoteroError` for a transport's message.
pub fn classify_error(raw: &str) -> ZoteroError {
    let trimmed = js_trim(raw);
    if let Some(caps) = http_status().exec(trimmed) {
        let status = caps.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        return classify_http_error(status, caps.get(2).unwrap_or_default());
    }
    if forbidden().test(trimmed) {
        return classify_http_error(403, trimmed);
    }
    if timed_out().test(trimmed) {
        return ZoteroError::new(
            ErrorCode::Timeout,
            "Zotero request timed out after 5 seconds. Make sure Zotero is responsive and try again.",
            trimmed,
        );
    }
    if not_running().test(trimmed) {
        return ZoteroError::new(
            ErrorCode::NotRunning,
            "Zotero is not running. Start Zotero on this computer and try again.",
            trimmed,
        );
    }
    if not_desktop().test(trimmed) {
        return ZoteroError::new(
            ErrorCode::NotDesktop,
            "Zotero integration is only available in the ProseCraft desktop app.",
            trimmed,
        );
    }
    let message = if trimmed.is_empty() {
        "Could not communicate with Zotero."
    } else {
        trimmed
    };
    ZoteroError::new(ErrorCode::Unknown, message, trimmed)
}

fn text(value: Option<&JsValue>) -> Option<String> {
    value
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
}

/// `sanitizeCitationId`, kept only when something is left.
fn cleaned_id(raw: &str) -> Option<String> {
    Some(sanitize_citation_id(raw)).filter(|id| !id.is_empty())
}

/// `extractZoteroKey`: the item key from a raw key field, a Zotero URI (`…/items/ABCD1234`) or
/// a plain id. A number's id is kept even when nothing of it is left.
pub fn extract_key(raw_id: Option<&JsValue>, raw_key: Option<&JsValue>) -> Option<String> {
    if let Some(key) = text(raw_key).filter(|k| !js_trim(k).is_empty())
        && let Some(cleaned) = cleaned_id(&key)
    {
        return Some(cleaned);
    }
    match raw_id {
        Some(JsValue::String(id)) => {
            let id = id.to_string_lossy();
            if let Some(caps) = item_uri().exec(&id)
                && let Some(cleaned) = caps.get(1).and_then(cleaned_id)
            {
                return Some(cleaned);
            }
            let trimmed = js_trim(&id);
            if !trimmed.is_empty() && !web_url().test(trimmed) {
                return cleaned_id(trimmed);
            }
            None
        }
        Some(JsValue::Number(n)) if n.is_finite() => {
            Some(sanitize_citation_id(&crate::js::number_to_string(*n)))
        }
        _ => None,
    }
}

/// `ZOTERO_TYPE_MAP`: Zotero item types (lower-case) to CSL types.
const TYPE_MAP: [(&str, &str); 30] = [
    ("journalarticle", "article-journal"),
    ("magazinearticle", "article-magazine"),
    ("newspaperarticle", "article-newspaper"),
    ("book", "book"),
    ("editedbook", "book"),
    ("booksection", "chapter"),
    ("conferencepaper", "paper-conference"),
    ("webpage", "webpage"),
    ("blogpost", "post-weblog"),
    ("forumpost", "post"),
    ("thesis", "thesis"),
    ("report", "report"),
    ("encyclopediaarticle", "entry-encyclopedia"),
    ("dictionaryentry", "entry-dictionary"),
    ("manuscript", "manuscript"),
    ("document", "document"),
    ("patent", "patent"),
    ("statute", "legislation"),
    ("bill", "bill"),
    ("case", "legal_case"),
    ("presentation", "speech"),
    ("interview", "interview"),
    ("film", "motion_picture"),
    ("videorecording", "motion_picture"),
    ("audiorecording", "song"),
    ("podcast", "song"),
    ("artwork", "graphic"),
    ("dataset", "dataset"),
    ("computerprogram", "software"),
    ("preprint", "article"),
];

/// `resolveZoteroCslType`: a CSL `type` kept, else the item type's CSL type ("document" for one
/// unknown).
fn csl_type(item: &JsObject) -> Option<String> {
    if let Some(kind) = text(item.get("type")).filter(|t| !js_trim(t).is_empty()) {
        return Some(js_trim(&kind).to_owned());
    }
    let item_type = text(item.get("itemType")).filter(|t| !js_trim(t).is_empty())?;
    let normalized = js_trim(&item_type).to_lowercase();
    Some(
        TYPE_MAP
            .iter()
            .find(|(zotero, _)| *zotero == normalized)
            .map_or("document", |(_, csl)| csl)
            .to_owned(),
    )
}

/// `readCiteKey`.
fn cite_key(item: &JsObject) -> Option<String> {
    ["citeKey", "citationKey", "citation-key", "citekey"]
        .iter()
        .filter_map(|key| text(item.get(key)))
        .find(|k| !js_trim(k).is_empty())
        .map(|k| js_trim(&k).to_owned())
}

/// JavaScript truthiness.
fn truthy(value: Option<&JsValue>) -> bool {
    match value {
        None | Some(JsValue::Null) => false,
        Some(JsValue::Bool(b)) => *b,
        Some(JsValue::Number(n)) => *n != 0.0 && !n.is_nan(),
        Some(JsValue::String(s)) => !s.is_empty(),
        Some(JsValue::Array(_) | JsValue::Object(_)) => true,
    }
}

/// `normalizeRawZoteroEntry`: an item (or a Better BibTeX entry wrapping one) with its id,
/// type and cite key resolved, marked as Zotero's; attachments, notes and annotations left out.
fn normalize_entry(entry: &JsValue) -> Option<JsValue> {
    let entry = entry.as_object()?;
    let wrapper_key = cite_key(entry);
    let item = entry
        .get("item")
        .and_then(JsValue::as_object)
        .unwrap_or(entry);
    let item_type = text(item.get("itemType"))
        .map(|t| js_trim(&t).to_lowercase())
        .unwrap_or_default();
    if matches!(item_type.as_str(), "attachment" | "note" | "annotation") {
        return None;
    }
    let key = cite_key(item).or(wrapper_key);
    let zotero_key = extract_key(item.get("id"), item.get("key"))
        .or_else(|| extract_key(entry.get("id"), entry.get("key")));
    let id = zotero_key.or_else(|| key.as_deref().map(sanitize_citation_id));
    let kind = csl_type(item).or_else(|| {
        (truthy(item.get("title")) || truthy(item.get("author"))).then(|| "document".to_owned())
    });
    let mut out = item.clone();
    if let Some(id) = id.filter(|id| !id.is_empty()) {
        out.insert("id", JsValue::str(&id));
    }
    if let Some(kind) = kind.filter(|k| !k.is_empty()) {
        out.insert("type", JsValue::str(&kind));
    }
    if let Some(key) = key.filter(|k| !k.is_empty()) {
        out.insert("citeKey", JsValue::str(&key));
    }
    for (upper, lower) in [("URL", "url"), ("DOI", "doi"), ("ISBN", "isbn")] {
        if item.get(upper).is_none()
            && let Some(value @ JsValue::String(_)) = item.get(lower)
        {
            out.insert(upper, value.clone());
        }
    }
    out.insert("provider", JsValue::str("zotero"));
    Some(JsValue::Object(out))
}

/// `mapZoteroItems`: the citation items of a Zotero CSL-JSON answer or a Better BibTeX pick.
pub fn map_items(value: &JsValue) -> Vec<JsValue> {
    let entries: &[JsValue] = match value {
        JsValue::Array(entries) => entries,
        JsValue::Object(object) => match object.get("items") {
            Some(JsValue::Array(entries)) => entries,
            _ => std::slice::from_ref(value),
        },
        _ => return Vec::new(),
    };
    let prepared: Vec<JsValue> = entries.iter().filter_map(normalize_entry).collect();
    normalize_csl_items_detailed(&JsValue::Array(prepared)).items
}

/// `mapZoteroCslJson`: Zotero's answer as citation items.
pub fn map_csl_json(raw: &str) -> Result<Vec<JsValue>, ZoteroError> {
    let cleaned = js_trim(raw.strip_prefix('\u{feff}').unwrap_or(raw));
    if cleaned.is_empty() {
        return Ok(Vec::new());
    }
    let parsed = crate::js::parse(cleaned).map_err(|_| ZoteroError {
        code: ErrorCode::InvalidResponse,
        message: "Zotero returned an invalid JSON response.".to_owned(),
        // `JSON.parse`'s own message, as the browser's engine words it.
        detail: Some(
            crate::js::parse_error_message(cleaned).unwrap_or_else(|| "invalid JSON".to_owned()),
        ),
    })?;
    Ok(map_items(&parsed))
}

/// `ZoteroLocal.checkStatus`: whether Zotero answers, or why not.
pub fn check_status(transport: &dyn Fn(&str) -> Result<String, String>) -> Result<(), ZoteroError> {
    transport(PING_PATH)
        .map(|_| ())
        .map_err(|e| classify_error(&e))
}

/// `ZoteroLocal.search`.
pub fn search(
    transport: &dyn Fn(&str) -> Result<String, String>,
    query: &str,
) -> Result<Vec<JsValue>, ZoteroError> {
    let body = transport(&search_path(query, None)).map_err(|e| classify_error(&e))?;
    map_csl_json(&body)
}

/// `ZoteroLocal.tryPick`: the items chosen in Better BibTeX's picker, or `None` when it can't
/// be used (Better BibTeX missing, or anything else going wrong).
pub fn try_pick(transport: &dyn Fn(&str) -> Result<String, String>) -> Option<Vec<JsValue>> {
    let body = transport(CAYW_PATH).ok()?;
    if js_trim(&body).is_empty() {
        return Some(Vec::new());
    }
    map_csl_json(&body).ok()
}

/// Where Zotero listens.
pub const HOST: &str = "127.0.0.1:23119";
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;
const MAX_PATH_LEN: usize = 4096;

/// `validate_zotero_path`.
fn validate_path(path: &str) -> Result<(), String> {
    if !path.starts_with('/') || path.starts_with("//") {
        return Err("Zotero request path must start with a single '/'.".to_owned());
    }
    if path.len() > MAX_PATH_LEN {
        return Err("Zotero request path is too long.".to_owned());
    }
    if !path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-._~/?&=+%:@!$'()*,;".contains(&b))
    {
        return Err("Zotero request path contains disallowed characters.".to_owned());
    }
    Ok(())
}

/// The status line's code, the declared length, and whether the body is chunked.
fn parse_headers(head: &[u8]) -> Result<(u16, Option<usize>, bool), String> {
    let head = std::str::from_utf8(head)
        .map_err(|_| "Zotero returned invalid HTTP headers.".to_owned())?;
    let mut lines = head.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| "Zotero returned an empty HTTP response.".to_owned())?;
    let mut parts = status_line.split_whitespace();
    if !parts.next().unwrap_or_default().starts_with("HTTP/") {
        return Err("Zotero returned a malformed HTTP status line.".to_owned());
    }
    let status = parts
        .next()
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "Zotero returned an invalid HTTP status code.".to_owned())?;
    let mut length = None;
    let mut chunked = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "content-length"
                && let Ok(len) = value.parse::<usize>()
            {
                length = Some(len);
            } else if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked")
            {
                chunked = true;
            }
        }
    }
    Ok((status, length, chunked))
}

/// A chunked body, or `None` while it isn't complete.
fn decode_chunked(raw: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let Some(crlf) = raw[pos..].windows(2).position(|w| w == b"\r\n") else {
            return Ok(None);
        };
        let size_line = std::str::from_utf8(&raw[pos..pos + crlf])
            .map_err(|_| "Invalid chunked encoding from Zotero.".to_owned())?;
        let size_hex = size_line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| "Invalid chunk size from Zotero.".to_owned())?;
        pos += crlf + 2;
        if size == 0 {
            return Ok((raw.len() >= pos + 2).then_some(out));
        }
        if out.len().saturating_add(size) > MAX_RESPONSE_BYTES {
            return Err("Zotero response exceeded 5 MB limit.".to_owned());
        }
        if raw.len() < pos + size + 2 {
            return Ok(None);
        }
        out.extend_from_slice(&raw[pos..pos + size]);
        if &raw[pos + size..pos + size + 2] != b"\r\n" {
            return Err("Malformed chunk terminator from Zotero.".to_owned());
        }
        pos += size + 2;
    }
}

fn io_error(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::ConnectionRefused => {
            "Zotero is not running (connection refused on 127.0.0.1:23119).".to_owned()
        }
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
            "Zotero request timed out after 5s.".to_owned()
        }
        _ => format!("Could not reach Zotero: {error}"),
    }
}

/// `zotero_request`: a GET to Zotero on this computer; the body, or the error as ProseCraft's
/// desktop app words it ("HTTP 403: …" for an answer that isn't a success).
pub fn request(path: &str) -> Result<String, String> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 23119));
    request_to(addr, path)
}

/// [`request`] to another address (for tests).
pub fn request_to(addr: std::net::SocketAddr, path: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    validate_path(path)?;
    let deadline = std::time::Instant::now() + TIMEOUT;
    let mut stream =
        std::net::TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| io_error(&e))?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|e| io_error(&e))?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {HOST}\r\nUser-Agent: ProseCraft\r\nAccept: application/json, text/plain, */*\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| io_error(&e))?;
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut headers: Option<(usize, u16, Option<usize>, bool)> = None;
    let header_end = |raw: &[u8]| raw.windows(4).position(|w| w == b"\r\n\r\n");
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err("Zotero request timed out after 5s.".to_owned());
        }
        let _ = stream.set_read_timeout(Some(remaining));
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                if raw.len().saturating_add(read) > MAX_RESPONSE_BYTES {
                    return Err("Zotero response exceeded 5 MB limit.".to_owned());
                }
                raw.extend_from_slice(&chunk[..read]);
                if headers.is_none()
                    && let Some(end) = header_end(&raw)
                {
                    let (status, length, chunked) = parse_headers(&raw[..end])?;
                    if length.is_some_and(|len| len > MAX_RESPONSE_BYTES) {
                        return Err("Zotero response exceeded 5 MB limit.".to_owned());
                    }
                    headers = Some((end + 4, status, length, chunked));
                }
                if let Some((start, _, length, chunked)) = headers {
                    let body = &raw[start..];
                    if chunked {
                        if decode_chunked(body)?.is_some() {
                            break;
                        }
                    } else if length.is_some_and(|len| body.len() >= len) {
                        break;
                    }
                }
            }
            Err(error) => return Err(io_error(&error)),
        }
    }
    let (start, status, length, chunked) = match headers {
        Some(headers) => headers,
        None => {
            let end = header_end(&raw)
                .ok_or_else(|| "Zotero returned an incomplete HTTP response.".to_owned())?;
            let (status, length, chunked) = parse_headers(&raw[..end])?;
            (end + 4, status, length, chunked)
        }
    };
    let body = &raw[start..];
    let body = if chunked {
        decode_chunked(body)?.ok_or_else(|| {
            "Zotero closed the connection before completing chunked body.".to_owned()
        })?
    } else if let Some(len) = length {
        if body.len() < len {
            return Err(
                "Zotero closed the connection before sending the full response.".to_owned(),
            );
        }
        body[..len].to_vec()
    } else {
        body.to_vec()
    };
    if body.len() > MAX_RESPONSE_BYTES {
        return Err("Zotero response exceeded 5 MB limit.".to_owned());
    }
    if (200..300).contains(&status) {
        String::from_utf8(body).map_err(|_| "Zotero response was not valid UTF-8.".to_owned())
    } else {
        let message = String::from_utf8_lossy(&body).trim().to_owned();
        if message.is_empty() {
            Err(format!("HTTP {status}"))
        } else {
            Err(format!("HTTP {status}: {message}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-shot server answering one request with `answer`.
    fn serve(answer: Vec<u8>) -> std::net::SocketAddr {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(&answer);
            }
        });
        addr
    }

    #[test]
    fn requests_read_lengths_chunks_and_errors() {
        let ok = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello trailing".to_vec());
        assert_eq!(request_to(ok, PING_PATH), Ok("hello".to_owned()));
        let chunked = serve(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n[1,\r\n2;x\r\n2]\r\n0\r\n\r\n"
                .to_vec(),
        );
        assert_eq!(request_to(chunked, "/a"), Ok("[1,2]".to_owned()));
        let forbidden = serve(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 25\r\n\r\nLocal API is not enabled\n"
                .to_vec(),
        );
        assert_eq!(
            request_to(forbidden, "/a"),
            Err("HTTP 403: Local API is not enabled".to_owned())
        );
        let closed = serve(b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\nshort".to_vec());
        assert_eq!(
            request_to(closed, "/a"),
            Err("Zotero closed the connection before sending the full response.".to_owned())
        );
        // Nothing listening: a port that was just free.
        let free = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .expect("a free port");
        let refused = request_to(free, "/a").expect_err("refused");
        assert_eq!(classify_error(&refused).code, ErrorCode::NotRunning);
        assert_eq!(
            request_to(free, "no-slash"),
            Err("Zotero request path must start with a single '/'.".to_owned())
        );
        assert_eq!(
            request_to(free, "/a b"),
            Err("Zotero request path contains disallowed characters.".to_owned())
        );
    }
}
