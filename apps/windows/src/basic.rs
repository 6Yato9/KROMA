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

/// Draw the histogram.
///
/// Display-referred, like Lightroom's: the question it answers is "what is
/// about to clip on output", not "where did the light fall". Channels are
/// drawn additively so overlapping regions go pale, which is the convention
/// every editor uses and is genuinely the most readable option — a stack of
/// opaque curves hides whichever channel is drawn first.
pub fn histogram(ui: &mut egui::Ui, hist: Option<&Histogram>) {
    let height = 84.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, egui::Color32::from_gray(18));

    let Some(hist) = hist else {
        return;
    };
    let peak = hist.peak().max(1) as f32;

    // A little headroom, and a log-ish compression so a single spike does not
    // flatten everything else into the floor.
    let scale = |v: u32| -> f32 {
        let t = (v as f32 / peak).clamp(0.0, 1.0);
        t.powf(0.45)
    };

    for (channel, colour) in [
        (&hist.red, egui::Color32::from_rgb(220, 60, 60)),
        (&hist.green, egui::Color32::from_rgb(60, 200, 90)),
        (&hist.blue, egui::Color32::from_rgb(70, 110, 240)),
    ] {
        let mut points = Vec::with_capacity(BINS + 2);
        points.push(egui::pos2(rect.min.x, rect.max.y));
        for (i, count) in channel.iter().enumerate() {
            let x = rect.min.x + rect.width() * (i as f32 / (BINS - 1) as f32);
            let y = rect.max.y - rect.height() * scale(*count);
            points.push(egui::pos2(x, y));
        }
        points.push(egui::pos2(rect.max.x, rect.max.y));
        painter.add(egui::Shape::convex_polygon(
            points,
            colour.gamma_multiply(0.42),
            egui::Stroke::NONE,
        ));
    }

    // Quarter-tone guides, so you can see where the ends actually are.
    for q in 1..4 {
        let x = rect.min.x + rect.width() * (q as f32 / 4.0);
        painter.line_segment(
            [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(48)),
        );
    }

    if hist.over_white_fraction() > 0.001 {
        painter.circle_filled(
            egui::pos2(rect.max.x - 6.0, rect.min.y + 6.0),
            3.5,
            egui::Color32::from_rgb(240, 200, 90),
        );
    }
}

/// One Lightroom-style slider: label, track, value, all driving a named
/// parameter of a named effect.
fn slider(ui: &mut egui::Ui, history: &mut History, effect: &str, key: &'static str, label: &str) {
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

    ui.horizontal(|ui| {
        ui.add_sized(
            [76.0, 18.0],
            egui::Label::new(egui::RichText::new(label).small()),
        );
        let r = ui.add(
            egui::Slider::new(&mut v, min..=max)
                .show_value(false)
                .handle_shape(egui::style::HandleShape::Circle),
        );
        if r.changed() {
            let coalesce = Some(format!("{effect}.{key}"));
            history.edit(label.to_string(), coalesce, |doc| {
                if let Some(row) = doc.stack.get_mut(id) {
                    row.params.set(key, ParamValue::Float(v));
                }
            });
        }
        if r.drag_stopped() {
            history.break_coalescing();
        }
        // Double-click resets to *neutral*, not to the default — for a look
        // effect those differ, and "reset" should always mean "do nothing".
        if r.double_clicked() {
            history.edit(label.to_string(), None, |doc| {
                if let Some(row) = doc.stack.get_mut(id) {
                    row.params.set(key, ParamValue::Float(neutral));
                }
            });
        }
        ui.add_sized(
            [46.0, 18.0],
            egui::Label::new(
                egui::RichText::new(format_value(v, def.unit))
                    .small()
                    .monospace(),
            ),
        );
    });
}

fn format_value(v: f32, unit: &str) -> String {
    if unit == "K" {
        format!("{v:.0}")
    } else if v.abs() >= 100.0 {
        format!("{v:.0}{unit}")
    } else {
        format!("{v:+.2}{unit}")
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
