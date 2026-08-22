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

use crate::{EffectDef, Gate, Group, ParamDef, ParamKind, When};

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
        section: "",
    }
}

/// Shorthand for a 0..100 control that starts at full, like Resolve's
/// per-channel curve intensities.
const fn intensity(key: &'static str, name: &'static str) -> ParamDef {
    ParamDef {
        key,
        name,
        kind: ParamKind::Float {
            min: 0.0,
            max: 100.0,
            default: 100.0,
            neutral: 100.0,
        },
        unit: "",
        section: "",
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
        section: "",
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
        gates: &[],
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
        gates: &[],
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
                section: "",
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
        gates: &[],
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
                section: "",
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
        gates: &[],
        params: &[
            ParamDef {
                key: "luma",
                name: "Luma",
                kind: ParamKind::Curve { flat: false },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "red",
                name: "Red",
                kind: ParamKind::Curve { flat: false },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "green",
                name: "Green",
                kind: ParamKind::Curve { flat: false },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "blue",
                name: "Blue",
                kind: ParamKind::Curve { flat: false },
                unit: "",
                section: "",
            },
            // The secondaries. Each answers "what should happen to this hue"
            // — or to this luminance, or this saturation — rather than mapping
            // a level onto a level, which is why their identity is a flat line
            // and a tone curve's is a diagonal.
            ParamDef {
                key: "hue_vs_hue",
                name: "Hue Vs Hue",
                kind: ParamKind::Curve { flat: true },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "hue_vs_sat",
                name: "Hue Vs Sat",
                kind: ParamKind::Curve { flat: true },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "hue_vs_lum",
                name: "Hue Vs Lum",
                kind: ParamKind::Curve { flat: true },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "lum_vs_sat",
                name: "Lum Vs Sat",
                kind: ParamKind::Curve { flat: true },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "sat_vs_sat",
                name: "Sat Vs Sat",
                kind: ParamKind::Curve { flat: true },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "sat_vs_lum",
                name: "Sat Vs Lum",
                kind: ParamKind::Curve { flat: true },
                unit: "",
                section: "",
            },
            // How much of each drawn curve to apply. Resolve shows these as
            // 0 to 100 beside the channel buttons, and they are the reason you
            // can dial a curve back without redrawing it.
            intensity("luma_intensity", "Y"),
            intensity("red_intensity", "R"),
            intensity("green_intensity", "G"),
            intensity("blue_intensity", "B"),
            // Soft Clip, in Resolve's four parts: where the knee starts at
            // each end, and how gradual it is. A single "amount" cannot say
            // both, and which of the two you want is the whole question.
            amount("soft_clip_low", "Low").in_section("Soft Clip"),
            amount("soft_clip_low_soft", "Low Soft").in_section("Soft Clip"),
            amount("soft_clip_high", "High").in_section("Soft Clip"),
            amount("soft_clip_high_soft", "High Soft").in_section("Soft Clip"),
            // The parametric curve. Four regions and three movable boundaries
            // between them — a shape that cannot be made un-smooth, which is
            // the whole reason to offer it alongside the point curves.
            bipolar("param_shadows", "Shadows", 1.0, ""),
            bipolar("param_darks", "Darks", 1.0, ""),
            bipolar("param_lights", "Lights", 1.0, ""),
            bipolar("param_highlights", "Highlights", 1.0, ""),
            ParamDef {
                key: "split_low",
                name: "Shadow Split",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.25,
                    neutral: 0.25,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "split_mid",
                name: "Midtone Split",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "split_high",
                name: "Highlight Split",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.75,
                    neutral: 0.75,
                },
                unit: "",
                section: "",
            },
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
        gates: &[],
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
        gates: &[],
        params: &[
            ParamDef {
                key: "lift",
                name: "Lift",
                kind: ParamKind::Wheel,
                unit: "",
                section: "",
            },
            ParamDef {
                key: "gamma",
                name: "Gamma",
                kind: ParamKind::Wheel,
                unit: "",
                section: "",
            },
            ParamDef {
                key: "gain",
                name: "Gain",
                kind: ParamKind::Wheel,
                unit: "",
                section: "",
            },
            // The fourth wheel. Resolve has it and it is the one people
            // actually reach for; leaving it out is the most common way a
            // clone of these controls feels wrong.
            ParamDef {
                key: "offset",
                name: "Offset",
                kind: ParamKind::Wheel,
                unit: "",
                section: "",
            },
        ],
    },
    // Parameter names, ranges and defaults follow Resolve 20.1 — see
    // docs/resolve-parameters.md. Strength 0.5 / Pivot 0.3 / Hue Angle 20 are
    // read off the Resolve UI, so this effect is deliberately *not* neutral at
    // its defaults; see `EFFECTS_WITH_VISIBLE_DEFAULTS`.
    EffectDef {
        key: "split_tone",
        name: "Split Tone",
        group: Group::Color,
        space: WorkingSpace::Log,
        shader: "split_tone",
        spatial: false,
        derived_slots: 0,
        gates: &[Gate {
            by: "mode",
            when: When::Is("Custom"),
            params: &[
                "shadow_strength",
                "shadow_hue",
                "highlight_strength",
                "highlight_hue",
            ],
        }],
        params: &[
            // Resolve's panel, in Resolve's order. The Colour Space Overrides it
            // also carries are not here and will not be: which space an effect runs
            // in is the renderer's decision under the two-space rule, not a
            // per-effect override.
            ParamDef {
                key: "mode",
                name: "Split Tone Mode",
                kind: ParamKind::Choice {
                    options: &["Natural", "Strong", "Custom"],
                    default: "Natural",
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "preview_influence",
                name: "Preview Influence",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "strength",
                name: "Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.0,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "pivot",
                name: "Pivot",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.3,
                    neutral: 0.3,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "hue_angle",
                name: "Hue Angle",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 360.0,
                    default: 20.0,
                    neutral: 20.0,
                },
                unit: "°",
                section: "",
            },
            ParamDef {
                key: "protect_neutrals",
                name: "Protect Neutrals",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "",
            },
            // Custom mode only, and dimmed until it is chosen. Resolve shows
            // nothing under Protect Neutrals and nothing under the other two
            // modes, which is why the saturation band that used to be exposed
            // here is now fixed in the shader: it was two controls solving a
            // problem the checkbox already solves.
            ParamDef {
                key: "shadow_strength",
                name: "Shadow Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Shadows",
            },
            ParamDef {
                key: "shadow_hue",
                name: "Shadow Hue",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 360.0,
                    default: 200.0,
                    neutral: 200.0,
                },
                unit: "°",
                section: "Shadows",
            },
            ParamDef {
                key: "highlight_strength",
                name: "Highlight Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Highlights",
            },
            ParamDef {
                key: "highlight_hue",
                name: "Highlight Hue",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 360.0,
                    default: 20.0,
                    neutral: 20.0,
                },
                unit: "°",
                section: "Highlights",
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
        gates: &[],
        // Follows Resolve's Film Grain parameter set. Notably their
        // Shadow/Midtone/Highlight Gain trio replaces the single "shadow bias"
        // slider we had: three independent controls are strictly better than
        // one that slides a peak around, and it is how a colourist expects to
        // put grain in the midtones only.
        params: &[
            // Declaration order is the order the panel draws them, and it
            // follows Resolve's Film Grain: the format and how the grain
            // layer composites first, then the grain itself, then the
            // per-channel and per-tone trims.
            //
            // Slots follow this order too, so `grain.wgsl` has to move
            // with it.
            ParamDef {
                key: "preset",
                name: "Film Grain Presets",
                kind: ParamKind::Choice {
                    options: &["16mm", "35mm", "65mm", "Custom"],
                    default: "16mm",
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "composite",
                name: "Composite Type",
                kind: ParamKind::Choice {
                    options: &["Overlay", "Soft Light", "Add", "Screen"],
                    default: "Overlay",
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "opacity",
                name: "Opacity",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "grain_only",
                name: "Grain Only",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "texture",
                name: "Texture",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.704,
                    neutral: 0.704,
                },
                unit: "",
                section: "Grain Params",
            },
            ParamDef {
                key: "size",
                name: "Grain Size",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.66,
                    neutral: 0.66,
                },
                // Microns on a 35mm frame, not pixels. This is what makes the
                // 1200px preview and the 6000px export agree.
                unit: "µm",
                section: "Grain Params",
            },
            ParamDef {
                key: "aspect",
                name: "Grain Aspect Ratio",
                kind: ParamKind::Float {
                    min: 0.25,
                    max: 4.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Grain Params",
            },
            ParamDef {
                key: "strength",
                name: "Grain Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.149,
                    neutral: 0.0,
                },
                unit: "",
                section: "Grain Params",
            },
            ParamDef {
                key: "offset",
                name: "Offset",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.482,
                    neutral: 0.5,
                },
                unit: "",
                section: "Grain Params",
            },
            ParamDef {
                key: "symmetry",
                name: "Symmetry",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Grain Params",
            },
            ParamDef {
                key: "softness",
                name: "Softness",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.298,
                    neutral: 0.0,
                },
                unit: "",
                section: "Grain Params",
            },
            ParamDef {
                key: "saturation",
                name: "Saturation",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Grain Params",
            },
            ParamDef {
                key: "red",
                name: "Red",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Advanced Controls",
            },
            ParamDef {
                key: "green",
                name: "Green",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Advanced Controls",
            },
            ParamDef {
                key: "blue",
                name: "Blue",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Advanced Controls",
            },
            ParamDef {
                key: "shadow_gain",
                name: "Shadows",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Advanced Controls",
            },
            ParamDef {
                key: "midtone_gain",
                name: "Midtones",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Advanced Controls",
            },
            ParamDef {
                key: "highlight_gain",
                name: "Highlights",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Advanced Controls",
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
        gates: &[
            Gate {
                by: "fine_tune_spread",
                when: When::True,
                params: &["relative_red", "relative_green", "relative_blue"],
            },
            Gate {
                by: "secondary_strength",
                when: When::Positive,
                params: &["secondary_gamma", "secondary_spread", "secondary_filter"],
            },
            Gate {
                by: "append_grain",
                when: When::True,
                params: &[
                    "grain_strength",
                    "grain_size",
                    "grain_softness",
                    "grain_saturation",
                ],
            },
        ],
        // Follows Resolve's Halation structure. Two changes worth noting
        // against our M1 version: isolation is a *band* (Threshold is the low
        // clip, Normalization the high clip) rather than a single threshold,
        // and the glow has two independent layers. A secondary glow with its
        // own spread is what gives the effect a tight core and a wide falloff
        // at once, which one blur radius cannot do.
        params: &[
            // Resolve's five groups, in Resolve's order.
            //
            // Hue is gone. Resolve has no hue control here and it was right not to:
            // the red-orange is not a tint someone chose, it is what light
            // scattering back off the film base through the dye layers *is*. It is
            // a constant in the shader now, and Saturation says how much of it
            // reaches the picture — which is the control Resolve actually gives.
            // ---- Isolation ----
            // Threshold is the low clip and Normalization the high one, so the
            // source of the glow is a band rather than everything above a level.
            // That is what stops a bright sky glowing as hard as a specular.
            ParamDef {
                key: "threshold",
                name: "Threshold",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.2,
                    neutral: 0.2,
                },
                unit: "",
                section: "Isolation",
            },
            ParamDef {
                key: "normalization",
                name: "Normalization",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Isolation",
            },
            // How much a colour's saturation counts towards being isolated. A
            // saturated highlight halates harder than a neutral one of the same
            // brightness, because the dye layer it came through is denser.
            ParamDef {
                key: "film_saturation_level",
                name: "Film Saturation Level",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 10.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Isolation",
            },
            ParamDef {
                key: "view_isolated",
                name: "View Isolated Regions",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "Isolation",
            },
            // ---- Dye Layer Reflections ----
            ParamDef {
                key: "strength",
                name: "Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.0,
                },
                unit: "",
                section: "Dye Layer Reflections",
            },
            ParamDef {
                key: "gamma",
                name: "Gamma",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 3.0,
                    default: 1.35,
                    neutral: 1.0,
                },
                unit: "",
                section: "Dye Layer Reflections",
            },
            ParamDef {
                key: "saturation",
                name: "Saturation",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 3.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Dye Layer Reflections",
            },
            // Fraction of the frame, never pixels: a radius in pixels shrinks to
            // a rim on export.
            ParamDef {
                key: "spread",
                name: "Spread",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.333,
                    neutral: 0.0,
                },
                unit: "",
                section: "Dye Layer Reflections",
            },
            // With it off the glow is one radius wearing the dye colour. With it
            // on each channel scatters its own distance and the red fringe comes
            // out of the physics instead: longer wavelengths reach further into
            // the emulsion and scatter wider, so red spreads past green, which
            // spreads past blue.
            ParamDef {
                key: "fine_tune_spread",
                name: "Fine Tune Relative Spread",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "Dye Layer Reflections",
            },
            ParamDef {
                key: "relative_red",
                name: "Relative Red",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Fine Tune Spread",
            },
            ParamDef {
                key: "relative_green",
                name: "Relative Green",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 0.7,
                    neutral: 0.7,
                },
                unit: "",
                section: "Fine Tune Spread",
            },
            ParamDef {
                key: "relative_blue",
                name: "Relative Blue",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Fine Tune Spread",
            },
            // ---- Secondary Glow ----
            // Wider and weaker than the primary. Together they give a bright core
            // with a long falloff, which one radius cannot do at all.
            ParamDef {
                key: "secondary_strength",
                name: "Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Secondary Glow",
            },
            ParamDef {
                key: "secondary_gamma",
                name: "Gamma",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 3.0,
                    default: 1.35,
                    neutral: 1.0,
                },
                unit: "",
                section: "Secondary Glow",
            },
            ParamDef {
                key: "secondary_spread",
                name: "Spread",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.6,
                    neutral: 0.0,
                },
                unit: "",
                section: "Secondary Glow",
            },
            ParamDef {
                key: "secondary_filter",
                name: "Filter",
                kind: ParamKind::Rgb {
                    default: [0.5, 0.5, 0.5],
                },
                unit: "",
                section: "Secondary Glow",
            },
            // ---- Basic Grain ----
            // Grain applied inside the halation rather than after it. The order
            // matters: grain laid over a glow sits on top of it like dust on
            // glass, where grain inside the glow is in the emulsion the glow
            // happened in. Off by default, because most stacks already have a
            // Grain row and two lots of grain is one too many.
            ParamDef {
                key: "append_grain",
                name: "Append Grain Internally",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "Basic Grain",
            },
            ParamDef {
                key: "grain_strength",
                name: "Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.25,
                    neutral: 0.25,
                },
                unit: "",
                section: "Basic Grain",
            },
            ParamDef {
                key: "grain_size",
                name: "Size",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Basic Grain",
            },
            ParamDef {
                key: "grain_softness",
                name: "Softness",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.1,
                    neutral: 0.1,
                },
                unit: "",
                section: "Basic Grain",
            },
            ParamDef {
                key: "grain_saturation",
                name: "Saturation",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.15,
                    neutral: 0.15,
                },
                unit: "",
                section: "Basic Grain",
            },
            // ---- Global Adjustments ----
            ParamDef {
                key: "view_glow_alone",
                name: "View Glow Alone",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "Global Adjustments",
            },
            // The glow adds light, so without this the picture gets brighter as
            // well as glowier. Pulling the highlights back down is what makes the
            // effect read as scattering rather than as exposure.
            ParamDef {
                key: "reduce_highlights",
                name: "Reduce Highlights",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.0,
                },
                unit: "",
                section: "Global Adjustments",
            },
            ParamDef {
                key: "aspect_ratio",
                name: "Aspect Ratio",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 3.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Global Adjustments",
            },
            // Softens the picture under the glow, the way a halated frame loses
            // fine detail to the scattered light.
            ParamDef {
                key: "detail_loss",
                name: "Detail Loss",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Global Adjustments",
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
        gates: &[Gate {
            by: "operating_mode",
            when: When::Is("Advanced"),
            params: &["border_shape", "rotation", "center_x", "center_y"],
        }],
        params: &[
            // Resolve's panel, group for group. Two of its controls are absent on
            // purpose: Composite Type is the row's blend mode, which every effect
            // here already carries, and Use Alpha needs an alpha channel we do not.
            //
            // Amount is gone. Resolve's Basic set has no such control — the Global
            // Blend at the bottom of its panel is how you get a subtle vignette,
            // and that is this row's own Blend. Ours could also go *negative* to
            // brighten the corners, which is a real capability and not one Resolve
            // has; it is the price of the panel matching.
            ParamDef {
                key: "operating_mode",
                name: "Operating Mode",
                kind: ParamKind::Choice {
                    options: &["Basic", "Advanced"],
                    default: "Basic",
                },
                unit: "",
                section: "",
            },
            // Neutral is zero, not the default. A vignette of no size does nothing,
            // which is what makes an all-neutral row skippable — without that the
            // renderer would have to run a pass to draw a vignette nobody asked for.
            ParamDef {
                key: "size",
                name: "Size",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.0,
                },
                unit: "",
                section: "Shape",
            },
            // The shape of the frame the vignette is cut for, as a ratio: 1.0 is a
            // circle and 1.78 is 16:9, which is why that is where Resolve starts.
            ParamDef {
                key: "anamorphism",
                name: "Anamorphism",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 3.0,
                    default: 1.78,
                    neutral: 1.0,
                },
                unit: "",
                section: "Shape",
            },
            ParamDef {
                key: "softness",
                name: "Softness",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Appearance",
            },
            ParamDef {
                key: "color",
                name: "Color",
                kind: ParamKind::Rgb {
                    default: [0.0, 0.0, 0.0],
                },
                unit: "",
                section: "Appearance",
            },
            // Resolve's Advanced set. Dimmed in Basic, as they are there — kept
            // visible rather than hidden, so switching mode is a discovery rather
            // than a surprise.
            ParamDef {
                key: "border_shape",
                name: "Border Shape",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Advanced",
            },
            ParamDef {
                key: "rotation",
                name: "Rotation",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 360.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "°",
                section: "Advanced",
            },
            ParamDef {
                key: "center_x",
                name: "Center X",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Advanced",
            },
            ParamDef {
                key: "center_y",
                name: "Center Y",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Advanced",
            },
        ],
    },
    EffectDef {
        key: "cinematic_haze",
        name: "Cinematic Haze",
        group: Group::Optics,
        space: WorkingSpace::Linear,
        shader: "cinematic_haze",
        spatial: true,
        derived_slots: 0,
        gates: &[
            Gate {
                by: "adjust_levels",
                when: When::True,
                params: &["far_limit", "near_limit", "depth_gamma"],
            },
            Gate {
                by: "rays_enable",
                when: When::True,
                params: &[
                    "rays_preview",
                    "source_threshold",
                    "ray_directions",
                    "ray_angle",
                    "ray_length",
                    "ray_soften",
                    "ray_brightness",
                    "ray_saturation",
                ],
            },
            Gate {
                by: "disturbance_enable",
                when: When::True,
                params: &[
                    "disturbance_preview",
                    "intensity",
                    "disturbance_brightness",
                    "disturbance_scale",
                    "disturbance_detail",
                    "start_frame",
                ],
            },
        ],
        params: &[
            // Resolve's panel, group for group, less the parts a photograph cannot
            // have. Colour Space Overrides is the renderer's decision under the
            // two-space rule. Depth Map Source is a dropdown with one option here —
            // there is no external depth input to choose — and a dropdown with one
            // option is a dead control. Advanced Depth Controls reveals controls no
            // screenshot we have shows, and a switch that reveals nothing is worse
            // than a missing one.
            // ---- Depth Map ----
            ParamDef {
                key: "depth_preview",
                name: "Depth Map Preview",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "Depth Map",
            },
            ParamDef {
                key: "quality",
                name: "Quality",
                kind: ParamKind::Choice {
                    options: &["Faster", "Better", "Best"],
                    default: "Better",
                },
                unit: "",
                section: "Depth Map",
            },
            // The estimate reads *haze*, and haze means far — so the raw map is
            // already a distance. Invert is on by default because Resolve's is, and
            // because the useful convention is near = 1.
            ParamDef {
                key: "invert",
                name: "Invert",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Depth Map",
            },
            ParamDef {
                key: "adjust_levels",
                name: "Adjust Map Levels",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Depth Map",
            },
            ParamDef {
                key: "far_limit",
                name: "Far Limit",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Depth Map",
            },
            ParamDef {
                key: "near_limit",
                name: "Near Limit",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Depth Map",
            },
            ParamDef {
                key: "depth_gamma",
                name: "Gamma",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Depth Map",
            },
            // ---- Atmospheric Scattering ----
            // The physics of the whole effect, in two numbers. Airlight is how
            // bright the scattered light is; Density is how much of it there is per
            // unit of distance. Together they are the standard atmospheric model,
            // which is also — read backwards — what Dehaze undoes.
            ParamDef {
                key: "airlight",
                name: "Airlight",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.4,
                    neutral: 0.0,
                },
                unit: "",
                section: "Atmospheric Scattering",
            },
            ParamDef {
                key: "density",
                name: "Density",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.1,
                    neutral: 0.0,
                },
                unit: "",
                section: "Atmospheric Scattering",
            },
            // Distance costs detail as well as contrast. Without this the far
            // hillside is a flat wash at full sharpness, which reads as a filter
            // over the picture rather than as air in front of it.
            ParamDef {
                key: "resolution_loss",
                name: "Resolution Loss",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.0,
                },
                unit: "",
                section: "Atmospheric Scattering",
            },
            ParamDef {
                key: "scatter_colorize",
                name: "Colorize",
                kind: ParamKind::Rgb {
                    default: [1.0, 1.0, 1.0],
                },
                unit: "",
                section: "Atmospheric Scattering",
            },
            // ---- Light Halos ----
            // A bright thing seen through air acquires a halo, and a *distant*
            // bright thing acquires a bigger one — which is why this is in the same
            // effect as the depth map rather than being Bloom again.
            ParamDef {
                key: "halo_threshold",
                name: "Halo Threshold",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.65,
                    neutral: 0.65,
                },
                unit: "",
                section: "Light Halos",
            },
            ParamDef {
                key: "halo_size",
                name: "Size",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Light Halos",
            },
            ParamDef {
                key: "halo_brightness",
                name: "Brightness",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.25,
                    neutral: 0.0,
                },
                unit: "",
                section: "Light Halos",
            },
            ParamDef {
                key: "halo_saturation",
                name: "Saturation",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Light Halos",
            },
            ParamDef {
                key: "halo_colorize",
                name: "Colorize",
                kind: ParamKind::Rgb {
                    default: [1.0, 1.0, 1.0],
                },
                unit: "",
                section: "Light Halos",
            },
            // ---- Light Rays ----
            ParamDef {
                key: "rays_enable",
                name: "Enable Light Rays",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "Light Rays",
            },
            ParamDef {
                key: "rays_preview",
                name: "Preview Threshold",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "Light Rays",
            },
            ParamDef {
                key: "source_threshold",
                name: "Source Threshold",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.7,
                    neutral: 0.7,
                },
                unit: "",
                section: "Light Rays",
            },
            ParamDef {
                key: "ray_directions",
                name: "Ray Directions",
                kind: ParamKind::Choice {
                    options: &["At An Angle", "Radial"],
                    default: "At An Angle",
                },
                unit: "",
                section: "Light Rays",
            },
            ParamDef {
                key: "ray_angle",
                name: "Angle",
                kind: ParamKind::Float {
                    min: -180.0,
                    max: 180.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "°",
                section: "Light Rays",
            },
            ParamDef {
                key: "ray_length",
                name: "Length",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.75,
                    neutral: 0.75,
                },
                unit: "",
                section: "Light Rays",
            },
            ParamDef {
                key: "ray_soften",
                name: "Soften",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.15,
                    neutral: 0.15,
                },
                unit: "",
                section: "Light Rays",
            },
            ParamDef {
                key: "ray_brightness",
                name: "Brightness",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.3,
                    neutral: 0.0,
                },
                unit: "",
                section: "Light Rays",
            },
            ParamDef {
                key: "ray_saturation",
                name: "Saturation",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Light Rays",
            },
            // ---- Air Disturbance ----
            // Four of Resolve's controls here are absent: Follow FX Tracker needs a
            // tracker, and Flow Speed, Seethe Rate and Randomize Start Frame all
            // describe how the field changes between exposures. Start Frame stays,
            // because for one frame it is not a time at all — it is which slice of
            // the field you get, which is a seed.
            ParamDef {
                key: "disturbance_enable",
                name: "Enable Disturbance",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "Air Disturbance",
            },
            ParamDef {
                key: "disturbance_preview",
                name: "Preview Influence",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "Air Disturbance",
            },
            ParamDef {
                key: "intensity",
                name: "Intensity",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.25,
                    neutral: 0.0,
                },
                unit: "",
                section: "Air Disturbance",
            },
            ParamDef {
                key: "disturbance_brightness",
                name: "Brightness",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Air Disturbance",
            },
            ParamDef {
                key: "disturbance_scale",
                name: "Scale",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 6.0,
                    default: 2.0,
                    neutral: 2.0,
                },
                unit: "",
                section: "Air Disturbance",
            },
            ParamDef {
                key: "disturbance_detail",
                name: "Detail",
                kind: ParamKind::Float {
                    min: 1.0,
                    max: 16.0,
                    default: 7.0,
                    neutral: 7.0,
                },
                unit: "",
                section: "Air Disturbance",
            },
            ParamDef {
                key: "start_frame",
                name: "Start Frame",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1000.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Air Disturbance",
            },
        ],
    },
    EffectDef {
        key: "dehaze",
        name: "Dehaze",
        group: Group::Optics,
        space: WorkingSpace::Linear,
        shader: "dehaze",
        spatial: true,
        derived_slots: 0,
        gates: &[],
        params: &[
            // Bipolar like Resolve's: above zero removes haze, below zero adds
            // it by running the same scattering model forwards.
            // Declaration order is the order the panel draws them, so it
            // follows Resolve's: strength, the colour being removed, the depth
            // view, then the two tonal trims.
            //
            // Arrives at 0.8, because you add a Dehaze row to remove haze.
            // Neutral is still zero, so reset means "do nothing" as it does
            // everywhere else.
            ParamDef {
                key: "strength",
                name: "Dehaze Strength",
                kind: ParamKind::Float {
                    min: -1.0,
                    max: 1.0,
                    default: 0.209,
                    neutral: 0.0,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "haze_color",
                name: "Haze Color",
                // White, as Resolve ships it. Atmospheric scattering is
                // usually a little blue-grey, but starting neutral means the
                // control removes whatever haze the picture has rather than
                // the haze the default assumed it had.
                kind: ParamKind::Rgb {
                    default: [1.0, 1.0, 1.0],
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "display_depth",
                name: "Display Depth",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "",
            },
            bipolar("shadow", "Shadow", 1.0, ""),
            bipolar("highlight", "Highlight", 1.0, ""),
        ],
    },
    EffectDef {
        key: "bloom",
        name: "Bloom",
        group: Group::Film,
        space: WorkingSpace::Linear,
        shader: "bloom",
        spatial: true,
        derived_slots: 0,
        gates: &[],
        params: &[
            ParamDef {
                key: "amount",
                name: "Amount",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.4,
                    neutral: 0.0,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "radius",
                name: "Radius",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.3,
                    default: 0.06,
                    neutral: 0.06,
                },
                unit: "",
                section: "",
            },
            // Linear light again: 1.0 is diffuse white and an SDR photo never
            // passes it. The same trap Halation had.
            ParamDef {
                key: "threshold",
                name: "Threshold",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 4.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "",
            },
        ],
    },
    EffectDef {
        key: "film_damage",
        name: "Film Damage",
        group: Group::Film,
        space: WorkingSpace::Linear,
        shader: "film_damage",
        spatial: true,
        derived_slots: 0,
        gates: &[],
        // Parameter order is the shader ABI — see the slot table at the top
        // of shaders/effects/film_damage.wgsl. Reordering this array quietly
        // rewires every saved document that uses the effect.
        params: &[
            ParamDef {
                key: "film_blur",
                name: "Film Blur",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "",
            },
            // Positive warms, simulating a projector bulb running hot;
            // positive tint yellows, simulating dye failure.
            ParamDef {
                key: "temp_shift",
                name: "Temp. Shift",
                kind: ParamKind::Float {
                    min: -1.0,
                    max: 1.0,
                    default: 0.25,
                    neutral: 0.0,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "tint_shift",
                name: "Tint Shift",
                kind: ParamKind::Float {
                    min: -1.0,
                    max: 1.0,
                    default: -0.1,
                    neutral: 0.0,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "focal_factor",
                name: "Focal Factor",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.1,
                    neutral: 0.0,
                },
                unit: "",
                section: "Add Vignetting",
            },
            ParamDef {
                key: "geometry_factor",
                name: "Geometry Factor",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.25,
                    neutral: 0.5,
                },
                unit: "",
                section: "Add Vignetting",
            },
            ParamDef {
                key: "tilt_amount",
                name: "Tilt Amount",
                kind: ParamKind::Float {
                    min: -1.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Add Vignetting",
            },
            ParamDef {
                key: "tilt_angle",
                name: "Tilt Angle",
                kind: ParamKind::Float {
                    min: -180.0,
                    max: 180.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "°",
                section: "Add Vignetting",
            },
            ParamDef {
                key: "dirt_density",
                name: "Dirt Density",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 10.0,
                    default: 2.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Add Dirt",
            },
            ParamDef {
                key: "dirt_size",
                name: "Dirt Size",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 8.0,
                    default: 1.758,
                    neutral: 1.758,
                },
                unit: "",
                section: "Add Dirt",
            },
            ParamDef {
                key: "dirt_blur",
                name: "Dirt Blur",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.245,
                    neutral: 0.245,
                },
                unit: "",
                section: "Add Dirt",
            },
            ParamDef {
                key: "dirt_seed",
                name: "Dirt Seed",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 10.0,
                    default: 5.0,
                    neutral: 5.0,
                },
                unit: "",
                section: "Add Dirt",
            },
            ParamDef {
                key: "dirt_color",
                name: "Dirt Color",
                // Black reads as dirt on a print, white as dirt on a negative.
                kind: ParamKind::Rgb {
                    default: [0.0, 0.0, 0.0],
                },
                unit: "",
                section: "Add Dirt",
            },
            // Five independent scratches, matching Resolve. A count
            // parameter could not place them, and placement is most of
            // what makes damage read as real rather than procedural.
            //
            // Each carries its own colour and blur. Resolve does the same,
            // and it is not decoration: a strip of film that has been
            // through a projector has a sharp black gouge on the negative
            // and a soft white one on the print, and one shared colour
            // cannot say both.
            ParamDef {
                key: "scratch1_enable",
                name: "Enable",
                // All five are enabled; strength is what decides whether one
                // is visible, and it starts at zero for all but the first.
                //
                // The alternative — four of them switched off — means turning
                // up Scratch 3's strength does nothing, with no hint as to
                // why. Enable is for silencing a scratch you have set up, not
                // for gating one you have not.
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Add Scratch 1",
            },
            ParamDef {
                key: "scratch1_color",
                name: "Scratch Color",
                // White reads as emulsion gouged off a print, black as a
                // scratch on the negative. Per scratch, because a real strip
                // of film has both.
                kind: ParamKind::Rgb {
                    default: [0.0, 0.0, 0.0],
                },
                unit: "",
                section: "Add Scratch 1",
            },
            ParamDef {
                key: "scratch1_position",
                name: "Scratch Position",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.159,
                    neutral: 0.159,
                },
                unit: "",
                section: "Add Scratch 1",
            },
            ParamDef {
                key: "scratch1_width",
                name: "Scratch Width",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.5,
                    default: 0.044,
                    neutral: 0.044,
                },
                unit: "",
                section: "Add Scratch 1",
            },
            ParamDef {
                key: "scratch1_strength",
                name: "Scratch Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.0,
                },
                unit: "",
                section: "Add Scratch 1",
            },
            ParamDef {
                key: "scratch1_blur",
                name: "Scratch Blur",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.35,
                    neutral: 0.35,
                },
                unit: "",
                section: "Add Scratch 1",
            },
            ParamDef {
                key: "scratch2_enable",
                name: "Enable",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Add Scratch 2",
            },
            ParamDef {
                key: "scratch2_color",
                name: "Scratch Color",
                // White reads as emulsion gouged off a print, black as a
                // scratch on the negative. Per scratch, because a real strip
                // of film has both.
                kind: ParamKind::Rgb {
                    default: [0.0, 0.0, 0.0],
                },
                unit: "",
                section: "Add Scratch 2",
            },
            ParamDef {
                key: "scratch2_position",
                name: "Scratch Position",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.35,
                    neutral: 0.35,
                },
                unit: "",
                section: "Add Scratch 2",
            },
            ParamDef {
                key: "scratch2_width",
                name: "Scratch Width",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.5,
                    default: 0.044,
                    neutral: 0.044,
                },
                unit: "",
                section: "Add Scratch 2",
            },
            ParamDef {
                key: "scratch2_strength",
                name: "Scratch Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Add Scratch 2",
            },
            ParamDef {
                key: "scratch2_blur",
                name: "Scratch Blur",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.35,
                    neutral: 0.35,
                },
                unit: "",
                section: "Add Scratch 2",
            },
            ParamDef {
                key: "scratch3_enable",
                name: "Enable",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Add Scratch 3",
            },
            ParamDef {
                key: "scratch3_color",
                name: "Scratch Color",
                // White reads as emulsion gouged off a print, black as a
                // scratch on the negative. Per scratch, because a real strip
                // of film has both.
                kind: ParamKind::Rgb {
                    default: [0.0, 0.0, 0.0],
                },
                unit: "",
                section: "Add Scratch 3",
            },
            ParamDef {
                key: "scratch3_position",
                name: "Scratch Position",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Add Scratch 3",
            },
            ParamDef {
                key: "scratch3_width",
                name: "Scratch Width",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.5,
                    default: 0.044,
                    neutral: 0.044,
                },
                unit: "",
                section: "Add Scratch 3",
            },
            ParamDef {
                key: "scratch3_strength",
                name: "Scratch Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Add Scratch 3",
            },
            ParamDef {
                key: "scratch3_blur",
                name: "Scratch Blur",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.35,
                    neutral: 0.35,
                },
                unit: "",
                section: "Add Scratch 3",
            },
            ParamDef {
                key: "scratch4_enable",
                name: "Enable",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Add Scratch 4",
            },
            ParamDef {
                key: "scratch4_color",
                name: "Scratch Color",
                // White reads as emulsion gouged off a print, black as a
                // scratch on the negative. Per scratch, because a real strip
                // of film has both.
                kind: ParamKind::Rgb {
                    default: [0.0, 0.0, 0.0],
                },
                unit: "",
                section: "Add Scratch 4",
            },
            ParamDef {
                key: "scratch4_position",
                name: "Scratch Position",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.65,
                    neutral: 0.65,
                },
                unit: "",
                section: "Add Scratch 4",
            },
            ParamDef {
                key: "scratch4_width",
                name: "Scratch Width",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.5,
                    default: 0.044,
                    neutral: 0.044,
                },
                unit: "",
                section: "Add Scratch 4",
            },
            ParamDef {
                key: "scratch4_strength",
                name: "Scratch Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Add Scratch 4",
            },
            ParamDef {
                key: "scratch4_blur",
                name: "Scratch Blur",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.35,
                    neutral: 0.35,
                },
                unit: "",
                section: "Add Scratch 4",
            },
            ParamDef {
                key: "scratch5_enable",
                name: "Enable",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Add Scratch 5",
            },
            ParamDef {
                key: "scratch5_color",
                name: "Scratch Color",
                // White reads as emulsion gouged off a print, black as a
                // scratch on the negative. Per scratch, because a real strip
                // of film has both.
                kind: ParamKind::Rgb {
                    default: [0.0, 0.0, 0.0],
                },
                unit: "",
                section: "Add Scratch 5",
            },
            ParamDef {
                key: "scratch5_position",
                name: "Scratch Position",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.8,
                    neutral: 0.8,
                },
                unit: "",
                section: "Add Scratch 5",
            },
            ParamDef {
                key: "scratch5_width",
                name: "Scratch Width",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.5,
                    default: 0.044,
                    neutral: 0.044,
                },
                unit: "",
                section: "Add Scratch 5",
            },
            ParamDef {
                key: "scratch5_strength",
                name: "Scratch Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Add Scratch 5",
            },
            ParamDef {
                key: "scratch5_blur",
                name: "Scratch Blur",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.35,
                    neutral: 0.35,
                },
                unit: "",
                section: "Add Scratch 5",
            },
        ],
    },
    // --- Lightroom's Basic panel ------------------------------------------
    // These three fill the gaps between the effects that already existed, so
    // one Basic panel can drive the whole set. The panel spans both working
    // spaces, which is why it is several rows rather than one effect.
    EffectDef {
        key: "tone",
        name: "Tone",
        group: Group::Basic,
        space: WorkingSpace::Log,
        shader: "tone",
        spatial: false,
        derived_slots: 0,
        gates: &[],
        params: &[
            bipolar("highlights", "Highlights", 1.0, ""),
            bipolar("shadows", "Shadows", 1.0, ""),
            bipolar("whites", "Whites", 1.0, ""),
            bipolar("blacks", "Blacks", 1.0, ""),
        ],
    },
    EffectDef {
        key: "presence",
        name: "Presence",
        group: Group::Basic,
        space: WorkingSpace::Linear,
        shader: "presence",
        // Local contrast: it reads the neighbourhood, so its radii are
        // frame-relative and it cannot be fused with its neighbours.
        spatial: true,
        derived_slots: 0,
        gates: &[],
        params: &[
            bipolar("texture", "Texture", 1.0, ""),
            bipolar("clarity", "Clarity", 1.0, ""),
        ],
    },
    EffectDef {
        key: "colour",
        name: "Colour",
        group: Group::Basic,
        space: WorkingSpace::Log,
        shader: "colour",
        spatial: false,
        derived_slots: 0,
        gates: &[],
        params: &[
            bipolar("vibrance", "Vibrance", 1.0, ""),
            bipolar("saturation", "Saturation", 1.0, ""),
            // A global hue rotation, in degrees. Resolve's Primaries panel
            // shows it as 0 to 100 with 50 neutral; the document stores the
            // rotation itself, which is the thing that has a meaning.
            bipolar("hue", "Hue", 180.0, "°"),
            // How much of a saturation or hue move is allowed to change how
            // bright the pixel is.
            //
            // At zero, pushing saturation leaves luminance exactly where it
            // was — which is the whole reason the control exists, because
            // saturating a face otherwise darkens it and the correction for
            // that costs another control.
            ParamDef {
                key: "lum_mix",
                name: "Lum Mix",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "",
            },
        ],
    },
    EffectDef {
        key: "log_wheels",
        name: "Log Wheels",
        group: Group::Color,
        space: WorkingSpace::Log,
        shader: "log_wheels",
        spatial: false,
        derived_slots: 0,
        gates: &[],
        params: &[
            ParamDef {
                key: "shadow",
                name: "Shadow",
                kind: ParamKind::Wheel,
                unit: "",
                section: "",
            },
            ParamDef {
                key: "midtone",
                name: "Midtone",
                kind: ParamKind::Wheel,
                unit: "",
                section: "",
            },
            ParamDef {
                key: "highlight",
                name: "Highlight",
                kind: ParamKind::Wheel,
                unit: "",
                section: "",
            },
            ParamDef {
                key: "offset",
                name: "Offset",
                kind: ParamKind::Wheel,
                unit: "",
                section: "",
            },
            // Where "shadow" stops and "highlight" starts. Defaults sit
            // either side of 18% grey (0.4136 in ACEScct), which is where a
            // colourist would put them by hand.
            ParamDef {
                key: "low_range",
                name: "Low Range",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.30,
                    neutral: 0.30,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "high_range",
                name: "High Range",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.48,
                    neutral: 0.48,
                },
                unit: "",
                section: "",
            },
        ],
    },
    EffectDef {
        key: "colour_mixer",
        name: "Colour Mixer",
        group: Group::Color,
        space: WorkingSpace::Log,
        shader: "colour_mixer",
        spatial: false,
        derived_slots: 0,
        gates: &[],
        // Three slots per band, in band order — the shader indexes them
        // arithmetically, so this order is load-bearing.
        params: &[
            // Red
            bipolar("red_hue", "Red Hue", 1.0, ""),
            bipolar("red_saturation", "Red Saturation", 1.0, ""),
            bipolar("red_luminance", "Red Luminance", 1.0, ""),
            // Orange
            bipolar("orange_hue", "Orange Hue", 1.0, ""),
            bipolar("orange_saturation", "Orange Saturation", 1.0, ""),
            bipolar("orange_luminance", "Orange Luminance", 1.0, ""),
            // Yellow
            bipolar("yellow_hue", "Yellow Hue", 1.0, ""),
            bipolar("yellow_saturation", "Yellow Saturation", 1.0, ""),
            bipolar("yellow_luminance", "Yellow Luminance", 1.0, ""),
            // Green
            bipolar("green_hue", "Green Hue", 1.0, ""),
            bipolar("green_saturation", "Green Saturation", 1.0, ""),
            bipolar("green_luminance", "Green Luminance", 1.0, ""),
            // Aqua
            bipolar("aqua_hue", "Aqua Hue", 1.0, ""),
            bipolar("aqua_saturation", "Aqua Saturation", 1.0, ""),
            bipolar("aqua_luminance", "Aqua Luminance", 1.0, ""),
            // Blue
            bipolar("blue_hue", "Blue Hue", 1.0, ""),
            bipolar("blue_saturation", "Blue Saturation", 1.0, ""),
            bipolar("blue_luminance", "Blue Luminance", 1.0, ""),
            // Purple
            bipolar("purple_hue", "Purple Hue", 1.0, ""),
            bipolar("purple_saturation", "Purple Saturation", 1.0, ""),
            bipolar("purple_luminance", "Purple Luminance", 1.0, ""),
            // Magenta
            bipolar("magenta_hue", "Magenta Hue", 1.0, ""),
            bipolar("magenta_saturation", "Magenta Saturation", 1.0, ""),
            bipolar("magenta_luminance", "Magenta Luminance", 1.0, ""),
        ],
    },
    // Resolve's Color Stabilizer, less the half that needs a second frame.
    // What is left — measure a region, correct it to neutral — is the
    // eyedropper every editor has, with the region and the strength exposed
    // instead of hidden.
    EffectDef {
        key: "color_stabilizer",
        name: "Color Stabilizer",
        group: Group::Color,
        space: WorkingSpace::Linear,
        shader: "color_stabilizer",
        // It reads a region rather than only the pixel under it.
        spatial: true,
        derived_slots: 0,
        gates: &[],
        params: &[
            ParamDef {
                key: "region",
                name: "Region Of Analysis",
                kind: ParamKind::Choice {
                    options: &["Selected Area", "Entire Frame"],
                    default: "Selected Area",
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "source_x",
                name: "Source X Position",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Analysis Region",
            },
            ParamDef {
                key: "source_y",
                name: "Source Y Position",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Analysis Region",
            },
            ParamDef {
                key: "source_width",
                name: "Source Width",
                kind: ParamKind::Float {
                    min: 0.01,
                    max: 1.0,
                    default: 0.2,
                    neutral: 0.2,
                },
                unit: "",
                section: "Analysis Region",
            },
            ParamDef {
                key: "source_height",
                name: "Source Height",
                kind: ParamKind::Float {
                    min: 0.01,
                    max: 1.0,
                    default: 0.25,
                    neutral: 0.25,
                },
                unit: "",
                section: "Analysis Region",
            },
            // Resolve's Mode, which is the control. The two checkboxes
            // beside it there — Stabilize White Balance and Stabilize
            // Brightness — are greyed out and read back what Mode has chosen,
            // so they are a readout rather than a second way to set it.
            ParamDef {
                key: "mode",
                name: "Mode",
                kind: ParamKind::Choice {
                    options: &["Balance, Brightness", "Balance", "Brightness"],
                    default: "Balance, Brightness",
                },
                unit: "",
                section: "Stabilization",
            },
            ParamDef {
                key: "stabilize",
                name: "Stabilize",
                kind: ParamKind::Choice {
                    options: &["Levels", "Levels and Contrast"],
                    default: "Levels and Contrast",
                },
                unit: "",
                section: "Stabilization",
            },
            // Starts at zero, and that is the gate.
            //
            // Resolve does nothing until you press Analyze Now. Our
            // measurement is live, so there is nothing to press — but a
            // corrective tool that changes the picture the moment it is added
            // is surprising, and every other corrective effect here starts
            // neutral. Dragging this up applies the correction gradually,
            // which is a better control than a button anyway.
            ParamDef {
                key: "strength",
                name: "Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Stabilization",
            },
        ],
    },
    // A rotational blur: every pixel smeared along the arc it would sweep if
    // the picture spun about a point.
    EffectDef {
        key: "radial_blur",
        name: "Radial Blur",
        group: Group::Optics,
        space: WorkingSpace::Linear,
        shader: "radial_blur",
        spatial: true,
        derived_slots: 0,
        gates: &[],
        params: &[
            // Resolve's panel. Alpha is absent from Channel Adjustment and Use
            // Alpha from the bottom, because there is no alpha channel here;
            // Global Blend is the row's own Blend.
            ParamDef {
                key: "strength",
                name: "Smooth Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.4,
                    neutral: 0.0,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "blur_type",
                name: "Blur Type",
                kind: ParamKind::Choice {
                    options: &["Realistic", "Even"],
                    default: "Realistic",
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "symmetry",
                name: "Blur Symmetry",
                kind: ParamKind::Choice {
                    options: &["Symmetric", "Asymmetric"],
                    default: "Symmetric",
                },
                unit: "",
                section: "",
            },
            // Each channel mixes between sharp and blurred on its own, which is how
            // this effect makes a chromatic streak rather than plain motion. The
            // range runs past one on purpose, as Resolve's does: above it the mix
            // extrapolates, pushing the channel further from the original than the
            // blur itself went.
            ParamDef {
                key: "red",
                name: "Red",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Channel Adjustment",
            },
            ParamDef {
                key: "green",
                name: "Green",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Channel Adjustment",
            },
            ParamDef {
                key: "blue",
                name: "Blue",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Channel Adjustment",
            },
            ParamDef {
                key: "center_x",
                name: "Position X",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Center Position",
            },
            ParamDef {
                key: "center_y",
                name: "Position Y",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Center Position",
            },
            ParamDef {
                key: "quality",
                name: "Quality",
                kind: ParamKind::Choice {
                    options: &["Faster", "Better", "Best"],
                    default: "Better",
                },
                unit: "",
                section: "Advanced Controls",
            },
            // What to read where a sample lands outside the picture. It matters
            // more than it looks: a rotational blur near a corner reaches past the
            // edge on every sample, so this is most of what the corner becomes.
            ParamDef {
                key: "border",
                name: "Border Type",
                kind: ParamKind::Choice {
                    options: &["Replicate", "Mirror", "Wrap", "Black"],
                    default: "Replicate",
                },
                unit: "",
                section: "Advanced Controls",
            },
            // The centre belongs to the photograph, so it stays put when the picture
            // is cropped, panned or zoomed. Turn it off and the centre belongs to the
            // *output* instead: the blur stays where it is on screen while the picture
            // moves under it.
            ParamDef {
                key: "move_with_sizing",
                name: "Move With Sizing",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Advanced Controls",
            },
        ],
    },
    // The same idea along the radius instead of the arc.
    EffectDef {
        key: "zoom_blur",
        name: "Zoom Blur",
        group: Group::Optics,
        space: WorkingSpace::Linear,
        shader: "zoom_blur",
        spatial: true,
        derived_slots: 0,
        gates: &[],
        params: &[
            // Resolve's panel. Two of Radial Blur's controls are absent because
            // they are absent there too: Zoom Blur has no Blur Symmetry, and its
            // Border Type is greyed out — permanently, not conditionally — so
            // carrying it would be carrying a control that cannot do anything.
            ParamDef {
                key: "strength",
                name: "Zoom Amount",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.4,
                    neutral: 0.0,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "blur_type",
                name: "Blur Type",
                kind: ParamKind::Choice {
                    options: &["Realistic", "Even"],
                    default: "Realistic",
                },
                unit: "",
                section: "",
            },
            // A disc around the centre that stays sharp. The classic use of a zoom
            // blur is speed behind a subject that is still readable, and without
            // this the subject sits at the one point where the blur is weakest
            // rather than at a point where it is absent.
            ParamDef {
                key: "center_exclusion",
                name: "Center Exclusion",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "",
            },
            // Each channel mixes between sharp and blurred on its own, which is how
            // this effect makes a chromatic streak rather than plain motion. The
            // range runs past one on purpose, as Resolve's does: above it the mix
            // extrapolates, pushing the channel further from the original than the
            // blur itself went.
            ParamDef {
                key: "red",
                name: "Red",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Channel Adjustment",
            },
            ParamDef {
                key: "green",
                name: "Green",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Channel Adjustment",
            },
            ParamDef {
                key: "blue",
                name: "Blue",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Channel Adjustment",
            },
            ParamDef {
                key: "center_x",
                name: "Position X",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Center Position",
            },
            ParamDef {
                key: "center_y",
                name: "Position Y",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Center Position",
            },
            ParamDef {
                key: "quality",
                name: "Quality",
                kind: ParamKind::Choice {
                    options: &["Faster", "Better", "Best"],
                    default: "Better",
                },
                unit: "",
                section: "Advanced Controls",
            },
            // The centre belongs to the photograph, so it stays put when the picture
            // is cropped, panned or zoomed. Turn it off and the centre belongs to the
            // *output* instead: the blur stays where it is on screen while the picture
            // moves under it.
            ParamDef {
                key: "move_with_sizing",
                name: "Move With Sizing",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Advanced Controls",
            },
        ],
    },
    // The spatial half of Resolve's Noise Reduction. The temporal half
    // compares a frame against its neighbours, and there is no next frame
    // here — but the spatial half is the one that works on a photograph
    // anyway.
    EffectDef {
        key: "noise_reduction",
        name: "Noise Reduction",
        group: Group::Optics,
        space: WorkingSpace::Linear,
        shader: "noise_reduction",
        spatial: true,
        derived_slots: 0,
        gates: &[
            Gate {
                by: "split_luma_chroma",
                when: When::True,
                params: &["luma_threshold", "chroma_threshold"],
            },
            Gate {
                by: "split_luma_chroma",
                when: When::False,
                params: &["threshold"],
            },
        ],
        params: &[
            // Resolve's spatial half. The temporal half — Frames Either Side, Mo.
            // Est. Type, Motion Range, and the whole Temporal Threshold group —
            // compares a frame against its neighbours, and a photograph has no
            // neighbours. Same reasoning that dropped Film Damage's Changing Dirt
            // and Film Look Creator's Gate Weave.
            ParamDef {
                key: "mode",
                name: "Mode",
                kind: ParamKind::Choice {
                    options: &["Faster", "Better", "Enhanced"],
                    default: "Faster",
                },
                unit: "",
                section: "Spatial NR",
            },
            // A dropdown in Resolve, not a slider, and the sizes are fractions of
            // the frame rather than pixels — a 1200px preview and a 6000px export
            // have to smooth the same real detail or what you approve is not what
            // you get.
            ParamDef {
                key: "radius",
                name: "Radius",
                kind: ParamKind::Choice {
                    options: &["Small", "Medium", "Large"],
                    default: "Small",
                },
                unit: "",
                section: "Spatial NR",
            },
            // Luma and chroma carry different noise. Chroma noise is coarse, ugly
            // and almost free to remove — the eye has little colour acuity, so it
            // can be smoothed hard before anything is lost. Luma noise is fine and
            // sits on top of real detail, so the same treatment takes the detail
            // with it. One threshold for both means choosing which of those two
            // mistakes to make, which is exactly why this box exists.
            ParamDef {
                key: "split_luma_chroma",
                name: "Split Luma Chroma",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "Spatial Threshold",
            },
            ParamDef {
                key: "threshold",
                name: "Threshold",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 100.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Spatial Threshold",
            },
            ParamDef {
                key: "luma_threshold",
                name: "Luma Threshold",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 100.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Spatial Threshold",
            },
            ParamDef {
                key: "chroma_threshold",
                name: "Chroma Threshold",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 100.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Spatial Threshold",
            },
            // How much of the original is blended back over the cleaned picture.
            // Zero is the full effect, which is why zero is the default: the
            // control that decides whether anything happens is the threshold above.
            ParamDef {
                key: "blend",
                name: "Blend",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 100.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Spatial Threshold",
            },
        ],
    },
    // The response half of Resolve's Film Look Creator: what a stock does to
    // the light. The other half — halation, grain, bloom, vignette — is a set
    // of rows in this application already, and writing them again in here
    // would mean two implementations of halation that drift apart the first
    // time one is fixed.
    //
    // Visible at its defaults; see EFFECTS_WITH_VISIBLE_DEFAULTS.
    EffectDef {
        key: "film_look",
        name: "Film Look Creator",
        group: Group::Film,
        space: WorkingSpace::Log,
        shader: "film_look",
        spatial: true,
        derived_slots: 0,
        gates: &[
            Gate {
                by: "split_tone_enable",
                when: When::True,
                params: &[
                    "split_tone_mode",
                    "split_tone_amount",
                    "split_tone_hue",
                    "split_tone_pivot",
                ],
            },
            Gate {
                by: "vignette_enable",
                when: When::True,
                params: &["vignette_amount", "vignette_size"],
            },
            Gate {
                by: "halation_enable",
                when: When::True,
                params: &[
                    "halation_highlights_only",
                    "halation_amount",
                    "halation_radius",
                    "halation_saturation",
                    "halation_hue",
                ],
            },
            Gate {
                by: "bloom_enable",
                when: When::True,
                params: &["bloom_amount", "bloom_radius"],
            },
            Gate {
                by: "grain_enable",
                when: When::True,
                params: &[
                    "grain_preset",
                    "grain_amount",
                    "grain_size",
                    "grain_softness",
                    "grain_saturation",
                    "image_defocus",
                ],
            },
            Gate {
                by: "gate_enable",
                when: When::True,
                params: &[
                    "gate_preset",
                    "gate_ratio_h",
                    "gate_ratio_v",
                    "gate_curvature",
                    "gate_padding",
                ],
            },
        ],
        params: &[
            // Resolve's Film Look Creator, group for group.
            //
            // It is a *bundle*: a film response, then the five things a film print
            // does on the way to the screen. Every one of those five is also a row
            // of its own in this application, which was the argument for not having
            // them here — two implementations of halation drift apart the moment one
            // is fixed, and it is always the forgotten one that is wrong.
            //
            // So there is still one implementation. The gathers, the vignette
            // falloff and the grain lattice live in common.wgsl and both callers
            // reach for the same function. What this effect adds is Resolve's
            // *arrangement* of them — one row that produces a coherent look,
            // instead of five rows the user has to balance by hand.
            //
            // Two of Resolve's groups are missing and will stay missing. Flicker and
            // Gate Weave describe what the frame does between exposures, and a
            // photograph has no next frame.
            ParamDef {
                key: "preset",
                name: "Presets",
                kind: ParamKind::Choice {
                    options: &[
                        "Default 65mm",
                        "Default 35mm",
                        "Default 16mm",
                        "Default Super 8",
                    ],
                    default: "Default 65mm",
                },
                unit: "",
                section: "",
            },
            // The two halves, blended separately against the input. Colour Blend
            // holds the response and the grade; Effects Blend holds everything
            // spatial. Being able to keep the stock and drop the grain is most of
            // why they are two controls and not one.
            ParamDef {
                key: "color_blend",
                name: "Color Blend",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "effects_blend",
                name: "Effects Blend",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "",
            },
            // A LUT is a function of one colour. Halation, bloom, vignette, grain
            // and the gate all read *other* pixels or the pixel's position, so none
            // of them can be baked into one. With this ticked they switch off and
            // what remains is exactly what a 3D LUT could reproduce.
            ParamDef {
                key: "lut_compatible",
                name: "3D LUT Compatible",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "",
            },
            ParamDef {
                key: "film_look_blend",
                name: "Film Look Blend",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Film Look",
            },
            ParamDef {
                key: "core_look",
                name: "Core Look",
                kind: ParamKind::Choice {
                    options: &["Cinematic", "Vintage", "Modern", "Bleach", "Neutral"],
                    default: "Cinematic",
                },
                unit: "",
                section: "Film Look",
            },
            // How far the look is allowed to move skin. Film stocks are chosen for
            // what they do to faces more than for anything else they do, so a
            // control that holds skin still while the rest of the frame takes the
            // stock is not a nicety.
            ParamDef {
                key: "skin_bias",
                name: "Skin Bias",
                kind: ParamKind::Float {
                    min: -1.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Film Look",
            },
            ParamDef {
                key: "exposure",
                name: "Exposure",
                kind: ParamKind::Float {
                    min: -2.0,
                    max: 2.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "EV",
                section: "Color Settings",
            },
            ParamDef {
                key: "contrast",
                name: "Contrast",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.25,
                    neutral: 1.0,
                },
                unit: "",
                section: "Color Settings",
            },
            // The shoulder. Film has no clipping point — density keeps rising all
            // the way up, ever more slowly — which is why a highlight on film rolls
            // off instead of stopping.
            ParamDef {
                key: "highlights",
                name: "Highlights",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.35,
                    neutral: 0.0,
                },
                unit: "",
                section: "Color Settings",
            },
            // And the toe, lifted: the milky black of a print that has been
            // projected a few hundred times.
            ParamDef {
                key: "fade",
                name: "Fade",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.285,
                    neutral: 0.0,
                },
                unit: "",
                section: "Color Settings",
            },
            ParamDef {
                key: "white_balance",
                name: "White Balance",
                kind: ParamKind::Float {
                    min: 2000.0,
                    max: 20000.0,
                    default: 6500.0,
                    neutral: 6500.0,
                },
                unit: "K",
                section: "Color Settings",
            },
            ParamDef {
                key: "tint",
                name: "Tint",
                kind: ParamKind::Float {
                    min: -100.0,
                    max: 100.0,
                    default: 10.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Color Settings",
            },
            // Saturation done the way a print does it — by subtracting dye rather
            // than by pushing chroma. The difference shows in the highlights: a
            // subtractive push darkens as it saturates, which is why film reds go
            // deep instead of going electric.
            ParamDef {
                key: "subtractive_sat",
                name: "Subtractive Sat",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 3.0,
                    default: 1.2,
                    neutral: 1.0,
                },
                unit: "",
                section: "Color Settings",
            },
            ParamDef {
                key: "richness",
                name: "Richness",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 3.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Color Settings",
            },
            ParamDef {
                key: "bleach_bypass",
                name: "Bleach Bypass",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Color Settings",
            },
            ParamDef {
                key: "split_tone_enable",
                name: "Enable Split Tone",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "Split Tone",
            },
            ParamDef {
                key: "split_tone_mode",
                name: "Split Tone Mode",
                kind: ParamKind::Choice {
                    options: &["Natural", "Strong", "Custom"],
                    default: "Natural",
                },
                unit: "",
                section: "Split Tone",
            },
            ParamDef {
                key: "split_tone_amount",
                name: "Amount",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Split Tone",
            },
            ParamDef {
                key: "split_tone_hue",
                name: "Hue Angle",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 360.0,
                    default: 20.0,
                    neutral: 20.0,
                },
                unit: "°",
                section: "Split Tone",
            },
            ParamDef {
                key: "split_tone_pivot",
                name: "Pivot",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.3,
                    neutral: 0.3,
                },
                unit: "",
                section: "Split Tone",
            },
            ParamDef {
                key: "vignette_enable",
                name: "Enable Vignette",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Vignette",
            },
            ParamDef {
                key: "vignette_amount",
                name: "Amount",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.25,
                    neutral: 0.0,
                },
                unit: "",
                section: "Vignette",
            },
            ParamDef {
                key: "vignette_size",
                name: "Size",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.25,
                    neutral: 0.25,
                },
                unit: "",
                section: "Vignette",
            },
            ParamDef {
                key: "halation_enable",
                name: "Enable Halation",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Halation",
            },
            // Off, the whole frame contributes to the glow. On, only what is
            // already bright does — which is what halation actually is.
            ParamDef {
                key: "halation_highlights_only",
                name: "Highlights Only",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Halation",
            },
            ParamDef {
                key: "halation_amount",
                name: "Amount",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.25,
                    neutral: 0.0,
                },
                unit: "",
                section: "Halation",
            },
            ParamDef {
                key: "halation_radius",
                name: "Radius",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 10.0,
                    default: 4.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Halation",
            },
            ParamDef {
                key: "halation_saturation",
                name: "Saturation",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Halation",
            },
            // The hue of the dye reflection, as a position on the wheel rather
            // than in degrees: 0.5 is the red-orange a colour negative gives.
            ParamDef {
                key: "halation_hue",
                name: "Hue",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
                section: "Halation",
            },
            ParamDef {
                key: "bloom_enable",
                name: "Enable Bloom",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Bloom",
            },
            ParamDef {
                key: "bloom_amount",
                name: "Amount",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.25,
                    neutral: 0.0,
                },
                unit: "",
                section: "Bloom",
            },
            ParamDef {
                key: "bloom_radius",
                name: "Radius",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 100.0,
                    default: 10.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Bloom",
            },
            ParamDef {
                key: "grain_enable",
                name: "Enable Grain",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Grain",
            },
            ParamDef {
                key: "grain_preset",
                name: "Preset",
                kind: ParamKind::Choice {
                    options: &["16mm", "35mm", "65mm", "Custom"],
                    default: "65mm",
                },
                unit: "",
                section: "Grain",
            },
            ParamDef {
                key: "grain_amount",
                name: "Amount",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.125,
                    neutral: 0.0,
                },
                unit: "",
                section: "Grain",
            },
            ParamDef {
                key: "grain_size",
                name: "Size",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Grain",
            },
            ParamDef {
                key: "grain_softness",
                name: "Softness",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.1,
                    neutral: 0.1,
                },
                unit: "",
                section: "Grain",
            },
            ParamDef {
                key: "grain_saturation",
                name: "Saturation",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.3,
                    neutral: 0.3,
                },
                unit: "",
                section: "Grain",
            },
            // Grain sits on a picture that has already given up a little of its
            // finest detail — that is what puts the grain *in* the image rather
            // than on top of it. The amount is small on purpose: this is the
            // softening of a print, not a defocus.
            ParamDef {
                key: "image_defocus",
                name: "Image Defocus",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Grain",
            },
            ParamDef {
                key: "gate_enable",
                name: "Enable Film Gate",
                kind: ParamKind::Bool { default: false },
                unit: "",
                section: "Film Gate",
            },
            ParamDef {
                key: "gate_preset",
                name: "Preset",
                kind: ParamKind::Choice {
                    options: &["35mm Silent", "35mm Academy", "16mm", "Super 8"],
                    default: "35mm Silent",
                },
                unit: "",
                section: "Film Gate",
            },
            ParamDef {
                key: "gate_ratio_h",
                name: "Ratio H",
                kind: ParamKind::Float {
                    min: 0.5,
                    max: 4.0,
                    default: 1.33,
                    neutral: 1.33,
                },
                unit: "",
                section: "Film Gate",
            },
            ParamDef {
                key: "gate_ratio_v",
                name: "Ratio V",
                kind: ParamKind::Float {
                    min: 0.5,
                    max: 4.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
                section: "Film Gate",
            },
            // A real gate is a stamped hole, not a rectangle: its corners are
            // rounded because the punch was.
            ParamDef {
                key: "gate_curvature",
                name: "Enable Curvature",
                kind: ParamKind::Bool { default: true },
                unit: "",
                section: "Film Gate",
            },
            ParamDef {
                key: "gate_padding",
                name: "Padding",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
                section: "Film Gate",
            },
        ],
    },
];

