//! Colour wheels.
//!
//! The widget that makes this feel like Resolve rather than Lightroom, and the
//! one no toolkit provides any part of: a hue disc drawn as a mesh, a puck you
//! drag, and a master below it.
//!
//! # The projection
//!
//! A wheel edits an RGB *offset* — three numbers — through a two-dimensional
//! control, so something has to define the mapping. The three primaries sit
//! 120 degrees apart with red to the right, which is the standard RGB triangle
//! and matches how a vectorscope reads.
//!
//! Going out (offset to position) is a projection onto that triangle; coming
//! back (position to offset) is its transpose scaled by 2/3, which is the
//! pseudo-inverse for a zero-mean triple. That factor is not cosmetic: without
//! it, dragging the puck to the rim and reading the value back would report an
//! offset half again as large as the one you asked for, and a round trip
//! through the wheel would drift.

use std::f32::consts::TAU;

use pe_core::{History, ParamValue, RowId, Wheel};

/// Chroma offset represented by a puck at the rim.
///
/// Deliberately small. Colour wheels are for nudging a grade, and a full-radius
/// drag that shifted the image by ±1.0 would make the outer half of the disc
/// unusable.
const RANGE: f32 = 0.2;

/// Angles of the three primaries, red first.
const PRIMARY_ANGLES: [f32; 3] = [0.0, TAU / 3.0, 2.0 * TAU / 3.0];

/// Offset triple to a position on the disc, in -1..1.
fn rgb_to_xy(rgb: [f32; 3]) -> egui::Vec2 {
    let mean = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
    let mut v = egui::Vec2::ZERO;
    for (i, angle) in PRIMARY_ANGLES.iter().enumerate() {
        let d = rgb[i] - mean;
        v += egui::vec2(d * angle.cos(), d * angle.sin());
    }
    v / RANGE
}

/// Position on the disc back to an offset triple.
fn xy_to_rgb(v: egui::Vec2) -> [f32; 3] {
    let mut rgb = [0.0f32; 3];
    for (i, angle) in PRIMARY_ANGLES.iter().enumerate() {
        // Transpose of the projection, scaled by 2/3 so that a round trip is
        // the identity rather than a 1.5x amplification.
        rgb[i] = (v.x * angle.cos() + v.y * angle.sin()) * RANGE * 2.0 / 3.0;
    }
    rgb
}

/// The colour to paint at a point on the rim.
fn rim_colour(angle: f32) -> egui::Color32 {
    let rgb = xy_to_rgb(egui::vec2(angle.cos(), angle.sin()));
    // Lift off mid-grey so the ring reads as hue rather than as a dark smear,
    // and normalise by RANGE so the ring is fully saturated at the rim
    // whatever RANGE happens to be.
    let f = |v: f32| ((0.5 + v / RANGE * 0.75).clamp(0.0, 1.0) * 255.0) as u8;
    egui::Color32::from_rgb(f(rgb[0]), f(rgb[1]), f(rgb[2]))
}

