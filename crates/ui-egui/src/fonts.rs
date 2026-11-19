//! The app's fonts: Inter for the interface (as ProseCraft uses) and Source Serif 4 for the
//! manuscript and headings (ProseCraft uses Georgia, which isn't freely licensed), over craft-ui's
//! shared font stack, with the system's fonts for emoji and every other script.

use craft_fonts_build::EmbeddedFace;
use craft_ui::fonts::{CraftFace, Family, I18nScripts, SystemFonts, UiFonts};
use egui::{FontData, FontFamily, FontId};

/// The faces `build.rs` embedded from craft-fonts (empty without a checkout).
static APP_FACES: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/app_fonts.rs"));
static CRAFT_FONTS: &[EmbeddedFace] = include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

pub const SEMIBOLD: &str = "semibold";
pub const SERIF: &str = "serif";
pub const SERIF_ITALIC: &str = "serif-italic";
pub const SERIF_SEMIBOLD: &str = "serif-semibold";
/// The writing area's text (ProseCraft's `--writing-font`): the Writing font chosen in Settings,
/// else the manuscript serif.
pub const WRITING: &str = "writing";
pub const WRITING_ITALIC: &str = "writing-italic";
pub const WRITING_BOLD: &str = "writing-bold";
/// The Markdown editor's text (`--markdown-font`), when a Markdown source font is chosen.
pub const MARKDOWN: &str = "markdown";

/// A face of an installed font: its file's bytes and its index in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Face {
    pub bytes: Vec<u8>,
    pub index: u32,
}

/// The faces chosen for the writing area and the Markdown editor (`None`: the defaults).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WritingFaces {
    pub regular: Option<Face>,
    pub italic: Option<Face>,
    pub bold: Option<Face>,
    pub markdown: Option<Face>,
}

/// How [`install`] was last asked to use the system's fonts, and whether a Markdown face is in.
static SYSTEM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static MARKDOWN_SET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The families each embedded face heads.
fn families_of(face: &str) -> Vec<FontFamily> {
    let named = |name: &str| FontFamily::Name(name.into());
    match face {
        "Inter-Regular" => vec![FontFamily::Proportional],
        "Inter-SemiBold" => vec![named(SEMIBOLD)],
        "SourceSerif4-Regular" => vec![named(SERIF), named(WRITING)],
        "SourceSerif4-It" => vec![named(SERIF_ITALIC), named(WRITING_ITALIC)],
        "SourceSerif4-Semibold" => vec![named(SERIF_SEMIBOLD), named(WRITING_BOLD)],
        _ => Vec::new(),
    }
}

/// Installs the stack in `ctx`. Every named family exists even without craft-fonts (egui's
/// default faces then stand in), so text set in them never fails. Without `system`, installed
/// fonts aren't used, so text draws the same on every machine and no font arrives late from
/// the background index (for tests).
pub fn install(ctx: &egui::Context, system: bool) {
    install_with(ctx, system, &WritingFaces::default());
}

/// [`install`] again with the writing area's and the Markdown editor's chosen faces ahead of
/// their defaults, keeping how the system's fonts are used.
pub fn reinstall(ctx: &egui::Context, faces: &WritingFaces) {
    install_with(
        ctx,
        SYSTEM.load(std::sync::atomic::Ordering::Relaxed),
        faces,
    );
}

fn install_with(ctx: &egui::Context, system: bool, faces: &WritingFaces) {
    SYSTEM.store(system, std::sync::atomic::Ordering::Relaxed);
    MARKDOWN_SET.store(
        faces.markdown.is_some(),
        std::sync::atomic::Ordering::Relaxed,
    );
    let craft: Vec<CraftFace> = CRAFT_FONTS.iter().cloned().map(CraftFace::from).collect();
    let mut fonts = UiFonts::new(&I18nScripts)
        .languages(&["en"])
        .ui_language("en")
        .craft_faces(&craft)
        .system_fonts(if system {
            SystemFonts::AfterCraft
        } else {
            SystemFonts::Off
        });
    // A chosen face heads its family; a missing italic or bold falls back to the regular.
    let named = |name: &str| FontFamily::Name(name.into());
    let chosen = |face: &Face| {
        let mut data = FontData::from_owned(face.bytes.clone());
        data.index = face.index;
        data
    };
    let regular = faces.regular.as_ref();
    for (name, face, family) in [
        ("writing-regular", regular, WRITING),
        (
            "writing-italic",
            faces.italic.as_ref().or(regular),
            WRITING_ITALIC,
        ),
        (
            "writing-bold",
            faces.bold.as_ref().or(regular),
            WRITING_BOLD,
        ),
        ("markdown-regular", faces.markdown.as_ref(), MARKDOWN),
    ] {
        if let Some(face) = face {
            fonts = fonts.app_face(name, chosen(face), &[named(family)]);
        }
    }
    for (name, data) in APP_FACES {
        let families = families_of(name);
        if !families.is_empty() {
            fonts = fonts.app_face(name, FontData::from_static(data), &families);
        }
    }
    fonts = fonts
        .family(Family::named(SEMIBOLD).weight(600))
        .family(Family::named(SERIF))
        .family(Family::named(SERIF_ITALIC))
        .family(Family::named(SERIF_SEMIBOLD).weight(600))
        .family(Family::named(WRITING))
        .family(Family::named(WRITING_ITALIC))
        .family(Family::named(WRITING_BOLD).weight(600))
        .family(Family::named(MARKDOWN));
    fonts.install(ctx);
}

/// One of the faces `build.rs` embedded, by name (`None` without the craft-fonts checkout).
pub(crate) fn app_face(name: &str) -> Option<&'static [u8]> {
    APP_FACES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, data)| *data)
}

/// Whether the build has the real faces (for tests and the About window).
pub fn has_app_faces() -> bool {
    !APP_FACES.is_empty()
}

/// The writing area's text.
pub fn writing(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(WRITING.into()))
}

pub fn writing_italic(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(WRITING_ITALIC.into()))
}

pub fn writing_bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(WRITING_BOLD.into()))
}

/// The Markdown editor's text: the chosen font, else the monospace stack.
pub fn markdown(size: f32) -> FontId {
    if MARKDOWN_SET.load(std::sync::atomic::Ordering::Relaxed) {
        FontId::new(size, FontFamily::Name(MARKDOWN.into()))
    } else {
        FontId::monospace(size)
    }
}

pub fn serif(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SERIF.into()))
}

pub fn serif_italic(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SERIF_ITALIC.into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}
