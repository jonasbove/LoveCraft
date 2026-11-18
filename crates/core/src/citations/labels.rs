//! The labels the editor shows for citation markers (ProseCraft `renderCitationLabels` in
//! `src/lib/citations/cite-marker.ts`, upstream `7dae544`): each marker's sources formatted in
//! the publish style, with sources missing from the library kept as `[@key]`.

use std::collections::HashMap;

use super::bibliography::Lookup;
use super::format::{CitationContext, CitationInput, compute_disambiguation, format_citation};
use crate::cite::{CiteRefItem, parse_cite_data};
use crate::html::{Node, parse_fragment};
use crate::js::JsValue;

fn id_of(item: &JsValue) -> String {
    item.get("id")
        .and_then(JsValue::as_js_string)
        .map(|s| s.to_string_lossy())
        .unwrap_or_default()
}

/// A marker's sources, from its `data-cites` (`None` when that isn't valid).
fn cites_of(marker: &str) -> Option<Vec<CiteRefItem>> {
    let raw = parse_fragment(marker).into_iter().find_map(|n| match n {
        Node::Element(e) => e.attr("data-cites").map(str::to_owned),
        Node::Text(_) => None,
    });
    parse_cite_data(raw.as_deref())
}

/// `buildNumericIndices`: sources numbered in the order the markers first cite them.
fn numeric_indices(markers: &[Option<Vec<CiteRefItem>>], lookup: &Lookup) -> HashMap<String, f64> {
    let mut indices = HashMap::new();
    for cite in markers.iter().flatten().flatten() {
        if let Some(item) = lookup.resolve(&cite.id) {
            let next = indices.len() as f64 + 1.0;
            indices.entry(id_of(item)).or_insert(next);
        }
    }
    indices
}

/// `renderCitationLabels`: each marker's label (its outer HTML, in document order), `None`
/// for a marker whose `data-cites` isn't valid.
pub fn citation_labels(
    markers: &[&str],
    items: &[JsValue],
    style: &str,
    locale: &str,
) -> Vec<Option<String>> {
    if markers.is_empty() {
        return Vec::new();
    }
    let parsed: Vec<Option<Vec<CiteRefItem>>> = markers.iter().map(|m| cites_of(m)).collect();
    let lookup = Lookup::new(items);
    let numeric = super::styles::styles()
        .into_iter()
        .any(|s| s.id == style && s.kind == "numeric");
    let context = CitationContext {
        indices: numeric.then(|| numeric_indices(&parsed, &lookup)),
        locale: locale.to_owned(),
        year_suffixes: Some(compute_disambiguation(style, items, locale)),
        ..CitationContext::default()
    };
    parsed
        .iter()
        .map(|cites| {
            cites
                .as_ref()
                .map(|cites| label(cites, &lookup, style, &context))
        })
        .collect()
}

fn label(cites: &[CiteRefItem], lookup: &Lookup, style: &str, context: &CitationContext) -> String {
    let mut inputs = Vec::new();
    let mut unresolved = Vec::new();
    for cite in cites {
        let Some(item) = lookup.resolve(&cite.id) else {
            unresolved.push(format!("@{}", cite.id));
            continue;
        };
        inputs.push(CitationInput {
            item: item.clone(),
            locator: cite.locator.clone().filter(|l| !l.is_empty()),
            label: cite.label.clone().filter(|l| !l.is_empty()),
            prefix: cite.prefix.clone().filter(|p| !p.is_empty()),
            suffix: cite.suffix.clone().filter(|s| !s.is_empty()),
            suppress_author: cite.suppress_author,
        });
    }
    if inputs.is_empty() {
        let keys: Vec<String> = cites.iter().map(|c| format!("@{}", c.id)).collect();
        return format!("[{}]", keys.join("; "));
    }
    let text = format_citation(style, &inputs, context).text;
    if unresolved.is_empty() {
        text
    } else {
        format!("{text} [{}]", unresolved.join("; "))
    }
}

/// [`citation_labels`] remembered for the inputs it was last asked with: the editor asks every
/// frame, and working the labels out (year suffixes across the library) takes milliseconds
/// for a large library.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LabelCache {
    key: Option<(Vec<String>, Vec<JsValue>, String, String)>,
    labels: Vec<Option<String>>,
}

impl LabelCache {
    /// The labels, worked out again only when the markers, library, style or locale changed.
    pub fn labels(
        &mut self,
        markers: &[&str],
        items: &[JsValue],
        style: &str,
        locale: &str,
    ) -> Vec<Option<String>> {
        let same = self.key.as_ref().is_some_and(|(m, i, s, l)| {
            s == style
                && l == locale
                && m.len() == markers.len()
                && m.iter().zip(markers).all(|(a, b)| a == b)
                && i.as_slice() == items
        });
        if !same {
            self.labels = citation_labels(markers, items, style, locale);
            self.key = Some((
                markers.iter().map(|m| (*m).to_owned()).collect(),
                items.to_vec(),
                style.to_owned(),
                locale.to_owned(),
            ));
        }
        self.labels.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(id: &str, year: f64) -> JsValue {
        crate::js::parse(&format!(
            r#"{{"id":"{id}","type":"book","title":"T","author":[{{"family":"Smith"}}],"issued":{{"date-parts":[[{year}]]}}}}"#
        ))
        .unwrap_or(JsValue::Null)
    }

    #[test]
    fn the_cache_follows_its_inputs() {
        let marker = crate::cite::cite_marker_html(&[CiteRefItem {
            id: "a".into(),
            ..CiteRefItem::default()
        }])
        .unwrap_or_default();
        let mut cache = LabelCache::default();
        let one = [source("a", 2020.0)];
        let two = [source("a", 2020.0), source("b", 2020.0)];
        assert_eq!(
            cache.labels(&[&marker], &one, "apa7", "en"),
            [Some("(Smith, 2020)".to_owned())]
        );
        // Another source of the same author and year: the citation needs its suffix now.
        assert_eq!(
            cache.labels(&[&marker], &two, "apa7", "en"),
            citation_labels(&[&marker], &two, "apa7", "en")
        );
        assert_ne!(
            cache.labels(&[&marker], &two, "apa7", "en"),
            [Some("(Smith, 2020)".to_owned())]
        );
        assert_eq!(
            cache.labels(&[&marker], &one, "ieee", "en"),
            citation_labels(&[&marker], &one, "ieee", "en")
        );
        assert_eq!(
            cache.labels(&[], &one, "ieee", "en"),
            Vec::<Option<String>>::new()
        );
    }
}
