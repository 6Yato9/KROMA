//! The tone curve editor.
//!
//! The evaluator was written and tested at M0 — monotone cubic, so it cannot
//! overshoot. None of that is here. This file is the *interaction*, which is
//! the part no toolkit provides: hit-testing control points, dragging them
//! without letting them cross, adding on click, removing on right-click.
//!
//! The curve operates on log-encoded signal, which is why a straight line from
//! corner to corner is the identity rather than a gamma ramp. The histogram
//! drawn behind it is binned in that same domain — a display-referred one
//! would put every tone in the wrong place, which is worse than drawing
//! nothing.
//!
//! That domain is wider than a photograph. An SDR frame occupies roughly 0.073
//! to 0.555 of the ACEScct range, so the trace sits in the left half of the
//! plot and the rest is headroom above diffuse white. The shaded bands say so
//! rather than leaving it looking like a bug — that headroom is real, it is
//! where a recovered highlight lives, and a curve editor that hid it would be
//! hiding the part of the signal a colourist most wants to reach.

use crate::basic;
use crate::preview::Scopes;
use crate::resolve;
use pe_core::{Curve, History, ParamValue, RowId};
use pe_scopes::{BINS, Histogram};

/// Where diffuse white and black sit in the curve's own domain. The same
/// numbers as the shader's CCT_WHITE and CCT_BLACK.
const LOG_BLACK: f32 = 0.072_905_53;
const LOG_WHITE: f32 = 0.554_794_5;

/// The seven curves Resolve offers, in the order its icon strip shows them.
///
/// The first maps a level onto a level; the other six answer "what should
/// happen to this hue" — or to this luminance, or this saturation. That is the
/// difference that decides everything else about them: their identity is a
/// flat line rather than a diagonal, their background is a spectrum rather
/// than a grid, and the histogram behind them counts hues rather than tones.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Custom,
    HueVsHue,
    HueVsSat,
    HueVsLum,
    LumVsSat,
    SatVsSat,
    SatVsLum,
}

impl Mode {
    const ALL: [Mode; 7] = [
        Mode::Custom,
        Mode::HueVsHue,
        Mode::HueVsSat,
        Mode::HueVsLum,
        Mode::LumVsSat,
        Mode::SatVsSat,
        Mode::SatVsLum,
    ];

    fn title(self) -> &'static str {
        match self {
            Mode::Custom => "Curves - Custom",
            Mode::HueVsHue => "Curves - Hue Vs Hue",
            Mode::HueVsSat => "Curves - Hue Vs Sat",
            Mode::HueVsLum => "Curves - Hue Vs Lum",
            Mode::LumVsSat => "Curves - Lum Vs Sat",
            Mode::SatVsSat => "Curves - Sat Vs Sat",
            Mode::SatVsLum => "Curves - Sat Vs Lum",
        }
    }

    /// The parameter it edits. `None` for Custom, which has four.
    fn key(self) -> Option<&'static str> {
        match self {
            Mode::Custom => None,
            Mode::HueVsHue => Some("hue_vs_hue"),
            Mode::HueVsSat => Some("hue_vs_sat"),
            Mode::HueVsLum => Some("hue_vs_lum"),
            Mode::LumVsSat => Some("lum_vs_sat"),
            Mode::SatVsSat => Some("sat_vs_sat"),
            Mode::SatVsLum => Some("sat_vs_lum"),
        }
    }

    /// What the two readouts under the plot are called, and how a point's
    /// stored 0..1 becomes the number Resolve shows.
    fn readouts(self) -> Readouts {
        match self {
            Mode::Custom => ("Input", "Output", |v| v, |v| v),
            Mode::HueVsHue => (
                "Input Hue",
                "Hue Rotate",
                |v| v * 360.0,
                |v| (v - 0.5) * 180.0,
            ),
            Mode::HueVsSat => ("Input Hue", "Saturation", |v| v * 360.0, |v| v * 2.0),
            Mode::HueVsLum => ("Input Hue", "Lum Gain", |v| v * 360.0, |v| v * 2.0),
            Mode::LumVsSat => ("Input Lum", "Saturation", |v| v, |v| v * 2.0),
            Mode::SatVsSat => ("Input Sat", "Output Sat", |v| v, |v| v * 2.0),
            Mode::SatVsLum => ("Input Sat", "Lum", |v| v, |v| v * 2.0),
        }
    }

    fn x_is_hue(self) -> bool {
        matches!(self, Mode::HueVsHue | Mode::HueVsSat | Mode::HueVsLum)
    }

    fn x_is_saturation(self) -> bool {
        matches!(self, Mode::SatVsSat | Mode::SatVsLum)
    }
}

/// What the two numbers under a plot are called, and how a stored 0..1
/// becomes the number shown.
///
/// The pair travels together because they are one decision: Hue Rotate reads
/// in degrees either side of zero and Saturation reads as a multiplier, and a
/// label without its conversion is a number in the wrong units.
type Readouts = (&'static str, &'static str, fn(f32) -> f32, fn(f32) -> f32);

/// The icon strip. Drawn rather than typed, like every other icon here.
///
/// Each is the same glyph Resolve uses: a rectangle for Custom, and a ring of
/// segments for the secondaries with the ones it acts on filled in. They are
/// small and abstract, which is why the tooltip carries the name.
fn mode_strip(ui: &mut egui::Ui, current: &mut Mode) {
    ui.horizontal(|ui| {
        for mode in Mode::ALL {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(26.0, 22.0), egui::Sense::click());
            if response.clicked() {
                *current = mode;
            }
            if ui.is_rect_visible(rect) {
                let active = *current == mode;
                let painter = ui.painter();
                if active {
                    painter.rect_filled(rect, 3.0, egui::Color32::from_gray(56));
                }
                let tint = if active {
                    egui::Color32::from_gray(235)
                } else if response.hovered() {
                    egui::Color32::from_gray(190)
                } else {
                    egui::Color32::from_gray(140)
                };
                let c = rect.center();
                if mode == Mode::Custom {
                    // A rectangle with a diagonal through it.
                    let box_rect = egui::Rect::from_center_size(c, egui::vec2(13.0, 10.0));
                    painter.rect_stroke(
                        box_rect,
                        1.0,
                        egui::Stroke::new(1.2_f32, tint),
                        egui::StrokeKind::Inside,
                    );
                    painter.line_segment(
                        [box_rect.left_bottom(), box_rect.right_top()],
                        egui::Stroke::new(1.2_f32, tint),
                    );
                } else {
                    // A ring of six segments. Which ones are filled says which
                    // pair the curve relates, so the six glyphs differ from
                    // each other the way the curves do.
                    let index = Mode::ALL.iter().position(|m| *m == mode).unwrap_or(1);
                    for i in 0..6 {
                        let a =
                            i as f32 / 6.0 * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                        let at = c + egui::vec2(a.cos(), a.sin()) * 5.5;
                        let on = (i + index) % 3 == 0;
                        painter.circle_filled(
                            at,
                            if on { 2.2 } else { 1.4 },
                            if on { tint } else { tint.gamma_multiply(0.45) },
                        );
                    }
                }
            }
            response.on_hover_text(mode.title().trim_start_matches("Curves - "));
        }
    });
}

