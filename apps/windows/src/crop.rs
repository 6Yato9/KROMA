//! The crop tool: the overlay on the image, and the panel beside it.
//!
//! While the tool is open the viewer shows the *enclosing* frame — the whole
//! source, straightened — rather than the cropped result. That is what makes
//! the rectangle draggable: the user can see what is outside the crop and pull
//! it back in, and because the enclosing frame carries the same angle and turn
//! as the crop, the rectangle stays axis-aligned on screen at any angle.
//!
//! Nothing here knows about pixels or source coordinates. It reads the crop as
//! a rectangle in the displayed frame's uv, moves that rectangle, and writes it
//! back; `pe_core::geometry` owns the arithmetic that makes those two the same
//! thing.

use pe_core::{AspectLock, Geometry, History, Resize};

/// How close, in points, the pointer has to be to grab an edge or a corner.
const GRAB: f32 = 14.0;

/// The smallest crop the tool will let you make, as a fraction of the frame.
/// Small enough to be no practical limit, large enough that the handles never
/// end up on top of each other.
const MIN_SIZE: f32 = 0.02;

/// Which part of the rectangle a drag has hold of.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Grip {
    Move,
    Edge {
        left: bool,
        right: bool,
        top: bool,
        bottom: bool,
    },
}

/// Work out what the pointer is over.
///
/// Corners win over edges because they are the smaller target and the one the
/// user had to aim for; the interior wins only when nothing else is close.
fn grip_at(rect: egui::Rect, pos: egui::Pos2) -> Option<Grip> {
    let left = (pos.x - rect.min.x).abs() <= GRAB;
    let right = (pos.x - rect.max.x).abs() <= GRAB;
    let top = (pos.y - rect.min.y).abs() <= GRAB;
    let bottom = (pos.y - rect.max.y).abs() <= GRAB;

    let inside_x = pos.x >= rect.min.x - GRAB && pos.x <= rect.max.x + GRAB;
    let inside_y = pos.y >= rect.min.y - GRAB && pos.y <= rect.max.y + GRAB;
    if !inside_x || !inside_y {
        return None;
    }

    if left || right || top || bottom {
        return Some(Grip::Edge {
            left: left && inside_y,
            right: right && inside_y,
            top: top && inside_x,
            bottom: bottom && inside_x,
        });
    }
    rect.contains(pos).then_some(Grip::Move)
}

/// Apply a drag to the rectangle, in the frame's uv.
fn dragged(rect: egui::Rect, grip: Grip, delta: egui::Vec2, ratio: Option<f32>) -> egui::Rect {
    let mut r = rect;
    match grip {
        Grip::Move => r = r.translate(delta),
        Grip::Edge {
            left,
            right,
            top,
            bottom,
        } => {
            if left {
                r.min.x = (r.min.x + delta.x).min(r.max.x - MIN_SIZE);
            }
            if right {
                r.max.x = (r.max.x + delta.x).max(r.min.x + MIN_SIZE);
            }
            if top {
                r.min.y = (r.min.y + delta.y).min(r.max.y - MIN_SIZE);
            }
            if bottom {
                r.max.y = (r.max.y + delta.y).max(r.min.y + MIN_SIZE);
            }
        }
    }

    // Hold the locked ratio by moving whichever edges the drag was not already
    // moving, so the corner under the pointer stays under the pointer.
    if let (
        Some(ratio),
        Grip::Edge {
            left,
            right,
            top,
            bottom,
        },
    ) = (ratio, grip)
    {
        let want_h = r.width() / ratio.max(1e-6);
        if (want_h - r.height()).abs() > 1e-6 {
            if top && !bottom {
                r.min.y = r.max.y - want_h;
            } else if bottom && !top {
                r.max.y = r.min.y + want_h;
            } else if left || right {
                let c = r.center().y;
                r.min.y = c - want_h * 0.5;
                r.max.y = c + want_h * 0.5;
            }
        }
    }
    r
}

