//! Dehaze, Bloom and Film Damage.
//!
//! Kept apart from `looks.rs` because these three are testable against
//! something stronger than "it changed": Dehaze can be pointed at a synthetic
//! haze it should invert, Bloom has a colour it must *not* have, and Film
//! Damage places artefacts at coordinates that can be checked.

use pe_core::{Document, ParamValue, RowId, StackRow};
use pe_io::DecodedImage;
use pe_render::{EffectRenderer, GpuContext};

const TOLERANCE: u8 = 2;

fn render(gpu: &GpuContext, src: &DecodedImage, doc: &Document) -> DecodedImage {
    let renderer = EffectRenderer::new(&gpu.device);
    let pixels = pe_render::render_full(gpu, &renderer, src.width, src.height, &src.pixels, doc)
        .expect("export");
    DecodedImage::new(src.width, src.height, pixels).expect("decoded")
}

fn look(effect: &str, params: &[(&str, ParamValue)]) -> Document {
    let mut doc = Document::from_path("chart.png");
    let def = pe_effects::by_key(effect).expect("effect exists");
    let mut row = StackRow::new(RowId(0), effect);
    row.params = def.default_params();
    for (k, v) in params {
        row.params.set(*k, v.clone());
    }
    doc.stack.push(row);
    doc
}

/// Film Damage with its colour shifts silenced.
///
/// Resolve ships it with Temp. Shift at 0.25 and Tint Shift at -0.1, so a
/// freshly added row already moves the colour of the whole frame. A test about
/// the vignette, the dirt or one scratch is not a test about that, and leaving
/// them on would have every one of them measuring two things at once.
fn damage(params: &[(&str, ParamValue)]) -> Document {
    let mut all = vec![
        ("temp_shift", ParamValue::Float(0.0)),
        ("tint_shift", ParamValue::Float(0.0)),
    ];
    all.extend(params.iter().cloned());
    look("film_damage", &all)
}

fn chart() -> DecodedImage {
    pe_io::test_chart(256, 192)
}

