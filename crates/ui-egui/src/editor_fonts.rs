//! The writing fonts (ProseCraft's `src/lib/editor-fonts.ts` and `EditorFontPicker.svelte`): a
//! ProseCraft font or an installed family for the writing area, and another for the Markdown
//! source, kept on this device only.

use crate::fonts::{Face, WritingFaces};

/// ProseCraft's `localStorage` keys.
pub const FONT_KEY: &str = "prosecraft-editor-font";
pub const MARKDOWN_FONT_KEY: &str = "prosecraft-markdown-editor-font";
pub const SYSTEM_FONT_KEY: &str = "prosecraft-editor-system-font";
pub const MARKDOWN_SYSTEM_FONT_KEY: &str = "prosecraft-markdown-editor-system-font";

/// `RICH_TEXT_FONT_OPTIONS`: id, label, and the installed families its CSS stack would use
/// first (the bundled face stands in for none of them: Source Serif for Georgia, Inter for the
/// system sans).
pub const RICH_TEXT_FONTS: [(&str, &str, &[&str]); 7] = [
    ("serif", "Georgia", &["Georgia"]),
    ("bitter", "Bitter", &["Bitter Variable", "Bitter"]),
    ("literata", "Literata", &["Literata Variable", "Literata"]),
    ("ibm-plex-serif", "IBM Plex Serif", &["IBM Plex Serif"]),
    (
        "ibm-plex-sans",
        "IBM Plex Sans",
        &["IBM Plex Sans", "Inter"],
    ),
    ("sans", "System Sans Serif", &["Inter"]),
    (
        "mono",
        "SF Mono",
        &["SFMono-Regular", "SF Mono", "Consolas", "Liberation Mono"],
    ),
];

/// `MARKDOWN_FONT_OPTIONS`, as [`RICH_TEXT_FONTS`].
pub const MARKDOWN_FONTS: [(&str, &str, &[&str]); 4] = [
    (
        "sf-mono",
        "SF Mono",
        &["SFMono-Regular", "SF Mono", "Consolas", "Liberation Mono"],
    ),
    ("menlo", "Menlo", &["Menlo", "SFMono-Regular", "SF Mono"]),
    ("monaco", "Monaco", &["Monaco", "SFMono-Regular", "SF Mono"]),
    ("courier", "Courier New", &["Courier New", "Courier"]),
];

/// An installed family (`SystemFontFamily`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledFamily {
    pub family: String,
    pub monospaced: bool,
}

/// A family's faces, as the host loads them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FamilyFaces {
    pub regular: Option<Face>,
    pub italic: Option<Face>,
    pub bold: Option<Face>,
}

/// The device's installed fonts (`list_system_font_families` on the desktop).
pub trait InstalledFonts: Send + Sync {
    /// The families, sorted and without duplicates; `None` while they are still being listed.
    fn families(&self) -> Option<Vec<InstalledFamily>>;
    /// A family's regular, italic and bold faces; `None` when it isn't installed.
    fn faces(&self, family: &str) -> Option<FamilyFaces>;
}

/// The writing fonts chosen: a ProseCraft font's id, and an installed family that wins over it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontPrefs {
    pub rich: String,
    pub rich_system: String,
    pub markdown: String,
    pub markdown_system: String,
}

impl Default for FontPrefs {
    fn default() -> Self {
        Self {
            rich: "serif".into(),
            rich_system: String::new(),
            markdown: "sf-mono".into(),
            markdown_system: String::new(),
        }
    }
}

impl FontPrefs {
    /// From the device's settings; unknown ids are the defaults (`isRichTextFontPreference`).
    pub fn read(load: &mut dyn FnMut(&str) -> Option<String>) -> Self {
        let default = Self::default();
        let known =
            |id: &str, list: &[(&str, &str, &[&str])]| list.iter().any(|(i, _, _)| *i == id);
        Self {
            rich: load(FONT_KEY)
                .filter(|v| known(v, &RICH_TEXT_FONTS))
                .unwrap_or(default.rich),
            rich_system: load(SYSTEM_FONT_KEY).unwrap_or_default(),
            markdown: load(MARKDOWN_FONT_KEY)
                .filter(|v| known(v, &MARKDOWN_FONTS))
                .unwrap_or(default.markdown),
            markdown_system: load(MARKDOWN_SYSTEM_FONT_KEY).unwrap_or_default(),
        }
    }

