//! Radial Blur, Zoom Blur and Color Stabilizer.
//!
//! The two blurs are the same machinery pointed in different directions, so
//! most of what is worth testing is that they are pointed in the directions
//! they claim: a rotational blur must smear along the arc and leave the centre
//! alone, and a zoom blur must smear along the radius and do the same. Get one
//! of them backwards and the result still looks like a blur.

use pe_core::{Document, ParamValue, RowId, StackRow};
use pe_io::DecodedImage;
use pe_render::{EffectRenderer, GpuContext};

fn render(gpu: &GpuContext, src: &DecodedImage, doc: &Document) -> DecodedImage {
    let renderer = EffectRenderer::new(&gpu.device);
    let pixels = pe_render::render_full(gpu, &renderer, src.width, src.height, &src.pixels, doc)
        .expect("export");
    DecodedImage::new(src.width, src.height, pixels).expect("decoded")
}

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

/// A single bright dot on black, `n` pixels from the centre along +x.
///
/// The cleanest thing to blur: whatever the dot turns into is the shape of the
/// blur, with nothing else in the frame to confuse it.
fn dot(size: u32, offset: u32) -> DecodedImage {
    let mut px = vec![0u8; (size * size * 4) as usize];
    let c = size / 2;
    for y in (c - 1)..=(c + 1) {
        for x in (c + offset - 1)..=(c + offset + 1) {
            let i = ((y * size + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    DecodedImage::new(size, size, px).expect("dot")
}

/// The brightest pixel's position and value.
fn brightest(img: &DecodedImage) -> (u32, u32, u8) {
    let mut best = (0u32, 0u32, 0u8);
    for y in 0..img.height {
        for x in 0..img.width {
            let v = img.pixel(x, y)[0];
            if v > best.2 {
                best = (x, y, v);
            }
        }
    }
    best
}

/// How much of a row has anything in it at all.
fn lit_in_row(img: &DecodedImage, y: u32, threshold: u8) -> usize {
    (0..img.width)
        .filter(|x| img.pixel(*x, y)[0] > threshold)
        .count()
}

fn lit_in_column(img: &DecodedImage, x: u32, threshold: u8) -> usize {
    (0..img.height)
        .filter(|y| img.pixel(x, *y)[0] > threshold)
        .count()
}

// ---------------------------------------------------------------------------
// Radial Blur
// ---------------------------------------------------------------------------

/// A rotational blur smears along the arc, which for a dot to the right of the
/// centre means vertically. Smearing it horizontally would be a zoom blur, and
/// the two look similar enough on a photograph that only a test tells them
/// apart.
#[test]
fn a_radial_blur_smears_along_the_arc() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = dot(128, 40);
    let out = render(
        gpu,
        &src,
        &look("radial_blur", &[("strength", ParamValue::Float(1.0))]),
    );

    let across = lit_in_row(&out, 64, 20);
    let along = lit_in_column(&out, 104, 20);
    assert!(
        along > across * 2,
        "the dot spread {across} across and {along} along — that is not an arc"
    );
}

/// The centre of rotation does not move, so nothing there can smear.
#[test]
fn a_radial_blur_leaves_its_own_centre_alone() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = dot(128, 0);
    let out = render(
        gpu,
        &src,
        &look("radial_blur", &[("strength", ParamValue::Float(1.0))]),
    );
    let (_, _, peak) = brightest(&out);
    assert!(
        peak > 200,
        "a dot at the centre of rotation was blurred away to {peak}"
    );
}

#[test]
fn moving_the_centre_moves_what_stays_sharp() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = dot(128, 40);
    // Put the centre of rotation on the dot: it should survive.
    let out = render(
        gpu,
        &src,
        &look(
            "radial_blur",
            &[
                ("strength", ParamValue::Float(1.0)),
                ("center_x", ParamValue::Float(104.0 / 128.0)),
                ("center_y", ParamValue::Float(0.5)),
            ],
        ),
    );
    let (_, _, peak) = brightest(&out);
    assert!(peak > 200, "the dot under the new centre smeared to {peak}");
}

/// Channel Adjustment is what makes this a chromatic streak rather than plain
/// motion, so a channel set to zero must come through untouched.
#[test]
fn a_channel_at_zero_is_not_blurred() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = dot(128, 40);
    let out = render(
        gpu,
        &src,
        &look(
            "radial_blur",
            &[
                ("strength", ParamValue::Float(1.0)),
                ("red", ParamValue::Float(0.0)),
                ("green", ParamValue::Float(1.0)),
                ("blue", ParamValue::Float(1.0)),
            ],
        ),
    );
    // The dot's own pixels: red kept all of its light, green gave most away.
    let p = out.pixel(104, 64);
    assert!(
        p[0] as i32 > p[1] as i32 + 30,
        "red was blurred along with green, got {p:?}"
    );
}

