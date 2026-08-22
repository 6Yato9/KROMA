//! The Open FX list.
//!
//! Resolve's inspector rather than its node graph: an ordered list of effects,
//! each with an enable toggle, a title bar that expands it, reorder arrows, a
//! bin, and a reset. The rows themselves are drawn by [`crate::resolve`], so
//! every parameter in the application lines up in the same columns.
//!
//! Reordering is arrows, not drag-and-drop. Drag-to-reorder is the fiddliest
//! interaction in the whole application — hit testing, an insertion indicator,
//! autoscroll, a touch story for the tablet later — and it is not what makes
//! the panel usable.

use pe_core::{BlendMode, Curve, History, ParamValue, RowId, RowIdGenerator, StackRow, Wheel};
use pe_effects::{EffectDef, Group, ParamDef, ParamKind};

use crate::resolve::{self, Edit};

pub fn show(ui: &mut egui::Ui, history: &mut History, ids: &mut RowIdGenerator) {
    ui.add_space(4.0);
    add_effect_menu(ui, history, ids);
    ui.add_space(6.0);

    // Only the rows the user added. The pinned panels are drawn by whichever
    // panel owns them.
    let rows: Vec<(RowId, String)> = history
        .document()
        .stack
        .iter()
        .filter(|r| !r.pinned)
        .map(|r| (r.id, r.effect.clone()))
        .collect();

    if rows.is_empty() {
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("No effects yet").weak().small());
        });
        return;
    }

    // Indices here are positions among the *user* rows, so the arrows read
    // naturally; `reorder` maps them back past the pinned floor.
    let floor = history.document().stack.pinned_count();
    let count = rows.len();
    for (index, (id, effect_key)) in rows.into_iter().enumerate() {
        let Some(def) = pe_effects::by_key(&effect_key) else {
            unknown_row(ui, history, id, &effect_key);
            continue;
        };
        row_ui(ui, history, id, def, index, count, floor);
    }
}

fn add_effect_menu(ui: &mut egui::Ui, history: &mut History, ids: &mut RowIdGenerator) {
    ui.menu_button("+  Add effect", |ui| {
        ui.set_min_width(180.0);
        // The menu opens from a button near the bottom of a scrolled panel, so
        // there is often less room below it than the list needs. Without this
        // the popup is simply clipped — no scrollbar, no indication that
        // anything is missing, and half the effects unreachable.
        egui::ScrollArea::vertical()
            .max_height(340.0)
            .show(ui, |ui| {
                let mut first = true;
                for group in [Group::Basic, Group::Color, Group::Film, Group::Optics] {
                    // Effects that already exist as fixed panels are not
                    // offered again — adding a second Exposure row is
                    // legitimate, but the menu is not where anyone would look
                    // for it.
                    let available: Vec<_> = pe_effects::all()
                        .iter()
                        .filter(|e| e.group == group)
                        .filter(|e| !pe_effects::registry::PINNED_ROWS.contains(&e.key))
                        .collect();
                    // Every Basic effect is a pinned panel, so that heading has
                    // nothing under it. A heading over nothing reads as a list
                    // that failed to load.
                    if available.is_empty() {
                        continue;
                    }
                    if !first {
                        ui.separator();
                    }
                    first = false;

                    ui.label(egui::RichText::new(group.as_str()).small().weak());
                    for def in available {
                        if ui.button(def.name).clicked() {
                            let id = ids.allocate();
                            history.edit(format!("Add {}", def.name), None, |doc| {
                                let mut row = StackRow::new(id, def.key);
                                row.params = def.default_params();
                                doc.stack.push(row);
                            });
                            // Open it. You added it to change something, and
                            // a row that arrives shut costs a click to say so.
                            let open = ui.make_persistent_id(("fx", id.0)).with("open");
                            ui.data_mut(|d| d.insert_temp(open, true));
                            ui.close();
                        }
                    }
                }
            });
    });
}

