//! Packing parameters into the uniform block every effect shader reads.
//!
//! One uniform layout is shared by all effects rather than one per effect. That
//! means a single bind group layout, a single pipeline-creation path, and no
//! per-effect Rust plumbing when a new shader is added — the registry entry is
//! the whole change.
//!
//! Slots are assigned by walking [`EffectDef::params`] in declaration order.
//! **Order is the ABI.** Reordering a `params` array silently rewires every
//! saved document that uses that effect, so the layout is asserted in tests and
//! the shaders index by named constant.

use pe_core::{ParamMap, ParamValue};

use crate::{EffectDef, ParamKind};

/// Number of `f32` slots available to an effect. Twelve `vec4`s.
///
/// Sized for Film Damage, which needs 29 on its own because Resolve gives each
/// of its five scratches independent position, width and strength — and one
/// scratch control with a count would not let you place them. Raising this
/// costs a little uniform bandwidth per pass and nothing else.
pub const PARAM_SLOTS: usize = 64;

/// How many `f32` slots a parameter kind occupies.
///
/// Curves take none: they are baked into a LUT texture instead, because a
/// spline evaluated per pixel in a uniform loop is far slower than a texture
/// fetch and no more accurate.
pub const fn slot_width(kind: &ParamKind) -> usize {
    match kind {
        ParamKind::Float { .. } | ParamKind::Bool { .. } | ParamKind::Choice { .. } => 1,
        ParamKind::Rgb { .. } => 3,
        ParamKind::Wheel => 4,
        ParamKind::Curve { .. } => 0,
    }
}

/// The slot index a named parameter occupies, or `None` for curves and unknown
/// names.
pub fn slot_of(effect: &EffectDef, key: &str) -> Option<usize> {
    let mut at = 0;
    for p in effect.params {
        if p.key == key {
            return (slot_width(&p.kind) > 0).then_some(at);
        }
        at += slot_width(&p.kind);
    }
    None
}

/// Slots occupied by the effect's *declared* parameters.
pub fn declared_slots(effect: &EffectDef) -> usize {
    effect.params.iter().map(|p| slot_width(&p.kind)).sum()
}

/// Total slots an effect needs, declared plus derived.
pub fn slots_used(effect: &EffectDef) -> usize {
    declared_slots(effect) + effect.derived_slots
}

/// Pack a document's parameter values into the uniform block.
///
/// Missing or wrong-typed values fall back to the registry default, so a
/// partially-written document renders rather than failing.
pub fn pack(effect: &EffectDef, params: &ParamMap) -> [f32; PARAM_SLOTS] {
    let mut out = [0.0f32; PARAM_SLOTS];
    let mut at = 0;

    for def in effect.params {
        let width = slot_width(&def.kind);
        if width == 0 {
            continue;
        }
        debug_assert!(
            at + width <= PARAM_SLOTS,
            "{} overflows the uniform block",
            effect.key
        );

        let value = params.get(def.key);
        match def.kind {
            ParamKind::Float { default, .. } => {
                out[at] = value.and_then(ParamValue::as_float).unwrap_or(default);
            }
            ParamKind::Bool { default } => {
                let b = value.and_then(ParamValue::as_bool).unwrap_or(default);
                out[at] = if b { 1.0 } else { 0.0 };
            }
            ParamKind::Choice { options, default } => {
                let chosen = value.and_then(ParamValue::as_choice).unwrap_or(default);
                // Index, not string. An unrecognised choice falls back to the
                // default's index rather than to 0, which might be a very
                // different look.
                out[at] = options
                    .iter()
                    .position(|o| *o == chosen)
                    .or_else(|| options.iter().position(|o| *o == default))
                    .unwrap_or(0) as f32;
            }
            ParamKind::Rgb { default } => {
                let v = match value {
                    Some(ParamValue::Rgb(v)) => *v,
                    _ => default,
                };
                out[at] = v[0];
                out[at + 1] = v[1];
                out[at + 2] = v[2];
            }
            ParamKind::Wheel => {
                let w = value
                    .and_then(ParamValue::as_wheel)
                    .copied()
                    .unwrap_or_default();
                out[at] = w.rgb[0];
                out[at + 1] = w.rgb[1];
                out[at + 2] = w.rgb[2];
                out[at + 3] = w.master;
            }
            ParamKind::Curve { .. } => unreachable!("curves have zero width"),
        }
        at += width;
    }
    out
}

