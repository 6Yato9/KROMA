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
