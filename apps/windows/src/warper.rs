//! The Colour Warper: a grid you drag, over a slice of colour.
//!
//! Resolve gives this three windows and one object. Hue against saturation is
//! drawn as a hexagonal web; chroma against luma is drawn as two rectangular
//! grids about two chromaticity axes. The axes change and the lattice does
//! not, which is why the views switch on an icon rather than being separate
//! tools — and why there is one widget in this file rather than three.
//!
//! The lattice is drawn *displaced*. A vertex sits where its own warp has put
//! it, so the web itself shows the shape of the edit; a grid that stayed on
//! its lattice and showed the displacement some other way would be a table of
//! numbers with lines between them.

use pe_core::{History, ParamValue, RowId, Warp};

/// Which window onto the lattice is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    ChromaWarp,
    HueSat,
    ChromaLuma,
}

impl View {
    fn title(self) -> &'static str {
        match self {
            View::ChromaWarp => "Chroma Warp",
            View::HueSat => "Hue - Saturation",
            View::ChromaLuma => "Chroma - Luma",
        }
    }
}

/// How close the pointer has to be to grab a vertex, in points.
const GRAB: f32 = 11.0;

/// The whole panel: the view strip, the plot, and the controls under it.
pub fn panel(ui: &mut egui::Ui, history: &mut History, id: RowId, row_id: egui::Id) {
    let view_id = row_id.with("warper_view");
    let mut view: View = ui.data_mut(|d| *d.get_temp_mut_or(view_id, View::HueSat));

    view_strip(ui, &mut view);
    ui.data_mut(|d| d.insert_temp(view_id, view));
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(view.title())
            .small()
            .color(crate::resolve::colour::TITLE),
    );
    ui.add_space(2.0);

    match view {
        View::HueSat => {
            grid_plot(ui, history, id, row_id, "hue_sat", Axes::HueSat);
            ui.add_space(4.0);
            crate::basic::slider_of(ui, history, id, "colour_warper", "hue_divisions");
            crate::basic::slider_of(ui, history, id, "colour_warper", "sat_divisions");
        }
        View::ChromaLuma => {
            let which = row_id.with("warper_grid");
            let mut second: bool = ui.data_mut(|d| *d.get_temp_mut_or(which, false));
            ui.horizontal(|ui| {
                if ui.selectable_label(!second, "Grid 1").clicked() {
                    second = false;
                }
                if ui.selectable_label(second, "Grid 2").clicked() {
                    second = true;
                }
            });
            ui.data_mut(|d| d.insert_temp(which, second));
            let key = if second {
                "chroma_luma_2"
            } else {
                "chroma_luma_1"
            };
            grid_plot(ui, history, id, row_id, key, Axes::ChromaLuma);
            ui.add_space(4.0);
            crate::basic::slider_of(ui, history, id, "colour_warper", "chroma_divisions");
            crate::basic::slider_of(ui, history, id, "colour_warper", "luma_divisions");
            crate::basic::slider_of(ui, history, id, "colour_warper", "axis_angle");
        }
        View::ChromaWarp => {
            not_yet(ui);
        }
    }
}

/// Which two axes the lattice is being read against.
#[derive(Clone, Copy, PartialEq)]
enum Axes {
    /// Hue around, saturation out from the middle.
    HueSat,
    /// Chroma across, luma up.
    ChromaLuma,
}

impl Axes {
    /// Whether the first axis is a circle. See `Warp::sample`.
    fn wraps(self) -> bool {
        self == Axes::HueSat
    }
}

/// The icon row that switches windows.
fn view_strip(ui: &mut egui::Ui, view: &mut View) {
    ui.horizontal(|ui| {
        for option in [View::ChromaWarp, View::HueSat, View::ChromaLuma] {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(30.0, 24.0), egui::Sense::click());
            if response.clicked() {
                *view = option;
            }
            if ui.is_rect_visible(rect) {
                let on = *view == option;
                let painter = ui.painter();
                if on {
                    painter.rect_filled(rect, 3.0, crate::theme::colour::CONTROL);
                }
                let tint = if on || response.hovered() {
                    crate::theme::colour::TITLE
                } else {
                    crate::theme::colour::ICON
                };
                icon(painter, rect, option, tint);
            }
            response.on_hover_text(option.title());
        }
    });
}

