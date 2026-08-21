//! **The M0 exit criterion.**
//!
//! An sRGB image travels linear → log → linear → sRGB and comes back matching
//! what went in. If this fails, nothing built on top of the pipeline can be
//! trusted, so it is the gate on the whole milestone.

use pe_color::{Pipeline, WorkingSpace, space};
use pe_golden::{assert_matches, render_reference};

const CHART: (u32, u32) = (256, 192);

/// Linear → log → linear → log, which is what a realistic stack does as it
/// alternates between light-simulating and perceptual effects.
const REALISTIC_CHAIN: &[WorkingSpace] = &[
    WorkingSpace::Linear,
    WorkingSpace::Log,
    WorkingSpace::Linear,
    WorkingSpace::Log,
    WorkingSpace::Linear,
];

#[test]
fn srgb_survives_a_full_pipeline_round_trip() {
    let src = pe_io::test_chart(CHART.0, CHART.1);
    let out = render_reference(&src, &Pipeline::default(), REALISTIC_CHAIN);

    let delta = out
        .max_channel_delta(&src)
        .expect("round trip must preserve dimensions");

    // Zero, not "close to zero". The only lossy step is the final quantisation
    // back to 8 bits, and that is exactly invertible for a value that started
    // as an 8-bit integer.
    assert_eq!(
        delta, 0,
        "a no-op pipeline altered the image by {delta} levels"
    );
}

#[test]
fn the_round_trip_is_lossless_for_every_output_space() {
    // Going out to another space and back is exactly lossless in the float
    // domain, which is what the real pipeline does. No tolerance for gamut
    // width here: the maths is invertible.
    let src = pe_io::test_chart(128, 96);
    for out_space in [
        space::SRGB,
        space::DISPLAY_P3,
        space::REC2020,
        space::ACESCG,
    ] {
        let delta = pe_golden::round_trip_delta_f64(&src, out_space, REALISTIC_CHAIN);
        assert!(
            delta < 1e-9,
            "sRGB -> {} -> sRGB drifted by {delta} in float",
            out_space.name
        );
    }
}

/// Why the pipeline uses 16-bit float intermediates, demonstrated.
///
/// The same round trip *with an 8-bit intermediate* is measurably lossy, and
/// gets worse the wider the intermediate gamut is — an sRGB-gamut colour uses
/// a smaller slice of a wide gamut's numeric range, so quantising there and
/// converting back amplifies the error. This is not a bug to fix; it is the
/// reason `WORKING_FORMAT` is `Rgba16Float` and why no intermediate is ever
/// stored as 8-bit.
#[test]
fn an_8bit_intermediate_loses_precision_in_proportion_to_gamut_width() {
    let src = pe_io::test_chart(128, 96);

    let quantised_drift = |via: space::ColorSpace| -> u8 {
        let mid = render_reference(&src, &Pipeline::new(space::SRGB, via), REALISTIC_CHAIN);
        let out = render_reference(&mid, &Pipeline::new(via, space::SRGB), REALISTIC_CHAIN);
        out.max_channel_delta(&src).unwrap()
    };

    let srgb = quantised_drift(space::SRGB);
    let p3 = quantised_drift(space::DISPLAY_P3);
    let rec2020 = quantised_drift(space::REC2020);

    assert_eq!(srgb, 0, "a same-gamut 8-bit round trip should be exact");
    assert!(p3 > srgb, "P3 is wider than sRGB, so it should cost more");
    assert!(
        rec2020 > p3,
        "Rec.2020 is wider than P3 ({rec2020} vs {p3}), so it should cost more still"
    );
    // Bounds the damage so a genuine regression is still distinguishable from
    // the expected quantisation cost.
    assert!(
        rec2020 < 40,
        "Rec.2020 8-bit drift of {rec2020} is too large"
    );
}

#[test]
fn a_neutral_ramp_stays_neutral_through_the_pipeline() {
    // The most sensitive check available: AP1 is D60 and sRGB is D65, so a
    // missing or wrong chromatic adaptation shows up here as a tint long
    // before it is visible in a photograph.
    let src = pe_io::test_chart(256, 8);
    let out = render_reference(&src, &Pipeline::default(), REALISTIC_CHAIN);

    for x in 0..out.width {
        let [r, g, b, _] = out.pixel(x, 0);
        assert_eq!(
            (r, g, b),
            (r, r, r),
            "column {x} picked up a tint: {r},{g},{b}"
        );
    }
}

#[test]
fn deep_shadows_survive_the_acescct_toe() {
    // Values below 0.0078125 linear sit in ACEScct's linear toe, which is a
    // different code path from the log segment. Levels 0-8 of an 8-bit image
    // land there, and crushing them is a classic log-pipeline bug.
    let mut pixels = Vec::new();
    for level in 0u8..=16 {
        pixels.extend_from_slice(&[level, level, level, 255]);
    }
    let src = pe_io::DecodedImage::new(17, 1, pixels).unwrap();

    let out = render_reference(&src, &Pipeline::default(), REALISTIC_CHAIN);
    for level in 0u8..=16 {
        assert_eq!(
            out.pixel(level as u32, 0)[0],
            level,
            "shadow level {level} was crushed"
        );
    }
}

/// The committed reference. Guards against the whole pipeline changing shape
/// without anyone noticing — including changes that are individually
/// well-tested but combine differently.
#[test]
fn pipeline_output_matches_the_golden_reference() {
    let src = pe_io::test_chart(CHART.0, CHART.1);
    let out = render_reference(&src, &Pipeline::default(), REALISTIC_CHAIN);
    assert_matches("pipeline_identity_srgb", &out, 0);
}

#[test]
fn acescg_working_space_reference() {
    // Renders the chart *as seen in the working space*, so a change to the
    // working gamut is caught here rather than cancelling out in a round trip.
    let src = pe_io::test_chart(CHART.0, CHART.1);
    let to_working = Pipeline::new(space::SRGB, space::ACESCG);
    let out = render_reference(&src, &to_working, &[]);
    assert_matches("chart_in_acescg", &out, 0);
}

#[test]
fn acescct_working_space_reference() {
    let src = pe_io::test_chart(CHART.0, CHART.1);
    let to_log = Pipeline::new(space::SRGB, space::ACESCCT);
    let out = render_reference(&src, &to_log, &[]);
    assert_matches("chart_in_acescct", &out, 0);
}