/// Effects whose registry defaults deliberately change the image.
///
/// The dividing line is **corrective versus look**, and it is worth stating
/// plainly because getting it wrong is what made this list grow.
///
/// A *corrective* tool must be invisible until touched — Exposure, White
/// Balance, Contrast, Curves, HSL, the wheels, Dehaze. Adding one and seeing an
/// unexplained jump reads as a bug, and zero is the honest starting point.
///
/// A *look* effect is the opposite. You add Halation because you want
/// halation; if it sits there doing nothing until you find the right slider,
/// the effect reads as broken. Resolve ships Split Tone at Strength 0.5 for
/// exactly this reason, and the same argument covers Grain, Bloom, Vignette
/// and Film Damage.
///
/// Every parameter still carries its true `neutral` value separately, so
/// double-clicking a slider returns it to no-op even where the default is
/// visible.
///
/// Adding to this list should be a decision about which kind of tool an effect
/// is, never a way to silence a failing test.
pub const EFFECTS_WITH_VISIBLE_DEFAULTS: &[&str] = &[
    "split_tone",
    "halation",
    "bloom",
    "grain",
    "vignette",
    "film_damage",
    // You add a blur because you want a blur. Resolve ships both at 0.4 for
    // the same reason a Grain row arrives with grain in it.
    "radial_blur",
    "zoom_blur",
    // A Film Look row with no film look in it would be a strange thing to
    // add on purpose.
    "film_look",
    "dehaze",
    // The one on this list that is *not* obvious, so it is worth saying why:
    // Dehaze is corrective and Cinematic Haze is its mirror image, which
    // might suggest the same treatment. It is not the same kind of tool.
    // Dehaze answers "there is haze here I did not want"; this one answers
    // "I want haze here", and a haze effect that adds no haze until you find
    // the right slider reads as broken. Resolve ships it visible for the same
    // reason.
    "cinematic_haze",
];