/// One glyph per view, drawn rather than lettered.
///
/// Each is a picture of its own axes: the horseshoe of a chromaticity
/// diagram, the hexagonal web of hue against saturation, the plain lattice of
/// chroma against luma. Resolve's are the same three shapes, and they are the
/// only labels the strip has room for.
fn icon(painter: &egui::Painter, rect: egui::Rect, view: View, tint: egui::Color32) {
    let c = rect.center();
    let stroke = egui::Stroke::new(1.2_f32, tint);
    match view {
        View::ChromaWarp => {
            // The spectral horseshoe: a curve up one side and a straight line
            // closing it, which is the shape of the visible gamut.
            let mut pts = Vec::new();
            for i in 0..=10 {
                let t = i as f32 / 10.0;
                let a = -2.2 + t * 2.6;
                pts.push(egui::pos2(
                    c.x + 7.0 * a.cos() - 1.0,
                    c.y - 7.0 * a.sin() + 1.0,
                ));
            }
            painter.add(egui::Shape::line(pts.clone(), stroke));
            if let (Some(f), Some(l)) = (pts.first(), pts.last()) {
                painter.line_segment([*f, *l], stroke);
            }
        }
        View::HueSat => {
            let mut pts = Vec::new();
            for i in 0..=6 {
                let a = std::f32::consts::TAU * i as f32 / 6.0;
                pts.push(egui::pos2(c.x + 7.0 * a.cos(), c.y - 7.0 * a.sin()));
            }
            painter.add(egui::Shape::line(pts, stroke));
            for i in 0..6 {
                let a = std::f32::consts::TAU * i as f32 / 6.0;
                painter.line_segment(
                    [c, egui::pos2(c.x + 7.0 * a.cos(), c.y - 7.0 * a.sin())],
                    egui::Stroke::new(0.8_f32, tint),
                );
            }
        }
        View::ChromaLuma => {
            let r = egui::Rect::from_center_size(c, egui::vec2(14.0, 12.0));
            painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
            for i in 1..3 {
                let t = i as f32 / 3.0;
                let x = r.min.x + t * r.width();
                let y = r.min.y + t * r.height();
                painter.line_segment(
                    [egui::pos2(x, r.min.y), egui::pos2(x, r.max.y)],
                    egui::Stroke::new(0.8_f32, tint),
                );
                painter.line_segment(
                    [egui::pos2(r.min.x, y), egui::pos2(r.max.x, y)],
                    egui::Stroke::new(0.8_f32, tint),
                );
            }
        }
    }
}

fn not_yet(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 96.0), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, 3.0, crate::theme::colour::WELL);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Chroma Warp is not built yet.\nIt places pins on the gamut rather than dragging a grid,\nwhich is a different tool wearing the same panel.",
            egui::FontId::proportional(11.0),
            crate::theme::colour::DIM,
        );
    }
}

/// Read the lattice, draw it, and turn a drag on a vertex into an edit.
fn grid_plot(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    row_id: egui::Id,
    key: &'static str,
    axes: Axes,
) {
    let warp = history
        .document()
        .stack
        .get(id)
        .and_then(|r| r.params.get(key))
        .and_then(ParamValue::as_warp)
        .cloned()
        .unwrap_or_default();

    let side = ui.available_width().clamp(120.0, 320.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());

    if ui.is_rect_visible(rect) {
        background(ui.painter(), rect, axes);
        lattice(ui.painter(), rect, &warp, axes);
    }

    // Which vertex is being dragged, decided once when the drag starts. Picking
    // the nearest one every frame instead would hand the drag to a neighbour
    // the moment it passed under the pointer.
    let held_id = row_id.with((key, "held"));
    let mut held: Option<(u32, u32)> = ui.data_mut(|d| d.get_temp(held_id).unwrap_or(None));

    if response.drag_started()
        && let Some(p) = response.interact_pointer_pos()
    {
        held = nearest(rect, &warp, axes, p);
        ui.data_mut(|d| d.insert_temp(held_id, held));
    }
    if response.drag_stopped() {
        ui.data_mut(|d| d.insert_temp(held_id, None::<(u32, u32)>));
        history.break_coalescing();
    }

    if response.dragged()
        && let Some((col, row)) = held
        && let Some(p) = response.interact_pointer_pos()
    {
        let want = from_screen(rect, axes, p);
        let home = home_of(&warp, axes, col, row);
        // The offset is what is stored, so it is the *difference* from where
        // the vertex would sit if it had never been touched.
        let mut offset = [want[0] - home[0], want[1] - home[1]];
        if axes.wraps() {
            // Round the hue difference the short way. Without this, dragging a
            // red vertex a little anticlockwise records almost a full turn.
            offset[0] -= offset[0].round();
        }
        offset[0] = offset[0].clamp(-0.5, 0.5);
        offset[1] = offset[1].clamp(-1.0, 1.0);

        if offset != warp.at(col, row) {
            let mut next = warp.clone();
            next.set(col, row, offset);
            history.edit(
                "Colour Warper",
                Some(format!("{}.{key}", id.0)),
                move |doc| {
                    if let Some(r) = doc.stack.get_mut(id) {
                        r.params.set(key, ParamValue::Warp(next));
                    }
                },
            );
        }
    }

    // Double-click a vertex to put it back, which is the reset arrow a plot
    // cannot have one of per control point.
    if response.double_clicked()
        && let Some(p) = response.interact_pointer_pos()
        && let Some((col, row)) = nearest(rect, &warp, axes, p)
    {
        let mut next = warp.clone();
        next.set(col, row, [0.0, 0.0]);
        history.edit("Colour Warper", None, move |doc| {
            if let Some(r) = doc.stack.get_mut(id) {
                r.params.set(key, ParamValue::Warp(next));
            }
        });
    }

    ui.horizontal(|ui| {
        if ui.small_button("Reset grid").clicked() {
            let mut next = warp.clone();
            next.clear();
            history.edit("Reset Colour Warper", None, move |doc| {
                if let Some(r) = doc.stack.get_mut(id) {
                    r.params.set(key, ParamValue::Warp(next));
                }
            });
        }
        ui.label(
            egui::RichText::new("drag a point · double-click to put one back")
                .small()
                .weak(),
        );
    });
}