/// Build a hazy image from a clean one using the same scattering model Dehaze
/// inverts: `observed = scene * t + haze * (1 - t)`, with transmission falling
/// off toward the top of the frame the way aerial perspective does.
///
/// Synthesising the haze gives a real ground truth to measure against, rather
/// than only asserting that something changed.
fn hazy(src: &DecodedImage, haze: [f64; 3]) -> DecodedImage {
    let mut pixels = Vec::with_capacity(src.pixels.len());
    for y in 0..src.height {
        // Top of the frame is most distant, so least transmission.
        let t = 0.25 + 0.7 * (y as f64 / (src.height.max(2) - 1) as f64);
        for x in 0..src.width {
            let p = src.pixel(x, y);
            for ch in 0..3 {
                let lin = pe_color::TransferFn::Srgb.decode(p[ch] as f64 / 255.0);
                let observed = lin * t + haze[ch] * (1.0 - t);
                let enc = pe_color::TransferFn::Srgb.encode(observed);
                pixels.push((enc.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            pixels.push(255);
        }
    }
    DecodedImage::new(src.width, src.height, pixels).expect("hazy image")
}

/// Mean absolute per-channel distance between two images over a band of rows.
fn distance(a: &DecodedImage, b: &DecodedImage, y0: u32, y1: u32) -> f64 {
    let mut sum = 0.0;
    let mut n = 0.0;
    for y in y0..y1 {
        for x in 0..a.width {
            for ch in 0..3 {
                sum += (a.pixel(x, y)[ch] as f64 - b.pixel(x, y)[ch] as f64).abs();
                n += 1.0;
            }
        }
    }
    sum / n
}

// ---------------------------------------------------------------------------
// Dehaze
// ---------------------------------------------------------------------------

#[test]
fn dehaze_moves_a_hazy_image_back_toward_the_original() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let clean = chart();
    let haze = [0.62, 0.66, 0.72];
    let veiled = hazy(&clean, haze);

    let doc = look(
        "dehaze",
        &[
            ("strength", ParamValue::Float(1.0)),
            (
                "haze_color",
                ParamValue::Rgb([haze[0] as f32, haze[1] as f32, haze[2] as f32]),
            ),
        ],
    );
    let recovered = render(gpu, &veiled, &doc);

    // Only the top of the frame is heavily hazed, so measure there.
    let before = distance(&veiled, &clean, 0, clean.height / 4);
    let after = distance(&recovered, &clean, 0, clean.height / 4);
    assert!(
        after < before * 0.85,
        "dehaze moved the image from {before:.1} to {after:.1} away from the original"
    );
}

#[test]
fn negative_dehaze_adds_haze_instead() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let clean = chart();
    let out = render(
        gpu,
        &clean,
        &look("dehaze", &[("strength", ParamValue::Float(-1.0))]),
    );

    // Adding haze lifts the blacks and flattens contrast, so the dark end of
    // the neutral ramp must get brighter.
    let before = clean.pixel(4, 2)[0];
    let after = out.pixel(4, 2)[0];
    assert!(
        after > before,
        "negative dehaze should lift shadows: {before} -> {after}"
    );
}

#[test]
fn display_depth_shows_a_neutral_matte() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let veiled = hazy(&chart(), [0.62, 0.66, 0.72]);
    let out = render(
        gpu,
        &veiled,
        &look("dehaze", &[("display_depth", ParamValue::Bool(true))]),
    );

    // A depth matte is greyscale by definition.
    for (x, y) in [(20u32, 10u32), (128, 96), (200, 150)] {
        let p = out.pixel(x, y);
        let spread = p[0].max(p[1]).max(p[2]) as i32 - p[0].min(p[1]).min(p[2]) as i32;
        assert!(
            spread <= 3,
            "depth matte is not neutral at ({x},{y}): {p:?}"
        );
    }
}

/// The depth estimate has to vary with the haze, or the whole effect is a
/// uniform colour shift wearing a depth map's name.
#[test]
fn the_depth_matte_tracks_the_haze_gradient() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let veiled = hazy(&chart(), [0.62, 0.66, 0.72]);
    let out = render(
        gpu,
        &veiled,
        &look("dehaze", &[("display_depth", ParamValue::Bool(true))]),
    );

    // Transmission is lowest at the top of the synthetic haze and highest at
    // the bottom, so the matte should be darker up top.
    let band = |y: u32| -> f64 {
        (0..out.width)
            .map(|x| out.pixel(x, y)[0] as f64)
            .sum::<f64>()
            / out.width as f64
    };
    let far = band(4);
    let near = band(out.height - 5);
    assert!(
        near > far + 4.0,
        "depth matte is flat: far={far:.1}, near={near:.1}"
    );
}

// ---------------------------------------------------------------------------
// Bloom
// ---------------------------------------------------------------------------

#[test]
fn bloom_spills_from_highlights_and_leaves_shadows_alone() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();
    let out = render(
        gpu,
        &src,
        &look(
            "bloom",
            &[
                ("amount", ParamValue::Float(1.0)),
                ("radius", ParamValue::Float(0.06)),
                ("threshold", ParamValue::Float(0.5)),
            ],
        ),
    );

    let shadow = out.pixel(2, 2)[0] as i32 - src.pixel(2, 2)[0] as i32;
    assert!(
        shadow.abs() <= 3,
        "bloom leaked into the shadows by {shadow}"
    );

    let bright: i32 = (200..240)
        .map(|x| out.pixel(x, 2)[0] as i32 - src.pixel(x, 2)[0] as i32)
        .sum();
    assert!(bright > 0, "highlights gained no bloom");
}

