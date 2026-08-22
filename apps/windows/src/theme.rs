//! Kroma's palette, and the one place egui is told about it.
//!
//! Every colour in the application comes from here. That is not tidiness for
//! its own sake: before this file existed the greys were written as
//! `Color32::from_gray(24)` at each call site, and they had already drifted —
//! the viewer surround, the filmstrip and the status bar were three different
//! shades of what was meant to be the same background.
//!
//! The scheme is Resolve's, read off the colour page. It is built from very
//! few values, which is most of why it reads as one application rather than a
//! collection of panels:
//!
//! - four greys for surfaces, from the viewer surround up to a raised header,
//! - one hairline for every division,
//! - three text weights, and
//! - a single warm accent, spent only on what is *active*.
//!
//! Resolve's own restraint is the part worth copying. Its interface is almost
//! entirely grey, so the one orange title tells you where you are without
//! having to be loud. An accent used on every heading would say nothing.

use egui::Color32;

pub mod colour {
    use egui::Color32;

    // ---- Surfaces, darkest to lightest -----------------------------------
    /// Behind the photograph. Darkest, so nothing in the frame competes with
    /// it — a surround lighter than the picture's own shadows makes the
    /// shadows look lifted, which is a lie told to someone grading them.
    pub const VIEWER: Color32 = Color32::from_rgb(18, 18, 18);
    /// The inside of anything you type into or read a graph out of.
    pub const WELL: Color32 = Color32::from_rgb(22, 22, 22);
    /// Panel background — the inspector, the scopes, the filmstrip.
    pub const PANEL: Color32 = Color32::from_rgb(33, 33, 33);
    /// One step up: headers, the toolbar, a hovered row.
    pub const RAISED: Color32 = Color32::from_rgb(43, 43, 43);
    /// A control that sits on a panel: buttons, combo boxes, tiles.
    pub const CONTROL: Color32 = Color32::from_rgb(56, 56, 56);
    pub const CONTROL_HOT: Color32 = Color32::from_rgb(70, 70, 70);

    // ---- Lines -----------------------------------------------------------
    /// Every division in the interface is this one hairline.
    pub const RULE: Color32 = Color32::from_rgb(58, 58, 58);
    pub const BOX_EDGE: Color32 = Color32::from_rgb(70, 70, 70);
    /// The inside of the boxed number.
    pub const BOX_FILL: Color32 = Color32::from_rgb(20, 20, 20);

    // ---- Text ------------------------------------------------------------
    pub const TITLE: Color32 = Color32::from_rgb(228, 228, 228);
    pub const LABEL: Color32 = Color32::from_rgb(176, 176, 176);
    pub const DIM: Color32 = Color32::from_rgb(128, 128, 128);
    pub const ICON: Color32 = Color32::from_rgb(150, 150, 150);

    // ---- Controls --------------------------------------------------------
    pub const TRACK: Color32 = Color32::from_rgb(74, 74, 74);
    /// How far the value has been pushed from neutral.
    pub const TRACK_FILL: Color32 = Color32::from_rgb(122, 122, 122);
    pub const HANDLE: Color32 = Color32::from_rgb(190, 190, 190);
    pub const HANDLE_HOT: Color32 = Color32::from_rgb(240, 240, 240);
    /// Drawn around the handle so it stays legible on a coloured track.
    pub const HANDLE_EDGE: Color32 = Color32::from_rgb(16, 16, 16);

    /// The grid inside a plot — the curve editor, the scopes, the histogram.
    pub const GRID: Color32 = Color32::from_rgb(44, 44, 44);

    /// Red, green and blue, wherever a channel has to be named by colour: a
    /// curve trace, a parade panel, a mixer band. One set, because three
    /// slightly different reds across three panels reads as three different
    /// meanings to anyone who has not seen the source.
    pub const CHANNEL: [Color32; 3] = [
        Color32::from_rgb(226, 86, 86),
        Color32::from_rgb(92, 206, 110),
        Color32::from_rgb(104, 142, 240),
    ];

    // ---- The accent ------------------------------------------------------
    /// Resolve titles the open effect in this, and spends it nowhere else.
    pub const ACCENT: Color32 = Color32::from_rgb(224, 106, 90);
    /// The accent, dimmed for a fill behind text.
    pub const ACCENT_DIM: Color32 = Color32::from_rgb(70, 26, 24);
    /// Selection. Resolve's is a muted blue, deliberately not the accent —
    /// "this is chosen" and "this is doing something" are different facts.
    pub const SELECT: Color32 = Color32::from_rgb(46, 84, 122);
    pub const WARN: Color32 = Color32::from_rgb(226, 168, 74);
    pub const ERROR: Color32 = Color32::from_rgb(226, 96, 82);
}