/// One wheel: hue disc, puck, and a master below.
fn wheel(ui: &mut egui::Ui, history: &mut History, id: RowId, key: &'static str, label: &str) {
    let mut value = history
        .document()
        .stack
        .get(id)
        .and_then(|r| r.params.get(key))
        .and_then(ParamValue::as_wheel)
        .copied()
        .unwrap_or_default();

    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).small().weak());

        let size = 108.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click_and_drag());
        let centre = rect.center();
        let radius = size * 0.46;

        // --- interaction -----------------------------------------------------
        let mut changed = false;
        if response.double_clicked() {
            value.rgb = [0.0; 3];
            changed = true;
        } else if response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            // Screen y grows downward; the disc reads counter-clockwise like a
            // vectorscope, so y is flipped going in and out.
            let mut v = egui::vec2((pos.x - centre.x) / radius, -(pos.y - centre.y) / radius);
            if v.length() > 1.0 {
                v /= v.length();
            }
            value.rgb = xy_to_rgb(v);
            changed = true;
        }

        if changed {
            let coalesce = Some(format!("wheel.{key}"));
            history.edit(label.to_string(), coalesce, |doc| {
                if let Some(row) = doc.stack.get_mut(id) {
                    row.params.set(key, ParamValue::Wheel(value));
                }
            });
        }
        if response.drag_stopped() {
            history.break_coalescing();
        }

        // --- drawing ---------------------------------------------------------
        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            let segments = 48;
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(centre, egui::Color32::from_gray(86));
            for i in 0..=segments {
                let a = i as f32 / segments as f32 * TAU;
                mesh.colored_vertex(
                    centre + egui::vec2(a.cos(), -a.sin()) * radius,
                    rim_colour(a),
                );
            }
            for i in 0..segments {
                mesh.add_triangle(0, 1 + i as u32, 2 + i as u32);
            }
            painter.add(egui::Shape::mesh(mesh));
            painter.circle_stroke(
                centre,
                radius,
                egui::Stroke::new(1.0_f32, egui::Color32::from_gray(30)),
            );

            let v = rgb_to_xy(value.rgb);
            let clamped = if v.length() > 1.0 { v / v.length() } else { v };
            let puck = centre + egui::vec2(clamped.x, -clamped.y) * radius;
            painter.circle_stroke(puck, 5.5, egui::Stroke::new(2.0_f32, egui::Color32::WHITE));
            painter.circle_stroke(
                puck,
                6.8,
                egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(140)),
            );
        }

        // Master. Resolve puts this as a ring around the wheel; a bar under it
        // is easier to hit at this size and does the same job.
        let mut master = value.master;
        let r = ui.add_sized(
            [size, 14.0],
            egui::Slider::new(&mut master, -0.5..=0.5).show_value(false),
        );
        if r.changed() {
            value.master = master;
            history.edit(
                label.to_string(),
                Some(format!("wheel.{key}.master")),
                |doc| {
                    if let Some(row) = doc.stack.get_mut(id) {
                        row.params.set(key, ParamValue::Wheel(value));
                    }
                },
            );
        }
        if r.drag_stopped() {
            history.break_coalescing();
        }
        if r.double_clicked() {
            value.master = 0.0;
            history.edit(label.to_string(), None, |doc| {
                if let Some(row) = doc.stack.get_mut(id) {
                    row.params.set(key, ParamValue::Wheel(value));
                }
            });
        }
    });
}

/// The four-way primaries panel.
///
/// Four wheels, not three. Offset is the one colourists reach for first, and
/// leaving it out is the most common way a clone of these controls feels
/// wrong — so it gets equal billing rather than being tucked away.
pub fn primaries(ui: &mut egui::Ui, history: &mut History) {
    let Some(id) = history.document().stack.find_by_effect("primaries") else {
        return;
    };

    // Two by two: four across would be under 80 points each in a 340 point
    // panel, which is too small to aim a puck at.
    ui.horizontal(|ui| {
        wheel(ui, history, id, "lift", "Lift");
        ui.add_space(6.0);
        wheel(ui, history, id, "gamma", "Gamma");
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        wheel(ui, history, id, "gain", "Gain");
        ui.add_space(6.0);
        wheel(ui, history, id, "offset", "Offset");
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.small_button("Reset wheels").clicked() {
            history.edit("Reset wheels", None, |doc| {
                if let Some(row) = doc.stack.get_mut(id) {
                    for key in ["lift", "gamma", "gain", "offset"] {
                        row.params.set(key, ParamValue::Wheel(Wheel::default()));
                    }
                }
            });
        }
        ui.label(
            egui::RichText::new("drag the puck · double-click to reset")
                .small()
                .weak(),
        );
    });
}

