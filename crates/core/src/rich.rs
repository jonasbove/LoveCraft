//! Equations, diagrams and calculations drawn (ProseCraft's `src/lib/rich/render.ts`,
//! `prepare.ts`, `raster.ts` and the markup of `dom.ts`).
//!
//! ProseCraft typesets TeX with MathJax. Here it is laid out by RaTeX (KaTeX's rules and its
//! fonts) into SVG, which resvg rasterizes when an export needs a picture. A calculation is
//! its worksheet's LaTeX (`worksheetToLatex`). Mermaid diagrams are laid out by
//! mermaid-rs-renderer with Mermaid's neutral theme, as ProseCraft configures mermaid.js; their
//! labels are drawn in the fonts the app gives ([`set_fonts`]).

use std::sync::{Mutex, OnceLock};

use base64::Engine;

use crate::html::{Element, Node, parse_fragment, serialize};

/// `MAX_MATH_SOURCE_LENGTH`, `MAX_DIAGRAM_SOURCE_LENGTH`, `MAX_CALC_SOURCE_LENGTH`.
pub const MAX_MATH_SOURCE_LENGTH: usize = 4_000;
pub const MAX_DIAGRAM_SOURCE_LENGTH: usize = 20_000;

/// `RichKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RichKind {
    MathInline,
    MathBlock,
    Diagram,
    Calc,
}

impl RichKind {
    /// `richKindOf`: the first class it has.
    pub fn of(element: &Element) -> Option<Self> {
        if element.has_class("math-inline") {
            Some(Self::MathInline)
        } else if element.has_class("math-block") {
            Some(Self::MathBlock)
        } else if element.has_class("mermaid-diagram") {
            Some(Self::Diagram)
        } else if element.has_class("calc-worksheet") {
            Some(Self::Calc)
        } else {
            None
        }
    }

    fn attribute(self) -> &'static str {
        match self {
            Self::Diagram | Self::Calc => "data-source",
            _ => "data-tex",
        }
    }

    fn limit(self) -> usize {
        match self {
            Self::Diagram => MAX_DIAGRAM_SOURCE_LENGTH,
            Self::Calc => crate::calc::MAX_SOURCE_LENGTH,
            _ => MAX_MATH_SOURCE_LENGTH,
        }
    }
}

/// `readRichPlaceholder`: the kind and its source (trimmed, cut to the kind's limit).
pub fn read_placeholder(element: &Element) -> Option<(RichKind, String)> {
    let kind = RichKind::of(element)?;
    let source = element
        .attr(kind.attribute())
        .unwrap_or_default()
        .trim_matches(crate::html::is_js_whitespace);
    if source.is_empty() {
        return None;
    }
    let units: Vec<u16> = source.encode_utf16().take(kind.limit()).collect();
    Some((kind, String::from_utf16_lossy(&units)))
}

/// How a rendering is wanted (`RichRenderOptions`): SVG for HTML, a PNG otherwise.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderOptions {
    pub png: bool,
    /// Ink for pictures (`color`).
    pub color: String,
    /// The body size in points, which sizes equations (`bodySizePt`).
    pub body_size_pt: f64,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            png: false,
            color: "#111111".into(),
            body_size_pt: 12.0,
        }
    }
}

/// `RenderedRich`.
#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    pub kind: RichKind,
    pub source: String,
    pub alt: String,
    pub width: f64,
    pub height: f64,
    pub svg: Option<String>,
    pub data_url: Option<String>,
}

fn first_line(source: &str, skip: impl Fn(&str) -> bool) -> Option<String> {
    source
        .split('\n')
        .map(|l| {
            l.strip_suffix('\r')
                .unwrap_or(l)
                .trim_matches(crate::html::is_js_whitespace)
        })
        .find(|l| !l.is_empty() && !skip(l))
        .map(|l| l.encode_utf16().take(160).collect::<Vec<_>>())
        .map(|u| String::from_utf16_lossy(&u))
}

/// `calcAlt`.
fn calc_alt(source: &str) -> String {
    first_line(source, |l| l.starts_with('#'))
        .map_or("Calculation".into(), |l| format!("Calculation: {l}"))
}