// ---------------------------------------------------------------------------
// Zoom Blur
// ---------------------------------------------------------------------------

/// The other direction: along the radius, so a dot to the right of the centre
/// smears horizontally.
#[test]
fn a_zoom_blur_smears_along_the_radius() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = dot(128, 40);
    let out = render(
        gpu,
        &src,
        &look("zoom_blur", &[("strength", ParamValue::Float(1.0))]),
    );

    let across = lit_in_row(&out, 64, 20);
    let along = lit_in_column(&out, 104, 20);
    assert!(
        across > along * 2,
        "the dot spread {across} across and {along} along — that is not a radius"
    );
}

#[test]
fn a_zoom_blur_leaves_its_own_centre_alone() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = dot(128, 0);
    let out = render(
        gpu,
        &src,
        &look("zoom_blur", &[("strength", ParamValue::Float(1.0))]),
    );
    let (_, _, peak) = brightest(&out);
    assert!(peak > 200, "a dot at the centre smeared to {peak}");
}

/// Asymmetric puts every sample on one side, which reads as a trail rather
/// than as motion with no direction. If it did not, the two settings would be
/// the same control.
#[test]
fn asymmetric_streaks_one_way_and_symmetric_streaks_both() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = dot(128, 30);
    let with = |mode: &str| {
        render(
            gpu,
            &src,
            &look(
                "zoom_blur",
                &[
                    ("strength", ParamValue::Float(1.0)),
                    ("symmetry", ParamValue::Choice(mode.into())),
                ],
            ),
        )
    };
    let symmetric = with("Symmetric");
    let asymmetric = with("Asymmetric");

    // Outward of the dot, away from the centre.
    //
    // Both settings reach *inward* by the same amount: a sample taken further
    // out pulls that content towards the centre either way. What tells them
    // apart is the far side — symmetric also samples inward, which throws the
    // streak outward, and asymmetric never does.
    let outward = |img: &DecodedImage| (98..124u32).filter(|x| img.pixel(*x, 64)[0] > 20).count();
    assert!(
        outward(&symmetric) > outward(&asymmetric) + 2,
        "symmetric should streak outward and asymmetric should not ({} against {})",
        outward(&symmetric),
        outward(&asymmetric)
    );
}

// ---------------------------------------------------------------------------
// Color Stabilizer
// ---------------------------------------------------------------------------

fn flat(w: u32, h: u32, c: [u8; 3]) -> DecodedImage {
    let px: Vec<u8> = std::iter::repeat_n([c[0], c[1], c[2], 255], (w * h) as usize)
        .flatten()
        .collect();
    DecodedImage::new(w, h, px).unwrap()
}

/// What the effect is for: point it at something that should be grey and it
/// works out the white balance.
#[test]
fn stabilizing_white_balance_neutralises_a_cast() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat(64, 64, [190, 150, 110]);
    let out = render(
        gpu,
        &src,
        &look("color_stabilizer", &[("strength", ParamValue::Float(1.0))]),
    );
    let p = out.pixel(32, 32);
    let spread = p[0].max(p[1]).max(p[2]) as i32 - p[0].min(p[1]).min(p[2]) as i32;
    assert!(
        spread < 8,
        "a warm cast survived the correction: {p:?} is still {spread} apart"
    );
}

