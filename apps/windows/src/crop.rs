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
/// Draw the crop rectangle and handle a drag on it.
///
/// `visible` is the part of the frame that `target` shows. The two used to be
/// assumed equal, which is why the viewer was forced to fit whenever the crop
/// tool was open: any other zoom and the rectangle drifted away from the crop
/// it was supposed to be drawing. Told where it is looking, the overlay works
/// at any zoom, and the two controls stop being one.
pub fn overlay(
    ui: &egui::Ui,
    response: &egui::Response,
    target: egui::Rect,
    visible: egui::Rect,
    geometry: Geometry,
    source: (u32, u32),
) -> Option<Geometry> {
    let frame = geometry.enclosing(source.0, source.1);
    let uv = geometry.crop_uv_in(&frame, source.0, source.1);
    let span = egui::vec2(visible.width().max(1e-6), visible.height().max(1e-6));
    let rect = place(uv, target, visible);

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
        // Screen points into the frame's uv. Zoomed in, a point on screen is
        // a smaller step across the frame, which is what `visible` carries.
        let delta = egui::vec2(
            response.drag_delta().x / target.width().max(1e-4) * span.x,
            response.drag_delta().y / target.height().max(1e-4) * span.y,
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

/// Where a rectangle given in frame uv lands on screen.
///
/// `target` is the on-screen rectangle and `visible` is the part of the frame
/// it shows. Those two used to be assumed identical, which is why the viewer
/// was pinned to fit whenever this tool was open — at any other zoom the crop
/// rectangle drifted away from the crop it was drawing. Separating them is
/// what lets the two controls be two controls.
fn place(uv: [f32; 4], target: egui::Rect, visible: egui::Rect) -> egui::Rect {
    let span = egui::vec2(visible.width().max(1e-6), visible.height().max(1e-6));
    let at = |u: f32, v: f32| {
        egui::pos2(
            target.min.x + (u - visible.min.x) / span.x * target.width(),
            target.min.y + (v - visible.min.y) / span.y * target.height(),
        )
    };
    egui::Rect::from_min_max(at(uv[0], uv[1]), at(uv[2], uv[3]))
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

/// A pair of linked or independent numbers, the way Resolve shows Zoom and
/// Position.
///
/// Returns the new pair when either moved.
/// What an X/Y row reports.
#[derive(Clone, Copy, PartialEq)]
enum XyEdit {
    Set([f32; 2]),
    Reset,
}

fn xy_row(
    ui: &mut egui::Ui,
    label: &str,
    value: [f32; 2],
    range: std::ops::RangeInclusive<f32>,
    link: Option<&mut bool>,
) -> Option<XyEdit> {
    let mut next = value;
    let mut changed = false;
    let mut reset = false;
    ui.horizontal(|ui| {
        ui.add_sized(
            [96.0, 18.0],
            egui::Label::new(
                egui::RichText::new(label)
                    .small()
                    .color(crate::resolve::colour::LABEL),
            ),
        );
        let (lo, hi) = (*range.start(), *range.end());
        for (i, axis) in ["X", "Y"].iter().enumerate() {
            ui.label(egui::RichText::new(*axis).small().weak());
            let before = next[i];
            // Draggable, at a rate that takes eight hundred pixels to cross
            // the range. Zoom and Position are read to three decimals and
            // nudged rather than swept.
            ui.add_sized(
                [58.0, 18.0],
                egui::DragValue::new(&mut next[i])
                    .fixed_decimals(3)
                    .range(lo..=hi)
                    .speed((hi - lo) / 800.0),
            );
            if (before - next[i]).abs() > 1e-9 {
                next[i] = next[i].clamp(lo, hi);
                changed = true;
                if link.as_ref().is_some_and(|l| **l) {
                    next[1 - i] = next[i];
                }
            }
            if i == 0
                && let Some(l) = link.as_ref()
            {
                // The link chain between the two, exactly where Resolve puts
                // it. Zoom is the one people want locked; Position is not.
                let mut on = **l;
                let text = if on { "[=]" } else { "[ ]" };
                if ui
                    .small_button(text)
                    .on_hover_text("Link X and Y")
                    .clicked()
                {
                    on = !on;
                    changed = true;
                    next[1] = next[0];
                }
                if on != **l {
                    // Written back below, outside the borrow.
                    ui.data_mut(|d| d.insert_temp(egui::Id::new(("xy_link_pending", label)), on));
                }
            }
        }
        // The reset arrow every other row in the application has. These two
        // did not, which made them the only controls with no way back short
        // of remembering what the number used to be.
        ui.add_space(4.0);
        let arrow = egui::Rect::from_min_size(
            egui::pos2(ui.cursor().min.x, ui.min_rect().min.y),
            egui::vec2(18.0, 18.0),
        );
        ui.allocate_rect(arrow, egui::Sense::hover());
        reset = crate::resolve::reset_button(ui, arrow, egui::Id::new(("xy_reset", label)));
    });
    if let Some(l) = link
        && let Some(on) = ui.data(|d| d.get_temp::<bool>(egui::Id::new(("xy_link_pending", label))))
    {
        *l = on;
        ui.data_mut(|d| d.remove::<bool>(egui::Id::new(("xy_link_pending", label))));
    }
    if reset {
        return Some(XyEdit::Reset);
    }
    changed.then_some(XyEdit::Set(next))
}

/// Resolve's Transform panel, against our geometry.
///
/// Zoom is the reciprocal of the crop's size — zooming to 2 keeps half the
/// frame — and Position is where that crop sits. Saying it that way round
/// costs one division and means the panel reads exactly like Resolve's while
/// the document still stores the rectangle, which is the thing the renderer
/// needs and the only thing that survives a change of image size.
fn transform_section(ui: &mut egui::Ui, history: &mut History, source: (u32, u32)) {
    let g = history.document().geometry;

    let zoom = [1.0 / g.size[0].max(1e-3), 1.0 / g.size[1].max(1e-3)];
    let link_id = ui.make_persistent_id("zoom_link");
    let mut link: bool = ui.data_mut(|d| *d.get_temp_mut_or(link_id, true));
    match xy_row(ui, "Zoom", zoom, 1.0..=20.0, Some(&mut link)) {
        Some(XyEdit::Set(next)) => {
            edit(history, source, "Zoom", move |g| {
                g.size = [1.0 / next[0].max(1e-3), 1.0 / next[1].max(1e-3)];
            });
        }
        Some(XyEdit::Reset) => {
            edit(history, source, "Zoom", |g| g.size = [1.0, 1.0]);
        }
        None => {}
    }
    ui.data_mut(|d| d.insert_temp(link_id, link));

    match xy_row(ui, "Position", g.centre, -1.0..=1.0, None) {
        Some(XyEdit::Set(next)) => {
            // Slid back inside rather than shrunk. Moving a crop does not make
            // it stop fitting the way straightening does — the rectangle is
            // the same rectangle — and shrinking it here made Position quietly
            // change Zoom, which is one control writing another's value.
            let from = g.centre;
            history.edit("Position", None, move |doc| {
                doc.geometry.centre = next;
                doc.geometry.slide_to_fit(from, source.0, source.1);
            });
        }
        Some(XyEdit::Reset) => {
            edit(history, source, "Position", |g| g.centre = [0.0, 0.0]);
        }
        None => {}
    }

    let mut angle = g.angle;
    let row = crate::resolve::slider_row(
        ui,
        ui.id().with("crop_angle"),
        "Rotation Angle",
        &mut angle,
        -45.0..=45.0,
        3,
    );
    if row.changed {
        edit_coalesced(history, source, "Straighten", "crop.angle", move |g| {
            g.angle = angle
        });
    }
    if row.released {
        history.break_coalescing();
    }
    if row.reset {
        edit(history, source, "Straighten", |g| g.angle = 0.0);
    }

    ui.horizontal(|ui| {
        ui.add_sized(
            [96.0, 18.0],
            egui::Label::new(
                egui::RichText::new("Flip")
                    .small()
                    .color(crate::resolve::colour::LABEL),
            ),
        );
        let mut flip_h = g.flip_h;
        if ui
            .toggle_value(&mut flip_h, "H")
            .on_hover_text("Flip horizontally")
            .clicked()
        {
            edit(history, source, "Flip", move |g| g.flip_h = flip_h);
        }
        let mut flip_v = g.flip_v;
        if ui
            .toggle_value(&mut flip_v, "V")
            .on_hover_text("Flip vertically")
            .clicked()
        {
            edit(history, source, "Flip", move |g| g.flip_v = flip_v);
        }
        ui.add_space(8.0);
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
    });
}

/// Resolve's Cropping panel: how much comes off each edge.
///
/// The document stores a centre and a size because that is what survives a
/// change of image size and what the sampling map wants. Four edges is the
/// same rectangle said the other way, and it is the way people describe a
/// crop out loud — "take a bit off the left" is one number, not two.
fn cropping_section(ui: &mut egui::Ui, history: &mut History, source: (u32, u32)) {
    let g = history.document().geometry;
    let edges = [
        ("Crop Left", 0.5 + g.centre[0] - g.size[0] * 0.5),
        ("Crop Right", 0.5 - g.centre[0] - g.size[0] * 0.5),
        ("Crop Top", 0.5 + g.centre[1] - g.size[1] * 0.5),
        ("Crop Bottom", 0.5 - g.centre[1] - g.size[1] * 0.5),
    ];

    let mut next: Option<(usize, f32)> = None;
    for (i, (label, value)) in edges.iter().enumerate() {
        let mut v = value.max(0.0);
        let row = crate::resolve::slider_row(
            ui,
            ui.id().with(("crop_edge", i)),
            label,
            &mut v,
            0.0..=0.9,
            3,
        );
        if row.changed {
            next = Some((i, v));
        }
        if row.released {
            history.break_coalescing();
        }
        if row.reset {
            next = Some((i, 0.0));
        }
    }

    if let Some((i, v)) = next {
        let mut e = [
            edges[0].1.max(0.0),
            edges[1].1.max(0.0),
            edges[2].1.max(0.0),
            edges[3].1.max(0.0),
        ];
        // Hold the opposite edge still and let this one move, which is what
        // dragging a single edge means. Stopping short of it keeps the crop
        // from collapsing to nothing under a fast drag.
        let opposite = e[i ^ 1];
        e[i] = v.clamp(0.0, (1.0 - opposite - MIN_SIZE).max(0.0));
        edit_coalesced(history, source, "Crop", "crop.edges", move |g| {
            g.size = [1.0 - e[0] - e[1], 1.0 - e[2] - e[3]];
            g.centre = [(e[0] - e[1]) * 0.5, (e[2] - e[3]) * 0.5];
        });
    }
}

/// The Image page: transform, cropping, and what comes out.
pub fn panel(ui: &mut egui::Ui, history: &mut History, source: (u32, u32), active: &mut bool) {
    egui::CollapsingHeader::new("Transform")
        .default_open(true)
        .show(ui, |ui| transform_section(ui, history, source));

    egui::CollapsingHeader::new("Cropping")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.toggle_value(active, "Crop tool")
                    .on_hover_text("C — show the whole frame and drag the rectangle");
                if ui.small_button("Reset").clicked() {
                    edit(history, source, "Reset Crop", |g| *g = Geometry::default());
                }
            });
            ui.add_space(4.0);
            cropping_section(ui, history, source);

            ui.add_space(4.0);
            let g = history.document().geometry;
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
        });

    egui::CollapsingHeader::new("Output")
        .default_open(true)
        .show(ui, |ui| {
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
        });
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

    /// At fit the mapping is the identity it always was, so nothing that was
    /// working before depends on the new argument being right.
    #[test]
    fn at_fit_a_crop_lands_where_it_did() {
        let target = egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(210.0, 120.0));
        let whole = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        let r = place([0.25, 0.5, 0.75, 1.0], target, whole);
        assert!((r.min.x - 60.0).abs() < 0.01, "{r:?}");
        assert!((r.min.y - 70.0).abs() < 0.01, "{r:?}");
        assert!((r.max.x - 160.0).abs() < 0.01, "{r:?}");
        assert!((r.max.y - 120.0).abs() < 0.01, "{r:?}");
    }

    /// And zoomed in, a crop that fills the visible half fills the target.
    ///
    /// This is the whole reason the crop tool used to force the viewer back to
    /// fit: without the visible rectangle, the overlay drew the crop at a
    /// quarter of the size and a drag moved it at four times the rate.
    #[test]
    fn zoomed_in_the_crop_is_drawn_against_what_is_on_screen() {
        let target = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 100.0));
        // The viewer is showing the middle half of the frame in each axis.
        let visible = egui::Rect::from_min_max(egui::pos2(0.25, 0.25), egui::pos2(0.75, 0.75));
        let r = place([0.25, 0.25, 0.75, 0.75], target, visible);
        assert!((r.min.x - target.min.x).abs() < 0.01, "{r:?}");
        assert!((r.max.x - target.max.x).abs() < 0.01, "{r:?}");
        assert!((r.max.y - target.max.y).abs() < 0.01, "{r:?}");
    }
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
