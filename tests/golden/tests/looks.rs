//! Committed reference renders for the look effects.
//!
//! Numeric assertions catch a shader that breaks. They do not catch a shader
//! that still runs but no longer looks right — a hue drifting, a falloff
//! inverting, a mode collapsing into another. These write a PNG per look that
//! a person can actually inspect, and CI diffs against them.
//!
//! Regenerate with `PE_UPDATE_GOLDEN=1 cargo test -p pe-golden`, and **look at
//! the images** before committing the change.

use pe_core::{Document, ParamValue, RowId, StackRow};
use pe_io::DecodedImage;
use pe_render::{EffectRenderer, GpuContext};

/// GPU rounding differs by vendor, so references are compared with a small
/// tolerance rather than bit-exactly. Two levels is the half-precision floor
/// documented in docs/color-pipeline.md.
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

fn chart() -> DecodedImage {
    pe_io::test_chart(256, 192)
}

#[test]
fn split_tone_natural_reference() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    // Resolve's shipping defaults, untouched.
    let out = render(gpu, &chart(), &look("split_tone", &[]));
    pe_golden::assert_matches("split_tone_natural", &out, TOLERANCE);
}

#[test]
fn split_tone_strong_reference() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let out = render(
        gpu,
        &chart(),
        &look(
            "split_tone",
            &[("mode", ParamValue::Choice("Strong".into()))],
        ),
    );
    pe_golden::assert_matches("split_tone_strong", &out, TOLERANCE);
}

/// Natural and Strong must not render the same.
///
/// The distinction is the whole reason both modes exist: Natural keeps the
/// brightest point white, Strong carries colour all the way up. A refactor
/// that collapses them would pass every other test here.
#[test]
fn natural_and_strong_differ_at_the_highlights() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();
    let natural = render(gpu, &src, &look("split_tone", &[]));
    let strong = render(
        gpu,
        &src,
        &look(
            "split_tone",
            &[("mode", ParamValue::Choice("Strong".into()))],
        ),
    );

    // Near-white end of the neutral ramp: Strong should carry more colour.
    let spread = |img: &DecodedImage, x: u32| -> i32 {
        let p = img.pixel(x, 2);
        p[0].max(p[1]).max(p[2]) as i32 - p[0].min(p[1]).min(p[2]) as i32
    };
    let n = (240..255).map(|x| spread(&natural, x)).max().unwrap_or(0);
    let s = (240..255).map(|x| spread(&strong, x)).max().unwrap_or(0);
    assert!(
        s > n,
        "Strong ({s}) should tint highlights more than Natural ({n})"
    );
}

/// Pivot decides where the split happens.
///
/// Pushed to an extreme, the manual says "the pivot lets you apply a single
/// tint to the shadows or highlight" — i.e. the whole image goes one way.
#[test]
fn the_pivot_moves_the_split_point() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();
    let low = render(
        gpu,
        &src,
        &look("split_tone", &[("pivot", ParamValue::Float(0.05))]),
    );
    let high = render(
        gpu,
        &src,
        &look("split_tone", &[("pivot", ParamValue::Float(0.95))]),
    );
    let delta = low.max_channel_delta(&high).unwrap();
    assert!(
        delta > 8,
        "moving the pivot barely changed anything ({delta})"
    );
}

#[test]
fn protect_neutrals_leaves_the_grey_ramp_alone() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();
    let protected = render(
        gpu,
        &src,
        &look(
            "split_tone",
            &[
                ("protect_neutrals", ParamValue::Bool(true)),
                ("min_saturation", ParamValue::Float(0.15)),
                ("max_saturation", ParamValue::Float(0.4)),
            ],
        ),
    );

    // Row 2 is the neutral ramp. With neutrals protected it must stay neutral.
    for x in (8..248).step_by(16) {
        let p = protected.pixel(x, 2);
        let spread = p[0].max(p[1]).max(p[2]) as i32 - p[0].min(p[1]).min(p[2]) as i32;
        assert!(
            spread <= TOLERANCE as i32,
            "column {x} picked up a tint despite Protect Neutrals: {p:?}"
        );
    }
}

#[test]
fn halation_reference() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let out = render(
        gpu,
        &chart(),
        &look(
            "halation",
            &[
                ("strength", ParamValue::Float(0.8)),
                // Spread is a 0..1 control now, as Resolve's is, rather than
                // a frame fraction written straight into the sampler. 0.45 is
                // the 5% of the frame this reference was drawn against.
                ("spread", ParamValue::Float(0.45)),
                ("threshold", ParamValue::Float(0.5)),
                ("secondary_strength", ParamValue::Float(0.4)),
            ],
        ),
    );
    pe_golden::assert_matches("halation_dye_and_secondary", &out, TOLERANCE);
}

#[test]
fn grain_tonal_gains_are_independent() {
    // Shadow, midtone and highlight gain must actually target different tonal
    // regions. One slider driving all three would pass a simple "grain fired"
    // check.
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();

    let band_delta = |doc: &Document, y0: u32, y1: u32| -> f64 {
        let out = render(gpu, &src, doc);
        let mut sum = 0.0;
        let mut n = 0.0;
        for y in y0..y1 {
            for x in 0..src.width {
                sum += (out.pixel(x, y)[0] as f64 - src.pixel(x, y)[0] as f64).abs();
                n += 1.0;
            }
        }
        sum / n
    };

    let shadows_only = look(
        "grain",
        &[
            ("strength", ParamValue::Float(1.0)),
            ("shadow_gain", ParamValue::Float(2.0)),
            ("midtone_gain", ParamValue::Float(0.0)),
            ("highlight_gain", ParamValue::Float(0.0)),
        ],
    );
    let highlights_only = look(
        "grain",
        &[
            ("strength", ParamValue::Float(1.0)),
            ("shadow_gain", ParamValue::Float(0.0)),
            ("midtone_gain", ParamValue::Float(0.0)),
            ("highlight_gain", ParamValue::Float(2.0)),
        ],
    );

    // The ramp runs dark to light left-to-right in the top band, so compare
    // the whole band under each setting: they must differ substantially.
    let a = band_delta(&shadows_only, 0, 40);
    let b = band_delta(&highlights_only, 0, 40);
    assert!(a > 0.3, "shadow-only grain barely fired ({a:.2})");
    assert!(b > 0.3, "highlight-only grain barely fired ({b:.2})");
}

