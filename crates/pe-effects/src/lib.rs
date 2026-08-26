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
pub mod tool;

pub use pack::{PARAM_SLOTS, declared_slots, pack, pack_all, slot_of, slots_used};
pub use registry::{
    EFFECTS, EFFECTS_WITH_VISIBLE_DEFAULTS, PINNED_ROWS, all, by_key, new_document,
};
pub use tool::Tool;

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
    /// Every group, in the order the effect browser lists them.
    ///
    /// Here rather than in the browser so that adding a variant and forgetting
    /// to list it is a compile error in one place instead of an effect that is
    /// in the registry, is fully implemented, has a shader, passes its tests —
    /// and cannot be added to a stack, because nothing draws a heading for it.
    pub const ALL: [Group; 4] = [Group::Basic, Group::Color, Group::Film, Group::Optics];

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
    ///
    /// Carries its own shape because the four are not interchangeable.
    /// Lift and Gamma sit at zero, Gain at one, Offset at twenty-five — and
    /// Offset has no master ring at all, only the three channels, which is
    /// how Resolve draws it. A single `Wheel` with none of that would have
    /// meant a Gain wheel that reads 0.00 when it is doing nothing.
    Wheel {
        min: f32,
        max: f32,
        default: f32,
        /// Whether the wheel has a fourth, achromatic *readout*.
        ///
        /// Not whether it has an achromatic control at all — every wheel has
        /// the ribbed bar under it, Offset included. The bar is a nudge you
        /// make without looking; the box is a value you read. Resolve draws
        /// four bars and three of Offset's boxes, and on a wheel with no
        /// master the bar moves the three channels together.
        master: bool,
    },
    /// An editable curve.
    /// A drawn curve, baked into the LUT texture.
    ///
    /// `flat` says what the identity is. A tone curve maps a level onto a
    /// level, so leaving it alone is the diagonal; a secondary answers "what
    /// should happen to this hue", and the answer that changes nothing is the
    /// same everywhere — a level line down the middle. Getting this wrong
    /// makes a freshly added Curves row rotate every hue in the picture.
    Curve {
        flat: bool,
    },
    Choice {
        options: &'static [&'static str],
        default: &'static str,
    },
    /// Pins on the chromaticity diagram. Like a warp, it rides the LUT.
    Pins,
    /// A lattice of displacements, dragged by hand — the Colour Warper's
    /// grids. Like a curve it takes no uniform slots: it travels to the GPU
    /// inside the LUT texture, because a hundred control points read through
    /// a uniform buffer per pixel is not a thing worth doing.
    Warp,
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
    /// Collapsible group this parameter belongs to in the inspector, or `""`
    /// for the top level.
    ///
    /// Resolve's plugins put their controls under headings — Add Vignetting,
    /// Add Dirt, Add Scratch 1 — and once an effect has thirty parameters that
    /// stops being decoration. Film Damage is unusable as a flat list.
    pub section: &'static str,
}

impl ParamDef {
    /// The same parameter, filed under a heading.
    ///
    /// A method rather than another argument on every constructor: most
    /// parameters have no section, and threading an empty string through a
    /// hundred call sites to serve the handful that do is the wrong trade.
    pub const fn in_section(self, section: &'static str) -> ParamDef {
        ParamDef {
            key: self.key,
            name: self.name,
            kind: self.kind,
            unit: self.unit,
            section,
        }
    }

    /// The value this parameter has when the user has not touched it.
    pub fn default_value(&self) -> ParamValue {
        match self.kind {
            ParamKind::Float { default, .. } => ParamValue::Float(default),
            ParamKind::Bool { default } => ParamValue::Bool(default),
            ParamKind::Rgb { default } => ParamValue::Rgb(default),
            ParamKind::Wheel { default, .. } => ParamValue::Wheel(pe_core::Wheel::uniform(default)),
            ParamKind::Curve { flat } => ParamValue::Curve(if flat {
                pe_core::Curve::flat()
            } else {
                pe_core::Curve::default()
            }),
            ParamKind::Choice { default, .. } => ParamValue::Choice(default.into()),
            ParamKind::Warp => ParamValue::Warp(pe_core::Warp::default()),
            ParamKind::Pins => ParamValue::Pins(pe_core::Pins::default()),
        }
    }
}

