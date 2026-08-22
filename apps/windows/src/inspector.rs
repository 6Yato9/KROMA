//! The Open FX list.
//!
//! Resolve's inspector rather than its node graph: an ordered list of effects,
//! each with an enable toggle, a title bar that expands it, reorder arrows, a
//! bin, and a reset. The rows themselves are drawn by [`crate::resolve`], so
//! every parameter in the application lines up in the same columns.
//!
//! Above the list sits the browser: every effect that can be added, always
//! visible rather than hidden behind a menu. Hovering one previews it on the
//! picture, which is the only way to answer "what does Halation look like on
//! *this* photograph" without adding it and undoing.
//!
//! The preview costs one GPU pass. It appends a row to the end of the stack,
//! and the stage cache only re-runs from the first changed row — so everything
//! above it is already in VRAM and the frame under the pointer is one pass
//! more than the frame beside it. That is the whole reason this is affordable
//! to do on hover rather than on click.
//!
//! Reordering is still arrows, not drag-and-drop. Dragging *into* the list is
//! easy because there is only one place it can land; dragging *within* it
//! needs an insertion indicator and autoscroll, and it is not what makes the
//! panel usable.

use pe_core::{BlendMode, Curve, History, ParamValue, RowId, RowIdGenerator, StackRow};
use pe_effects::{EffectDef, Group, ParamDef, ParamKind};

use crate::resolve::{self, Edit};
use crate::settings::Settings;

/// Padding inside a tile: room for the star on the right, and a little either
/// side of the name.
const TILE_PAD: egui::Vec2 = egui::vec2(30.0, 9.0);

/// How much of the tab the shelf gets before it starts scrolling.
const BROWSER_HEIGHT: f32 = 250.0;

pub fn show(
    ui: &mut egui::Ui,
    history: &mut History,
    ids: &mut RowIdGenerator,
    dragging: &mut Option<&'static str>,
    settings: &mut Settings,
) -> Option<&'static str> {
    ui.add_space(4.0);
    let preview = browser(ui, history, ids, dragging, settings);
    ui.add_space(8.0);
    ui.separator();
    ui.label(
        egui::RichText::new("Enabled")
            .small()
            .color(resolve::colour::LABEL),
    );
    ui.add_space(2.0);

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
            ui.label(
                egui::RichText::new("Drag an effect here, or click one above")
                    .weak()
                    .small(),
            );
        });
        ui.add_space(12.0);
        // A drop anywhere in the empty list still counts. There is nothing to
        // aim at, and refusing the drop because the list is empty would be the
        // panel being pedantic about its own layout.
        take_drop(ui, history, ids, dragging);
        return preview;
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
    take_drop(ui, history, ids, dragging);
    preview
}

/// Where a row's expanded state is kept.
///
/// Not `ui.make_persistent_id`, which mixes in the id of whichever `Ui` asked.
/// The row is opened from the shelf and read by the list, and those are two
/// different `Ui`s — so deriving it from either means the flag is written
/// under one key and looked for under another, and a freshly added effect
/// arrives shut.
pub fn open_flag(row: RowId) -> egui::Id {
    egui::Id::new(("fx", row.0)).with("open")
}

/// Add `key` to the end of the stack, opened.
fn add(ui: &egui::Ui, history: &mut History, ids: &mut RowIdGenerator, def: &'static EffectDef) {
    let id = ids.allocate();
    history.edit(format!("Add {}", def.name), None, |doc| {
        let mut row = StackRow::new(id, def.key);
        row.params = def.default_params();
        doc.stack.push(row);
    });
    // Open it. You added it to change something, and a row that arrives shut
    // costs a click to say so.
    ui.data_mut(|d| d.insert_temp(open_flag(id), true));
}

