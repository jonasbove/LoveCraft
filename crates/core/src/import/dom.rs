//! A small document tree for reading EPUB content documents as ProseCraft's `DOMParser` does:
//! XHTML read as XML (namespace-aware and strict, so a document the browser's XML parser
//! rejects falls back to HTML), elements that keep their identity, and `outerHTML` written as
//! the browser serializes an XML document (or an HTML one, for the fallback).

use crate::html::{Node, parse_fragment};

pub(crate) const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";

/// Elements HTML writes without an end tag.
const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

#[derive(Clone, Debug)]
pub(crate) enum Kind {
    Document,
    Element {
        /// The name as written (`epub:switch`, `svg`).
        name: String,
        local: String,
        ns: Option<String>,
        attrs: Vec<(String, String)>,
    },
    Text(String),
}

#[derive(Clone, Debug)]
pub(crate) struct DNode {
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub kind: Kind,
}

/// A document: node 0 is the document itself.
#[derive(Clone, Debug)]
pub(crate) struct Dom {
    pub nodes: Vec<DNode>,
    /// Read as XML (serialized as XML), or as HTML.
    pub xml: bool,
}

fn local_of(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, l)| l)
}

fn prefix_of(name: &str) -> Option<&str> {
    name.split_once(':').map(|(p, _)| p)
}

impl Dom {
    fn empty(xml: bool) -> Self {
        Self {
            nodes: vec![DNode {
                parent: None,
                children: Vec::new(),
                kind: Kind::Document,
            }],
            xml,
        }
    }

    pub fn add(&mut self, parent: Option<usize>, kind: Kind) -> usize {
        let id = self.nodes.len();
        self.nodes.push(DNode {
            parent,
            children: Vec::new(),
            kind,
        });
        if let Some(p) = parent {
            self.nodes[p].children.push(id);
        }
        id
    }

    /// `parseFromString(text, "application/xml")`: `None` where the browser reports a parser
    /// error (malformed XML, an unknown entity, an undeclared prefix).
    pub fn from_xml(bytes: &[u8]) -> Option<Self> {
        let options = craft_xml::Options {
            ns_decls: true,
            ..craft_xml::Options::default()
        }
        .strict();
        let mut events = options.events(bytes);
        let mut dom = Self::empty(true);
        let mut stack: Vec<usize> = vec![0];
        // In-scope namespace bindings: (prefix, uri), "" for the default namespace.
        let mut scopes: Vec<Vec<(String, String)>> = vec![vec![(
            "xml".into(),
            "http://www.w3.org/XML/1998/namespace".into(),
        )]];
        let lookup = |scopes: &[Vec<(String, String)>], prefix: &str| -> Option<String> {
            scopes
                .iter()
                .rev()
                .flat_map(|s| s.iter().rev())
                .find(|(p, _)| p == prefix)
                .map(|(_, u)| u.clone())
        };
        let mut seen_root = false;
        loop {
            match events.next() {
                Err(_) => return None,
                Ok(None) => break,
                Ok(Some(event)) => match event {
                    craft_xml::Event::Start { name, attrs } => {
                        if stack.len() == 1 && seen_root {
                            return None;
                        }
                        seen_root = true;
                        let mut scope = Vec::new();
                        for (key, value) in &attrs {
                            if key == "xmlns" {
                                scope.push((String::new(), value.clone()));
                            } else if let Some(p) = key.strip_prefix("xmlns:") {
                                scope.push((p.to_owned(), value.clone()));
                            }
                        }
                        scopes.push(scope);
                        let ns = match prefix_of(&name) {
                            Some(p) => Some(lookup(&scopes, p)?),
                            None => lookup(&scopes, "").filter(|u| !u.is_empty()),
                        };
                        for (key, _) in &attrs {
                            if let Some(p) = prefix_of(key)
                                && p != "xmlns"
                                && lookup(&scopes, p).is_none()
                            {
                                return None;
                            }
                        }
                        let parent = *stack.last()?;
                        let id = dom.add(
                            Some(parent),
                            Kind::Element {
                                local: local_of(&name).to_owned(),
                                name,
                                ns,
                                attrs,
                            },
                        );
                        stack.push(id);
                    }
                    craft_xml::Event::End { .. } => {
                        stack.pop();
                        scopes.pop();
                    }
                    craft_xml::Event::Text(text) | craft_xml::Event::CData(text) => {
                        let parent = *stack.last()?;
                        if parent == 0 {
                            continue; // whitespace around the root
                        }
                        match dom.nodes[parent].children.last().copied() {
                            Some(last) if matches!(dom.nodes[last].kind, Kind::Text(_)) => {
                                if let Kind::Text(t) = &mut dom.nodes[last].kind {
                                    t.push_str(&text);
                                }
                            }
                            _ => {
                                dom.add(Some(parent), Kind::Text(text));
                            }
                        }
                    }
                    craft_xml::Event::Pi { .. } | craft_xml::Event::Comment(_) => {}
                },
            }
        }
        seen_root.then_some(dom)
    }

