//! The nine effects M1 implements, declared.
//!
//! The `space` field on each is the interesting column. Read down it:
//!
//! | Effect        | Space  | Why |
//! |---------------|--------|-----|
//! | Exposure      | Linear | Multiplying light by a scalar. Anywhere else it is not exposure, it is a brightness slider. |
//! | White balance | Linear | Channel gains modelling illuminant change. |
//! | Halation      | Linear | Light scattering back off the film base. |
//! | Vignette      | Linear | Light falloff across the frame. |
//! | Contrast      | Log    | Pivoting around mid-grey the way a viewer perceives it. |
//! | Curves        | Log    | The user is drawing a perceptual response, not a photometric one. |
//! | HSL           | Log    | Hue and saturation are perceptual constructs. |
//! | Lift/Gamma/Gain | Log  | The classic grading controls are defined on log-encoded signal. |
//! | Grain         | Log    | Film grain is a *density* fluctuation in the negative, not a light phenomenon. In linear it vanishes from the shadows. |

use pe_color::WorkingSpace;

use crate::{EffectDef, Group, ParamDef, ParamKind};

/// Shorthand for a bipolar slider whose neutral point is zero.
const fn bipolar(
    key: &'static str,
    name: &'static str,
    range: f32,
    unit: &'static str,
) -> ParamDef {
    ParamDef {
        key,
        name,
        kind: ParamKind::Float {
            min: -range,
            max: range,
            default: 0.0,
            neutral: 0.0,
        },
        unit,
    }
}

/// Shorthand for a unipolar 0..1 slider that starts at zero.
const fn amount(key: &'static str, name: &'static str) -> ParamDef {
    ParamDef {
        key,
        name,
        kind: ParamKind::Float {
            min: 0.0,
            max: 1.0,
            default: 0.0,
            neutral: 0.0,
        },
        unit: "",
    }
}