/// `diagramAlt`.
pub fn diagram_alt(source: &str) -> String {
    first_line(source, |l| l.starts_with("%%") || l.starts_with("---"))
        .map_or("Diagram".into(), |l| format!("Diagram: {l}"))
}

/// The equation laid out and written as SVG, `em` pixels to the em; its width and height.
fn math_svg(
    tex: &str,
    display: bool,
    em: f64,
    max_length: usize,
) -> Result<(String, f64, f64), String> {
    let source = tex.trim_matches(crate::html::is_js_whitespace);
    if source.is_empty() {
        return Err("Add some LaTeX to render an equation.".into());
    }
    if source.encode_utf16().count() > max_length {
        return Err("That equation is too large to render.".into());
    }
    let syntax = || "Could not render that equation. Check the LaTeX syntax.".to_owned();
    let nodes = ratex_parser::parse(source).map_err(|_| syntax())?;
    let options = ratex_layout::LayoutOptions {
        style: if display {
            ratex_types::math_style::MathStyle::Display
        } else {
            ratex_types::math_style::MathStyle::Text
        },
        ..Default::default()
    };
    let laid = ratex_layout::layout(&nodes, &options);
    let list = ratex_layout::to_display_list(&laid);
    let svg = ratex_svg::render_to_svg(
        &list,
        &ratex_svg::SvgOptions {
            font_size: em,
            padding: 0.0,
            embed_glyphs: true,
            ..Default::default()
        },
    );
    let size = |name: &str| {
        let at = svg.find(&format!(" {name}=\""))? + name.len() + 3;
        let end = svg[at..].find('"')?;
        svg[at..at + end].trim_end_matches("pt").parse::<f64>().ok()
    };
    let (width, height) = (
        size("width").ok_or_else(syntax)?,
        size("height").ok_or_else(syntax)?,
    );
    // Pixels, as MathJax's sizes are used; ProseCraft's ink is the text's (`currentColor`).
    let svg = svg
        .replacen(
            &format!("width=\"{}pt\"", fmt(width)),
            &format!("width=\"{}\"", fmt(width)),
            1,
        )
        .replacen(
            &format!("height=\"{}pt\"", fmt(height)),
            &format!("height=\"{}\"", fmt(height)),
            1,
        )
        .replace("rgba(0,0,0,1)", "currentColor")
        .replacen("<svg ", "<svg role=\"img\" focusable=\"false\" ", 1);
    Ok((svg, width.max(1.0), height.max(1.0)))
}

/// RaTeX's own number format for the SVG's sizes.
fn fmt(value: f64) -> String {
    let mut out = String::new();
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{value}"));
    out
}

/// The fonts drawings' text is set in (the app's own faces; a diagram's labels name common
/// sans-serif families, which all resolve to the first face given).
pub fn set_fonts(faces: &[&[u8]]) {
    let mut db = resvg::usvg::fontdb::Database::new();
    for face in faces {
        db.load_font_data(face.to_vec());
    }
    let family = db
        .faces()
        .next()
        .and_then(|f| f.families.first().map(|(name, _)| name.clone()));
    if let Some(family) = family {
        db.set_sans_serif_family(family.clone());
        db.set_serif_family(family);
    }
    if let Ok(mut fonts) = fonts().lock() {
        *fonts = std::sync::Arc::new(db);
    }
}

fn fonts() -> &'static Mutex<std::sync::Arc<resvg::usvg::fontdb::Database>> {
    static FONTS: OnceLock<Mutex<std::sync::Arc<resvg::usvg::fontdb::Database>>> = OnceLock::new();
    FONTS.get_or_init(Default::default)
}

