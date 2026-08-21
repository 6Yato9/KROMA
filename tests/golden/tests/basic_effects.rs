//! Tone, Presence, Colour and the Log Wheels.
//!
//! These are the effects behind the Basic panel and the second wheel set. Each
//! one claims to act on a *part* of the picture, so most of what follows is
//! about whether it leaves the rest of it alone. A Shadows slider that also
//! moves the highlights is a worse tool than no Shadows slider at all.

use pe_core::{Document, ParamValue, RowId, StackRow, Wheel};
use pe_io::DecodedImage;
use pe_render::{EffectRenderer, GpuContext};

fn render(gpu: &GpuContext, src: &DecodedImage, doc: &Document) -> DecodedImage {
    let renderer = EffectRenderer::new(&gpu.device);
    let pixels = pe_render::render_full(gpu, &renderer, src.width, src.height, &src.pixels, doc)
        .expect("export");
    DecodedImage::new(src.width, src.height, pixels).expect("decoded")
}

/// One effect at its defaults, with the named parameters overridden.
fn look(effect: &str, params: &[(&str, ParamValue)]) -> Document {
    let mut doc = Document::from_path("test.png");
    let def = pe_effects::by_key(effect).expect("effect exists");
    let mut row = StackRow::new(RowId(0), effect);
    row.params = def.default_params();
    for (k, v) in params {
        row.params.set(*k, v.clone());
    }
    doc.stack.push(row);
    doc
}

fn wheel(master: f32) -> ParamValue {
    ParamValue::Wheel(Wheel {
        rgb: [0.0; 3],
        master,
    })
}

/// A black-to-white ramp across x, so a tonal control's reach can be read off
/// by looking at which columns moved.
fn ramp() -> DecodedImage {
    let mut pixels = Vec::new();
    for _ in 0..8 {
        for x in 0..256u32 {
            let v = x as u8;
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    DecodedImage::new(256, 8, pixels).expect("ramp")
}

/// How far a column of the ramp moved, signed, in 8-bit levels.
fn delta(a: &DecodedImage, b: &DecodedImage, x: u32) -> i32 {
    b.pixel(x, 4)[0] as i32 - a.pixel(x, 4)[0] as i32
}

/// How far apart the channels are — a stand-in for saturation that needs no
/// colour conversion to read.
fn spread(img: &DecodedImage, x: u32, y: u32) -> i32 {
    let p = img.pixel(x, y);
    p[0].max(p[1]).max(p[2]) as i32 - p[0].min(p[1]).min(p[2]) as i32
}

// ---------------------------------------------------------------------------
// Tone
// ---------------------------------------------------------------------------

#[test]
fn highlights_and_shadows_act_on_opposite_ends() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = ramp();

    let hi = render(
        gpu,
        &src,
        &look("tone", &[("highlights", ParamValue::Float(-1.0))]),
    );
    let sh = render(
        gpu,
        &src,
        &look("tone", &[("shadows", ParamValue::Float(1.0))]),
    );

    assert!(delta(&src, &hi, 220) < -3, "highlights did not come down");
    assert!(
        delta(&src, &hi, 30).abs() <= 2,
        "highlights reached into the shadows by {}",
        delta(&src, &hi, 30)
    );

    assert!(delta(&src, &sh, 30) > 3, "shadows did not lift");
    assert!(
        delta(&src, &sh, 220).abs() <= 2,
        "shadows reached into the highlights by {}",
        delta(&src, &sh, 220)
    );
}

/// Whites and Blacks are concentrated further out than Highlights and Shadows.
///
/// This is the property that makes four sliders worth having instead of two:
/// once Highlights has been pulled down, Whites must still have somewhere to
/// work. If both covered the same range the second one would be dead travel.
#[test]
fn whites_and_blacks_work_further_out_than_highlights_and_shadows() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = ramp();

    // Pull down rather than up, so nothing clips against white and the
    // comparison stays about the weighting rather than about the ceiling.
    let highlights = render(
        gpu,
        &src,
        &look("tone", &[("highlights", ParamValue::Float(-1.0))]),
    );
    let whites = render(
        gpu,
        &src,
        &look("tone", &[("whites", ParamValue::Float(-1.0))]),
    );

    // x=190 is the upper midtones, where Highlights lives; x=250 is the very
    // top, which is Whites' territory.
    let h_mid = delta(&src, &highlights, 190).abs();
    let w_mid = delta(&src, &whites, 190).abs();
    let w_top = delta(&src, &whites, 250).abs();
    assert!(
        w_mid * 4 < h_mid,
        "Whites should stay out of the upper midtones ({w_mid} vs Highlights' {h_mid})"
    );
    assert!(
        w_top > w_mid * 3 && w_top > 8,
        "Whites should still have somewhere to work at the top ({w_top} vs {w_mid})"
    );

    // The same, mirrored, at the bottom of the range.
    let shadows = render(
        gpu,
        &src,
        &look("tone", &[("shadows", ParamValue::Float(1.0))]),
    );
    let blacks = render(
        gpu,
        &src,
        &look("tone", &[("blacks", ParamValue::Float(1.0))]),
    );
    let s_mid = delta(&src, &shadows, 30).abs();
    let b_mid = delta(&src, &blacks, 30).abs();
    let b_low = delta(&src, &blacks, 8).abs();
    assert!(
        b_mid * 4 < s_mid,
        "Blacks should stay out of the lower midtones ({b_mid} vs Shadows' {s_mid})"
    );
    assert!(
        b_low > b_mid * 3 && b_low > 5,
        "Blacks should still have somewhere to work at the bottom ({b_low} vs {b_mid})"
    );
}

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

