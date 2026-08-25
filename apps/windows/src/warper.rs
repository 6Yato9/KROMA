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

use pe_core::pins::{PLOT_MIN, PLOT_SPAN, plot_fraction, plot_value};
use pe_core::{History, ParamValue, Pin, Pins, RowId, Warp};
use pe_scopes::warper::{Distribution, GRID};

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
pub fn panel(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    row_id: egui::Id,
    seen: Option<&Distribution>,
) {
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
            grid_plot(ui, history, id, row_id, "hue_sat", Axes::HueSat, seen);
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
            grid_plot(ui, history, id, row_id, key, Axes::ChromaLuma, seen);
            ui.add_space(4.0);
            crate::basic::slider_of(ui, history, id, "colour_warper", "chroma_divisions");
            crate::basic::slider_of(ui, history, id, "colour_warper", "luma_divisions");
            crate::basic::slider_of(ui, history, id, "colour_warper", "axis_angle");
        }
        View::ChromaWarp => {
            chroma_warp(ui, history, id, row_id, seen);
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
fn chroma_warp(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    row_id: egui::Id,
    seen: Option<&Distribution>,
) {
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
        draw_plot(ui, rect, Plot::Chromaticity, seen);
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

// Plot coordinates are CIE xy, which the plot draws directly: x across and y
// up, over `PLOT_MIN..PLOT_SPAN`. The range and both mappings live in
// `pe_core::pins` rather than here, because the macOS shell draws the same plot
// and a shell is not somewhere the other shell can read a constant from.

fn plot_to_screen(rect: egui::Rect, at: [f32; 2]) -> egui::Pos2 {
    egui::pos2(
        rect.min.x + plot_fraction(at[0]) * rect.width(),
        rect.max.y - plot_fraction(at[1]) * rect.height(),
    )
}

fn plot_from_screen(rect: egui::Rect, p: egui::Pos2) -> [f32; 2] {
    [
        plot_value((p.x - rect.min.x) / rect.width().max(1e-4)),
        plot_value((rect.max.y - p.y) / rect.height().max(1e-4)),
    ]
}

fn grabbed(rect: egui::Rect, pins: &Pins, p: egui::Pos2) -> Option<usize> {
    let want = plot_from_screen(rect, p);
    let (i, _) = pins.nearest(want)?;
    let pin = pins.get(i)?;
    (plot_to_screen(rect, pin.to).distance(p) <= GRAB).then_some(i)
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
        let reach = pin.chroma_range / (PLOT_SPAN - PLOT_MIN) * rect.width();
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
#[allow(clippy::too_many_arguments)]
fn grid_plot(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    row_id: egui::Id,
    key: &'static str,
    axes: Axes,
    seen: Option<&Distribution>,
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
        // Each view needs its own measurement: a cloud plotted for one set
        // of axes says nothing on another.
        let plot = match axes {
            Axes::HueSat => Plot::HueSat,
            Axes::ChromaLuma => Plot::ChromaLuma,
        };
        draw_plot(ui, rect, plot, seen);
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
        let home = warp.home(col, row, axes.wraps());
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

/// Which plot is being drawn, for the one function that draws all three.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Plot {
    Chromaticity,
    HueSat,
    ChromaLuma,
}

/// How many texels across each plot's image is built.
///
/// Generated at a fixed size and scaled to whatever the panel gives it, rather
/// than rebuilt on every resize: the plot is smooth, so a linear filter loses
/// nothing you can see, and 384 squared is a tenth of a megapixel to fill.
const PLOT_TEXELS: usize = 384;

/// The plot, as an image: the space itself with the photograph's own colours
/// over it.
///
/// One image rather than a background with something drawn on top, because the
/// two have to be *composited*, and compositing on the CPU is the only place
/// the two can meet at the same resolution. Drawing the haze as a mesh of
/// little rectangles over a mesh of big ones is what made ours look like a
/// mosaic beside Resolve's: both grids showed through as their own geometry
/// instead of the picture they were meant to describe.
fn plot_image(plot: Plot, seen: Option<&Distribution>) -> egui::ColorImage {
    let n = PLOT_TEXELS;
    let mut pixels = vec![egui::Color32::BLACK; n * n];
    let raw = match (plot, seen) {
        (Plot::Chromaticity, Some(d)) => Some(&d.chromaticity),
        (Plot::HueSat, Some(d)) => Some(&d.hue_sat),
        (Plot::ChromaLuma, Some(d)) => Some(&d.chroma_luma),
        (_, None) => None,
    };
    // Smoothed before it is drawn. A frame's colours are a *sample* of a
    // continuous distribution, and at this grid most cells hold nothing or
    // one: reading between them bilinearly still shows the lattice, because
    // the lattice is genuinely what the counts look like. Spreading each
    // sample over its neighbours is the density it was measured from, and it
    // is the same argument the waveform makes for smoothing its levels.
    let smoothed = raw.map(|g| blur(g));
    let peak = smoothed
        .as_ref()
        .map_or(0.0, |g| g.iter().cloned().fold(0.0f32, f32::max));

    for row in 0..n {
        for col in 0..n {
            // Texel centres, and v measured upwards to match every plot here.
            let u = (col as f32 + 0.5) / n as f32;
            let v = 1.0 - (row as f32 + 0.5) / n as f32;

            let base = match plot {
                // The whole square is coloured and what is *outside* the
                // locus is dimmed, rather than the outside being black. A
                // black surround makes the plot a shape floating in nothing;
                // a dimmed one makes it a bright region of a continuous
                // field, which is what a gamut actually is — and it is how
                // Resolve draws it.
                //
                // The bright half is still well under full: the plot is a map
                // and the photograph's colours are what you came to look at,
                // and at full brightness the map wins.
                Plot::Chromaticity => {
                    let (x, y) = (plot_value(u), plot_value(v));
                    crate::locus::colour_at(x, y).map(|c| {
                        let dim = if crate::locus::inside(x, y) {
                            0.62
                        } else {
                            0.16
                        };
                        [c[0] * dim, c[1] * dim, c[2] * dim]
                    })
                }
                Plot::HueSat => {
                    // The square the hue/saturation grid is stored in: the disc
                    // of radius one, and nothing outside it.
                    let (x, y) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
                    let r = (x * x + y * y).sqrt();
                    (r <= 1.0).then(|| {
                        let hue = y.atan2(x) / std::f32::consts::TAU;
                        let c = crate::theme::Ramp::Hue.at(hue.rem_euclid(1.0));
                        let grey = 0.28;
                        [
                            grey + (c.r() as f32 / 255.0 - grey) * r,
                            grey + (c.g() as f32 / 255.0 - grey) * r,
                            grey + (c.b() as f32 / 255.0 - grey) * r,
                        ]
                    })
                }
                Plot::ChromaLuma => {
                    // Grey to colourful across, dark to light up.
                    let c = crate::theme::Ramp::Chroma.at(u);
                    let shade = 0.18 + v * 0.78;
                    Some([
                        c.r() as f32 / 255.0 * shade,
                        c.g() as f32 / 255.0 * shade,
                        c.b() as f32 / 255.0 * shade,
                    ])
                }
            };

            // The haze belongs only where colours can be. The blur above
            // happily spreads counts past the boundary, which drew a smear
            // along the outside of the horseshoe — a cloud over a region that
            // has no colours in it by definition.
            let inside = match plot {
                Plot::Chromaticity => crate::locus::inside(plot_value(u), plot_value(v)),
                _ => base.is_some(),
            };
            let mut rgb = base.unwrap_or([0.03, 0.03, 0.035]);

            // The photograph's colours, added rather than mixed: the haze
            // brightens whatever it lands on instead of tinting it, so a dense
            // cloud over a green still reads as a green with a lot of pixels
            // in it.
            if inside
                && let Some(grid) = smoothed.as_ref()
                && peak > 0.0
            {
                let t = sample_grid(grid, u, v) / peak;
                // A fourth root, because a photograph's colours are wildly
                // unevenly distributed: a sky is thousands of pixels in a
                // handful of cells and a red jacket is a hundred over dozens.
                // On a linear scale the jacket is invisible, and seeing the
                // jacket is the entire point.
                let haze = t.max(0.0).powf(0.25).clamp(0.0, 1.0) * 0.85;
                for c in &mut rgb {
                    *c = (*c + haze).min(1.0);
                }
            }

            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
            pixels[row * n + col] =
                egui::Color32::from_rgb(byte(rgb[0]), byte(rgb[1]), byte(rgb[2]));
        }
    }

    egui::ColorImage {
        size: [n, n],
        pixels,
        source_size: egui::vec2(n as f32, n as f32),
    }
}

/// Spread each cell's count over its neighbours.
///
/// Two separable passes with a [1, 4, 6, 4, 1] kernel, which is a five-wide
/// binomial — near enough a Gaussian for anything drawn at this size, and it
/// costs two multiply-adds a cell instead of twenty-five.
fn blur(grid: &[u32]) -> Vec<f32> {
    const K: [f32; 5] = [1.0, 4.0, 6.0, 4.0, 1.0];
    const SUM: f32 = 16.0;
    let mut across = vec![0.0f32; GRID * GRID];
    for row in 0..GRID {
        for col in 0..GRID {
            let mut total = 0.0;
            for (i, k) in K.iter().enumerate() {
                let x = (col as isize + i as isize - 2).clamp(0, GRID as isize - 1) as usize;
                total += grid[row * GRID + x] as f32 * k;
            }
            across[row * GRID + col] = total / SUM;
        }
    }
    let mut out = vec![0.0f32; GRID * GRID];
    for row in 0..GRID {
        for col in 0..GRID {
            let mut total = 0.0;
            for (i, k) in K.iter().enumerate() {
                let y = (row as isize + i as isize - 2).clamp(0, GRID as isize - 1) as usize;
                total += across[y * GRID + col] * k;
            }
            out[row * GRID + col] = total / SUM;
        }
    }
    out
}

/// A count grid read at a point, bilinearly.
///
/// Bilinear because the grid is coarser than the image it is being drawn into,
/// and the whole complaint about the old drawing was that you could see the
/// cells. Reading between them is what turns a lattice of counts back into the
/// cloud it was measured from.
///
/// `u` and `v` are fractions *of the plot*, and every grid is binned in those
/// same terms — the chromaticity one over `PLOT_MIN..PLOT_SPAN`, because that
/// is the range the plot is drawn over. One binned to a range of its own would
/// still be read here as though it were this one, which is how the cloud came
/// to sit eight cells from the colours it was measured from.
fn sample_grid(grid: &[f32], u: f32, v: f32) -> f32 {
    let n = GRID as f32;
    // The grid stores v downwards; the plot reads it upwards.
    let fx = (u * n - 0.5).clamp(0.0, n - 1.0);
    let fy = ((1.0 - v) * n - 0.5).clamp(0.0, n - 1.0);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(GRID - 1), (y0 + 1).min(GRID - 1));
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let at = |x: usize, y: usize| grid[y * GRID + x];
    let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * tx;
    let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * tx;
    top + (bottom - top) * ty
}

/// Draw a plot, keeping its image until something it depends on changes.
///
/// Rebuilt on the distribution's generation rather than every frame: it is a
/// hundred thousand texels of transcendental arithmetic, which is nothing once
/// and far too much sixty times a second.
fn draw_plot(ui: &mut egui::Ui, rect: egui::Rect, plot: Plot, seen: Option<&Distribution>) {
    let key = (
        plot,
        seen.map_or(0, |d| d.peaks[0] ^ d.peaks[1] ^ d.peaks[2]),
    );
    let id = egui::Id::new(("warper_plot", plot));
    let cached: Option<(u64, egui::TextureHandle)> = ui.data(|d| d.get_temp(id));
    let stamp = key.1 as u64;
    let texture = match cached {
        Some((had, handle)) if had == stamp => handle,
        _ => {
            let handle = ui.ctx().load_texture(
                format!("warper-plot-{}", stamp),
                plot_image(plot, seen),
                egui::TextureOptions::LINEAR,
            );
            ui.data_mut(|d| d.insert_temp(id, (stamp, handle.clone())));
            handle
        }
    };
    ui.painter().add(egui::Shape::image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    ));

    // The boundary of colour, traced. It is the one line on this plot that
    // means something on its own, and without it the shape's edge is only
    // wherever the colour happens to stop.
    if plot == Plot::Chromaticity {
        let curve = crate::locus::curve();
        let points: Vec<egui::Pos2> = curve
            .iter()
            .chain(curve.first())
            .map(|p| plot_to_screen(rect, *p))
            .collect();
        ui.painter().add(egui::Shape::line(
            points,
            egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(60)),
        ));
    }

    // A faint grid, so a position can be read rather than only seen.
    let grid = egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(18));
    for i in 1..4 {
        let t = i as f32 / 4.0;
        let x = rect.min.x + t * rect.width();
        let y = rect.min.y + t * rect.height();
        ui.painter()
            .line_segment([egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)], grid);
        ui.painter()
            .line_segment([egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)], grid);
    }
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
            let home = warp.home(col, row, axes.wraps());
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