    /// `parseFromString(text, "text/html")` as far as this reader needs: the tree of the page.
    pub fn from_html(text: &str) -> Self {
        let mut dom = Self::empty(false);
        fn add_nodes(dom: &mut Dom, parent: usize, nodes: Vec<Node>) {
            for node in nodes {
                match node {
                    Node::Text(t) => {
                        dom.add(Some(parent), Kind::Text(t));
                    }
                    Node::Element(e) => {
                        let attrs = e.attrs.clone();
                        let id = dom.add(
                            Some(parent),
                            Kind::Element {
                                local: e.name.clone(),
                                name: e.name.clone(),
                                ns: None,
                                attrs,
                            },
                        );
                        add_nodes(dom, id, e.children);
                    }
                }
            }
        }
        add_nodes(&mut dom, 0, parse_fragment(text));
        dom
    }

    pub fn local(&self, id: usize) -> &str {
        match &self.nodes[id].kind {
            Kind::Element { local, .. } => local,
            _ => "",
        }
    }

    pub fn is_element(&self, id: usize) -> bool {
        matches!(self.nodes[id].kind, Kind::Element { .. })
    }

    pub fn attr(&self, id: usize, key: &str) -> Option<&str> {
        match &self.nodes[id].kind {
            Kind::Element { attrs, .. } => attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    pub fn set_attr(&mut self, id: usize, key: &str, value: &str) {
        if let Kind::Element { attrs, .. } = &mut self.nodes[id].kind {
            match attrs.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = value.to_owned(),
                None => attrs.push((key.to_owned(), value.to_owned())),
            }
        }
    }

    /// Element children.
    pub fn child_elements(&self, id: usize) -> Vec<usize> {
        self.nodes[id]
            .children
            .iter()
            .copied()
            .filter(|c| self.is_element(*c))
            .collect()
    }

    /// Every node under `id` (not `id`), in document order.
    pub fn descendants(&self, id: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack: Vec<usize> = self.nodes[id].children.iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.nodes[n].children.iter().rev().copied());
        }
        out
    }

    /// Descendant elements named `local` (`getElementsByTagName` by local name).
    pub fn elements(&self, id: usize, local: &str) -> Vec<usize> {
        self.descendants(id)
            .into_iter()
            .filter(|n| self.is_element(*n) && self.local(*n) == local)
            .collect()
    }

    /// A descendant element named one of `names` (`querySelector("img, svg")`).
    pub fn has_descendant(&self, id: usize, names: &[&str]) -> bool {
        self.descendants(id)
            .into_iter()
            .any(|n| self.is_element(n) && names.contains(&self.local(n)))
    }

    pub fn text_content(&self, id: usize) -> String {
        match &self.nodes[id].kind {
            Kind::Text(t) => t.clone(),
            _ => self
                .descendants(id)
                .into_iter()
                .filter_map(|n| match &self.nodes[n].kind {
                    Kind::Text(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect(),
        }
    }

    /// `a.contains(b)`: `b` is `a` or inside it.
    pub fn contains(&self, a: usize, b: usize) -> bool {
        let mut at = Some(b);
        while let Some(n) = at {
            if n == a {
                return true;
            }
            at = self.nodes[n].parent;
        }
        false
    }

    /// The node's place in document order (only nodes in the tree).
    pub fn order(&self) -> Vec<usize> {
        let mut rank = vec![usize::MAX; self.nodes.len()];
        rank[0] = 0;
        for (i, n) in self.descendants(0).into_iter().enumerate() {
            rank[n] = i + 1;
        }
        rank
    }

    /// `getElementById`.
    pub fn element_by_id(&self, id: &str) -> Option<usize> {
        self.descendants(0)
            .into_iter()
            .find(|n| self.is_element(*n) && self.attr(*n, "id") == Some(id))
    }

    /// The `body` element: for XML, the XHTML `body` child of the root `html`.
    pub fn body(&self) -> Option<usize> {
        if self.xml {
            let root = *self.child_elements(0).first()?;
            if self.local(root) != "html" {
                return None;
            }
            self.child_elements(root).into_iter().find(|c| self.local(*c) == "body" && matches!(&self.nodes[*c].kind, Kind::Element { ns: Some(ns), .. } if ns == XHTML_NS))
        } else {
            self.elements(0, "body").first().copied()
        }
    }

    pub fn document_element(&self) -> Option<usize> {
        self.child_elements(0).first().copied()
    }

    /// An element made by `ownerDocument.createElement` (in the XHTML namespace for XHTML).
    pub fn create(&mut self, local: &str) -> usize {
        let ns = self.xml.then(|| XHTML_NS.to_owned());
        self.add(
            None,
            Kind::Element {
                name: local.to_owned(),
                local: local.to_owned(),
                ns,
                attrs: Vec::new(),
            },
        )
    }

    pub fn detach(&mut self, id: usize) {
        if let Some(p) = self.nodes[id].parent.take() {
            self.nodes[p].children.retain(|c| *c != id);
        }
    }

    /// `old.replaceWith(new)`.
    pub fn replace(&mut self, old: usize, new: usize) {
        if let Some(p) = self.nodes[old].parent {
            if let Some(slot) = self.nodes[p].children.iter_mut().find(|c| **c == old) {
                *slot = new;
            }
            self.nodes[new].parent = Some(p);
            self.nodes[old].parent = None;
        }
    }

    /// `outerHTML`.
    pub fn outer_html(&self, id: usize) -> String {
        let mut out = String::new();
        if self.xml {
            self.write_xml(id, None, &mut out);
        } else {
            self.write_html(id, &mut out);
        }
        out
    }

    fn write_html(&self, id: usize, out: &mut String) {
        match &self.nodes[id].kind {
            Kind::Text(t) => out.push_str(&escape(t, false)),
            Kind::Document => {
                for c in &self.nodes[id].children {
                    self.write_html(*c, out);
                }
            }
            Kind::Element { name, attrs, .. } => {
                out.push('<');
                out.push_str(name);
                for (k, v) in attrs {
                    out.push_str(&format!(" {k}=\"{}\"", escape(v, true)));
                }
                out.push('>');
                if VOID.contains(&name.as_str()) {
                    return;
                }
                for c in &self.nodes[id].children {
                    self.write_html(*c, out);
                }
                out.push_str(&format!("</{name}>"));
            }
        }
    }

    /// XML serialization (DOM Parsing): a namespace declared where it changes; an empty XHTML
    /// void element as `<br />`, another empty XHTML element with its end tag, an empty
    /// element of another namespace as `<x/>`.
    fn write_xml(&self, id: usize, context_ns: Option<&str>, out: &mut String) {
        match &self.nodes[id].kind {
            Kind::Text(t) => out.push_str(&escape(t, false)),
            Kind::Document => {
                for c in &self.nodes[id].children {
                    self.write_xml(*c, context_ns, out);
                }
            }
            Kind::Element {
                name,
                local,
                ns,
                attrs,
            } => {
                out.push('<');
                out.push_str(name);
                let prefixed = name.contains(':');
                let mut declared = false;
                for (k, v) in attrs {
                    if k == "xmlns" {
                        declared = true;
                    }
                    out.push_str(&format!(" {k}=\"{}\"", escape(v, true)));
                }
                if !prefixed && !declared && ns.as_deref() != context_ns {
                    out.push_str(&format!(" xmlns=\"{}\"", ns.as_deref().unwrap_or_default()));
                }
                let children = &self.nodes[id].children;
                let xhtml = ns.as_deref() == Some(XHTML_NS);
                if children.is_empty() {
                    if xhtml && VOID.contains(&local.as_str()) {
                        out.push_str(" />");
                        return;
                    }
                    if !xhtml {
                        out.push_str("/>");
                        return;
                    }
                }
                out.push('>');
                for c in children {
                    self.write_xml(*c, ns.as_deref(), out);
                }
                out.push_str(&format!("</{name}>"));
            }
        }
    }
}

fn escape(s: &str, attr: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            '\u{a0}' if !attr => out.push_str("&nbsp;"),
            c => out.push(c),
        }
    }
    out
}
