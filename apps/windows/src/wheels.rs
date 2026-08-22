//! Resolve's Primaries — Color Wheels panel.
//!
//! Four wheels, a row of controls above them and a row below. That layout is
//! the most-used screen in colour grading and every part of it earns its
//! place: the wheels for where a cast is, the numbers under them for saying a
//! value exactly, the master bar for the one adjustment you make without
//! looking away from the picture.
//!
//! The panel drives several pinned rows at once — Temp and Tint are white
//! balance, Contrast and Pivot are contrast, the wheels are Primaries, and the
//! bottom row is spread across Colour, Tone and Presence. That is the same
//! arrangement the Basic panel uses, and it works because a row is looked up
//! by the effect it runs, never by where it sits in the stack.
//!
//! Two of Resolve's numbers are shown in Resolve's units rather than the
//! document's. Saturation and Hue read 0 to 100 with 50 neutral, because that
//! is what a colourist's hand expects; the document stores the rotation in
//! degrees and the saturation as a signed multiplier, because those are the
//! quantities that mean something on their own. A panel is a view, and a view
//! may present a value in the units its reader thinks in — as long as both
//! directions of the mapping live side by side, which is why they do.
//!
//! # The projection
//!
//! A wheel edits an RGB *offset* — three numbers — through a two-dimensional
//! control, so something has to define the mapping. The three primaries sit
//! 120 degrees apart with red to the right, which is the standard RGB triangle
//! and matches how a vectorscope reads.
//!
//! Going out is a projection onto that triangle; coming back is its transpose
//! scaled by 2/3, the pseudo-inverse for a zero-mean triple. That factor is
//! not cosmetic: without it, dragging the puck to the rim and reading the
//! value back would report an offset half again as large as the one you asked
//! for, and a round trip through the wheel would drift.

use std::f32::consts::TAU;

use pe_core::{History, ParamValue, RowId, Wheel};

use crate::basic;
use crate::resolve;

/// Chroma offset represented by a puck at the rim.
///
/// Deliberately small. Colour wheels are for nudging a grade, and a
/// full-radius drag that shifted the image by ±1.0 would make the outer half
/// of the disc unusable.
const RANGE: f32 = 0.2;

/// Angles of the three primaries, red first.
const PRIMARY_ANGLES: [f32; 3] = [0.0, TAU / 3.0, 2.0 * TAU / 3.0];

/// Offset triple to a position on the disc, in -1..1.
fn rgb_to_xy(rgb: [f32; 3]) -> egui::Vec2 {
    let mean = (rgb[0] + rgb[1] + rgb[2]) / 3.0;
    let mut v = egui::Vec2::ZERO;
    for (i, angle) in PRIMARY_ANGLES.iter().enumerate() {
        let d = rgb[i] - mean;
        v += egui::vec2(angle.cos(), angle.sin()) * d;
    }
    v / RANGE
}

/// A position on the disc back to an offset triple.
fn xy_to_rgb(v: egui::Vec2) -> [f32; 3] {
    let mut rgb = [0.0f32; 3];
    for (i, angle) in PRIMARY_ANGLES.iter().enumerate() {
        rgb[i] = (v.x * angle.cos() + v.y * angle.sin()) * RANGE * 2.0 / 3.0;
    }
    rgb
}

/// The colour of each readout's underline: master, then the three channels.
fn channel_tint(i: usize) -> egui::Color32 {
    match i {
        1 => egui::Color32::from_rgb(226, 68, 68),
        2 => egui::Color32::from_rgb(64, 200, 84),
        3 => egui::Color32::from_rgb(74, 118, 236),
        _ => egui::Color32::from_gray(220),
    }
}

/// A fully saturated colour at a hue, for the ring.
fn hue_colour(hue: f32) -> egui::Color32 {
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
        (r * 205.0 + 34.0) as u8,
        (g * 205.0 + 34.0) as u8,
        (b * 205.0 + 34.0) as u8,
    )
}