/// Bloom is colourless; Halation is not. If Bloom ever picks up a cast, the
/// two effects have been conflated — which is exactly why Resolve ships them
/// separately rather than as one effect with a tint control.
#[test]
fn bloom_is_neutral_in_colour() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();
    let out = render(
        gpu,
        &src,
        &look(
            "bloom",
            &[
                ("amount", ParamValue::Float(1.0)),
                ("radius", ParamValue::Float(0.06)),
                ("threshold", ParamValue::Float(0.3)),
            ],
        ),
    );

    for x in (8..248).step_by(24) {
        let p = out.pixel(x, 2);
        let spread = p[0].max(p[1]).max(p[2]) as i32 - p[0].min(p[1]).min(p[2]) as i32;
        assert!(spread <= 3, "bloom tinted the neutral ramp at x={x}: {p:?}");
    }
}

#[test]
fn the_bloom_threshold_decides_what_glows() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();
    let spill = |threshold: f32| -> f64 {
        let out = render(
            gpu,
            &src,
            &look(
                "bloom",
                &[
                    ("amount", ParamValue::Float(1.0)),
                    ("radius", ParamValue::Float(0.06)),
                    ("threshold", ParamValue::Float(threshold)),
                ],
            ),
        );
        distance(&out, &src, 0, src.height / 4)
    };
    // A low threshold lets the midtones glow too, so more of the image moves.
    assert!(
        spill(0.2) > spill(2.0),
        "raising the threshold did not restrict the glow"
    );
}

// ---------------------------------------------------------------------------
// Film Damage
// ---------------------------------------------------------------------------

/// The reason there are five independent scratch groups rather than one with a
/// count: position has to be per-scratch.
#[test]
fn each_scratch_lands_where_it_is_placed() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();
    let out = render(
        gpu,
        &src,
        &damage(&[
            ("scratch1_position", ParamValue::Float(0.25)),
            ("scratch1_width", ParamValue::Float(0.012)),
            ("scratch1_strength", ParamValue::Float(1.0)),
            ("scratch2_position", ParamValue::Float(0.75)),
            ("scratch2_width", ParamValue::Float(0.012)),
            ("scratch2_strength", ParamValue::Float(1.0)),
        ]),
    );

    let changed = |x: u32| -> i64 {
        (0..src.height)
            .map(|y| (out.pixel(x, y)[0] as i64 - src.pixel(x, y)[0] as i64).abs())
            .sum()
    };
    let first = changed((0.25 * src.width as f32) as u32);
    let second = changed((0.75 * src.width as f32) as u32);
    let between = changed((0.50 * src.width as f32) as u32);

    assert!(first > 0, "scratch 1 did not appear at 0.25");
    assert!(second > 0, "scratch 2 did not appear at 0.75");
    assert!(
        between * 4 < first,
        "the gap between the scratches was damaged too ({between} vs {first})"
    );
}

#[test]
fn dirt_density_controls_how_much_dirt_appears() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();
    let count = |density: f32| -> usize {
        let out = render(
            gpu,
            &src,
            &damage(&[
                ("dirt_density", ParamValue::Float(density)),
                ("dirt_size", ParamValue::Float(1.0)),
            ]),
        );
        src.pixels
            .iter()
            .zip(&out.pixels)
            .filter(|(a, b)| a.abs_diff(**b) > 8)
            .count()
    };

    let sparse = count(0.5);
    let dense = count(6.0);
    assert!(sparse > 0, "no dirt at all at density 0.5");
    assert!(
        dense > sparse * 2,
        "density barely changed the amount of dirt ({sparse} -> {dense})"
    );
}

#[test]
fn film_damage_vignetting_darkens_the_corners() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();
    let out = render(
        gpu,
        &src,
        &damage(&[
            ("focal_factor", ParamValue::Float(0.8)),
            ("geometry_factor", ParamValue::Float(0.9)),
        ]),
    );

    let corner_before = src.pixel(2, 190)[0];
    let corner_after = out.pixel(2, 190)[0];
    assert!(
        corner_after < corner_before,
        "corner went {corner_before} -> {corner_after}"
    );

    let centre_before = src.pixel(128, 96);
    let centre_after = out.pixel(128, 96);
    assert!(
        centre_after[0].abs_diff(centre_before[0]) <= 6,
        "centre moved {centre_before:?} -> {centre_after:?}"
    );
}