/// The spectrum or ramp behind a secondary curve.
///
/// Resolve paints the plot with what the axis *is*: a hue curve gets a
/// rainbow, a luminance curve gets a black-to-white ramp. It is not
/// decoration — it is the only thing that says where a peak in the histogram
/// sits without counting grid lines.
fn axis_background(painter: &egui::Painter, rect: egui::Rect, mode: Mode) {
    const STEPS: usize = 96;
    let mut mesh = egui::Mesh::default();
    for i in 0..STEPS {
        let t0 = i as f32 / STEPS as f32;
        let t1 = (i + 1) as f32 / STEPS as f32;
        let x0 = rect.min.x + t0 * rect.width();
        let x1 = rect.min.x + t1 * rect.width();
        for (t, x) in [(t0, x0), (t1, x1)] {
            let colour = if mode.x_is_hue() {
                // Dark, because the curve and the histogram are drawn on top
                // of it and a full-strength rainbow would drown both.
                hue_swatch(t).gamma_multiply(0.42)
            } else if mode.x_is_saturation() {
                // Saturation runs grey to grey; the ramp says how far along
                // the axis you are, not what colour it is.
                egui::Color32::from_gray((22.0 + t * 150.0) as u8)
            } else {
                egui::Color32::from_gray((14.0 + t * 158.0) as u8)
            };
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(egui::pos2(x, rect.min.y), colour);
            mesh.colored_vertex(egui::pos2(x, rect.max.y), colour);
            let _ = base;
        }
        let n = mesh.vertices.len() as u32;
        mesh.add_triangle(n - 4, n - 3, n - 1);
        mesh.add_triangle(n - 4, n - 1, n - 2);
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn hue_swatch(hue: f32) -> egui::Color32 {
    let h = hue * 6.0;
    let f = h - h.floor();
    let (r, g, b) = match h.floor() as i32 % 6 {
        0 => (1.0, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    };
    egui::Color32::from_rgb(
        (r * 235.0 + 20.0) as u8,
        (g * 235.0 + 20.0) as u8,
        (b * 235.0 + 20.0) as u8,
    )
}

/// The histogram behind a secondary: hues or saturations rather than tones.
fn spread_behind(
    painter: &egui::Painter,
    rect: egui::Rect,
    mode: Mode,
    spread: Option<&pe_scopes::ColourSpread>,
) {
    let Some(spread) = spread else {
        return;
    };
    let bins = if mode.x_is_hue() {
        &spread.hue
    } else {
        &spread.saturation
    };
    let peak = spread.peak().max(1) as f32;
    let heights = trace(bins, peak);
    let height = rect.height() * 0.92;
    let x_of = |i: usize| rect.min.x + rect.width() * (i as f32 / (BINS - 1) as f32);
    let y_of = |v: f32| rect.max.y - v * height;

    let mut mesh = egui::Mesh::default();
    let fill = egui::Color32::from_rgba_unmultiplied(230, 230, 230, 48);
    for i in 0..BINS - 1 {
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(egui::pos2(x_of(i), y_of(heights[i])), fill);
        mesh.colored_vertex(egui::pos2(x_of(i + 1), y_of(heights[i + 1])), fill);
        mesh.colored_vertex(egui::pos2(x_of(i + 1), rect.max.y), fill);
        mesh.colored_vertex(egui::pos2(x_of(i), rect.max.y), fill);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.add(egui::Shape::line(
        (0..BINS)
            .map(|i| egui::pos2(x_of(i), y_of(heights[i])))
            .collect(),
        egui::Stroke::new(1.2_f32, egui::Color32::from_white_alpha(210)),
    ));
}

/// The six hue buttons under a hue curve, which drop a point at that hue.
///
/// Resolve puts them there because the useful thing to do with a hue curve is
/// almost always "grab the reds", and hunting for red along a rainbow is a
/// worse way to do that than pressing a red dot.
fn hue_presets(ui: &mut egui::Ui, history: &mut History, id: RowId, mode: Mode) {
    let Some(key) = mode.key() else {
        return;
    };
    if !mode.x_is_hue() {
        return;
    }
    ui.horizontal(|ui| {
        for (name, hue) in [
            ("Red", 0.0f32),
            ("Yellow", 1.0 / 6.0),
            ("Green", 2.0 / 6.0),
            ("Cyan", 3.0 / 6.0),
            ("Blue", 4.0 / 6.0),
            ("Magenta", 5.0 / 6.0),
        ] {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(20.0, 18.0), egui::Sense::click());
            if ui.is_rect_visible(rect) {
                ui.painter()
                    .circle_filled(rect.center(), 6.0, hue_swatch(hue));
            }
            if response.on_hover_text(name).clicked() {
                add_point(history, id, key, hue);
            }
        }
    });
}

/// Drop a control point on a curve at `x`, leaving the shape it already has.
fn add_point(history: &mut History, id: RowId, key: &'static str, x: f32) {
    let mut curve = history
        .document()
        .stack
        .get(id)
        .and_then(|r| r.params.get(key))
        .and_then(ParamValue::as_curve)
        .cloned()
        .unwrap_or_else(Curve::flat);
    if curve.points.iter().any(|p| (p[0] - x).abs() < MIN_SPACING) {
        return;
    }
    let y = curve.sample(x);
    let at = curve
        .points
        .iter()
        .position(|q| q[0] > x)
        .unwrap_or(curve.points.len());
    curve.points.insert(at, [x, y]);
    set(history, id, key, curve, None);
}

/// The four curves the effect carries, in the order the tabs show them.
const CHANNELS: [(&str, &str); 4] = [
    ("luma", "Luma"),
    ("red", "Red"),
    ("green", "Green"),
    ("blue", "Blue"),
];

/// How close, in points, the pointer has to be to grab a control point.
const GRAB_RADIUS: f32 = 9.0;

/// Points nearer than this in x are treated as the same point, which is what
/// stops a drag from stacking two on top of each other and making the curve
/// undraggable afterwards.
const MIN_SPACING: f32 = 0.012;

/// The four region sliders, top to bottom, and the parameter each drives.
///
/// Highlights first, so the tab reads like the curve it draws: the bright end
/// of the picture at the top.
const REGIONS: [(&str, &str); 4] = [
    ("param_highlights", "Highlights"),
    ("param_lights", "Lights"),
    ("param_darks", "Darks"),
    ("param_shadows", "Shadows"),
];

/// The three boundaries, in the order they appear left to right.
const SPLITS: [&str; 3] = ["split_low", "split_mid", "split_high"];

pub fn editor(ui: &mut egui::Ui, history: &mut History, scopes: Option<&Scopes>) {
    let Some(id) = history.document().stack.find_by_effect("curves") else {
        return;
    };

    let mode_id = ui.make_persistent_id("curve_mode");
    let mut mode: Mode = ui.data_mut(|d| *d.get_temp_mut_or(mode_id, Mode::Custom));
    let parametric_id = ui.make_persistent_id("tone_curve_parametric");
    let mut parametric_mode: bool = ui.data_mut(|d| *d.get_temp_mut_or(parametric_id, false));

    ui.horizontal(|ui| {
        mode_strip(ui, &mut mode);
        ui.add_space(6.0);
        if mode == Mode::Custom
            && ui
                .selectable_label(parametric_mode, "Parametric")
                .on_hover_text("Four regions and three movable boundaries")
                .clicked()
        {
            parametric_mode = !parametric_mode;
        }
    });
    ui.data_mut(|d| {
        d.insert_temp(mode_id, mode);
        d.insert_temp(parametric_id, parametric_mode);
    });
    ui.label(
        egui::RichText::new(mode.title())
            .small()
            .color(resolve::colour::TITLE),
    );
    ui.add_space(4.0);

    if mode != Mode::Custom {
        secondary(ui, history, id, mode, scopes);
        return;
    }
    if parametric_mode {
        parametric(ui, history, id);
        return;
    }

    let tab_id = ui.make_persistent_id("tone_curve_channel");
    let mut channel: usize = ui.data_mut(|d| *d.get_temp_mut_or(tab_id, 0usize));

    // The plot takes the whole width and the controls sit under it.
    //
    // Resolve puts them side by side because its colour page is a whole
    // screen wide. In a docked inspector the same arrangement leaves the plot
    // too narrow to place a control point in, and the plot is the part you
    // cannot do without.
    let plot_w = ui.available_width().clamp(140.0, 460.0);
    canvas(ui, history, id, CHANNELS[channel].0, scopes, plot_w);
    ui.add_space(6.0);
    edit_column(ui, history, id, &mut channel);
    ui.data_mut(|d| d.insert_temp(tab_id, channel));
}

/// The colour of each channel's button and trace.
fn channel_colour(i: usize) -> egui::Color32 {
    match i {
        1 => egui::Color32::from_rgb(226, 68, 68),
        2 => egui::Color32::from_rgb(64, 200, 84),
        3 => egui::Color32::from_rgb(74, 118, 236),
        _ => egui::Color32::from_gray(210),
    }
}

/// A small square channel button, like Resolve's Y R G B row.
fn channel_button(ui: &mut egui::Ui, i: usize, label: &str, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 18.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let tint = channel_colour(i);
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            2.0,
            if selected {
                tint
            } else {
                tint.gamma_multiply(0.30)
            },
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(11.0),
            if selected {
                egui::Color32::from_gray(16)
            } else {
                egui::Color32::from_gray(210)
            },
        );
    }
    response.clicked()
}

/// Resolve's Edit and Soft Clip column.
fn edit_column(ui: &mut egui::Ui, history: &mut History, id: RowId, channel: &mut usize) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Edit")
                .small()
                .color(resolve::colour::TITLE),
        );
        ui.add_space(6.0);
        for (i, label) in ["Y", "R", "G", "B"].iter().enumerate() {
            if channel_button(ui, i, label, *channel == i) {
                *channel = i;
            }
        }
    });
    ui.add_space(2.0);

    // How much of each drawn curve to apply.
    const INTENSITY: [(&str, usize); 4] = [
        ("luma_intensity", 0),
        ("red_intensity", 1),
        ("green_intensity", 2),
        ("blue_intensity", 3),
    ];
    for (key, i) in INTENSITY {
        let mut v = float_param(history, id, key, 100.0);
        // Through the same row as every other parameter in the application, so
        // the tracks line up with the panel above and below rather than
        // starting wherever this label happened to end.
        let edit = resolve::slider_row(
            ui,
            ui.id().with(key),
            ["Y", "R", "G", "B"][i],
            &mut v,
            0.0..=100.0,
            0,
        );
        apply(history, id, key, v, edit, 100.0);
    }

    ui.add_space(8.0);
    ui.label(
        egui::RichText::new("Soft Clip")
            .small()
            .color(resolve::colour::TITLE),
    );
    // Linked across the three channels rather than one set each. Resolve
    // offers per-channel soft clip; three sets of four would be twelve
    // controls to solve a problem — a channel clipping before the others —
    // that the colour mixer already handles.
    for (key, label) in [
        ("soft_clip_low", "Low"),
        ("soft_clip_low_soft", "Low Soft"),
        ("soft_clip_high", "High"),
        ("soft_clip_high_soft", "High Soft"),
    ] {
        let mut v = float_param(history, id, key, 0.0);
        let edit = resolve::slider_row(ui, ui.id().with(key), label, &mut v, 0.0..=1.0, 3);
        apply(history, id, key, v, edit, 0.0);
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let key = CHANNELS[*channel].0;
        if ui.small_button("Reset").clicked() {
            set(history, id, key, Curve::default(), None);
        }
        if ui.small_button("S-curve").clicked() {
            set(
                history,
                id,
                key,
                Curve {
                    points: vec![[0.0, 0.0], [0.25, 0.18], [0.75, 0.82], [1.0, 1.0]],
                },
                None,
            );
        }
    });
}

