//! The stacked inspector.
//!
//! Resolve's Cut-page inspector rather than its node graph: an ordered list of
//! rows, each with its own enable, opacity, blend mode and parameters, and
//! reorderable.
//!
//! Reordering is up/down buttons at M1, not drag-and-drop. Drag-to-reorder is
//! the single fiddliest interaction in the whole application — it wants hit
//! testing, an insertion indicator, autoscroll, and a touch story for the
//! tablet later — and building it against a UI that is being thrown away at M2
//! would be building it twice.

use pe_core::{BlendMode, Curve, History, ParamValue, RowId, RowIdGenerator, StackRow, Wheel};
use pe_effects::{EffectDef, Group, ParamKind};

pub fn show(ui: &mut egui::Ui, history: &mut History, ids: &mut RowIdGenerator) {
    ui.add_space(6.0);
    add_effect_menu(ui, history, ids);
    ui.separator();

    let rows: Vec<(RowId, String)> = history
        .document()
        .stack
        .iter()
        .map(|r| (r.id, r.effect.clone()))
        .collect();

    if rows.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("No effects yet").weak());
            ui.label(egui::RichText::new("Add one above.").weak().small());
        });
        return;
    }

    let count = rows.len();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (index, (id, effect_key)) in rows.into_iter().enumerate() {
            let Some(def) = pe_effects::by_key(&effect_key) else {
                unknown_row(ui, history, id, &effect_key);
                continue;
            };
            row_ui(ui, history, id, def, index, count);
            ui.add_space(4.0);
        }
    });
}

fn add_effect_menu(ui: &mut egui::Ui, history: &mut History, ids: &mut RowIdGenerator) {
    ui.menu_button("＋  Add effect", |ui| {
        for group in [Group::Basic, Group::Color, Group::Film, Group::Optics] {
            ui.label(egui::RichText::new(group.as_str()).small().weak());
            for def in pe_effects::all().iter().filter(|e| e.group == group) {
                if ui.button(def.name).clicked() {
                    let id = ids.allocate();
                    history.edit(format!("Add {}", def.name), None, |doc| {
                        let mut row = StackRow::new(id, def.key);
                        row.params = def.default_params();
                        doc.stack.push(row);
                    });
                    ui.close();
                }
            }
            ui.separator();
        }
    });
}

fn unknown_row(ui: &mut egui::Ui, history: &mut History, id: RowId, key: &str) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("⚠ {key}")).weak());
            if ui.small_button("✕").clicked() {
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
    });
}

fn row_ui(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    def: &'static EffectDef,
    index: usize,
    count: usize,
) {
    let Some(row) = history.document().stack.get(id) else {
        return;
    };
    let mut enabled = row.enabled;
    let mut opacity = row.opacity;
    let mut blend = row.blend;

    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.checkbox(&mut enabled, "").changed() {
                history.edit(
                    if enabled { "Enable row" } else { "Disable row" },
                    None,
                    |doc| {
                        if let Some(r) = doc.stack.get_mut(id) {
                            r.enabled = enabled;
                        }
                    },
                );
            }
            ui.label(egui::RichText::new(def.name).strong());

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕").on_hover_text("Delete").clicked() {
                    history.edit(format!("Delete {}", def.name), None, |doc| {
                        doc.stack.remove(id);
                    });
                }
                ui.add_enabled_ui(index + 1 < count, |ui| {
                    if ui.small_button("▼").on_hover_text("Move down").clicked() {
                        history.edit("Reorder", None, |doc| {
                            doc.stack.reorder(id, index + 1);
                        });
                    }
                });
                ui.add_enabled_ui(index > 0, |ui| {
                    if ui.small_button("▲").on_hover_text("Move up").clicked() {
                        history.edit("Reorder", None, |doc| {
                            doc.stack.reorder(id, index.saturating_sub(1));
                        });
                    }
                });
                // The space the effect runs in. Shown because it explains why
                // a control behaves the way it does, and because seeing it
                // wrong is the fastest way to catch a registry mistake.
                ui.label(
                    egui::RichText::new(def.space.as_str())
                        .small()
                        .weak()
                        .monospace(),
                );
            });
        });

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            // Resolve calls this Blend and puts it in every plugin title bar:
            // default 1.0, range 0..1. Ours lives on the row, so every effect
            // gets one for free.
            ui.label(egui::RichText::new("blend").small().weak())
                .on_hover_text("Mixes this effect against its input. Resolve's per-effect Blend.");
            let r = ui.add(
                egui::Slider::new(&mut opacity, 0.0..=1.0)
                    .show_value(false)
                    .fixed_decimals(2),
            );
            if r.changed() {
                history.edit("Blend", Some(format!("{}.blend", id.0)), |doc| {
                    if let Some(row) = doc.stack.get_mut(id) {
                        row.opacity = opacity;
                    }
                });
            }
            if r.drag_stopped() {
                history.break_coalescing();
            }
            ui.label(
                egui::RichText::new(format!("{:.0}%", opacity * 100.0))
                    .small()
                    .monospace(),
            );

            egui::ComboBox::from_id_salt(("blend", id.0))
                .selected_text(blend.as_str())
                .width(96.0)
                .show_ui(ui, |ui| {
                    for mode in BlendMode::ALL {
                        if ui
                            .selectable_value(&mut blend, *mode, mode.as_str())
                            .clicked()
                        {
                            history.edit("Blend mode", None, |doc| {
                                if let Some(row) = doc.stack.get_mut(id) {
                                    row.blend = blend;
                                }
                            });
                        }
                    }
                });
        });

        ui.add_space(4.0);
        for param in def.params {
            param_ui(ui, history, id, param);
        }
    });
}