pub static EFFECTS: &[EffectDef] = &[
    EffectDef {
        key: "exposure",
        name: "Exposure",
        group: Group::Basic,
        space: WorkingSpace::Linear,
        shader: "exposure",
        spatial: false,
        derived_slots: 0,
        params: &[bipolar("ev", "Exposure", 5.0, "EV")],
    },
    EffectDef {
        key: "white_balance",
        name: "White Balance",
        group: Group::Basic,
        space: WorkingSpace::Linear,
        shader: "white_balance",
        spatial: false,
        // r, g, b gains, computed by pe_color::white_balance.
        derived_slots: 3,
        params: &[
            ParamDef {
                key: "temperature",
                name: "Temperature",
                kind: ParamKind::Float {
                    min: 2000.0,
                    max: 15000.0,
                    default: 6500.0,
                    neutral: 6500.0,
                },
                unit: "K",
            },
            bipolar("tint", "Tint", 100.0, ""),
        ],
    },
    EffectDef {
        key: "contrast",
        name: "Contrast",
        group: Group::Basic,
        space: WorkingSpace::Log,
        shader: "contrast",
        spatial: false,
        derived_slots: 0,
        params: &[
            bipolar("contrast", "Contrast", 1.0, ""),
            ParamDef {
                key: "pivot",
                name: "Pivot",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    // Mid-grey in ACEScct, not 0.5. Pivoting contrast around
                    // the wrong point drags the whole image up or down.
                    default: 0.4135,
                    neutral: 0.4135,
                },
                unit: "",
            },
        ],
    },
    EffectDef {
        key: "curves",
        name: "Custom Curves",
        group: Group::Color,
        space: WorkingSpace::Log,
        shader: "curves",
        spatial: false,
        derived_slots: 0,
        params: &[
            ParamDef {
                key: "luma",
                name: "Luma",
                kind: ParamKind::Curve,
                unit: "",
            },
            ParamDef {
                key: "red",
                name: "Red",
                kind: ParamKind::Curve,
                unit: "",
            },
            ParamDef {
                key: "green",
                name: "Green",
                kind: ParamKind::Curve,
                unit: "",
            },
            ParamDef {
                key: "blue",
                name: "Blue",
                kind: ParamKind::Curve,
                unit: "",
            },
            amount("soft_clip", "Soft Clip"),
        ],
    },
    EffectDef {
        key: "hsl",
        name: "HSL",
        group: Group::Color,
        space: WorkingSpace::Log,
        shader: "hsl",
        spatial: false,
        derived_slots: 0,
        params: &[
            bipolar("hue", "Hue", 180.0, "°"),
            bipolar("saturation", "Saturation", 1.0, ""),
            bipolar("luminance", "Luminance", 1.0, ""),
        ],
    },
    EffectDef {
        key: "primaries",
        name: "Lift / Gamma / Gain",
        group: Group::Color,
        space: WorkingSpace::Log,
        shader: "primaries",
        spatial: false,
        derived_slots: 0,
        params: &[
            ParamDef {
                key: "lift",
                name: "Lift",
                kind: ParamKind::Wheel,
                unit: "",
            },
            ParamDef {
                key: "gamma",
                name: "Gamma",
                kind: ParamKind::Wheel,
                unit: "",
            },
            ParamDef {
                key: "gain",
                name: "Gain",
                kind: ParamKind::Wheel,
                unit: "",
            },
            // The fourth wheel. Resolve has it and it is the one people
            // actually reach for; leaving it out is the most common way a
            // clone of these controls feels wrong.
            ParamDef {
                key: "offset",
                name: "Offset",
                kind: ParamKind::Wheel,
                unit: "",
            },
        ],
    },
    EffectDef {
        key: "grain",
        name: "Film Grain",
        group: Group::Film,
        space: WorkingSpace::Log,
        shader: "grain",
        spatial: true,
        derived_slots: 0,
        params: &[
            amount("strength", "Strength"),
            ParamDef {
                key: "size",
                name: "Size",
                kind: ParamKind::Float {
                    min: 0.5,
                    max: 8.0,
                    default: 2.0,
                    neutral: 2.0,
                },
                // Microns on a 35mm frame, not pixels. This is what makes the
                // 1200px preview and the 6000px export agree.
                unit: "µm",
            },
            ParamDef {
                key: "shadow_bias",
                name: "Shadow Bias",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
            },
            ParamDef {
                key: "monochrome",
                name: "Monochrome",
                kind: ParamKind::Bool { default: false },
                unit: "",
            },
        ],
    },
    EffectDef {
        key: "halation",
        name: "Halation",
        group: Group::Film,
        space: WorkingSpace::Linear,
        shader: "halation",
        spatial: true,
        derived_slots: 0,
        params: &[
            amount("strength", "Strength"),
            ParamDef {
                key: "radius",
                name: "Radius",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.2,
                    default: 0.02,
                    neutral: 0.0,
                },
                // Fraction of the image's long edge. Resolution independence
                // again: a pixel radius would shrink to a rim on export.
                unit: "",
            },
            ParamDef {
                key: "threshold",
                name: "Threshold",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 4.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
            },
            ParamDef {
                key: "tint",
                name: "Tint",
                kind: ParamKind::Choice {
                    options: &["red", "orange", "warm", "neutral"],
                    default: "orange",
                },
                unit: "",
            },
        ],
    },
    EffectDef {
        key: "vignette",
        name: "Vignette",
        group: Group::Optics,
        space: WorkingSpace::Linear,
        shader: "vignette",
        spatial: true,
        derived_slots: 0,
        params: &[
            bipolar("amount", "Amount", 1.0, ""),
            ParamDef {
                key: "midpoint",
                name: "Midpoint",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
            },
            ParamDef {
                key: "roundness",
                name: "Roundness",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
            },
            ParamDef {
                key: "feather",
                name: "Feather",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
            },
        ],
    },
];

pub fn all() -> &'static [EffectDef] {
    EFFECTS
}