fn float_param(history: &History, id: RowId, key: &str, fallback: f32) -> f32 {
    history
        .document()
        .stack
        .get(id)
        .and_then(|r| r.params.get(key))
        .and_then(ParamValue::as_float)
        .unwrap_or(fallback)
}

fn apply(
    history: &mut History,
    id: RowId,
    key: &'static str,
    value: f32,
    edit: resolve::Edit,
    _neutral: f32,
) {
    if edit.changed {
        history.edit(key, Some(format!("curve.{key}")), move |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.params.set(key, ParamValue::Float(value));
            }
        });
    }
    if edit.released {
        history.break_coalescing();
    }
}

fn set(
    history: &mut History,
    id: RowId,
    key: &'static str,
    curve: Curve,
    coalesce: Option<String>,
) {
    history.edit("Tone Curve", coalesce, |doc| {
        if let Some(row) = doc.stack.get_mut(id) {
            row.params.set(key, ParamValue::Curve(curve));
        }
    });
}

fn canvas(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    key: &'static str,
    scopes: Option<&Scopes>,
    width: f32,
) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, width.min(300.0)),
        egui::Sense::click_and_drag(),
    );

    let mut curve = history
        .document()
        .stack
        .get(id)
        .and_then(|r| r.params.get(key))
        .and_then(ParamValue::as_curve)
        .cloned()
        .unwrap_or_default();
    curve.points = curve.sorted();

    // Curve space is x right, y *up*; screen space is y down.
    let to_screen = |p: [f32; 2]| -> egui::Pos2 {
        egui::pos2(
            rect.min.x + p[0].clamp(0.0, 1.0) * rect.width(),
            rect.max.y - p[1].clamp(0.0, 1.0) * rect.height(),
        )
    };
    let to_curve = |p: egui::Pos2| -> [f32; 2] {
        [
            ((p.x - rect.min.x) / rect.width().max(1e-4)).clamp(0.0, 1.0),
            ((rect.max.y - p.y) / rect.height().max(1e-4)).clamp(0.0, 1.0),
        ]
    };

    // The two levels the curve arrives at: black out at the left end, white
    // out at the right. Resolve shows them as an arrow on the left edge with a
    // line across, which is the right way round for a *level* — you are saying
    // how bright white should be, not where along the range it sits.
    let black_out = curve.points.first().map_or(0.0, |p| p[1]);
    let white_out = curve.points.last().map_or(1.0, |p| p[1]);

    let drag_id = ui.make_persistent_id(("curve_drag", key));
    let mut dragging: Option<usize> = ui.data_mut(|d| d.get_temp(drag_id).unwrap_or(None));
    let end_id = ui.make_persistent_id(("curve_end_drag", key));
    // 0 is the black end, 1 the white one.
    let mut end: Option<usize> = ui.data_mut(|d| d.get_temp(end_id).unwrap_or(None));

    // --- interaction ---------------------------------------------------------
    // The end handles are checked first and only near the left edge. Anywhere
    // else would fight with adding a control point, and the point is what the
    // plot is mostly for.
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
        && pos.x < rect.min.x + HANDLE_GRAB
    {
        for (i, t) in [(0usize, black_out), (1, white_out)] {
            if (pos.y - (rect.max.y - t * rect.height())).abs() <= GRAB_RADIUS {
                end = Some(i);
            }
        }
        ui.data_mut(|d| d.insert_temp(end_id, end));
    }
    if response.dragged()
        && let Some(i) = end
        && let Some(pos) = response.interact_pointer_pos()
    {
        let v = ((rect.max.y - pos.y) / rect.height().max(1e-4)).clamp(0.0, 1.0);
        let (black, white) = mirrored_ends(v, i);
        let last = curve.points.len() - 1;
        curve.points[0][1] = black;
        curve.points[last][1] = white;
        set(
            history,
            id,
            key,
            curve.clone(),
            Some(format!("curve.{key}.end{i}")),
        );
    }
    if response.drag_stopped() {
        ui.data_mut(|d| d.insert_temp(end_id, None::<usize>));
        history.break_coalescing();
    }
    // A drag that has hold of a limit is not also placing a control point,
    // which is what `knee.is_none()` guards below.
    if response.drag_started()
        && end.is_none()
        && let Some(pos) = response.interact_pointer_pos()
    {
        dragging = nearest(&curve.points, pos, &to_screen);
        ui.data_mut(|d| d.insert_temp(drag_id, dragging));
    }

    if response.dragged()
        && end.is_none()
        && let (Some(index), Some(pos)) = (dragging, response.interact_pointer_pos())
    {
        let mut p = to_curve(pos);
        let last = curve.points.len() - 1;
        // The endpoints define the ends of the range, so they slide only in y.
        // Letting them move in x would leave a flat dead zone at one end that
        // is hard to undo and easy to create by accident.
        if index == 0 {
            p[0] = 0.0;
        } else if index == last {
            p[0] = 1.0;
        } else {
            let lo = curve.points[index - 1][0] + MIN_SPACING;
            let hi = curve.points[index + 1][0] - MIN_SPACING;
            p[0] = p[0].clamp(lo.min(hi), hi.max(lo));
        }
        curve.points[index] = p;
        set(
            history,
            id,
            key,
            curve.clone(),
            Some(format!("curve.{key}.{index}")),
        );
    }

    if response.drag_stopped() {
        ui.data_mut(|d| d.insert_temp(drag_id, None::<usize>));
        history.break_coalescing();
    }

    if response.clicked()
        && end.is_none()
        && pos_is_clear(&response, rect)
        && let Some(pos) = response.interact_pointer_pos()
        && nearest(&curve.points, pos, &to_screen).is_none()
    {
        let p = to_curve(pos);
        // Insert in order so the spline stays well defined without waiting for
        // the next sort.
        let at = curve
            .points
            .iter()
            .position(|q| q[0] > p[0])
            .unwrap_or(curve.points.len());
        let crowded = curve
            .points
            .iter()
            .any(|q| (q[0] - p[0]).abs() < MIN_SPACING);
        if !crowded {
            curve.points.insert(at, p);
            set(history, id, key, curve.clone(), None);
        }
    }

    if response.secondary_clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(index) = nearest(&curve.points, pos, &to_screen)
    {
        // The endpoints are the range; removing one would be meaningless.
        if index != 0 && index != curve.points.len() - 1 {
            curve.points.remove(index);
            set(history, id, key, curve.clone(), None);
        }
    }

    // --- drawing -------------------------------------------------------------
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, egui::Color32::from_gray(18));

    histogram_behind(&painter, rect, scopes.map(|s| &s.log_histogram));

    end_handles(&painter, rect, black_out, white_out, end);

    // Clipping, at the end it is happening, from the *display* histogram.
    //
    // The trace behind the curve is binned in the curve's domain, which is the
    // right space to read tones in and the wrong one to ask "will this survive
    // the output". Those are two questions and they want two measurements —
    // this is the second one, kept because losing it was the only cost of
    // dropping the panel's own histogram.
    if let Some(display) = scopes.map(|s| &s.histogram) {
        let total = display.total.max(1) as f32;
        let crushed = (display.red[0] + display.green[0] + display.blue[0]) as f32 / (3.0 * total);
        if crushed > 0.002 {
            clip_mark(&painter, egui::pos2(rect.min.x + 8.0, rect.min.y + 8.0));
        }
        if display.over_white_fraction() > 0.001 {
            clip_mark(&painter, egui::pos2(rect.max.x - 8.0, rect.min.y + 8.0));
        }
    }

    let grid = egui::Stroke::new(1.0_f32, egui::Color32::from_gray(42));
    for q in 1..4 {
        let t = q as f32 / 4.0;
        painter.line_segment(
            [
                egui::pos2(rect.min.x + t * rect.width(), rect.min.y),
                egui::pos2(rect.min.x + t * rect.width(), rect.max.y),
            ],
            grid,
        );
        painter.line_segment(
            [
                egui::pos2(rect.min.x, rect.min.y + t * rect.height()),
                egui::pos2(rect.max.x, rect.min.y + t * rect.height()),
            ],
            grid,
        );
    }
    // Mid grey, which is the tone a colourist is nearly always working
    // relative to. Log puts more of the range below 18% than above it, so it
    // does not land in the middle of the plot and there is no reading it off
    // the quarter lines.
    let grey_x = rect.min.x + rect.width() * (LOG_GREY - LOG_BLACK) / (LOG_WHITE - LOG_BLACK);
    painter.line_segment(
        [
            egui::pos2(grey_x, rect.min.y),
            egui::pos2(grey_x, rect.max.y),
        ],
        egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(46)),
    );

    // The identity, so how far the curve has been bent is readable at a glance.
    painter.line_segment(
        [
            egui::pos2(rect.min.x, rect.max.y),
            egui::pos2(rect.max.x, rect.min.y),
        ],
        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(58)),
    );

    let colour = match key {
        "red" => egui::Color32::from_rgb(230, 90, 90),
        "green" => egui::Color32::from_rgb(90, 210, 110),
        "blue" => egui::Color32::from_rgb(100, 140, 245),
        _ => egui::Color32::from_gray(225),
    };

    // Sample the real evaluator rather than drawing straight segments between
    // control points — otherwise the line would not be the curve the shader
    // applies, which is the one thing this widget must not get wrong.
    let steps = 96;
    let line: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let x = i as f32 / steps as f32;
            to_screen([x, curve.sample(x)])
        })
        .collect();
    painter.add(egui::Shape::line(line, egui::Stroke::new(1.8_f32, colour)));

    let hover = response.hover_pos();
    for (i, p) in curve.points.iter().enumerate() {
        let at = to_screen(*p);
        let near = hover.is_some_and(|h| h.distance(at) <= GRAB_RADIUS) || dragging == Some(i);
        let r = if near { 5.5 } else { 4.0 };
        painter.circle_filled(at, r, colour);
        painter.circle_stroke(
            at,
            r,
            egui::Stroke::new(1.2_f32, egui::Color32::from_gray(20)),
        );
    }
}