/// Where a vertex sits when nothing has been done to it, in axis units.
fn home_of(warp: &Warp, axes: Axes, col: u32, row: u32) -> [f32; 2] {
    let u = match axes {
        // Hue wraps, so the last column is *not* the first: there are `cols`
        // distinct hues around the circle, not `cols - 1` plus a repeat.
        Axes::HueSat => col as f32 / warp.cols() as f32,
        Axes::ChromaLuma => col as f32 / (warp.cols() - 1).max(1) as f32,
    };
    let v = row as f32 / (warp.rows() - 1).max(1) as f32;
    [u, v]
}

/// Axis units to a point on screen.
fn to_screen(rect: egui::Rect, axes: Axes, at: [f32; 2]) -> egui::Pos2 {
    match axes {
        Axes::HueSat => {
            let a = at[0] * std::f32::consts::TAU;
            let r = at[1].clamp(0.0, 1.0) * rect.width() * 0.45;
            egui::pos2(rect.center().x + r * a.cos(), rect.center().y - r * a.sin())
        }
        Axes::ChromaLuma => egui::pos2(
            rect.min.x + at[0].clamp(0.0, 1.0) * rect.width(),
            // Luma up, which is the way every other plot in the application
            // draws it.
            rect.max.y - at[1].clamp(0.0, 1.0) * rect.height(),
        ),
    }
}

/// And back again.
fn from_screen(rect: egui::Rect, axes: Axes, p: egui::Pos2) -> [f32; 2] {
    match axes {
        Axes::HueSat => {
            let d = p - rect.center();
            let a = (-d.y).atan2(d.x).rem_euclid(std::f32::consts::TAU);
            let r = d.length() / (rect.width() * 0.45).max(1e-4);
            [a / std::f32::consts::TAU, r.clamp(0.0, 1.0)]
        }
        Axes::ChromaLuma => [
            ((p.x - rect.min.x) / rect.width().max(1e-4)).clamp(0.0, 1.0),
            ((rect.max.y - p.y) / rect.height().max(1e-4)).clamp(0.0, 1.0),
        ],
    }
}

/// The vertex nearest a point, if one is close enough to have been aimed at.
fn nearest(rect: egui::Rect, warp: &Warp, axes: Axes, p: egui::Pos2) -> Option<(u32, u32)> {
    let mut best: Option<((u32, u32), f32)> = None;
    for row in 0..warp.rows() {
        for col in 0..warp.cols() {
            let home = home_of(warp, axes, col, row);
            let o = warp.at(col, row);
            let at = to_screen(rect, axes, [home[0] + o[0], home[1] + o[1]]);
            let d = at.distance(p);
            if best.is_none_or(|(_, b)| d < b) {
                best = Some(((col, row), d));
            }
        }
    }
    best.filter(|(_, d)| *d <= GRAB).map(|(v, _)| v)
}

