//! The scopes panel: waveform, parade and vectorscope.
//!
//! `pe-scopes` produces counts; everything here is the reading of them. That
//! split matters more than it looks, because almost all of the judgement in a
//! scope is in the display: how a cell's count becomes a brightness, what the
//! graticule says, where the reference lines sit. None of that is arithmetic
//! anyone can check without looking at it, so it lives away from the part that
//! can be tested.
//!
//! The grids are uploaded as textures rather than drawn as shapes. A waveform
//! is 320 columns by 256 levels; as egui geometry that is eighty thousand
//! quads a frame, and as a texture it is one.

use pe_scopes::{Channel, LEVELS, VECTOR_SIZE, Vectorscope, Waveform};

use crate::preview::Scopes;

/// Which scopes are on screen. Several at once, like Resolve — the whole point
/// is reading one against another.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    pub waveform: bool,
    /// Whether the waveform overlays the three channels rather than showing
    /// luma alone.
    ///
    /// Both readings are worth having and neither replaces the other: luma is
    /// what you set exposure against, and the overlay is where a cast shows up
    /// as the channels pulling apart at one end of the range.
    pub waveform_rgb: bool,
    pub parade: bool,
    pub vectorscope: bool,
}

impl Default for Shown {
    fn default() -> Self {
        // A waveform on its own is the one most people want first: it is the
        // only scope that says *where* in the frame something is happening.
        Self {
            waveform: true,
            waveform_rgb: false,
            parade: false,
            vectorscope: false,
        }
    }
}

impl Shown {
    pub fn any(&self) -> bool {
        self.waveform || self.parade || self.vectorscope
    }

    fn count(&self) -> usize {
        [self.waveform, self.parade, self.vectorscope]
            .iter()
            .filter(|v| **v)
            .count()
    }
}

/// The uploaded grids, kept between frames.
///
/// Textures are re-uploaded only when the measurement behind them changes,
/// which during an idle frame is never and during a slider drag is once. The
/// generation counter is what makes that check a comparison rather than a
/// pixel diff.
#[derive(Default)]
pub struct Textures {
    waveform: Option<egui::TextureHandle>,
    parade: Option<egui::TextureHandle>,
    vectorscope: Option<egui::TextureHandle>,
    generation: u64,
    /// What the uploaded grids were drawn for. Switching the waveform between
    /// luma and RGB changes the picture without changing the measurement, so
    /// the generation alone is not enough to know when to redraw.
    shown: Option<Shown>,
}

/// The panel. Draws whichever scopes are selected, side by side.
pub fn panel(ui: &mut egui::Ui, textures: &mut Textures, scopes: Option<&Scopes>, shown: &Shown) {
    let Some(scopes) = scopes else {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new("no measurement yet").weak().small());
        });
        return;
    };

    if !shown.any() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new("nothing selected").weak().small());
        });
        return;
    }

    if textures.generation != scopes.generation || textures.shown != Some(*shown) {
        textures.generation = scopes.generation;
        textures.shown = Some(*shown);
        upload(ui.ctx(), textures, scopes, shown);
    }

    let count = shown.count().max(1);
    let gap = 6.0;
    let width = ((ui.available_width() - gap * (count as f32 - 1.0)) / count as f32).max(60.0);
    // All of it. The panel claims any remainder itself, so there is no reason
    // to leave a margin here — and leaving one is what used to walk the panel
    // down to its minimum a few points per frame.
    let height = ui.available_height().max(60.0);

    ui.horizontal(|ui| {
        if shown.waveform {
            let title = if shown.waveform_rgb {
                "Waveform · RGB"
            } else {
                "Waveform · Luma"
            };
            frame(ui, title, width, height, |ui, rect| {
                if let Some(t) = &textures.waveform {
                    image(ui, rect, t);
                }
                levels(ui, rect);
            });
        }
        if shown.parade {
            frame(ui, "Parade", width, height, |ui, rect| {
                if let Some(t) = &textures.parade {
                    image(ui, rect, t);
                }
                levels(ui, rect);
                // The seams between the three panels, so it reads as three
                // scopes rather than one wide one.
                for i in 1..3 {
                    let x = rect.min.x + rect.width() * i as f32 / 3.0;
                    ui.painter().line_segment(
                        [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                        egui::Stroke::new(1.0_f32, crate::theme::colour::GRID),
                    );
                }
            });
        }
        if shown.vectorscope {
            // Square, or the hue circle would be an ellipse and the graticule
            // boxes would stop meaning anything.
            let side = width.min(height);
            frame(ui, "Vectorscope", side, height, |ui, rect| {
                let square = egui::Rect::from_center_size(
                    rect.center(),
                    egui::Vec2::splat(rect.width().min(rect.height())),
                );
                if let Some(t) = &textures.vectorscope {
                    image(ui, square, t);
                }
                graticule(ui, square);
            });
        }
    });
}