/// Draw and drive the overlay. Returns the new geometry when it moved.
pub fn overlay(
    ui: &egui::Ui,
    response: &egui::Response,
    target: egui::Rect,
    geometry: Geometry,
    source: (u32, u32),
) -> Option<Geometry> {
    let frame = geometry.enclosing(source.0, source.1);
    let uv = geometry.crop_uv_in(&frame, source.0, source.1);
    let to_screen = |u: f32, v: f32| {
        egui::pos2(
            target.min.x + u * target.width(),
            target.min.y + v * target.height(),
        )
    };
    let rect = egui::Rect::from_min_max(to_screen(uv[0], uv[1]), to_screen(uv[2], uv[3]));

    let ratio = geometry.aspect.ratio(source.0, source.1);
    let grip_id = ui.make_persistent_id("crop_grip");
    let mut grip: Option<Grip> = ui.data_mut(|d| d.get_temp(grip_id).unwrap_or(None));

    let mut moved = None;
    if response.drag_started() {
        grip = response
            .interact_pointer_pos()
            .and_then(|p| grip_at(rect, p));
        ui.data_mut(|d| d.insert_temp(grip_id, grip));
    }
    if response.dragged()
        && let Some(g) = grip
    {
        // Screen points into the frame's uv.
        let delta = egui::vec2(
            response.drag_delta().x / target.width().max(1e-4),
            response.drag_delta().y / target.height().max(1e-4),
        );
        let uv_rect = egui::Rect::from_min_max(egui::pos2(uv[0], uv[1]), egui::pos2(uv[2], uv[3]));
        let next = dragged(uv_rect, g, delta, ratio);

        let mut candidate = geometry;
        candidate.set_crop_uv_in(
            &frame,
            source.0,
            source.1,
            [next.min.x, next.min.y, next.max.x, next.max.y],
        );
        // Refuse a drag that would put blank space in the picture rather than
        // letting it happen and leaving the user to discover it at export.
        // With no straightening angle this is simply the edge of the image, so
        // the crop stops where the photograph does.
        if candidate.fits(source.0, source.1) {
            moved = Some(candidate);
        }
    }
    if response.drag_stopped() {
        ui.data_mut(|d| d.insert_temp(grip_id, None::<Grip>));
    }

    draw(ui, target, rect);
    moved
}

fn draw(ui: &egui::Ui, target: egui::Rect, rect: egui::Rect) {
    let painter = ui.painter_at(target);

    // Everything outside the crop, dimmed. Four bands rather than a stencil,
    // which keeps it to four draw calls and no allocation.
    let shade = egui::Color32::from_black_alpha(150);
    let above = egui::Rect::from_min_max(target.min, egui::pos2(target.max.x, rect.min.y));
    let below = egui::Rect::from_min_max(egui::pos2(target.min.x, rect.max.y), target.max);
    let left = egui::Rect::from_min_max(
        egui::pos2(target.min.x, rect.min.y),
        egui::pos2(rect.min.x, rect.max.y),
    );
    let right = egui::Rect::from_min_max(
        egui::pos2(rect.max.x, rect.min.y),
        egui::pos2(target.max.x, rect.max.y),
    );
    for band in [above, below, left, right] {
        if band.width() > 0.0 && band.height() > 0.0 {
            painter.rect_filled(band, 0.0, shade);
        }
    }

    // Thirds. The one grid worth drawing by default: it is the composition
    // most people are checking against when they reach for the crop tool.
    let thin = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(70));
    for i in 1..3 {
        let t = i as f32 / 3.0;
        let x = rect.min.x + t * rect.width();
        let y = rect.min.y + t * rect.height();
        painter.line_segment([egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)], thin);
        painter.line_segment([egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)], thin);
    }

    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(1.5_f32, egui::Color32::from_white_alpha(220)),
        egui::StrokeKind::Inside,
    );

    // Corner brackets rather than square handles: they sit inside the crop, so
    // they never hide the edge they are attached to.
    let arm = 18.0_f32.min(rect.width() * 0.3).min(rect.height() * 0.3);
    let heavy = egui::Stroke::new(3.0_f32, egui::Color32::WHITE);
    for (corner, dx, dy) in [
        (rect.left_top(), 1.0, 1.0),
        (rect.right_top(), -1.0, 1.0),
        (rect.left_bottom(), 1.0, -1.0),
        (rect.right_bottom(), -1.0, -1.0),
    ] {
        painter.line_segment([corner, egui::pos2(corner.x + arm * dx, corner.y)], heavy);
        painter.line_segment([corner, egui::pos2(corner.x, corner.y + arm * dy)], heavy);
    }
}