// ---------------------------------------------------------------------------
// The parametric curve
// ---------------------------------------------------------------------------

/// The parametric curve: four sliders and three movable boundaries.
///
/// It cannot make a shape that isn't smooth, which is exactly why it earns a
/// place next to a point curve that can make any shape at all. The maths lives
/// in `pe_core::parametric` so that this drawing and the shader are the same
/// curve rather than two curves that resemble each other.
fn parametric(ui: &mut egui::Ui, history: &mut History, id: RowId) {
    let amounts = read(history, id, &REGIONS.map(|(k, _)| k));
    let splits = read(history, id, &SPLITS);

    region_canvas(ui, history, id, amounts, splits);

    ui.add_space(2.0);
    for (key, label) in REGIONS {
        basic::slider(ui, history, "curves", key, label);
    }

    ui.horizontal(|ui| {
        if ui.small_button("Reset").clicked() {
            history.edit("Reset Parametric Curve", None, |doc| {
                if let Some(row) = doc.stack.get_mut(id) {
                    for (key, _) in REGIONS {
                        row.params.set(key, ParamValue::Float(0.0));
                    }
                    for (key, v) in SPLITS.iter().zip(pe_core::parametric::DEFAULT_SPLITS) {
                        row.params.set(*key, ParamValue::Float(v));
                    }
                }
            });
        }
        ui.label(
            egui::RichText::new("drag the handles to move a boundary")
                .small()
                .weak(),
        );
    });
}