/// A single hue at fixed brightness, sweeping from nearly grey to fully
/// saturated across x. The one image that can tell Saturation and Vibrance
/// apart, because the difference between them is entirely a function of how
/// saturated a colour already was.
fn saturation_ramp() -> DecodedImage {
    let mut pixels = Vec::new();
    for _ in 0..8 {
        for x in 0..256u32 {
            let s = 0.04 + 0.94 * (x as f32 / 255.0);
            // Orange, near the hue a Vibrance slider is built to protect.
            let (r, g, b) = (0.78_f32, 0.78 * (1.0 - 0.5 * s), 0.78 * (1.0 - s));
            pixels.extend_from_slice(&[
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8,
                255,
            ]);
        }
    }
    DecodedImage::new(256, 8, pixels).expect("saturation ramp")
}

#[test]
fn saturation_raises_saturation_and_leaves_neutrals_alone() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let doc = look("colour", &[("saturation", ParamValue::Float(0.6))]);

    let src = saturation_ramp();
    let out = render(gpu, &src, &doc);
    for x in [40u32, 128, 220] {
        assert!(
            spread(&out, x, 4) > spread(&src, x, 4),
            "saturation did not increase at x={x}"
        );
    }

    // Grey has no hue to amplify, so any movement here means the colour
    // conversion is leaking rather than the slider working.
    let grey = ramp();
    let out = render(gpu, &grey, &doc);
    for x in (8..248).step_by(24) {
        assert!(
            spread(&out, x, 4) <= 2,
            "the grey ramp picked up colour at x={x}"
        );
    }
}

/// Vibrance holds back on colours that are already vivid.
///
/// That restraint is the entire difference between the two controls: it is
/// what stops a push turning skin orange while it lifts a muted background.
#[test]
fn vibrance_protects_colours_that_are_already_saturated() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = saturation_ramp();
    let vib = render(
        gpu,
        &src,
        &look("colour", &[("vibrance", ParamValue::Float(1.0))]),
    );
    let sat = render(
        gpu,
        &src,
        &look("colour", &[("saturation", ParamValue::Float(1.0))]),
    );

    // Measure each against Saturation at the same column, so the comparison is
    // "how much of a uniform push did Vibrance allow here" rather than raw
    // levels, which differ by an order of magnitude across the ramp.
    let share = |x: u32| -> f32 {
        let v = (spread(&vib, x, 4) - spread(&src, x, 4)) as f32;
        let s = (spread(&sat, x, 4) - spread(&src, x, 4)) as f32;
        assert!(s > 0.0, "saturation did nothing at x={x}");
        v / s
    };

    let muted = share(20);
    let vivid = share(240);
    assert!(muted > 0.0, "vibrance did not lift the muted end");
    assert!(
        muted > vivid * 2.0,
        "vibrance should favour muted colour far more than vivid \
         ({muted:.2} of a full push vs {vivid:.2})"
    );
}

// ---------------------------------------------------------------------------
// Presence
// ---------------------------------------------------------------------------

