//! Vignette shaping, and Halation's per-channel relative spread.
//!
//! Both are tested on a synthetic image rather than the chart: a single white
//! square on black gives a clean halo to measure and an unambiguous corner to
//! check, where the chart's ramps and hue sweep would muddy both.

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
    let mut doc = Document::from_path("synthetic.png");
    let def = pe_effects::by_key(effect).expect("effect exists");
    let mut row = StackRow::new(RowId(0), effect);
    row.params = def.default_params();
    for (k, v) in params {
        row.params.set(*k, v.clone());
    }
    doc.stack.push(row);
    doc
}

/// A white square centred on black. The sharp edge makes a halo that can be
/// measured at a known distance, and the flat field makes a vignette obvious.
fn square_on_black(size: u32, half: u32) -> DecodedImage {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    let c = size / 2;
    for y in 0..size {
        for x in 0..size {
            let inside = x.abs_diff(c) < half && y.abs_diff(c) < half;
            let v = if inside { 255u8 } else { 0u8 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    DecodedImage::new(size, size, pixels).expect("synthetic image")
}

/// A flat mid-grey field, so a vignette's shape is the only thing visible.
fn flat_grey(width: u32, height: u32) -> DecodedImage {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..width * height {
        pixels.extend_from_slice(&[160, 160, 160, 255]);
    }
    DecodedImage::new(width, height, pixels).expect("flat field")
}

// ---------------------------------------------------------------------------
// Vignette
// ---------------------------------------------------------------------------

/// Border Shape moves between an ellipse and a rectangle.
///
/// The discriminator is the **corner**, not the edge midpoint. At an edge
/// midpoint one coordinate dominates and the two shapes give almost the same
/// radius, so comparing there would pass whatever the exponent is. Along the
/// diagonal an ellipse reaches sqrt(2) further than a box does, so a
/// rectangular border leaves the corner markedly lighter.
///
/// The softness range also has to be wide enough that both corners are not
/// simply saturated to black, or the difference is clipped away before it can
/// be measured.
#[test]
fn border_shape_moves_between_an_ellipse_and_a_rectangle() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat_grey(200, 200);
    let sample = |shape: f32| -> (u8, u8) {
        let out = render(
            gpu,
            &src,
            &look(
                "vignette",
                &[
                    // Resolve hides these behind Operating Mode.
                    ("operating_mode", ParamValue::Choice("Advanced".into())),
                    // Size and softness chosen so neither corner saturates to
                    // black — a clipped corner hides the very difference this
                    // test exists to measure.
                    ("size", ParamValue::Float(0.3)),
                    ("softness", ParamValue::Float(0.6)),
                    ("border_shape", ParamValue::Float(shape)),
                ],
            ),
        );
        // Middle of the right edge, and the bottom-right corner.
        (out.pixel(197, 100)[0], out.pixel(197, 197)[0])
    };

    let (ellipse_edge, ellipse_corner) = sample(0.0);
    let (rect_edge, rect_corner) = sample(1.0);

    assert!(
        rect_corner > ellipse_corner + 8,
        "a rectangular border should leave the corner lighter than an ellipse \
         (rect {rect_corner} vs ellipse {ellipse_corner})"
    );
    assert!(
        rect_edge.abs_diff(ellipse_edge) < 12,
        "the edge midpoint should barely differ between the two shapes \
         (rect {rect_edge} vs ellipse {ellipse_edge})"
    );
}

#[test]
fn the_centre_control_moves_the_vignette() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat_grey(200, 200);
    let out = render(
        gpu,
        &src,
        &look(
            "vignette",
            &[
                // Resolve hides these behind Operating Mode.
                ("operating_mode", ParamValue::Choice("Advanced".into())),
                ("size", ParamValue::Float(0.6)),
                ("center_x", ParamValue::Float(0.2)),
            ],
        ),
    );

    // With the centre pushed left, the right side is further from it and so
    // must be darker than the left.
    let left = out.pixel(6, 100)[0];
    let right = out.pixel(193, 100)[0];
    assert!(
        right < left,
        "moving the centre left should darken the right ({left} vs {right})"
    );
}

#[test]
fn anamorphism_stretches_the_shape_horizontally() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat_grey(200, 200);
    let sample = |anamorphism: f32| -> u8 {
        let out = render(
            gpu,
            &src,
            &look(
                "vignette",
                &[
                    ("size", ParamValue::Float(0.55)),
                    ("anamorphism", ParamValue::Float(anamorphism)),
                ],
            ),
        );
        out.pixel(193, 100)[0]
    };
    // Widening the shape pulls the darkening away from the horizontal edges.
    // 1.0 is the circle, as Resolve counts it, not 0.0.
    assert!(
        sample(1.8) > sample(1.0),
        "a wider shape should lighten the side of the frame"
    );
}

