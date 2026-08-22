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

use pe_core::{History, ParamValue, Pin, Pins, RowId, Warp};

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
            chroma_warp(ui, history, id, row_id);
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

/// The chromaticity plot, its pins, and the selected pin's controls.
///
/// A pin is placed where the colour you care about *is*, dragged to where you
/// want it to go, and told how far around itself to reach. That is a different
/// question from the one the grids answer, which is why this is a view and not
/// a third pair of axes.
fn chroma_warp(ui: &mut egui::Ui, history: &mut History, id: RowId, row_id: egui::Id) {
    let pins = read_pins(history, id);
    let chosen_id = row_id.with("pin");
    let mut chosen: Option<usize> = ui.data_mut(|d| d.get_temp(chosen_id).unwrap_or(None));
    if chosen.is_some_and(|i| i >= pins.len()) {
        chosen = None;
    }

    let mut next: Option<Pins> = None;
    let side = ui.available_width().clamp(120.0, 320.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click_and_drag());

    if ui.is_rect_visible(rect) {
        gamut(ui.painter(), rect);
        draw_pins(ui.painter(), rect, &pins, chosen);
    }

    // Which pin a drag belongs to, decided once when it starts.
    let held_id = row_id.with("pin_held");
    let mut held: Option<usize> = ui.data_mut(|d| d.get_temp(held_id).unwrap_or(None));
    if response.drag_started()
        && let Some(p) = response.interact_pointer_pos()
    {
        held = grabbed(rect, &pins, p);
        if held.is_some() {
            chosen = held;
        }
        ui.data_mut(|d| d.insert_temp(held_id, held));
    }
    if response.drag_stopped() {
        ui.data_mut(|d| d.insert_temp(held_id, None::<usize>));
        history.break_coalescing();
    }
    if response.dragged()
        && let Some(i) = held
        && let Some(p) = response.interact_pointer_pos()
    {
        let mut moved = pins.clone();
        if let Some(pin) = moved.get_mut(i) {
            pin.to = plot_from_screen(rect, p);
        }
        next = Some(moved);
    }

    // A click on empty plot selects nothing; a click on a pin selects it.
    if response.clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        chosen = grabbed(rect, &pins, p);
    }

    ui.horizontal(|ui| {
        let room = pins.len() < pe_core::pins::MAX_PINS;
        if ui
            .add_enabled(room, egui::Button::new("Add pin"))
            .on_hover_text("Places a pin in the middle of the plot")
            .clicked()
        {
            let mut added = pins.clone();
            if let Some(i) = added.add(Pin::placed([0.33, 0.35])) {
                chosen = Some(i);
            }
            next = Some(added);
        }
        if ui
            .add_enabled(chosen.is_some(), egui::Button::new("Delete"))
            .clicked()
            && let Some(i) = chosen
        {
            let mut fewer = pins.clone();
            fewer.remove(i);
            chosen = None;
            next = Some(fewer);
        }
        ui.label(
            egui::RichText::new(match pins.len() {
                0 => "add a pin, then drag it".to_string(),
                n => format!("{n} pin{}", if n == 1 { "" } else { "s" }),
            })
            .small()
            .weak(),
        );
    });

    ui.data_mut(|d| d.insert_temp(chosen_id, chosen));

    // The selected pin's own controls. Dimmed with nothing selected, which is
    // how Resolve draws them and the only honest thing to do — they have
    // nothing to act on.
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Pin")
            .small()
            .color(crate::resolve::colour::TITLE),
    );
    let selected = chosen.and_then(|i| pins.get(i).copied());
    let mut edited = selected;
    ui.scope(|ui| {
        if selected.is_none() {
            ui.disable();
        }
        let mut pin = selected.unwrap_or(Pin::placed([0.33, 0.35]));
        let mut touched = false;
        for (label, value, range, decimals) in [
            ("Chroma Range", &mut pin.chroma_range, 0.0..=0.5, 3),
            ("Tonal Range Low", &mut pin.tonal_low, 0.0..=1.0, 3),
            ("Tonal Range High", &mut pin.tonal_high, 0.0..=1.0, 3),
            ("Tonal Range Pivot", &mut pin.tonal_pivot, 0.0..=1.0, 3),
            ("Exposure", &mut pin.exposure, -2.0..=2.0, 3),
        ] {
            let edit = crate::resolve::slider_row(
                ui,
                row_id.with(("pin_param", label)),
                label,
                value,
                range,
                decimals,
            );
            touched |= edit.changed || edit.reset;
            if edit.reset {
                let fresh = Pin::placed([0.0, 0.0]);
                *value = match label {
                    "Chroma Range" => fresh.chroma_range,
                    "Tonal Range Low" => fresh.tonal_low,
                    "Tonal Range High" => fresh.tonal_high,
                    "Tonal Range Pivot" => fresh.tonal_pivot,
                    _ => fresh.exposure,
                };
            }
            if edit.released {
                history.break_coalescing();
            }
        }
        if touched && selected.is_some() {
            edited = Some(pin);
        }
    });

    if let (Some(i), Some(pin)) = (chosen, edited)
        && selected != Some(pin)
    {
        let mut changed = pins.clone();
        if let Some(slot) = changed.get_mut(i) {
            *slot = pin;
        }
        next = Some(changed);
    }

    if let Some(pins) = next {
        history.edit("Chroma Warp", Some(format!("{}.pins", id.0)), move |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.params.set("pins", ParamValue::Pins(pins));
            }
        });
    }
}