/// Resolve's log wheels.
///
/// The primaries wheels hinge the transfer curve at its ends, so they
/// interact: pull Lift up and the midtones follow. These address three tonal
/// *bands* whose boundaries you set yourself, so a shadow push genuinely
/// leaves the highlights alone. That is why both sets exist, and why Low and
/// High Range are controls — deciding where "shadow" stops is the point of the
/// tool.
pub fn log_wheels(ui: &mut egui::Ui, history: &mut History) {
    let Some(id) = history.document().stack.find_by_effect("log_wheels") else {
        // A document saved before log wheels existed simply has no such row.
        return;
    };

    ui.horizontal(|ui| {
        wheel(ui, history, id, "shadow", "Shadow");
        ui.add_space(6.0);
        wheel(ui, history, id, "midtone", "Midtone");
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        wheel(ui, history, id, "highlight", "Highlight");
        ui.add_space(6.0);
        wheel(ui, history, id, "offset", "Offset");
    });

    ui.add_space(4.0);
    crate::basic::slider(ui, history, "log_wheels", "low_range", "Low Range");
    crate::basic::slider(ui, history, "log_wheels", "high_range", "High Range");

    if ui.small_button("Reset wheels").clicked() {
        history.edit("Reset log wheels", None, |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                for key in ["shadow", "midtone", "highlight", "offset"] {
                    row.params.set(key, ParamValue::Wheel(Wheel::default()));
                }
            }
        });
    }
}

/// Both wheel sets, behind a tab.
pub fn panel(ui: &mut egui::Ui, history: &mut History) {
    let tab_id = ui.make_persistent_id("wheel_set");
    let mut log: bool = ui.data_mut(|d| *d.get_temp_mut_or(tab_id, false));
    ui.horizontal(|ui| {
        if ui.selectable_label(!log, "Primaries").clicked() {
            log = false;
        }
        if ui.selectable_label(log, "Log").clicked() {
            log = true;
        }
    });
    ui.data_mut(|d| d.insert_temp(tab_id, log));
    ui.add_space(4.0);

    if log {
        log_wheels(ui, history);
    } else {
        primaries(ui, history);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_projection_round_trips() {
        // The 2/3 factor is the whole point: without it, reading a puck
        // position back would report an offset half again too large, and every
        // pass through the widget would inflate the grade.
        for rgb in [
            [0.1f32, -0.05, -0.05],
            [-0.08, 0.12, -0.04],
            [0.0, 0.0, 0.0],
            [0.03, 0.01, -0.04],
        ] {
            let back = xy_to_rgb(rgb_to_xy(rgb));
            for i in 0..3 {
                assert!(
                    (back[i] - rgb[i]).abs() < 1e-5,
                    "channel {i}: {rgb:?} -> {back:?}"
                );
            }
        }
    }

    #[test]
    fn a_neutral_wheel_sits_at_the_centre() {
        assert_eq!(rgb_to_xy([0.0; 3]), egui::Vec2::ZERO);
        // A pure luminance offset carries no chroma, so it must not move the
        // puck either — that is what the master is for.
        assert_eq!(rgb_to_xy([0.2; 3]), egui::Vec2::ZERO);
    }

    #[test]
    fn red_sits_to_the_right() {
        let v = rgb_to_xy(xy_to_rgb(egui::vec2(1.0, 0.0)));
        assert!(
            v.x > 0.9 && v.y.abs() < 1e-4,
            "red should be at 0 degrees: {v:?}"
        );
    }

    #[test]
    fn the_primaries_are_evenly_spaced() {
        let angle = |v: egui::Vec2| v.y.atan2(v.x);
        let sep = |a: f32, b: f32| {
            let d = (a - b).abs() % TAU;
            d.min(TAU - d)
        };
        let red = angle(rgb_to_xy([1.0, 0.0, 0.0]));
        let green = angle(rgb_to_xy([0.0, 1.0, 0.0]));
        let blue = angle(rgb_to_xy([0.0, 0.0, 1.0]));

        assert!((sep(red, green) - TAU / 3.0).abs() < 1e-4);
        assert!((sep(green, blue) - TAU / 3.0).abs() < 1e-4);
        assert!((sep(blue, red) - TAU / 3.0).abs() < 1e-4);
    }

    #[test]
    fn a_rim_colour_is_saturated_and_a_centre_colour_is_not() {
        let red = rim_colour(0.0);
        assert!(red.r() > red.b() + 60, "rim at 0 should read red: {red:?}");
    }
}
