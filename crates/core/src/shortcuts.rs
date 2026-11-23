//! The keyboard shortcuts sheet (ProseCraft `src/lib/shortcuts.ts`, upstream `2d1cf18`): the
//! shortcuts by group, their keys as macOS or Windows and Linux show them, as text, and as a
//! printable PDF.

use crate::publish::pdf::{Layout, Op, Page, Rgb};
use crate::publish::pdf_text::{Face, Family};

/// One shortcut: its keys (`Mod` is Cmd or Ctrl) and what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub keys: &'static [&'static str],
    pub action: &'static str,
}

/// A group of shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    pub title: &'static str,
    pub entries: &'static [Shortcut],
}

const TITLE: &str = "LoveCraft keyboard shortcuts";

/// `SHORTCUT_GROUPS`.
pub const GROUPS: [Group; 4] = [
    Group {
        title: "General",
        entries: &[
            Shortcut {
                keys: &["Mod", "N"],
                action: "New document",
            },
            Shortcut {
                keys: &["Mod", ","],
                action: "Settings",
            },
            Shortcut {
                keys: &["Mod", "Shift", "E"],
                action: "Export scene",
            },
            Shortcut {
                keys: &["Mod", "Shift", "P"],
                action: "Publish manuscript",
            },
            Shortcut {
                keys: &["Mod", "/"],
                action: "Keyboard shortcuts",
            },
        ],
    },
    Group {
        title: "Writing",
        entries: &[
            Shortcut {
                keys: &["Mod", "B"],
                action: "Bold",
            },
            Shortcut {
                keys: &["Mod", "I"],
                action: "Italic",
            },
            Shortcut {
                keys: &["Mod", "U"],
                action: "Underline",
            },
            Shortcut {
                keys: &["Mod", "Alt", "F"],
                action: "Insert footnote",
            },
            Shortcut {
                keys: &["Mod", "Alt", "C"],
                action: "Insert citation",
            },
            Shortcut {
                keys: &["Mod", "Z"],
                action: "Undo",
            },
            Shortcut {
                keys: &["Mod", "Shift", "Z"],
                action: "Redo",
            },
            Shortcut {
                keys: &["Mod", "Y"],
                action: "Redo (alternate)",
            },
        ],
    },
    Group {
        title: "Search",
        entries: &[Shortcut {
            keys: &["Mod", "Shift", "F"],
            action: "Search the project",
        }],
    },
    Group {
        title: "Layout",
        entries: &[
            Shortcut {
                keys: &["Mod", "Shift", "Enter"],
                action: "Toggle focus mode",
            },
            Shortcut {
                keys: &["Esc"],
                action: "Leave focus mode",
            },
            Shortcut {
                keys: &["Alt", "Click"],
                action: "Open scene, wiki page or file beside",
            },
        ],
    },
];

/// `formatKey`.
pub fn format_key(token: &str, mac: bool) -> &str {
    let (on_mac, other) = match token {
        "Mod" => ("⌘", "Ctrl"),
        "Alt" => ("⌥", "Alt"),
        "Shift" => ("⇧", "Shift"),
        "Enter" => ("↩", "Enter"),
        "Esc" => ("⎋", "Esc"),
        _ => return token,
    };
    if mac { on_mac } else { other }
}

/// `formatShortcut`.
pub fn format_shortcut(keys: &[&str], mac: bool) -> String {
    let labels: Vec<&str> = keys.iter().map(|k| format_key(k, mac)).collect();
    labels.join(if mac { "" } else { "+" })
}

/// `formatPdfShortcut`: every key spelled out (the PDF's built-in fonts have no ⌘ ⌥ ⇧ ↩ ⎋).
fn pdf_shortcut(keys: &[&str], mac: bool) -> String {
    let labels: Vec<&str> = keys
        .iter()
        .map(|k| match *k {
            "Mod" if mac => "Cmd",
            "Alt" if mac => "Option",
            k => format_key(k, false),
        })
        .collect();
    labels.join("+")
}

/// `shortcutsToPlainText`.
pub fn plain_text(groups: &[Group], mac: bool) -> String {
    let mut lines = vec![TITLE.to_owned(), String::new()];
    for group in groups {
        lines.push(group.title.to_owned());
        lines.push("-".repeat(group.title.chars().count()));
        let width = group
            .entries
            .iter()
            .map(|e| e.action.chars().count())
            .max()
            .unwrap_or(0)
            + 2;
        for entry in group.entries {
            let line = format!(
                "{:width$}{}",
                entry.action,
                format_shortcut(entry.keys, mac)
            );
            lines.push(line.trim_end().to_owned());
        }
        lines.push(String::new());
    }
    format!("{}\n", lines.join("\n").trim_end())
}

