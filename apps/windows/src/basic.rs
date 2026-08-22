//! The histogram and the Basic panel.
//!
//! Lightroom's layout, driving pinned rows. The panel spans both working
//! spaces — Temperature and Exposure are light, Contrast and the tonal sliders
//! are perception — and a row must be one space or the other, so one panel
//! writes to seven rows. The user never sees the seam; the two-space rule
//! survives intact.
//!
//! Nothing here knows which index a row landed at. Each control names the
//! effect it drives and looks the row up, which is what keeps the panel
//! working no matter what the user does to the effects list below it.

use pe_core::{History, ParamValue};
use pe_effects::ParamKind;
use pe_scopes::{BINS, Histogram};

/// The three channel colours.
///
/// Chosen so that all three together come to white and any two come to a clean
/// secondary — yellow, cyan, magenta. That is what makes the shape readable:
/// the white core is where the channels agree, and a coloured fringe is
/// exactly where one of them has drifted away from the others.
const CHANNEL_COLOURS: [[u16; 3]; 3] = [[160, 48, 48], [48, 158, 56], [56, 68, 160]];

/// Add channel colours, saturating.
///
/// Shared with the curve editor, which draws the same histogram in a different
/// domain — one copy of the compositing means the two cannot drift into
/// disagreeing about what "red plus green" looks like.
pub fn additive_channels(channels: &[(f32, usize)]) -> egui::Color32 {
    additive(channels)
}

fn additive(channels: &[(f32, usize)]) -> egui::Color32 {
    let mut sum = [0u16; 3];
    for (_, which) in channels {
        for c in 0..3 {
            sum[c] += CHANNEL_COLOURS[*which][c];
        }
    }
    egui::Color32::from_rgb(
        sum[0].min(255) as u8,
        sum[1].min(255) as u8,
        sum[2].min(255) as u8,
    )
}

/// Draw the histogram.
///
/// Display-referred, like Lightroom's: the question it answers is "what is
/// about to clip on output", not "where did the light fall".
///
/// Composited additively, one column of the picture per bin. Additive is the
/// convention every editor uses and it is genuinely the most readable option —
/// a stack of opaque curves hides whichever channel is drawn first, and
/// alpha-blended ones muddy into brown wherever they cross. Doing it as three
/// stacked segments per bin gets the real additive result with no blending at
/// all: the shortest channel's height is where all three overlap and comes out
/// white, above it two remain, above that one.
pub fn histogram(ui: &mut egui::Ui, hist: Option<&Histogram>) {
    let height = 92.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, egui::Color32::from_gray(16));

    // Quarter-tone guides, behind the trace rather than across it.
    for q in 1..4 {
        let x = rect.min.x + rect.width() * (q as f32 / 4.0);
        painter.line_segment(
            [
                egui::pos2(x, rect.min.y + 1.0),
                egui::pos2(x, rect.max.y - 1.0),
            ],
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(38)),
        );
    }

    let Some(hist) = hist else {
        border(&painter, rect);
        return;
    };
    let peak = hist.peak().max(1) as f32;

    // A compression curve, not a straight scale. One flat area of sky can hold
    // a fifth of the frame in a single bin, and against that everything else
    // in the picture would be a pixel high.
    let scale = |v: u32| -> f32 {
        let t = (v as f32 / peak).clamp(0.0, 1.0);
        t.powf(0.45)
    };

    let mut mesh = egui::Mesh::default();
    // Half a pixel of overlap: at panel width a bin is barely more than a
    // pixel across, and exact abutment leaves seams wherever one lands on a
    // device pixel boundary.
    let step = rect.width() / BINS as f32;
    for i in 0..BINS {
        let x0 = rect.min.x + i as f32 * step;
        let x1 = (x0 + step + 0.5).min(rect.max.x);

        let mut heights = [
            (scale(hist.red[i]), 0usize),
            (scale(hist.green[i]), 1),
            (scale(hist.blue[i]), 2),
        ];
        heights.sort_by(|a, b| a.0.total_cmp(&b.0));

        let mut base = 0.0f32;
        for k in 0..3 {
            let top = heights[k].0;
            if top > base {
                mesh.add_colored_rect(
                    egui::Rect::from_min_max(
                        egui::pos2(x0, rect.max.y - top * (rect.height() - 2.0)),
                        egui::pos2(x1, rect.max.y - base * (rect.height() - 2.0)),
                    ),
                    additive(&heights[k..]),
                );
                base = top;
            }
        }
    }
    painter.add(egui::Shape::mesh(mesh));

    // Clipping, at the end it is happening. Both ends, because a crushed black
    // costs a photograph as much as a blown highlight and is far easier to
    // miss on a bright screen.
    let total = hist.total.max(1) as f32;
    let shadows = (hist.red[0] + hist.green[0] + hist.blue[0]) as f32 / (3.0 * total);
    if shadows > 0.002 {
        clip_mark(&painter, egui::pos2(rect.min.x + 7.0, rect.min.y + 7.0));
    }
    if hist.over_white_fraction() > 0.001 {
        clip_mark(&painter, egui::pos2(rect.max.x - 7.0, rect.min.y + 7.0));
    }

    border(&painter, rect);
}