fn param_ui(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    def: &'static pe_effects::ParamDef,
) {
    let current = history
        .document()
        .stack
        .get(id)
        .and_then(|r| r.params.get(def.key))
        .cloned();
    let coalesce = Some(format!("{}.{}", id.0, def.key));

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
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::Slider::new(&mut v, min..=max)
                        .text(def.name)
                        .suffix(def.unit),
                );
                if r.changed() {
                    set(
                        history,
                        id,
                        def.key,
                        ParamValue::Float(v),
                        def.name,
                        coalesce.clone(),
                    );
                }
                if r.drag_stopped() {
                    history.break_coalescing();
                }
                // Double-click-to-reset is the interaction people expect from
                // a grading control, and it needs the *neutral* value, which
                // is not always the default.
                if r.double_clicked() {
                    set(
                        history,
                        id,
                        def.key,
                        ParamValue::Float(neutral),
                        def.name,
                        None,
                    );
                }
            });
        }
        ParamKind::Bool { default } => {
            let mut v = current
                .as_ref()
                .and_then(ParamValue::as_bool)
                .unwrap_or(default);
            if ui.checkbox(&mut v, def.name).changed() {
                set(history, id, def.key, ParamValue::Bool(v), def.name, None);
            }
        }
        ParamKind::Choice { options, default } => {
            let mut v = current
                .as_ref()
                .and_then(ParamValue::as_choice)
                .unwrap_or(default)
                .to_string();
            ui.horizontal(|ui| {
                ui.label(def.name);
                egui::ComboBox::from_id_salt((id.0, def.key))
                    .selected_text(&v)
                    .show_ui(ui, |ui| {
                        for option in options {
                            if ui
                                .selectable_value(&mut v, (*option).to_string(), *option)
                                .clicked()
                            {
                                set(
                                    history,
                                    id,
                                    def.key,
                                    ParamValue::Choice(v.clone()),
                                    def.name,
                                    None,
                                );
                            }
                        }
                    });
            });
        }
        ParamKind::Rgb { default } => {
            let mut v = match current {
                Some(ParamValue::Rgb(v)) => v,
                _ => default,
            };
            ui.horizontal(|ui| {
                ui.label(def.name);
                // Working-gamut linear values, so the picker is fed the same
                // numbers the shader sees rather than a display-space guess.
                if ui.color_edit_button_rgb(&mut v).changed() {
                    set(history, id, def.key, ParamValue::Rgb(v), def.name, None);
                }
                if ui.small_button("Reset").clicked() {
                    set(
                        history,
                        id,
                        def.key,
                        ParamValue::Rgb(default),
                        def.name,
                        None,
                    );
                }
            });
        }
        ParamKind::Wheel => {
            let mut w = current
                .as_ref()
                .and_then(ParamValue::as_wheel)
                .copied()
                .unwrap_or_default();
            ui.collapsing(def.name, |ui| {
                // Four drag values at M1. A real colour wheel — a draggable
                // puck over a hue disc with a luminance ring — is a from-scratch
                // custom widget and belongs with the rest of M2's palette work.
                let mut changed = false;
                for (i, label) in ["R", "G", "B"].iter().enumerate() {
                    let r = ui.add(
                        egui::Slider::new(&mut w.rgb[i], -0.5..=0.5)
                            .text(*label)
                            .fixed_decimals(3),
                    );
                    changed |= r.changed();
                    if r.drag_stopped() {
                        history.break_coalescing();
                    }
                }
                let r = ui.add(
                    egui::Slider::new(&mut w.master, -0.5..=0.5)
                        .text("Master")
                        .fixed_decimals(3),
                );
                changed |= r.changed();
                if r.drag_stopped() {
                    history.break_coalescing();
                }
                if changed {
                    set(
                        history,
                        id,
                        def.key,
                        ParamValue::Wheel(w),
                        def.name,
                        coalesce.clone(),
                    );
                }
                if ui.small_button("Reset").clicked() {
                    set(
                        history,
                        id,
                        def.key,
                        ParamValue::Wheel(Wheel::default()),
                        def.name,
                        None,
                    );
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
                // A draggable spline editor is M2. Two presets are enough to
                // prove the LUT path end to end without pretending otherwise.
                if ui.small_button("S-curve").clicked() {
                    set(
                        history,
                        id,
                        def.key,
                        ParamValue::Curve(Curve {
                            points: vec![[0.0, 0.0], [0.25, 0.18], [0.75, 0.82], [1.0, 1.0]],
                        }),
                        def.name,
                        None,
                    );
                }
                if ui.small_button("Reset").clicked() {
                    set(
                        history,
                        id,
                        def.key,
                        ParamValue::Curve(Curve::default()),
                        def.name,
                        None,
                    );
                }
            });
        }
    }
}

fn set(
    history: &mut History,
    id: RowId,
    key: &'static str,
    value: ParamValue,
    label: &'static str,
    coalesce: Option<String>,
) {
    history.edit(label, coalesce, |doc| {
        if let Some(row) = doc.stack.get_mut(id) {
            row.params.set(key, value);
        }
    });
}