    /// The settings to save (`savePreferences`).
    pub fn entries(&self) -> [(&'static str, &str); 4] {
        [
            (FONT_KEY, &self.rich),
            (MARKDOWN_FONT_KEY, &self.markdown),
            (SYSTEM_FONT_KEY, &self.rich_system),
            (MARKDOWN_SYSTEM_FONT_KEY, &self.markdown_system),
        ]
    }

    /// The faces the stacks come to (`richTextFontStack`, `markdownFontStack`): the installed
    /// family when one is chosen and installed, else the first installed family of the
    /// ProseCraft font's stack, else the defaults.
    pub fn faces(&self, installed: Option<&dyn InstalledFonts>) -> WritingFaces {
        let Some(installed) = installed else {
            return WritingFaces::default();
        };
        let candidates = |system: &str, id: &str, list: &[(&str, &str, &[&str])]| {
            let mut names: Vec<String> = Vec::new();
            if !system.trim().is_empty() {
                names.push(system.trim().to_owned());
            }
            if let Some((_, _, stack)) = list.iter().find(|(i, _, _)| *i == id) {
                names.extend(stack.iter().map(|s| (*s).to_owned()));
            }
            names
                .into_iter()
                .find_map(|name| installed.faces(&name))
                .unwrap_or_default()
        };
        let rich = candidates(&self.rich_system, &self.rich, &RICH_TEXT_FONTS);
        let markdown = candidates(&self.markdown_system, &self.markdown, &MARKDOWN_FONTS);
        WritingFaces {
            regular: rich.regular,
            italic: rich.italic,
            bold: rich.bold,
            markdown: markdown.regular,
        }
    }
}

/// What a picker chose (`EditorFontSelection`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    Builtin(String),
    System(String),
}

/// `EditorFontPicker`: the ProseCraft fonts, then the installed ones (monospaced only, unless
/// Show all fonts, for the Markdown source).
pub fn picker(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    (id, name): (&str, &str),
    options: &[(&str, &str, &[&str])],
    (builtin, system): (&str, &str),
    (families, supported, monospace_only): (Option<&[InstalledFamily]>, bool, bool),
) -> Option<Selection> {
    let show_all_id = egui::Id::unique(("font-picker-show-all", id));
    let mut show_all = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(show_all_id))
        .unwrap_or(false);
    let ready = supported && families.is_some();
    if ready && monospace_only && ui.checkbox(&mut show_all, "Show all fonts").changed() {
        ui.ctx().data_mut(|d| d.insert_temp(show_all_id, show_all));
    }
    let selected_family = system.trim();
    let visible: Vec<&InstalledFamily> = families
        .unwrap_or_default()
        .iter()
        .filter(|f| !monospace_only || show_all || f.monospaced)
        .collect();
    let current = if selected_family.is_empty() {
        options
            .iter()
            .find(|(i, _, _)| *i == builtin)
            .map_or(builtin.to_owned(), |(_, l, _)| (*l).to_owned())
    } else {
        selected_family.to_owned()
    };
    let mut chosen = None;
    let muted = |text: &str| {
        egui::RichText::new(text)
            .size(11.0)
            .color(palette.text_muted)
    };
    let combo = craft_ui::ComboBox::from_id_salt(id)
        .selected_text(current.clone())
        .width(ui.available_width().min(320.0))
        .show_ui(ui, |ui| {
            ui.label(muted(&format!(
                "{} fonts",
                prosecraft_core::about::APP_NAME
            )));
            for (option, label, _) in options {
                let on = selected_family.is_empty() && *option == builtin;
                if ui.selectable_label(on, *label).clicked() {
                    chosen = Some(Selection::Builtin((*option).to_owned()));
                }
            }
            ui.label(muted("Installed fonts"));
            if !selected_family.is_empty()
                && !(ready && visible.iter().any(|f| f.family == selected_family))
                && ui
                    .selectable_label(true, format!("{selected_family} (current)"))
                    .clicked()
            {
                chosen = Some(Selection::System(selected_family.to_owned()));
            }
            if !supported {
                ui.label(muted("Available in the desktop app"));
            } else if families.is_none() {
                ui.label(muted("Loading installed fonts…"));
            } else {
                for family in &visible {
                    if ui
                        .selectable_label(family.family == selected_family, &family.family)
                        .clicked()
                    {
                        chosen = Some(Selection::System(family.family.clone()));
                    }
                }
            }
        });
    let name = name.to_owned();
    ui.ctx()
        .accesskit_node_builder(combo.response.id, move |node| {
            node.set_label(name);
            node.set_value(current);
        });
    if !supported {
        ui.label(muted(
            "Installed font selection is available in the desktop app.",
        ));
    } else if families.is_none() {
        ui.label(muted("Loading installed fonts…"));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    } else if visible.is_empty() && selected_family.is_empty() {
        ui.label(muted(if monospace_only && !show_all {
            "No monospaced fonts match. Show all fonts to see other families."
        } else {
            "No installed fonts match."
        }));
    }
    chosen
}

