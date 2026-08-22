//! Resolve's inspector, as widgets.
//!
//! Every control in Resolve's Open FX panel is the same four-column row: a
//! right-aligned label, a thin track with a round handle, a boxed number you
//! can type into, and a reset arrow. Getting that row right once is most of
//! what makes the panel look like Resolve, and doing it by hand at each call
//! site is how the columns end up not lining up.
//!
//! egui's stock `Slider` cannot be this row — its label and value are part of
//! the widget, sized to their content, so two sliders with labels of different
//! lengths put their tracks in different places. A panel of thirty parameters
//! makes that immediately obvious.
//!
//! One thing from the screenshots is deliberately absent: the keyframe
//! diamond. There is no timeline in a photo editor, so it would be a control
//! that never does anything, and a dead button is worse than a missing one.

/// Width of the label column. Wide enough for "Geometry Factor" and
/// "Flickering Speed", which are the longest labels Resolve's own plugins use.
const LABEL_W: f32 = 112.0;
/// The label column, for callers that need to line something up with it.
pub const LABEL_WIDTH: f32 = LABEL_W;
/// Width of the boxed number.
const VALUE_W: f32 = 58.0;
/// Width of the reset arrow's hit area.
const RESET_W: f32 = 18.0;
const GAP: f32 = 6.0;
/// Height of one row. Resolve's are tight; this is what keeps thirty
/// parameters on one screen.
const ROW_H: f32 = 22.0;
/// Below this the track stops being a control worth dragging, so the label
/// column gives way instead.
const MIN_TRACK: f32 = 40.0;

use crate::theme::Ramp;
pub use crate::theme::colour;

/// How a track is drawn.
///
/// Two facts a plain grey bar cannot carry: what the parameter's axis *is*,
/// and where on that axis it does nothing. Both are worth a few lines of
/// painting — the second especially, since "put it back where it was" is the
/// most common thing anyone wants from a slider they have pushed too far.
#[derive(Clone, Copy, Default)]
pub struct TrackStyle {
    pub ramp: Ramp,
    /// Where neutral sits, as a fraction along the track. `None` when the
    /// parameter's neutral is its minimum — an exposure slider that starts at
    /// zero needs no mark, because the left end already is one.
    pub neutral: Option<f32>,
}

impl TrackStyle {
    /// Work the style out from the parameter's own definition.
    pub fn of(effect: &str, key: &str, min: f32, max: f32, neutral: f32) -> Self {
        let span = max - min;
        let t = if span.abs() < 1e-9 {
            0.0
        } else {
            (neutral - min) / span
        };
        Self {
            ramp: crate::theme::ramp_for(effect, key),
            // Only when it is somewhere you could miss. At either end the
            // track's own end is already the mark.
            neutral: (0.04..0.96).contains(&t).then_some(t),
        }
    }
}

/// The colour a control draws in, given whether it can do anything.
///
/// egui greys out a disabled `Ui` by adjusting its *visuals*, which is no help
/// to anything painted with a colour of its own — and almost everything here
/// is. Each of these rows dims itself.
fn dim(ui: &egui::Ui, c: egui::Color32) -> egui::Color32 {
    if ui.is_enabled() {
        c
    } else {
        c.gamma_multiply(0.42)
    }
}

/// What a parameter row reports back.
#[derive(Clone, Copy, Default)]
pub struct Edit {
    /// The value moved this frame.
    pub changed: bool,
    /// A drag finished, so the undo entry should stop coalescing.
    pub released: bool,
    /// The reset arrow was pressed.
    pub reset: bool,
}

/// Split a row into Resolve's columns: label, track, value, reset.
fn columns(ui: &mut egui::Ui, width: f32) -> (egui::Rect, egui::Rect, egui::Rect, egui::Rect) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::hover());

    // Narrow the label column rather than the track. A track shorter than
    // about forty points is not a control any more, and the arithmetic that
    // gives it a *negative* width hands egui a rectangle it treats as
    // interacting everywhere on screen.
    let fixed = GAP * 2.0 + VALUE_W + RESET_W;
    let label_w = LABEL_W.min((width - fixed - MIN_TRACK).max(24.0));
    let label = egui::Rect::from_min_size(rect.min, egui::vec2(label_w, ROW_H));
    let reset = egui::Rect::from_min_size(
        egui::pos2(rect.max.x - RESET_W, rect.min.y),
        egui::vec2(RESET_W, ROW_H),
    );
    let value = egui::Rect::from_min_size(
        egui::pos2(reset.min.x - GAP - VALUE_W, rect.min.y),
        egui::vec2(VALUE_W, ROW_H),
    );
    let left = label.max.x + GAP;
    let track = egui::Rect::from_min_max(
        egui::pos2(left, rect.min.y),
        egui::pos2((value.min.x - GAP).max(left), rect.max.y),
    );
    (label, track, value, reset)
}