/// The hue disc, the master arc around it, and the puck.
///
/// `home` is where this wheel's channels sit when it is doing nothing, and
/// `range` is how far they may go. The puck measures *from* home, so a Gain
/// wheel's puck sits in the middle at 1.00 exactly as a Lift wheel's does at
/// 0.00 — the two read the same because they mean the same thing.
fn disc(
    ui: &mut egui::Ui,
    size: f32,
    wheel: Wheel,
    home: f32,
    range: (f32, f32),
    master: bool,
) -> Option<Wheel> {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click_and_drag());
    let centre = rect.center();
    // The disc sits inside the master arc, which rides the outer edge.
    let radius = (size * 0.5 - 7.0).max(8.0);

    let mut moved = None;
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let mut v = (pos - centre) / radius;
        // Round, not square. A puck that could reach the corners would push a
        // channel half again as far diagonally as it does straight up.
        if v.length() > 1.0 {
            v /= v.length();
        }
        // The puck's travel is a fraction of whichever side of home is
        // shorter, so it can never ask for a value the box would refuse.
        let reach = (range.1 - home).min(home - range.0).abs().clamp(1e-4, 1.0);
        let rgb = xy_to_rgb(v);
        moved = Some(Wheel {
            rgb: [
                home + rgb[0] * reach,
                home + rgb[1] * reach,
                home + rgb[2] * reach,
            ],
            master: wheel.master,
        });
    }
    if response.double_clicked() {
        moved = Some(Wheel {
            rgb: [home; 3],
            master: wheel.master,
        });
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);

        // The hue ring, as a fan of quads out from a neutral centre. Saturated
        // at the rim and neutral in the middle, because the useful part of a
        // colour wheel is its edge — the middle is where you already are.
        const SEGMENTS: usize = 64;
        let mut mesh = egui::Mesh::default();
        for i in 0..SEGMENTS {
            let a0 = i as f32 / SEGMENTS as f32 * TAU;
            let a1 = (i + 1) as f32 / SEGMENTS as f32 * TAU;
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(centre, egui::Color32::from_gray(30));
            mesh.colored_vertex(
                centre + egui::vec2(a0.cos(), a0.sin()) * radius,
                hue_colour((a0 / TAU).rem_euclid(1.0)),
            );
            mesh.colored_vertex(
                centre + egui::vec2(a1.cos(), a1.sin()) * radius,
                hue_colour((a1 / TAU).rem_euclid(1.0)),
            );
            mesh.add_triangle(base, base + 1, base + 2);
        }
        painter.add(egui::Shape::mesh(mesh));

        // Crosshair, so the neutral point stays visible under the puck.
        let hair = egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(90));
        painter.line_segment(
            [
                egui::pos2(centre.x - radius, centre.y),
                egui::pos2(centre.x + radius, centre.y),
            ],
            hair,
        );
        painter.line_segment(
            [
                egui::pos2(centre.x, centre.y - radius),
                egui::pos2(centre.x, centre.y + radius),
            ],
            hair,
        );

        // The master, as an arc riding the outer edge. The same value as the
        // bar below, and worth showing twice: the arc is what you read while
        // your eyes are still on the wheel.
        let outer = size * 0.5 - 2.5;
        painter.circle_stroke(
            centre,
            outer,
            egui::Stroke::new(3.0_f32, egui::Color32::from_gray(42)),
        );
        // Where the master sits *in its range*, filled from the bottom
        // clockwise — not a signed sweep either side of home.
        //
        // Which is why Offset opens half full on the right: it sits at 25 on a
        // range of -175 to 255, which is a shade under halfway, and half of a
        // fill that starts at six o'clock is exactly the right-hand side. A
        // signed sweep would have shown nothing there, because nothing is what
        // a default has moved.
        //
        // On a wheel with no master the bar writes into the three channels
        // together, so the arc reads their mean — that *is* the achromatic
        // value there. Reading `master` on those would have left the ring
        // pinned wherever the default put it, which is the same frozen ring in
        // a new disguise.
        let achromatic = if master {
            wheel.master
        } else {
            (wheel.rgb[0] + wheel.rgb[1] + wheel.rgb[2]) / 3.0
        };
        let span = (range.1 - range.0).max(1e-4);
        let sweep = ((achromatic - range.0) / span).clamp(0.0, 1.0);
        if sweep.abs() > 1e-3 {
            let start = -std::f32::consts::FRAC_PI_2;
            let steps = 40;
            let points: Vec<egui::Pos2> = (0..=steps)
                .map(|i| {
                    let t = i as f32 / steps as f32 * sweep * std::f32::consts::PI;
                    egui::pos2(
                        centre.x + outer * (start + t).cos(),
                        centre.y + outer * (start + t).sin(),
                    )
                })
                .collect();
            painter.add(egui::Shape::line(
                points,
                egui::Stroke::new(3.0_f32, egui::Color32::from_gray(215)),
            ));
        }

        let at = centre + rgb_to_xy(wheel.rgb) * radius;
        let hot = response.hovered() || response.dragged();
        let r = if hot { 6.0 } else { 5.0 };
        painter.circle_filled(at, r, egui::Color32::WHITE);
        painter.circle_stroke(
            at,
            r,
            egui::Stroke::new(1.2_f32, egui::Color32::from_gray(30)),
        );
    }

    moved
}