/// `installedFontMissing`: a chosen installed family the device doesn't have.
pub fn missing(family: &str, families: Option<&[InstalledFamily]>) -> bool {
    let family = family.trim();
    match families {
        Some(list) if !family.is_empty() => {
            !list.iter().any(|f| f.family.eq_ignore_ascii_case(family))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fonts;
    impl InstalledFonts for Fonts {
        fn families(&self) -> Option<Vec<InstalledFamily>> {
            Some(vec![InstalledFamily {
                family: "Literata".into(),
                monospaced: false,
            }])
        }
        fn faces(&self, family: &str) -> Option<FamilyFaces> {
            let face = |b: u8| {
                Some(Face {
                    bytes: vec![b],
                    index: 0,
                })
            };
            match family {
                "Literata" => Some(FamilyFaces {
                    regular: face(1),
                    italic: face(2),
                    bold: None,
                }),
                "Liberation Mono" => Some(FamilyFaces {
                    regular: face(9),
                    ..Default::default()
                }),
                _ => None,
            }
        }
    }

    #[test]
    fn preferences_resolve_to_installed_faces_as_the_css_stacks_do() {
        let mut stored = std::collections::HashMap::from([
            (FONT_KEY, "literata"),
            (MARKDOWN_FONT_KEY, "nonsense"),
        ]);
        let prefs = FontPrefs::read(&mut |k| stored.remove(k).map(str::to_owned));
        assert_eq!(prefs.rich, "literata");
        assert_eq!(prefs.markdown, "sf-mono");
        let faces = prefs.faces(Some(&Fonts));
        assert_eq!(faces.regular.map(|f| f.bytes), Some(vec![1]));
        assert_eq!(faces.italic.map(|f| f.bytes), Some(vec![2]));
        assert_eq!(faces.markdown.map(|f| f.bytes), Some(vec![9]));
        // An installed family wins; one that isn't installed falls back to the stack.
        let gone = FontPrefs {
            rich_system: "Missing Sans".into(),
            ..prefs.clone()
        };
        assert_eq!(
            gone.faces(Some(&Fonts)).regular.map(|f| f.bytes),
            Some(vec![1])
        );
        assert!(missing("Missing Sans", Fonts.families().as_deref()));
        assert!(!missing("literata", Fonts.families().as_deref()));
        // Georgia isn't installed here: the bundled serif.
        assert_eq!(FontPrefs::default().faces(Some(&Fonts)).regular, None);
        assert_eq!(FontPrefs::default().faces(None), WritingFaces::default());
    }
}
