//! The colour mixer panel.
//!
//! Eight hue bands, three sliders each — twenty-four controls, which is far
//! too many to put on screen at once. Lightroom's answer is to show one band
//! at a time, and it is the right one: the mixer is used to fix a particular
//! colour, so the user already knows which band they want before they open the
//! panel.
//!
//! The band picker is drawn as swatches rather than written as names because
//! "aqua" and "blue" are much easier to tell apart as colours than as words,
//! and picking the wrong one is the mistake this panel invites.

use crate::basic;
use pe_core::History;

/// The bands, in the order the shader indexes them. Each carries the key
/// prefix, the label, and the swatch colour.
///
/// The swatches are drawn at the band's own centre hue, so the picker is a
/// picture of what the shader is doing rather than a decorative palette.
const BANDS: [Band; 8] = [
    band(
        "Red",
        214,
        69,
        69,
        ["red_hue", "red_saturation", "red_luminance"],
    ),
    band(
        "Orange",
        214,
        134,
        55,
        ["orange_hue", "orange_saturation", "orange_luminance"],
    ),
    band(
        "Yellow",
        200,
        186,
        60,
        ["yellow_hue", "yellow_saturation", "yellow_luminance"],
    ),
    band(
        "Green",
        74,
        170,
        88,
        ["green_hue", "green_saturation", "green_luminance"],
    ),
    band(
        "Aqua",
        62,
        172,
        180,
        ["aqua_hue", "aqua_saturation", "aqua_luminance"],
    ),
    band(
        "Blue",
        72,
        118,
        206,
        ["blue_hue", "blue_saturation", "blue_luminance"],
    ),
    band(
        "Purple",
        133,
        88,
        202,
        ["purple_hue", "purple_saturation", "purple_luminance"],
    ),
    band(
        "Magenta",
        200,
        72,
        152,
        ["magenta_hue", "magenta_saturation", "magenta_luminance"],
    ),
];

struct Band {
    label: &'static str,
    swatch: egui::Color32,
    /// Hue, saturation and luminance, spelled out rather than built from a
    /// prefix at draw time — these are needed as `&'static str` and building
    /// them per frame would mean either an allocation or a leak.
    keys: [&'static str; 3],
}

const fn band(label: &'static str, r: u8, g: u8, b: u8, keys: [&'static str; 3]) -> Band {
    Band {
        label,
        swatch: egui::Color32::from_rgb(r, g, b),
        keys,
    }
}

/// What each band's three sliders are called, in key order.
const CONTROLS: [&str; 3] = ["Hue", "Saturation", "Luminance"];

pub fn panel(ui: &mut egui::Ui, history: &mut History) {
    // A document saved before the mixer existed simply has no such row, and a
    // panel with nowhere to write is better absent than broken.
    if history
        .document()
        .stack
        .find_by_effect("colour_mixer")
        .is_none()
    {
        return;
    }

    let band_id = ui.make_persistent_id("mixer_band");
    let mut band: usize = ui.data_mut(|d| *d.get_temp_mut_or(band_id, 0usize));

    ui.horizontal(|ui| {
        for (i, b) in BANDS.iter().enumerate() {
            if swatch(ui, b.swatch, band == i, touched(history, i)).clicked() {
                band = i;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(band_id, band));

    ui.add_space(4.0);
    ui.label(egui::RichText::new(BANDS[band].label).small().strong());

    for (key, label) in BANDS[band].keys.iter().zip(CONTROLS) {
        basic::slider(ui, history, "colour_mixer", key, label);
    }

    ui.add_space(2.0);
    ui.horizontal(|ui| {
        if ui.small_button("Reset band").clicked() {
            reset(history, Some(band));
        }
        if ui.small_button("Reset all").clicked() {
            reset(history, None);
        }
    });
}

/// Whether any of a band's three sliders has been moved.
///
/// Worth a dot on the swatch: with one band visible at a time, work done in
/// another band is otherwise invisible, and an unexplained colour shift is a
/// long hunt.
fn touched(history: &History, band: usize) -> bool {
    let Some(id) = history.document().stack.find_by_effect("colour_mixer") else {
        return false;
    };
    let Some(row) = history.document().stack.get(id) else {
        return false;
    };
    BANDS[band].keys.iter().any(|key| {
        row.params
            .get(key)
            .and_then(pe_core::ParamValue::as_float)
            .is_some_and(|v| v != 0.0)
    })
}

fn swatch(
    ui: &mut egui::Ui,
    colour: egui::Color32,
    selected: bool,
    touched: bool,
) -> egui::Response {
    let size = egui::vec2(22.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let painter = ui.painter_at(rect);
    let body = rect.shrink(2.0);
    painter.rect_filled(body, 3.0, colour);
    if selected {
        painter.rect_stroke(
            rect.shrink(0.5),
            4.0,
            egui::Stroke::new(1.6_f32, egui::Color32::from_gray(235)),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_stroke(
            rect.shrink(0.5),
            4.0,
            egui::Stroke::new(1.0_f32, egui::Color32::from_gray(140)),
            egui::StrokeKind::Inside,
        );
    }
    if touched {
        painter.circle_filled(
            egui::pos2(body.max.x - 2.5, body.min.y + 2.5),
            2.2,
            egui::Color32::from_gray(20),
        );
        painter.circle_filled(
            egui::pos2(body.max.x - 2.5, body.min.y + 2.5),
            1.4,
            egui::Color32::WHITE,
        );
    }
    response
}

fn reset(history: &mut History, band: Option<usize>) {
    let Some(id) = history.document().stack.find_by_effect("colour_mixer") else {
        return;
    };
    let label = match band {
        Some(b) => format!("Reset {}", BANDS[b].label),
        None => "Reset Colour Mixer".to_string(),
    };
    history.edit(label, None, |doc| {
        let Some(row) = doc.stack.get_mut(id) else {
            return;
        };
        let bands: &[Band] = match band {
            Some(b) => &BANDS[b..=b],
            None => &BANDS,
        };
        for b in bands {
            for key in b.keys {
                row.params.set(key, pe_core::ParamValue::Float(0.0));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panel builds parameter keys by joining a band prefix to a control
    /// suffix. If the registry ever disagrees, the sliders silently do nothing
    /// — they look the parameter up and give up when it is missing.
    #[test]
    fn every_key_the_panel_builds_exists_on_the_effect() {
        let def = pe_effects::by_key("colour_mixer").expect("registered");
        for b in &BANDS {
            for key in b.keys {
                assert!(def.param(key).is_some(), "no parameter {key}");
            }
        }
        assert_eq!(
            def.params.len(),
            BANDS.len() * CONTROLS.len(),
            "the panel should cover every parameter the effect has"
        );
    }

    /// The shader reads the bands by arithmetic on the slot index, so the
    /// registry's order is what decides which slider drives which hue.
    #[test]
    fn the_panel_lists_the_bands_in_the_order_the_shader_indexes_them() {
        let def = pe_effects::by_key("colour_mixer").expect("registered");
        for (i, b) in BANDS.iter().enumerate() {
            assert_eq!(def.params[i * 3].key, b.keys[0], "band {i} is out of order");
        }
    }
}