/// White balance and brightness are two controls, and doing both when only one
/// was asked for would make the other impossible to turn off.
#[test]
fn stabilizing_white_balance_does_not_change_the_exposure() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat(64, 64, [190, 150, 110]);
    let out = render(
        gpu,
        &src,
        &look("color_stabilizer", &[("strength", ParamValue::Float(1.0))]),
    );
    let before = 0.2126 * 190.0 + 0.7152 * 150.0 + 0.0722 * 110.0;
    let p = out.pixel(32, 32);
    let after = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
    assert!(
        (after - before).abs() < 14.0,
        "the balance correction moved the exposure from {before:.0} to {after:.0}"
    );
}

#[test]
fn stabilizing_brightness_pulls_the_region_towards_mid_grey() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let dark = flat(64, 64, [40, 40, 40]);
    let out = render(
        gpu,
        &dark,
        &look(
            "color_stabilizer",
            &[
                ("strength", ParamValue::Float(1.0)),
                ("stabilize_wb", ParamValue::Bool(false)),
                ("stabilize_brightness", ParamValue::Bool(true)),
            ],
        ),
    );
    let v = out.pixel(32, 32)[0] as i32;
    // 18% in linear is about 118 in sRGB.
    assert!(
        (v - 118).abs() < 14,
        "a dark frame came out at {v}, not near mid grey"
    );
}

/// The gate. Resolve does nothing until Analyze Now; ours does nothing until
/// the strength is turned up, and a corrective tool that moves the picture the
/// moment it is added is surprising either way.
#[test]
fn the_stabilizer_does_nothing_until_its_strength_is_raised() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat(64, 64, [190, 150, 110]);
    let out = render(gpu, &src, &look("color_stabilizer", &[]));
    for c in 0..3 {
        assert_eq!(
            out.pixel(32, 32)[c],
            src.pixel(32, 32)[c],
            "the stabilizer applied itself at zero strength"
        );
    }
}

/// A region that is not the whole frame has to actually be used, or the
/// Analysis Region controls are decoration.
#[test]
fn a_selected_area_measures_that_area_and_not_the_frame() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    // Warm on the left, neutral on the right.
    let mut px = Vec::new();
    for _ in 0..64u32 {
        for x in 0..128u32 {
            let c: [u8; 4] = if x < 64 {
                [210, 150, 90, 255]
            } else {
                [150, 150, 150, 255]
            };
            px.extend_from_slice(&c);
        }
    }
    let src = DecodedImage::new(128, 64, px).unwrap();

    let of = |x: f32| {
        render(
            gpu,
            &src,
            &look(
                "color_stabilizer",
                &[
                    ("strength", ParamValue::Float(1.0)),
                    ("region", ParamValue::Choice("Selected Area".into())),
                    ("source_x", ParamValue::Float(x)),
                    ("source_y", ParamValue::Float(0.5)),
                    ("source_width", ParamValue::Float(0.2)),
                    ("source_height", ParamValue::Float(0.5)),
                ],
            ),
        )
    };

    // Measuring the warm half corrects it to neutral.
    let from_warm = of(0.25);
    let p = from_warm.pixel(20, 32);
    let spread = p[0].max(p[1]).max(p[2]) as i32 - p[0].min(p[1]).min(p[2]) as i32;
    assert!(
        spread < 10,
        "measuring the warm half left it {spread} apart"
    );

    // Measuring the neutral half finds nothing to correct, so the warm half
    // stays warm.
    let from_neutral = of(0.75);
    let q = from_neutral.pixel(20, 32);
    assert!(
        q[0] as i32 > q[2] as i32 + 40,
        "the region was ignored: the warm half came back as {q:?}"
    );
}

// ---------------------------------------------------------------------------
// Noise Reduction
// ---------------------------------------------------------------------------

/// A flat field with per-pixel noise on it, and a hard edge down the middle.
///
/// Both halves of the job in one picture: the noise is what should go, the
/// edge is what should stay, and an effect that cannot tell them apart is a
/// blur with a different name.
fn noisy_edge(size: u32) -> DecodedImage {
    let mut px = Vec::new();
    for y in 0..size {
        for x in 0..size {
            let base: i32 = if x < size / 2 { 70 } else { 170 };
            // Deterministic, so the test means the same thing every run.
            let n = ((x * 7919 + y * 104_729) % 41) as i32 - 20;
            let v = (base + n).clamp(0, 255) as u8;
            px.extend_from_slice(&[v, v, v, 255]);
        }
    }
    DecodedImage::new(size, size, px).expect("noisy edge")
}

