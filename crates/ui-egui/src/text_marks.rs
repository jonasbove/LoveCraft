//! Comment and Wiki-mention highlights over the editor's text (ProseCraft's
//! `refreshCommentHighlights` and `refreshWikiMentions`, drawn with the CSS Highlight API
//! there), and the cards hovering them shows: the Wiki article's excerpt
//! (`wiki-mention-preview`) and the open comments with Go to and Resolve (`comment-preview`).

use std::ops::Range;

use egui::{Color32, RichText, Ui};
use prosecraft_core::comments::{Anchor, locate_anchor};
use prosecraft_core::mentions::Matcher;
use prosecraft_core::project::{Comment, Document};
use prosecraft_core::richtext::RichDoc;

use crate::theme::Palette;

/// How long the comment card stays after the pointer leaves (`hideCommentPreviewSoon`).
const LINGER: f64 = 0.16;

/// What the highlights are worked out from.
pub(crate) struct Sources<'a> {
    pub comments: &'a [Comment],
    pub documents: &'a [Document],
}

/// A highlighted stretch of the editor's characters.
#[derive(Clone)]
struct Spans {
    /// Open comments: their passage and id.
    comments: Vec<(Range<usize>, String)>,
    /// Wiki mentions: their characters and article id.
    mentions: Vec<(Range<usize>, String)>,
}

/// The comment card on screen.
#[derive(Clone)]
struct Card {
    ids: Vec<String>,
    at: egui::Pos2,
    /// When it goes, unless the pointer is back on the text or on the card.
    until: Option<f64>,
}

/// What the cards asked for.
pub(crate) enum Asked {
    None,
    /// Select the comment's passage.
    GoTo(usize, usize),
    Resolve(String),
}

/// The rows a stretch of characters takes in `galley`, relative to it.
pub(crate) fn row_rects(galley: &egui::Galley, range: &Range<usize>) -> Vec<egui::Rect> {
    let start = galley.pos_from_cursor(egui::text::CCursor::new(range.start));
    let end = galley.pos_from_cursor(egui::text::CCursor::new(range.end));
    let mut out = Vec::new();
    for row in &galley.rows {
        let rect = row.rect();
        if rect.top() + 0.5 < start.top() || rect.top() > end.top() + 0.5 {
            continue;
        }
        let left = if (rect.top() - start.top()).abs() < 0.5 {
            start.left()
        } else {
            rect.left()
        };
        let right = if (rect.top() - end.top()).abs() < 0.5 {
            end.left()
        } else {
            rect.right()
        };
        if right > left {
            out.push(egui::Rect::from_x_y_ranges(left..=right, rect.y_range()));
        }
    }
    out
}

/// `underline dotted`.
fn dots(painter: &egui::Painter, left: f32, right: f32, y: f32, color: Color32) {
    let mut x = left;
    while x < right {
        painter.circle_filled(egui::pos2(x + 0.6, y), 0.6, color);
        x += 3.0;
    }
}

fn spans(doc: &RichDoc, document: &str, sources: &Sources<'_>) -> Spans {
    // `refreshCommentHighlights`: each open comment of the scene, found by its anchor.
    let mut comments = Vec::new();
    let mine: Vec<&Comment> = sources
        .comments
        .iter()
        .filter(|c| c.document_id == document && !c.resolved)
        .collect();
    if !mine.is_empty() {
        let map = doc.comment_map();
        for comment in mine {
            let anchor = Anchor {
                quote: comment.quote.encode_utf16().collect(),
                prefix: comment.prefix.encode_utf16().collect(),
                suffix: comment.suffix.encode_utf16().collect(),
            };
            if let Some((s, e)) = locate_anchor(&map.text, &anchor) {
                comments.push((map.to_editor(s)..map.to_editor(e), comment.id.clone()));
            }
        }
    }
    // `refreshWikiMentions`: the other Wiki articles with a title.
    let articles: Vec<&Document> = sources
        .documents
        .iter()
        .filter(|d| {
            d.space.as_deref() == Some("Wiki")
                && !d.trashed
                && d.id != document
                && !d.title.trim().is_empty()
        })
        .collect();
    let matcher = Matcher::new(articles.iter().map(|d| d.title.as_str()));
    let mentions = doc
        .mentions(&matcher)
        .into_iter()
        .filter_map(|(range, index)| articles.get(index).map(|d| (range, d.id.clone())))
        .collect();
    Spans { comments, mentions }
}

/// The spans for this text, worked out again only when it or the comments and articles change.
fn cached(ui: &Ui, doc: &RichDoc, body: &str, document: &str, sources: &Sources<'_>) -> Spans {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (body, document).hash(&mut hasher);
    for c in sources
        .comments
        .iter()
        .filter(|c| c.document_id == document)
    {
        (&c.id, &c.quote, &c.prefix, &c.suffix, c.resolved).hash(&mut hasher);
    }
    for d in sources
        .documents
        .iter()
        .filter(|d| d.space.as_deref() == Some("Wiki"))
    {
        (&d.id, &d.title, d.trashed).hash(&mut hasher);
    }
    let key = hasher.finish();
    let id = egui::Id::unique("text-marks-spans");
    if let Some((k, found)) = ui.ctx().data(|d| d.get_temp::<(u64, Spans)>(id))
        && k == key
    {
        return found;
    }
    let found = spans(doc, document, sources);
    ui.ctx()
        .data_mut(|d| d.insert_temp(id, (key, found.clone())));
    found
}

fn excerpt(article: &Document) -> String {
    let options = prosecraft_core::html::PlainTextOptions {
        rich: prosecraft_core::html::RichText::Omit,
        ..Default::default()
    };
    let plain = prosecraft_core::html::html_to_plain_text(
        &prosecraft_core::html::sanitize::sanitize_html(&article.content),
        &options,
    );
    plain
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(360)
        .collect()
}

