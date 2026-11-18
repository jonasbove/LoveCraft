//! ProseCraft's own palette ("ProseCraft" colour scheme, light and dark), as egui visuals.
//!
//! The values are ProseCraft's CSS custom properties (`src/routes/+page.svelte`, upstream
//! `7dae544`). Where one of them misses the suite's contrast rule (4.5:1 for text against the
//! surfaces it is drawn on), it is darkened or lightened just enough, and marked so below.

use egui::{Color32, CornerRadius, Stroke, Style, Visuals, vec2};

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
        heat: [rgb(0xe6e8e1), rgb(0xb9c9b1), rgb(0x8fa985), rgb(0x64805d)],
    };

    /// The egui visuals for this palette.
    pub fn visuals(&self) -> Visuals {
        let mut v = if self.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
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

    /// Visuals plus ProseCraft's roomier spacing.
    pub fn apply(&self, ctx: &egui::Context) {
        let visuals = self.visuals();
        ctx.global_style_mut(|style: &mut Style| {
            style.visuals = visuals.clone();
            style.spacing.item_spacing = vec2(8.0, 6.0);
            style.spacing.button_padding = vec2(10.0, 5.0);
            style.spacing.interact_size.y = 26.0;
        });
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
        for p in [Palette::DARK, Palette::LIGHT] {
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
                        "{} theme: {tn} on {sn} is {ratio:.2}:1",
                        if p.dark { "dark" } else { "light" }
                    );
                }
            }
        }
    }
}