/// A titled, sunken box for one scope.
fn frame(
    ui: &mut egui::Ui,
    title: &str,
    width: f32,
    height: f32,
    body: impl FnOnce(&egui::Ui, egui::Rect),
) {
    ui.allocate_ui(egui::vec2(width, height), |ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(title).small().weak());
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(width, (height - 20.0).max(40.0)),
                egui::Sense::hover(),
            );
            if !ui.is_rect_visible(rect) {
                return;
            }
            ui.painter()
                .rect_filled(rect, 3.0, crate::theme::colour::WELL);
            body(ui, rect);
            ui.painter().rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(1.0_f32, egui::Color32::from_gray(46)),
                egui::StrokeKind::Inside,
            );
        });
    });
}

fn image(ui: &egui::Ui, rect: egui::Rect, texture: &egui::TextureHandle) {
    ui.painter().add(egui::Shape::image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    ));
}

/// The horizontal reference lines a waveform is read against: black, quarters,
/// and white.
fn levels(ui: &egui::Ui, rect: egui::Rect) {
    let painter = ui.painter_at(rect);
    for (t, weight) in [(0.0, 2u8), (0.25, 1), (0.5, 1), (0.75, 1), (1.0, 2)] {
        let y = rect.max.y - t * rect.height();
        painter.line_segment(
            [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
            egui::Stroke::new(
                1.0_f32,
                egui::Color32::from_white_alpha(if weight == 2 { 60 } else { 26 }),
            ),
        );
    }
}

/// The colour bar boxes and the skin line.
///
/// Drawn by running the same projection the pixels went through, so a box can
/// never end up somewhere the pixels cannot reach.
fn graticule(ui: &egui::Ui, rect: egui::Rect) {
    let painter = ui.painter_at(rect);
    let centre = rect.center();
    let radius = rect.width() * 0.5;
    let place = |p: [f32; 2]| egui::pos2(centre.x + p[0] * radius, centre.y - p[1] * radius);

    painter.circle_stroke(
        centre,
        radius * 0.75,
        egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(30)),
    );
    for (dx, dy) in [(1.0, 0.0), (0.0, 1.0)] {
        painter.line_segment(
            [
                egui::pos2(centre.x - dx * radius, centre.y - dy * radius),
                egui::pos2(centre.x + dx * radius, centre.y + dy * radius),
            ],
            egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(22)),
        );
    }

    // The skin line. One sample is enough because skin of every shade points
    // the same way out of the middle — it is how far out and how bright that
    // varies, not the direction.
    let skin = pe_scopes::waveform::position(pe_scopes::SKIN);
    let len = (skin[0] * skin[0] + skin[1] * skin[1]).sqrt().max(1e-4);
    painter.line_segment(
        [centre, place([skin[0] / len * 0.95, skin[1] / len * 0.95])],
        egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_rgba_unmultiplied(255, 190, 150, 90),
        ),
    );

    for (name, rgb) in pe_scopes::TARGETS {
        let at = place(pe_scopes::waveform::position(rgb));
        painter.rect_stroke(
            egui::Rect::from_center_size(at, egui::Vec2::splat(9.0)),
            1.0,
            egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(90)),
            egui::StrokeKind::Middle,
        );
        painter.text(
            at + egui::vec2(9.0, -9.0),
            egui::Align2::LEFT_BOTTOM,
            name,
            egui::FontId::proportional(9.0),
            egui::Color32::from_white_alpha(110),
        );
    }
}