fn label_text(ui: &egui::Ui, rect: egui::Rect, text: &str) {
    ui.painter().text(
        egui::pos2(rect.max.x, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        text,
        egui::FontId::proportional(11.5),
        dim(ui, colour::LABEL),
    );
}

/// Glyphs the toolbar draws instead of spelling out.
///
/// Only where the picture is unambiguous. "Undo" as a word costs four times
/// the width of the arrow everybody already reads as undo, and a toolbar is
/// mostly a competition for width — but a glyph nobody recognises costs the
/// user a hover and a guess, which is a far worse trade.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Undo,
    Redo,
}

/// A toolbar button with a drawn glyph rather than a label.
pub fn icon_button(ui: &mut egui::Ui, glyph: Glyph, tooltip: &str) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(26.0, 20.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() && ui.is_enabled() {
            painter.rect_filled(rect, 2.0, colour::CONTROL_HOT);
        }
        let tint = dim(
            ui,
            if response.hovered() {
                colour::TITLE
            } else {
                colour::ICON
            },
        );
        arrow(painter, rect.center(), glyph, tint);
    }
    response.on_hover_text(tooltip).clicked()
}

/// An arc with a head on it, one way round or the other.
///
/// The same glyph as the reset arrow at the end of every parameter row, which
/// is deliberate: they mean nearly the same thing, and a second drawing of
/// "put it back" would be a second thing to learn.
fn arrow(painter: &egui::Painter, centre: egui::Pos2, glyph: Glyph, tint: egui::Color32) {
    use std::f32::consts::PI;
    let r = 6.0_f32;
    // Mirrored rather than drawn twice: redo is undo the other way round, and
    // two hand-placed arcs would drift apart the first time either is nudged.
    let flip = if glyph == Glyph::Undo { 1.0 } else { -1.0 };
    let at = |a: f32| egui::pos2(centre.x + flip * r * a.cos(), centre.y - r * a.sin());

    // From the lower right, up over the top, back down the left: the shape
    // everybody already reads as one step backwards.
    let (from, to) = (-0.25 * PI, 1.14 * PI);
    const STEPS: usize = 20;
    let points: Vec<egui::Pos2> = (0..=STEPS)
        .map(|i| at(from + (to - from) * i as f32 / STEPS as f32))
        .collect();
    painter.add(egui::Shape::line(
        points.clone(),
        egui::Stroke::new(1.4_f32, tint),
    ));

    // The head takes the arc's own heading at the end, rather than a triangle
    // placed by hand: adjust the sweep and it stays attached and pointing the
    // way the line was travelling.
    let end = points[STEPS];
    let dir = (end - points[STEPS - 1]).normalized();
    let side = egui::vec2(-dir.y, dir.x);
    painter.add(egui::Shape::convex_polygon(
        vec![
            end + dir * 3.2,
            end - dir * 1.4 + side * 2.9,
            end - dir * 1.4 - side * 2.9,
        ],
        tint,
        egui::Stroke::NONE,
    ));
}

/// The circular reset arrow at the end of every row.
///
/// Public because the Image tab's X/Y rows are laid out by hand — two boxes
/// and a link chain will not fit the four-column row — and they still have to
/// end in the same arrow as everything else.
pub fn reset_button(ui: &mut egui::Ui, rect: egui::Rect, id: egui::Id) -> bool {
    let response = ui.interact(rect, id, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let c = rect.center();
        let r = 5.5_f32;
        let tint = dim(
            ui,
            if response.hovered() {
                colour::HANDLE_HOT
            } else {
                colour::ICON
            },
        );
        let stroke = egui::Stroke::new(1.3_f32, tint);
        // Three quarters of a circle, then an arrowhead — an anticlockwise
        // "undo" arc, which is the glyph Resolve uses and the one people read
        // as "put it back".
        let mut points = Vec::with_capacity(18);
        for i in 0..=16 {
            let a = std::f32::consts::PI * 0.35 + (i as f32 / 16.0) * std::f32::consts::PI * 1.5;
            points.push(egui::pos2(c.x + r * a.cos(), c.y - r * a.sin()));
        }
        ui.painter().add(egui::Shape::line(points.clone(), stroke));
        if let Some(end) = points.last() {
            let head = 2.6;
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    *end + egui::vec2(-head, -head),
                    *end + egui::vec2(head, -head * 0.4),
                    *end + egui::vec2(-head * 0.2, head),
                ],
                tint,
                egui::Stroke::NONE,
            ));
        }
    }
    response.on_hover_text("Reset").clicked()
}