#[test]
fn temp_shift_warms_and_cools() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();
    let red_over_blue = |shift: f32| -> f32 {
        let out = render(
            gpu,
            &src,
            &look("film_damage", &[("temp_shift", ParamValue::Float(shift))]),
        );
        let p = out.pixel(160, 2);
        p[0] as f32 / p[2].max(1) as f32
    };
    let warm = red_over_blue(0.8);
    let cool = red_over_blue(-0.8);
    assert!(
        warm > cool,
        "positive temp shift should warm the image ({warm:.3} vs {cool:.3})"
    );
}

#[test]
fn film_damage_reference() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let out = render(
        gpu,
        &chart(),
        &look(
            "film_damage",
            &[
                ("film_blur", ParamValue::Float(0.4)),
                ("temp_shift", ParamValue::Float(0.3)),
                ("tint_shift", ParamValue::Float(0.2)),
                ("focal_factor", ParamValue::Float(0.6)),
                ("dirt_density", ParamValue::Float(0.25)),
                ("scratch1_strength", ParamValue::Float(0.8)),
                ("scratch3_strength", ParamValue::Float(0.5)),
            ],
        ),
    );
    pe_golden::assert_matches("film_damage_worn_print", &out, TOLERANCE);
}

/// A flat mid-grey frame, so a scratch is the only thing in it.
fn flat(w: u32, h: u32) -> DecodedImage {
    let px: Vec<u8> = std::iter::repeat_n([120u8, 120, 120, 255], (w * h) as usize)
        .flatten()
        .collect();
    DecodedImage::new(w, h, px).unwrap()
}

/// Each scratch carries its own colour, which is the reason Resolve gives
/// them one each. A strip of film that has been through a projector has a
/// sharp black gouge on the negative and a soft white one on the print, and
/// one shared colour cannot say both.
#[test]
fn each_scratch_uses_its_own_colour() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat(256, 64);
    let out = render(
        gpu,
        &src,
        &look(
            "film_damage",
            &[
                ("scratch1_position", ParamValue::Float(0.25)),
                ("scratch1_width", ParamValue::Float(0.03)),
                ("scratch1_strength", ParamValue::Float(1.0)),
                ("scratch1_blur", ParamValue::Float(0.0)),
                ("scratch1_color", ParamValue::Rgb([1.0, 1.0, 1.0])),
                ("scratch2_position", ParamValue::Float(0.75)),
                ("scratch2_width", ParamValue::Float(0.03)),
                ("scratch2_strength", ParamValue::Float(1.0)),
                ("scratch2_blur", ParamValue::Float(0.0)),
                ("scratch2_color", ParamValue::Rgb([0.0, 0.0, 0.0])),
            ],
        ),
    );

    let white = out.pixel(64, 32)[0] as i32;
    let black = out.pixel(192, 32)[0] as i32;
    let clean = out.pixel(128, 32)[0] as i32;
    assert!(
        white > clean + 40,
        "the first scratch should be white ({white} against {clean})"
    );
    assert!(
        black < clean - 40,
        "the second should be black ({black} against {clean})"
    );
}