/// usvg's options with the drawings' fonts; every family a diagram names falls back to them.
fn svg_options() -> resvg::usvg::Options<'static> {
    let mut options = resvg::usvg::Options::default();
    if let Ok(db) = fonts().lock() {
        options.fontdb = db.clone();
        if let Some(family) = db
            .faces()
            .next()
            .and_then(|f| f.families.first().map(|(n, _)| n.clone()))
        {
            options.font_family = family;
        }
    }
    let fallback = options.font_family.clone();
    options.font_resolver.select_font = Box::new(move |font, db| {
        let mut families: Vec<resvg::usvg::fontdb::Family<'_>> = font
            .families()
            .iter()
            .map(|f| match f {
                resvg::usvg::FontFamily::Serif => resvg::usvg::fontdb::Family::Serif,
                resvg::usvg::FontFamily::SansSerif => resvg::usvg::fontdb::Family::SansSerif,
                resvg::usvg::FontFamily::Cursive => resvg::usvg::fontdb::Family::Cursive,
                resvg::usvg::FontFamily::Fantasy => resvg::usvg::fontdb::Family::Fantasy,
                resvg::usvg::FontFamily::Monospace => resvg::usvg::fontdb::Family::Monospace,
                resvg::usvg::FontFamily::Named(name) => resvg::usvg::fontdb::Family::Name(name),
            })
            .collect();
        families.push(resvg::usvg::fontdb::Family::Name(&fallback));
        let query = resvg::usvg::fontdb::Query {
            families: &families,
            weight: resvg::usvg::fontdb::Weight(font.weight()),
            ..Default::default()
        };
        db.query(&query).or_else(|| db.faces().next().map(|f| f.id))
    });
    options
}

/// `renderDiagramSvg`: the diagram as SVG sized for the page (at most 680 × 900).
fn diagram_svg(source: &str) -> Result<(String, f64, f64), String> {
    const MAX_WIDTH: f64 = 680.0;
    const MAX_HEIGHT: f64 = 900.0;
    let definition = source.trim_matches(crate::html::is_js_whitespace);
    if definition.is_empty() {
        return Err("Add a Mermaid definition to render a diagram.".into());
    }
    if definition.encode_utf16().count() > MAX_DIAGRAM_SOURCE_LENGTH {
        return Err("That diagram is too large to render.".into());
    }
    let options = mermaid_rs_renderer::RenderOptions {
        theme: mermaid_rs_renderer::Theme::neutral(),
        ..Default::default()
    };
    let svg = mermaid_rs_renderer::render_with_options(definition, options).map_err(|e| {
        let message = e.to_string();
        match message.lines().next().filter(|l| !l.is_empty()) {
            Some(first) => format!("Could not render that diagram: {first}"),
            None => "Could not render that diagram. Check the Mermaid syntax.".into(),
        }
    })?;
    let syntax = || "Could not render that diagram. Check the Mermaid syntax.".to_owned();
    let start = svg.find("<svg").ok_or_else(syntax)?;
    let end = start + svg[start..].find('>').ok_or_else(syntax)?;
    let tag = &svg[start..end];
    let attr = |name: &str| -> Option<String> {
        let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
        let close = tag[at..].find('"')?;
        Some(tag[at..at + close].to_owned())
    };
    let number = |value: Option<String>| -> Option<f64> {
        let value = value?;
        let n: f64 = value.trim_end_matches("px").parse().ok()?;
        (n.is_finite() && n > 0.0).then_some(n)
    };
    let view: Option<(f64, f64)> = attr("viewBox").and_then(|v| {
        let parts: Vec<f64> = v
            .split([' ', ','])
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse().ok())
            .collect();
        (parts.len() == 4 && parts[2] > 0.0 && parts[3] > 0.0).then(|| (parts[2], parts[3]))
    });
    let natural_width = number(attr("width"))
        .or(view.map(|v| v.0))
        .unwrap_or(MAX_WIDTH);
    let natural_height = number(attr("height"))
        .or(view.map(|v| v.1))
        .unwrap_or(MAX_HEIGHT);
    let scale = 1f64
        .min(MAX_WIDTH / natural_width)
        .min(MAX_HEIGHT / natural_height);
    let width = (natural_width * scale).round().max(1.0);
    let height = (natural_height * scale).round().max(1.0);
    // The opening tag rewritten: the page size, as an image, no inline style.
    let mut kept: Vec<String> = Vec::new();
    let mut rest = &tag[4..];
    while let Some(eq) = rest.find("=\"") {
        let name = rest[..eq].trim().to_owned();
        let close = rest[eq + 2..].find('"').map_or(rest.len(), |c| eq + 2 + c);
        let value = &rest[eq + 2..close.min(rest.len())];
        if !matches!(
            name.as_str(),
            "width" | "height" | "role" | "focusable" | "style" | "xmlns"
        ) && !name.is_empty()
        {
            kept.push(format!("{name}=\"{value}\""));
        }
        rest = rest.get(close + 1..).unwrap_or_default();
    }
    if attr("viewBox").is_none() {
        kept.push(format!("viewBox=\"0 0 {natural_width} {natural_height}\""));
    }
    let opening = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" {} width=\"{width}\" height=\"{height}\" role=\"img\" focusable=\"false\"",
        kept.join(" ")
    );
    let svg = format!("{opening}{}", &svg[end..]);
    Ok((svg, width, height))
}