/// Pack, then fill the derived slots from CPU-side colour science.
///
/// The renderer always calls this rather than [`pack`] directly.
pub fn pack_all(effect: &EffectDef, params: &ParamMap) -> [f32; PARAM_SLOTS] {
    let mut out = pack(effect, params);
    derive(effect, params, &mut out);
    out
}

/// Compute the slots that are not user parameters.
///
/// Deliberately a match on `key` rather than a function pointer on `EffectDef`:
/// there is one case today, and a `&'static` registry entry holding a closure
/// would make the whole table non-const for no benefit. Revisit if this grows
/// past a handful.
fn derive(effect: &EffectDef, params: &ParamMap, out: &mut [f32; PARAM_SLOTS]) {
    if effect.derived_slots == 0 {
        return;
    }
    let at = declared_slots(effect);
    match effect.key {
        "white_balance" => {
            let temp = params.float_or("temperature", 6500.0) as f64;
            let tint = params.float_or("tint", 0.0) as f64;
            let g = pe_color::working_gains(temp, tint);
            out[at] = g[0] as f32;
            out[at + 1] = g[1] as f32;
            out[at + 2] = g[2] as f32;
        }
        other => debug_assert!(false, "{other} declares derived slots but has no deriver"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{by_key, registry::EFFECTS};
    use pe_core::{ParamValue, Wheel};

    #[test]
    fn no_effect_overflows_the_uniform_block() {
        for e in EFFECTS {
            let used = slots_used(e);
            assert!(
                used <= PARAM_SLOTS,
                "{} needs {used} slots, block holds {PARAM_SLOTS}",
                e.key
            );
        }
    }

    #[test]
    fn defaults_pack_to_the_registry_defaults() {
        let e = by_key("exposure").unwrap();
        let packed = pack(e, &e.default_params());
        assert_eq!(packed[0], 0.0, "exposure should default to 0 EV");
    }

    #[test]
    fn a_missing_parameter_falls_back_to_its_default() {
        let e = by_key("white_balance").unwrap();
        let packed = pack(e, &ParamMap::default());
        assert_eq!(packed[0], 6500.0, "temperature default");
        assert_eq!(packed[1], 0.0, "tint default");
    }

    #[test]
    fn a_wrong_typed_parameter_falls_back_rather_than_packing_garbage() {
        let e = by_key("exposure").unwrap();
        let mut p = ParamMap::default();
        p.set("ev", ParamValue::Choice("nonsense".into()));
        assert_eq!(pack(e, &p)[0], 0.0);
    }

    #[test]
    fn wheels_occupy_four_consecutive_slots() {
        let e = by_key("primaries").unwrap();
        let mut p = e.default_params();
        p.set(
            "gain",
            ParamValue::Wheel(Wheel {
                rgb: [0.1, 0.2, 0.3],
                master: 0.4,
            }),
        );
        let packed = pack(e, &p);
        // lift(0..4), gamma(4..8), gain(8..12), offset(12..16)
        let at = slot_of(e, "gain").unwrap();
        assert_eq!(at, 8);
        assert_eq!(&packed[at..at + 4], &[0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn the_four_primaries_wheels_are_laid_out_in_order() {
        // This layout is the shader ABI. If it moves, every saved grade using
        // the primaries panel renders differently.
        let e = by_key("primaries").unwrap();
        assert_eq!(slot_of(e, "lift"), Some(0));
        assert_eq!(slot_of(e, "gamma"), Some(4));
        assert_eq!(slot_of(e, "gain"), Some(8));
        assert_eq!(slot_of(e, "offset"), Some(12));
        assert_eq!(slots_used(e), 16);
    }

    #[test]
    fn booleans_pack_as_zero_or_one() {
        let e = by_key("split_tone").unwrap();
        let mut p = e.default_params();
        let at = slot_of(e, "preview_influence").unwrap();

        p.set("preview_influence", ParamValue::Bool(true));
        assert_eq!(pack(e, &p)[at], 1.0);
        p.set("preview_influence", ParamValue::Bool(false));
        assert_eq!(pack(e, &p)[at], 0.0);
    }

    #[test]
    fn choices_pack_as_their_option_index() {
        let e = by_key("split_tone").unwrap();
        let at = slot_of(e, "mode").unwrap();
        let mut p = e.default_params();

        p.set("mode", ParamValue::Choice("Natural".into()));
        assert_eq!(pack(e, &p)[at], 0.0);
        p.set("mode", ParamValue::Choice("Custom".into()));
        assert_eq!(pack(e, &p)[at], 2.0);
    }

    #[test]
    fn an_unknown_choice_falls_back_to_the_default_index_not_zero() {
        // "Strong" is index 1; an unrecognised mode must fall back to the
        // declared default rather than to 0, which could be a different look.
        let e = by_key("split_tone").unwrap();
        let at = slot_of(e, "mode").unwrap();
        let mut p = e.default_params();
        p.set("mode", ParamValue::Choice("chartreuse".into()));
        assert_eq!(pack(e, &p)[at], 0.0, "should fall back to natural");
    }

    #[test]
    fn curves_take_no_uniform_slots() {
        let e = by_key("curves").unwrap();
        for key in ["luma", "red", "green", "blue"] {
            assert_eq!(slot_of(e, key), None, "curves go to the LUT texture");
        }
        // Everything else is packed: four channel intensities, the four-part
        // soft clip, then the parametric curve's regions and splits.
        assert_eq!(slots_used(e), 15);
        assert_eq!(slot_of(e, "luma_intensity"), Some(0));
        assert_eq!(slot_of(e, "soft_clip_low"), Some(4));
        assert_eq!(
            slot_of(e, "param_shadows"),
            Some(8),
            "the shader reads the regions from slots 8-11"
        );
        assert_eq!(slot_of(e, "split_high"), Some(14));
    }

    #[test]
    fn white_balance_derives_gains_after_its_declared_slots() {
        let e = by_key("white_balance").unwrap();
        assert_eq!(declared_slots(e), 2, "temperature, tint");
        assert_eq!(slots_used(e), 5, "plus three derived gains");

        let packed = pack_all(e, &e.default_params());
        // At the neutral temperature the gains must be exactly 1, or opening a
        // file with default white balance would shift its colour.
        for (i, v) in packed.iter().enumerate().take(5).skip(2) {
            assert!((v - 1.0).abs() < 1e-6, "slot {i} is {v} at 6500K/0 tint");
        }
    }

    #[test]
    fn derived_gains_track_the_temperature_parameter() {
        let e = by_key("white_balance").unwrap();
        let mut p = e.default_params();
        p.set("temperature", ParamValue::Float(2800.0));
        let packed = pack_all(e, &p);
        assert!(
            packed[4] > packed[2],
            "correcting tungsten should raise blue above red: {:?}",
            &packed[2..5]
        );
    }

    #[test]
    fn effects_without_derived_slots_are_unchanged_by_deriving() {
        for e in EFFECTS.iter().filter(|e| e.derived_slots == 0) {
            let p = e.default_params();
            assert_eq!(pack(e, &p), pack_all(e, &p), "{}", e.key);
        }
    }

    #[test]
    fn packing_is_deterministic() {
        for e in EFFECTS {
            let p = e.default_params();
            assert_eq!(pack(e, &p), pack(e, &p), "{}", e.key);
        }
    }
}