// ---------------------------------------------------------------------------
// Turning counts into pixels
// ---------------------------------------------------------------------------

/// How a cell's count becomes a brightness.
///
/// The count in a cell is bounded by the number of image rows in that column,
/// so that is the natural full scale — and unlike the observed peak it does
/// not move as the picture is graded, so the display does not flicker under
/// the user's hand.
///
/// The square root is the part that makes it readable. A flat sky puts a whole
/// column in one cell and a gradient spreads it over two hundred; on a linear
/// scale the gradient is one two-hundredth as bright as the sky, which is to
/// say invisible. Every hardware scope applies a curve here for exactly this
/// reason.
fn intensity(count: u32, rows: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let f = count as f32 / rows.max(1) as f32;
    (f.sqrt() * 1.7).clamp(0.06, 1.0)
}

fn upload(ctx: &egui::Context, textures: &mut Textures, scopes: &Scopes, shown: &Shown) {
    let options = egui::TextureOptions::LINEAR;
    if shown.waveform {
        let channels: &[Channel] = if shown.waveform_rgb {
            &[Channel::Red, Channel::Green, Channel::Blue]
        } else {
            &[Channel::Luma]
        };
        let img = waveform_image(&scopes.waveform, channels);
        set(ctx, &mut textures.waveform, "scope-waveform", img, options);
    }
    if shown.parade {
        let img = parade_image(&scopes.waveform);
        set(ctx, &mut textures.parade, "scope-parade", img, options);
    }
    if shown.vectorscope {
        let img = vectorscope_image(&scopes.vectorscope);
        set(
            ctx,
            &mut textures.vectorscope,
            "scope-vectorscope",
            img,
            options,
        );
    }
}

fn set(
    ctx: &egui::Context,
    slot: &mut Option<egui::TextureHandle>,
    name: &str,
    image: egui::ColorImage,
    options: egui::TextureOptions,
) {
    match slot {
        // In place, so a slider drag does not allocate and free a texture
        // sixty times a second.
        Some(handle) => handle.set(image, options),
        None => *slot = Some(ctx.load_texture(name, image, options)),
    }
}

/// The tint each channel is drawn in. Additive, so where all three overlap the
/// result goes pale — the same convention as the histogram, and the only one
/// that does not hide whichever channel happens to be drawn first.
fn tint(channel: Channel) -> [f32; 3] {
    match channel {
        Channel::Red => [1.0, 0.18, 0.18],
        Channel::Green => [0.25, 1.0, 0.35],
        Channel::Blue => [0.35, 0.5, 1.0],
        Channel::Luma => [0.82, 0.88, 0.95],
    }
}

fn waveform_image(w: &Waveform, channels: &[Channel]) -> egui::ColorImage {
    let (cols, rows) = (w.columns(), w.rows());
    let mut px = vec![egui::Color32::TRANSPARENT; cols * LEVELS];
    for channel in channels {
        let tint = tint(*channel);
        let grid = w.channel(*channel);
        for column in 0..cols {
            for level in 0..LEVELS {
                let i = intensity(grid[column * LEVELS + level], rows);
                if i <= 0.0 {
                    continue;
                }
                // Level 0 is black, which belongs at the bottom of the plot.
                let out = &mut px[(LEVELS - 1 - level) * cols + column];
                *out = add(*out, tint, i);
            }
        }
    }
    egui::ColorImage {
        size: [cols, LEVELS],
        pixels: px,
        source_size: egui::vec2(cols as f32, LEVELS as f32),
    }
}

/// Three waveforms side by side. The same counts as [`waveform_image`]; what
/// changes is that the channels are laid out rather than overlaid, which is
/// the reading you want when you are chasing a cast rather than exposure.
fn parade_image(w: &Waveform) -> egui::ColorImage {
    let (cols, rows) = (w.columns(), w.rows());
    let total = cols * 3;
    let mut px = vec![egui::Color32::TRANSPARENT; total * LEVELS];
    for (panel, channel) in [Channel::Red, Channel::Green, Channel::Blue]
        .iter()
        .enumerate()
    {
        let tint = tint(*channel);
        let grid = w.channel(*channel);
        for column in 0..cols {
            for level in 0..LEVELS {
                let i = intensity(grid[column * LEVELS + level], rows);
                if i <= 0.0 {
                    continue;
                }
                let x = panel * cols + column;
                let out = &mut px[(LEVELS - 1 - level) * total + x];
                *out = add(*out, tint, i);
            }
        }
    }
    egui::ColorImage {
        size: [total, LEVELS],
        pixels: px,
        source_size: egui::vec2(total as f32, LEVELS as f32),
    }
}

