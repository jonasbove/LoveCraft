//! The app's fonts: Inter for the interface (as ProseCraft uses) and Source Serif 4 for the
//! manuscript and headings (ProseCraft uses Georgia, which isn't freely licensed), over craft-ui's
//! shared font stack, with the system's fonts for emoji and every other script.

use craft_fonts_build::EmbeddedFace;
use craft_ui::fonts::{CraftFace, Family, I18nScripts, UiFonts};
use egui::{FontData, FontFamily, FontId};

/// The faces `build.rs` embedded from craft-fonts (empty without a checkout).
static APP_FACES: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/app_fonts.rs"));
static CRAFT_FONTS: &[EmbeddedFace] = include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

pub const SEMIBOLD: &str = "semibold";
pub const SERIF: &str = "serif";
pub const SERIF_ITALIC: &str = "serif-italic";
pub const SERIF_SEMIBOLD: &str = "serif-semibold";

/// The families each embedded face heads.
fn families_of(face: &str) -> Vec<FontFamily> {
    let named = |name: &str| FontFamily::Name(name.into());
    match face {
        "Inter-Regular" => vec![FontFamily::Proportional],
        "Inter-SemiBold" => vec![named(SEMIBOLD)],
        "SourceSerif4-Regular" => vec![named(SERIF)],
        "SourceSerif4-It" => vec![named(SERIF_ITALIC)],
        "SourceSerif4-Semibold" => vec![named(SERIF_SEMIBOLD)],
        _ => Vec::new(),
    }
}

/// Installs the stack in `ctx`. Every named family exists even without craft-fonts (egui's
/// default faces then stand in), so text set in them never fails.
pub fn install(ctx: &egui::Context) {
    let craft: Vec<CraftFace> = CRAFT_FONTS.iter().cloned().map(CraftFace::from).collect();
    let mut fonts = UiFonts::new(&I18nScripts)
        .languages(&["en"])
        .ui_language("en")
        .craft_faces(&craft);
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
        .family(Family::named(SERIF_SEMIBOLD).weight(600));
    fonts.install(ctx);
}

/// Whether the build has the real faces (for tests and the About window).
pub fn has_app_faces() -> bool {
    !APP_FACES.is_empty()
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
