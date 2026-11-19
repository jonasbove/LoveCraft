//! ProseCraft's own palette ("ProseCraft" colour scheme, light and dark), as egui visuals.
//!
//! The values are ProseCraft's CSS custom properties (`src/routes/+page.svelte`, upstream
//! `7dae544`). Where one of them misses the suite's contrast rule (4.5:1 for text against the
//! surfaces it is drawn on), it is darkened or lightened just enough, and marked so below.

use egui::{Color32, CornerRadius, Stroke, Style, Visuals, vec2};

/// Set by [`crate::setup_for_tests`]: the text caret doesn't blink.
pub(crate) static STEADY_CURSOR: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// The colour tokens the interface uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub page_bg: Color32,
    pub sidebar_bg: Color32,
    pub surface: Color32,
    pub surface_raised: Color32,
    pub border: Color32,
    pub border_control: Color32,
    pub text_primary: Color32,
    pub text_heading: Color32,
    pub text_body: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub hover_bg: Color32,
    pub selected_bg: Color32,
    pub selected_text: Color32,
    pub control_bg: Color32,
    pub eyebrow: Color32,
    pub quote: Color32,
    pub link: Color32,
    pub accent: Color32,
    pub accent_strong: Color32,
    pub danger: Color32,
    pub warning: Color32,
    /// `--success`: the relation network's "friend" colour.
    pub success: Color32,
    /// `--text-faint`: suggested relations in the network.
    pub text_faint: Color32,
    pub heat: [Color32; 4],
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Palette {
    pub const DARK: Self = Self {
        dark: true,
        page_bg: rgb(0x191b19),
        sidebar_bg: rgb(0x202320),
        surface: rgb(0x20221f),
        surface_raised: rgb(0x282b27),
        border: rgb(0x353933),
        border_control: rgb(0x454a42),
        text_primary: rgb(0xe2e5df),
        text_heading: rgb(0xe1e5dd),
        text_body: rgb(0xc6cbc2),
        text_secondary: rgb(0xadb3a9),
        // ProseCraft's #9da397 is 4.4:1 on the selected row; lightened a touch.
        text_muted: rgb(0x9fa599),
        hover_bg: rgb(0x2d312c),
        selected_bg: rgb(0x343c33),
        selected_text: rgb(0xb6cbae),
        control_bg: rgb(0x292c28),
        eyebrow: rgb(0xa8b69d),
        quote: rgb(0xadb8a6),
        link: rgb(0xb2cba8),
        accent: rgb(0x81977b),
        accent_strong: rgb(0x536d58),
        // ProseCraft's #ba6158 is 3.6:1 on the dark surfaces; lightened to pass 4.5:1.
        danger: rgb(0xe48c82),
        warning: rgb(0xc2a16c),
        success: rgb(0x4c6a51),
        text_faint: rgb(0x969d91),
        heat: [rgb(0x2a2e29), rgb(0x64805d), rgb(0x8fa985), rgb(0xb9c9b1)],
    };

    pub const LIGHT: Self = Self {
        dark: false,
        page_bg: rgb(0xf8f8f6),
        sidebar_bg: rgb(0xf1f1ed),
        surface: rgb(0xfbfbf9),
        surface_raised: rgb(0xffffff),
        border: rgb(0xe8e8e3),
        border_control: rgb(0xe0e2db),
        text_primary: rgb(0x272a27),
        text_heading: rgb(0x323832),
        text_body: rgb(0x565b53),
        // ProseCraft's #73766e is 3.7:1 on the hover and selected rows; darkened.
        text_secondary: rgb(0x666962),
        // ProseCraft's #999b94 is 2.6:1 on the sidebar; darkened to pass 4.5:1.
        text_muted: rgb(0x666961),
        hover_bg: rgb(0xe9eae5),
        selected_bg: rgb(0xe6e8e1),
        selected_text: rgb(0x3e5845),
        control_bg: rgb(0xfbfbf9),
        // ProseCraft's #87917f is 3.0:1; darkened to pass 4.5:1.
        eyebrow: rgb(0x5f6858),
        quote: rgb(0x5e665a),
        // ProseCraft's #55715b is 4.4:1 on the selected row; darkened a touch.
        link: rgb(0x526e58),
        accent: rgb(0x81977b),
        accent_strong: rgb(0x536d58),
        // ProseCraft's #ba6158 is 3.9:1 on the light surfaces; darkened to pass 4.5:1.
        danger: rgb(0xa14f47),
        // ProseCraft's #c2a16c is 2.2:1; darkened to pass 4.5:1.
        warning: rgb(0x7d6430),
        success: rgb(0x4c6a51),
        text_faint: rgb(0xa0a198),
        heat: [rgb(0xe6e8e1), rgb(0xb9c9b1), rgb(0x8fa985), rgb(0x64805d)],
    };

    /// The egui visuals for this palette.
    pub fn visuals(&self) -> Visuals {
        let mut v = if self.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        // Tests keep the caret steady: its blink follows the clock, and would make how many
        // frames a test takes to settle depend on how loaded the machine is.
        v.text_cursor.blink = !STEADY_CURSOR.load(std::sync::atomic::Ordering::Relaxed);
        let radius = CornerRadius::same(6);
        v.panel_fill = self.page_bg;
        v.window_fill = self.surface_raised;
        v.window_stroke = Stroke::new(1.0, self.border);
        v.window_corner_radius = CornerRadius::same(10);
        v.extreme_bg_color = self.control_bg;
        v.faint_bg_color = self.surface;
        v.code_bg_color = self.surface_raised;
        v.hyperlink_color = self.link;
        v.error_fg_color = self.danger;
        v.warn_fg_color = self.warning;
        v.selection.bg_fill = self.selected_bg;
        v.selection.stroke = Stroke::new(1.0, self.selected_text);
        v.override_text_color = None;

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.surface;
        w.noninteractive.weak_bg_fill = self.surface;
        w.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        w.noninteractive.fg_stroke = Stroke::new(1.0, self.text_body);
        w.noninteractive.corner_radius = radius;
        w.inactive.bg_fill = self.control_bg;
        w.inactive.weak_bg_fill = self.control_bg;
        w.inactive.bg_stroke = Stroke::new(1.0, self.border_control);
        w.inactive.fg_stroke = Stroke::new(1.0, self.text_primary);
        w.inactive.corner_radius = radius;
        w.hovered.bg_fill = self.hover_bg;
        w.hovered.weak_bg_fill = self.hover_bg;
        w.hovered.bg_stroke = Stroke::new(1.0, self.border_control);
        w.hovered.fg_stroke = Stroke::new(1.0, self.text_primary);
        w.hovered.corner_radius = radius;
        w.active.bg_fill = self.selected_bg;
        w.active.weak_bg_fill = self.selected_bg;
        w.active.bg_stroke = Stroke::new(1.0, self.accent);
        w.active.fg_stroke = Stroke::new(1.0, self.text_primary);
        w.active.corner_radius = radius;
        w.open = w.active;
        v
    }

    /// Visuals plus ProseCraft's roomier spacing, in both of egui's styles (its dark and light
    /// slots): the app chooses the palette itself, so whichever slot egui uses shows it.
    pub fn apply(&self, ctx: &egui::Context) {
        let visuals = self.visuals();
        ctx.all_styles_mut(|style: &mut Style| {
            style.visuals = visuals.clone();
            style.spacing.item_spacing = vec2(8.0, 6.0);
            style.spacing.button_padding = vec2(10.0, 5.0);
            style.spacing.interact_size.y = 26.0;
        });
    }
}