/// The numbers under a wheel, each with its own coloured underline.
///
/// Four of them where there is a master and three where there is not, which
/// is how Resolve draws Offset: three channels and no achromatic ring.
fn readouts(
    ui: &mut egui::Ui,
    width: f32,
    wheel: Wheel,
    range: (f32, f32),
    master: bool,
) -> Option<Wheel> {
    let mut next = wheel;
    let mut changed = false;
    let count = if master { 4 } else { 3 };
    let cell = (width / count as f32).max(26.0);
    let first = if master { 0 } else { 1 };

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for i in first..4 {
            let mut v = if i == 0 {
                wheel.master
            } else {
                wheel.rgb[i - 1]
            };
            let before = v;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(cell, 25.0), egui::Sense::hover());
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_size(rect.min, egui::vec2(cell, 18.0))),
            );
            // Draggable, and finely: the puck is the coarse control and this
            // is how you settle on a number without it.
            child.add_sized(
                egui::vec2(cell - 2.0, 18.0),
                egui::DragValue::new(&mut v)
                    // Two everywhere, as Resolve shows them: Lift reads 0.00
                    // and Offset reads 25.00.
                    .fixed_decimals(2)
                    .range(range.0..=range.1)
                    .speed((range.1 - range.0) / 500.0),
            );
            if (before - v).abs() > 1e-9 {
                let v = v.clamp(range.0, range.1);
                if i == 0 {
                    next.master = v;
                } else {
                    next.rgb[i - 1] = v;
                }
                changed = true;
            }
            // Resolve underlines each readout in its channel's colour, which
            // is how four identical numbers stay apart at a glance.
            if ui.is_rect_visible(rect) {
                ui.painter().line_segment(
                    [
                        egui::pos2(rect.min.x + 3.0, rect.min.y + 21.0),
                        egui::pos2(rect.max.x - 3.0, rect.min.y + 21.0),
                    ],
                    egui::Stroke::new(2.0_f32, channel_tint(i)),
                );
            }
        }
    });

    changed.then_some(next)
}