/// Resolve a drop that landed on the enabled list.
///
/// The whole panel counts as the target rather than the gap between two rows.
/// Choosing a position on the way in would need an insertion indicator and a
/// scroll-while-dragging story, and the arrows already move a row once it is
/// there.
fn take_drop(
    ui: &egui::Ui,
    history: &mut History,
    ids: &mut RowIdGenerator,
    dragging: &mut Option<&'static str>,
) {
    let Some(key) = *dragging else {
        return;
    };
    if !ui.input(|i| i.pointer.any_released()) {
        return;
    }
    let over = ui
        .input(|i| i.pointer.interact_pos())
        .is_some_and(|p| ui.min_rect().expand(4.0).contains(p));
    if over && let Some(def) = pe_effects::by_key(key) {
        add(ui, history, ids, def);
    }
    *dragging = None;
}

/// Every effect that can be added, in a list rather than behind a menu.
///
/// A menu is the right shape for a command you already know the name of. This
/// is a shelf you browse, and browsing is what you are doing when the question
/// is "what would look good here".
fn browser(
    ui: &mut egui::Ui,
    history: &mut History,
    ids: &mut RowIdGenerator,
    dragging: &mut Option<&'static str>,
    settings: &mut Settings,
) -> Option<&'static str> {
    let mut preview = None;
    egui::ScrollArea::vertical()
        .id_salt("effect_browser")
        .max_height(BROWSER_HEIGHT)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Starred first, and only there. Listing a favourite twice would
            // mean two tiles that do the same thing and one of them wrong
            // whenever the star is clicked on the other.
            let starred: Vec<_> = pe_effects::all()
                .iter()
                .filter(|e| !pe_effects::registry::PINNED_ROWS.contains(&e.key))
                .filter(|e| settings.is_favourite(e.key))
                .collect();
            if !starred.is_empty() {
                heading(ui, "Favourites");
                if let Some(hovered) = tiles(ui, history, ids, &starred, dragging, settings) {
                    preview = Some(hovered);
                }
            }

            for group in [Group::Basic, Group::Color, Group::Film, Group::Optics] {
                let available: Vec<_> = pe_effects::all()
                    .iter()
                    .filter(|e| e.group == group)
                    .filter(|e| !pe_effects::registry::PINNED_ROWS.contains(&e.key))
                    .filter(|e| !settings.is_favourite(e.key))
                    .collect();
                // Every Basic effect is a pinned panel, so that heading has
                // nothing under it. A heading over nothing reads as a list
                // that failed to load — and so does one whose entries have all
                // been starred away into the group above.
                if available.is_empty() {
                    continue;
                }
                heading(ui, group.as_str());
                if let Some(hovered) = tiles(ui, history, ids, &available, dragging, settings) {
                    preview = Some(hovered);
                }
            }
        });
    preview
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(text)
            .small()
            .color(resolve::colour::LABEL),
    );
    ui.add_space(2.0);
}

/// How big every tile is.
///
/// One size for all of them, set by the longest name. Tiles that each fit
/// their own text come out ragged and read as a list of buttons; a grid reads
/// as a set of things of the same kind, which is what they are. Measured
/// rather than guessed, so it follows the font instead of a number that was
/// right on one machine.
fn tile_size(ui: &egui::Ui) -> egui::Vec2 {
    let font = egui::FontId::proportional(11.5);
    let mut widest: f32 = 0.0;
    let mut line: f32 = 0.0;
    for def in pe_effects::all() {
        if pe_effects::registry::PINNED_ROWS.contains(&def.key) {
            continue;
        }
        let galley =
            ui.painter()
                .layout_no_wrap(def.name.to_string(), font.clone(), egui::Color32::WHITE);
        widest = widest.max(galley.size().x);
        line = line.max(galley.size().y);
    }
    egui::vec2(widest + TILE_PAD.x, line + TILE_PAD.y * 2.0)
}

/// A row of tiles, wrapping to as many as fit.
fn tiles(
    ui: &mut egui::Ui,
    history: &mut History,
    ids: &mut RowIdGenerator,
    defs: &[&'static EffectDef],
    dragging: &mut Option<&'static str>,
    settings: &mut Settings,
) -> Option<&'static str> {
    let mut preview = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for def in defs {
            if let Some(hovered) = tile(ui, history, ids, def, dragging, settings) {
                preview = Some(hovered);
            }
        }
    });
    preview
}