/// ProseCraft's `localStorage` key for the Color scheme preference.
pub const SCHEME_KEY: &str = "prosecraft-color-scheme";

/// `colorSchemeOptions`: each scheme's value and label.
pub const SCHEMES: [(&str, &str); 8] = [
    ("prosecraft", "ProseCraft"),
    ("nord", "Nord"),
    ("gruvbox", "Gruvbox"),
    ("solarized", "Solarized"),
    ("catppuccin", "Catppuccin"),
    ("everforest", "Everforest"),
    ("rose-pine", "Rosé Pine"),
    ("dracula", "Dracula"),
];

/// A scheme's primitives (`--p-*`), which the shared mapping turns into the tokens.
#[derive(Clone, Copy)]
struct Primitives {
    base0: Color32,
    base1: Color32,
    base2: Color32,
    base3: Color32,
    fg_strong: Color32,
    fg: Color32,
    fg_muted: Color32,
    fg_faint: Color32,
    accent: Color32,
    #[allow(dead_code)]
    accent_soft: Color32,
    accent_strong: Color32,
    #[allow(dead_code)]
    accent_contrast: Color32,
    selected_bg: Color32,
    selected_text: Color32,
    danger: Color32,
    warning: Color32,
    success: Color32,
}

/// Each scheme's primitives (`src/lib/color-schemes.css`): light, then dark.
const PRIMITIVES: [(&str, [Primitives; 2]); 7] = [
    (
        "nord",
        [
            Primitives {
                base0: rgb(0xeceff4),
                base1: rgb(0xe5e9f0),
                base2: rgb(0xf8f9fb),
                base3: rgb(0xffffff),
                fg_strong: rgb(0x2e3440),
                fg: rgb(0x3b4252),
                fg_muted: rgb(0x5f6b7a),
                fg_faint: rgb(0x8a94a3),
                accent: rgb(0x5e81ac),
                accent_soft: rgb(0x81a1c1),
                accent_strong: rgb(0x5e81ac),
                accent_contrast: rgb(0xffffff),
                selected_bg: rgb(0xdfe5ec),
                selected_text: rgb(0x3b5f8a),
                danger: rgb(0xbf616a),
                warning: rgb(0x9a7400),
                success: rgb(0x4f7a4a),
            },
            Primitives {
                base0: rgb(0x2e3440),
                base1: rgb(0x262b34),
                base2: rgb(0x343b48),
                base3: rgb(0x3b4252),
                fg_strong: rgb(0xeceff4),
                fg: rgb(0xd8dee9),
                fg_muted: rgb(0xa3adbd),
                fg_faint: rgb(0x7f8b9e),
                accent: rgb(0x88c0d0),
                accent_soft: rgb(0x81a1c1),
                accent_strong: rgb(0x5e81ac),
                accent_contrast: rgb(0xffffff),
                selected_bg: rgb(0x3b4a5e),
                selected_text: rgb(0xd8e5f0),
                danger: rgb(0xbf616a),
                warning: rgb(0xebcb8b),
                success: rgb(0xa3be8c),
            },
        ],
    ),
    (
        "gruvbox",
        [
            Primitives {
                base0: rgb(0xfbf1c7),
                base1: rgb(0xf2e5bc),
                base2: rgb(0xf9f0d0),
                base3: rgb(0xfffaf0),
                fg_strong: rgb(0x3c3836),
                fg: rgb(0x504945),
                fg_muted: rgb(0x7c6f64),
                fg_faint: rgb(0xa89984),
                accent: rgb(0x79740e),
                accent_soft: rgb(0xb8bb26),
                accent_strong: rgb(0x79740e),
                accent_contrast: rgb(0xffffff),
                selected_bg: rgb(0xe8d9a8),
                selected_text: rgb(0x5f5a08),
                danger: rgb(0x9d0006),
                warning: rgb(0xb57614),
                success: rgb(0x6f7d00),
            },
            Primitives {
                base0: rgb(0x282828),
                base1: rgb(0x1d2021),
                base2: rgb(0x32302f),
                base3: rgb(0x3c3836),
                fg_strong: rgb(0xfbf1c7),
                fg: rgb(0xebdbb2),
                fg_muted: rgb(0xbdae93),
                fg_faint: rgb(0xa89984),
                accent: rgb(0xb8bb26),
                accent_soft: rgb(0xd79921),
                accent_strong: rgb(0xb8bb26),
                accent_contrast: rgb(0x282828),
                selected_bg: rgb(0x4a4632),
                selected_text: rgb(0xf0e6c8),
                danger: rgb(0xfb4934),
                warning: rgb(0xfabd2f),
                success: rgb(0xb8bb26),
            },
        ],
    ),
    (
        "solarized",
        [
            Primitives {
                base0: rgb(0xfdf6e3),
                base1: rgb(0xeee8d5),
                base2: rgb(0xf7f0dd),
                base3: rgb(0xfffaf0),
                fg_strong: rgb(0x073642),
                fg: rgb(0x586e75),
                fg_muted: rgb(0x657b83),
                fg_faint: rgb(0x93a1a1),
                accent: rgb(0x268bd2),
                accent_soft: rgb(0x7aa6c2),
                accent_strong: rgb(0x268bd2),
                accent_contrast: rgb(0xffffff),
                selected_bg: rgb(0xe8e2ca),
                selected_text: rgb(0x1a6fa8),
                danger: rgb(0xdc322f),
                warning: rgb(0xb58900),
                success: rgb(0x859900),
            },
            Primitives {
                base0: rgb(0x002b36),
                base1: rgb(0x00252f),
                base2: rgb(0x073642),
                base3: rgb(0x0b3f4d),
                fg_strong: rgb(0xfdf6e3),
                fg: rgb(0x93a1a1),
                fg_muted: rgb(0x839496),
                fg_faint: rgb(0x657b83),
                accent: rgb(0x268bd2),
                accent_soft: rgb(0x2aa198),
                accent_strong: rgb(0x268bd2),
                accent_contrast: rgb(0xffffff),
                selected_bg: rgb(0x0d4a5c),
                selected_text: rgb(0xb8e0e8),
                danger: rgb(0xdc322f),
                warning: rgb(0xb58900),
                success: rgb(0x859900),
            },
        ],
    ),
    (
        "catppuccin",
        [
            Primitives {
                base0: rgb(0xeff1f5),
                base1: rgb(0xe6e9ef),
                base2: rgb(0xf7f8fa),
                base3: rgb(0xffffff),
                fg_strong: rgb(0x4c4f69),
                fg: rgb(0x5c5f77),
                fg_muted: rgb(0x6c6f85),
                fg_faint: rgb(0x9ca0b0),
                accent: rgb(0x8839ef),
                accent_soft: rgb(0xb79bea),
                accent_strong: rgb(0x8839ef),
                accent_contrast: rgb(0xffffff),
                selected_bg: rgb(0xe4dcf7),
                selected_text: rgb(0x6c2bd9),
                danger: rgb(0xd20f39),
                warning: rgb(0xdf8e1d),
                success: rgb(0x40a02b),
            },
            Primitives {
                base0: rgb(0x1e1e2e),
                base1: rgb(0x181825),
                base2: rgb(0x313244),
                base3: rgb(0x45475a),
                fg_strong: rgb(0xf5f5ff),
                fg: rgb(0xcdd6f4),
                fg_muted: rgb(0xa6adc8),
                fg_faint: rgb(0x7f849c),
                accent: rgb(0xcba6f7),
                accent_soft: rgb(0xb4befe),
                accent_strong: rgb(0xcba6f7),
                accent_contrast: rgb(0x1e1e2e),
                selected_bg: rgb(0x3d3552),
                selected_text: rgb(0xe6dcff),
                danger: rgb(0xf38ba8),
                warning: rgb(0xf9e2af),
                success: rgb(0xa6e3a1),
            },
        ],
    ),
    (
        "everforest",
        [
            Primitives {
                base0: rgb(0xfdf6e3),
                base1: rgb(0xf6f0d9),
                base2: rgb(0xefebd4),
                base3: rgb(0xfffdf5),
                fg_strong: rgb(0x3d484d),
                fg: rgb(0x5c6a72),
                fg_muted: rgb(0x829181),
                fg_faint: rgb(0xa6b0a0),
                accent: rgb(0x8da101),
                accent_soft: rgb(0xa7c080),
                accent_strong: rgb(0x6f7d00),
                accent_contrast: rgb(0xffffff),
                selected_bg: rgb(0xe6e5c8),
                selected_text: rgb(0x6f7d00),
                danger: rgb(0xf85552),
                warning: rgb(0xdfa000),
                success: rgb(0x8da101),
            },
            Primitives {
                base0: rgb(0x2d353b),
                base1: rgb(0x232a2e),
                base2: rgb(0x343f44),
                base3: rgb(0x3d484d),
                fg_strong: rgb(0xf0ead2),
                fg: rgb(0xd3c6aa),
                fg_muted: rgb(0xa7b39a),
                fg_faint: rgb(0x859289),
                accent: rgb(0xa7c080),
                accent_soft: rgb(0x83c092),
                accent_strong: rgb(0xa7c080),
                accent_contrast: rgb(0x2d353b),
                selected_bg: rgb(0x425047),
                selected_text: rgb(0xd3e6b8),
                danger: rgb(0xe67e80),
                warning: rgb(0xdbbc7f),
                success: rgb(0x83c092),
            },
        ],
    ),
    (
        "rose-pine",
        [
            Primitives {
                base0: rgb(0xfaf4ed),
                base1: rgb(0xf2e9e1),
                base2: rgb(0xfffaf3),
                base3: rgb(0xfffdf9),
                fg_strong: rgb(0x575279),
                fg: rgb(0x625d7e),
                fg_muted: rgb(0x797593),
                fg_faint: rgb(0x9893a5),
                accent: rgb(0x907aa9),
                accent_soft: rgb(0xb8a3cd),
                accent_strong: rgb(0x907aa9),
                accent_contrast: rgb(0xffffff),
                selected_bg: rgb(0xece2ec),
                selected_text: rgb(0x6f5a89),
                danger: rgb(0xb4637a),
                warning: rgb(0xea9d34),
                success: rgb(0x286983),
            },
            Primitives {
                base0: rgb(0x191724),
                base1: rgb(0x1f1d2e),
                base2: rgb(0x26233a),
                base3: rgb(0x2a273f),
                fg_strong: rgb(0xe0def4),
                fg: rgb(0xe0def4),
                fg_muted: rgb(0x908caa),
                fg_faint: rgb(0x6e6a86),
                accent: rgb(0xc4a7e7),
                accent_soft: rgb(0xebbcba),
                accent_strong: rgb(0xc4a7e7),
                accent_contrast: rgb(0x191724),
                selected_bg: rgb(0x34314a),
                selected_text: rgb(0xe6dcff),
                danger: rgb(0xeb6f92),
                warning: rgb(0xf6c177),
                success: rgb(0x9ccfd8),
            },
        ],
    ),
    (
        "dracula",
        [
            Primitives {
                base0: rgb(0xf5f5f5),
                base1: rgb(0xebebeb),
                base2: rgb(0xffffff),
                base3: rgb(0xffffff),
                fg_strong: rgb(0x1f1f1f),
                fg: rgb(0x3b3b3b),
                fg_muted: rgb(0x6c664b),
                fg_faint: rgb(0x9a9a9a),
                accent: rgb(0x644ac9),
                accent_soft: rgb(0xa89de0),
                accent_strong: rgb(0x644ac9),
                accent_contrast: rgb(0xffffff),
                selected_bg: rgb(0xe2def5),
                selected_text: rgb(0x533bb0),
                danger: rgb(0xcb3a2a),
                warning: rgb(0x846e15),
                success: rgb(0x14710a),
            },
            Primitives {
                base0: rgb(0x282a36),
                base1: rgb(0x21222c),
                base2: rgb(0x343746),
                base3: rgb(0x44475a),
                fg_strong: rgb(0xf8f8f2),
                fg: rgb(0xf8f8f2),
                fg_muted: rgb(0xb8b9c8),
                fg_faint: rgb(0x8a8fa8),
                accent: rgb(0xbd93f9),
                accent_soft: rgb(0xff79c6),
                accent_strong: rgb(0xbd93f9),
                accent_contrast: rgb(0x282a36),
                selected_bg: rgb(0x44475a),
                selected_text: rgb(0xf8f8f2),
                danger: rgb(0xff5555),
                warning: rgb(0xf1fa8c),
                success: rgb(0x50fa7b),
            },
        ],
    ),
];