fn roughness(img: &DecodedImage, y: u32, from: u32, to: u32) -> f32 {
    let values: Vec<f32> = (from..to).map(|x| img.pixel(x, y)[0] as f32).collect();
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    (values.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / values.len() as f32).sqrt()
}

/// The property that separates noise reduction from blur: the noise goes and
/// the edge stays. A plain average would take both.
#[test]
fn noise_reduction_smooths_the_noise_and_keeps_the_edge() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = noisy_edge(128);
    let out = render(
        gpu,
        &src,
        &look(
            "noise_reduction",
            &[
                ("luma_threshold", ParamValue::Float(0.5)),
                ("radius", ParamValue::Float(2.0)),
            ],
        ),
    );

    let before = roughness(&src, 64, 8, 56);
    let after = roughness(&out, 64, 8, 56);
    assert!(
        after < before * 0.7,
        "the noise survived: {before:.1} became {after:.1}"
    );

    // The edge is still an edge.
    let step = out.pixel(66, 64)[0] as i32 - out.pixel(61, 64)[0] as i32;
    assert!(
        step > 60,
        "the edge was smoothed away along with the noise (step of {step})"
    );
}

/// Chroma noise is coarse and almost free to remove; luma noise sits on top of
/// real detail. One threshold for both would mean choosing which of those two
/// mistakes to make, so each has to work on its own.
#[test]
fn the_two_thresholds_act_on_different_noise() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    // Colour noise on a flat grey: the channels disagree, the luminance does
    // not.
    //
    // Big enough that the radius is worth something. It is a fraction of the
    // frame — so that a preview and an export smooth the same real detail —
    // which means a small fixture asks the effect to average over less than a
    // pixel and then complains that nothing happened.
    const SIZE: u32 = 256;
    let mut px = Vec::new();
    for y in 0..SIZE {
        for x in 0..SIZE {
            let n = ((x * 7919 + y * 104_729) % 31) as i32 - 15;
            px.extend_from_slice(&[
                (120 + n).clamp(0, 255) as u8,
                120,
                (120 - n).clamp(0, 255) as u8,
                255,
            ]);
        }
    }
    let src = DecodedImage::new(SIZE, SIZE, px).unwrap();

    let spread = |img: &DecodedImage| {
        let mut worst = 0i32;
        for y in 32..224u32 {
            for x in 32..224u32 {
                let p = img.pixel(x, y);
                worst = worst.max(p[0] as i32 - p[2] as i32);
            }
        }
        worst
    };

    let chroma = render(
        gpu,
        &src,
        &look(
            "noise_reduction",
            &[
                ("chroma_threshold", ParamValue::Float(1.0)),
                ("mode", ParamValue::Choice("Enhanced".into())),
                ("radius", ParamValue::Float(3.0)),
            ],
        ),
    );
    let luma_only = render(
        gpu,
        &src,
        &look(
            "noise_reduction",
            &[
                ("luma_threshold", ParamValue::Float(1.0)),
                ("mode", ParamValue::Choice("Enhanced".into())),
                ("radius", ParamValue::Float(3.0)),
            ],
        ),
    );

    assert!(
        spread(&chroma) < spread(&src) / 2,
        "the chroma threshold did not touch colour noise ({} against {})",
        spread(&chroma),
        spread(&src)
    );
    assert!(
        spread(&luma_only) > spread(&src) * 3 / 4,
        "the luma threshold removed colour noise, which is not its job"
    );
}