/// A drawing's pixels: `scale` × its size, RGBA (not premultiplied), with that size.
#[derive(Clone, Debug, PartialEq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn raster(tree: &resvg::usvg::Tree, w: f64, h: f64, scale: f64) -> Option<Pixels> {
    const MAX_SIDE: f64 = 4096.0;
    let cw = (w * scale).ceil().clamp(1.0, MAX_SIDE);
    let ch = (h * scale).ceil().clamp(1.0, MAX_SIDE);
    let effective = (cw / w).min(ch / h);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(cw as u32, ch as u32)?;
    let size = tree.size();
    let fit = ((w * effective) as f32 / size.width()).min((h * effective) as f32 / size.height());
    resvg::render(
        tree,
        resvg::tiny_skia::Transform::from_scale(fit, fit),
        &mut pixmap.as_mut(),
    );
    let rgba = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Some(Pixels {
        width: cw as u32,
        height: ch as u32,
        rgba,
    })
}

/// An equation or calculation drawn for the screen: its size in points and its pixels at
/// `scale`, in `color`.
pub fn screen_pixels(
    kind: RichKind,
    source: &str,
    body_size_pt: f64,
    color: &str,
    scale: f64,
) -> Result<(f64, f64, Pixels), String> {
    let options = RenderOptions {
        png: false,
        color: color.to_owned(),
        body_size_pt,
    };
    let rendered = render(kind, source, &options)?;
    let svg = rendered
        .svg
        .unwrap_or_default()
        .replace("currentColor", color);
    let failed = || "Could not render this content.".to_owned();
    let tree = resvg::usvg::Tree::from_str(&svg, &svg_options()).map_err(|_| failed())?;
    let pixels = raster(
        &tree,
        rendered.width.round().max(1.0),
        rendered.height.round().max(1.0),
        scale,
    )
    .ok_or_else(failed)?;
    Ok((rendered.width, rendered.height, pixels))
}

/// `rasterizeSvg`: a PNG data URL of the drawing at `width` × `height`, three times as fine
/// (less when that is too large), in `color`.
pub fn rasterize(
    svg: &str,
    width: f64,
    height: f64,
    scale: f64,
    color: &str,
) -> Result<String, String> {
    let failed = || "Could not prepare an equation or diagram for export.".to_owned();
    let source = svg.replace("currentColor", color);
    let tree = resvg::usvg::Tree::from_str(&source, &svg_options()).map_err(|_| failed())?;
    let (w, h) = (width.round().max(1.0), height.round().max(1.0));
    let scales =
        std::iter::once(scale).chain([3.0, 2.0, 1.5, 1.0].into_iter().filter(|s| *s < scale));
    let mut last = 0;
    for scale in scales {
        let pixels = raster(&tree, w, h, scale).ok_or_else(failed)?;
        let image = craft_image_codecs::Image::from_u8(
            pixels.width,
            pixels.height,
            craft_image_codecs::ChannelLayout::Rgba,
            pixels.rgba,
        )
        .map_err(|_| failed())?;
        let png = craft_image_codecs::encode(
            &image,
            craft_image_codecs::Format::Png,
            &Default::default(),
        )
        .map_err(|_| failed())?;
        last = png.len();
        let url = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        );
        if crate::html::sanitize::is_embedded_image_src(&url) {
            return Ok(url);
        }
    }
    Err(format!(
        "An equation or diagram is too large to export ({} KB). Try simplifying it.",
        (last as f64 / 1024.0).round()
    ))
}

