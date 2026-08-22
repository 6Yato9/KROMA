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
use crate::resolve;
use pe_core::{Curve, History, ParamValue, RowId};
use pe_scopes::{BINS, Histogram};

/// Where diffuse white and black sit in the curve's own domain. The same
/// numbers as the shader's CCT_WHITE and CCT_BLACK.
const LOG_BLACK: f32 = 0.072_905_53;
const LOG_WHITE: f32 = 0.554_794_5;

/// Width of the column of controls to the right of the plot.
const EDIT_W: f32 = 172.0;

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

pub fn editor(ui: &mut egui::Ui, history: &mut History, scopes: Option<&Histogram>) {
    let Some(id) = history.document().stack.find_by_effect("curves") else {
        return;
    };

    let mode_id = ui.make_persistent_id("tone_curve_mode");
    let mut parametric_mode: bool = ui.data_mut(|d| *d.get_temp_mut_or(mode_id, false));
    ui.horizontal(|ui| {
        if ui.selectable_label(!parametric_mode, "Custom").clicked() {
            parametric_mode = false;
        }
        if ui.selectable_label(parametric_mode, "Parametric").clicked() {
            parametric_mode = true;
        }
        ui.label(
            egui::RichText::new("click to add · right-click to remove")
                .small()
                .weak(),
        );
    });
    ui.data_mut(|d| d.insert_temp(mode_id, parametric_mode));
    ui.add_space(4.0);

    if parametric_mode {
        parametric(ui, history, id);
        return;
    }

    let tab_id = ui.make_persistent_id("tone_curve_channel");
    let mut channel: usize = ui.data_mut(|d| *d.get_temp_mut_or(tab_id, 0usize));

    // Plot on the left, the channel and soft-clip controls on the right, the
    // way Resolve lays the panel out.
    let available = ui.available_width();
    let plot_w = (available - EDIT_W - 8.0).clamp(140.0, 460.0);
    ui.horizontal_top(|ui| {
        canvas(ui, history, id, CHANNELS[channel].0, scopes, plot_w);
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.set_width(EDIT_W);
            edit_column(ui, history, id, &mut channel);
        });
    });
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

/// A compact slider row for the narrow right-hand column.
///
/// Not `resolve::slider_row`: that one reserves a label column wide enough for
/// "Geometry Factor", and here the label is a single letter or two words in a
/// column a third the width.
fn narrow_row(
    ui: &mut egui::Ui,
    id: egui::Id,
    lead: Option<(usize, &str)>,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: usize,
) -> resolve::Edit {
    let width = ui.available_width();
    ui.horizontal(|ui| {
        let mut out = resolve::Edit::default();
        if let Some((i, text)) = lead
            && channel_button(ui, i, text, false)
        {
            // The lead swatch is decoration here; selection lives in the row
            // of buttons above.
        }
        if !label.is_empty() {
            ui.add_sized(
                [56.0, 16.0],
                egui::Label::new(
                    egui::RichText::new(label)
                        .small()
                        .color(resolve::colour::LABEL),
                ),
            );
        }
        let track_w = (width - 56.0 - 44.0 - 26.0).max(30.0);
        let (lo, hi) = (*range.start(), *range.end());
        let r = ui.add_sized(
            [track_w, 16.0],
            egui::Slider::new(value, lo..=hi).show_value(false),
        );
        out.changed = r.changed();
        out.released = r.drag_stopped();
        // Every value in the panel is typeable. A slider is for finding a
        // number and a box is for saying one, and a control that only offers
        // the first cannot be told "exactly 50".
        let before = *value;
        ui.add_sized(
            [42.0, 16.0],
            egui::DragValue::new(value)
                .fixed_decimals(decimals)
                .speed(0.0),
        );
        if (before - *value).abs() > 1e-6 {
            *value = value.clamp(lo, hi);
            out.changed = true;
            out.released = true;
        }
        let _ = id;
        out
    })
    .inner
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
        let edit = narrow_row(
            ui,
            ui.id().with(key),
            Some((i, "")),
            "",
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
        let edit = narrow_row(ui, ui.id().with(key), None, label, &mut v, 0.0..=1.0, 2);
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
    scopes: Option<&Histogram>,
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

    let drag_id = ui.make_persistent_id(("curve_drag", key));
    let mut dragging: Option<usize> = ui.data_mut(|d| d.get_temp(drag_id).unwrap_or(None));

    // --- interaction ---------------------------------------------------------
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

    // Where the photograph actually lives inside the curve's domain. The rest
    // is real headroom, not empty space, so it is shaded rather than hidden.
    for band in [
        egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.min.x + rect.width() * LOG_BLACK, rect.max.y),
        ),
        egui::Rect::from_min_max(
            egui::pos2(rect.min.x + rect.width() * LOG_WHITE, rect.min.y),
            rect.max,
        ),
    ] {
        painter.rect_filled(band, 0.0, egui::Color32::from_black_alpha(70));
    }

    histogram_behind(&painter, rect, scopes);

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

/// The picture's tones, in the curve's own domain, behind the curve.
///
/// Dimmer than the panel histogram and additive the same way: the white core
/// is where the channels agree and a coloured fringe is where one has drifted.
/// It is a reference, not a scope, so it gives way to the curve on top of it.
fn histogram_behind(painter: &egui::Painter, rect: egui::Rect, hist: Option<&Histogram>) {
    let Some(hist) = hist else {
        return;
    };
    let peak = hist.peak().max(1) as f32;
    let scale = |v: u32| (v as f32 / peak).clamp(0.0, 1.0).powf(0.42);

    let mut mesh = egui::Mesh::default();
    let step = rect.width() / BINS as f32;
    for i in 0..BINS {
        let x0 = rect.min.x + i as f32 * step;
        let x1 = (x0 + step + 0.5).min(rect.max.x);
        let mut heights = [
            (scale(hist.red[i]), 0usize),
            (scale(hist.green[i]), 1),
            (scale(hist.blue[i]), 2),
        ];
        heights.sort_by(|a, b| a.0.total_cmp(&b.0));

        let mut base = 0.0f32;
        for k in 0..3 {
            let top = heights[k].0;
            if top > base {
                mesh.add_colored_rect(
                    egui::Rect::from_min_max(
                        egui::pos2(x0, rect.max.y - top * rect.height() * 0.92),
                        egui::pos2(x1, rect.max.y - base * rect.height() * 0.92),
                    ),
                    basic::additive_channels(&heights[k..]).gamma_multiply(0.55),
                );
                base = top;
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));
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