#[test]
fn noise_reduction_does_nothing_until_a_threshold_is_raised() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = noisy_edge(64);
    let out = render(gpu, &src, &look("noise_reduction", &[]));
    for y in [16u32, 32, 48] {
        for x in [16u32, 32, 48] {
            assert_eq!(
                out.pixel(x, y)[0],
                src.pixel(x, y)[0],
                "an untouched Noise Reduction row changed the picture"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Film Look Creator
// ---------------------------------------------------------------------------

/// A black-to-white ramp across x.
fn ramp() -> DecodedImage {
    let mut px = Vec::new();
    for _ in 0..8 {
        for x in 0..256u32 {
            let v = x as u8;
            px.extend_from_slice(&[v, v, v, 255]);
        }
    }
    DecodedImage::new(256, 8, px).expect("ramp")
}

/// The shoulder is the point of the whole effect. Film has no clipping point —
/// density keeps rising, ever more slowly — so a highlight rolls off rather
/// than stopping, and a shoulder that flattened would be a clip with extra
/// steps.
#[test]
fn the_highlight_rolloff_compresses_without_flattening() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = ramp();
    let out = render(
        gpu,
        &src,
        &look(
            "film_look",
            &[
                ("highlight_rolloff", ParamValue::Float(2.0)),
                ("shadow_rolloff", ParamValue::Float(0.0)),
                ("film_contrast", ParamValue::Float(1.0)),
                ("film_saturation", ParamValue::Float(1.0)),
                ("shadow_tone", ParamValue::Float(0.0)),
                ("highlight_tone", ParamValue::Float(0.0)),
            ],
        ),
    );

    let top = out.pixel(250, 4)[0] as i32;
    let src_top = src.pixel(250, 4)[0] as i32;
    assert!(top < src_top - 4, "the highlights did not roll off");
    // Still climbing all the way to the end.
    assert!(
        out.pixel(255, 4)[0] as i32 > out.pixel(235, 4)[0] as i32,
        "the shoulder flattened into a clip"
    );
    // And the shadows are untouched by a highlight control.
    assert!(
        (out.pixel(40, 4)[0] as i32 - src.pixel(40, 4)[0] as i32).abs() <= 3,
        "the highlight rolloff reached the shadows"
    );
}

/// Choosing a stock changes the base the sliders modulate rather than
/// overwriting them, so the three have to differ with the sliders left alone.
#[test]
fn the_three_stocks_are_actually_different() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = ramp();
    let of = |stock: &str| {
        render(
            gpu,
            &src,
            &look("film_look", &[("stock", ParamValue::Choice(stock.into()))]),
        )
    };
    let negative = of("Colour Negative");
    let reversal = of("Reversal");

    // Reversal is the harder stock, so the two ends pull apart.
    let range = |img: &DecodedImage| img.pixel(230, 4)[0] as i32 - img.pixel(25, 4)[0] as i32;
    assert!(
        range(&reversal) > range(&negative) + 6,
        "reversal should be the harder stock ({} against {})",
        range(&reversal),
        range(&negative)
    );
}

/// Toning shifts the colour without lifting the level. A tint that also
/// brightened would be two controls in one, and the second one impossible to
/// turn off.
#[test]
fn split_toning_colours_the_ends_without_moving_them() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = ramp();
    let plain = render(
        gpu,
        &src,
        &look(
            "film_look",
            &[
                ("shadow_tone", ParamValue::Float(0.0)),
                ("highlight_tone", ParamValue::Float(0.0)),
            ],
        ),
    );
    let toned = render(
        gpu,
        &src,
        &look(
            "film_look",
            &[
                ("shadow_hue", ParamValue::Float(210.0)),
                ("shadow_tone", ParamValue::Float(1.0)),
                ("highlight_tone", ParamValue::Float(0.0)),
            ],
        ),
    );

    // The shadows went blue.
    let p = toned.pixel(30, 4);
    assert!(
        p[2] as i32 > p[0] as i32 + 4,
        "the shadow tone did not reach the shadows, got {p:?}"
    );
    // And they are the same brightness they were.
    let before = 0.2126 * plain.pixel(30, 4)[0] as f32
        + 0.7152 * plain.pixel(30, 4)[1] as f32
        + 0.0722 * plain.pixel(30, 4)[2] as f32;
    let after = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
    assert!(
        (after - before).abs() < 6.0,
        "toning moved the level from {before:.0} to {after:.0}"
    );
    // The highlights were left alone.
    assert!(
        (toned.pixel(230, 4)[2] as i32 - plain.pixel(230, 4)[2] as i32).abs() <= 3,
        "a shadow tone reached the highlights"
    );
}