/// Read float parameters off the row, falling back to what the registry
/// declares.
///
/// The fallback matters here in a way it does not for an ordinary slider: a
/// split's resting value is 0.25 or 0.5 or 0.75, so defaulting a missing one
/// to zero would collapse three boundaries onto the black point.
fn read<const N: usize>(history: &History, id: RowId, keys: &[&str; N]) -> [f32; N] {
    let def = pe_effects::by_key("curves");
    let row = history.document().stack.get(id);
    let mut out = [0.0; N];
    for (i, key) in keys.iter().enumerate() {
        let declared = def
            .and_then(|e| e.param(key))
            .and_then(|p| match p.kind {
                pe_effects::ParamKind::Float { default, .. } => Some(default),
                _ => None,
            })
            .unwrap_or(0.0);
        out[i] = row
            .and_then(|r| r.params.get(key))
            .and_then(ParamValue::as_float)
            .unwrap_or(declared);
    }
    out
}

/// Height of the strip below the plot that the split handles live in.
const HANDLE_STRIP: f32 = 13.0;

fn region_canvas(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    amounts: [f32; 4],
    splits: [f32; 3],
) {
    let side = ui.available_width().min(300.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());
    let plot =
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.max.y - HANDLE_STRIP));

    let x_of = |t: f32| plot.min.x + t.clamp(0.0, 1.0) * plot.width();
    let t_of = |x: f32| ((x - plot.min.x) / plot.width().max(1e-4)).clamp(0.0, 1.0);
    let y_of = |v: f32| plot.max.y - v.clamp(0.0, 1.0) * plot.height();

    // --- interaction ---------------------------------------------------------
    let drag_id = ui.make_persistent_id("parametric_split_drag");
    let mut dragging: Option<usize> = ui.data_mut(|d| d.get_temp(drag_id).unwrap_or(None));

    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        dragging = splits
            .iter()
            .enumerate()
            .map(|(i, s)| (i, (x_of(*s) - pos.x).abs()))
            .filter(|(_, d)| *d <= GRAB_RADIUS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i);
        ui.data_mut(|d| d.insert_temp(drag_id, dragging));
    }

    if response.dragged()
        && let (Some(i), Some(pos)) = (dragging, response.interact_pointer_pos())
    {
        // Keep the handles in order. The shader sorts them anyway, but a
        // handle that swaps places with its neighbour under the pointer is
        // disorienting in a way the correct result does not excuse.
        let lo = if i == 0 {
            0.0
        } else {
            splits[i - 1] + MIN_SPACING
        };
        let hi = if i == 2 {
            1.0
        } else {
            splits[i + 1] - MIN_SPACING
        };
        let v = t_of(pos.x).clamp(lo.min(hi), hi.max(lo));
        let key = SPLITS[i];
        history.edit("Tone Curve Split", Some(format!("curve.{key}")), |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.params.set(key, ParamValue::Float(v));
            }
        });
    }

    if response.drag_stopped() {
        ui.data_mut(|d| d.insert_temp(drag_id, None::<usize>));
        history.break_coalescing();
    }

    // --- drawing -------------------------------------------------------------
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(plot, 3.0, egui::Color32::from_gray(18));

    let grid = egui::Stroke::new(1.0_f32, egui::Color32::from_gray(42));
    for q in 1..4 {
        let t = q as f32 / 4.0;
        painter.line_segment(
            [
                egui::pos2(plot.min.x, plot.min.y + t * plot.height()),
                egui::pos2(plot.max.x, plot.min.y + t * plot.height()),
            ],
            grid,
        );
    }
    painter.line_segment(
        [
            egui::pos2(plot.min.x, plot.max.y),
            egui::pos2(plot.max.x, plot.min.y),
        ],
        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(58)),
    );

    // The boundaries, drawn full height so it is plain which stretch of the
    // curve each slider owns.
    for s in splits {
        painter.line_segment(
            [
                egui::pos2(x_of(s), plot.min.y),
                egui::pos2(x_of(s), plot.max.y),
            ],
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(64)),
        );
    }

    let steps = 96;
    let line: Vec<egui::Pos2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            egui::pos2(
                x_of(t),
                y_of(pe_core::parametric::tone_out(t, amounts, splits)),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        line,
        egui::Stroke::new(1.8_f32, egui::Color32::from_gray(225)),
    ));

    let hover = response.hover_pos();
    for (i, s) in splits.iter().enumerate() {
        let x = x_of(*s);
        let near = hover.is_some_and(|h| (h.x - x).abs() <= GRAB_RADIUS) || dragging == Some(i);
        let y = rect.max.y - HANDLE_STRIP * 0.5;
        let w = if near { 5.0 } else { 4.0 };
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x, y - w),
                egui::pos2(x + w, y + w * 0.6),
                egui::pos2(x - w, y + w * 0.6),
            ],
            egui::Color32::from_gray(if near { 235 } else { 170 }),
            egui::Stroke::NONE,
        ));
    }
}