/// What a slider's track is filled with.
///
/// A grey track says nothing about what it does; a track that runs blue to
/// yellow says *temperature* before the label is read. The rule for adding
/// one: the gradient must show the parameter's own axis. Exposure gets no
/// ramp, because a black-to-white track under a control that moves the whole
/// picture would be decoration pretending to be information.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Ramp {
    #[default]
    Plain,
    /// Cool to warm, through neutral.
    Temp,
    /// Green to magenta, through neutral — the other white-balance axis.
    Tint,
    /// The whole hue circle.
    Hue,
    /// A window of the hue circle, centred on one band's own colour, so the
    /// mixer's Red row shows what red actually shifts towards.
    HueAround(f32),
    /// Grey to that band's colour.
    Sat(Color32),
    /// Grey to increasingly colourful, across the spectrum. What a master
    /// saturation control does, drawn.
    Chroma,
    /// Black to white.
    Luma,
    /// One channel's own axis, through neutral: cyan to red, magenta to
    /// green, yellow to blue. What a wheel's Red slider actually does is
    /// take red *out* on the way down, and taking red out is adding cyan.
    Axis(Color32, Color32),
}

impl Ramp {
    /// The colour at `t` along the track.
    pub fn at(self, t: f32) -> Color32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Plain => colour::TRACK,
            Self::Temp => mix3(
                Color32::from_rgb(48, 108, 214),
                Color32::from_rgb(126, 126, 126),
                Color32::from_rgb(232, 196, 88),
                t,
            ),
            Self::Tint => mix3(
                Color32::from_rgb(62, 190, 104),
                Color32::from_rgb(126, 126, 126),
                Color32::from_rgb(204, 84, 196),
                t,
            ),
            Self::Hue => hsv(t, 0.80, 0.86),
            // A little over a fifth of the circle either side: wide enough to
            // show which way the neighbours lie, narrow enough that the ends
            // are not some other colour entirely.
            Self::HueAround(deg) => hsv((deg / 360.0) + (t - 0.5) * 0.22, 0.80, 0.86),
            Self::Sat(vivid) => lerp(Color32::from_rgb(104, 104, 104), vivid, t),
            // Grey at the left, vivid at the right, at a roughly constant
            // lightness — a saturation control does not change how bright the
            // picture is, and a ramp that got brighter as it got more colourful
            // would say it does.
            Self::Chroma => hsv(t, t * 0.9, 0.46 + t * 0.4),
            Self::Luma => lerp(
                Color32::from_rgb(14, 14, 14),
                Color32::from_rgb(236, 236, 236),
                t,
            ),
            Self::Axis(neg, pos) => mix3(neg, Color32::from_rgb(126, 126, 126), pos, t),
        }
    }

    pub fn is_plain(self) -> bool {
        self == Self::Plain
    }
}