/// The colour behind the grid: the slice of colour the axes cut.
fn background(painter: &egui::Painter, rect: egui::Rect, axes: Axes) {
    painter.rect_filled(rect, 3.0, crate::theme::colour::WELL);
    let mut mesh = egui::Mesh::default();
    match axes {
        Axes::HueSat => {
            // A fan from a grey middle out to full colour at the rim, which is
            // what the two axes are.
            let centre = rect.center();
            let radius = rect.width() * 0.45;
            const STEPS: usize = 48;
            mesh.colored_vertex(centre, egui::Color32::from_gray(70));
            for i in 0..=STEPS {
                let t = i as f32 / STEPS as f32;
                let a = t * std::f32::consts::TAU;
                mesh.colored_vertex(
                    egui::pos2(centre.x + radius * a.cos(), centre.y - radius * a.sin()),
                    crate::theme::Ramp::Hue.at(t),
                );
                if i > 0 {
                    mesh.add_triangle(0, i as u32, i as u32 + 1);
                }
            }
        }
        Axes::ChromaLuma => {
            // Grey to colourful across, dark to light up. Drawn as one quad
            // per corner colour, which the GPU interpolates for free.
            const STEPS: usize = 12;
            for i in 0..=STEPS {
                let t = i as f32 / STEPS as f32;
                let x = rect.min.x + t * rect.width();
                for (j, y) in [rect.max.y, rect.min.y].into_iter().enumerate() {
                    let v = if j == 0 { 0.25_f32 } else { 0.95 };
                    let c = crate::theme::Ramp::Chroma.at(t);
                    mesh.colored_vertex(
                        egui::pos2(x, y),
                        egui::Color32::from_rgb(
                            (c.r() as f32 * v) as u8,
                            (c.g() as f32 * v) as u8,
                            (c.b() as f32 * v) as u8,
                        ),
                    );
                }
                if i > 0 {
                    let b = (i as u32 - 1) * 2;
                    mesh.add_triangle(b, b + 1, b + 2);
                    mesh.add_triangle(b + 1, b + 2, b + 3);
                }
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The web itself, drawn where the warp has put it.
fn lattice(painter: &egui::Painter, rect: egui::Rect, warp: &Warp, axes: Axes) {
    let at = |col: u32, row: u32| {
        let home = home_of(warp, axes, col, row);
        let o = warp.at(col, row);
        to_screen(rect, axes, [home[0] + o[0], home[1] + o[1]])
    };
    let thin = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(150));

    for row in 0..warp.rows() {
        // Along the first axis, closing the ring when it is one.
        let last = if axes.wraps() {
            warp.cols()
        } else {
            warp.cols() - 1
        };
        for col in 0..last {
            let next = (col + 1) % warp.cols();
            painter.line_segment([at(col, row), at(next, row)], thin);
        }
    }
    for col in 0..warp.cols() {
        for row in 0..warp.rows().saturating_sub(1) {
            painter.line_segment([at(col, row), at(col, row + 1)], thin);
        }
    }
    for row in 0..warp.rows() {
        for col in 0..warp.cols() {
            let moved = warp.at(col, row) != [0.0, 0.0];
            painter.circle_filled(
                at(col, row),
                if moved { 3.2 } else { 2.4 },
                if moved {
                    crate::theme::colour::ACCENT
                } else {
                    egui::Color32::from_white_alpha(220)
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plot() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 200.0))
    }

    /// Screen and axis units have to be inverses, or a dragged vertex lands
    /// somewhere other than the pointer.
    #[test]
    fn the_two_mappings_are_inverses() {
        for axes in [Axes::HueSat, Axes::ChromaLuma] {
            for at in [[0.25_f32, 0.4], [0.6, 0.9], [0.1, 0.2]] {
                let back = from_screen(plot(), axes, to_screen(plot(), axes, at));
                assert!(
                    (back[0] - at[0]).abs() < 0.01 && (back[1] - at[1]).abs() < 0.01,
                    "{at:?} came back as {back:?}"
                );
            }
        }
    }

    /// The hue axis has `cols` distinct hues around the circle, not `cols - 1`
    /// plus a repeat of the first. Getting this wrong puts every vertex in
    /// slightly the wrong place and leaves a visible kink at red.
    #[test]
    fn the_hue_axis_spaces_its_vertices_around_a_full_circle() {
        let w = Warp::identity(6, 4);
        assert_eq!(home_of(&w, Axes::HueSat, 0, 0)[0], 0.0);
        assert!((home_of(&w, Axes::HueSat, 3, 0)[0] - 0.5).abs() < 1e-6);
        // The chroma axis does not wrap, so its last column *is* the end.
        assert!((home_of(&w, Axes::ChromaLuma, 5, 0)[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_vertex_is_only_grabbed_when_it_was_aimed_at() {
        let w = Warp::identity(6, 6);
        let on_it = to_screen(
            plot(),
            Axes::ChromaLuma,
            home_of(&w, Axes::ChromaLuma, 2, 3),
        );
        assert_eq!(nearest(plot(), &w, Axes::ChromaLuma, on_it), Some((2, 3)));
        // The middle of a cell, which is the only place genuinely far from
        // every vertex. A point three grab-radii to the *right* is not — on a
        // six-column grid that is most of the way to the next one along, and
        // the first version of this test asserted the opposite of what it
        // meant.
        let between = on_it + egui::vec2(20.0, 20.0);
        assert_eq!(nearest(plot(), &w, Axes::ChromaLuma, between), None);
    }
}