/// A hard vertical edge: dark to the left of x=64, light to the right.
fn edge() -> DecodedImage {
    // Square, and no smaller: both radii are a fraction of the frame, so a
    // short frame would shrink them below a pixel and neither slider would
    // have anything to average over.
    let mut pixels = Vec::new();
    for _ in 0..128 {
        for x in 0..128u32 {
            let v = if x < 64 { 60u8 } else { 190u8 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    DecodedImage::new(128, 128, pixels).expect("edge")
}

#[test]
fn clarity_separates_the_two_sides_of_an_edge() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = edge();
    let out = render(
        gpu,
        &src,
        &look("presence", &[("clarity", ParamValue::Float(1.0))]),
    );

    // Local contrast means exactly this: the dark side of an edge goes darker
    // and the light side lighter.
    let dark = out.pixel(62, 64)[0] as i32 - src.pixel(62, 64)[0] as i32;
    let light = out.pixel(65, 64)[0] as i32 - src.pixel(65, 64)[0] as i32;
    assert!(
        dark < -4,
        "the dark side of the edge did not deepen ({dark})"
    );
    assert!(
        light > 4,
        "the light side of the edge did not lift ({light})"
    );
}

/// Texture and Clarity are the same operation at different scales, so the only
/// thing that distinguishes them is reach. If Texture responded to a broad
/// edge it would just be a second Clarity slider.
#[test]
fn texture_works_at_a_finer_scale_than_clarity() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = edge();
    let tex = render(
        gpu,
        &src,
        &look("presence", &[("texture", ParamValue::Float(1.0))]),
    );
    let clar = render(
        gpu,
        &src,
        &look("presence", &[("clarity", ParamValue::Float(1.0))]),
    );

    // Two pixels back from the edge is inside Clarity's radius and outside
    // Texture's.
    for x in [62u32, 65] {
        let t = (tex.pixel(x, 64)[0] as i32 - src.pixel(x, 64)[0] as i32).abs();
        let c = (clar.pixel(x, 64)[0] as i32 - src.pixel(x, 64)[0] as i32).abs();
        assert!(
            t * 3 < c,
            "texture reached as far as clarity at x={x} ({t} vs {c})"
        );
    }
}

// ---------------------------------------------------------------------------
// Log wheels
// ---------------------------------------------------------------------------

/// The reason there are two sets of wheels.
///
/// The primaries wheels hinge the transfer curve at its ends, so they
/// interact — pull Lift up and the midtones follow. These address tonal bands,
/// so a shadow push has to genuinely leave the highlights where they were.
#[test]
fn a_shadow_wheel_leaves_the_highlights_alone() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = ramp();
    let out = render(gpu, &src, &look("log_wheels", &[("shadow", wheel(0.15))]));

    assert!(delta(&src, &out, 20) > 3, "the shadow wheel did nothing");
    assert!(
        delta(&src, &out, 235).abs() <= 2,
        "the shadow wheel reached the highlights by {}",
        delta(&src, &out, 235)
    );
}

#[test]
fn the_offset_wheel_moves_everything() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = ramp();
    let out = render(gpu, &src, &look("log_wheels", &[("offset", wheel(0.1))]));

    // Unweighted by design, which is why colourists reach for it first.
    for x in [20u32, 128, 200] {
        assert!(delta(&src, &out, x) > 2, "offset missed x={x}");
    }
}

/// Low Range and High Range are controls rather than constants because
/// deciding where "shadow" stops is the point of the tool.
#[test]
fn the_range_pivots_move_the_band_boundaries() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = ramp();
    let band = |low: f32, high: f32| {
        look(
            "log_wheels",
            &[
                ("shadow", wheel(0.15)),
                ("low_range", ParamValue::Float(low)),
                ("high_range", ParamValue::Float(high)),
            ],
        )
    };
    let narrow = render(gpu, &src, &band(0.20, 0.30));
    let wide = render(gpu, &src, &band(0.50, 0.55));

    let at_narrow = delta(&src, &narrow, 120);
    let at_wide = delta(&src, &wide, 120);
    assert!(
        at_wide > at_narrow + 3,
        "raising Low Range should extend the shadow band up the ramp \
         ({at_wide} vs {at_narrow})"
    );
}