fn vectorscope_image(v: &Vectorscope) -> egui::ColorImage {
    let peak = v.peak().max(1) as f32;
    let mut px = vec![egui::Color32::TRANSPARENT; VECTOR_SIZE * VECTOR_SIZE];
    for (i, count) in v.bins().iter().enumerate() {
        if *count == 0 {
            continue;
        }
        // Against the peak, because unlike a waveform there is no natural
        // ceiling: a flat frame lands entirely in one cell and a rainbow
        // spreads over thousands.
        let f = ((*count as f32 / peak).powf(0.4) * 1.1).clamp(0.1, 1.0);
        px[i] = add(egui::Color32::TRANSPARENT, [0.55, 1.0, 0.7], f);
    }
    egui::ColorImage {
        size: [VECTOR_SIZE, VECTOR_SIZE],
        pixels: px,
        source_size: egui::vec2(VECTOR_SIZE as f32, VECTOR_SIZE as f32),
    }
}

/// Additive compositing, saturating.
fn add(base: egui::Color32, tint: [f32; 3], amount: f32) -> egui::Color32 {
    let mix = |a: u8, t: f32| ((a as f32 + t * amount * 255.0).min(255.0)) as u8;
    egui::Color32::from_rgba_premultiplied(
        mix(base.r(), tint[0]),
        mix(base.g(), tint[1]),
        mix(base.b(), tint[2]),
        255,
    )
}

#[cfg(test)]
mod tests {

