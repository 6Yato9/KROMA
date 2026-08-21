//! Export, and the claim that what you see is what you get.
//!
//! The preview and the export are separate code paths — one caches a texture
//! per row at screen resolution, the other ping-pongs two textures at full
//! resolution. They run identical shaders, but "identical shaders" is not the
//! same as "identical output", and the difference is exactly where
//! resolution-dependence hides.

use pe_core::{Document, ParamValue, RowId, StackRow};
use pe_io::DecodedImage;
use pe_render::{EffectRenderer, GpuContext};

fn gpu() -> Option<&'static GpuContext> {
    pe_golden::shared_gpu()
}

fn doc_with(rows: &[(&str, &[(&str, ParamValue)])]) -> Document {
    let mut doc = Document::from_path("test.jpg");
    for (i, (effect, params)) in rows.iter().enumerate() {
        let mut row = StackRow::new(RowId(i as u64), *effect);
        row.params = pe_effects::by_key(effect)
            .expect("effect exists")
            .default_params();
        for (k, v) in *params {
            row.params.set(*k, v.clone());
        }
        doc.stack.push(row);
    }
    doc
}

fn export(gpu: &GpuContext, src: &DecodedImage, doc: &Document) -> DecodedImage {
    let renderer = EffectRenderer::new(&gpu.device);
    let pixels = pe_render::render_full(gpu, &renderer, src.width, src.height, &src.pixels, doc)
        .expect("export");
    DecodedImage::new(src.width, src.height, pixels).expect("decoded")
}

#[test]
fn exporting_an_empty_stack_returns_the_source() {
    let Some(gpu) = gpu() else { return };
    let src = pe_io::test_chart(128, 96);
    let out = export(gpu, &src, &Document::from_path("t.jpg"));
    let delta = out.max_channel_delta(&src).unwrap();
    assert!(delta <= 1, "an empty export changed the image by {delta}");
}

#[test]
fn export_applies_the_stack() {
    let Some(gpu) = gpu() else { return };
    let src = pe_io::test_chart(128, 96);
    let doc = doc_with(&[("exposure", &[("ev", ParamValue::Float(1.0))])]);
    let out = export(gpu, &src, &doc);

    let before = pe_color::TransferFn::Srgb.decode(src.pixel(64, 2)[0] as f64 / 255.0);
    let after = pe_color::TransferFn::Srgb.decode(out.pixel(64, 2)[0] as f64 / 255.0);
    assert!((after / before - 2.0).abs() < 0.06, "{}", after / before);
}

#[test]
fn export_runs_rows_in_order() {
    // Multiply-then-lift is not the same as lift-then-multiply. If export ever
    // reorders or batches rows, this catches it.
    let Some(gpu) = gpu() else { return };
    let src = pe_io::test_chart(64, 48);

    let a = export(
        gpu,
        &src,
        &doc_with(&[
            ("exposure", &[("ev", ParamValue::Float(1.0))]),
            ("contrast", &[("contrast", ParamValue::Float(0.6))]),
        ]),
    );
    let b = export(
        gpu,
        &src,
        &doc_with(&[
            ("contrast", &[("contrast", ParamValue::Float(0.6))]),
            ("exposure", &[("ev", ParamValue::Float(1.0))]),
        ]),
    );
    assert!(
        a.max_channel_delta(&b).unwrap() > 3,
        "reordering the stack made no difference, so order is being ignored"
    );
}

#[test]
fn a_disabled_row_is_skipped_on_export_too() {
    let Some(gpu) = gpu() else { return };
    let src = pe_io::test_chart(64, 48);
    let mut doc = doc_with(&[("exposure", &[("ev", ParamValue::Float(2.0))])]);
    doc.stack.rows[0].enabled = false;
    let out = export(gpu, &src, &doc);
    assert!(out.max_channel_delta(&src).unwrap() <= 1);
}

