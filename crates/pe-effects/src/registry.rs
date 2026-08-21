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
        params: &[
            ParamDef {
                key: "mode",
                name: "Split Tone Mode",
                kind: ParamKind::Choice {
                    options: &["natural", "strong", "custom"],
                    default: "natural",
                },
                unit: "",
            },
            ParamDef {
                key: "preview_influence",
                name: "Preview Influence",
                kind: ParamKind::Bool { default: false },
                unit: "",
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
            },
            ParamDef {
                key: "protect_neutrals",
                name: "Protect Neutrals",
                kind: ParamKind::Bool { default: false },
                unit: "",
            },
            ParamDef {
                key: "min_saturation",
                name: "Minimum Saturation",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
            },
            ParamDef {
                key: "max_saturation",
                name: "Maximum Saturation",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
            },
            // Custom mode only. Resolve hides these behind the mode dropdown;
            // we keep them present so a document round-trips whatever mode it
            // was saved in.
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
        // Follows Resolve's Film Grain parameter set. Notably their
        // Shadow/Midtone/Highlight Gain trio replaces the single "shadow bias"
        // slider we had: three independent controls are strictly better than
        // one that slides a peak around, and it is how a colourist expects to
        // put grain in the midtones only.
        params: &[
            ParamDef {
                key: "strength",
                name: "Grain Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.35,
                    neutral: 0.0,
                },
                unit: "",
            },
            ParamDef {
                key: "size",
                name: "Grain Size",
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
                key: "softness",
                name: "Softness",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
            },
            // Resolve: "At a value of 0, grain is monochrome." A continuous
            // control, not the boolean we had.
            ParamDef {
                key: "saturation",
                name: "Saturation",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
            },
            // "Lower values emphasize lighter grains, higher values emphasize
            // darker grains."
            bipolar("offset", "Offset", 1.0, ""),
            ParamDef {
                key: "shadow_gain",
                name: "Shadow Gain",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
            },
            ParamDef {
                key: "midtone_gain",
                name: "Midtone Gain",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
            },
            ParamDef {
                key: "highlight_gain",
                name: "Highlight Gain",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
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
        // Follows Resolve's Halation structure. Two changes worth noting
        // against our M1 version: isolation is a *band* (Threshold is the low
        // clip, Normalization the high clip) rather than a single threshold,
        // and the glow has two independent layers. A secondary glow with its
        // own spread is what gives the effect a tight core and a wide falloff
        // at once, which one blur radius cannot do.
        params: &[
            // A look effect, so it ships visible — see
            // EFFECTS_WITH_VISIBLE_DEFAULTS.
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
            },
            // Threshold and Normalization are in *linear light*, where diffuse
            // white is 1.0. Defaulting the threshold to 1.0 meant nothing in an
            // SDR photograph ever exceeded it, so the effect could not fire at
            // any strength. 0.5 to 1.0 is the top stop of an SDR image, which
            // is exactly what should be glowing.
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
            },
            ParamDef {
                key: "normalization",
                name: "Normalization",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 8.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
            },
            ParamDef {
                key: "spread",
                name: "Spread",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.2,
                    default: 0.04,
                    neutral: 0.0,
                },
                // Fraction of the image's long edge. Resolution independence
                // again: a pixel radius would shrink to a rim on export.
                unit: "",
            },
            ParamDef {
                key: "saturation",
                name: "Saturation",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
            },
            // Red-orange, the characteristic colour of light scattering back
            // off the film base through the dye layers.
            ParamDef {
                key: "hue",
                name: "Hue",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 360.0,
                    default: 12.0,
                    neutral: 12.0,
                },
                unit: "°",
            },
            amount("secondary_strength", "Secondary Glow"),
            ParamDef {
                key: "secondary_spread",
                name: "Secondary Spread",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.4,
                    default: 0.08,
                    neutral: 0.0,
                },
                unit: "",
            },
            // Resolve's Fine Tune Relative Spread. With it off the glow is one
            // radius tinted by Hue; with it on each channel scatters its own
            // distance and the red fringe emerges from the physics instead.
            // Defaults are ordered red > green > blue because longer
            // wavelengths penetrate the emulsion further and scatter wider, so
            // ticking the box immediately gives the characteristic look.
            ParamDef {
                key: "fine_tune_spread",
                name: "Fine Tune Relative Spread",
                kind: ParamKind::Bool { default: false },
                unit: "",
            },
            ParamDef {
                key: "relative_red",
                name: "Relative Spread Red",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
            },
            ParamDef {
                key: "relative_green",
                name: "Relative Spread Green",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 0.7,
                    neutral: 0.7,
                },
                unit: "",
            },
            ParamDef {
                key: "relative_blue",
                name: "Relative Spread Blue",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 0.5,
                    neutral: 0.5,
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
        // Follows Resolve's Vignette: a Basic set (Size, Anamorphism,
        // Softness, Color) and an Advanced set (Border Shape, Rotation,
        // Center). Two of Resolve's controls are deliberately not here —
        // Composite Type is the row's blend mode, and Transparency is folded
        // into Amount, which is bipolar and so can brighten the corners too.
        params: &[
            ParamDef {
                key: "amount",
                name: "Amount",
                kind: ParamKind::Float {
                    min: -1.0,
                    max: 1.0,
                    default: 0.4,
                    neutral: 0.0,
                },
                unit: "",
            },
            ParamDef {
                key: "size",
                name: "Size",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
            },
            // Stretches the shape horizontally. At 0 the vignette follows
            // the frame; positive values widen it the way an anamorphic lens
            // would.
            bipolar("anamorphism", "Anamorphism", 1.0, ""),
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
            },
            // 0 is an ellipse, 1 approaches a rectangle, by way of a
            // superellipse exponent.
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
            },
            ParamDef {
                key: "color",
                name: "Color",
                // Black is the classic darkening; any other colour tints the
                // border instead.
                kind: ParamKind::Rgb {
                    default: [0.0, 0.0, 0.0],
                },
                unit: "",
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
        params: &[
            // Bipolar like Resolve's: above zero removes haze, below zero adds
            // it by running the same scattering model forwards.
            bipolar("strength", "Dehaze Strength", 1.0, ""),
            ParamDef {
                key: "display_depth",
                name: "Display Depth",
                kind: ParamKind::Bool { default: false },
                unit: "",
            },
            bipolar("shadow", "Shadow", 1.0, ""),
            bipolar("highlight", "Highlight", 1.0, ""),
            ParamDef {
                key: "haze_color",
                name: "Haze Color",
                // Slightly blue-grey: the usual colour of atmospheric
                // scattering, and a sane start before sampling the image.
                kind: ParamKind::Rgb {
                    default: [0.78, 0.82, 0.90],
                },
                unit: "",
            },
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
                    default: 0.25,
                    neutral: 0.0,
                },
                unit: "",
            },
            // Positive warms, simulating a projector bulb running hot;
            // positive tint yellows, simulating dye failure.
            bipolar("temp_shift", "Temp Shift", 1.0, ""),
            bipolar("tint_shift", "Tint Shift", 1.0, ""),
            ParamDef {
                key: "focal_factor",
                name: "Focal Factor",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
            },
            ParamDef {
                key: "geometry_factor",
                name: "Geometry Factor",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
            },
            ParamDef {
                key: "tilt_amount",
                name: "Tilt Amount",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
            },
            ParamDef {
                key: "tilt_angle",
                name: "Tilt Angle",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 360.0,
                    default: 90.0,
                    neutral: 90.0,
                },
                unit: "°",
            },
            ParamDef {
                key: "dirt_density",
                name: "Dirt Density",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.15,
                    neutral: 0.0,
                },
                unit: "",
            },
            ParamDef {
                key: "dirt_size",
                name: "Dirt Size",
                kind: ParamKind::Float {
                    min: 0.05,
                    max: 4.0,
                    default: 1.0,
                    neutral: 1.0,
                },
                unit: "",
            },
            ParamDef {
                key: "dirt_blur",
                name: "Dirt Blur",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 0.4,
                    neutral: 0.4,
                },
                unit: "",
            },
            ParamDef {
                key: "dirt_seed",
                name: "Dirt Seed",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 999.0,
                    default: 7.0,
                    neutral: 7.0,
                },
                unit: "",
            },
            ParamDef {
                key: "dirt_color",
                name: "Dirt Color",
                // Black reads as dirt on a print, white as dirt on a negative.
                kind: ParamKind::Rgb {
                    default: [0.0, 0.0, 0.0],
                },
                unit: "",
            },
            ParamDef {
                key: "scratch_blur",
                name: "Scratch Blur",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 2.0,
                    default: 0.3,
                    neutral: 0.3,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch_color",
                name: "Scratch Color",
                kind: ParamKind::Rgb {
                    default: [1.0, 1.0, 1.0],
                },
                unit: "",
            },
            // Five independent scratches, matching Resolve. A count
            // parameter could not place them, and placement is most of
            // what makes damage read as real rather than procedural.
            ParamDef {
                key: "scratch1_position",
                name: "Scratch 1 Position",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.2,
                    neutral: 0.2,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch1_width",
                name: "Scratch 1 Width",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.05,
                    default: 0.002,
                    neutral: 0.002,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch1_strength",
                name: "Scratch 1 Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.0,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch2_position",
                name: "Scratch 2 Position",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.35,
                    neutral: 0.35,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch2_width",
                name: "Scratch 2 Width",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.05,
                    default: 0.002,
                    neutral: 0.002,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch2_strength",
                name: "Scratch 2 Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch3_position",
                name: "Scratch 3 Position",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    neutral: 0.5,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch3_width",
                name: "Scratch 3 Width",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.05,
                    default: 0.002,
                    neutral: 0.002,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch3_strength",
                name: "Scratch 3 Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch4_position",
                name: "Scratch 4 Position",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.65,
                    neutral: 0.65,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch4_width",
                name: "Scratch 4 Width",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.05,
                    default: 0.002,
                    neutral: 0.002,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch4_strength",
                name: "Scratch 4 Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch5_position",
                name: "Scratch 5 Position",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.8,
                    neutral: 0.8,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch5_width",
                name: "Scratch 5 Width",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 0.05,
                    default: 0.002,
                    neutral: 0.002,
                },
                unit: "",
            },
            ParamDef {
                key: "scratch5_strength",
                name: "Scratch 5 Strength",
                kind: ParamKind::Float {
                    min: 0.0,
                    max: 1.0,
                    default: 0.0,
                    neutral: 0.0,
                },
                unit: "",
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
        params: &[
            bipolar("vibrance", "Vibrance", 1.0, ""),
            bipolar("saturation", "Saturation", 1.0, ""),
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
pub const PINNED_ROWS: &[&str] = &[
    "white_balance",
    "exposure",
    "contrast",
    "tone",
    "presence",
    "dehaze",
    "colour",
    "curves",
    "primaries",
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
        assert_eq!(EFFECTS.len(), 16);
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
            ("bloom", WorkingSpace::Linear),
            ("film_damage", WorkingSpace::Linear),
            // Local contrast adds light back to a region.
            ("presence", WorkingSpace::Linear),
            // Reshaping how the picture reads, not how much light fell.
            ("tone", WorkingSpace::Log),
            ("colour", WorkingSpace::Log),
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
                "grain" | "halation" | "vignette" | "bloom" | "dehaze" | "film_damage" | "presence"
            );
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
                assert_eq!(options, ["natural", "strong", "custom"]);
                assert_eq!(default, "natural");
            }
            _ => panic!("mode should be a choice"),
        }
    }
}