/// The web itself, drawn where the warp has put it.
fn lattice(painter: &egui::Painter, rect: egui::Rect, warp: &Warp, axes: Axes) {
    let at = |col: u32, row: u32| {
        let home = warp.home(col, row, axes.wraps());
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

    /// Writes each plot to a PNG so the drawing can be looked at, which is the
    /// only way to check a thing whose whole job is to look right. Ignored, so
    /// it runs when asked and never in CI.
    #[test]
    #[ignore = "writes files; run by hand when the plots change"]
    fn write_the_plots_out() {
        let dir = std::env::temp_dir();
        // With a real distribution over it, because an empty plot is the one
        // state that cannot show whether the haze reads.
        let chart = pe_io::test_chart(640, 480);
        let seen = Distribution::from_display(&chart.pixels);
        for (plot, name) in [
            (Plot::Chromaticity, "chromaticity"),
            (Plot::HueSat, "hue_sat"),
            (Plot::ChromaLuma, "chroma_luma"),
        ] {
            let img = plot_image(plot, Some(&seen));
            let bytes: Vec<u8> = img
                .pixels
                .iter()
                .flat_map(|c| [c.r(), c.g(), c.b(), 255])
                .collect();
            let out = dir.join(format!("warper-{name}.png"));
            let decoded =
                pe_io::DecodedImage::new(PLOT_TEXELS as u32, PLOT_TEXELS as u32, bytes).unwrap();
            pe_io::save_jpeg(
                &decoded,
                out.with_extension("jpg"),
                95,
                &pe_color::space::SRGB,
            )
            .unwrap();
            eprintln!("wrote {}", out.with_extension("jpg").display());
        }
    }
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

    #[test]
    fn a_vertex_is_only_grabbed_when_it_was_aimed_at() {
        let w = Warp::identity(6, 6);
        let on_it = to_screen(
            plot(),
            Axes::ChromaLuma,
            w.home(2, 3, Axes::ChromaLuma.wraps()),
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