/// `color-mix(in srgb, a p%, b)`.
fn mix(a: Color32, percent: f32, b: Color32) -> Color32 {
    let p = percent / 100.0;
    let channel = |x: u8, y: u8| (f32::from(x) * p + f32::from(y) * (1.0 - p)).round() as u8;
    Color32::from_rgb(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
    )
}

/// A text colour moved toward black (light) or white (dark) just far enough to be 4.5:1 on
/// every surface, as the ProseCraft palette's few adjusted values are.
fn legible(text: Color32, surfaces: &[Color32], dark: bool) -> Color32 {
    let target = if dark { Color32::WHITE } else { Color32::BLACK };
    let passes = |c: Color32| surfaces.iter().all(|s| contrast(c, *s) >= 4.5);
    (0..=100)
        .map(|step| mix(target, step as f32, text))
        .find(|c| passes(*c))
        .unwrap_or(target)
}

impl Palette {
    /// The palette of a colour scheme (`data-color-scheme`) in light or dark: ProseCraft's own,
    /// or another scheme's primitives through the shared mapping.
    pub fn for_scheme(scheme: &str, dark: bool) -> Self {
        let Some((_, modes)) = PRIMITIVES.iter().find(|(name, _)| *name == scheme) else {
            return if dark { Self::DARK } else { Self::LIGHT };
        };
        let p = modes[usize::from(dark)];
        let hover_bg = mix(p.accent, 7.0, p.base1);
        let mut out = Self {
            dark,
            page_bg: p.base0,
            sidebar_bg: p.base1,
            surface: p.base2,
            surface_raised: p.base3,
            border: mix(p.fg, 14.0, p.base1),
            border_control: mix(p.fg, 22.0, p.base1),
            text_primary: p.fg,
            text_heading: p.fg_strong,
            text_body: mix(p.fg, 88.0, p.base0),
            text_secondary: p.fg_muted,
            text_muted: p.fg_muted,
            hover_bg,
            selected_bg: p.selected_bg,
            selected_text: p.selected_text,
            control_bg: p.base2,
            eyebrow: p.accent,
            quote: p.fg_muted,
            link: mix(p.accent, 80.0, p.fg_strong),
            accent: p.accent,
            accent_strong: p.accent_strong,
            danger: p.danger,
            warning: p.warning,
            success: p.success,
            text_faint: p.fg_faint,
            heat: [
                p.base2,
                mix(p.accent_strong, 18.0, p.base1),
                mix(p.accent_strong, 42.0, p.base1),
                mix(p.accent_strong, 70.0, p.base1),
            ],
        };
        let surfaces = [
            out.page_bg,
            out.sidebar_bg,
            out.surface,
            out.surface_raised,
            out.control_bg,
            out.hover_bg,
            out.selected_bg,
        ];
        for text in [
            &mut out.text_primary,
            &mut out.text_heading,
            &mut out.text_body,
            &mut out.text_secondary,
            &mut out.text_muted,
            &mut out.selected_text,
            &mut out.eyebrow,
            &mut out.quote,
            &mut out.link,
            &mut out.danger,
            &mut out.warning,
        ] {
            *text = legible(*text, &surfaces, dark);
        }
        out
    }
}