/// The presets the panel offers, and what each locks to.
const ASPECTS: [(&str, AspectLock); 7] = [
    ("Free", AspectLock::Free),
    ("Original", AspectLock::Original),
    ("1:1", AspectLock::Ratio { w: 1.0, h: 1.0 }),
    ("3:2", AspectLock::Ratio { w: 3.0, h: 2.0 }),
    ("4:3", AspectLock::Ratio { w: 4.0, h: 3.0 }),
    ("16:9", AspectLock::Ratio { w: 16.0, h: 9.0 }),
    ("5:4", AspectLock::Ratio { w: 5.0, h: 4.0 }),
];

/// The long-edge sizes the export menu offers.
const SIZES: [(&str, Resize); 5] = [
    ("Full", Resize::Native),
    ("4096", Resize::LongEdge { pixels: 4096 }),
    ("2560", Resize::LongEdge { pixels: 2560 }),
    ("2048", Resize::LongEdge { pixels: 2048 }),
    ("1080", Resize::LongEdge { pixels: 1080 }),
];

/// The crop panel.
///
/// Every control writes through the same two steps: change the geometry, then
/// shrink it if the change left it hanging off the picture. Straightening
/// always costs some of the edges — the only question is whether the tool
/// takes them or leaves the user with blank corners.
pub fn panel(ui: &mut egui::Ui, history: &mut History, source: (u32, u32), active: &mut bool) {
    ui.horizontal(|ui| {
        ui.toggle_value(active, "Crop")
            .on_hover_text("C — show the whole frame and drag the rectangle");
        if ui.small_button("Reset").clicked() {
            edit(history, source, "Reset Crop", |g| *g = Geometry::default());
        }
    });

    let g = history.document().geometry;

    ui.add_space(4.0);
    let mut angle = g.angle;
    let edit_row = crate::resolve::slider_row(
        ui,
        ui.id().with("crop_angle"),
        "Angle (°)",
        &mut angle,
        -45.0..=45.0,
        2,
    );
    if edit_row.changed {
        edit_coalesced(history, source, "Straighten", "crop.angle", move |g| {
            g.angle = angle
        });
    }
    if edit_row.released {
        history.break_coalescing();
    }
    if edit_row.reset {
        edit(history, source, "Straighten", |g| g.angle = 0.0);
    }

    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        for (label, lock) in ASPECTS {
            if ui.selectable_label(g.aspect == lock, label).clicked() {
                edit(history, source, "Crop Aspect", move |g| {
                    g.aspect = lock;
                    g.apply_aspect(source.0, source.1);
                });
            }
        }
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui
            .button("⟲")
            .on_hover_text("Rotate anticlockwise")
            .clicked()
        {
            edit(history, source, "Rotate", |g| g.turns = (g.turns + 3) % 4);
        }
        if ui.button("⟳").on_hover_text("Rotate clockwise").clicked() {
            edit(history, source, "Rotate", |g| g.turns = (g.turns + 1) % 4);
        }
        let mut flip_h = g.flip_h;
        if ui.toggle_value(&mut flip_h, "Flip H").clicked() {
            edit(history, source, "Flip", move |g| g.flip_h = flip_h);
        }
        let mut flip_v = g.flip_v;
        if ui.toggle_value(&mut flip_v, "Flip V").clicked() {
            edit(history, source, "Flip", move |g| g.flip_v = flip_v);
        }
    });

    ui.add_space(6.0);
    ui.label(egui::RichText::new("Export size").small().strong());
    ui.horizontal_wrapped(|ui| {
        let current = history.document().resize;
        for (label, size) in SIZES {
            if ui.selectable_label(current == size, label).clicked() {
                history.edit("Export Size", None, move |doc| doc.resize = size);
            }
        }
    });

    let (w, h) = pe_render::export::output_size(history.document(), source.0, source.1);
    ui.label(
        egui::RichText::new(format!("{w} x {h} px"))
            .small()
            .monospace()
            .weak(),
    );
}

fn edit(history: &mut History, source: (u32, u32), label: &str, f: impl FnOnce(&mut Geometry)) {
    history.edit(label.to_string(), None, |doc| {
        f(&mut doc.geometry);
        doc.geometry.shrink_to_fit(source.0, source.1);
    });
}