/// Enable silences a scratch you have set up. Without it the only way to turn
/// one off is to zero its strength, which loses the setting.
#[test]
fn disabling_a_scratch_removes_it_without_touching_its_settings() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat(256, 64);
    let settings = |enabled: bool| {
        damage(&[
            ("scratch1_position", ParamValue::Float(0.5)),
            ("scratch1_width", ParamValue::Float(0.04)),
            ("scratch1_strength", ParamValue::Float(1.0)),
            ("scratch1_color", ParamValue::Rgb([1.0, 1.0, 1.0])),
            ("scratch1_enable", ParamValue::Bool(enabled)),
        ])
    };
    let on = render(gpu, &src, &settings(true));
    let off = render(gpu, &src, &settings(false));

    assert!(
        on.pixel(128, 32)[0] as i32 > src.pixel(128, 32)[0] as i32 + 40,
        "the scratch was not drawn when enabled"
    );
    assert!(
        (off.pixel(128, 32)[0] as i32 - src.pixel(128, 32)[0] as i32).abs() <= 2,
        "a disabled scratch still drew"
    );
}

/// Pins the two shifts to their slots. They sit at 1 and 2, ahead of the
/// vignetting group, and reading one off its neighbour would tint the picture
/// when the user asked for a vignette.
#[test]
fn the_temperature_shift_warms_the_picture_and_the_tint_shift_does_not() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat(64, 64);
    let warm = render(
        gpu,
        &src,
        &damage(&[("temp_shift", ParamValue::Float(1.0))]),
    );
    let p = warm.pixel(32, 32);
    assert!(
        p[0] as i32 > p[2] as i32 + 6,
        "a positive temperature shift should warm the picture, got {p:?}"
    );

    let tinted = render(
        gpu,
        &src,
        &damage(&[("tint_shift", ParamValue::Float(1.0))]),
    );
    let q = tinted.pixel(32, 32);
    assert!(
        (q[0] as i32 - q[2] as i32).abs() < 6,
        "a tint shift should move green against magenta, not red against blue, got {q:?}"
    );
}

// ---------------------------------------------------------------------------
// Film Grain
// ---------------------------------------------------------------------------

/// How much a channel varies across a row — the size of the grain's swing.
fn spread_of(img: &DecodedImage, y: u32, channel: usize) -> f32 {
    let values: Vec<f32> = (0..img.width)
        .map(|x| img.pixel(x, y)[channel] as f32)
        .collect();
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    (values.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / values.len() as f32).sqrt()
}

/// How often a row crosses its own average — a stand-in for how fine the grain
/// is. Coarse grain crosses rarely because each grain covers many pixels.
fn crossings(img: &DecodedImage, y: u32) -> usize {
    let values: Vec<f32> = (0..img.width).map(|x| img.pixel(x, y)[0] as f32).collect();
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    values
        .windows(2)
        .filter(|w| (w[0] - mean).is_sign_positive() != (w[1] - mean).is_sign_positive())
        .count()
}

fn grain(params: &[(&str, ParamValue)]) -> Document {
    look("grain", params)
}

/// The reason a format preset is a real control and not a label: the same
/// emulsion on a 16mm frame is magnified nearly three times as much by the
/// time it reaches the same print, so its grain is that much coarser.
#[test]
fn a_format_preset_changes_how_coarse_the_grain_is() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat(512, 32);
    let of = |preset: &str| {
        render(
            gpu,
            &src,
            &grain(&[
                ("preset", ParamValue::Choice(preset.into())),
                ("strength", ParamValue::Float(1.0)),
                // Coarse, in the 0..1 the control now runs in.
                ("size", ParamValue::Float(0.2)),
            ]),
        )
    };
    let small = crossings(&of("16mm"), 16);
    let large = crossings(&of("65mm"), 16);
    assert!(
        large > small * 2,
        "65mm should be much finer than 16mm ({large} crossings against {small})"
    );
}

/// Film's three dye layers are not equally grainy — the blue-sensitive layer
/// is the worst of them — so this is not a trim, it is most of what separates
/// one stock from another.
#[test]
fn a_channel_gain_puts_the_grain_in_that_channel() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat(256, 32);
    let out = render(
        gpu,
        &src,
        &grain(&[
            ("strength", ParamValue::Float(1.0)),
            ("saturation", ParamValue::Float(1.0)),
            ("red", ParamValue::Float(2.0)),
            ("green", ParamValue::Float(0.0)),
            ("blue", ParamValue::Float(0.0)),
        ]),
    );
    let red = spread_of(&out, 16, 0);
    let green = spread_of(&out, 16, 1);
    assert!(
        red > green * 2.0,
        "the grain did not follow the channel gains ({red:.2} red against {green:.2} green)"
    );
}