/// What has to be true for a group of parameters to apply.
///
/// Resolve greys out controls that cannot do anything: the Basic Grain
/// sliders inside Halation until Append Grain Internally is ticked, the
/// Secondary Glow's Gamma and Spread until its Strength leaves zero, every
/// Split Tone control inside Film Look Creator until it is enabled.
///
/// Worth copying, and not only for the look of it. A panel of forty controls
/// where a third of them silently do nothing is a panel that teaches the user
/// wrong things about the effect — they move a slider, see no change, and
/// conclude the slider is broken rather than switched off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gate {
    /// The parameter that decides.
    pub by: &'static str,
    pub when: When,
    /// The parameters it decides for.
    pub params: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum When {
    /// A checkbox that has to be ticked.
    True,
    /// And one that has to be clear. Resolve's Split Luma Chroma is the case:
    /// ticking it replaces one Threshold with two, so each set of controls
    /// gates on the opposite state of the same box.
    False,
    /// A slider that has to be off zero — an amount of nothing gates the
    /// controls that shape it.
    Positive,
    /// A dropdown that has to be on one particular option.
    Is(&'static str),
    /// A curve that has actually been drawn on.
    ///
    /// The case that made this worth having: the Curves panel's four
    /// intensity sliders scale a drawn curve, so on an untouched one they mix
    /// the picture with itself and do nothing. They were live and inert, which
    /// tells the user nothing about why — and "this slider is broken" is the
    /// only conclusion available to them.
    Drawn,
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
    /// Which controls switch other controls off. See [`Gate`].
    pub gates: &'static [Gate],
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

    /// Whether a parameter can currently do anything.
    ///
    /// The UI dims and disables the ones that cannot. The *shader* does not
    /// consult this — it reads the switch itself — because a gate is a
    /// statement about the interface, and duplicating it as a second source of
    /// truth is how the two come to disagree.
    pub fn is_active(&self, key: &str, params: &ParamMap) -> bool {
        let Some(gate) = self.gates.iter().find(|g| g.params.contains(&key)) else {
            return true;
        };
        let current = params
            .get(gate.by)
            .cloned()
            .or_else(|| self.param(gate.by).map(|p| p.default_value()));
        match (gate.when, current) {
            (When::True, Some(ParamValue::Bool(v))) => v,
            (When::False, Some(ParamValue::Bool(v))) => !v,
            (When::Positive, Some(ParamValue::Float(v))) => v.abs() > 1e-6,
            (When::Is(option), Some(ParamValue::Choice(v))) => v == option,
            (When::Drawn, Some(ParamValue::Curve(c))) => {
                match self.param(gate.by).map(|p| p.kind) {
                    // A tone curve's identity is the diagonal and a secondary's is
                    // a flat line, which is the same distinction `default_value`
                    // makes and for the same reason.
                    Some(ParamKind::Curve { flat: true }) => !c.is_flat(),
                    Some(ParamKind::Curve { flat: false }) => !c.is_identity(),
                    _ => true,
                }
            }
            // A gate naming a parameter that is not there, or one of the wrong
            // kind, must not silently disable the controls it guards — that
            // would be a typo taking a third of a panel away with no error.
            _ => true,
        }
    }

    /// Whether these parameters leave the image untouched.
    ///
    /// The pinned panels mean a fresh document already carries eleven rows. Each
    /// would otherwise cost a full-screen pass every frame to do nothing.
    /// Skipping the inert ones keeps a new document at zero passes and makes
    /// the pass counter mean what it says: the work your edit actually costs.
    ///
    /// Compares against each parameter's `neutral`, not its default — for a
    /// look effect those differ on purpose.
    pub fn is_neutral(&self, params: &ParamMap) -> bool {
        self.params.iter().all(|def| match def.kind {
            ParamKind::Float { neutral, .. } => {
                (params.float_or(def.key, neutral) - neutral).abs() < 1e-6
            }
            ParamKind::Bool { default } => params.bool_or(def.key, default) == default,
            ParamKind::Choice { default, .. } => params.choice_or(def.key, default) == default,
            ParamKind::Rgb { default } => params
                .get(def.key)
                .and_then(|v| match v {
                    ParamValue::Rgb(c) => Some(*c),
                    _ => None,
                })
                .is_none_or(|c| c == default),
            ParamKind::Wheel { default, .. } => params
                .get(def.key)
                .and_then(ParamValue::as_wheel)
                .is_none_or(|w| w.is_uniform(default)),
            ParamKind::Curve { flat } => params
                .get(def.key)
                .and_then(ParamValue::as_curve)
                .is_none_or(|c| if flat { c.is_flat() } else { c.is_identity() }),
            ParamKind::Warp => params
                .get(def.key)
                .and_then(ParamValue::as_warp)
                .is_none_or(|w| w.is_identity()),
            ParamKind::Pins => params
                .get(def.key)
                .and_then(ParamValue::as_pins)
                .is_none_or(|p| p.is_neutral()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every effect has to be somewhere a person can find it.
    ///
    /// The browser walks `Group::ALL` and draws a heading per group. An effect
    /// whose group is missing from that list is not merely hard to find — there
    /// is no heading it belongs under, so it is never drawn at all, and the only
    /// symptom is a tool nobody can add.
    #[test]
    fn every_effect_belongs_to_a_group_the_browser_lists() {
        for e in all() {
            assert!(
                Group::ALL.contains(&e.group),
                "{} is in {:?}, which the browser never lists",
                e.key,
                e.group
            );
        }
    }

    /// And no group is listed twice, which would draw its effects twice.
    #[test]
    fn the_group_list_has_no_repeats() {
        let mut seen = Group::ALL.to_vec();
        seen.sort_by_key(|g| g.as_str());
        seen.dedup();
        assert_eq!(seen.len(), Group::ALL.len());
    }

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