/// Resolution independence, measured.
///
/// The same document rendered at two sizes must produce the same *look*. Grain
/// and halation are the ones that break this: expressed in pixels they shrink
/// to invisible fizz on a large export, which is the bug that bites every
/// imaging project once, always at export time, always after a hundred looks
/// have been built on the wrong assumption.
#[test]
fn spatial_effects_look_the_same_at_two_resolutions() {
    let Some(gpu) = gpu() else { return };

    let doc = doc_with(&[
        (
            "halation",
            &[
                ("strength", ParamValue::Float(1.0)),
                ("radius", ParamValue::Float(0.06)),
                ("threshold", ParamValue::Float(0.4)),
            ],
        ),
        ("vignette", &[("amount", ParamValue::Float(0.7))]),
    ]);

    let small = export(gpu, &pe_io::test_chart(160, 120), &doc);
    let large = export(gpu, &pe_io::test_chart(640, 480), &doc);

    // Compare at matching relative positions. Exact equality is not the claim —
    // resampling differs — but the effect must land in the same place with a
    // similar magnitude.
    let mut worst = 0i32;
    for (fx, fy) in [
        (0.02, 0.02),
        (0.5, 0.5),
        (0.9, 0.1),
        (0.5, 0.03),
        (0.98, 0.98),
    ] {
        let s = small.pixel((fx * 159.0) as u32, (fy * 119.0) as u32);
        let l = large.pixel((fx * 639.0) as u32, (fy * 479.0) as u32);
        let d = (s[0] as i32 - l[0] as i32).abs();
        worst = worst.max(d);
    }
    assert!(
        worst < 20,
        "the same look differed by {worst} levels between 160px and 640px; \
         a spatial effect is expressed in pixels rather than frame-relative units"
    );
}

#[test]
fn grain_survives_a_large_export() {
    // The specific failure: grain sized in pixels becomes invisible when the
    // export is five times the preview.
    let Some(gpu) = gpu() else { return };
    let doc = doc_with(&[("grain", &[("strength", ParamValue::Float(1.0))])]);

    let variance = |w: u32, h: u32| -> f64 {
        let src = pe_io::test_chart(w, h);
        let out = export(gpu, &src, &doc);
        // Mean absolute deviation from the source, over the neutral ramp band.
        let n = (w * h / 4) as f64;
        (0..h / 4)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| (out.pixel(x, y)[0] as f64 - src.pixel(x, y)[0] as f64).abs())
            .sum::<f64>()
            / n
    };

    let small = variance(160, 120);
    let large = variance(640, 480);
    assert!(small > 0.5, "grain did not fire at 160px ({small:.2})");
    assert!(
        large > small * 0.4,
        "grain faded from {small:.2} to {large:.2} on a 4x larger export"
    );
}

#[test]
fn export_memory_is_independent_of_stack_depth() {
    // Not a GPU test: the arithmetic that justifies ping-ponging instead of
    // caching a texture per row on the export path.
    //
    // A 24MP export needs about 576 MB — two 8-byte working textures plus an
    // 8-bit source and output. That is a real cost and worth knowing before
    // someone batch-exports on a 4 GB card, but it is *flat*: a fifty-row
    // stack needs exactly the same as a one-row stack.
    let ping_pong = pe_render::export::estimated_vram(6000, 4000);
    assert!(
        ping_pong < 700 * 1024 * 1024,
        "24MP export needs {ping_pong} bytes, more than expected"
    );

    // What the preview's per-row caching would cost at the same resolution for
    // a nine-row stack. This is the number that makes the second code path
    // worth having: over 1.5 GB, versus 576 MB, and it keeps climbing with
    // every row the user adds.
    let per_row_cached = 6000u64 * 4000 * 8 * 9;
    assert!(
        per_row_cached > 1_500_000_000,
        "per-row caching at 24MP would cost {per_row_cached}"
    );
    assert!(
        per_row_cached > ping_pong * 2,
        "caching per row costs {per_row_cached}, ping-pong costs {ping_pong}"
    );
}