type CacheKey = (RichKind, bool, String, i64, String);
type Cache = Mutex<Vec<(CacheKey, Result<Rendered, String>)>>;

/// `cache` (160 renderings).
fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// `renderRich`: one equation, diagram or calculation, from the cache when it was drawn before.
pub fn render(kind: RichKind, source: &str, options: &RenderOptions) -> Result<Rendered, String> {
    let em = ((options.body_size_pt * 4.0 / 3.0) + 0.5).floor() as i64;
    let color = if options.png {
        options.color.clone()
    } else {
        "current".into()
    };
    let key = (kind, options.png, color, em, source.to_owned());
    if let Some((_, found)) = cache()
        .lock()
        .ok()
        .and_then(|c| c.iter().find(|(k, _)| *k == key).cloned())
    {
        return found;
    }
    let result = render_uncached(kind, source, options);
    if let Ok(mut c) = cache().lock() {
        c.push((key, result.clone()));
        if c.len() > 160 {
            c.drain(..1);
        }
    }
    result
}

fn render_uncached(
    kind: RichKind,
    source: &str,
    options: &RenderOptions,
) -> Result<Rendered, String> {
    if kind == RichKind::Diagram {
        let (svg, width, height) = diagram_svg(source)?;
        let mut rendered = Rendered {
            kind,
            source: source.to_owned(),
            alt: diagram_alt(source),
            width,
            height,
            svg: None,
            data_url: None,
        };
        if options.png {
            rendered.data_url = Some(rasterize(&svg, width, height, 2.5, &options.color)?);
        } else {
            rendered.svg = Some(svg);
        }
        return Ok(rendered);
    }
    let em = (options.body_size_pt * 4.0 / 3.0).clamp(10.0, 48.0);
    let (svg, width, height) = if kind == RichKind::Calc {
        let lines = crate::calc::evaluate_worksheet(source).map_err(|e| e.0)?;
        let latex = crate::calc::worksheet_to_latex(&lines).map_err(|e| e.0)?;
        math_svg(&latex, true, em, crate::calc::MAX_SOURCE_LENGTH * 8)?
    } else {
        math_svg(
            source,
            kind == RichKind::MathBlock,
            em,
            MAX_MATH_SOURCE_LENGTH,
        )?
    };
    let alt = if kind == RichKind::Calc {
        calc_alt(source)
    } else {
        format!("Equation: {source}")
    };
    let mut rendered = Rendered {
        kind,
        source: source.to_owned(),
        alt,
        width,
        height,
        svg: None,
        data_url: None,
    };
    if options.png {
        rendered.data_url = Some(rasterize(&svg, width, height, 3.0, &options.color)?);
    } else {
        rendered.svg = Some(svg);
    }
    Ok(rendered)
}

/// `applyRichRender`. An SVG drawing goes in as a marker in `svgs`, put back after the HTML is
/// written: the HTML parser would lowercase its attribute names (`viewBox`, `refX`).
fn apply(element: &mut Element, rendered: &Rendered, svgs: &mut Vec<String>) {
    element.children.clear();
    add_class(element, "rich-rendered");
    set_attr(element, "role", "img");
    set_attr(element, "aria-label", &rendered.alt);
    set_attr(element, "data-rendered-source", &rendered.source);
    if let Some(svg) = &rendered.svg {
        element.children = vec![Node::Text(svg_marker(svgs.len()))];
        svgs.push(svg.clone());
        return;
    }
    let Some(url) = &rendered.data_url else {
        return;
    };
    let mut image = Element {
        name: "img".into(),
        attrs: Vec::new(),
        children: Vec::new(),
    };
    set_attr(&mut image, "src", url);
    set_attr(&mut image, "alt", &rendered.alt);
    set_attr(
        &mut image,
        "width",
        &(rendered.width.round().max(1.0) as i64).to_string(),
    );
    set_attr(
        &mut image,
        "height",
        &(rendered.height.round().max(1.0) as i64).to_string(),
    );
    if rendered.kind == RichKind::MathInline {
        set_attr(&mut image, "data-inline", "true");
    }
    element.children.push(Node::Element(image));
}