#[test]
fn grain_only_shows_the_grain_without_the_picture() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let doc = grain(&[
        ("strength", ParamValue::Float(1.0)),
        ("grain_only", ParamValue::Bool(true)),
    ]);
    // Two very different pictures have to give the same grain, or it is not
    // the grain on its own.
    let dark = render(gpu, &flat(128, 32), &doc);
    let bright: Vec<u8> = std::iter::repeat_n([230u8, 210, 190, 255], 128 * 32)
        .flatten()
        .collect();
    let bright = render(gpu, &DecodedImage::new(128, 32, bright).unwrap(), &doc);

    let mut worst = 0i32;
    for y in 0..32 {
        for x in 0..128 {
            for c in 0..3 {
                worst =
                    worst.max((dark.pixel(x, y)[c] as i32 - bright.pixel(x, y)[c] as i32).abs());
            }
        }
    }
    assert!(
        worst <= 6,
        "the picture showed through Grain Only by {worst} levels"
    );
    assert!(
        spread_of(&dark, 16, 0) > 1.0,
        "Grain Only showed no grain at all"
    );
}

/// Overlay leaves the ends of the range alone and puts the grain in the
/// midtones, which is where film puts it. Add does not, which is the whole
/// reason there is a choice.
#[test]
fn the_composite_type_decides_where_the_grain_lands() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    // Near black, where the two composites disagree most.
    let px: Vec<u8> = std::iter::repeat_n([6u8, 6, 6, 255], 256 * 32)
        .flatten()
        .collect();
    let src = DecodedImage::new(256, 32, px).unwrap();
    let with = |mode: &str| {
        render(
            gpu,
            &src,
            &grain(&[
                ("strength", ParamValue::Float(1.0)),
                ("opacity", ParamValue::Float(1.0)),
                ("composite", ParamValue::Choice(mode.into())),
            ]),
        )
    };
    let overlay = spread_of(&with("Overlay"), 16, 0);
    let add = spread_of(&with("Add"), 16, 0);
    assert!(
        add > overlay,
        "Add should put more grain in the blacks than Overlay ({add:.2} against {overlay:.2})"
    );
}

#[test]
fn an_opacity_of_zero_leaves_the_picture_alone() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat(128, 32);
    let out = render(
        gpu,
        &src,
        &grain(&[
            ("strength", ParamValue::Float(1.0)),
            ("opacity", ParamValue::Float(0.0)),
        ]),
    );
    assert!(
        spread_of(&out, 16, 0) < 0.6,
        "grain got through at zero opacity"
    );
}

#[test]
fn dehaze_reference() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    const HAZE: [f64; 3] = [0.62, 0.66, 0.72];
    let veiled = hazy(&chart(), HAZE);
    // Told what colour the haze is, rather than leaning on the default.
    //
    // Resolve ships Haze Color white, which is the honest starting point — it
    // removes whatever haze is there rather than the haze the default assumed.
    // But this fixture built a blue-grey veil on purpose, and a test that did
    // not say so would be measuring how close the default happened to be
    // instead of whether the model inverts what it was given.
    let out = render(
        gpu,
        &veiled,
        &look(
            "dehaze",
            &[
                ("strength", ParamValue::Float(0.9)),
                (
                    "haze_color",
                    ParamValue::Rgb([HAZE[0] as f32, HAZE[1] as f32, HAZE[2] as f32]),
                ),
            ],
        ),
    );
    pe_golden::assert_matches("dehaze_recovered", &out, TOLERANCE);
}