/// Half the pointer's width, which is also how far the track is inset at
/// each end so the pointer never hangs off its own track.
const HANDLE_HW: f32 = 5.0;

/// The pointer that marks the value.
///
/// A house shape with its point up, not a circle. A circle marks a position;
/// a point marks a *place on a scale*, which is what a slider has. On a
/// coloured track that difference is the whole game — a disc covers the part
/// of the gradient you are trying to read, and its widest part sits exactly
/// where you want to see the colour underneath. The point is one pixel wide
/// where it meets the track, so it can stand on a hue ramp without hiding the
/// hue it is pointing at.
///
/// The dark outline is not decoration: the fill is a light grey, and against
/// the pale end of a temperature or luma ramp it would otherwise vanish.
fn pointer(painter: &egui::Painter, x: f32, y: f32, hot: bool, faded: bool) {
    let hw = HANDLE_HW;
    let mut fill = if hot {
        colour::HANDLE_HOT
    } else {
        colour::HANDLE
    };
    if faded {
        fill = fill.gamma_multiply(0.42);
    }
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(x, y - 7.0),
            egui::pos2(x + hw, y - 1.5),
            egui::pos2(x + hw, y + 5.5),
            egui::pos2(x - hw, y + 5.5),
            egui::pos2(x - hw, y - 1.5),
        ],
        fill,
        egui::Stroke::new(1.0_f32, colour::HANDLE_EDGE),
    ));
}

/// A gradient, drawn as a strip of coloured quads.
///
/// egui has no gradient brush and does not need one: a mesh with a colour per
/// vertex is interpolated by the GPU for free. Twenty-four steps is past the
/// point where more of them change the picture.
fn gradient(painter: &egui::Painter, rect: egui::Rect, ramp: Ramp, faded: bool) {
    const STEPS: usize = 24;
    let mut mesh = egui::Mesh::default();
    for i in 0..=STEPS {
        let t = i as f32 / STEPS as f32;
        let x = rect.min.x + t * rect.width();
        let mut c = ramp.at(t);
        if faded {
            c = c.gamma_multiply(0.42);
        }
        mesh.colored_vertex(egui::pos2(x, rect.min.y), c);
        mesh.colored_vertex(egui::pos2(x, rect.max.y), c);
        if i > 0 {
            let b = (i as u32 - 1) * 2;
            mesh.add_triangle(b, b + 1, b + 2);
            mesh.add_triangle(b + 1, b + 2, b + 3);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Draw the track and pointer, and turn a drag on it into a value.
fn track(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    t: f32,
    style: TrackStyle,
) -> (Option<f32>, bool) {
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    let mut moved = None;
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        // Measured against the inset span, so clicking where the pointer
        // *looks* like it should go puts it there. Without this the last few
        // points at each end cannot be reached by a click.
        let inner = (rect.width() - HANDLE_HW * 2.0).max(1e-4);
        moved = Some(((pos.x - rect.min.x - HANDLE_HW) / inner).clamp(0.0, 1.0));
    }

    if ui.is_rect_visible(rect) {
        let y = rect.center().y;
        let painter = ui.painter();
        let span = (rect.width() - HANDLE_HW * 2.0).max(0.0);
        let at = |v: f32| rect.min.x + HANDLE_HW + v.clamp(0.0, 1.0) * span;
        let bar = |half: f32| {
            egui::Rect::from_min_max(
                egui::pos2(rect.min.x, y - half),
                egui::pos2(rect.max.x, y + half),
            )
        };

        let faded = !ui.is_enabled();
        if style.ramp.is_plain() {
            painter.rect_filled(bar(2.0), 2.0, dim(ui, colour::TRACK));
            // How far it has been pushed, and from where. On a slider whose
            // neutral is the left end this is the ordinary "filled up to
            // here"; on a bipolar one it grows out of the middle, which is
            // the only drawing that gives you the sign at a glance.
            let from = style.neutral.unwrap_or(0.0);
            if (t - from).abs() > 0.002 {
                let (a, b) = (at(from.min(t)), at(from.max(t)));
                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(a, y - 2.0), egui::pos2(b, y + 2.0)),
                    2.0,
                    dim(ui, colour::TRACK_FILL),
                );
            }
        } else {
            gradient(painter, bar(2.5), style.ramp, faded);
        }

        // The neutral mark: where the parameter does nothing.
        if let Some(n) = style.neutral {
            let x = at(n);
            painter.line_segment(
                [egui::pos2(x, y - 4.5), egui::pos2(x, y + 4.5)],
                egui::Stroke::new(1.0_f32, colour::HANDLE_EDGE),
            );
        }

        pointer(
            painter,
            at(t),
            y,
            response.hovered() || response.dragged(),
            faded,
        );
    }
    (moved, response.drag_stopped())
}