#[test]
fn a_coloured_wheel_tints_rather_than_only_brightening() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = ramp();
    let out = render(
        gpu,
        &src,
        &look(
            "log_wheels",
            &[(
                "shadow",
                ParamValue::Wheel(Wheel {
                    rgb: [0.08, -0.02, -0.02],
                    master: 0.0,
                }),
            )],
        ),
    );

    let p = out.pixel(30, 4);
    assert!(
        p[0] as i32 > p[2] as i32 + 3,
        "a red-pushed shadow wheel should warm the shadows, got {p:?}"
    );
    // And it must stay a shadow control even when it is tinting.
    assert!(
        spread(&out, 235, 4) <= 2,
        "the tint reached the highlights ({:?})",
        out.pixel(235, 4)
    );
}

// ---------------------------------------------------------------------------
// The parametric curve
// ---------------------------------------------------------------------------

/// sRGB decode, so a ratio can be taken in the light the numbers describe
/// rather than in the encoding.
fn linear(v: u8) -> f32 {
    let s = v as f32 / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

fn parametric(params: &[(&str, f32)]) -> Document {
    let overrides: Vec<(&str, ParamValue)> = params
        .iter()
        .map(|(k, v)| (*k, ParamValue::Float(*v)))
        .collect();
    look("curves", &overrides)
}

#[test]
fn a_parametric_region_acts_on_its_own_end_of_the_range() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = ramp();
    let shadows = render(gpu, &src, &parametric(&[("param_shadows", 1.0)]));
    let highlights = render(gpu, &src, &parametric(&[("param_highlights", 1.0)]));

    assert!(delta(&src, &shadows, 24) > 3, "shadows did not lift");
    assert!(
        delta(&src, &shadows, 235).abs() <= 2,
        "the shadows region reached the highlights by {}",
        delta(&src, &shadows, 235)
    );
    assert!(delta(&src, &highlights, 235) > 3, "highlights did not lift");
    assert!(
        delta(&src, &highlights, 24).abs() <= 2,
        "the highlights region reached the shadows by {}",
        delta(&src, &highlights, 24)
    );
}

/// The weights sum to one, verified through the shader rather than in Rust.
///
/// `pe_core::parametric` proves the arithmetic; this proves the shader is
/// running that arithmetic. Four equal regions must come out as a plain
/// exposure change — the same multiplier on every tone — because adding a
/// constant in log is multiplying in linear.
#[test]
fn four_equal_regions_are_a_uniform_exposure_change() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = ramp();
    let out = render(
        gpu,
        &src,
        &parametric(&[
            ("param_shadows", 0.5),
            ("param_darks", 0.5),
            ("param_lights", 0.5),
            ("param_highlights", 0.5),
        ]),
    );

    // Below 255 at the top and above the 8-bit floor at the bottom, so neither
    // end is measuring a clip.
    let ratio = |x: u32| linear(out.pixel(x, 4)[0]) / linear(src.pixel(x, 4)[0]).max(1e-6);
    let reference = ratio(60);
    assert!(
        (reference - 2.0).abs() < 0.25,
        "0.5 on every region should be about a stop, got {reference:.3}x"
    );
    for x in [40u32, 80, 110, 140] {
        let r = ratio(x);
        assert!(
            (r - reference).abs() < reference * 0.06,
            "tone {x} moved by {r:.3}x where tone 60 moved by {reference:.3}x —              the region weights do not sum to one"
        );
    }
}

#[test]
fn moving_a_split_moves_which_tones_respond() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = ramp();
    let low = render(
        gpu,
        &src,
        &parametric(&[("param_shadows", 1.0), ("split_low", 0.15)]),
    );
    let high = render(
        gpu,
        &src,
        &parametric(&[("param_shadows", 1.0), ("split_low", 0.55)]),
    );

    // 8-bit level 40 sits around a third of the way up the tonal range once
    // it is log-encoded — inside the shadows region when the split is high,
    // outside it when the split is low.
    let at_low = delta(&src, &low, 40);
    let at_high = delta(&src, &high, 40);
    assert!(
        at_high > at_low + 3,
        "raising the first split should give the shadows region more of the \
         midtones ({at_high} vs {at_low})"
    );
}

// ---------------------------------------------------------------------------
// Colour mixer
// ---------------------------------------------------------------------------