/// The three channel colours, for the traces.
const CHANNEL_COLOURS: [egui::Color32; 3] = [
    egui::Color32::from_rgb(214, 96, 96),
    egui::Color32::from_rgb(96, 206, 116),
    egui::Color32::from_rgb(110, 148, 236),
];

/// How far either side of a bin the smoothing reaches.
///
/// A histogram of a photograph is spiky — real images have runs of identical
/// values, and every one of them is a bin standing alone. Drawn raw that reads
/// as a bar chart, which is a picture of the sampling rather than of the
/// photograph. Three bins either side is enough to make it a curve and short
/// enough that a genuine spike is still a spike.
const SMOOTH: usize = 3;

/// Smooth and normalise one channel into 0..1 heights.
fn trace(bins: &[u32; BINS], peak: f32) -> Vec<f32> {
    (0..BINS)
        .map(|i| {
            let mut sum = 0.0;
            let mut weight = 0.0;
            for d in -(SMOOTH as i32)..=(SMOOTH as i32) {
                let j = i as i32 + d;
                if !(0..BINS as i32).contains(&j) {
                    continue;
                }
                // Triangular, which is a box filter applied twice and quite
                // smooth enough for something drawn a few hundred pixels wide.
                let w = 1.0 - (d.abs() as f32 / (SMOOTH as f32 + 1.0));
                sum += bins[j as usize] as f32 * w;
                weight += w;
            }
            let v = sum / weight.max(1e-4) / peak;
            // The same compression the panel histogram used: one flat area of
            // sky can hold a fifth of the frame in a single bin, and against
            // that everything else would be a pixel high.
            v.clamp(0.0, 1.0).powf(0.42)
        })
        .collect()
}

