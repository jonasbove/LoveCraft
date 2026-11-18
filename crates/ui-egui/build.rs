//! Embeds the interface and manuscript fonts from a craft-fonts checkout (`CRAFT_FONTS_DIR`).
//!
//! Font files live in storytold/craft-fonts, not in this repository (Rust house rule 3). The
//! checkout is optional: without it the app builds and uses egui's and the system's fonts.
//! `CRAFT_FONTS_REQUIRED=1` (release builds) makes a missing checkout an error.

use std::fmt::Write as _;
use std::path::PathBuf;

/// The app's own faces: (egui font name, file in the checkout).
const APP_FACES: &[(&str, &str)] = &[
    ("Inter-Regular", "fonts/inter/Inter-Regular.ttf"),
    ("Inter-SemiBold", "fonts/inter/Inter-SemiBold.ttf"),
    ("Inter-Bold", "fonts/inter/Inter-Bold.ttf"),
    ("Inter-Italic", "fonts/inter/Inter-Italic.ttf"),
    (
        "SourceSerif4-Regular",
        "fonts/source-serif-4/SourceSerif4-Regular.ttf",
    ),
    (
        "SourceSerif4-It",
        "fonts/source-serif-4/SourceSerif4-It.ttf",
    ),
    (
        "SourceSerif4-Semibold",
        "fonts/source-serif-4/SourceSerif4-Semibold.ttf",
    ),
    (
        "SourceSerif4-Bold",
        "fonts/source-serif-4/SourceSerif4-Bold.ttf",
    ),
];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default());
    let required = std::env::var("CRAFT_FONTS_REQUIRED").is_ok_and(|v| v == "1");
    let dir = std::env::var_os("CRAFT_FONTS_DIR")
        .map(PathBuf::from)
        .filter(|d| !d.as_os_str().is_empty());

    let mut app = String::from("&[\n");
    if let Some(dir) = &dir {
        for (name, file) in APP_FACES {
            let path = dir.join(file);
            println!("cargo::rerun-if-changed={}", path.display());
            if path.is_file() {
                let _ = writeln!(
                    app,
                    "    ({name:?}, include_bytes!({:?}) as &[u8]),",
                    path.display().to_string()
                );
            } else if required {
                println!(
                    "cargo::error=CRAFT_FONTS_REQUIRED is set but {} is missing",
                    path.display()
                );
            } else {
                println!(
                    "cargo::warning=building without {name}: {} is missing",
                    path.display()
                );
            }
        }
    } else if required {
        println!("cargo::error=CRAFT_FONTS_REQUIRED is set but CRAFT_FONTS_DIR is missing");
    }
    app.push_str("]\n");
    if let Err(e) = std::fs::write(out.join("app_fonts.rs"), app) {
        println!("cargo::error=writing app_fonts.rs: {e}");
    }

    // Fallback faces from the manifest: the symbol faces (✱, ⁂, arrows). Other scripts and emoji
    // come from the installed system fonts on the desktop; the web build adds served faces later.
    let embedded = craft_fonts_build::Embed::new()
        .filter(|face| face.scripts.iter().any(|s| s == "Zsym"))
        .write("craft_fonts.rs");
    if let Err(e) = embedded {
        println!("cargo::error={e}");
    }
}
