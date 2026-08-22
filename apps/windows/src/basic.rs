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
/// The same row, for a parameter on a row we already have the id of.
///
/// `slider` finds its row by effect key, which is what a pinned panel wants —
/// there is exactly one White Balance. The Colour Warper is an ordinary
/// effect, so there can be several, and the panel has to say *which*.
pub fn slider_of(
    ui: &mut egui::Ui,
    history: &mut History,
    id: pe_core::RowId,
    effect: &str,
    key: &'static str,
) {
    let Some(def) = pe_effects::by_key(effect).and_then(|e| e.param(key)) else {
        return;
    };
    crate::inspector::param_ui(ui, history, id, def, ui.id().with(("warper", key)));
}

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
    let edit = crate::resolve::slider_row_styled(
        ui,
        ui.id().with((effect, key)),
        &title,
        &mut v,
        min..=max,
        decimals(min, max),
        crate::resolve::TrackStyle::of(effect, key, min, max, neutral),
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
    // Reset puts the parameter back to its **default** — the value the effect
    // arrives with — not to its neutral.
    //
    // Those differ for a look effect, and this used to choose neutral on the
    // argument that a reset should always mean "do nothing". That argument was
    // wrong for one decisive reason: the reset arrow on the effect's own title
    // bar already restored defaults, so the same icon meant two things
    // depending on which row of the panel it sat in. Resolve's means default
    // everywhere, and one meaning beats a defensible second one.
    //
    // `neutral` is still what the slider draws its fill from, which is the
    // question it actually answers: where does this control stop doing
    // anything.
    if edit.reset {
        history.edit(label.to_string(), None, |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.params.set(key, ParamValue::Float(default));
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
    slider(ui, history, "contrast", "pivot", "Pivot");
    slider(ui, history, "tone", "highlights", "Highlights");
    slider(ui, history, "tone", "shadows", "Shadows");
    slider(ui, history, "tone", "whites", "Whites");
    slider(ui, history, "tone", "blacks", "Blacks");

    heading(ui, "Presence");
    slider(ui, history, "presence", "texture", "Texture");
    slider(ui, history, "presence", "clarity", "Clarity");
    slider(ui, history, "colour", "vibrance", "Vibrance");
    slider(ui, history, "colour", "saturation", "Saturation");
    // Moved here from the wheels panel, where they were the only two
    // controls in that row Basic did not already have. Two panels showing
    // the same parameter is two places to look for it, one of which is
    // always the wrong one.
    slider(ui, history, "colour", "hue", "Hue");
    slider(ui, history, "colour", "lum_mix", "Lum Mix");
    ui.add_space(4.0);
}

/// Reset every parameter of the Basic panel's rows to its default.
///
/// The same meaning as every other reset in the application. For these rows
/// the two coincide — a corrective panel arrives doing nothing — but saying
/// "default" here keeps the one rule readable rather than leaving a reader to
/// work out whether this one is an exception.
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
                if let ParamKind::Float { default, .. } = param.kind {
                    row.params.set(param.key, ParamValue::Float(default));
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use pe_core::Document;

    fn pushed(effect: &str, key: &str, to: f32) -> (History, pe_core::RowId) {
        let doc: Document = pe_effects::new_document("photo.jpg");
        let id = doc.stack.find_by_effect(effect).expect("a pinned row");
        let mut history = History::new(doc);
        history.edit("push", None, |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.params.set(key, ParamValue::Float(to));
            }
        });
        (history, id)
    }

    fn value(history: &History, id: pe_core::RowId, key: &str) -> f32 {
        history
            .document()
            .stack
            .get(id)
            .and_then(|r| r.params.get(key))
            .and_then(ParamValue::as_float)
            .expect("set")
    }

    /// Click the reset arrow on a real Basic row and see what the document
    /// says afterwards.
    ///
    /// Driven as a click rather than by calling the handler, because the two
    /// failures this is guarding against are different and only one of them
    /// is visible from the code: a caller that drops `Edit::reset` — which is
    /// what the Curves panel did for months — and a row whose arrow is not
    /// where the click lands.
    ///
    /// Two frames, because egui hit-tests against the rectangles the previous
    /// frame registered.
    #[test]
    fn the_reset_arrow_on_a_basic_row_puts_the_value_back() {
        let (mut history, id) = pushed("exposure", "ev", 1.75);
        assert_eq!(value(&history, id, "ev"), 1.75);

        let ctx = egui::Context::default();
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(420.0, 300.0));
        let frame = |input: egui::RawInput, history: &mut History| {
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
                    slider(&mut child, history, "exposure", "ev", "Exposure");
                });
            });
        };

        let base = egui::RawInput {
            screen_rect: Some(area),
            ..Default::default()
        };
        frame(base.clone(), &mut history);

        // The reset arrow sits at the end of the row, nine points in from the
        // right edge — the centre of its column.
        let at = egui::pos2(area.max.x - 9.0, area.min.y + 11.0);
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
        frame(input, &mut history);

        assert_eq!(
            value(&history, id, "ev"),
            0.0,
            "the reset arrow left the value where it was"
        );
    }
}
