//! The registry and the open document, as JSON, for a front end that cannot
//! see Rust types.
//!
//! `pe-effects`'s own types are `&'static str` and cannot derive `Serialize`,
//! and should not: the registry is a compile-time table, and making it
//! serialisable to suit one consumer would put a serde attribute on every line
//! of a 4,700-line file. These are flat mirrors, built on demand.
//!
//! Flat on purpose. A tagged union with associated values is pleasant in Rust
//! and in Swift and unpleasant in the JSON between them, where every optional
//! field costs one `if let` on each side. `kind` is a string and the fields
//! that apply to that kind are set; the rest are absent.

use serde::{Deserialize, Serialize};

use pe_effects::{Gate, ParamKind, When};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Registry {
    /// Every effect, in the order the browser lists them.
    pub effects: Vec<Effect>,
    /// The rows a fresh document starts with, as fixed panels.
    pub pinned: Vec<String>,
    /// Effects that do something visible at their defaults, so the UI can warn
    /// that adding one changes the picture immediately.
    pub visible_at_defaults: Vec<String>,
    /// Group headings, in the order the browser shows them.
    pub groups: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Effect {
    pub key: String,
    pub name: String,
    pub group: String,
    /// `"linear"` or `"log"` — which working space the renderer runs it in.
    /// The UI never acts on this; it is here for the About panel and for bug
    /// reports, where "which space was that in" is the first question.
    pub space: String,
    pub spatial: bool,
    pub params: Vec<Param>,
    pub gates: Vec<GateJson>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Param {
    pub key: String,
    pub name: String,
    /// One of: float, bool, rgb, wheel, curve, choice, pins, warp.
    pub kind: String,
    pub unit: String,
    pub section: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_float: Option<f32>,
    /// Where "no change" sits — usually but not always the default. Sliders
    /// draw their fill from here and double-click resets to it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub neutral: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_bool: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_rgb: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_choice: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// Wheels only: whether there is a fourth, achromatic readout. Not whether
    /// there is an achromatic control — every wheel has the ribbed bar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master: Option<bool>,
    /// Curves only: whether the identity is a flat line rather than the
    /// diagonal. Getting this wrong rotates every hue in the picture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flat: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateJson {
    pub by: String,
    /// One of: true, false, positive, is, drawn.
    pub when: String,
    /// Set only when `when` is `is`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option: Option<String>,
    pub params: Vec<String>,
}

fn param(p: &pe_effects::ParamDef) -> Param {
    let mut out = Param {
        key: p.key.to_string(),
        name: p.name.to_string(),
        kind: String::new(),
        unit: p.unit.to_string(),
        section: p.section.to_string(),
        min: None,
        max: None,
        default_float: None,
        neutral: None,
        default_bool: None,
        default_rgb: None,
        default_choice: None,
        options: Vec::new(),
        master: None,
        flat: None,
    };
    match p.kind {
        ParamKind::Float {
            min,
            max,
            default,
            neutral,
        } => {
            out.kind = "float".into();
            out.min = Some(min);
            out.max = Some(max);
            out.default_float = Some(default);
            out.neutral = Some(neutral);
        }
        ParamKind::Bool { default } => {
            out.kind = "bool".into();
            out.default_bool = Some(default);
        }
        ParamKind::Rgb { default } => {
            out.kind = "rgb".into();
            out.default_rgb = Some(default);
        }
        ParamKind::Wheel {
            min,
            max,
            default,
            master,
        } => {
            out.kind = "wheel".into();
            out.min = Some(min);
            out.max = Some(max);
            out.default_float = Some(default);
            out.neutral = Some(default);
            out.master = Some(master);
        }
        ParamKind::Curve { flat } => {
            out.kind = "curve".into();
            out.flat = Some(flat);
        }
        ParamKind::Choice { options, default } => {
            out.kind = "choice".into();
            out.options = options.iter().map(|o| o.to_string()).collect();
            out.default_choice = Some(default.to_string());
        }
        ParamKind::Pins => out.kind = "pins".into(),
        ParamKind::Warp => out.kind = "warp".into(),
    }
    out
}

fn gate(g: &Gate) -> GateJson {
    let (when, option) = match g.when {
        When::True => ("true", None),
        When::False => ("false", None),
        When::Positive => ("positive", None),
        When::Is(o) => ("is", Some(o.to_string())),
        When::Drawn => ("drawn", None),
    };
    GateJson {
        by: g.by.to_string(),
        when: when.to_string(),
        option,
        params: g.params.iter().map(|p| p.to_string()).collect(),
    }
}

/// The whole registry, ready to hand to a front end.
pub fn registry() -> Registry {
    Registry {
        effects: pe_effects::all()
            .iter()
            .map(|e| Effect {
                key: e.key.to_string(),
                name: e.name.to_string(),
                group: e.group.as_str().to_string(),
                space: match e.space {
                    pe_color::WorkingSpace::Linear => "linear",
                    pe_color::WorkingSpace::Log => "log",
                }
                .to_string(),
                spatial: e.spatial,
                params: e.params.iter().map(param).collect(),
                gates: e.gates.iter().map(gate).collect(),
            })
            .collect(),
        pinned: pe_effects::PINNED_ROWS
            .iter()
            .map(|s| s.to_string())
            .collect(),
        visible_at_defaults: pe_effects::EFFECTS_WITH_VISIBLE_DEFAULTS
            .iter()
            .map(|s| s.to_string())
            .collect(),
        groups: pe_effects::Group::ALL
            .iter()
            .map(|g| g.as_str().to_string())
            .collect(),
    }
}

/// Everything a shell needs to draw itself, in one document.
///
/// Derived from the session, never authored. Mutations go one way in through
/// typed calls; this comes one way back out. Two directions and one source of
/// truth, which is what keeps a Swift `@Observable` idiomatic without becoming
/// a second implementation of the document model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    /// Bumped by every mutation. Compare before decoding.
    pub version: u64,
    pub is_open: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// File name alone, for the title bar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub width: u32,
    pub height: u32,
    /// What an export will actually produce.
    ///
    /// Not the source's size: the crop decides how much picture there is and
    /// the resize decides how many pixels it is delivered in, and a
    /// quarter-turn swaps the two. Carried rather than recomputed in each
    /// shell, because it is [`pe_render::export::output_size`] and that is
    /// where the rule lives.
    pub output_width: u32,
    pub output_height: u32,
    pub rows: Vec<Row>,
    pub color: Color,
    /// The crop the document holds. Always present — the identity when nothing
    /// is open — so the shell draws the overlay from one branch and not two.
    pub geometry: GeometryJson,
    /// Passes the last frame executed. The number that proves the stage cache
    /// works: with a nine-row stack, dragging the deepest slider reads 1.
    pub passes: usize,
    pub can_undo: bool,
    pub can_redo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redo_label: Option<String>,
    pub export_format: String,
    pub export_quality: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Row {
    pub id: u64,
    pub effect: String,
    pub enabled: bool,
    pub opacity: f32,
    pub blend: String,
    /// Fixed panels, which cannot be removed or reordered.
    pub pinned: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The document's own parameter representation, verbatim — `{"t":"float",
    /// "v":0.35}`. One shape on the wire rather than two.
    pub params: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Color {
    pub input: String,
    pub output: String,
}

/// The crop, straighten, quarter-turns and flips.
///
/// Flat, like everything else here. `AspectLock` is a Rust enum with a payload
/// on one of its three arms, and that shape costs an `if let` on each side of
/// the wire for no gain — so it travels the way a `Choice` travels: `aspect` is
/// a string naming the arm, and the two numbers that only a ratio has are set
/// only when it is one.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeometryJson {
    /// Centre of the crop as an offset from the middle of the source, in units
    /// of the source's own width and height.
    pub centre: [f32; 2],
    /// Size of the crop as a fraction of the source.
    pub size: [f32; 2],
    /// Straightening angle in degrees, positive anticlockwise.
    pub angle: f32,
    /// Quarter-turns clockwise, 0 to 3.
    pub turns: u8,
    pub flip_h: bool,
    pub flip_v: bool,
    /// One of: free, original, ratio.
    pub aspect: String,
    /// Set only when `aspect` is `ratio`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_w: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_h: Option<f32>,
}

fn geometry(g: &pe_core::Geometry) -> GeometryJson {
    let (aspect, aspect_w, aspect_h) = match g.aspect {
        pe_core::AspectLock::Free => ("free", None, None),
        pe_core::AspectLock::Original => ("original", None, None),
        pe_core::AspectLock::Ratio { w, h } => ("ratio", Some(w), Some(h)),
    };
    GeometryJson {
        centre: g.centre,
        size: g.size,
        angle: g.angle,
        turns: g.turns,
        flip_h: g.flip_h,
        flip_v: g.flip_v,
        aspect: aspect.to_string(),
        aspect_w,
        aspect_h,
    }
}

pub fn snapshot(session: &crate::Session) -> Snapshot {
    let doc = session.document();
    let (width, height) = session.image_size();
    // The source's size when nothing is open, which is (0, 0): there is no
    // document to ask, and no export to size.
    let (output_width, output_height) = doc
        .map(|d| pe_render::export::output_size(d, width, height))
        .unwrap_or((width, height));
    let rows = doc
        .map(|d| {
            d.stack
                .iter()
                .map(|r| Row {
                    id: r.id.0,
                    effect: r.effect.clone(),
                    enabled: r.enabled,
                    opacity: r.opacity,
                    blend: serde_json::to_value(r.blend)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_else(|| "normal".into()),
                    pinned: r.pinned,
                    label: r.label.clone(),
                    params: r
                        .params
                        .0
                        .iter()
                        .filter_map(|(k, v)| Some((k.clone(), serde_json::to_value(v).ok()?)))
                        .collect(),
                })
                .collect()
        })
        .unwrap_or_default();

    let export = session.export_settings();
    Snapshot {
        version: session.snapshot_version(),
        is_open: session.is_open(),
        path: session.path().map(|p| p.display().to_string()),
        name: session
            .path()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string()),
        width,
        height,
        output_width,
        output_height,
        rows,
        color: Color {
            input: doc.map(|d| d.color.input.clone()).unwrap_or_default(),
            output: doc.map(|d| d.color.output.clone()).unwrap_or_default(),
        },
        geometry: geometry(&session.geometry().unwrap_or_default()),
        passes: session.last_passes(),
        can_undo: session.can_undo(),
        can_redo: session.can_redo(),
        undo_label: session.undo_label(),
        redo_label: session.redo_label(),
        export_format: export.format.name().to_string(),
        export_quality: export.quality,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The output is not the source once anything has been cropped or turned,
    /// which is the entire reason the File page shows both numbers.
    ///
    /// The quarter-turn is here on purpose: it is what separates reading
    /// `pe_render::export::output_size` from multiplying the width by the
    /// crop's own fraction, which agrees with it right up until the picture is
    /// stood on its side.
    #[test]
    fn the_snapshot_reports_the_size_an_export_will_be() {
        let mut s = crate::Session::new();
        s.open_test_chart(256, 256).unwrap();

        let before = snapshot(&s);
        assert_eq!((before.output_width, before.output_height), (256, 256));

        // The left half of the picture, stood on its side.
        s.set_geometry(pe_core::Geometry {
            size: [0.5, 1.0],
            turns: 1,
            ..Default::default()
        })
        .unwrap();

        let after = snapshot(&s);
        assert_eq!(
            (after.width, after.height),
            (256, 256),
            "the source is untouched"
        );
        assert_eq!((after.output_width, after.output_height), (256, 128));
    }

    #[test]
    fn every_registered_effect_is_described() {
        let r = registry();
        assert_eq!(r.effects.len(), pe_effects::all().len());
        assert_eq!(r.pinned, pe_effects::PINNED_ROWS);
    }

    #[test]
    fn a_float_parameter_carries_everything_a_slider_needs() {
        let r = registry();
        let exposure = r.effects.iter().find(|e| e.key == "exposure").unwrap();
        let ev = exposure.params.iter().find(|p| p.key == "ev").unwrap();
        assert_eq!(ev.kind, "float");
        // Without all four a slider cannot draw itself: where it starts, where
        // it ends, where it rests, and where its fill grows from.
        assert!(ev.min.is_some());
        assert!(ev.max.is_some());
        assert!(ev.default_float.is_some());
        assert!(ev.neutral.is_some());
    }

    #[test]
    fn every_param_kind_survives_the_crossing() {
        // Eight kinds, eight control views on the Swift side. A kind that
        // serialises as something Swift cannot name is a control that silently
        // does not appear.
        let r = registry();
        let kinds: std::collections::BTreeSet<&str> = r
            .effects
            .iter()
            .flat_map(|e| e.params.iter().map(|p| p.kind.as_str()))
            .collect();
        for expected in [
            "float", "bool", "rgb", "wheel", "curve", "choice", "pins", "warp",
        ] {
            assert!(
                kinds.contains(expected),
                "no parameter serialised as {expected}"
            );
        }
    }

    #[test]
    fn a_choice_lists_its_options() {
        let r = registry();
        let choice = r
            .effects
            .iter()
            .flat_map(|e| e.params.iter())
            .find(|p| p.kind == "choice")
            .expect("some effect has a dropdown");
        assert!(!choice.options.is_empty());
        assert!(choice.default_choice.is_some());
    }

    #[test]
    fn gates_travel_with_the_effect_that_owns_them() {
        let r = registry();
        let gated = r
            .effects
            .iter()
            .find(|e| !e.gates.is_empty())
            .expect("some effect greys out controls");
        let g = &gated.gates[0];
        assert!(!g.by.is_empty());
        assert!(!g.params.is_empty());
        assert!(["true", "false", "positive", "is", "drawn"].contains(&g.when.as_str()));
    }

    #[test]
    fn a_snapshot_of_nothing_is_still_a_snapshot() {
        // The shell draws an empty viewer from this rather than from a null.
        let s = crate::Session::new();
        let snap = snapshot(&s);
        assert!(!snap.is_open);
        assert!(snap.rows.is_empty());
        assert!(!snap.can_undo);
    }

    #[test]
    fn a_row_carries_everything_the_inspector_draws() {
        let mut s = crate::Session::new();
        s.open_test_chart(64, 64).unwrap();
        let id = s.add_effect("exposure").unwrap();
        s.set_float(id, "ev", 0.75).unwrap();

        let snap = snapshot(&s);
        let row = snap.rows.iter().find(|r| r.id == id.0).unwrap();
        assert_eq!(row.effect, "exposure");
        assert!(row.enabled);
        assert_eq!(row.blend, "normal");
        assert!(!row.pinned);
        // Parameters keep the document's own representation, so there is one
        // shape on the wire and not two.
        let ev = row.params.get("ev").unwrap();
        assert_eq!(ev["t"], "float");
        assert_eq!(ev["v"], 0.75);
    }

    #[test]
    fn undo_shows_in_the_snapshot_and_says_what_it_would_undo() {
        let mut s = crate::Session::new();
        s.open_test_chart(64, 64).unwrap();
        s.add_effect("exposure").unwrap();
        let snap = snapshot(&s);
        assert!(snap.can_undo);
        assert_eq!(snap.undo_label.as_deref(), Some("Add Exposure"));
    }

    /// The snapshot carries it, so the shell can draw the crop it is editing.
    #[test]
    fn the_snapshot_carries_the_geometry() {
        let mut s = crate::Session::new();
        s.open_test_chart(64, 64).unwrap();
        let want = pe_core::Geometry {
            angle: 7.5,
            turns: 1,
            flip_h: true,
            ..Default::default()
        };
        s.set_geometry(want).unwrap();

        let json = serde_json::to_value(snapshot(&s)).unwrap();
        let g = &json["geometry"];
        assert!((g["angle"].as_f64().unwrap() - 7.5).abs() < 1e-4);
        assert_eq!(g["turns"], 1);
        assert_eq!(g["flip_h"], true);
    }

    /// The aspect lock is a Rust enum, and it crosses as a string plus the two
    /// numbers only a ratio has — the shape a choice takes everywhere else
    /// here. A shell reads one field to know which arm it is.
    #[test]
    fn an_aspect_lock_crosses_as_a_name_and_its_numbers() {
        let mut s = crate::Session::new();
        s.open_test_chart(64, 64).unwrap();

        let json = serde_json::to_value(snapshot(&s)).unwrap();
        assert_eq!(json["geometry"]["aspect"], "free");
        assert!(json["geometry"]["aspect_w"].is_null(), "free has no ratio");

        let want = pe_core::Geometry {
            aspect: pe_core::AspectLock::Ratio { w: 16.0, h: 9.0 },
            ..Default::default()
        };
        s.set_geometry(want).unwrap();
        let json = serde_json::to_value(snapshot(&s)).unwrap();
        assert_eq!(json["geometry"]["aspect"], "ratio");
        assert_eq!(json["geometry"]["aspect_w"], 16.0);
        assert_eq!(json["geometry"]["aspect_h"], 9.0);
    }

    /// And a shut session still has one, so the shell draws its overlay from
    /// one branch rather than two.
    #[test]
    fn a_snapshot_of_nothing_carries_an_uncropped_geometry() {
        let snap = snapshot(&crate::Session::new());
        assert_eq!(snap.geometry.size, [1.0, 1.0]);
        assert_eq!(snap.geometry.aspect, "free");
    }

    #[test]
    fn the_version_moves_on_a_change_and_not_otherwise() {
        // What lets a shell skip decoding an unchanged frame for the cost of
        // one integer comparison.
        let mut s = crate::Session::new();
        s.open_test_chart(64, 64).unwrap();
        let before = snapshot(&s).version;
        assert_eq!(snapshot(&s).version, before);
        s.add_effect("exposure").unwrap();
        assert!(snapshot(&s).version > before);
    }
}