/// A secondary curve: the plot, its spectrum, and the readouts under it.
///
/// The editing is the same as a tone curve's — drag a point, click to add,
/// right-click to remove — because it is the same widget looked at through a
/// different axis. What changes is the identity it resets to, what is painted
/// behind it, and what the two numbers underneath are called.
fn secondary(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    mode: Mode,
    scopes: Option<&Scopes>,
) {
    let Some(key) = mode.key() else {
        return;
    };
    let width = ui.available_width().clamp(140.0, 620.0);
    let height = (width * 0.42).clamp(110.0, 240.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());

    let mut curve = history
        .document()
        .stack
        .get(id)
        .and_then(|r| r.params.get(key))
        .and_then(ParamValue::as_curve)
        .cloned()
        .unwrap_or_else(Curve::flat);
    curve.points = curve.sorted();

    let to_screen = |p: [f32; 2]| -> egui::Pos2 {
        egui::pos2(
            rect.min.x + p[0].clamp(0.0, 1.0) * rect.width(),
            rect.max.y - p[1].clamp(0.0, 1.0) * rect.height(),
        )
    };
    let to_curve = |p: egui::Pos2| -> [f32; 2] {
        [
            ((p.x - rect.min.x) / rect.width().max(1e-4)).clamp(0.0, 1.0),
            ((rect.max.y - p.y) / rect.height().max(1e-4)).clamp(0.0, 1.0),
        ]
    };

    let drag_id = ui.make_persistent_id(("secondary_drag", key));
    let mut dragging: Option<usize> = ui.data_mut(|d| d.get_temp(drag_id).unwrap_or(None));

    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        dragging = nearest(&curve.points, pos, &to_screen);
        ui.data_mut(|d| d.insert_temp(drag_id, dragging));
    }
    if response.dragged()
        && let (Some(index), Some(pos)) = (dragging, response.interact_pointer_pos())
    {
        let mut p = to_curve(pos);
        let last = curve.points.len() - 1;
        // The ends anchor the range, so they slide only in y — the same rule
        // the tone curve follows, and for the same reason: a flat dead zone at
        // one end is easy to create by accident and hard to undo.
        if index == 0 {
            p[0] = 0.0;
        } else if index == last {
            p[0] = 1.0;
        } else {
            let lo = curve.points[index - 1][0] + MIN_SPACING;
            let hi = curve.points[index + 1][0] - MIN_SPACING;
            p[0] = p[0].clamp(lo.min(hi), hi.max(lo));
        }
        curve.points[index] = p;
        set(
            history,
            id,
            key,
            curve.clone(),
            Some(format!("curve.{key}.{index}")),
        );
    }
    if response.drag_stopped() {
        ui.data_mut(|d| d.insert_temp(drag_id, None::<usize>));
        history.break_coalescing();
    }
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && nearest(&curve.points, pos, &to_screen).is_none()
    {
        let p = to_curve(pos);
        if !curve
            .points
            .iter()
            .any(|q| (q[0] - p[0]).abs() < MIN_SPACING)
        {
            let at = curve
                .points
                .iter()
                .position(|q| q[0] > p[0])
                .unwrap_or(curve.points.len());
            curve.points.insert(at, p);
            set(history, id, key, curve.clone(), None);
        }
    }
    if response.secondary_clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(index) = nearest(&curve.points, pos, &to_screen)
        && index != 0
        && index != curve.points.len() - 1
    {
        curve.points.remove(index);
        set(history, id, key, curve.clone(), None);
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        axis_background(&painter, rect, mode);
        spread_behind(&painter, rect, mode, scopes.map(|s| &s.colour));

        let grid = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(26));
        for q in 1..8 {
            let x = rect.min.x + rect.width() * (q as f32 / 8.0);
            painter.line_segment([egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)], grid);
        }
        // The neutral line, which is where the curve sits when it is doing
        // nothing. On a secondary that is the middle, not the diagonal.
        painter.line_segment(
            [
                egui::pos2(rect.min.x, rect.center().y),
                egui::pos2(rect.max.x, rect.center().y),
            ],
            egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(60)),
        );

        let steps = 128;
        painter.add(egui::Shape::line(
            (0..=steps)
                .map(|i| {
                    let x = i as f32 / steps as f32;
                    to_screen([x, curve.sample(x)])
                })
                .collect(),
            egui::Stroke::new(1.8_f32, egui::Color32::from_gray(235)),
        ));

        let hover = response.hover_pos();
        for (i, p) in curve.points.iter().enumerate() {
            let at = to_screen(*p);
            let near = hover.is_some_and(|h| h.distance(at) <= GRAB_RADIUS) || dragging == Some(i);
            let r = if near { 5.5 } else { 4.0 };
            painter.circle_filled(at, r, egui::Color32::WHITE);
            painter.circle_stroke(
                at,
                r,
                egui::Stroke::new(1.2_f32, egui::Color32::from_gray(30)),
            );
        }
    }

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        hue_presets(ui, history, id, mode);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Whichever point is in hand, or the last one placed. Resolve
            // shows the same two numbers and they are how you set a value
            // exactly rather than by eye.
            let (x_name, y_name, to_x, to_y) = mode.readouts();
            let point = dragging
                .and_then(|i| curve.points.get(i))
                .or_else(|| curve.points.get(curve.points.len().saturating_sub(1)));
            let (x, y) = point.map_or((0.0, 0.5), |p| (p[0], p[1]));
            ui.label(
                egui::RichText::new(format!("{:.2}", to_y(y)))
                    .small()
                    .monospace(),
            );
            ui.label(egui::RichText::new(y_name).small().weak());
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("{:.2}", to_x(x)))
                    .small()
                    .monospace(),
            );
            ui.label(egui::RichText::new(x_name).small().weak());
        });
    });

    if ui.small_button("Reset").clicked() {
        set(history, id, key, Curve::flat(), None);
    }
}

/// The picture's tones, in the curve's own domain, behind the curve.
///
/// One filled area per channel with a line along its top: the fills are
/// translucent so where the channels agree they build into a pale grey, and
/// the lines say which channel each edge belongs to. That is the reading a
/// colourist wants — the grey mass is the picture, and a coloured edge showing
/// out of it is a channel that has drifted from the others.
fn histogram_behind(painter: &egui::Painter, rect: egui::Rect, hist: Option<&Histogram>) {
    let Some(hist) = hist else {
        return;
    };
    let peak = hist.peak().max(1) as f32;
    let height = rect.height() * 0.92;

    for (bins, colour) in [
        (&hist.red, CHANNEL_COLOURS[0]),
        (&hist.green, CHANNEL_COLOURS[1]),
        (&hist.blue, CHANNEL_COLOURS[2]),
    ] {
        // The plot spans black to diffuse white, not the whole log domain, so
        // the bins are read through that range rather than laid out edge to
        // edge. Drawing them straight across would put every tone about a
        // seventh of the plot to the left of where the curve acts on it.
        let heights = trace(bins, peak);
        let sample = |i: usize| {
            let t = LOG_BLACK + (i as f32 / (BINS - 1) as f32) * (LOG_WHITE - LOG_BLACK);
            heights[((t * (BINS - 1) as f32).round() as usize).min(BINS - 1)]
        };
        let x_of = |i: usize| rect.min.x + rect.width() * (i as f32 / (BINS - 1) as f32);
        let y_of = |v: f32| rect.max.y - v * height;

        // The fill, as a strip of quads from the baseline. A polygon would be
        // the obvious shape, but egui only fills convex ones and a histogram
        // is the least convex outline there is.
        let mut mesh = egui::Mesh::default();
        let fill = egui::Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), 56);
        for i in 0..BINS - 1 {
            let (x0, x1) = (x_of(i), x_of(i + 1));
            let (y0, y1) = (y_of(sample(i)), y_of(sample(i + 1)));
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(egui::pos2(x0, y0), fill);
            mesh.colored_vertex(egui::pos2(x1, y1), fill);
            mesh.colored_vertex(egui::pos2(x1, rect.max.y), fill);
            mesh.colored_vertex(egui::pos2(x0, rect.max.y), fill);
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
        painter.add(egui::Shape::mesh(mesh));

        let line: Vec<egui::Pos2> = (0..BINS)
            .map(|i| egui::pos2(x_of(i), y_of(sample(i))))
            .collect();
        painter.add(egui::Shape::line(
            line,
            egui::Stroke::new(1.2_f32, colour.gamma_multiply(0.85)),
        ));
    }
}

