//! Palette, visuals, and small styled-widget helpers shared across the UI.
//!
//! Base colors flow from a runtime [`Palette`] (built-in Dark/Light or a
//! user-authored custom theme). The four status accents (info/warn/ok/error)
//! stay fixed for legibility. [`theme_colors`] reads the currently active
//! palette, so all existing call sites automatically follow the active theme.

use eframe::egui;
use egui::{Color32, Frame, Margin, RichText, Stroke};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

pub const TEXT: Color32 = Color32::PLACEHOLDER;
pub const MUTED: Color32 = Color32::PLACEHOLDER;
pub const EMBER: Color32 = Color32::from_rgb(196, 119, 44);
pub const CYAN: Color32 = Color32::from_rgb(39, 141, 204);
pub const GREEN: Color32 = Color32::from_rgb(46, 157, 96);
pub const RED: Color32 = Color32::from_rgb(212, 72, 85);

/// A serializable base color palette that a theme is built from. Each field is
/// an RGB triple; `dark` hints egui's base light/dark visuals and cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    pub background: [u8; 3],
    pub surface: [u8; 3],
    pub raised: [u8; 3],
    pub menu: [u8; 3],
    pub border: [u8; 3],
    pub text: [u8; 3],
    pub muted: [u8; 3],
    #[serde(default)]
    pub dark: bool,
}

impl Palette {
    /// The built-in dark palette (the historical default look).
    pub fn dark() -> Self {
        Self {
            background: [20, 24, 31],
            surface: [27, 33, 42],
            raised: [35, 42, 53],
            menu: [16, 20, 26],
            border: [62, 73, 88],
            text: [226, 231, 238],
            muted: [164, 174, 188],
            dark: true,
        }
    }

    /// The built-in light palette.
    pub fn light() -> Self {
        Self {
            background: [248, 249, 251],
            surface: [236, 239, 243],
            raised: [255, 255, 255],
            menu: [224, 228, 234],
            border: [166, 174, 185],
            text: [25, 30, 38],
            muted: [76, 86, 99],
            dark: false,
        }
    }

    /// The seven editable color slots, as (label, mutable-reference) pairs, for
    /// building a color-picker UI without duplicating the field list.
    pub fn slots(&mut self) -> [(&'static str, &mut [u8; 3]); 7] {
        [
            ("Background", &mut self.background),
            ("Surface", &mut self.surface),
            ("Raised", &mut self.raised),
            ("Menu", &mut self.menu),
            ("Border", &mut self.border),
            ("Text", &mut self.text),
            ("Muted", &mut self.muted),
        ]
    }

    /// Render a triple as `#rrggbb`, for display beside a color swatch.
    pub fn to_hex(rgb: [u8; 3]) -> String {
        format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
    }
}

/// A named theme = a label plus its palette. Built-in themes are not stored;
/// only user-authored ones live in this list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedTheme {
    pub name: String,
    pub palette: Palette,
}

thread_local! {
    static ACTIVE: RefCell<Palette> = RefCell::new(Palette::dark());
}

/// Set the palette that [`theme_colors`] returns until changed. Called by the
/// styling pass whenever the active theme changes.
pub fn set_active_palette(palette: &Palette) {
    ACTIVE.with(|active| *active.borrow_mut() = palette.clone());
}

/// The currently active palette.
pub fn active_palette() -> Palette {
    ACTIVE.with(|active| active.borrow().clone())
}

fn c(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Linear blend of two colors (t in 0..=1 moves from `a` toward `b`).
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(
        lerp(a.r(), b.r()),
        lerp(a.g(), b.g()),
        lerp(a.b(), b.b()),
    )
}

pub struct ThemeColors {
    pub background: Color32,
    pub surface: Color32,
    pub raised: Color32,
    pub menu: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
}

impl From<&Palette> for ThemeColors {
    fn from(p: &Palette) -> Self {
        ThemeColors {
            background: c(p.background),
            surface: c(p.surface),
            raised: c(p.raised),
            menu: c(p.menu),
            border: c(p.border),
            text: c(p.text),
            muted: c(p.muted),
        }
    }
}

/// Colors of the active theme. The `dark` argument is retained for source
/// compatibility but ignored — the active palette (set by the styling pass) is
/// authoritative, so custom themes flow through every existing call site.
pub fn theme_colors(_dark: bool) -> ThemeColors {
    ThemeColors::from(&active_palette())
}

pub fn panel_frame(fill: Color32, dark: bool) -> Frame {
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, theme_colors(dark).border))
        .inner_margin(Margin::same(12))
}

pub fn compact_panel_frame(fill: Color32, dark: bool) -> Frame {
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, theme_colors(dark).border))
        .inner_margin(Margin::symmetric(8, 4))
}