fn edit_coalesced(
    history: &mut History,
    source: (u32, u32),
    label: &str,
    key: &str,
    f: impl FnOnce(&mut Geometry),
) {
    history.edit(label.to_string(), Some(key.to_string()), |doc| {
        f(&mut doc.geometry);
        doc.geometry.shrink_to_fit(source.0, source.1);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1))
    }

    #[test]
    fn a_corner_wins_over_the_edges_that_meet_there() {
        let r = rect(100.0, 100.0, 300.0, 200.0);
        let g = grip_at(r, egui::pos2(101.0, 101.0)).expect("something");
        assert_eq!(
            g,
            Grip::Edge {
                left: true,
                right: false,
                top: true,
                bottom: false
            },
            "the top-left corner grabs both of its edges at once"
        );
    }

    #[test]
    fn the_middle_moves_the_whole_rectangle() {
        let r = rect(100.0, 100.0, 300.0, 200.0);
        assert_eq!(grip_at(r, egui::pos2(200.0, 150.0)), Some(Grip::Move));
    }

    #[test]
    fn well_outside_grabs_nothing() {
        let r = rect(100.0, 100.0, 300.0, 200.0);
        assert_eq!(grip_at(r, egui::pos2(20.0, 150.0)), None);
    }

    #[test]
    fn dragging_an_edge_moves_only_that_edge() {
        let r = rect(0.2, 0.2, 0.8, 0.8);
        let out = dragged(
            r,
            Grip::Edge {
                left: true,
                right: false,
                top: false,
                bottom: false,
            },
            egui::vec2(0.1, 0.1),
            None,
        );
        assert!((out.min.x - 0.3).abs() < 1e-5);
        assert_eq!(out.max, r.max, "the far corner should not have moved");
        assert!((out.min.y - 0.2).abs() < 1e-5, "nor the top edge");
    }

    /// An edge dragged past its opposite would invert the rectangle, and the
    /// handles would swap under the pointer mid-drag.
    #[test]
    fn an_edge_cannot_be_dragged_through_its_opposite() {
        let r = rect(0.2, 0.2, 0.8, 0.8);
        let out = dragged(
            r,
            Grip::Edge {
                left: true,
                right: false,
                top: false,
                bottom: false,
            },
            egui::vec2(5.0, 0.0),
            None,
        );
        assert!(out.min.x < out.max.x, "{out:?} is inside out");
        assert!((out.width() - MIN_SIZE).abs() < 1e-5);
    }

    #[test]
    fn moving_keeps_the_size() {
        let r = rect(0.2, 0.2, 0.8, 0.6);
        let out = dragged(r, Grip::Move, egui::vec2(0.05, -0.05), None);
        assert!((out.width() - r.width()).abs() < 1e-6);
        assert!((out.height() - r.height()).abs() < 1e-6);
        assert!((out.min.x - 0.25).abs() < 1e-6);
    }

    /// With a ratio locked, dragging a side has to move the top and bottom to
    /// match — otherwise the lock would only hold on corner drags.
    #[test]
    fn a_locked_ratio_survives_dragging_a_side() {
        let r = rect(0.2, 0.2, 0.8, 0.8);
        let out = dragged(
            r,
            Grip::Edge {
                left: false,
                right: true,
                top: false,
                bottom: false,
            },
            egui::vec2(-0.3, 0.0),
            Some(1.0),
        );
        assert!(
            (out.width() - out.height()).abs() < 1e-5,
            "{out:?} is not square"
        );
        // A side drag grows or shrinks about the middle, so the rectangle does
        // not walk up the frame as the user works.
        assert!((out.center().y - r.center().y).abs() < 1e-5);
    }

    #[test]
    fn a_locked_ratio_holds_when_a_corner_is_dragged() {
        let r = rect(0.2, 0.2, 0.8, 0.8);
        let out = dragged(
            r,
            Grip::Edge {
                left: false,
                right: true,
                top: false,
                bottom: true,
            },
            egui::vec2(-0.2, 0.05),
            Some(2.0),
        );
        assert!(
            (out.width() / out.height() - 2.0).abs() < 1e-4,
            "{out:?} is {} to 1",
            out.width() / out.height()
        );
    }
}
