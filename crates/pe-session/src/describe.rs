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

#[cfg(test)]
mod tests {
    use super::*;

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
}