fn unknown_row(ui: &mut egui::Ui, history: &mut History, id: RowId, key: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("unknown: {key}")).weak());
        if ui.small_button("Remove").clicked() {
            history.edit("Delete row", None, |doc| {
                doc.stack.remove(id);
            });
        }
    });
    ui.label(
        egui::RichText::new("Unknown to this build — kept so the file round-trips.")
            .small()
            .weak(),
    );
}

fn row_ui(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    def: &'static EffectDef,
    index: usize,
    count: usize,
    floor: usize,
) {
    let Some(row) = history.document().stack.get(id) else {
        return;
    };
    let enabled = row.enabled;
    let row_id = ui.make_persistent_id(("fx", id.0));
    let open_id = row_id.with("open");
    let mut open: bool = ui.data_mut(|d| *d.get_temp_mut_or(open_id, false));

    let action = resolve::effect_header(
        ui,
        row_id,
        def.name,
        enabled,
        open,
        (index > 0, index + 1 < count),
    );

    if action.expand {
        open = !open;
        ui.data_mut(|d| d.insert_temp(open_id, open));
    }
    if action.toggled {
        history.edit(
            if enabled { "Disable row" } else { "Enable row" },
            None,
            |doc| {
                if let Some(r) = doc.stack.get_mut(id) {
                    r.enabled = !enabled;
                }
            },
        );
    }
    if action.delete {
        history.edit(format!("Delete {}", def.name), None, |doc| {
            doc.stack.remove(id);
        });
        return;
    }
    if action.up {
        history.edit("Reorder", None, |doc| {
            doc.stack.reorder(id, floor + index.saturating_sub(1));
        });
    }
    if action.down {
        history.edit("Reorder", None, |doc| {
            doc.stack.reorder(id, floor + index + 1);
        });
    }
    if action.reset {
        history.edit(format!("Reset {}", def.name), None, |doc| {
            if let Some(r) = doc.stack.get_mut(id) {
                r.params = def.default_params();
                r.opacity = 1.0;
                r.blend = BlendMode::Normal;
            }
        });
    }

    if !open {
        return;
    }

    ui.add_space(2.0);
    // Top-level parameters first, in declaration order, then each heading in
    // the order it first appears. That is the order the effect declared, which
    // is the order the person who wrote it meant them to be read in.
    for param in def.params.iter().filter(|p| p.section.is_empty()) {
        param_ui(ui, history, id, param, row_id);
    }
    let mut seen: Vec<&'static str> = Vec::new();
    for param in def.params.iter().filter(|p| !p.section.is_empty()) {
        if seen.contains(&param.section) {
            continue;
        }
        seen.push(param.section);
        let section = param.section;
        resolve::section(ui, row_id.with(section), section, |ui| {
            for p in def.params.iter().filter(|p| p.section == section) {
                param_ui(ui, history, id, p, row_id);
            }
        });
    }

    // Resolve gives every plugin a Global Blend at the bottom. Ours lives on
    // the row, so every effect gets one for free — and the blend *mode* with
    // it, which Resolve's OFX plugins do not have at all.
    resolve::section(ui, row_id.with("blend"), "Global Blend", |ui| {
        blend_ui(ui, history, id, row_id);
    });
    ui.add_space(6.0);
}

fn blend_ui(ui: &mut egui::Ui, history: &mut History, id: RowId, row_id: egui::Id) {
    let Some(row) = history.document().stack.get(id) else {
        return;
    };
    let mut opacity = row.opacity;
    let mut blend = row.blend;

    let edit = resolve::slider_row(
        ui,
        row_id.with("opacity"),
        "Blend",
        &mut opacity,
        0.0..=1.0,
        3,
    );
    if edit.changed {
        history.edit("Blend", Some(format!("{}.blend", id.0)), |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.opacity = opacity;
            }
        });
    }
    if edit.released {
        history.break_coalescing();
    }
    if edit.reset {
        history.edit("Blend", None, |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.opacity = 1.0;
            }
        });
    }

    let mut mode = blend.as_str().to_string();
    let names: Vec<&'static str> = BlendMode::ALL.iter().map(|m| m.as_str()).collect();
    let edit = resolve::choice_row(ui, row_id.with("mode"), "Composite", &names, &mut mode);
    if edit.changed
        && let Some(next) = BlendMode::ALL.iter().find(|m| m.as_str() == mode)
    {
        blend = *next;
        history.edit("Blend mode", None, |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.blend = blend;
            }
        });
    }
    if edit.reset {
        history.edit("Blend mode", None, |doc| {
            if let Some(row) = doc.stack.get_mut(id) {
                row.blend = BlendMode::Normal;
            }
        });
    }
}