/// The fixed panels, in the order they are applied.
///
/// A document is created with exactly these rows, pinned. The order is
/// Lightroom's, which is also the order that makes physical sense: neutralise
/// the light, set the exposure, then shape the tone, then the detail, then the
/// colour, and finally the curve and the wheels.
///
/// Their effects are ordinary registry entries — nothing about them is special
/// beyond being created up front and refusing to be deleted or moved.
/// The rows a new document starts with, in render order.
///
/// A pinned row is one a fixed panel drives, so it cannot be deleted or
/// reordered. Dehaze is deliberately *not* here: it has five controls
/// including a colour and a depth view, and reducing it to one slider on the
/// Basic panel threw four of them away. It is an effect, and it lives in the
/// effects list like one.
pub const PINNED_ROWS: &[&str] = &[
    "white_balance",
    "exposure",
    "contrast",
    "tone",
    "presence",
    "colour",
    "colour_mixer",
    "curves",
    "primaries",
    "log_wheels",
];

/// A new document with the fixed panels already in place.
pub fn new_document(source: impl Into<String>) -> pe_core::Document {
    let mut doc = pe_core::Document::from_path(source);
    for (i, key) in PINNED_ROWS.iter().enumerate() {
        let def = by_key(key).expect("every pinned row is a registered effect");
        let mut row = pe_core::StackRow::pinned(pe_core::RowId(i as u64), *key);
        row.params = def.default_params();
        doc.stack.push(row);
    }
    doc
}

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
    fn the_registry_is_the_expected_size() {
        // Nine at M1, plus Split Tone once the Resolve parameter research
        // landed. Pinned so an accidental duplicate or deletion is visible.
        assert_eq!(EFFECTS.len(), 24);
    }

    /// A gate naming a parameter that does not exist would quietly do
    /// nothing, and "quietly does nothing" is exactly what a gate is for —
    /// so the typo would look like the feature working.
    #[test]
    fn every_gate_names_parameters_that_exist() {
        for e in EFFECTS {
            for gate in e.gates {
                assert!(
                    e.param(gate.by).is_some(),
                    "{}: gate is driven by {}, which is not a parameter",
                    e.key,
                    gate.by
                );
                for key in gate.params {
                    assert!(
                        e.param(key).is_some(),
                        "{}: gate guards {key}, which is not a parameter",
                        e.key
                    );
                }
            }
        }
    }

    /// And the controlling parameter has to be the kind the condition can
    /// read. `When::True` against a slider is a gate that never closes.
    #[test]
    fn every_gate_reads_a_control_of_the_kind_it_expects() {
        for e in EFFECTS {
            for gate in e.gates {
                let kind = e.param(gate.by).unwrap().kind;
                let ok = match gate.when {
                    When::True | When::False => matches!(kind, ParamKind::Bool { .. }),
                    When::Positive => matches!(kind, ParamKind::Float { .. }),
                    When::Is(option) => match kind {
                        ParamKind::Choice { options, .. } => options.contains(&option),
                        _ => false,
                    },
                };
                assert!(
                    ok,
                    "{}: {:?} cannot read {} ({:?})",
                    e.key, gate.when, gate.by, kind
                );
            }
        }
    }

    /// No parameter may be guarded twice. `is_active` stops at the first gate
    /// that names it, so a second one would silently never apply.
    #[test]
    fn no_parameter_is_guarded_by_two_gates() {
        for e in EFFECTS {
            let mut seen: Vec<&str> = Vec::new();
            for gate in e.gates {
                for key in gate.params {
                    assert!(!seen.contains(key), "{}: {key} is guarded twice", e.key);
                    seen.push(key);
                }
            }
        }
    }

    /// The behaviour itself, on the case the screenshots showed: Halation's
    /// Basic Grain sliders are dead until Append Grain Internally is ticked.
    #[test]
    fn a_gate_opens_when_its_switch_is_thrown() {
        let e = by_key("halation").unwrap();
        let mut p = e.default_params();
        assert!(
            !e.is_active("grain_strength", &p),
            "grain starts switched off"
        );
        assert!(
            e.is_active("strength", &p),
            "an ungated control is always live"
        );

        p.set("append_grain", pe_core::ParamValue::Bool(true));
        assert!(e.is_active("grain_strength", &p));
        assert!(e.is_active("grain_size", &p));
    }

    /// And on a slider rather than a checkbox: the Secondary Glow's shape
    /// controls mean nothing while its Strength is zero.
    #[test]
    fn an_amount_of_nothing_gates_the_controls_that_shape_it() {
        let e = by_key("halation").unwrap();
        let mut p = e.default_params();
        assert!(!e.is_active("secondary_spread", &p));
        p.set("secondary_strength", pe_core::ParamValue::Float(0.4));
        assert!(e.is_active("secondary_spread", &p));
    }

    /// And on a dropdown: Split Tone's custom hues only exist in Custom.
    #[test]
    fn a_mode_gates_the_controls_that_belong_to_it() {
        let e = by_key("split_tone").unwrap();
        let mut p = e.default_params();
        assert!(!e.is_active("shadow_hue", &p));
        p.set("mode", pe_core::ParamValue::Choice("Custom".into()));
        assert!(e.is_active("shadow_hue", &p));
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
            // Aerial perspective, lens spill and physical film damage are all
            // things happening to light.
            ("dehaze", WorkingSpace::Linear),
            // The same model as Dehaze, run forwards instead of backwards.
            ("cinematic_haze", WorkingSpace::Linear),
            ("bloom", WorkingSpace::Linear),
            ("film_damage", WorkingSpace::Linear),
            // Local contrast adds light back to a region.
            ("presence", WorkingSpace::Linear),
            // Reshaping how the picture reads, not how much light fell.
            ("tone", WorkingSpace::Log),
            ("colour", WorkingSpace::Log),
            // Tonal bands of a log-encoded signal, by definition.
            ("log_wheels", WorkingSpace::Log),
            ("colour_mixer", WorkingSpace::Log),
            // Channel gains and an exposure scale: light, both of them.
            ("color_stabilizer", WorkingSpace::Linear),
            // A blur is an average of light. Averaged anywhere else it
            // comes out dark, the same way a downscale does.
            ("radial_blur", WorkingSpace::Linear),
            ("zoom_blur", WorkingSpace::Linear),
            // An average of light, like any blur.
            ("noise_reduction", WorkingSpace::Linear),
            // A film response curve is a perceptual object: the shape of
            // the print, not a photometric measurement.
            ("film_look", WorkingSpace::Log),
            // Perception: pivoting, shaping, drawing.
            ("contrast", WorkingSpace::Log),
            ("curves", WorkingSpace::Log),
            ("hsl", WorkingSpace::Log),
            ("primaries", WorkingSpace::Log),
            ("split_tone", WorkingSpace::Log),
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
            let expected = matches!(
                e.key,
                "grain"
                    | "halation"
                    | "vignette"
                    | "bloom"
                    | "dehaze"
                    | "film_damage"
                    | "presence"
                    | "color_stabilizer"
                    | "radial_blur"
                    | "zoom_blur"
                    | "noise_reduction"
                    // A depth estimate, a glow, rays and a shimmer: every part
                    // of it reads its neighbours.
                    | "cinematic_haze"
                    // Film Look Creator carries Resolve's halation, bloom,
                    // vignette and grain sections, and every one of those
                    // reads its neighbours.
                    | "film_look"
            );
            assert_eq!(e.spatial, expected, "{}", e.key);
        }
    }

    /// A new document arrives with its pinned rows already holding ids from
    /// zero upwards, so anything that hands out new ones has to be told where
    /// they stop.
    ///
    /// Getting this wrong does not error. Every lookup is a scan that stops at
    /// the first match, so a duplicate id silently resolves to the pinned row
    /// — the added effect draws, and its bin, its arrows and its parameters
    /// all act on White Balance instead.
    #[test]
    fn a_generator_resumed_on_a_new_document_cannot_collide_with_it() {
        let doc = new_document("a.jpg");
        let mut ids = pe_core::RowIdGenerator::resuming(&doc);
        let existing: Vec<u64> = doc.stack.iter().map(|r| r.id.0).collect();
        assert_eq!(existing.len(), PINNED_ROWS.len());
        for _ in 0..8 {
            let next = ids.allocate();
            assert!(
                !existing.contains(&next.0),
                "id {} is already in the stack",
                next.0
            );
        }
    }

    /// And a generator that was *not* resumed collides immediately, which is
    /// the mistake this is here to describe.
    #[test]
    fn a_default_generator_collides_with_the_pinned_rows() {
        let doc = new_document("a.jpg");
        let mut ids = pe_core::RowIdGenerator::default();
        let first = ids.allocate();
        assert!(
            doc.stack.get(first).is_some(),
            "the trap has moved: a default generator no longer collides, so              the comment on `resuming` needs revisiting"
        );
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
        assert!(by_key("colour_warper").is_none(), "the colour warper is M2");
    }

    /// A fresh document must cost nothing to render.
    ///
    /// Nine pinned panels exist the moment a photo is opened. If each ran a
    /// full-screen pass to do nothing, the pass counter would read 9 before
    /// the user touched anything and the number would stop meaning "the work
    /// your edit costs".
    #[test]
    fn a_new_document_is_entirely_neutral() {
        let doc = new_document("photo.jpg");
        assert_eq!(doc.stack.len(), PINNED_ROWS.len());
        assert_eq!(doc.stack.pinned_count(), PINNED_ROWS.len());
        for row in doc.stack.iter() {
            let def = by_key(&row.effect).unwrap();
            assert!(
                def.is_neutral(&row.params),
                "{} is not neutral in a fresh document",
                row.effect
            );
        }
    }

    #[test]
    fn no_pinned_row_is_a_look_effect() {
        // Look effects ship visible on purpose. One of those pinned into every
        // new document would mean opening a photo already changed it.
        for key in PINNED_ROWS {
            assert!(
                !EFFECTS_WITH_VISIBLE_DEFAULTS.contains(key),
                "{key} ships visible and must not be a fixed panel"
            );
        }
    }

    #[test]
    fn touching_a_parameter_stops_it_being_neutral() {
        let e = by_key("tone").unwrap();
        let mut p = e.default_params();
        assert!(e.is_neutral(&p));
        p.set("shadows", pe_core::ParamValue::Float(0.4));
        assert!(!e.is_neutral(&p));
    }

    #[test]
    fn every_visible_default_effect_exists() {
        for key in EFFECTS_WITH_VISIBLE_DEFAULTS {
            assert!(
                by_key(key).is_some(),
                "{key} is exempted but not registered"
            );
        }
    }

    /// Resolve ships Split Tone at Strength 0.5, Pivot 0.3, Hue Angle 20 with
    /// ranges 1 / 1 / 360. Pinned because these came from the real UI and a
    /// silent drift would make our looks disagree with a colourist muscle
    /// memory. See docs/resolve-parameters.md.
    #[test]
    fn split_tone_matches_the_resolve_defaults() {
        let e = by_key("split_tone").unwrap();
        let expect = [
            ("strength", 0.5f32, 1.0f32),
            ("pivot", 0.3, 1.0),
            ("hue_angle", 20.0, 360.0),
        ];
        for (key, default, max) in expect {
            match e.param(key).unwrap().kind {
                ParamKind::Float {
                    default: d,
                    max: m,
                    min,
                    ..
                } => {
                    assert!((d - default).abs() < 1e-6, "{key} default is {d}");
                    assert!((m - max).abs() < 1e-6, "{key} max is {m}");
                    assert_eq!(min, 0.0, "{key} min is {min}");
                }
                _ => panic!("{key} should be a float"),
            }
        }
        match e.param("mode").unwrap().kind {
            ParamKind::Choice { options, default } => {
                assert_eq!(options, ["Natural", "Strong", "Custom"]);
                assert_eq!(default, "Natural");
            }
            _ => panic!("mode should be a choice"),
        }
    }
}