#[test]
fn the_colour_control_tints_rather_than_only_darkening() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat_grey(200, 200);
    let out = render(
        gpu,
        &src,
        &look(
            "vignette",
            &[
                ("size", ParamValue::Float(0.6)),
                ("color", ParamValue::Rgb([0.6, 0.0, 0.0])),
            ],
        ),
    );
    let corner = out.pixel(197, 197);
    assert!(
        corner[0] as i32 > corner[2] as i32 + 20,
        "a red vignette colour should leave a red corner, got {corner:?}"
    );
}

#[test]
fn rotation_turns_a_non_circular_vignette() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = flat_grey(200, 200);
    let sample = |rotation: f32| -> (u8, u8) {
        let out = render(
            gpu,
            &src,
            &look(
                "vignette",
                &[
                    ("size", ParamValue::Float(0.55)),
                    // A stretched shape has an orientation to rotate; a circle
                    // would look identical at every angle and prove nothing.
                    ("anamorphism", ParamValue::Float(1.9)),
                ],
            ),
        );
        let _ = rotation;
        (out.pixel(193, 100)[0], out.pixel(100, 193)[0])
    };
    let (side_0, top_0) = sample(0.0);

    let rotated = render(
        gpu,
        &src,
        &look(
            "vignette",
            &[
                // Resolve hides these behind Operating Mode.
                ("operating_mode", ParamValue::Choice("Advanced".into())),
                ("size", ParamValue::Float(0.55)),
                ("anamorphism", ParamValue::Float(1.9)),
                ("rotation", ParamValue::Float(90.0)),
            ],
        ),
    );
    let side_90 = rotated.pixel(193, 100)[0];
    let top_90 = rotated.pixel(100, 193)[0];

    // Rotating a stretched vignette by 90 degrees swaps which axis is spared.
    assert!(
        side_0 > top_0 && top_90 > side_90,
        "rotation did not turn the shape: \
         0deg (side {side_0}, top {top_0}), 90deg (side {side_90}, top {top_90})"
    );
}

#[test]
fn vignette_reference() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let out = render(
        gpu,
        &pe_io::test_chart(256, 192),
        &look(
            "vignette",
            &[
                // Resolve hides these behind Operating Mode.
                ("operating_mode", ParamValue::Choice("Advanced".into())),
                ("size", ParamValue::Float(0.55)),
                ("softness", ParamValue::Float(0.4)),
                ("border_shape", ParamValue::Float(0.35)),
                ("anamorphism", ParamValue::Float(1.2)),
            ],
        ),
    );
    pe_golden::assert_matches("vignette_shaped", &out, TOLERANCE);
}

// ---------------------------------------------------------------------------
// Halation relative spread
// ---------------------------------------------------------------------------

fn halo(gpu: &GpuContext, fine_tune: bool) -> DecodedImage {
    let src = square_on_black(200, 30);
    render(
        gpu,
        &src,
        &look(
            "halation",
            &[
                ("strength", ParamValue::Float(1.0)),
                // Spread is a 0..1 control mapped through a square, as
                // Resolve's is, rather than a frame fraction written straight
                // into the sampler. 0.94 is the 22% of the frame these tests
                // were written against.
                ("spread", ParamValue::Float(0.94)),
                ("threshold", ParamValue::Float(0.2)),
                ("normalization", ParamValue::Float(1.0)),
                ("fine_tune_spread", ParamValue::Bool(fine_tune)),
            ],
        ),
    )
}

/// With per-channel spread on, the halo is red-biased without anyone tinting
/// it — the bias comes out of red simply reaching further.
#[test]
fn relative_spread_produces_a_red_biased_halo() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let out = halo(gpu, true);

    // Just outside the square's right edge, along its centre line.
    let p = out.pixel(140, 100);
    assert!(
        p[0] as i32 > p[2] as i32 + 4,
        "halo should be red-biased with relative spread on, got {p:?}"
    );
}

/// The test that separates a real per-channel spread from a tint.
///
/// A tint multiplies every channel by a fixed ratio, so red/blue is constant
/// with distance. Genuinely different radii make red outlast blue, so the
/// ratio *grows* as you move away from the source. Only the second is physics.
#[test]
fn the_red_bias_increases_with_distance_which_a_tint_cannot_do() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let out = halo(gpu, true);

    // Sample outward from the square edge along its centre line.
    let ratio_at = |x: u32| -> f64 {
        let p = out.pixel(x, 100);
        (p[0] as f64 + 1.0) / (p[2] as f64 + 1.0)
    };
    let near = ratio_at(134);
    let far = ratio_at(152);

    assert!(
        far > near * 1.05,
        "red/blue should widen with distance if the channels really spread \
         differently: near {near:.3}, far {far:.3}"
    );
}