/// Draws the highlights and the hover cards.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show(
    ui: &mut Ui,
    palette: &Palette,
    sources: &Sources<'_>,
    document: &str,
    doc: &RichDoc,
    body: &str,
    output: &egui::text_edit::TextEditOutput,
    now: f64,
) -> Asked {
    let spans = cached(ui, doc, body, document, sources);
    let galley = &output.galley;
    let origin = output.galley_pos.to_vec2();
    let painter = ui.painter().with_clip_rect(output.text_clip_rect);
    let length = body.chars().count();
    let comment_color = Color32::from_rgb(0xe0, 0xb0, 0x00);
    for (spans, color, fill) in [
        (&spans.comments, comment_color, 0.16),
        (&spans.mentions, palette.accent, 0.13),
    ] {
        for (range, _) in spans.iter().filter(|(r, _)| r.end <= length) {
            for rect in row_rects(galley, range) {
                let rect = rect.translate(origin);
                painter.rect_filled(rect, 0.0, color.gamma_multiply(fill));
                dots(
                    &painter,
                    rect.left(),
                    rect.right(),
                    rect.bottom() - 1.0,
                    color.gamma_multiply(0.7),
                );
            }
        }
    }

    // The character under the pointer, while nothing is selected.
    let selecting = output.cursor_range.is_some_and(|r| !r.is_empty());
    let hovered = output
        .response
        .hover_pos()
        .filter(|_| !selecting)
        .filter(|pos| galley.rect.translate(origin).expand(2.0).contains(*pos))
        .map(|pos| (pos, galley.cursor_from_pos(pos - origin.to_pos2()).index.0));
    let card_id = egui::Id::unique("comment-preview");
    let mut card = ui.ctx().data(|d| d.get_temp::<Card>(card_id));
    let mut asked = Asked::None;

    let mention = hovered.and_then(|(pos, at)| {
        spans
            .mentions
            .iter()
            .find(|(r, _)| r.start <= at && at < r.end)
            .map(|(_, id)| (pos, id))
    });
    if let Some((pos, id)) = mention {
        card = None;
        if let Some(article) = sources.documents.iter().find(|d| d.id == *id) {
            let text = excerpt(article);
            let area = egui::Area::new(egui::Id::unique("wiki-mention-preview"))
                .order(egui::Order::Tooltip)
                .fixed_pos(pos + egui::vec2(14.0, 16.0))
                .interactable(false)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(340.0);
                        ui.label(
                            RichText::new(&article.title)
                                .strong()
                                .color(palette.text_heading),
                        );
                        let text = if text.is_empty() {
                            "This Wiki article has no text yet.".to_owned()
                        } else {
                            text
                        };
                        ui.add(
                            egui::Label::new(
                                RichText::new(text).size(12.0).color(palette.text_body),
                            )
                            .wrap(),
                        );
                    });
                });
            let title = article.title.clone();
            ui.ctx()
                .accesskit_node_builder(area.response.id, move |node| {
                    node.set_role(egui::accesskit::Role::Tooltip);
                    node.set_label(format!("Wiki article {title}"));
                });
        }
    } else if let Some((pos, at)) = hovered {
        let ids: Vec<String> = spans
            .comments
            .iter()
            .filter(|(r, _)| r.start <= at && at < r.end)
            .take(3)
            .map(|(_, id)| id.clone())
            .collect();
        if !ids.is_empty() {
            card = Some(Card {
                ids,
                at: pos + egui::vec2(14.0, 16.0),
                until: None,
            });
        }
    }

    if let Some(shown) = card.as_mut() {
        let area = egui::Area::new(card_id)
            .order(egui::Order::Foreground)
            .fixed_pos(shown.at)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(320.0);
                    for id in &shown.ids {
                        let Some(comment) = sources.comments.iter().find(|c| c.id == *id) else {
                            continue;
                        };
                        let date =
                            chrono::DateTime::from_timestamp_millis(comment.created_at as i64)
                                .map(|t| t.format("%Y-%m-%d").to_string())
                                .unwrap_or_default();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&comment.author).strong().size(12.0));
                            ui.label(RichText::new(date).size(11.0).color(palette.text_muted));
                        });
                        ui.add(
                            egui::Label::new(RichText::new(&comment.body).color(palette.text_body))
                                .wrap(),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Go to").clicked()
                                && let Some((range, _)) =
                                    spans.comments.iter().find(|(_, c)| c == id)
                            {
                                asked = Asked::GoTo(range.start, range.end);
                            }
                            if ui.button("Resolve").clicked() {
                                asked = Asked::Resolve(id.clone());
                            }
                        });
                    }
                });
            });
        ui.ctx().accesskit_node_builder(area.response.id, |node| {
            node.set_role(egui::accesskit::Role::Group);
            node.set_label("Comment on this text");
        });
        let on_card = ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|p| area.response.rect.expand(4.0).contains(p));
        let on_text = hovered.is_some_and(|(_, at)| {
            spans
                .comments
                .iter()
                .any(|(r, _)| r.start <= at && at < r.end)
        });
        if on_card || on_text {
            shown.until = None;
        } else if shown.until.is_none() {
            shown.until = Some(now + LINGER);
        }
        if !matches!(asked, Asked::None) || shown.until.is_some_and(|t| now >= t) {
            card = None;
        } else if shown.until.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(LINGER));
        }
    }
    ui.ctx().data_mut(|d| match card {
        Some(card) => {
            d.insert_temp(card_id, card);
        }
        None => d.remove::<Card>(card_id),
    });
    asked
}
