//! The tone curve editor.
//!
//! The evaluator was written and tested at M0 — monotone cubic, so it cannot
//! overshoot. None of that is here. This file is the *interaction*, which is
//! the part no toolkit provides: hit-testing control points, dragging them
//! without letting them cross, adding on click, removing on right-click.
//!
//! The curve operates on log-encoded signal, which is why a straight line from
//! corner to corner is the identity rather than a gamma ramp. It also means
//! the histogram over the Basic panel — which is display-referred — would not
//! line up if it were drawn behind this, so it deliberately is not. A
//! log-referred histogram to sit behind the curve belongs with the scopes.

use crate::basic;
use pe_core::{Curve, History, ParamValue, RowId};

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

pub fn editor(ui: &mut egui::Ui, history: &mut History) {
    let Some(id) = history.document().stack.find_by_effect("curves") else {
        return;
    };

    let tab_id = ui.make_persistent_id("tone_curve_channel");
    // 0 is the parametric curve; 1..=4 are the point curves, offset by one.
    let mut channel: usize = ui.data_mut(|d| *d.get_temp_mut_or(tab_id, 0usize));

    ui.horizontal(|ui| {
        if ui.selectable_label(channel == 0, "Parametric").clicked() {
            channel = 0;
        }
        for (i, (_, label)) in CHANNELS.iter().enumerate() {
            if ui.selectable_label(channel == i + 1, *label).clicked() {
                channel = i + 1;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(tab_id, channel));

    if channel == 0 {
        parametric(ui, history, id);
        return;
    }

    let key = CHANNELS[channel - 1].0;
    canvas(ui, history, id, key);

    ui.horizontal(|ui| {
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
        ui.label(
            egui::RichText::new("click to add · right-click to remove")
                .small()
                .weak(),
        );
    });
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

fn canvas(ui: &mut egui::Ui, history: &mut History, id: RowId, key: &'static str) {
    let side = ui.available_width().min(300.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());

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