/// How much finer dragging the number is than dragging the track.
///
/// The track crosses its whole range in the width of the panel; the box takes
/// four times as far. That ratio is the point of having both — the slider is
/// for finding roughly the right value and the box is for settling on one, and
/// a box that moved at the same rate would just be a second slider.
const FINE: f32 = 4.0;

/// The boxed number. Typed into, or dragged for a fine adjustment.
fn value_box(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    value: &mut f32,
    decimals: usize,
    speed: f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    // Shorter than the row it sits in. A field that fills the row reads as a
    // button, and thirty of them stacked up is a wall of boxes rather than a
    // column of numbers.
    let rect = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 17.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let visuals = child.visuals_mut();
    visuals.widgets.inactive.weak_bg_fill = colour::BOX_FILL;
    visuals.widgets.hovered.weak_bg_fill = colour::BOX_FILL;
    visuals.widgets.active.weak_bg_fill = colour::BOX_FILL;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, colour::BOX_EDGE);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, colour::HANDLE);
    visuals.extreme_bg_color = colour::BOX_FILL;
    child
        .add_sized(
            rect.size(),
            egui::DragValue::new(value)
                .fixed_decimals(decimals)
                .range(range)
                .speed(speed),
        )
        .changed()
}

/// One float parameter, laid out Resolve's way.
///
/// Three ways to set it, which is not redundancy: drag the track to find a
/// value, drag the number to settle on one, type into it to say one exactly.
/// The box moves four times slower than the track, and that difference is
/// what makes it a second control rather than a second copy of the first.
pub fn slider_row(
    ui: &mut egui::Ui,
    id: egui::Id,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: usize,
) -> Edit {
    slider_row_styled(ui, id, label, value, range, decimals, TrackStyle::default())
}

/// The same row, with the track told what it is measuring.
#[allow(clippy::too_many_arguments)]
pub fn slider_row_styled(
    ui: &mut egui::Ui,
    id: egui::Id,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: usize,
    style: TrackStyle,
) -> Edit {
    let width = ui.available_width();
    let (label_rect, track_rect, value_rect, reset_rect) = columns(ui, width);
    label_text(ui, label_rect, label);

    let (lo, hi) = (*range.start(), *range.end());
    let span = (hi - lo).max(1e-6);
    let mut out = Edit::default();

    let (moved, released) = track(
        ui,
        track_rect,
        id.with("track"),
        (*value - lo) / span,
        style,
    );
    if let Some(t) = moved {
        let next = lo + t * span;
        if (next - *value).abs() > 1e-9 {
            *value = next;
            out.changed = true;
        }
    }
    out.released = released;

    // Derived from the track's own width, so the ratio holds at any panel
    // size rather than being tuned for one.
    let speed = span / (track_rect.width().max(40.0) * FINE);
    if value_box(ui, value_rect, value, decimals, speed, lo..=hi) {
        *value = value.clamp(lo, hi);
        out.changed = true;
        // Not `released`: a drag on the box is still in progress, and breaking
        // the coalescing here would put one undo entry per pixel moved.
        out.released = ui.input(|i| i.pointer.any_released());
    }
    out.reset = reset_button(ui, reset_rect, id.with("reset"));
    out
}

/// A checkbox, aligned to the track column so it lines up with the sliders.
pub fn check_row(ui: &mut egui::Ui, id: egui::Id, label: &str, value: &mut bool) -> Edit {
    let width = ui.available_width();
    let (label_rect, track_rect, _, reset_rect) = columns(ui, width);
    let _ = label_rect;

    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(track_rect));
    let changed = child
        .horizontal(|ui| ui.checkbox(value, label).changed())
        .inner;
    Edit {
        changed,
        released: changed,
        reset: reset_button(ui, reset_rect, id.with("reset")),
    }
}

/// A dropdown. Resolve gives these the whole width between label and reset.
pub fn choice_row(
    ui: &mut egui::Ui,
    id: egui::Id,
    label: &str,
    options: &[&'static str],
    value: &mut String,
) -> Edit {
    let width = ui.available_width();
    let (label_rect, track_rect, value_rect, reset_rect) = columns(ui, width);
    label_text(ui, label_rect, label);

    let wide = egui::Rect::from_min_max(track_rect.min, value_rect.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(wide));
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.clone())
        .width(wide.width() - 4.0)
        .show_ui(&mut child, |ui| {
            for option in options {
                if ui
                    .selectable_value(value, (*option).to_string(), *option)
                    .clicked()
                {
                    changed = true;
                }
            }
        });
    Edit {
        changed,
        released: changed,
        reset: reset_button(ui, reset_rect, id.with("reset")),
    }
}