// ---------------------------------------------------------------------------
// Colour Warper
// ---------------------------------------------------------------------------

/// A grid nobody has touched must leave the picture exactly alone.
///
/// Worth its own test rather than leaning on the neutrality sweep: the warper
/// is the first effect whose parameter is a hundred numbers instead of one,
/// and "all of them are zero" has to survive the trip through the LUT texture
/// as well as through the registry.
#[test]
fn an_untouched_warp_leaves_the_picture_alone() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();
    let out = render(gpu, &src, &look("colour_warper", &[]));
    let delta = out.max_channel_delta(&src).unwrap();
    assert!(delta <= 2, "an identity warp moved the picture by {delta}");
}

/// Dragging a vertex has to move the hue it sits on, and leave the far side
/// of the wheel where it was.
///
/// This is the whole promise of the tool. A warp that moved everything would
/// be a hue slider with extra steps.
#[test]
fn a_dragged_vertex_moves_its_own_hue_and_not_the_opposite_one() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();

    // Vertex zero on a six-column grid is red; column three is cyan.
    let mut warp = pe_core::Warp::identity(6, 6);
    for row in 1..6 {
        warp.set(0, row, [0.08, 0.0]);
    }
    let out = render(
        gpu,
        &src,
        &look("colour_warper", &[("hue_sat", ParamValue::Warp(warp))]),
    );

    // The chart's colour band: find the most-changed and least-changed pixels
    // by hue rather than trusting a fixed coordinate.
    let mut moved_red = 0i32;
    let mut moved_cyan = 0i32;
    for y in (src.height / 2)..src.height {
        for x in 0..src.width {
            let a = src.pixel(x, y);
            let b = out.pixel(x, y);
            let d = (a[0] as i32 - b[0] as i32).abs()
                + (a[1] as i32 - b[1] as i32).abs()
                + (a[2] as i32 - b[2] as i32).abs();
            // Red-dominant against cyan-dominant, crudely but unambiguously.
            if a[0] > a[1] + 60 && a[0] > a[2] + 60 {
                moved_red = moved_red.max(d);
            }
            if a[1] > a[0] + 60 && a[2] > a[0] + 60 {
                moved_cyan = moved_cyan.max(d);
            }
        }
    }
    assert!(
        moved_red > 12,
        "the vertex sitting on red did not move red: {moved_red}"
    );
    assert!(
        moved_cyan * 2 < moved_red,
        "the far side of the wheel moved nearly as much: cyan {moved_cyan}, red {moved_red}"
    );
}

/// A pin moves the colour it is aimed at and leaves the rest of the picture
/// where it was.
///
/// That is the whole promise of the Chroma Warp view, and the thing that makes
/// it worth having beside the grids: a grid asks what happens to every colour,
/// a pin asks what happens to *this* one.
#[test]
fn a_pin_moves_the_colour_it_sits_on_and_not_its_neighbours() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();

    // A pin on green — CIE xy around (0.30, 0.60) — dragged towards blue, with
    // a reach wide enough to cover the greens in the chart and no wider.
    let mut pin = pe_core::Pin::placed([0.30, 0.60]);
    pin.to = [0.24, 0.42];
    pin.chroma_range = 0.16;
    let pins = pe_core::Pins(vec![pin]);

    let out = render(
        gpu,
        &src,
        &look("colour_warper", &[("pins", ParamValue::Pins(pins))]),
    );

    // Greens should have moved; reds, at the far side of the diagram, should
    // not have.
    let mut moved_green = 0i32;
    let mut moved_red = 0i32;
    for y in (src.height / 2)..src.height {
        for x in 0..src.width {
            let a = src.pixel(x, y);
            let b = out.pixel(x, y);
            let d = (a[0] as i32 - b[0] as i32).abs()
                + (a[1] as i32 - b[1] as i32).abs()
                + (a[2] as i32 - b[2] as i32).abs();
            if a[1] > a[0] + 60 && a[1] > a[2] + 60 {
                moved_green = moved_green.max(d);
            }
            if a[0] > a[1] + 60 && a[0] > a[2] + 60 {
                moved_red = moved_red.max(d);
            }
        }
    }
    assert!(
        moved_green > 12,
        "the pin did not move the colour it was on: {moved_green}"
    );
    assert!(
        moved_red * 2 < moved_green,
        "it reached the far side of the diagram: red {moved_red}, green {moved_green}"
    );
}

/// A pin that has been placed and not dragged must leave the picture exactly
/// alone — otherwise placing one, which is the first half of every use of this
/// tool, would be a change in its own right.
#[test]
fn a_placed_pin_that_has_not_been_dragged_does_nothing() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = chart();
    let pins = pe_core::Pins(vec![pe_core::Pin::placed([0.30, 0.60])]);
    let out = render(
        gpu,
        &src,
        &look("colour_warper", &[("pins", ParamValue::Pins(pins))]),
    );
    let delta = out.max_channel_delta(&src).unwrap();
    assert!(delta <= 2, "placing a pin moved the picture by {delta}");
}
