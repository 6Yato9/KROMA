//! Kroma's palette, as egui is told about it.
//!
//! The numbers themselves are in `pe-theme`, which depends on nothing and is
//! read by the Mac shell too. What is left here is the glue: one conversion,
//! the palette under egui's own colour type so no call site has to say so, and
//! the one place egui's stock dark theme is overwritten.
//!
//! The reasoning behind each colour — why the surround is darker than the
//! picture, why the accent is spent on so little, why the ramps are hand-picked
//! rather than converted through a linear HSV — is on the constants in
//! `pe-theme`, where the second shell can read it.

use egui::Color32;

pub use pe_theme::{CHANNEL_AXES, Ramp, Rgb8, ramp_for};

/// A palette colour, as egui wants it.
///
/// `pe-theme` knows nothing about egui, which is the point of it: this is the
/// entire cost of that on this side.
pub const fn c(x: Rgb8) -> Color32 {
    Color32::from_rgb(x.r, x.g, x.b)
}

/// The palette, converted once.
///
/// A name-for-name mirror of `pe_theme::colour` and nothing else — not one
/// number appears here, so there is nothing that could drift. It exists so
/// every call site across the shell goes on reading `colour::PANEL` and getting
/// a `Color32`, rather than each one converting for itself.
pub mod colour {
    use super::c;
    use egui::Color32;

    pub const VIEWER: Color32 = c(pe_theme::colour::VIEWER);
    pub const WELL: Color32 = c(pe_theme::colour::WELL);
    pub const PANEL: Color32 = c(pe_theme::colour::PANEL);
    pub const RAISED: Color32 = c(pe_theme::colour::RAISED);
    pub const CONTROL: Color32 = c(pe_theme::colour::CONTROL);
    pub const CONTROL_HOT: Color32 = c(pe_theme::colour::CONTROL_HOT);
    pub const RULE: Color32 = c(pe_theme::colour::RULE);
    pub const BOX_EDGE: Color32 = c(pe_theme::colour::BOX_EDGE);
    pub const BOX_FILL: Color32 = c(pe_theme::colour::BOX_FILL);
    pub const TITLE: Color32 = c(pe_theme::colour::TITLE);
    pub const LABEL: Color32 = c(pe_theme::colour::LABEL);
    pub const DIM: Color32 = c(pe_theme::colour::DIM);
    pub const ICON: Color32 = c(pe_theme::colour::ICON);
    pub const TRACK: Color32 = c(pe_theme::colour::TRACK);
    pub const TRACK_FILL: Color32 = c(pe_theme::colour::TRACK_FILL);
    pub const HANDLE: Color32 = c(pe_theme::colour::HANDLE);
    pub const HANDLE_HOT: Color32 = c(pe_theme::colour::HANDLE_HOT);
    pub const HANDLE_EDGE: Color32 = c(pe_theme::colour::HANDLE_EDGE);
    pub const GRID: Color32 = c(pe_theme::colour::GRID);
    pub const ACCENT: Color32 = c(pe_theme::colour::ACCENT);
    pub const ACCENT_DIM: Color32 = c(pe_theme::colour::ACCENT_DIM);
    pub const SELECT: Color32 = c(pe_theme::colour::SELECT);
    pub const WARN: Color32 = c(pe_theme::colour::WARN);
    pub const ERROR: Color32 = c(pe_theme::colour::ERROR);

    /// Red, green and blue, in the order a parade draws them.
    pub const CHANNEL: [Color32; 3] = [
        c(pe_theme::colour::CHANNEL_R),
        c(pe_theme::colour::CHANNEL_G),
        c(pe_theme::colour::CHANNEL_B),
    ];
}