/// `buildShortcutsPdf`: an A4 sheet, the action on the left and the keys on the right; a long
/// sheet goes on to a second page without repeating its heading.
pub fn layout_pdf(groups: &[Group], mac: bool) -> Layout {
    const WIDTH: f64 = 595.28;
    const HEIGHT: f64 = 841.89;
    const MARGIN: f64 = 56.0;
    const INK: Rgb = [32, 36, 33];
    const MUTED: Rgb = [110, 114, 108];
    let right = WIDTH - MARGIN;
    let helvetica = |bold: bool| Face::new(Family::Helvetica, bold, false);
    let mut pages = vec![Page::default()];
    let mut y = MARGIN + 18.0;
    let text =
        |pages: &mut Vec<Page>, x: f64, y: f64, bold: bool, size: f64, color: Rgb, s: &str| {
            if let Some(page) = pages.last_mut() {
                page.ops.push(Op::Text {
                    x,
                    y,
                    face: helvetica(bold),
                    size,
                    color,
                    text: s.to_owned(),
                });
            }
        };
    let line = |pages: &mut Vec<Page>, y: f64, color: Rgb| {
        if let Some(page) = pages.last_mut() {
            page.ops.push(Op::Line {
                x1: MARGIN,
                y1: y,
                x2: right,
                y2: y,
                color,
                width: 0.8,
            });
        }
    };
    let ensure = |pages: &mut Vec<Page>, y: &mut f64, needed: f64| {
        if *y + needed > HEIGHT - MARGIN {
            pages.push(Page::default());
            *y = MARGIN;
        }
    };
    text(&mut pages, MARGIN, y, true, 20.0, INK, TITLE);
    y += 16.0;
    let layout_name = if mac {
        "macOS key layout"
    } else {
        "Windows and Linux key layout"
    };
    text(&mut pages, MARGIN, y, false, 9.5, MUTED, layout_name);
    y += 12.0;
    line(&mut pages, y, [206, 206, 200]);
    y += 16.0;
    for group in groups {
        ensure(&mut pages, &mut y, 50.0);
        y += 16.0;
        text(&mut pages, MARGIN, y, true, 12.0, INK, group.title);
        y += 10.0;
        line(&mut pages, y, [226, 226, 220]);
        y += 17.0;
        for entry in group.entries {
            ensure(&mut pages, &mut y, 19.0);
            text(&mut pages, MARGIN, y, false, 10.0, INK, entry.action);
            let keys = pdf_shortcut(entry.keys, mac);
            // `align: "right"`: the text ends at the right margin.
            let x = right - helvetica(true).aligned_width(&keys, 10.0);
            text(&mut pages, x, y, true, 10.0, MUTED, &keys);
            y += 19.0;
        }
        y += 8.0;
    }
    Layout {
        width: WIDTH,
        height: HEIGHT,
        pages,
        title: TITLE.to_owned(),
        author: String::new(),
        subtitle: String::new(),
        unshown: 0,
        sections: Vec::new(),
    }
}

/// The sheet as a PDF file.
pub fn pdf(mac: bool) -> Result<Vec<u8>, crate::publish::pdf_writer::PdfError> {
    crate::publish::pdf_writer::write_pdf(&layout_pdf(&GROUPS, mac))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_as_each_platform_shows_them() {
        assert_eq!(format_shortcut(&["Mod", "Shift", "Enter"], true), "⌘⇧↩");
        assert_eq!(
            format_shortcut(&["Mod", "Shift", "Enter"], false),
            "Ctrl+Shift+Enter"
        );
        assert_eq!(pdf_shortcut(&["Mod", "Alt", "F"], true), "Cmd+Option+F");
        assert_eq!(pdf_shortcut(&["Esc"], true), "Esc");
    }

    #[test]
    fn plain_text_lines_up_the_keys() {
        let text = plain_text(&GROUPS[2..3], false);
        assert_eq!(
            text,
            "LoveCraft keyboard shortcuts\n\nSearch\n------\nSearch the project  Ctrl+Shift+F\n"
        );
    }

    #[test]
    fn the_sheet_is_a_pdf_with_every_shortcut() {
        let layout = layout_pdf(&GROUPS, false);
        let texts: Vec<&str> = layout
            .pages
            .iter()
            .flat_map(|p| &p.ops)
            .filter_map(|op| match op {
                Op::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(texts.contains(&"Toggle focus mode"));
        assert!(texts.contains(&"Ctrl+Shift+Enter"));
        assert_eq!(layout.pages.len(), 1);
        let bytes = pdf(true).expect("pdf");
        assert!(bytes.starts_with(b"%PDF-"));
    }
}