    /// egui stores a resizable panel's *content* rect, not the rect it was
    /// dragged to. A panel whose content is shorter than the drag therefore
    /// springs back to the content's height on the very next frame, and the
    /// resize handle looks broken.
    ///
    /// This pins the mechanism, because the fix lives on our side — the
    /// content has to fill the panel — and a reader who does not know that
    /// would reasonably go looking in egui.
    #[test]
    fn a_bottom_panel_keeps_the_height_its_content_asks_for() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let input = || egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };

        // A panel whose content asks for only a little.
        let mut short = 0.0;
        for _ in 0..3 {
            let _ = ctx.run(input(), |ctx| {
                let r = egui::TopBottomPanel::bottom("short")
                    .resizable(true)
                    .default_height(300.0)
                    .height_range(20.0..=500.0)
                    .show(ctx, |ui| {
                        ui.allocate_exact_size(egui::vec2(10.0, 40.0), egui::Sense::hover());
                    });
                short = r.response.rect.height();
            });
        }

        // And one whose content fills whatever it is given.
        let ctx2 = egui::Context::default();
        let mut full = 0.0;
        for _ in 0..3 {
            let _ = ctx2.run(input(), |ctx| {
                let r = egui::TopBottomPanel::bottom("full")
                    .resizable(true)
                    .default_height(300.0)
                    .height_range(20.0..=500.0)
                    .show(ctx, |ui| {
                        let h = ui.available_height();
                        ui.allocate_exact_size(egui::vec2(10.0, h), egui::Sense::hover());
                    });
                full = r.response.rect.height();
            });
        }

        assert!(
            full > short + 100.0,
            "a panel holds the height its content asks for: short {short}, full {full}"
        );
        assert!(
            full > 250.0,
            "the filling panel should have kept its default height, got {full}"
        );
    }

    /// And the failure is a *creep*, not a one-off.
    ///
    /// Content a few points shorter than its panel makes the panel a few
    /// points shorter next frame, which makes the content shorter again. Over
    /// a second of frames it walks the whole thing down to its minimum — which
    /// is what "the scopes are crushed at the bottom and spring back when I
    /// drag them up" is, seen from the inside.
    #[test]
    fn content_a_little_short_of_its_panel_walks_the_panel_down() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let mut height = 0.0;
        for _ in 0..30 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    let r = egui::TopBottomPanel::bottom("creep")
                        .resizable(true)
                        .default_height(300.0)
                        .height_range(60.0..=500.0)
                        .show(ctx, |ui| {
                            // Four points shy, which is all it takes.
                            let h = (ui.available_height() - 4.0).max(10.0);
                            ui.allocate_exact_size(egui::vec2(10.0, h), egui::Sense::hover());
                        });
                    height = r.response.rect.height();
                },
            );
        }
        assert!(
            height < 200.0,
            "the panel should have collapsed, but held {height} — if this now              passes, egui changed and the fix in main.rs can be revisited"
        );
    }
    use super::*;

    /// The curve exists so that a spread-out trace is still visible next to a
    /// concentrated one. If it were linear a gradient would be two hundred
    /// times dimmer than a flat area and read as empty.
    #[test]
    fn a_thinly_spread_trace_is_still_visible() {
        let flat = intensity(240, 240);
        let spread = intensity(1, 240);
        assert!(flat > 0.9, "a full column should be near full brightness");
        assert!(
            spread > 0.05,
            "a single pixel came out at {spread}, which is invisible"
        );
        assert!(spread < flat, "and it should still be dimmer");
    }

    #[test]
    fn an_empty_cell_draws_nothing() {
        assert_eq!(intensity(0, 240), 0.0);
    }

    /// The brightness scale must not depend on what is in the picture, or the
    /// whole scope would shift under the user's hand as they graded.
    #[test]
    fn the_brightness_scale_does_not_move_with_the_content() {
        // Same fraction of the column, same brightness, whatever else the
        // frame happens to contain.
        assert!((intensity(120, 240) - intensity(60, 120)).abs() < 1e-6);
    }

    #[test]
    fn overlapping_channels_go_pale_rather_than_hiding_each_other() {
        let red = add(egui::Color32::TRANSPARENT, tint(Channel::Red), 1.0);
        let both = add(red, tint(Channel::Green), 1.0);
        assert!(
            both.g() > red.g() && both.r() >= red.r(),
            "green did not add to red: {red:?} -> {both:?}"
        );
    }

    #[test]
    fn a_waveform_image_is_as_wide_as_the_frame_and_as_tall_as_the_levels() {
        let px: Vec<u8> = std::iter::repeat_n([90u8, 90, 90, 255], 32 * 8)
            .flatten()
            .collect();
        let w = Waveform::from_display(&px, 32, 8);
        let img = waveform_image(&w, &[Channel::Luma]);
        assert_eq!(img.size, [32, LEVELS]);
    }

    /// Level zero is black and belongs at the bottom of the plot. Getting this
    /// upside down is the single easiest mistake to make here and the hardest
    /// to notice on a symmetric test image.
    #[test]
    fn dark_pixels_draw_at_the_bottom_of_the_waveform() {
        let px: Vec<u8> = std::iter::repeat_n([8u8, 8, 8, 255], 16 * 4)
            .flatten()
            .collect();
        let w = Waveform::from_display(&px, 16, 4);
        let img = waveform_image(&w, &[Channel::Luma]);
        // Level 8 belongs eight rows up from the bottom, not eight down from
        // the top.
        let lit = img.pixels[(LEVELS - 1 - 8) * 16];
        let mirrored = img.pixels[8 * 16];
        assert!(lit.a() > 0, "level 8 drew nothing at all");
        assert_eq!(
            mirrored.a(),
            0,
            "the waveform is upside down — level 8 drew near the top"
        );
    }

    #[test]
    fn a_parade_is_three_panels_wide() {
        let px: Vec<u8> = std::iter::repeat_n([90u8, 90, 90, 255], 32 * 8)
            .flatten()
            .collect();
        let w = Waveform::from_display(&px, 32, 8);
        assert_eq!(parade_image(&w).size, [96, LEVELS]);
    }

    #[test]
    fn the_panel_offers_something_by_default() {
        assert!(Shown::default().any());
    }
}
