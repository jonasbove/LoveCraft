//! ProseCraft's "Find and replace" dialog (`SearchDialog.svelte`): Find with Match case and
//! Whole word, Replace, the hits with a snippet each and a checkbox for the replace, and the
//! confirmed replace, which snapshots each changed scene first. Enter goes to the selected
//! hit, Ctrl+Enter replaces the selected ones, Esc closes.

use egui::{RichText, Ui};
use prosecraft_core::js::JsValue;
use prosecraft_core::search::{
    MAX_SEARCH_HITS, ReplaceResult, SearchHit, SearchOptions, replace_in_documents,
    search_documents,
};

use crate::AppState;

/// The dialog's state while it is open.
#[derive(Clone, Debug, Default)]
pub(crate) struct FindState {
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    /// Hits left out of the replace.
    pub excluded: Vec<String>,
    /// The hit Enter goes to.
    pub selected: usize,
    /// A replace waiting for confirmation: the ids it covers and what it would change.
    pub pending: Option<(Vec<String>, ReplaceResult)>,
    pub status: String,
    pub focus: bool,
}

impl FindState {
    pub fn new() -> Self {
        Self {
            focus: true,
            ..Self::default()
        }
    }

    fn options(&self) -> SearchOptions {
        SearchOptions {
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            include_trashed: false,
        }
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The snippet with its match highlighted.
fn snippet_job(hit: &SearchHit, palette: &crate::theme::Palette) -> egui::text::LayoutJob {
    let text = String::from_utf16_lossy(&hit.snippet);
    // UTF-16 offsets to byte offsets.
    let byte = |units: usize| {
        let mut count = 0;
        for (i, c) in text.char_indices() {
            if count >= units {
                return i;
            }
            count += c.len_utf16();
        }
        text.len()
    };
    let (start, end) = (
        byte(hit.match_start),
        byte(hit.match_start + hit.match_length),
    );
    let mut job = egui::text::LayoutJob::default();
    let plain = egui::TextFormat {
        font_id: egui::FontId::proportional(12.5),
        color: palette.text_secondary,
        ..Default::default()
    };
    let mark = egui::TextFormat {
        background: palette.selected_bg,
        color: palette.text_primary,
        ..plain.clone()
    };
    job.append(&text[..start], 0.0, plain.clone());
    job.append(&text[start..end], 0.0, mark);
    job.append(&text[end..], 0.0, plain);
    job.wrap.max_width = f32::INFINITY;
    job
}

pub(crate) fn show(ui: &mut Ui, state: &mut AppState) {
    let palette = state.palette;
    let editable = state.editable;
    let now_ms = state.now_ms;
    let Some(entry) = state
        .library
        .as_ref()
        .and_then(|l| l.projects().get(state.active))
    else {
        state.find = None;
        return;
    };
    let Some(find) = state.find.as_mut() else {
        return;
    };
    let data = entry
        .stored
        .get("project")
        .cloned()
        .unwrap_or(JsValue::Null);
    let documents = data
        .get("documents")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default();
    let chapters = data
        .get("chapters")
        .and_then(JsValue::as_array)
        .map(<[JsValue]>::to_vec)
        .unwrap_or_default();
    let hits = search_documents(&documents, &chapters, &find.query, find.options());
    let selected_ids: Vec<String> = hits
        .iter()
        .map(|h| h.document_id.clone())
        .filter(|id| !find.excluded.contains(id))
        .collect();
    let total_matches: usize = hits.iter().map(|h| h.matches).sum();
    let selected_matches: usize = hits
        .iter()
        .filter(|h| selected_ids.contains(&h.document_id))
        .map(|h| h.matches)
        .sum();
    // A dry run over the selection: matches that span formatting are found but not replaced.
    let preview = (!find.query.trim().is_empty()).then(|| {
        let mut copy = documents.clone();
        replace_in_documents(
            &mut copy,
            &find.query,
            &find.replacement,
            find.options(),
            false,
            Some(&selected_ids),
        )
    });
    let skipped = preview
        .as_ref()
        .map_or(0, |p| selected_matches.saturating_sub(p.total));
    find.selected = find.selected.min(hits.len().saturating_sub(1));

    let mut open = true;
    let mut go_to: Option<String> = None;
    let mut request: Option<bool> = None;
    let mut confirm = false;
    egui::Window::new("Find and replace")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(egui::vec2(680.0, 560.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Find")
                        .size(12.0)
                        .color(palette.text_secondary),
                );
                let field = ui.add(
                    egui::TextEdit::singleline(&mut find.query)
                        .hint_text("Search titles and text")
                        .desired_width(440.0),
                );
                crate::views::name_field(ui, &field, "Find");
                if std::mem::take(&mut find.focus) {
                    field.request_focus();
                }
                if field.changed() {
                    find.pending = None;
                    find.status.clear();
                    find.selected = 0;
                }
                let group = ui.scope(|ui| {
                    for (on, text, name) in [
                        (&mut find.case_sensitive, "Aa", "Match case"),
                        (&mut find.whole_word, "ab", "Whole word"),
                    ] {
                        let button = ui
                            .add(egui::Button::selectable(*on, text))
                            .on_hover_text(name);
                        let pressed = *on;
                        ui.ctx().accesskit_node_builder(button.id, move |node| {
                            node.set_label(name);
                            node.set_toggled(if pressed {
                                egui::accesskit::Toggled::True
                            } else {
                                egui::accesskit::Toggled::False
                            });
                        });
                        if button.clicked() {
                            *on = !*on;
                        }
                    }
                });
                ui.ctx().accesskit_node_builder(group.response.id, |node| {
                    node.set_role(egui::accesskit::Role::Group);
                    node.set_label("Search options");
                });
            });
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Replace")
                        .size(12.0)
                        .color(palette.text_secondary),
                );
                let field = ui.add(
                    egui::TextEdit::singleline(&mut find.replacement)
                        .hint_text("Replace with…")
                        .desired_width(440.0),
                );
                crate::views::name_field(ui, &field, "Replace");
                if field.changed() {
                    find.pending = None;
                    find.status.clear();
                }
            });
            ui.separator();
            if !hits.is_empty() {
                let (matches, docs) = match &preview {
                    Some(p) => (p.total, p.changed.len()),
                    None => (selected_matches, selected_ids.len()),
                };
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} in {}",
                            plural(matches, "match", "matches"),
                            plural(docs, "document", "documents")
                        ))
                        .size(12.0),
                    );
                    let all = find.excluded.is_empty();
                    if ui
                        .small_button(if all { "Select none" } else { "Select all" })
                        .clicked()
                    {
                        find.excluded = if all {
                            hits.iter().map(|h| h.document_id.clone()).collect()
                        } else {
                            Vec::new()
                        };
                        find.pending = None;
                    }
                });
                if skipped > 0 {
                    let span = if skipped == 1 {
                        "match spans formatting and is"
                    } else {
                        "matches span formatting and are"
                    };
                    ui.label(
                        RichText::new(format!("{skipped} {span} skipped."))
                            .size(12.0)
                            .color(palette.text_muted),
                    );
                }
            }
            egui::ScrollArea::vertical()
                .max_height(330.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if find.query.trim().is_empty() {
                        ui.label(
                            RichText::new("Type to search scene text, wiki pages, and titles.")
                                .color(palette.text_muted),
                        );
                    } else if hits.is_empty() {
                        ui.label(
                            RichText::new(format!("No matches for “{}”.", find.query.trim()))
                                .color(palette.text_muted),
                        );
                    }
                    for (index, hit) in hits.iter().enumerate() {
                        let title = if hit.title.is_empty() {
                            "Untitled".to_owned()
                        } else {
                            hit.title.clone()
                        };
                        ui.horizontal(|ui| {
                            let mut include = !find.excluded.contains(&hit.document_id);
                            let check = ui.checkbox(&mut include, "");
                            crate::views::name_field(
                                ui,
                                &check,
                                &format!("Include {title} in replace"),
                            );
                            if check.changed() {
                                if include {
                                    find.excluded.retain(|id| *id != hit.document_id);
                                } else {
                                    find.excluded.push(hit.document_id.clone());
                                }
                                find.pending = None;
                            }
                            let frame = egui::Frame::new()
                                .fill(if index == find.selected {
                                    palette.hover_bg
                                } else {
                                    egui::Color32::TRANSPARENT
                                })
                                .corner_radius(6)
                                .inner_margin(egui::Margin::symmetric(8, 4));
                            let response = frame
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(&title)
                                                    .strong()
                                                    .color(palette.text_heading),
                                            );
                                            ui.label(
                                                RichText::new(if hit.wiki {
                                                    "Wiki"
                                                } else {
                                                    "Scene"
                                                })
                                                .size(10.5)
                                                .color(palette.eyebrow),
                                            );
                                            if let Some(chapter) = &hit.chapter_title {
                                                ui.label(
                                                    RichText::new(chapter)
                                                        .size(11.5)
                                                        .color(palette.text_muted),
                                                );
                                            }
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(hit.matches.to_string())
                                                            .size(11.5)
                                                            .color(palette.text_muted),
                                                    )
                                                },
                                            );
                                        });
                                        ui.add(egui::Label::new(snippet_job(hit, &palette)).wrap());
                                    });
                                })
                                .response
                                .interact(egui::Sense::click());
                            let name = format!(
                                "Go to {} in {title}",
                                plural(hit.matches, "match", "matches")
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::Button, true, &name)
                            });
                            if response.hovered() {
                                find.selected = index;
                            }
                            if response.clicked() {
                                go_to = Some(hit.document_id.clone());
                            }
                        });
                    }
                });
            ui.separator();
            let status = if !find.status.is_empty() {
                find.status.clone()
            } else {
                let more = if hits.len() >= MAX_SEARCH_HITS {
                    format!(" (first {MAX_SEARCH_HITS} documents)")
                } else {
                    String::new()
                };
                format!(
                    "{} in {}{more}",
                    plural(total_matches, "hit", "hits"),
                    plural(hits.len(), "document", "documents")
                )
            };
            let status_label = ui.label(
                RichText::new(&status)
                    .size(12.0)
                    .color(palette.text_secondary),
            );
            ui.ctx()
                .accesskit_node_builder(status_label.id, move |node| {
                    node.set_role(egui::accesskit::Role::Status);
                    node.set_label(status);
                });
            let pending = find
                .pending
                .as_ref()
                .map(|(_, r)| (r.total, r.changed.len()));
            match pending {
                Some((total, documents)) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "Replace {} in {}? A snapshot of each changed scene is saved first.",
                            plural(total, "match", "matches"),
                            plural(documents, "document", "documents")
                        ));
                        if ui.button("Cancel").clicked() {
                            find.pending = None;
                        }
                        confirm = ui
                            .add_enabled(
                                editable,
                                egui::Button::new(RichText::new("Replace").strong()),
                            )
                            .clicked();
                    });
                }
                None => {
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                editable && selected_matches > 0,
                                egui::Button::new("Replace selected"),
                            )
                            .clicked()
                        {
                            request = Some(false);
                        }
                        let all_label = if find.replacement.is_empty() {
                            "Delete matches"
                        } else {
                            "Replace all"
                        };
                        if ui
                            .add_enabled(
                                editable && !hits.is_empty(),
                                egui::Button::new(RichText::new(all_label).strong()),
                            )
                            .clicked()
                        {
                            request = Some(true);
                        }
                    });
                }
            }
            ui.label(
                RichText::new(
                    "Enter goes to the match · Ctrl+Enter replaces selected · Esc closes",
                )
                .size(11.0)
                .color(palette.text_muted),
            );
        });

    // Keys: arrows choose a hit, Enter goes to it, Ctrl+Enter replaces the selected ones.
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
        find.selected = (find.selected + 1).min(hits.len().saturating_sub(1));
    }
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
        find.selected = find.selected.saturating_sub(1);
    }
    if !hits.is_empty()
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter))
    {
        request = Some(false);
    } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        && let Some(hit) = hits.get(find.selected)
    {
        go_to = Some(hit.document_id.clone());
    }
    if let Some(all) = request {
        let ids: Vec<String> = if all {
            hits.iter().map(|h| h.document_id.clone()).collect()
        } else {
            selected_ids.clone()
        };
        let mut copy = documents.clone();
        let result = replace_in_documents(
            &mut copy,
            &find.query,
            &find.replacement,
            find.options(),
            false,
            Some(&ids),
        );
        if result.total > 0 {
            find.status.clear();
            find.pending = Some((ids, result));
        } else {
            find.status = "Nothing to replace.".into();
        }
    }
    if confirm && let Some((ids, result)) = find.pending.take() {
        state
            .intents
            .push(crate::editing::Intent::ReplaceInProject {
                query: find.query.clone(),
                replacement: find.replacement.clone(),
                options: find.options(),
                only: ids,
                now: now_ms,
            });
        find.excluded.clear();
        find.selected = 0;
        find.status = format!(
            "Replaced {} in {}",
            plural(result.total, "match", "matches"),
            plural(result.changed.len(), "document", "documents")
        );
        state.draft = None;
    }
    if let Some(id) = go_to {
        // Open the document and select its first match in the editor.
        let (query, options) = (find.query.clone(), find.options());
        state.find = None;
        let content = documents
            .iter()
            .find(|d| {
                d.get("id")
                    .and_then(JsValue::as_js_string)
                    .is_some_and(|i| i.eq_str(&id))
            })
            .and_then(|d| d.get("content"))
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        let body = prosecraft_core::richtext::RichDoc::from_html(&content)
            .map(|d| d.text)
            .unwrap_or_default();
        if let Some((start, end)) =
            prosecraft_core::search::find_first_match(&body, &query, options)
        {
            let chars = |units: usize| {
                let mut count = 0;
                body.chars()
                    .take_while(|c| {
                        let more = count < units;
                        count += c.len_utf16();
                        more
                    })
                    .count()
            };
            state.comment_goto = Some((id.clone(), chars(start), chars(end)));
        }
        state.select(crate::Selection::Document(id));
        return;
    }
    if !open || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.find = None;
    }
}