/// The cheap path must stay cheap and stay neutral in shape.
///
/// With Fine Tune off the three channels share one radius, so the ratio is
/// flat with distance — the opposite of the test above, and the thing that
/// makes the two paths genuinely different rather than one being a slower
/// version of the other.
#[test]
fn without_fine_tune_the_channels_share_one_radius() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = square_on_black(200, 30);
    let out = render(
        gpu,
        &src,
        &look(
            "halation",
            &[
                ("strength", ParamValue::Float(1.0)),
                ("spread", ParamValue::Float(0.22)),
                ("threshold", ParamValue::Float(0.2)),
                ("normalization", ParamValue::Float(1.0)),
                ("fine_tune_spread", ParamValue::Bool(false)),
                // Neutral hue and no saturation, so nothing tints the glow and
                // any colour difference would have to come from the geometry.
                ("saturation", ParamValue::Float(0.0)),
            ],
        ),
    );

    let ratio_at = |x: u32| -> f64 {
        let p = out.pixel(x, 100);
        (p[0] as f64 + 1.0) / (p[2] as f64 + 1.0)
    };
    let near = ratio_at(134);
    let far = ratio_at(150);
    assert!(
        (far - near).abs() < 0.06,
        "a single radius should give a flat colour ratio: near {near:.3}, far {far:.3}"
    );
}

#[test]
fn relative_spread_reference() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let out = halo(gpu, true);
    pe_golden::assert_matches("halation_relative_spread", &out, TOLERANCE);
}

// ---------------------------------------------------------------------------
// Cinematic Haze
// ---------------------------------------------------------------------------

/// The depth estimate is the whole effect: everything else is weighted by it.
///
/// A bright square on black is the clearest case the dark-channel prior has.
/// Black has a dark channel of zero — nothing in front of it — and the square
/// has a high one, which the prior can only read as air. With Invert on, as
/// Resolve ships it, the map is *nearness*, so the black reads near and the
/// square reads far.
#[test]
fn the_depth_preview_separates_the_bright_subject_from_the_black() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = square_on_black(200, 40);
    let out = render(
        gpu,
        &src,
        &look(
            "cinematic_haze",
            &[("depth_preview", ParamValue::Bool(true))],
        ),
    );

    let inside = out.pixel(100, 100);
    let outside = out.pixel(10, 10);
    // Monochrome, because a depth map is one number.
    assert_eq!(inside[0], inside[1], "the preview is not monochrome");
    assert_eq!(inside[1], inside[2], "the preview is not monochrome");
    assert!(
        (outside[0] as i32) > (inside[0] as i32) + 20,
        "the black should read near and the bright square far:          outside {}, inside {}",
        outside[0],
        inside[0]
    );
}

/// What haze does: pulls the distance towards the airlight, and leaves what
/// is close alone. Both halves matter — an effect that lifted everything
/// equally would be a fog filter, not aerial perspective.
#[test]
fn haze_pulls_the_far_subject_towards_the_airlight_and_spares_the_near() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = square_on_black(200, 40);
    let out = render(
        gpu,
        &src,
        &look(
            "cinematic_haze",
            &[
                ("density", ParamValue::Float(1.0)),
                ("airlight", ParamValue::Float(0.4)),
                // One thing at a time: the halos would light the black up on
                // their own and this test is about the scattering.
                ("halo_brightness", ParamValue::Float(0.0)),
                ("resolution_loss", ParamValue::Float(0.0)),
            ],
        ),
    );

    let inside = out.pixel(100, 100)[0] as i32;
    let outside = out.pixel(10, 10)[0] as i32;
    let was_inside = src.pixel(100, 100)[0] as i32;
    let was_outside = src.pixel(10, 10)[0] as i32;

    assert!(
        inside < was_inside - 8,
        "the far subject should be pulled down towards the airlight:          {was_inside} became {inside}"
    );
    assert!(
        (outside - was_outside).abs() <= 4,
        "the near part of the frame should be left alone:          {was_outside} became {outside}"
    );
}

/// Light Rays march along a line rather than gathering over a disc, so they
/// have to reach further in the direction they are pointed than across it.
/// A round glow would pass any test that only asked "is it brighter".
#[test]
fn light_rays_reach_along_their_angle_and_not_across_it() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = square_on_black(200, 20);
    let out = render(
        gpu,
        &src,
        &look(
            "cinematic_haze",
            &[
                ("rays_enable", ParamValue::Bool(true)),
                ("ray_angle", ParamValue::Float(0.0)),
                ("ray_length", ParamValue::Float(1.0)),
                ("ray_brightness", ParamValue::Float(1.0)),
                ("ray_soften", ParamValue::Float(0.0)),
                ("density", ParamValue::Float(0.0)),
                ("halo_brightness", ParamValue::Float(0.0)),
                ("resolution_loss", ParamValue::Float(0.0)),
            ],
        ),
    );

    // Along the angle, and the same distance across it.
    let along = out.pixel(135, 100)[0] as i32 - src.pixel(135, 100)[0] as i32;
    let across = out.pixel(100, 135)[0] as i32 - src.pixel(100, 135)[0] as i32;
    assert!(
        along > across + 4,
        "the rays should be directional: along {along}, across {across}"
    );
}