fn read_pins(history: &History, id: RowId) -> Pins {
    history
        .document()
        .stack
        .get(id)
        .and_then(|r| r.params.get("pins"))
        .and_then(ParamValue::as_pins)
        .cloned()
        .unwrap_or_default()
}

/// Plot coordinates are CIE xy, which the plot draws directly: x across,
/// y up, both over 0..0.8 — the range the spectral locus actually occupies.
const PLOT_SPAN: f32 = 0.8;

fn plot_to_screen(rect: egui::Rect, at: [f32; 2]) -> egui::Pos2 {
    egui::pos2(
        rect.min.x + (at[0] / PLOT_SPAN).clamp(0.0, 1.0) * rect.width(),
        rect.max.y - (at[1] / PLOT_SPAN).clamp(0.0, 1.0) * rect.height(),
    )
}

fn plot_from_screen(rect: egui::Rect, p: egui::Pos2) -> [f32; 2] {
    [
        ((p.x - rect.min.x) / rect.width().max(1e-4)).clamp(0.0, 1.0) * PLOT_SPAN,
        ((rect.max.y - p.y) / rect.height().max(1e-4)).clamp(0.0, 1.0) * PLOT_SPAN,
    ]
}

fn grabbed(rect: egui::Rect, pins: &Pins, p: egui::Pos2) -> Option<usize> {
    let want = plot_from_screen(rect, p);
    let (i, _) = pins.nearest(want)?;
    let pin = pins.get(i)?;
    (plot_to_screen(rect, pin.to).distance(p) <= GRAB).then_some(i)
}

/// The chromaticity plane, drawn as the colour at each point.
///
/// Every pixel is asked what colour its own coordinates are, which is what
/// makes this a picture of the space rather than a decoration of it — the pin
/// you place at a green really is sitting on the green.
fn gamut(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, 3.0, crate::theme::colour::VIEWER);
    const STEPS: usize = 40;
    let mut mesh = egui::Mesh::default();
    for row in 0..=STEPS {
        for col in 0..=STEPS {
            let u = col as f32 / STEPS as f32;
            let v = row as f32 / STEPS as f32;
            let at = [u * PLOT_SPAN, v * PLOT_SPAN];
            mesh.colored_vertex(plot_to_screen(rect, at), xy_colour(at));
        }
    }
    let w = STEPS as u32 + 1;
    for row in 0..STEPS as u32 {
        for col in 0..STEPS as u32 {
            let i = row * w + col;
            mesh.add_triangle(i, i + 1, i + w);
            mesh.add_triangle(i + 1, i + w, i + w + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));

    // A faint grid, so a pin's position can be read rather than only seen.
    let grid = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(20));
    for i in 1..4 {
        let t = i as f32 / 4.0;
        let x = rect.min.x + t * rect.width();
        let y = rect.min.y + t * rect.height();
        painter.line_segment([egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)], grid);
        painter.line_segment([egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)], grid);
    }
}

/// What colour sits at a chromaticity, as far as the display can show it.
///
/// Out-of-gamut coordinates — most of the plot, since the horseshoe is far
/// larger than any display — are darkened rather than clipped to a lie. The
/// shape that emerges is the gamut itself, which is the right thing for the
/// plot to be showing.
fn xy_colour(at: [f32; 2]) -> egui::Color32 {
    let (x, y) = (at[0], at[1]);
    if y <= 1e-3 || x + y >= 1.0 {
        return crate::theme::colour::VIEWER;
    }
    let xyz = [x / y, 1.0, (1.0 - x - y) / y];
    // XYZ to sRGB, which is what the screen has.
    let m = [
        [3.2406, -1.5372, -0.4986],
        [-0.9689, 1.8758, 0.0415],
        [0.0557, -0.2040, 1.0570],
    ];
    let mut rgb = [0.0f32; 3];
    for i in 0..3 {
        rgb[i] = m[i][0] * xyz[0] + m[i][1] * xyz[1] + m[i][2] * xyz[2];
    }
    let outside = rgb.iter().any(|c| *c < 0.0);
    let peak = rgb.iter().cloned().fold(1e-4f32, f32::max);
    let dim = if outside { 0.22 } else { 0.95 };
    let encode = |v: f32| {
        let v = (v / peak).clamp(0.0, 1.0) * dim;
        let g = if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        (g * 255.0) as u8
    };
    egui::Color32::from_rgb(encode(rgb[0]), encode(rgb[1]), encode(rgb[2]))
}

fn draw_pins(painter: &egui::Painter, rect: egui::Rect, pins: &Pins, chosen: Option<usize>) {
    for (i, pin) in pins.iter().enumerate() {
        let from = plot_to_screen(rect, pin.at);
        let to = plot_to_screen(rect, pin.to);
        let on = chosen == Some(i);
        let tint = if on {
            crate::theme::colour::ACCENT
        } else {
            egui::Color32::from_white_alpha(210)
        };

        // How far the pin reaches, which is the control people forget is
        // there until they can see it.
        let reach = pin.chroma_range / PLOT_SPAN * rect.width();
        painter.circle_stroke(
            from,
            reach.max(2.0),
            egui::Stroke::new(1.0_f32, tint.gamma_multiply(0.45)),
        );
        if from.distance(to) > 0.5 {
            painter.line_segment([from, to], egui::Stroke::new(1.2_f32, tint));
        }
        // The origin is a ring and the handle is solid: one says where the
        // colour was, the other where it is going.
        painter.circle_stroke(from, 3.0, egui::Stroke::new(1.2_f32, tint));
        painter.circle_filled(to, if on { 5.0 } else { 4.0 }, tint);
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