/// A colour swatch.
pub fn colour_row(ui: &mut egui::Ui, id: egui::Id, label: &str, value: &mut [f32; 3]) -> Edit {
    let width = ui.available_width();
    let (label_rect, track_rect, _, reset_rect) = columns(ui, width);
    label_text(ui, label_rect, label);

    let swatch = egui::Rect::from_min_size(
        egui::pos2(track_rect.min.x, track_rect.min.y + 2.0),
        egui::vec2(52.0, ROW_H - 4.0),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(swatch));
    // Working-gamut linear values, so the picker is fed the same numbers the
    // shader sees rather than a display-space guess.
    let changed = child
        .add_sized(swatch.size(), |ui: &mut egui::Ui| {
            ui.color_edit_button_rgb(value)
        })
        .changed();
    let _ = id;
    Edit {
        changed,
        released: changed,
        reset: reset_button(ui, reset_rect, id.with("reset")),
    }
}

/// A collapsible heading, like Resolve's "Add Vignetting".
pub fn section(ui: &mut egui::Ui, id: egui::Id, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    let mut open: bool = ui.data_mut(|d| *d.get_temp_mut_or(id, true));
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::click());
    if response.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.line_segment(
            [
                egui::pos2(rect.min.x, rect.min.y),
                egui::pos2(rect.max.x, rect.min.y),
            ],
            egui::Stroke::new(1.0_f32, colour::RULE),
        );
        chevron(
            painter,
            egui::pos2(rect.min.x + 10.0, rect.center().y),
            open,
        );
        painter.text(
            egui::pos2(rect.min.x + 22.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            title,
            egui::FontId::proportional(12.0),
            dim(ui, colour::TITLE),
        );
    }

    if open {
        body(ui);
    }
}

fn chevron(painter: &egui::Painter, at: egui::Pos2, open: bool) {
    let r = 3.6_f32;
    let stroke = egui::Stroke::new(1.4_f32, colour::ICON);
    let points = if open {
        // Pointing down.
        vec![
            at + egui::vec2(-r, -r * 0.5),
            at + egui::vec2(0.0, r * 0.6),
            at + egui::vec2(r, -r * 0.5),
        ]
    } else {
        vec![
            at + egui::vec2(-r * 0.5, -r),
            at + egui::vec2(r * 0.6, 0.0),
            at + egui::vec2(-r * 0.5, r),
        ]
    };
    painter.add(egui::Shape::line(points, stroke));
}

/// What the user did to an effect's title bar.
#[derive(Clone, Copy, Default)]
pub struct HeaderAction {
    pub toggled: bool,
    pub expand: bool,
    pub up: bool,
    pub down: bool,
    pub delete: bool,
    pub reset: bool,
}