/// How many decimals a range wants.
///
/// A slider running to 360 degrees reading "180.000" is noise; one running to
/// 1.0 reading "0.2" has thrown away the resolution the control has.
fn decimals(min: f32, max: f32) -> usize {
    let span = (max - min).abs();
    if span > 100.0 {
        1
    } else if span > 10.0 {
        2
    } else {
        3
    }
}

fn param_ui(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    def: &'static ParamDef,
    row_id: egui::Id,
) {
    let current = history
        .document()
        .stack
        .get(id)
        .and_then(|r| r.params.get(def.key))
        .cloned();
    let coalesce = Some(format!("{}.{}", id.0, def.key));
    let param_id = row_id.with(def.key);

    match def.kind {
        ParamKind::Float {
            min,
            max,
            default,
            neutral,
        } => {
            let mut v = current
                .as_ref()
                .and_then(ParamValue::as_float)
                .unwrap_or(default);
            let name = if def.unit.is_empty() {
                def.name.to_string()
            } else {
                format!("{} ({})", def.name, def.unit)
            };
            let edit =
                resolve::slider_row(ui, param_id, &name, &mut v, min..=max, decimals(min, max));
            if edit.changed {
                set(history, id, def, ParamValue::Float(v), coalesce.clone());
            }
            if edit.released {
                history.break_coalescing();
            }
            // Reset means *neutral*, not default. For a look effect those
            // differ, and reset should always mean "do nothing".
            if edit.reset {
                set(history, id, def, ParamValue::Float(neutral), None);
            }
        }
        ParamKind::Bool { default } => {
            let mut v = current
                .as_ref()
                .and_then(ParamValue::as_bool)
                .unwrap_or(default);
            let edit = resolve::check_row(ui, param_id, def.name, &mut v);
            if edit.changed {
                set(history, id, def, ParamValue::Bool(v), None);
            }
            if edit.reset {
                set(history, id, def, ParamValue::Bool(default), None);
            }
        }
        ParamKind::Choice { options, default } => {
            let mut v = current
                .as_ref()
                .and_then(ParamValue::as_choice)
                .unwrap_or(default)
                .to_string();
            let edit = resolve::choice_row(ui, param_id, def.name, options, &mut v);
            if edit.changed {
                set(history, id, def, ParamValue::Choice(v), None);
            }
            if edit.reset {
                set(
                    history,
                    id,
                    def,
                    ParamValue::Choice(default.to_string()),
                    None,
                );
            }
        }
        ParamKind::Rgb { default } => {
            let mut v = match current {
                Some(ParamValue::Rgb(v)) => v,
                _ => default,
            };
            let edit = resolve::colour_row(ui, param_id, def.name, &mut v);
            if edit.changed {
                set(history, id, def, ParamValue::Rgb(v), None);
            }
            if edit.reset {
                set(history, id, def, ParamValue::Rgb(default), None);
            }
        }
        ParamKind::Wheel => {
            let mut w = current
                .as_ref()
                .and_then(ParamValue::as_wheel)
                .copied()
                .unwrap_or_default();
            resolve::section(ui, param_id, def.name, |ui| {
                let mut edit = Edit::default();
                for (i, label) in ["Red", "Green", "Blue"].iter().enumerate() {
                    let e = resolve::slider_row(
                        ui,
                        param_id.with(i),
                        label,
                        &mut w.rgb[i],
                        -0.5..=0.5,
                        3,
                    );
                    edit.changed |= e.changed;
                    edit.released |= e.released;
                    edit.reset |= e.reset;
                }
                let e = resolve::slider_row(
                    ui,
                    param_id.with("master"),
                    "Master",
                    &mut w.master,
                    -0.5..=0.5,
                    3,
                );
                edit.changed |= e.changed;
                edit.released |= e.released;
                edit.reset |= e.reset;

                if edit.changed {
                    set(history, id, def, ParamValue::Wheel(w), coalesce.clone());
                }
                if edit.released {
                    history.break_coalescing();
                }
                if edit.reset {
                    set(history, id, def, ParamValue::Wheel(Wheel::default()), None);
                }
            });
        }
        ParamKind::Curve => {
            let curve = current
                .as_ref()
                .and_then(ParamValue::as_curve)
                .cloned()
                .unwrap_or_default();
            ui.horizontal(|ui| {
                ui.add_space(resolve::LABEL_WIDTH - 60.0);
                ui.label(egui::RichText::new(def.name).small().weak());
                ui.label(
                    egui::RichText::new(if curve.is_identity() {
                        "identity"
                    } else {
                        "custom"
                    })
                    .small()
                    .monospace(),
                );
                // The real editor is the Tone Curve panel; a stacked Curves
                // row is rare enough that two presets are enough here.
                if ui.small_button("S-curve").clicked() {
                    set(
                        history,
                        id,
                        def,
                        ParamValue::Curve(Curve {
                            points: vec![[0.0, 0.0], [0.25, 0.18], [0.75, 0.82], [1.0, 1.0]],
                        }),
                        None,
                    );
                }
                if ui.small_button("Reset").clicked() {
                    set(history, id, def, ParamValue::Curve(Curve::default()), None);
                }
            });
        }
    }
}