/// A five-pointed star, filled when the effect is starred.
fn star(painter: &egui::Painter, at: egui::Pos2, filled: bool, hot: bool) {
    let r = 6.0_f32;
    let mut points = Vec::with_capacity(10);
    for i in 0..10 {
        // Alternating outer and inner radius, starting at the top.
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        let radius = if i % 2 == 0 { r } else { r * 0.44 };
        points.push(egui::pos2(at.x + radius * a.cos(), at.y + radius * a.sin()));
    }
    let gold = egui::Color32::from_rgb(238, 190, 84);
    if filled {
        // Not a convex polygon, so it is drawn as a fan from the middle.
        let mut mesh = egui::Mesh::default();
        for i in 0..10 {
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(at, gold);
            mesh.colored_vertex(points[i], gold);
            mesh.colored_vertex(points[(i + 1) % 10], gold);
            mesh.add_triangle(base, base + 1, base + 2);
        }
        painter.add(egui::Shape::mesh(mesh));
    } else {
        painter.add(egui::Shape::closed_line(
            points,
            egui::Stroke::new(
                1.2_f32,
                if hot {
                    gold
                } else {
                    egui::Color32::from_gray(110)
                },
            ),
        ));
    }
}

/// One shelf tile. Hover to preview, click or drag to add, star to keep.
fn tile(
    ui: &mut egui::Ui,
    history: &mut History,
    ids: &mut RowIdGenerator,
    def: &'static EffectDef,
    dragging: &mut Option<&'static str>,
    settings: &mut Settings,
) -> Option<&'static str> {
    let (rect, response) = ui.allocate_exact_size(tile_size(ui), egui::Sense::click_and_drag());

    // The star owns its own corner. Checked before the tile's own click, or
    // starring an effect would also add it.
    let star_at = egui::pos2(rect.max.x - 12.0, rect.center().y);
    let star_rect = egui::Rect::from_center_size(star_at, egui::Vec2::splat(20.0));
    let star_response = ui.interact(
        star_rect,
        ui.id().with((def.key, "star")),
        egui::Sense::click(),
    );
    if star_response.clicked() {
        settings.toggle_favourite(def.key);
    }
    let over_star = star_response.hovered();

    if response.drag_started() && !over_star {
        *dragging = Some(def.key);
    }
    if response.clicked() && !over_star {
        add(ui, history, ids, def);
    }

    let held = *dragging == Some(def.key);
    let hot = (response.hovered() && !over_star) || held;
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            4.0,
            if hot {
                crate::theme::colour::CONTROL
            } else {
                crate::theme::colour::PANEL
            },
        );
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(
                1.0_f32,
                if hot {
                    resolve::colour::ACCENT
                } else {
                    crate::theme::colour::CONTROL_HOT
                },
            ),
            egui::StrokeKind::Inside,
        );
        star(painter, star_at, settings.is_favourite(def.key), over_star);
        painter.text(
            egui::pos2(rect.min.x + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            def.name,
            egui::FontId::proportional(11.5),
            if hot {
                resolve::colour::TITLE
            } else {
                egui::Color32::from_gray(198)
            },
        );
    }
    let response = response.on_hover_text(if settings.is_favourite(def.key) {
        format!("{} — starred", def.name)
    } else {
        def.name.to_string()
    });

    // The chip under the pointer, so a drag looks like it is carrying
    // something rather than like nothing is happening.
    if held && let Some(at) = ui.ctx().pointer_interact_pos() {
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("effect_drag_chip"),
        ));
        let galley = painter.layout_no_wrap(
            def.name.to_string(),
            egui::FontId::proportional(12.0),
            egui::Color32::WHITE,
        );
        let chip = egui::Rect::from_min_size(at + egui::vec2(12.0, 8.0), galley.size())
            .expand2(egui::vec2(8.0, 5.0));
        painter.rect_filled(chip, 4.0, egui::Color32::from_black_alpha(220));
        painter.rect_stroke(
            chip,
            4.0,
            egui::Stroke::new(1.0_f32, resolve::colour::ACCENT),
            egui::StrokeKind::Inside,
        );
        painter.galley(
            chip.min + egui::vec2(8.0, 5.0),
            galley,
            egui::Color32::WHITE,
        );
    }

    // Previewing while dragging as well: the picture under the chip is what
    // you are deciding about. Not while the pointer is on the star, though —
    // that gesture is about the shelf, not about the picture.
    ((response.hovered() && !over_star) || held).then_some(def.key)
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
    let open_id = open_flag(id);
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