/// An effect's title bar: enable pill, name, reorder, bin, reset.
pub fn effect_header(
    ui: &mut egui::Ui,
    id: egui::Id,
    name: &str,
    enabled: bool,
    open: bool,
    can_move: (bool, bool),
) -> HeaderAction {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::hover());
    let mut action = HeaderAction::default();

    if ui.is_rect_visible(rect) {
        ui.painter().line_segment(
            [
                egui::pos2(rect.min.x, rect.min.y),
                egui::pos2(rect.max.x, rect.min.y),
            ],
            egui::Stroke::new(1.0_f32, colour::RULE),
        );
    }

    // The enable pill.
    let pill = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + 4.0, rect.center().y - 7.0),
        egui::vec2(28.0, 14.0),
    );
    let pill_response = ui.interact(pill, id.with("enable"), egui::Sense::click());
    if pill_response.clicked() {
        action.toggled = true;
    }
    if ui.is_rect_visible(pill) {
        let painter = ui.painter();
        painter.rect_filled(
            pill,
            7.0,
            if enabled {
                colour::ACCENT_DIM
            } else {
                colour::WELL
            },
        );
        painter.rect_stroke(
            pill,
            7.0,
            egui::Stroke::new(1.0_f32, colour::BOX_EDGE),
            egui::StrokeKind::Inside,
        );
        // The knob moves. A switch that only changes colour is a light, and a
        // light does not say which way to push it — the position is what makes
        // the state readable without knowing the convention.
        let x = if enabled {
            pill.max.x - 7.0
        } else {
            pill.min.x + 7.0
        };
        painter.circle_filled(
            egui::pos2(x, pill.center().y),
            5.0,
            if enabled {
                colour::ACCENT
            } else {
                colour::ICON
            },
        );
    }
    pill_response.on_hover_text(if enabled { "Disable" } else { "Enable" });

    // The name, which doubles as the expander.
    let title = egui::Rect::from_min_max(
        egui::pos2(pill.max.x + 8.0, rect.min.y),
        egui::pos2(rect.max.x - 82.0, rect.max.y),
    );
    let title_response = ui.interact(title, id.with("title"), egui::Sense::click());
    if title_response.clicked() {
        action.expand = true;
    }
    if ui.is_rect_visible(title) {
        ui.painter().text(
            egui::pos2(title.min.x, title.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(13.5),
            if open { colour::ACCENT } else { colour::TITLE },
        );
    }

    // Reorder, bin, reset, laid out from the right edge inwards.
    let slot = |right: f32, w: f32| {
        egui::Rect::from_min_size(
            egui::pos2(right - w, rect.center().y - 9.0),
            egui::vec2(w, 18.0),
        )
    };
    let reset_rect = slot(rect.max.x - 6.0, 20.0);
    let bin_rect = slot(reset_rect.min.x - 6.0, 20.0);
    let move_rect = slot(bin_rect.min.x - 4.0, 20.0);

    action.reset = reset_button(ui, reset_rect, id.with("row_reset"));
    action.delete = bin(ui, bin_rect, id.with("bin"));
    let (up, down) = arrows(ui, move_rect, id.with("move"), can_move);
    action.up = up;
    action.down = down;
    action
}

/// Resolve stacks a small up and down arrow in one control.
fn arrows(ui: &mut egui::Ui, rect: egui::Rect, id: egui::Id, can: (bool, bool)) -> (bool, bool) {
    let top = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.center().y));
    let bottom = egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.center().y), rect.max);
    let up = ui.interact(top, id.with("up"), egui::Sense::click());
    let down = ui.interact(bottom, id.with("down"), egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        for (r, pointing_up, allowed, hot) in [
            (top, true, can.0, up.hovered()),
            (bottom, false, can.1, down.hovered()),
        ] {
            let tint = if !allowed {
                egui::Color32::from_gray(70)
            } else if hot {
                colour::HANDLE_HOT
            } else {
                colour::ICON
            };
            let c = r.center();
            let w = 4.0;
            let h = 3.2;
            let pts = if pointing_up {
                vec![
                    c + egui::vec2(0.0, -h),
                    c + egui::vec2(-w, h),
                    c + egui::vec2(w, h),
                ]
            } else {
                vec![
                    c + egui::vec2(0.0, h),
                    c + egui::vec2(-w, -h),
                    c + egui::vec2(w, -h),
                ]
            };
            painter.add(egui::Shape::convex_polygon(pts, tint, egui::Stroke::NONE));
        }
    }

    (
        can.0 && up.on_hover_text("Move up").clicked(),
        can.1 && down.on_hover_text("Move down").clicked(),
    )
}