/// A full turn of the hue circle across x, one degree per column.
fn hue_sweep() -> DecodedImage {
    let mut pixels = Vec::new();
    for _ in 0..8 {
        for x in 0..360u32 {
            let h = x as f32 / 60.0;
            let c = 0.42_f32;
            let m = 0.12_f32;
            let sector = h.floor() as i32 % 6;
            let f = h - h.floor();
            let (r, g, b) = match sector {
                0 => (c, c * f, 0.0),
                1 => (c * (1.0 - f), c, 0.0),
                2 => (0.0, c, c * f),
                3 => (0.0, c * (1.0 - f), c),
                4 => (c * f, 0.0, c),
                _ => (c, 0.0, c * (1.0 - f)),
            };
            pixels.extend_from_slice(&[
                ((r + m) * 255.0).round() as u8,
                ((g + m) * 255.0).round() as u8,
                ((b + m) * 255.0).round() as u8,
                255,
            ]);
        }
    }
    DecodedImage::new(360, 8, pixels).expect("hue sweep")
}

fn mixer(params: &[(&str, f32)]) -> Document {
    let overrides: Vec<(&str, ParamValue)> = params
        .iter()
        .map(|(k, v)| (*k, ParamValue::Float(*v)))
        .collect();
    look("colour_mixer", &overrides)
}

/// The whole point of a mixer: fix one colour, leave the others.
#[test]
fn a_band_acts_on_its_own_hue_and_not_its_neighbours() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = hue_sweep();
    let out = render(gpu, &src, &mixer(&[("blue_saturation", 0.8)]));

    // 240° is the blue band's centre.
    assert!(
        spread(&out, 240, 4) > spread(&src, 240, 4) + 3,
        "the blue band did not saturate blue"
    );
    // 60° (yellow) and 0° (red) are outside blue's reach entirely — its
    // neighbours are aqua at 180 and purple at 285.
    for x in [0u32, 60, 120] {
        assert!(
            (spread(&out, x, 4) - spread(&src, x, 4)).abs() <= 2,
            "the blue band reached {x}° by {}",
            spread(&out, x, 4) - spread(&src, x, 4)
        );
    }
}

/// Adjacent bands share a hue between them rather than one of them owning it,
/// so the transition has to be gradual in both directions.
#[test]
fn a_band_fades_out_towards_its_neighbours() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = hue_sweep();
    let out = render(gpu, &src, &mixer(&[("blue_luminance", 0.4)]));

    let lift = |x: u32| {
        let p = out.pixel(x, 4);
        let q = src.pixel(x, 4);
        p[0].max(p[1]).max(p[2]) as i32 - q[0].max(q[1]).max(q[2]) as i32
    };
    let centre = lift(240);
    let toward_aqua = lift(210);
    let at_aqua = lift(180);
    assert!(centre > 6, "the blue band did not brighten blue ({centre})");
    assert!(
        toward_aqua < centre && toward_aqua > at_aqua,
        "blue should fade out towards aqua, not stop dead \
         (240°: {centre}, 210°: {toward_aqua}, 180°: {at_aqua})"
    );
    assert!(
        at_aqua.abs() <= 2,
        "blue reached aqua's centre by {at_aqua}"
    );
}

/// Every band weighted the same must come out as a plain exposure change.
///
/// This is the seam test. If the band weights did not sum to one at every hue
/// — including across the wrap from magenta back to red — some hues would get
/// less than the others and the mixer would band a smooth gradient.
#[test]
fn every_band_together_lifts_every_hue_equally() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = hue_sweep();
    let all: Vec<(&str, f32)> = [
        "red_luminance",
        "orange_luminance",
        "yellow_luminance",
        "green_luminance",
        "aqua_luminance",
        "blue_luminance",
        "purple_luminance",
        "magenta_luminance",
    ]
    .iter()
    .map(|k| (*k, 0.5))
    .collect();
    let out = render(gpu, &src, &mixer(&all));

    let ratio = |x: u32| {
        let p = out.pixel(x, 4);
        let q = src.pixel(x, 4);
        linear(p[0].max(p[1]).max(p[2])) / linear(q[0].max(q[1]).max(q[2])).max(1e-6)
    };
    let reference = ratio(0);
    for x in (0..360).step_by(30) {
        let r = ratio(x);
        assert!(
            (r - reference).abs() < reference * 0.08,
            "{x}° moved by {r:.3}x where 0° moved by {reference:.3}x — \
             the band weights do not sum to one"
        );
    }
}