pub fn param_ui(
    ui: &mut egui::Ui,
    history: &mut History,
    id: RowId,
    def: &'static ParamDef,
    row_id: egui::Id,
) {
    // Dimmed and dead when its switch is off, the way Resolve draws them.
    // Left visible rather than hidden: a control that vanishes takes the
    // knowledge that it exists with it, and the user has no way to find out
    // what ticking the box would give them.
    let active = history
        .document()
        .stack
        .get(id)
        .and_then(|row| pe_effects::by_key(&row.effect).map(|e| e.is_active(def.key, &row.params)))
        .unwrap_or(true);
    if !active {
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(ui.available_rect_before_wrap()));
        // Disabled, which is what stops a click reaching it — egui gives a
        // disabled `Ui` an inert response, so the row draws and reports
        // nothing. The history is the real one because the row still has to
        // *read* the value it is showing.
        child.disable();
        param_row(&mut child, history, id, def, row_id);
        let used = child.min_rect().height();
        ui.allocate_space(egui::vec2(ui.available_width(), used));
        return;
    }
    param_row(ui, history, id, def, row_id);
}

fn param_row(
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
    // Which effect this row is, so a parameter can be given the track that
    // matches its axis. Looked up rather than passed down because every
    // caller of this function already had to find the row to get here.
    let effect = history
        .document()
        .stack
        .get(id)
        .map(|r| r.effect.clone())
        .unwrap_or_default();

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
            let edit = resolve::slider_row_styled(
                ui,
                param_id,
                &name,
                &mut v,
                min..=max,
                decimals(min, max),
                resolve::TrackStyle::of(&effect, def.key, min, max, neutral),
            );
            if edit.changed {
                set(history, id, def, ParamValue::Float(v), coalesce.clone());
            }
            if edit.released {
                history.break_coalescing();
            }
            // The value the effect arrives with, not the value at which it
            // does nothing. See the note in `basic::slider`: the reset arrow
            // on the title bar has always restored defaults, and one icon
            // meaning two things depending on which row it sits in is worse
            // than either meaning on its own.
            if edit.reset {
                set(history, id, def, ParamValue::Float(default), None);
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
        ParamKind::Warp | ParamKind::Pins => {
            // Drawn by the panel, not by a row. Neither a lattice nor a set of
            // pins is a control with a value beside it, and the three views
            // share one plot.
        }
        ParamKind::Wheel {
            min,
            max,
            default,
            master,
        } => {
            let mut w = current
                .as_ref()
                .and_then(ParamValue::as_wheel)
                .copied()
                .unwrap_or_else(|| pe_core::Wheel::uniform(default));
            resolve::section(ui, param_id, def.name, |ui| {
                let mut edit = Edit::default();
                for (i, label) in ["Red", "Green", "Blue"].iter().enumerate() {
                    let e = resolve::slider_row_styled(
                        ui,
                        param_id.with(i),
                        label,
                        &mut w.rgb[i],
                        min..=max,
                        decimals(min, max),
                        resolve::TrackStyle {
                            ramp: crate::theme::CHANNEL_AXES[i],
                            neutral: Some(0.5),
                        },
                    );
                    edit.changed |= e.changed;
                    edit.released |= e.released;
                    edit.reset |= e.reset;
                }
                // Only where there is one. Resolve's Offset wheel has three
                // channels and no achromatic ring, because an achromatic
                // offset is an exposure change and there is a control for
                // that already.
                if master {
                    let e = resolve::slider_row_styled(
                        ui,
                        param_id.with("master"),
                        "Master",
                        &mut w.master,
                        min..=max,
                        decimals(min, max),
                        resolve::TrackStyle {
                            ramp: crate::theme::Ramp::Luma,
                            neutral: Some(0.5),
                        },
                    );
                    edit.changed |= e.changed;
                    edit.released |= e.released;
                    edit.reset |= e.reset;
                }

                if edit.changed {
                    set(history, id, def, ParamValue::Wheel(w), coalesce.clone());
                }
                if edit.released {
                    history.break_coalescing();
                }
                if edit.reset {
                    set(
                        history,
                        id,
                        def,
                        ParamValue::Wheel(pe_core::Wheel::uniform(default)),
                        None,
                    );
                }
            });
        }
        ParamKind::Curve { .. } => {
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

    /// Click the reset arrow on a real effect row and see what the document
    /// says afterwards.
    ///
    /// The Effects tab draws every parameter of every effect through one
    /// function, so this covers all of them at once — and it covers the thing
    /// a reading cannot: that the arrow is where the click lands.
    #[test]
    fn the_reset_arrow_on_an_effect_row_puts_the_value_back() {
        let mut doc = pe_effects::new_document("photo.jpg");
        let def = pe_effects::by_key("sharpen").expect("sharpen is registered");
        let id = RowId(900);
        let mut row = pe_core::StackRow::new(id, "sharpen");
        row.params = def.default_params();
        row.params.set("amount", ParamValue::Float(7.0));
        doc.stack.push(row);
        let mut history = History::new(doc);

        let amount = def.param("amount").expect("amount");
        let ctx = egui::Context::default();
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(420.0, 300.0));
        let frame = |input: egui::RawInput, history: &mut History| {
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
                    param_ui(&mut child, history, id, amount, egui::Id::new("row"));
                });
            });
        };

        let base = egui::RawInput {
            screen_rect: Some(area),
            ..Default::default()
        };
        frame(base.clone(), &mut history);

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

        let now = history
            .document()
            .stack
            .get(id)
            .and_then(|r| r.params.get("amount"))
            .and_then(ParamValue::as_float)
            .expect("set");
        // Resolve ships Sharpen at 1.8, and reset means *default* — the value
        // the effect arrives with. It used to mean neutral, which put this at
        // zero and made the same icon mean something different from the one
        // on the effect's own title bar.
        assert_eq!(now, 1.8, "the reset arrow did not restore the default");
    }

    /// A heading with one control under it is a heading that costs a click and
    /// buys nothing.
    ///
    /// Two exceptions, both Resolve's own, and both listed rather than waved
    /// through: there the heading is doing a *dividing* job even with one
    /// member — "Chroma" says the control below it is about colour and the
    /// ones above are about luminance. Naming them here keeps the rule with
    /// teeth for anything we invent ourselves.
    #[test]
    fn no_heading_holds_a_single_control() {
        const RESOLVE_HAS_THESE: [(&str, &str); 2] = [
            ("sharpen", "Chroma"),
            ("soften_sharpen", "Adjust Small Texture Granularity"),
        ];
        for effect in pe_effects::all() {
            let mut sections: Vec<(&str, usize)> = Vec::new();
            for p in effect.params.iter().filter(|p| !p.section.is_empty()) {
                match sections.iter_mut().find(|(s, _)| *s == p.section) {
                    Some((_, n)) => *n += 1,
                    None => sections.push((p.section, 1)),
                }
            }
            for (name, n) in sections {
                if RESOLVE_HAS_THESE.contains(&(effect.key, name)) {
                    continue;
                }
                assert!(n > 1, "{}'s \"{name}\" holds only {n} control", effect.key);
            }
        }
    }
}