fn bin(ui: &mut egui::Ui, rect: egui::Rect, id: egui::Id) -> bool {
    let response = ui.interact(rect, id, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let c = rect.center();
        let r = 5.0_f32;
        let tint = if response.hovered() {
            colour::HANDLE_HOT
        } else {
            colour::ICON
        };
        let stroke = egui::Stroke::new(1.3_f32, tint);
        let painter = ui.painter();
        // Handle, lid, then a tapered body.
        painter.line_segment(
            [c + egui::vec2(-r * 0.35, -r), c + egui::vec2(r * 0.35, -r)],
            stroke,
        );
        painter.line_segment(
            [c + egui::vec2(-r, -r * 0.55), c + egui::vec2(r, -r * 0.55)],
            stroke,
        );
        painter.line_segment(
            [
                c + egui::vec2(-r * 0.75, -r * 0.55),
                c + egui::vec2(-r * 0.55, r),
            ],
            stroke,
        );
        painter.line_segment(
            [
                c + egui::vec2(r * 0.75, -r * 0.55),
                c + egui::vec2(r * 0.55, r),
            ],
            stroke,
        );
        painter.line_segment(
            [c + egui::vec2(-r * 0.55, r), c + egui::vec2(r * 0.55, r)],
            stroke,
        );
    }
    response.on_hover_text("Delete").clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rects(width: f32) -> (egui::Rect, egui::Rect, egui::Rect, egui::Rect) {
        let ctx = egui::Context::default();
        let mut out = None;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(
                    egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(width, 400.0)),
                ));
                out = Some(columns(&mut child, width));
            });
        });
        out.expect("laid out")
    }

    /// Press the middle of an icon button and report whether it fired.
    ///
    /// Driven for real rather than reasoned about, because the whole question
    /// is whether a hand-painted widget inside a disabled `Ui` still counts a
    /// click — and that is egui's answer to give, not ours.
    fn press_icon(enabled: bool) -> bool {
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(120.0, 40.0));
        let mut fired = false;

        let mut run = |input: egui::RawInput| {
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                    child.add_enabled_ui(enabled, |ui| {
                        fired = icon_button(ui, Glyph::Undo, "Undo");
                    });
                });
            });
        };

        let base = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        // Frame one registers the widget; egui hit-tests against last frame.
        run(base.clone());

        let at = rect.min + egui::vec2(13.0, 10.0);
        let mut input = base;
        input.events = vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ];
        run(input);
        fired
    }

    /// Click into a row's number box, then press Left arrow the way the
    /// application does: consumed at the top of the frame, before any panel
    /// is drawn.
    ///
    /// Returns whether egui reports the keyboard as taken, and whether the
    /// key still reached the application anyway.
    ///
    /// Three frames to set up: one to register the widget, one to click it,
    /// and one for the field to come back as a text box and take focus.
    fn arrow_key_while_editing_a_number() -> (bool, bool) {
        let ctx = egui::Context::default();
        let width = 300.0;
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(width, 400.0));
        let mut value = 0.5_f32;
        let (mut busy, mut stolen) = (false, false);

        let mut frame = |input: egui::RawInput, value: &mut f32, watch: bool| {
            let _ = ctx.run(input, |ctx| {
                if watch {
                    // Exactly what the application does, in the same place.
                    busy = ctx.wants_keyboard_input();
                    stolen = ctx
                        .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft));
                }
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                    slider_row(
                        &mut child,
                        egui::Id::new("row"),
                        "Exposure",
                        value,
                        0.0..=1.0,
                        3,
                    );
                });
            });
        };

        let base = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        frame(base.clone(), &mut value, false);

        // The number box sits between the reset arrow and the track.
        let at = egui::pos2(
            rect.max.x - RESET_W - GAP - VALUE_W * 0.5,
            rect.min.y + ROW_H * 0.5,
        );
        let mut click = base.clone();
        click.events = vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ];
        frame(click, &mut value, false);

        let mut press = base;
        press.events = vec![egui::Event::Key {
            key: egui::Key::ArrowLeft,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }];
        frame(press, &mut value, true);
        (busy, stolen)
    }

    /// A row's number box is documented as something you can type into, and
    /// the application binds bare Left, Right, F, S and C to changing the
    /// photograph. Those two only coexist if the shortcuts stand down while a
    /// field has the keyboard.
    ///
    /// egui will not do it for you: the shortcuts are read at the top of the
    /// frame, before any widget is drawn, so the field never gets the chance
    /// to swallow the key. `wants_keyboard_input` is the only signal there is,
    /// and this pins down that it says what we need it to.
    #[test]
    fn a_focused_number_box_claims_the_keyboard() {
        let (busy, stolen) = arrow_key_while_editing_a_number();
        assert!(
            busy,
            "clicking the number box did not give it keyboard focus, so there is              nothing to hold the single-key shortcuts back with"
        );
        assert!(
            stolen,
            "egui declined the key on its own — if this ever starts failing, the              guard in the toolbar's input block is no longer carrying its weight"
        );
    }

    #[test]
    fn an_icon_button_reports_a_click() {
        assert!(press_icon(true), "clicking the glyph did nothing");
    }

    /// Undo with nothing to undo is drawn dim, and must be inert as well as
    /// look it: a toolbar that greys a control and then honours the click is
    /// worse than one that never greyed it.
    #[test]
    fn a_disabled_icon_button_does_not() {
        assert!(!press_icon(false), "a greyed-out glyph still fired");
    }

    /// Drive a real click at a point inside a row and report what the row
    /// says came back.
    ///
    /// Two frames, because egui hit-tests against the widget rectangles the
    /// *previous* frame registered — a one-frame test would click at a point
    /// where, as far as egui is concerned, there is nothing yet.
    fn click_at(width: f32, offset_from_right: f32) -> Edit {
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(width, 400.0));
        let mut value = 0.5_f32;

        let run = |input: egui::RawInput, value: &mut f32| -> Edit {
            let mut edit = Edit::default();
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                    edit = slider_row(
                        &mut child,
                        egui::Id::new("row"),
                        "Exposure",
                        value,
                        0.0..=1.0,
                        3,
                    );
                });
            });
            edit
        };

        // Frame one registers the widgets.
        let base = egui::RawInput {
            screen_rect: Some(rect),
            ..Default::default()
        };
        run(base.clone(), &mut value);

        // Frame two presses and releases on the point asked for.
        let at = egui::pos2(rect.max.x - offset_from_right, rect.min.y + ROW_H * 0.5);
        let mut input = base;
        input.events = vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
        ];
        run(input, &mut value)
    }

    /// The reset arrow has to be clickable where it is *drawn*.
    ///
    /// It sits at the very end of the row, which is also where a floating
    /// scrollbar lives and where a panel's edge is — the two places a click
    /// most easily goes somewhere else. Worth a test that presses the actual
    /// pixel rather than trusting the rectangle arithmetic.
    #[test]
    fn the_reset_arrow_answers_a_click_on_the_glyph() {
        // The glyph is drawn at the centre of the reset column.
        let edit = click_at(420.0, RESET_W * 0.5);
        assert!(edit.reset, "a click on the reset arrow did nothing");
        assert!(!edit.changed, "resetting is not a value change");
    }

    /// And anywhere in its column, since the hit area is the whole column and
    /// a user aiming at a 12-point glyph will miss it.
    #[test]
    fn the_reset_arrow_answers_a_click_anywhere_in_its_column() {
        for offset in [2.0_f32, 6.0, 9.0, 14.0, 17.0] {
            let edit = click_at(420.0, offset);
            assert!(
                edit.reset,
                "a click {offset} points from the right edge missed the reset arrow"
            );
        }
    }

    /// The reason this exists at all: every row's track has to start and end
    /// in the same place, whatever its label says. egui's stock slider sizes
    /// its label to the text, so a panel of thirty parameters comes out as
    /// thirty tracks at thirty different offsets.
    #[test]
    fn every_row_puts_its_track_in_the_same_place() {
        let (_, track, _, _) = rects(420.0);
        let (_, track2, _, _) = rects(420.0);
        assert_eq!(track.min.x, track2.min.x);
        assert_eq!(track.max.x, track2.max.x);
    }

    #[test]
    fn the_columns_do_not_overlap_and_fill_the_row() {
        let (label, track, value, reset) = rects(420.0);
        assert!(label.max.x < track.min.x, "label runs into the track");
        assert!(track.max.x < value.min.x, "track runs into the value box");
        assert!(value.max.x <= reset.min.x, "value box runs into the reset");
        assert!(
            (reset.max.x - label.min.x - 420.0).abs() < 1.0,
            "the row does not use its width"
        );
    }

    /// The box exists to be finer than the track. If it were not, it would be
    /// a second slider in a smaller box — and the whole reason for having both
    /// is that finding a value and settling on one want different rates.
    #[test]
    fn dragging_the_number_is_finer_than_dragging_the_track() {
        let (_, track, _, _) = rects(420.0);
        // Value units per pixel, for a 0..1 parameter.
        let by_track = 1.0 / track.width();
        let by_box = 1.0 / (track.width() * FINE);
        assert!(
            by_box < by_track,
            "the box moves at {by_box} against the track's {by_track}"
        );
        assert!(
            (by_track / by_box - FINE).abs() < 1e-4,
            "the ratio drifted from the constant that documents it"
        );
    }

    /// And the ratio has to hold at any panel size, or the box would be four
    /// times finer on a wide window and barely finer on a narrow one.
    #[test]
    fn the_fine_ratio_holds_at_any_panel_width() {
        for width in [240.0, 420.0, 620.0] {
            let (_, track, _, _) = rects(width);
            let speed = 1.0 / (track.width() * FINE);
            let coarse = 1.0 / track.width();
            assert!((coarse / speed - FINE).abs() < 1e-4, "at {width} points");
        }
    }

    /// A narrow panel must not give the track a negative width, which egui
    /// turns into a rectangle that interacts everywhere on screen.
    #[test]
    fn a_narrow_panel_does_not_invert_the_track() {
        for width in [90.0, 140.0, 200.0, 260.0, 340.0] {
            let (label, track, _, _) = rects(width);
            assert!(
                track.max.x >= track.min.x,
                "the track came out inside out at {width} points wide"
            );
            assert!(label.width() > 0.0, "the label column vanished at {width}");
        }
    }

    /// And a panel wide enough gets the full label column, so the common case
    /// is not paying for the defensive one.
    #[test]
    fn a_normal_panel_gets_the_whole_label_column() {
        let (label, _, _, _) = rects(400.0);
        assert!((label.width() - LABEL_W).abs() < 0.5, "{}", label.width());
    }
}
