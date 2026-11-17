//! Citation file import (ProseCraft `src/lib/citations/import.ts`, upstream `7dae544`): the
//! format of a file (CSL-JSON, BibTeX/BibLaTeX or RIS) told from its name or content, its
//! entries read and checked as library items, and warnings for what was left out.

use super::bibtex::parse_bibtex;
use super::ris::parse_ris;
use crate::csl::normalize_csl_items_detailed;
use crate::html::is_js_whitespace;
use crate::js::regexp::Re;
use crate::js::{self, JsValue};
use std::sync::OnceLock;

/// `CitationImportFormat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitationImportFormat {
    CslJson,
    Bibtex,
    Ris,
}

impl CitationImportFormat {
    /// The name ProseCraft uses: `csl-json`, `bibtex` or `ris`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CslJson => "csl-json",
            Self::Bibtex => "bibtex",
            Self::Ris => "ris",
        }
    }
}

/// `CitationImportResult`: the format found (if any), the items read, and warnings.
#[derive(Clone, Debug, PartialEq)]
pub struct CitationImport {
    pub format: Option<CitationImportFormat>,
    pub items: Vec<JsValue>,
    pub warnings: Vec<String>,
}

fn ris_start() -> &'static Re {
    static RULE: OnceLock<Re> = OnceLock::new();
    RULE.get_or_init(|| Re::new(r"^TY\s{1,2}-", "m"))
}

fn bibtex_entry() -> &'static Re {
    static RULE: OnceLock<Re> = OnceLock::new();
    RULE.get_or_init(|| Re::new(r"@[A-Za-z]+\s*[{(]", ""))
}

/// `detectFormatFromExtension`.
fn format_from_extension(filename: &str) -> Option<CitationImportFormat> {
    let lower = filename.trim_matches(is_js_whitespace).to_lowercase();
    if lower.is_empty() {
        return None;
    }
    if [".json", ".csljson", ".csl"]
        .iter()
        .any(|e| lower.ends_with(e))
    {
        return Some(CitationImportFormat::CslJson);
    }
    if [".bib", ".bibtex"].iter().any(|e| lower.ends_with(e)) {
        return Some(CitationImportFormat::Bibtex);
    }
    lower.ends_with(".ris").then_some(CitationImportFormat::Ris)
}

/// `detectFormatFromContent`.
fn format_from_content(text: &str) -> Option<CitationImportFormat> {
    let trimmed = text.trim_start_matches(is_js_whitespace);
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        return Some(CitationImportFormat::CslJson);
    }
    if ris_start().test(trimmed) {
        return Some(CitationImportFormat::Ris);
    }
    bibtex_entry()
        .test(trimmed)
        .then_some(CitationImportFormat::Bibtex)
}

fn failed(format: Option<CitationImportFormat>, warning: String) -> CitationImport {
    CitationImport {
        format,
        items: Vec::new(),
        warnings: vec![warning],
    }
}

/// The CSL-JSON branch: an array of items, `{ items: [...] }`, or a single item.
fn import_csl_json(cleaned: &str) -> CitationImport {
    let format = Some(CitationImportFormat::CslJson);
    let parsed = match js::parse(cleaned) {
        Ok(parsed) => parsed,
        Err(error) => {
            let detail = js::parse_error_message(cleaned).unwrap_or_else(|| error.to_string());
            return failed(format, format!("Invalid CSL-JSON input: {detail}"));
        }
    };
    let raw_entries = match &parsed {
        JsValue::Array(_) => parsed.clone(),
        JsValue::Object(object) => match (object.get("items"), object.get("type")) {
            (Some(items @ JsValue::Array(_)), _) => items.clone(),
            (_, Some(JsValue::String(_))) => JsValue::Array(vec![parsed.clone()]),
            _ => {
                return failed(
                    format,
                    "Expected a CSL-JSON array of citation items.".into(),
                );
            }
        },
        _ => {
            return failed(
                format,
                "Expected a CSL-JSON array of citation items.".into(),
            );
        }
    };
    let normalized = normalize_csl_items_detailed(&raw_entries);
    CitationImport {
        format,
        items: normalized.items,
        warnings: normalized.warnings,
    }
}

/// `importCitations`: the citation items of a file's `text`, its format told from `filename`
/// (`.json`/`.csljson`/`.csl`, `.bib`/`.bibtex`, `.ris`) or else from the text, each item
/// checked and given a unique id, and the warnings for what was skipped.
pub fn import_citations(text: &str, filename: &str) -> CitationImport {
    let cleaned = text.strip_prefix('\u{feff}').unwrap_or(text);
    let ext_format = format_from_extension(filename);
    if cleaned.trim_matches(is_js_whitespace).is_empty() {
        return failed(ext_format, "Citation input is empty.".into());
    }
    let Some(format) = ext_format.or_else(|| format_from_content(cleaned)) else {
        return failed(
            None,
            "Could not detect citation format (expected CSL-JSON, BibTeX, or RIS).".into(),
        );
    };
    let (items, mut warnings) = match format {
        CitationImportFormat::CslJson => return import_csl_json(cleaned),
        CitationImportFormat::Bibtex => {
            let parsed = parse_bibtex(cleaned);
            (parsed.items, parsed.warnings)
        }
        CitationImportFormat::Ris => {
            let parsed = parse_ris(cleaned);
            (parsed.items, parsed.warnings)
        }
    };
    let normalized = normalize_csl_items_detailed(&JsValue::Array(items));
    warnings.extend(normalized.warnings);
    CitationImport {
        format: Some(format),
        items: normalized.items,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rules_compile() {
        assert!(super::ris_start().compiled());
        assert!(super::bibtex_entry().compiled());
    }
}