/// `showRichFallback`: the source, with why it couldn't be drawn.
fn fallback(element: &mut Element, source: &str, message: &str) {
    element.children = vec![Node::Text(source.to_owned())];
    add_class(element, "rich-render-error");
    set_attr(element, "title", message);
}

fn set_attr(element: &mut Element, name: &str, value: &str) {
    match element.attrs.iter_mut().find(|(n, _)| n == name) {
        Some((_, v)) => *v = value.to_owned(),
        None => element.attrs.push((name.to_owned(), value.to_owned())),
    }
}

fn add_class(element: &mut Element, class: &str) {
    if element.has_class(class) {
        return;
    }
    let classes = element.attr("class").unwrap_or_default();
    let joined = if classes.is_empty() {
        class.to_owned()
    } else {
        format!("{classes} {class}")
    };
    set_attr(element, "class", &joined);
}

/// A marker no scene text holds (private-use characters around the drawing's number).
fn svg_marker(index: usize) -> String {
    format!("\u{F8F0}{index}\u{F8F1}")
}

fn walk(nodes: &mut [Node], options: &RenderOptions, depth: usize, svgs: &mut Vec<String>) {
    if depth > 256 {
        return;
    }
    for node in nodes {
        if let Node::Element(element) = node {
            if RichKind::of(element).is_some() {
                if let Some((kind, source)) = read_placeholder(element) {
                    match render(kind, &source, options) {
                        Ok(rendered) => apply(element, &rendered, svgs),
                        Err(message) => fallback(element, &source, &message),
                    }
                }
                continue;
            }
            walk(&mut element.children, options, depth + 1, svgs);
        }
    }
}

/// `prepareRichHtml`: every placeholder rendered, or its source left with why not.
pub fn prepare_rich_html(html: &str, options: &RenderOptions) -> String {
    if ![
        "math-inline",
        "math-block",
        "mermaid-diagram",
        "calc-worksheet",
    ]
    .iter()
    .any(|c| html.contains(c))
    {
        return html.to_owned();
    }
    let mut nodes = parse_fragment(html);
    let mut svgs = Vec::new();
    walk(&mut nodes, options, 0, &mut svgs);
    let mut out = serialize(&nodes);
    for (index, svg) in svgs.iter().enumerate() {
        out = out.replacen(&svg_marker(index), svg, 1);
    }
    out
}