impl Default for Palette {
    /// ProseCraft opens in its dark theme.
    fn default() -> Self {
        Self::DARK
    }
}

/// WCAG relative luminance.
fn luminance(c: Color32) -> f64 {
    let channel = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

/// WCAG contrast ratio of two colours.
pub fn contrast(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::{Palette, contrast};

    /// Accessibility rule 5: text at least 4.5:1 against every surface it is drawn on, in both
    /// themes.
    #[test]
    fn every_text_colour_passes_on_every_surface() {
        let schemes = super::SCHEMES.iter().flat_map(|(name, _)| {
            [
                Palette::for_scheme(name, true),
                Palette::for_scheme(name, false),
            ]
        });
        for p in [Palette::DARK, Palette::LIGHT].into_iter().chain(schemes) {
            let surfaces = [
                ("page", p.page_bg),
                ("sidebar", p.sidebar_bg),
                ("surface", p.surface),
                ("raised", p.surface_raised),
                ("control", p.control_bg),
                ("hover", p.hover_bg),
                ("selected", p.selected_bg),
            ];
            let texts = [
                ("primary", p.text_primary),
                ("heading", p.text_heading),
                ("body", p.text_body),
                ("secondary", p.text_secondary),
                ("muted", p.text_muted),
                ("selected", p.selected_text),
                ("eyebrow", p.eyebrow),
                ("quote", p.quote),
                ("link", p.link),
                ("danger", p.danger),
                ("warning", p.warning),
            ];
            for (tn, t) in texts {
                for (sn, s) in surfaces {
                    let ratio = contrast(t, s);
                    assert!(
                        ratio >= 4.5,
                        "{} theme ({:?}): {tn} on {sn} is {ratio:.2}:1",
                        if p.dark { "dark" } else { "light" },
                        p.page_bg
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod scheme_tests {
    use super::{Palette, mix, rgb};

    #[test]
    fn schemes_map_their_primitives_as_the_shared_css_does() {
        // Nord light: `--page-bg` is base0, `--border` mixes 14% of fg into base1.
        let nord = Palette::for_scheme("nord", false);
        assert_eq!(nord.page_bg, rgb(0xeceff4));
        assert_eq!(nord.sidebar_bg, rgb(0xe5e9f0));
        assert_eq!(nord.border, mix(rgb(0x3b4252), 14.0, rgb(0xe5e9f0)));
        assert_eq!(nord.text_heading, rgb(0x2e3440));
        // ProseCraft keeps its own values; an unknown scheme is ProseCraft's.
        assert_eq!(Palette::for_scheme("prosecraft", true), Palette::DARK);
        assert_eq!(Palette::for_scheme("unknown", false), Palette::LIGHT);
        assert_eq!(mix(rgb(0x000000), 50.0, rgb(0xffffff)), rgb(0x808080));
    }
}