/// HSV to a display colour.
///
/// Done here rather than through `egui::ecolor::Hsva`, whose components are
/// *linear* — `Hsva::new(0.0, 0.85, 0.92)` converts to a display red of 246,
/// not 235, and every ramp built that way came out looking bleached beside
/// the hand-picked colours next to it. These numbers are the ones that end up
/// on screen, which is what a palette wants.
fn hsv(h: f32, s: f32, v: f32) -> Color32 {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let byte = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(byte(r), byte(g), byte(b))
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| {
        (x as f32 + (y as f32 - x as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

fn mix3(a: Color32, mid: Color32, b: Color32, t: f32) -> Color32 {
    if t < 0.5 {
        lerp(a, mid, t * 2.0)
    } else {
        lerp(mid, b, (t - 0.5) * 2.0)
    }
}

/// The three channel axes, in the order a wheel's sliders are drawn.
pub const CHANNEL_AXES: [Ramp; 3] = [
    Ramp::Axis(
        Color32::from_rgb(72, 200, 208),
        Color32::from_rgb(226, 78, 72),
    ),
    Ramp::Axis(
        Color32::from_rgb(210, 76, 190),
        Color32::from_rgb(94, 202, 96),
    ),
    Ramp::Axis(
        Color32::from_rgb(226, 206, 84),
        Color32::from_rgb(86, 122, 226),
    ),
];

/// The colour a mixer band is named after.
fn band_hue(name: &str) -> Option<f32> {
    Some(match name {
        "red" => 0.0,
        "orange" => 28.0,
        "yellow" => 52.0,
        "green" => 110.0,
        "aqua" | "cyan" => 182.0,
        "blue" => 222.0,
        "purple" => 272.0,
        "magenta" => 312.0,
        _ => return None,
    })
}

/// Which ramp a parameter gets, decided from its key.
///
/// Keyed off the parameter rather than listed per panel because the same
/// parameter appears in several: Temp. Shift inside Film Damage is the same
/// axis as Temperature in Basic, and a user who has learnt that blue-to-yellow
/// means white balance should not have to learn it twice.
///
/// The matches are on whole words, not substrings. `contains("tint")` looked
/// fine until a `tilt` or a `saturation` inside some unrelated effect picked
/// up a gradient that made a promise the control does not keep.
pub fn ramp_for(effect: &str, key: &str) -> Ramp {
    // The colour mixer's three rows per band.
    if let Some((band, axis)) = key.split_once('_')
        && let Some(deg) = band_hue(band)
    {
        return match axis {
            "hue" => Ramp::HueAround(deg),
            "saturation" | "sat" => Ramp::Sat(hsv(deg / 360.0, 0.85, 0.92)),
            "luminance" | "lum" => Ramp::Luma,
            _ => Ramp::Plain,
        };
    }
    match (effect, key) {
        (_, "temperature" | "temp" | "temp_shift") => Ramp::Temp,
        (_, "tint" | "tint_shift") => Ramp::Tint,
        (_, "hue" | "hue_rotate") => Ramp::Hue,
        (_, "saturation" | "vibrance" | "sat") => Ramp::Chroma,
        _ => Ramp::Plain,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A ramp has to actually ramp, or it is a plain track drawn the
    /// expensive way.
    ///
    /// Counted across the track rather than by comparing the two ends,
    /// because one ramp legitimately has the same colour at both: the hue
    /// circle closes, and a Hue track that did *not* come back to red would
    /// be the broken one.
    #[test]
    fn every_ramp_actually_ramps() {
        for ramp in [
            Ramp::Temp,
            Ramp::Tint,
            Ramp::Hue,
            Ramp::HueAround(0.0),
            Ramp::Sat(Color32::RED),
            Ramp::Chroma,
            Ramp::Luma,
            Ramp::Axis(Color32::BLUE, Color32::RED),
        ] {
            let seen: std::collections::BTreeSet<[u8; 3]> = (0..=8)
                .map(|i| ramp.at(i as f32 / 8.0))
                .map(|c| [c.r(), c.g(), c.b()])
                .collect();
            assert!(
                seen.len() >= 4,
                "{ramp:?} only reaches {} colours across its track",
                seen.len()
            );
        }
    }

    /// And the hue circle in particular has to close.
    #[test]
    fn the_hue_ramp_comes_back_round() {
        assert_eq!(Ramp::Hue.at(0.0), Ramp::Hue.at(1.0));
        assert_ne!(Ramp::Hue.at(0.0), Ramp::Hue.at(0.5));
    }

    /// Both white-balance axes have to pass through neutral in the middle.
    /// A temperature slider whose centre is tinted would put the *look* of a
    /// cast on a value that has none.
    #[test]
    fn the_white_balance_ramps_are_neutral_in_the_middle() {
        for ramp in [Ramp::Temp, Ramp::Tint] {
            let mid = ramp.at(0.5);
            assert!(
                mid.r().abs_diff(mid.g()) < 6 && mid.g().abs_diff(mid.b()) < 6,
                "the centre of the ramp is tinted: {mid:?}"
            );
        }
    }

    #[test]
    fn the_mixer_bands_each_get_their_own_colour() {
        assert!(matches!(
            ramp_for("colour_mixer", "red_hue"),
            Ramp::HueAround(h) if h == 0.0
        ));
        assert!(matches!(
            ramp_for("colour_mixer", "blue_hue"),
            Ramp::HueAround(h) if h == 222.0
        ));
        assert!(matches!(
            ramp_for("colour_mixer", "green_saturation"),
            Ramp::Sat(_)
        ));
        assert!(matches!(
            ramp_for("colour_mixer", "aqua_luminance"),
            Ramp::Luma
        ));
    }

    /// The same axis, wherever it turns up.
    #[test]
    fn temperature_is_the_same_ramp_in_every_effect() {
        assert_eq!(
            ramp_for("white_balance", "temperature"),
            ramp_for("film_damage", "temp_shift")
        );
        assert_eq!(
            ramp_for("white_balance", "tint"),
            ramp_for("film_damage", "tint_shift")
        );
    }

    /// The reason the matches are on whole words. Every one of these read as
    /// a white-balance or hue control under a `contains` test, and each would
    /// have put a gradient on a slider that does something else entirely.
    #[test]
    fn parameters_that_merely_look_like_colour_controls_stay_plain() {
        for (effect, key) in [
            ("film_damage", "tilt_amount"),
            ("film_damage", "tilt_angle"),
            ("tone", "highlights"),
            ("grain", "shadow_gain"),
            ("contrast", "contrast"),
            ("presence", "texture"),
            ("dehaze", "display_depth"),
        ] {
            assert!(
                ramp_for(effect, key).is_plain(),
                "{effect}.{key} was given a gradient it has no business with"
            );
        }
    }
}