fn border(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_stroke(
        rect,
        3.0,
        egui::Stroke::new(1.0_f32, egui::Color32::from_gray(52)),
        egui::StrokeKind::Inside,
    );
}

fn clip_mark(painter: &egui::Painter, at: egui::Pos2) {
    painter.circle_filled(at, 4.0, egui::Color32::from_black_alpha(170));
    painter.circle_filled(at, 2.6, egui::Color32::from_rgb(240, 200, 90));
}

/// One Lightroom-style slider: label, track, value, all driving a named
/// parameter of a named effect.
/// One float parameter of one pinned row.
///
/// Shared with the mixer and curve panels: they all want the same double-
/// click-to-neutral and the same coalescing key, and a second copy of this
/// is a second place for those to drift.
pub fn slider(
    ui: &mut egui::Ui,
    history: &mut History,
    effect: &str,
    key: &'static str,
    label: &str,
) {
    let Some(def) = pe_effects::by_key(effect).and_then(|e| e.param(key)) else {
        return;
    };
    let ParamKind::Float {
        min,
        max,
        default,
        neutral,
    } = def.kind
    else {
        return;
    };
    let Some(id) = history.document().stack.find_by_effect(effect) else {
        return;
    };

    let mut v = history
        .document()
        .stack
        .get(id)
        .and_then(|r| r.params.get(key))
        .and_then(ParamValue::as_float)
        .unwrap_or(default);

    let title = if def.unit.is_empty() {
        label.to_string()
    } else {
        format!("{label} ({})", def.unit)
    };
    let edit = crate::resolve::slider_row(
        ui,
        ui.id().with((effect, key)),
        &title,
        &mut v,
        min..=max,
        decimals(min, max),
    );
    if edit.changed {
        let coalesce = Some(format!("{effect}.{key}"));
        history.edit(label.to_string(), coalesce, |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.params.set(key, ParamValue::Float(v));
            }
        });
    }
    if edit.released {
        history.break_coalescing();
    }
    // Reset means *neutral*, not the default — for a look effect those differ,
    // and reset should always mean "do nothing".
    if edit.reset {
        history.edit(label.to_string(), None, |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.params.set(key, ParamValue::Float(neutral));
            }
        });
    }
}

/// How many decimals a range wants.
///
/// A slider running to 15000 kelvin reading "6500.000" is noise; one running
/// to 1.0 reading "0.2" has thrown away the resolution the control has.
pub fn decimals(min: f32, max: f32) -> usize {
    let span = (max - min).abs();
    if span > 100.0 {
        0
    } else if span > 10.0 {
        2
    } else {
        3
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(text).small().weak());
}

/// The Basic panel.
pub fn panel(ui: &mut egui::Ui, history: &mut History) {
    heading(ui, "White Balance");
    slider(ui, history, "white_balance", "temperature", "Temp");
    slider(ui, history, "white_balance", "tint", "Tint");

    heading(ui, "Tone");
    slider(ui, history, "exposure", "ev", "Exposure");
    slider(ui, history, "contrast", "contrast", "Contrast");
    slider(ui, history, "tone", "highlights", "Highlights");
    slider(ui, history, "tone", "shadows", "Shadows");
    slider(ui, history, "tone", "whites", "Whites");
    slider(ui, history, "tone", "blacks", "Blacks");

    heading(ui, "Presence");
    slider(ui, history, "presence", "texture", "Texture");
    slider(ui, history, "presence", "clarity", "Clarity");
    slider(ui, history, "dehaze", "strength", "Dehaze");
    slider(ui, history, "colour", "vibrance", "Vibrance");
    slider(ui, history, "colour", "saturation", "Saturation");
    ui.add_space(4.0);
}

/// Reset every parameter of the Basic panel's rows to neutral.
pub fn reset(history: &mut History) {
    let targets = [
        "white_balance",
        "exposure",
        "contrast",
        "tone",
        "presence",
        "dehaze",
        "colour",
    ];
    history.edit("Reset Basic", None, |doc| {
        for effect in targets {
            let Some(def) = pe_effects::by_key(effect) else {
                continue;
            };
            let Some(id) = doc.stack.find_by_effect(effect) else {
                continue;
            };
            let Some(row) = doc.stack.get_mut(id) else {
                continue;
            };
            for param in def.params {
                if let ParamKind::Float { neutral, .. } = param.kind {
                    row.params.set(param.key, ParamValue::Float(neutral));
                }
            }
        }
    });
}