/// `prepareBookRichContent`: every scene of the book prepared for an export.
pub fn prepare_book(
    book: &crate::publish::CompiledBook,
    options: &RenderOptions,
) -> crate::publish::CompiledBook {
    let mut book = book.clone();
    for scene in book.chapters.iter_mut().flat_map(|c| c.scenes.iter_mut()) {
        scene.html = prepare_rich_html(&scene.html, options);
    }
    book
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_equations_are_drawn_larger_than_inline_ones() {
        let options = RenderOptions::default();
        let tex = r"\sum_{n=1}^{\infty} \frac{1}{n^2}";
        let block = render(RichKind::MathBlock, tex, &options).expect("block");
        let inline = render(RichKind::MathInline, tex, &options).expect("inline");
        assert!(
            block.height > inline.height * 1.3,
            "{} vs {}",
            block.height,
            inline.height
        );
        let svg = block.svg.expect("svg");
        assert!(
            svg.starts_with("<svg") && svg.contains("currentColor"),
            "{}",
            &svg[..120]
        );
        assert_eq!(block.alt, format!("Equation: {tex}"));
    }

    #[test]
    fn exports_get_pictures_in_ink() {
        let options = RenderOptions {
            png: true,
            color: "#1d211d".into(),
            body_size_pt: 12.0,
        };
        let html = prepare_rich_html(
            r#"<p>Area <span class="math-inline" data-tex="\pi r^2"></span>.</p>"#,
            &options,
        );
        assert!(
            html.contains(r#"class="math-inline rich-rendered""#),
            "{html}"
        );
        assert!(
            html.contains(r#"<img src="data:image/png;base64,"#),
            "{html}"
        );
        assert!(html.contains(r#"data-inline="true""#), "{html}");
        assert!(html.contains(r#"aria-label="Equation: \pi r^2""#), "{html}");
        let src = html
            .split("base64,")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .expect("png");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(src)
            .expect("base64");
        let image = craft_image_codecs::decode(&bytes).expect("png");
        let rgba = image.to_rgba8();
        assert!(
            rgba.as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[3] > 200 && p[0] < 0x40),
            "ink drawn"
        );
    }

    #[test]
    fn what_cant_be_drawn_keeps_its_source_and_says_why() {
        let options = RenderOptions::default();
        let html = prepare_rich_html(
            r#"<div class="math-block" data-tex="\frac{1"></div><div class="mermaid-diagram" data-source="graph TD; A--&gt;B"></div><div class="math-block" data-tex="  "></div>"#,
            &options,
        );
        assert!(
            html.contains(r#"<div class="math-block rich-render-error" data-tex="\frac{1" title="Could not render that equation. Check the LaTeX syntax.">\frac{1</div>"#),
            "{html}"
        );
        // A diagram that isn't Mermaid keeps its source and says why.
        let bad = prepare_rich_html(
            r#"<div class="mermaid-diagram" data-source="not a diagram at all"></div>"#,
            &options,
        );
        assert!(
            bad.contains(r#"class="mermaid-diagram rich-render-error""#)
                && bad.contains("Could not render that diagram")
                && bad.ends_with(">not a diagram at all</div>"),
            "{bad}"
        );
        // A drawing keeps its SVG's attribute names as written.
        assert!(
            html.contains("viewBox=") && !html.contains("viewbox="),
            "{html}"
        );
        // An empty placeholder is left as it is (`readRichPlaceholder` gives nothing).
        assert!(
            html.ends_with(r#"<div class="math-block" data-tex="  "></div>"#),
            "{html}"
        );
    }

    #[test]
    fn a_diagram_is_drawn_sized_for_the_page() {
        let source = "graph TD\n  A[Start] --> B{Is it raining?}\n  B -- Yes --> C[Umbrella]\n  B -- No --> D[Outside]";
        let svg = render(RichKind::Diagram, source, &RenderOptions::default()).expect("diagram");
        assert_eq!(svg.alt, "Diagram: graph TD");
        assert!(svg.width <= 680.0 && svg.height <= 900.0);
        let markup = svg.svg.expect("svg");
        assert!(
            markup.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""),
            "{}",
            &markup[..100]
        );
        assert!(
            markup.contains(&format!("width=\"{}\"", svg.width)) && markup.contains("role=\"img\"")
        );
        // A wide diagram is scaled down to 680 wide.
        let wide: String = (0..30)
            .map(|i| format!("  N{i} --> N{}\n", i + 1))
            .collect();
        let wide = render(
            RichKind::Diagram,
            &format!("graph LR\n{wide}"),
            &RenderOptions::default(),
        )
        .expect("wide");
        assert_eq!(wide.width, 680.0);
        let png = render(
            RichKind::Diagram,
            source,
            &RenderOptions {
                png: true,
                ..RenderOptions::default()
            },
        )
        .expect("png");
        assert!(
            png.data_url
                .expect("png")
                .starts_with("data:image/png;base64,")
        );
    }

    #[test]
    fn a_calculation_is_drawn_as_its_worksheet() {
        let options = RenderOptions::default();
        let rendered =
            render(RichKind::Calc, "# Load\nm := 5 kg\nF := m g =", &options).expect("calc");
        assert_eq!(rendered.alt, "Calculation: m := 5 kg");
        assert!(rendered.height > rendered.width / 10.0);
        let book_html = prepare_rich_html(
            r#"<div class="calc-worksheet" data-source="x := 2"></div>"#,
            &options,
        );
        assert!(book_html.contains("<svg"), "{book_html}");
    }
}