// Retained for the console styling now that the console renders inside a dock tile.
#[allow(dead_code)]
pub fn console_panel_frame(dark: bool) -> Frame {
    Frame::new()
        .fill(theme_colors(dark).surface)
        .inner_margin(Margin::same(12))
}

pub fn toolbar_icon_button(
    ui: &mut egui::Ui,
    icon: egui_phosphor_icons::Icon,
    tooltip: &str,
) -> egui::Response {
    enabled_toolbar_icon_button(ui, true, icon, tooltip)
}

pub fn enabled_toolbar_icon_button(
    ui: &mut egui::Ui,
    enabled: bool,
    icon: egui_phosphor_icons::Icon,
    tooltip: &str,
) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(icon.regular().size(17.0)).min_size(egui::vec2(30.0, 28.0)),
    )
    .on_hover_text(tooltip)
}

pub fn compact_icon_button(
    ui: &mut egui::Ui,
    icon: egui_phosphor_icons::Icon,
    tooltip: &str,
) -> egui::Response {
    ui.add(
        egui::Button::new(icon.regular().size(13.0))
            .small()
            .min_size(egui::vec2(22.0, 20.0)),
    )
    .on_hover_text(tooltip)
}

pub fn status_row(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(11.0).color(MUTED));
        ui.add_space((ui.available_width() - 70.0).max(4.0));
        ui.label(RichText::new(value).size(11.0).monospace().color(color));
    });
}

/// Apply `palette` as the active theme and rebuild egui's visuals from it.
/// Interactive states (hover/active/selection) are derived from the palette and
/// the fixed accent so custom themes stay coherent.
pub fn configure_style(ctx: &egui::Context, palette: &Palette, high_contrast: bool) {
    set_active_palette(palette);
    let dark = palette.dark;
    let colors = ThemeColors::from(palette);
    let theme = if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(theme);
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = colors.background;
    visuals.window_fill = colors.surface;
    visuals.extreme_bg_color = colors.raised;
    visuals.faint_bg_color = colors.raised;
    visuals.selection.bg_fill = mix(colors.surface, CYAN, if dark { 0.5 } else { 0.32 });
    visuals.selection.stroke = Stroke::new(1.0, CYAN);
    visuals.text_cursor.stroke = Stroke::new(
        2.5,
        if dark {
            Color32::from_rgb(88, 211, 255)
        } else {
            Color32::from_rgb(0, 67, 112)
        },
    );
    visuals.text_cursor.blink = true;
    visuals.text_cursor.on_duration = 0.7;
    visuals.text_cursor.off_duration = 0.3;
    visuals.override_text_color = Some(colors.text);
    visuals.weak_text_color = Some(colors.muted);
    visuals.hyperlink_color = CYAN;
    visuals.warn_fg_color = EMBER;
    visuals.error_fg_color = RED;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, colors.text);
    visuals.widgets.inactive.bg_fill = colors.raised;
    visuals.widgets.inactive.weak_bg_fill = colors.raised;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, colors.text);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, colors.text);
    visuals.widgets.hovered.bg_fill = mix(colors.raised, CYAN, if dark { 0.16 } else { 0.22 });
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, CYAN);
    visuals.widgets.active.bg_fill = mix(colors.raised, CYAN, if dark { 0.28 } else { 0.34 });
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, EMBER);
    if high_contrast {
        visuals.override_text_color = Some(if dark { Color32::WHITE } else { Color32::BLACK });
        visuals.weak_text_color = visuals.override_text_color;
        visuals.widgets.noninteractive.fg_stroke.width = 2.0;
        visuals.widgets.inactive.fg_stroke.width = 2.0;
        visuals.widgets.hovered.fg_stroke.width = 2.5;
        visuals.widgets.active.fg_stroke.width = 2.5;
        visuals.selection.stroke.width = 2.5;
    }
    ctx.set_visuals_of(theme, visuals);
    let mut style = (*ctx.style_of(theme)).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style_of(theme, style);
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_formats_a_triple() {
        assert_eq!(Palette::to_hex([27, 33, 42]), "#1b212a");
        assert_eq!(Palette::to_hex([255, 0, 16]), "#ff0010");
    }

    #[test]
    fn presets_are_distinct_and_serde_round_trip() {
        let dark = Palette::dark();
        assert!(dark.dark);
        assert!(!Palette::light().dark);
        assert_ne!(dark, Palette::light());
        let json = serde_json::to_string(&dark).unwrap();
        assert_eq!(serde_json::from_str::<Palette>(&json).unwrap(), dark);
    }

    #[test]
    fn active_palette_tracks_the_last_set() {
        set_active_palette(&Palette::light());
        assert_eq!(active_palette(), Palette::light());
        // theme_colors ignores its argument and follows the active palette.
        assert_eq!(theme_colors(true).background, Color32::from_rgb(248, 249, 251));
        set_active_palette(&Palette::dark());
        assert_eq!(theme_colors(false).background, Color32::from_rgb(20, 24, 31));
    }
}