/// The ribbed bar under each wheel: Resolve's master control.
///
/// Relative, not absolute — push it and the value moves, let go and the bar
/// stays where it was. That is what makes it usable without looking at it,
/// which is the whole reason it is separate from the numbers above.
fn master_bar(ui: &mut egui::Ui, width: f32, range: (f32, f32)) -> Option<f32> {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 13.0), egui::Sense::click_and_drag());
    let mut moved = None;
    if response.dragged() {
        // A full sweep of the bar crosses the wheel's own range, whatever that
        // range happens to be. Returned as a *delta*, because the caller is the
        // only one that knows whether it lands on the master or on the three
        // channels together.
        let per_point = (range.1 - range.0) / width.max(1e-4);
        moved = Some(response.drag_delta().x * per_point);
    }
    if response.double_clicked() {
        moved = Some(0.0);
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, egui::Color32::from_gray(24));
        let hot = response.hovered() || response.dragged();
        let tint = if hot {
            egui::Color32::from_gray(145)
        } else {
            egui::Color32::from_gray(92)
        };
        let mut x = rect.min.x + 3.0;
        while x < rect.max.x - 2.0 {
            painter.line_segment(
                [
                    egui::pos2(x, rect.min.y + 3.0),
                    egui::pos2(x, rect.max.y - 3.0),
                ],
                egui::Stroke::new(1.0_f32, tint),
            );
            x += 4.0;
        }
    }

    moved
}

/// One complete wheel: title, reset, disc, readouts, and a master bar where
/// there is a master.
///
/// The shape comes from the registry rather than from here, because the four
/// wheels are not interchangeable: Lift sits at zero and Gain at one, and
/// Offset has three channels where the others have four. Assuming otherwise
/// gave a Gain wheel that read 0.00 when it was doing nothing.
fn wheel_column(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    effect: &str,
    key: &'static str,
    title: &'static str,
    width: f32,
) {
    let Some(kind) = pe_effects::by_key(effect)
        .and_then(|e| e.param(key))
        .map(|p| p.kind)
    else {
        return;
    };
    let pe_effects::ParamKind::Wheel {
        min,
        max,
        default,
        master,
    } = kind
    else {
        return;
    };

    let wheel = history
        .document()
        .stack
        .get(id)
        .and_then(|r| r.params.get(key))
        .and_then(ParamValue::as_wheel)
        .copied()
        .unwrap_or_else(|| Wheel::uniform(default));

    let mut next: Option<Wheel> = None;
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.horizontal(|ui| {
            ui.add_sized(
                [(width - 24.0).max(20.0), 16.0],
                egui::Label::new(
                    egui::RichText::new(title)
                        .small()
                        .color(resolve::colour::TITLE),
                ),
            );
            if ui.small_button("R").on_hover_text("Reset").clicked() {
                next = Some(Wheel::uniform(default));
            }
        });
        if let Some(w) = disc(ui, width, wheel, default, (min, max), master) {
            next = Some(w);
        }
        ui.add_space(2.0);
        if let Some(w) = readouts(ui, width, wheel, (min, max), master) {
            next = Some(w);
        }
        // Every wheel has the bar, Offset included. What Offset does not have
        // is the fourth *readout box* — and those are two controls wearing one
        // idea: the box is an achromatic value you can read, the bar is an
        // achromatic nudge you cannot. Resolve draws four bars and three of
        // Offset's boxes.
        //
        // With no master to move, the bar moves the three channels together,
        // which is what an achromatic nudge means on a wheel that has no
        // achromatic component to put it in.
        if let Some(delta) = master_bar(ui, width, (min, max)) {
            next = Some(if master {
                Wheel {
                    rgb: wheel.rgb,
                    master: (wheel.master + delta).clamp(min, max),
                }
            } else {
                Wheel {
                    rgb: [
                        (wheel.rgb[0] + delta).clamp(min, max),
                        (wheel.rgb[1] + delta).clamp(min, max),
                        (wheel.rgb[2] + delta).clamp(min, max),
                    ],
                    master: wheel.master,
                }
            });
        }
    });

    if let Some(w) = next {
        history.edit(title, Some(format!("wheel.{key}")), move |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.params.set(key, ParamValue::Wheel(w));
            }
        });
    }
}