fn set(
    history: &mut History,
    id: RowId,
    def: &'static ParamDef,
    value: ParamValue,
    coalesce: Option<String>,
) {
    history.edit(def.name, coalesce, |doc| {
        if let Some(row) = doc.stack.get_mut(id) {
            row.params.set(def.key, value);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_range_reads_in_fewer_decimals_than_a_narrow_one() {
        assert_eq!(decimals(0.0, 360.0), 1);
        assert_eq!(decimals(0.0, 100.0), 2);
        assert_eq!(decimals(-1.0, 1.0), 3);
    }

    /// Every parameter of every effect has to be reachable. A heading is only
    /// drawn when something declares it, so a typo in a section name would
    /// hide a control rather than misfile it — and nothing would say so.
    #[test]
    fn every_parameter_lands_under_a_heading_or_at_the_top() {
        for effect in pe_effects::all() {
            let top = effect
                .params
                .iter()
                .filter(|p| p.section.is_empty())
                .count();
            let mut seen: Vec<&str> = Vec::new();
            let mut filed = 0;
            for p in effect.params.iter().filter(|p| !p.section.is_empty()) {
                if !seen.contains(&p.section) {
                    seen.push(p.section);
                }
                filed += 1;
            }
            assert_eq!(
                top + filed,
                effect.params.len(),
                "{} loses parameters between the top level and its headings",
                effect.key
            );
        }
    }

    /// A heading with one control under it is a heading that costs a click and
    /// buys nothing.
    #[test]
    fn no_heading_holds_a_single_control() {
        for effect in pe_effects::all() {
            let mut sections: Vec<(&str, usize)> = Vec::new();
            for p in effect.params.iter().filter(|p| !p.section.is_empty()) {
                match sections.iter_mut().find(|(s, _)| *s == p.section) {
                    Some((_, n)) => *n += 1,
                    None => sections.push((p.section, 1)),
                }
            }
            for (name, n) in sections {
                assert!(n > 1, "{}'s \"{name}\" holds only {n} control", effect.key);
            }
        }
    }
}