/// Hand egui the scheme.
///
/// Called once at startup. Everything drawn by hand reads `colour` directly;
/// this is for the widgets egui draws itself — combo boxes, checkboxes,
/// scrollbars, tooltips — which would otherwise arrive in its stock blue-grey
/// dark theme and sit in the panel looking borrowed.
pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = colour::PANEL;
    visuals.window_fill = colour::RAISED;
    visuals.extreme_bg_color = colour::WELL;
    visuals.faint_bg_color = Color32::from_rgb(38, 38, 38);
    visuals.code_bg_color = colour::WELL;
    visuals.window_stroke = egui::Stroke::new(1.0_f32, colour::RULE);
    visuals.window_corner_radius = 4.into();
    visuals.menu_corner_radius = 3.into();
    visuals.hyperlink_color = colour::ACCENT;
    // `.weak()` text is a named colour rather than the body colour faded, so
    // secondary text is the same grey everywhere instead of depending on
    // whatever it happens to be sitting on.
    visuals.weak_text_color = Some(colour::DIM);
    visuals.warn_fg_color = colour::WARN;
    visuals.error_fg_color = colour::ERROR;
    visuals.selection = egui::style::Selection {
        bg_fill: colour::SELECT,
        stroke: egui::Stroke::new(1.0_f32, colour::TITLE),
    };
    // Resolve's panels are flat. A drop shadow under every popup is the
    // single loudest thing egui does by default, and it reads as a web page.
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;

    let w = &mut visuals.widgets;
    w.noninteractive.bg_fill = colour::PANEL;
    w.noninteractive.weak_bg_fill = colour::PANEL;
    w.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, colour::RULE);
    w.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, colour::LABEL);

    w.inactive.bg_fill = colour::CONTROL;
    w.inactive.weak_bg_fill = colour::CONTROL;
    w.inactive.bg_stroke = egui::Stroke::new(1.0_f32, colour::BOX_EDGE);
    w.inactive.fg_stroke = egui::Stroke::new(1.0_f32, colour::LABEL);

    w.hovered.bg_fill = colour::CONTROL_HOT;
    w.hovered.weak_bg_fill = colour::CONTROL_HOT;
    w.hovered.bg_stroke = egui::Stroke::new(1.0_f32, colour::HANDLE);
    w.hovered.fg_stroke = egui::Stroke::new(1.0_f32, colour::TITLE);

    w.active.bg_fill = colour::CONTROL_HOT;
    w.active.weak_bg_fill = colour::CONTROL_HOT;
    w.active.bg_stroke = egui::Stroke::new(1.0_f32, colour::HANDLE_HOT);
    w.active.fg_stroke = egui::Stroke::new(1.0_f32, colour::TITLE);

    w.open.bg_fill = colour::RAISED;
    w.open.weak_bg_fill = colour::RAISED;
    w.open.bg_stroke = egui::Stroke::new(1.0_f32, colour::BOX_EDGE);
    w.open.fg_stroke = egui::Stroke::new(1.0_f32, colour::TITLE);

    // Small radii throughout. Resolve rounds its controls just enough to
    // soften the corner; anything more starts to look like a phone.
    for v in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        v.corner_radius = 2.into();
        v.expansion = 0.0;
    }

    let mut style = (*ctx.style()).clone();
    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(6.0, 3.0);
    style.spacing.button_padding = egui::vec2(7.0, 3.0);
    style.spacing.interact_size.y = 20.0;
    style.spacing.slider_width = 120.0;
    style.spacing.combo_height = 400.0;
    style.spacing.menu_margin = egui::Margin::same(4);
    // Thin, and only visible once there is something to scroll — a fat
    // permanent scrollbar in a 300-point inspector is a tenth of the panel.
    style.spacing.scroll = egui::style::ScrollStyle {
        bar_width: 7.0,
        floating_allocated_width: 0.0,
        ..egui::style::ScrollStyle::floating()
    };
    for (text_style, size) in [
        (egui::TextStyle::Body, 12.0),
        (egui::TextStyle::Button, 12.0),
        (egui::TextStyle::Small, 10.5),
        (egui::TextStyle::Monospace, 11.5),
        (egui::TextStyle::Heading, 15.0),
    ] {
        if let Some(font) = style.text_styles.get_mut(&text_style) {
            font.size = size;
        }
    }
    ctx.set_style(style);
}