pub fn by_key(key: &str) -> Option<&'static EffectDef> {
    EFFECTS.iter().find(|e| e.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn m1_ships_nine_effects() {
        assert_eq!(EFFECTS.len(), 9, "M1's scope is exactly nine effects");
    }

    #[test]
    fn keys_are_unique() {
        let keys: HashSet<_> = EFFECTS.iter().map(|e| e.key).collect();
        assert_eq!(keys.len(), EFFECTS.len());
    }

    #[test]
    fn parameter_keys_are_unique_within_an_effect() {
        for e in EFFECTS {
            let keys: HashSet<_> = e.params.iter().map(|p| p.key).collect();
            assert_eq!(keys.len(), e.params.len(), "{}", e.key);
        }
    }

    #[test]
    fn every_effect_has_at_least_one_parameter() {
        for e in EFFECTS {
            assert!(!e.params.is_empty(), "{} has no parameters", e.key);
        }
    }

    /// The two-space rule, asserted.
    ///
    /// Not a tautology: it pins the specific assignments so that a later "let's
    /// just move grain into linear, it's simpler" cannot happen silently.
    #[test]
    fn effects_run_in_the_space_their_physics_requires() {
        let expected = [
            // Light: multiplying, scattering, falling off.
            ("exposure", WorkingSpace::Linear),
            ("white_balance", WorkingSpace::Linear),
            ("halation", WorkingSpace::Linear),
            ("vignette", WorkingSpace::Linear),
            // Perception: pivoting, shaping, drawing.
            ("contrast", WorkingSpace::Log),
            ("curves", WorkingSpace::Log),
            ("hsl", WorkingSpace::Log),
            ("primaries", WorkingSpace::Log),
            // Density in the negative, not light.
            ("grain", WorkingSpace::Log),
        ];
        for (key, space) in expected {
            let e = by_key(key).unwrap_or_else(|| panic!("{key} missing from registry"));
            assert_eq!(e.space, space, "{key} is in the wrong working space");
        }
        assert_eq!(expected.len(), EFFECTS.len(), "an effect is untested");
    }

    #[test]
    fn spatial_effects_are_flagged() {
        // Spatial effects need their radius scaled by image dimensions and
        // cannot be fused into one pass. The renderer branches on this, so a
        // wrong flag is a correctness bug, not a performance one.
        for e in EFFECTS {
            let expected = matches!(e.key, "grain" | "halation" | "vignette");
            assert_eq!(e.spatial, expected, "{}", e.key);
        }
    }

    #[test]
    fn the_primaries_panel_has_four_wheels() {
        // Three wheels is the common mistake. Offset is the fourth.
        let p = by_key("primaries").unwrap();
        assert_eq!(p.params.len(), 4);
        assert!(p.param("offset").is_some(), "Offset wheel is missing");
    }

    #[test]
    fn contrast_pivots_around_log_mid_grey_not_half() {
        // 0.5 would be mid-grey in a display-referred space. In ACEScct, 18%
        // scene grey sits at ~0.4135.
        let pivot = by_key("contrast").unwrap().param("pivot").unwrap();
        match pivot.kind {
            ParamKind::Float { default, .. } => {
                assert!((default - 0.4135).abs() < 1e-3, "pivot is {default}");
            }
            _ => panic!("pivot should be a float"),
        }
    }

    #[test]
    fn spatial_radii_are_not_expressed_in_pixels() {
        // Guards the resolution-independence rule at the type level as far as
        // it can be guarded: no spatial size parameter may use a pixel unit.
        for e in EFFECTS.iter().filter(|e| e.spatial) {
            for p in e.params {
                assert_ne!(p.unit, "px", "{}.{} is in pixels", e.key, p.key);
            }
        }
    }

    #[test]
    fn unknown_keys_are_not_found() {
        assert!(by_key("dehaze").is_none(), "dehaze is M3, not M1");
    }
}
