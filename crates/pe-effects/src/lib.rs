//! The effect registry.
//!
//! Every effect declares three things: its parameters, its shader entry point,
//! and — the important one — **which working space it must run in**. The
//! renderer reads that declaration and inserts the colour transform. No effect
//! ever converts its own input.
//!
//! That single rule is what stops the pipeline decaying into scattered
//! `pow(x, 2.2)` calls. It is enforced structurally: [`EffectDef::space`] has no
//! default, so adding an effect without deciding does not compile.
//!
//! M0 defines the metadata for the nine effects M1 will implement. The shaders
//! arrive with M1; the declarations exist now because they are what the
//! renderer's row loop is written against.

use pe_color::WorkingSpace;
use pe_core::{ParamMap, ParamValue};

pub mod pack;
pub mod registry;

pub use pack::{PARAM_SLOTS, declared_slots, pack, pack_all, slot_of, slots_used};
pub use registry::{EFFECTS, all, by_key};

/// Which panel an effect appears under in the inspector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Exposure, white balance, contrast — the first things anyone reaches for.
    Basic,
    /// Wheels, curves, HSL, qualifier.
    Color,
    /// Grain, halation, bloom, film damage.
    Film,
    /// Blur, sharpen, vignette, lens.
    Optics,
}

impl Group {
    pub fn as_str(self) -> &'static str {
        match self {
            Group::Basic => "Basic",
            Group::Color => "Colour",
            Group::Film => "Film",
            Group::Optics => "Optics",
        }
    }
}

/// The kind and bounds of a single parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamKind {
    /// A slider. `min`/`max` bound the UI, `default` is the neutral value.
    Float {
        min: f32,
        max: f32,
        default: f32,
        /// Where the "no change" point sits — usually but not always the
        /// default. Sliders draw their fill from here, and double-click resets
        /// to it.
        neutral: f32,
    },
    Bool {
        default: bool,
    },
    /// A colour, in the working gamut. Resolve exposes these as a picker with
    /// an eyedropper — Haze Color, Dirt Color, Scratch Color.
    Rgb {
        default: [f32; 3],
    },
    /// A four-way colour wheel.
    Wheel,
    /// An editable curve.
    Curve,
    Choice {
        options: &'static [&'static str],
        default: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamDef {
    /// Key used in the document. Stable; never rename one without a migration.
    pub key: &'static str,
    /// Label shown in the inspector.
    pub name: &'static str,
    pub kind: ParamKind,
    /// Unit suffix for the readout, e.g. `"EV"`, `"K"`, `"%"`.
    pub unit: &'static str,
}

impl ParamDef {
    /// The value this parameter has when the user has not touched it.
    pub fn default_value(&self) -> ParamValue {
        match self.kind {
            ParamKind::Float { default, .. } => ParamValue::Float(default),
            ParamKind::Bool { default } => ParamValue::Bool(default),
            ParamKind::Rgb { default } => ParamValue::Rgb(default),
            ParamKind::Wheel => ParamValue::Wheel(Default::default()),
            ParamKind::Curve => ParamValue::Curve(Default::default()),
            ParamKind::Choice { default, .. } => ParamValue::Choice(default.into()),
        }
    }
}

/// Everything the renderer and the UI need to know about one effect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EffectDef {
    /// Registry key, stored in documents. Stable forever.
    pub key: &'static str,
    pub name: &'static str,
    pub group: Group,
    /// **Which working space this effect runs in.**
    ///
    /// Not optional and not defaulted. Getting this wrong is not a subtle bug —
    /// a blur in log space looks like fog, a lift wheel in linear space has no
    /// usable travel.
    pub space: WorkingSpace,
    pub params: &'static [ParamDef],
    /// Name of the WGSL entry point in `shaders/`.
    pub shader: &'static str,
    /// Whether the effect reads neighbouring pixels. Spatial effects need their
    /// radius scaled by image dimensions, and cannot be fused into a single
    /// pass with their neighbours.
    pub spatial: bool,
    /// Extra uniform slots the effect does not expose as parameters, filled by
    /// [`pack::derive`] from CPU-side colour science.
    ///
    /// White balance is the motivating case: the user edits temperature and
    /// tint, but the shader wants three channel gains, and computing those
    /// requires the working gamut's matrices. Doing it on the CPU keeps the
    /// colour science in `pe-color` where it is tested, and leaves the shader
    /// as a single multiply.
    pub derived_slots: usize,
}

impl EffectDef {
    /// A parameter map with every parameter at its default.
    pub fn default_params(&self) -> ParamMap {
        self.params
            .iter()
            .map(|p| (p.key.to_string(), p.default_value()))
            .collect()
    }

    pub fn param(&self, key: &str) -> Option<&'static ParamDef> {
        self.params.iter().find(|p| p.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_params_cover_every_declared_parameter() {
        for e in all() {
            let p = e.default_params();
            assert_eq!(p.len(), e.params.len(), "{}", e.key);
            for def in e.params {
                assert!(p.get(def.key).is_some(), "{}: missing {}", e.key, def.key);
            }
        }
    }

    #[test]
    fn float_defaults_lie_within_their_declared_range() {
        for e in all() {
            for p in e.params {
                if let ParamKind::Float {
                    min,
                    max,
                    default,
                    neutral,
                } = p.kind
                {
                    assert!(min < max, "{}.{}: empty range", e.key, p.key);
                    assert!(
                        (min..=max).contains(&default),
                        "{}.{}: default {default} outside {min}..={max}",
                        e.key,
                        p.key
                    );
                    assert!(
                        (min..=max).contains(&neutral),
                        "{}.{}: neutral {neutral} outside {min}..={max}",
                        e.key,
                        p.key
                    );
                }
            }
        }
    }

    #[test]
    fn choice_defaults_are_one_of_the_options() {
        for e in all() {
            for p in e.params {
                if let ParamKind::Choice { options, default } = p.kind {
                    assert!(!options.is_empty(), "{}.{}", e.key, p.key);
                    assert!(
                        options.contains(&default),
                        "{}.{}: default {default:?} not in {options:?}",
                        e.key,
                        p.key
                    );
                }
            }
        }
    }
}