/// The four wheels, two by two.
///
/// Resolve puts them in a row because its colour page is a whole screen wide.
/// In a docked inspector a row of four leaves each disc too small to place a
/// puck in, and a wheel you cannot aim is not a wheel.
fn wheel_grid(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    effect: &str,
    wheels: [(&'static str, &'static str); 4],
) {
    let width = ((ui.available_width() - 14.0) / 2.0).clamp(90.0, 200.0);
    for row in 0..2 {
        ui.horizontal_top(|ui| {
            for column in 0..2 {
                let (key, title) = wheels[row * 2 + column];
                wheel_column(ui, history, id, effect, key, title, width);
                ui.add_space(6.0);
            }
        });
        ui.add_space(6.0);
    }
}

/// The Primaries panel.
pub fn primaries(ui: &mut egui::Ui, history: &mut History) {
    let Some(id) = history.document().stack.find_by_effect("primaries") else {
        return;
    };

    // Resolve puts Temp, Tint, Contrast, Pivot and Mid/Detail above the
    // wheels and Color Boost, Shadows, Highlights, Saturation, Hue and Lum
    // Mix below them. Every one of those is on the Basic panel already, and
    // two panels showing the same parameter is two places to look for it —
    // one of which is always the wrong one. The wheels are what this panel
    // is for.
    wheel_grid(
        ui,
        history,
        id,
        "primaries",
        [
            ("lift", "Lift"),
            ("gamma", "Gamma"),
            ("gain", "Gain"),
            ("offset", "Offset"),
        ],
    );
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

    wheel_grid(
        ui,
        history,
        id,
        "log_wheels",
        [
            ("shadow", "Shadow"),
            ("midtone", "Midtone"),
            ("highlight", "Highlight"),
            ("offset", "Offset"),
        ],
    );

    ui.add_space(6.0);
    basic::slider(ui, history, "log_wheels", "low_range", "Low Range");
    basic::slider(ui, history, "log_wheels", "high_range", "High Range");
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

    /// The property that makes a puck feel solid: pick it up, put it down, and
    /// the numbers are exactly where they were. Anything less and a wheel
    /// drifts a little every time it is touched.
    #[test]
    fn a_puck_picked_up_and_put_down_changes_nothing() {
        for v in [
            egui::vec2(0.0, 0.0),
            egui::vec2(0.4, -0.2),
            egui::vec2(-0.9, 0.1),
            egui::vec2(0.0, 1.0),
        ] {
            let back = rgb_to_xy(xy_to_rgb(v));
            assert!((back - v).length() < 1e-5, "{v:?} came back as {back:?}");
        }
    }

    /// A wheel says which way the colour is pushed, not how bright it is. What
    /// all three channels share belongs to the master bar, so the puck has to
    /// ignore it completely.
    #[test]
    fn the_puck_ignores_what_all_three_channels_share() {
        let pushed = rgb_to_xy([0.1, 0.1, 0.1]);
        assert!(pushed.length() < 1e-6, "a neutral lift moved the puck");
        let a = rgb_to_xy([0.05, -0.02, -0.03]);
        let b = rgb_to_xy([0.15, 0.08, 0.07]);
        assert!(
            (a - b).length() < 1e-5,
            "the same push at two brightnesses landed apart"
        );
    }

    #[test]
    fn pushing_towards_red_moves_the_puck_towards_red() {
        let v = rgb_to_xy(xy_to_rgb(egui::vec2(1.0, 0.0)));
        assert!(v.x > 0.9 && v.y.abs() < 1e-4, "{v:?}");
    }

    /// And every effect it reaches into has to be pinned, or the panel would
    /// be driving something the user can delete out from under it.
    #[test]
    fn every_effect_the_panel_drives_is_pinned() {
        for effect in ["primaries", "log_wheels"] {
            assert!(
                pe_effects::registry::PINNED_ROWS.contains(&effect),
                "{effect} is not pinned"
            );
        }
    }
}
