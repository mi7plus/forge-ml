//! Palette, visuals, and small styled-widget helpers shared across the UI.

use eframe::egui;
use egui::{Color32, Frame, Margin, RichText, Stroke};

pub const TEXT: Color32 = Color32::PLACEHOLDER;
pub const MUTED: Color32 = Color32::PLACEHOLDER;
pub const EMBER: Color32 = Color32::from_rgb(196, 119, 44);
pub const CYAN: Color32 = Color32::from_rgb(39, 141, 204);
pub const GREEN: Color32 = Color32::from_rgb(46, 157, 96);
pub const RED: Color32 = Color32::from_rgb(212, 72, 85);

pub struct ThemeColors {
    pub background: Color32,
    pub surface: Color32,
    pub raised: Color32,
    pub menu: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
}

pub fn theme_colors(dark: bool) -> ThemeColors {
    if dark {
        ThemeColors {
            background: Color32::from_rgb(20, 24, 31),
            surface: Color32::from_rgb(27, 33, 42),
            raised: Color32::from_rgb(35, 42, 53),
            menu: Color32::from_rgb(16, 20, 26),
            border: Color32::from_rgb(62, 73, 88),
            text: Color32::from_rgb(226, 231, 238),
            muted: Color32::from_rgb(164, 174, 188),
        }
    } else {
        ThemeColors {
            background: Color32::from_rgb(248, 249, 251),
            surface: Color32::from_rgb(236, 239, 243),
            raised: Color32::from_rgb(255, 255, 255),
            menu: Color32::from_rgb(224, 228, 234),
            border: Color32::from_rgb(166, 174, 185),
            text: Color32::from_rgb(25, 30, 38),
            muted: Color32::from_rgb(76, 86, 99),
        }
    }
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

pub fn configure_style(ctx: &egui::Context, dark: bool, high_contrast: bool) {
    let theme = if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(theme);
    let colors = theme_colors(dark);
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = colors.background;
    visuals.window_fill = colors.surface;
    visuals.extreme_bg_color = colors.raised;
    visuals.faint_bg_color = colors.raised;
    visuals.selection.bg_fill = if dark {
        Color32::from_rgb(35, 86, 119)
    } else {
        Color32::from_rgb(184, 218, 240)
    };
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
    visuals.widgets.hovered.bg_fill = if dark {
        Color32::from_rgb(43, 61, 76)
    } else {
        Color32::from_rgb(211, 229, 241)
    };
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, CYAN);
    visuals.widgets.active.bg_fill = if dark {
        Color32::from_rgb(49, 72, 90)
    } else {
        Color32::from_rgb(196, 220, 236)
    };
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
