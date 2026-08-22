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