/// Whether a click landed clear of the soft clip handles.
///
/// Without this, clicking a handle to nudge it would drop a control point
/// underneath it, and the point would then be in the way of every later drag.
fn pos_is_clear(response: &egui::Response, rect: egui::Rect) -> bool {
    response
        .interact_pointer_pos()
        .is_none_or(|p| p.x >= rect.min.x + HANDLE_GRAB)
}

/// How close to the left edge a drag has to start to be taking hold of a soft
/// clip limit rather than placing a control point.
const HANDLE_GRAB: f32 = 22.0;

/// 18% grey in the curve's domain, the anchor both limits measure in from.
const LOG_GREY: f32 = 0.413_588_67;

/// Where the two ends go when one of them is dragged to `v`.
///
/// They are one control, mirrored about the middle. Moving the white end down
/// on its own would only darken the picture — the black end has to come up to
/// meet it, which collapses the contrast towards grey and then, once they
/// cross, turns the picture into a negative. That crossing is the whole point
/// of the control and it is why the two are linked rather than independent.
fn mirrored_ends(v: f32, which: usize) -> (f32, f32) {
    let white = if which == 1 { v } else { 1.0 - v };
    (1.0 - white, white)
}

/// The two end handles: an arrow on the left edge and, once moved, a line
/// across the plot at that level.
///
/// Only the white end shows at rest. The black end sits on the bottom frame
/// where there is nothing to see, and drawing an arrow on top of the border
/// would read as part of it. Both lines appear as soon as their end has been
/// moved off where it started, and stay — a level you have set is worth being
/// able to see without holding it.
fn end_handles(
    painter: &egui::Painter,
    rect: egui::Rect,
    black_out: f32,
    white_out: f32,
    dragging: Option<usize>,
) {
    for (i, t, default) in [(0usize, black_out, 0.0f32), (1, white_out, 1.0)] {
        let moved = (t - default).abs() > 1e-4;
        let held = dragging == Some(i);
        // The black end only once it has been moved; the white end always,
        // because it is the one people reach for.
        if i == 0 && !moved && !held {
            continue;
        }
        let y = rect.max.y - t * rect.height();
        if moved || held {
            painter.line_segment(
                [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                egui::Stroke::new(1.4_f32, egui::Color32::from_white_alpha(190)),
            );
        }
        let w = 5.5_f32;
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(rect.min.x, y - w),
                egui::pos2(rect.min.x, y + w),
                egui::pos2(rect.min.x + w * 1.7, y),
            ],
            if held {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_gray(225)
            },
            egui::Stroke::NONE,
        ));
    }
}

fn clip_mark(painter: &egui::Painter, at: egui::Pos2) {
    painter.circle_filled(at, 4.5, egui::Color32::from_black_alpha(180));
    painter.circle_filled(at, 2.8, egui::Color32::from_rgb(240, 200, 90));
}

fn nearest(
    points: &[[f32; 2]],
    pos: egui::Pos2,
    to_screen: &impl Fn([f32; 2]) -> egui::Pos2,
) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .map(|(i, p)| (i, to_screen(*p).distance(pos)))
        .filter(|(_, d)| *d <= GRAB_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two ends are one control. Moving the white end down on its own
    /// only darkens the picture; the black end coming up to meet it is what
    /// collapses the contrast and then inverts it.
    #[test]
    fn the_two_ends_mirror_about_the_middle() {
        // Dragging the white end to the top is the identity.
        assert_eq!(mirrored_ends(1.0, 1), (0.0, 1.0));
        // To the middle, both meet there and the picture goes flat.
        assert_eq!(mirrored_ends(0.5, 1), (0.5, 0.5));
        // Past it, they cross: white out below black out is a negative.
        let (black, white) = mirrored_ends(0.2, 1);
        assert!(
            white < black,
            "the ends did not cross ({white} against {black})"
        );
        // And either handle drives the pair, so grabbing the lower one works
        // the same way from the other side. Compared loosely because one path
        // subtracts twice and the other once.
        let (a_black, a_white) = mirrored_ends(0.8, 0);
        let (b_black, b_white) = mirrored_ends(0.2, 1);
        assert!((a_black - b_black).abs() < 1e-6 && (a_white - b_white).abs() < 1e-6);
    }

    /// The plot spans black to diffuse white, so the histogram behind it has
    /// to be read through that range. Laid out edge to edge instead, every
    /// tone would sit about a seventh of the plot to the left of where the
    /// curve acts on it — close enough to look plausible and wrong everywhere.
    #[test]
    fn the_plot_covers_black_to_diffuse_white() {
        let span = LOG_WHITE - LOG_BLACK;
        assert!(
            (span - 0.4819).abs() < 1e-3,
            "an SDR frame spans {span} of the log domain"
        );
        // The left edge of the plot is black and the right edge is white.
        let at = |u: f32| LOG_BLACK + u * span;
        assert!((at(0.0) - LOG_BLACK).abs() < 1e-6);
        assert!((at(1.0) - LOG_WHITE).abs() < 1e-6);
        // And mid grey lands where a colourist expects it: a little above the
        // middle, because log puts more of the range below 18% than above it.
        let grey = (LOG_GREY - LOG_BLACK) / span;
        assert!(
            (0.6..0.75).contains(&grey),
            "mid grey landed at {grey} of the way across"
        );
    }

    fn spike() -> [u32; BINS] {
        let mut bins = [0u32; BINS];
        bins[100] = 1000;
        bins
    }

    /// A histogram of a photograph is spiky — real images have runs of
    /// identical values, and drawn raw that reads as a bar chart rather than
    /// as a picture of the photograph.
    #[test]
    fn smoothing_spreads_a_spike_into_a_curve() {
        let t = trace(&spike(), 1000.0);
        assert!(t[100] > 0.0, "the spike vanished");
        for d in 1..=SMOOTH {
            assert!(
                t[100 - d] > 0.0 && t[100 + d] > 0.0,
                "the spike did not reach {d} bins out"
            );
            assert!(
                t[100 - d] < t[100 - d + 1],
                "the shoulder should fall away from the peak"
            );
        }
        assert!(
            t[100 - SMOOTH - 1] == 0.0,
            "the smoothing reached further than it should"
        );
    }

    #[test]
    fn a_trace_never_leaves_the_plot() {
        let mut bins = [0u32; BINS];
        // Everything in one bin, which is what a flat frame gives.
        bins[10] = u32::MAX / 2;
        let t = trace(&bins, 1.0);
        assert!(t.iter().all(|v| (0.0..=1.0).contains(v)), "{:?}", t[10]);
    }
}
