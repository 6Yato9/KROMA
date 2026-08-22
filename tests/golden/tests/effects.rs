//! M1: the nine effects, on the GPU.
//!
//! Skipped with a message when no GPU is available so CI runners without one
//! stay green rather than failing misleadingly.

use pe_color::space;
use pe_core::{BlendMode, Document, ParamValue, RowId, StackRow, Wheel};
use pe_io::DecodedImage;
use pe_render::{EffectRenderer, GpuContext, ImageTexture, TransformPass};

/// Tolerance for an operation that should be a no-op.
///
/// Two levels, not one, and the reason is worth knowing. Working textures are
/// `Rgba16Float`, so a value carries about 11 bits of mantissa. A log-space
/// effect reads half, computes in f32, and writes half again, which can land
/// one ulp away from where it started. On a saturated colour that ulp is
/// amplified on the way back out of the wide working gamut, because the
/// inverse matrix cancels large terms down to a near-zero channel — and sRGB
/// encoding is at its steepest right there.
///
/// So two levels on saturated cyan is the floor for a half-precision pipeline,
/// not a bug to chase. Anything above it is.
const TOLERANCE: u8 = 2;

struct Harness {
    gpu: &'static GpuContext,
    renderer: EffectRenderer,
    to_display: TransformPass,
    out: ImageTexture,
    working: ImageTexture,
    size: (u32, u32),
}

impl Harness {
    fn new(src: &DecodedImage) -> Option<Self> {
        let gpu = pe_golden::shared_gpu()?;
        let source = ImageTexture::upload_rgba8(
            &gpu.device,
            &gpu.queue,
            src.width,
            src.height,
            &src.pixels,
            "source",
        )
        .expect("upload");

        let to_working = TransformPass::new(&gpu.device, pe_render::WORKING_FORMAT);
        let to_display = TransformPass::new(&gpu.device, pe_render::SOURCE_FORMAT);
        let working = to_working.to_working(gpu, &source, &space::SRGB);
        let out = ImageTexture::new(
            &gpu.device,
            src.width,
            src.height,
            pe_render::SOURCE_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            "out",
        );
        let renderer = EffectRenderer::new(&gpu.device);

        // `source` and `to_working` are only needed to produce `working`; the
        // graded chain reads from that.
        drop(source);
        drop(to_working);

        Some(Self {
            gpu,
            renderer,
            to_display,
            out,
            working,
            size: (src.width, src.height),
        })
    }

    fn render(&mut self, doc: &Document) -> DecodedImage {
        let graded = self.renderer.render(self.gpu, &self.working, doc, 1);
        let graded_view = graded.view.clone();

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.to_display.encode(
            self.gpu,
            &mut encoder,
            &graded_view,
            &self.out.view,
            &space::ACESCG,
            &space::SRGB,
        );
        self.gpu.queue.submit([encoder.finish()]);

        let pixels = pe_render::read_rgba8(self.gpu, &self.out).expect("readback");
        DecodedImage::new(self.size.0, self.size.1, pixels).expect("decoded")
    }

    fn passes(&self) -> usize {
        self.renderer.last_pass_count()
    }
}

/// A row that stops doing anything has to stop showing.
///
/// This is what a reset arrow *is* — put the value back to neutral — and it
/// was broken in a way that made the arrow look broken instead. The renderer
/// skipped all its bookkeeping when no row needed drawing, so the alias
/// saying "row N's output lives in stage N" survived from when the row still
/// did something, and the picture kept an effect whose number had already
/// gone back to zero.
///
/// It needs the same renderer twice, because the whole bug is in what the
/// cache remembers between two frames.
#[test]
fn a_row_that_goes_neutral_stops_affecting_the_picture() {
    let Some(mut harness) = Harness::new(&chart()) else {
        return;
    };
    let plain = harness.render(&doc_with(&[]));
    let pushed = harness.render(&doc_with(&[(
        "exposure",
        &[("ev", ParamValue::Float(2.0))],
    )]));
    assert!(
        plain.max_channel_delta(&pushed).unwrap() > 20,
        "the fixture never showed the effect at all"
    );

    // Back to neutral, through the same renderer — which is the case the
    // cache gets wrong.
    let reset = harness.render(&doc_with(&[(
        "exposure",
        &[("ev", ParamValue::Float(0.0))],
    )]));
    assert!(
        plain.max_channel_delta(&reset).unwrap() <= 2,
        "the row went neutral and the picture kept the effect: {} levels",
        plain.max_channel_delta(&reset).unwrap()
    );
}

/// The same thing by the other route: switching a row off.
///
/// One bug, two controls — the enable pill on the last row of a stack was
/// just as dead as the reset arrow, and for exactly the same reason.
#[test]
fn disabling_the_last_row_stops_it_showing() {
    let Some(mut harness) = Harness::new(&chart()) else {
        return;
    };
    let plain = harness.render(&doc_with(&[]));
    let mut doc = doc_with(&[("exposure", &[("ev", ParamValue::Float(2.0))])]);
    let pushed = harness.render(&doc);
    assert!(plain.max_channel_delta(&pushed).unwrap() > 20);

    doc.stack.rows[0].enabled = false;
    let off = harness.render(&doc);
    assert!(
        plain.max_channel_delta(&off).unwrap() <= 2,
        "the row was switched off and the picture kept it: {} levels",
        plain.max_channel_delta(&off).unwrap()
    );
}

fn doc_with(rows: &[(&str, &[(&str, ParamValue)])]) -> Document {
    let mut doc = Document::from_path("test.jpg");
    for (i, (effect, params)) in rows.iter().enumerate() {
        let mut row = StackRow::new(RowId(i as u64), *effect);
        let def = pe_effects::by_key(effect).expect("effect exists");
        row.params = def.default_params();
        for (k, v) in *params {
            row.params.set(*k, v.clone());
        }
        doc.stack.push(row);
    }
    doc
}

fn chart() -> DecodedImage {
    pe_io::test_chart(128, 96)
}

/// Building the renderer compiles all nine pipelines. If any WGSL is invalid,
/// this is where it surfaces — and it is the failure a user would otherwise hit
/// as a crash at startup.
#[test]
fn every_effect_shader_compiles() {
    let Some(h) = Harness::new(&chart()) else {
        return;
    };
    drop(h);
}

#[test]
fn an_empty_stack_is_a_passthrough() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let out = h.render(&Document::from_path("test.jpg"));
    let delta = out.max_channel_delta(&src).unwrap();
    assert!(
        delta <= TOLERANCE,
        "empty stack changed the image by {delta}"
    );
}

/// Every look effect must visibly do something the moment it is added.
///
/// The counterpart to the neutrality test, and the more important half in
/// practice: this is the failure a user actually hits. Adding Halation and
/// watching nothing happen reads as a broken application, and it is exactly
/// what happened — its Threshold defaulted to linear 1.0, which is diffuse
/// white, so an SDR photograph could never exceed it at any strength.
///
/// It also stops anyone "fixing" the exemption list by quietly zeroing a
/// default until the neutrality test goes green.
#[test]
fn every_look_effect_does_something_at_its_defaults() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    for key in pe_effects::registry::EFFECTS_WITH_VISIBLE_DEFAULTS {
        let out = h.render(&doc_with(&[(key, &[])]));
        let delta = out.max_channel_delta(&src).unwrap();
        assert!(
            delta > 3,
            "{key} is listed as having a visible default but moved the image \
             by only {delta} levels — adding it would look like nothing happened"
        );
    }
}

/// The most valuable invariant in M1.
///
/// Adding any effect at its registry defaults must leave the image alone. If
/// one does not, the defaults are wrong, and every user who adds that effect
/// sees an unexplained jump before touching a control.
#[test]
fn every_effect_at_its_defaults_is_neutral() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    for effect in pe_effects::all() {
        // A short, sourced list of look effects that Resolve ships with a
        // visible default on purpose. Matching Resolve wins over the
        // invariant for those; everything else must be invisible until
        // touched.
        if pe_effects::registry::EFFECTS_WITH_VISIBLE_DEFAULTS.contains(&effect.key) {
            continue;
        }
        let doc = doc_with(&[(effect.key, &[])]);
        let out = h.render(&doc);
        let delta = out.max_channel_delta(&src).unwrap();
        assert!(
            delta <= TOLERANCE,
            "{} at defaults shifted the image by {delta} levels",
            effect.key
        );
    }
}

#[test]
fn exposure_doubles_linear_light_per_stop() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let doc = doc_with(&[("exposure", &[("ev", ParamValue::Float(1.0))])]);
    let out = h.render(&doc);

    // Mid-grey in the ramp, one stop up. Compare in linear, since that is
    // where the doubling happens.
    let before = pe_color::TransferFn::Srgb.decode(src.pixel(64, 2)[0] as f64 / 255.0);
    let after = pe_color::TransferFn::Srgb.decode(out.pixel(64, 2)[0] as f64 / 255.0);
    assert!(
        (after / before - 2.0).abs() < 0.06,
        "one stop scaled linear light by {:.3}, expected 2.0",
        after / before
    );
}

#[test]
fn negative_exposure_halves_it() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let doc = doc_with(&[("exposure", &[("ev", ParamValue::Float(-1.0))])]);
    let out = h.render(&doc);
    let before = pe_color::TransferFn::Srgb.decode(src.pixel(64, 2)[0] as f64 / 255.0);
    let after = pe_color::TransferFn::Srgb.decode(out.pixel(64, 2)[0] as f64 / 255.0);
    assert!((after / before - 0.5).abs() < 0.03, "{}", after / before);
}

#[test]
fn a_disabled_row_does_nothing() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let mut doc = doc_with(&[("exposure", &[("ev", ParamValue::Float(2.0))])]);
    doc.stack.rows[0].enabled = false;

    let out = h.render(&doc);
    assert!(out.max_channel_delta(&src).unwrap() <= TOLERANCE);
    assert_eq!(h.passes(), 0, "a disabled row should cost no GPU work");
}

#[test]
fn zero_opacity_does_nothing() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let mut doc = doc_with(&[("exposure", &[("ev", ParamValue::Float(2.0))])]);
    doc.stack.rows[0].opacity = 0.0;
    let out = h.render(&doc);
    assert!(out.max_channel_delta(&src).unwrap() <= TOLERANCE);
}

#[test]
fn opacity_scales_the_effect() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let full = {
        let doc = doc_with(&[("exposure", &[("ev", ParamValue::Float(1.0))])]);
        h.render(&doc).pixel(64, 2)[0]
    };
    let half = {
        let mut doc = doc_with(&[("exposure", &[("ev", ParamValue::Float(1.0))])]);
        doc.stack.rows[0].opacity = 0.5;
        h.render(&doc).pixel(64, 2)[0]
    };
    let none = src.pixel(64, 2)[0];

    assert!(
        half > none && half < full,
        "50% opacity gave {half}, between {none} and {full} expected"
    );
}

#[test]
fn white_balance_shifts_colour_in_the_right_direction() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let doc = doc_with(&[(
        "white_balance",
        &[("temperature", ParamValue::Float(2800.0))],
    )]);
    let out = h.render(&doc);

    // Correcting for tungsten cools the image: blue rises relative to red.
    let before = src.pixel(64, 2);
    let after = out.pixel(64, 2);
    let ratio_before = before[2] as f32 / before[0].max(1) as f32;
    let ratio_after = after[2] as f32 / after[0].max(1) as f32;
    assert!(
        ratio_after > ratio_before,
        "blue/red went {ratio_before:.3} -> {ratio_after:.3}"
    );
}

#[test]
fn contrast_pushes_away_from_the_pivot() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let doc = doc_with(&[("contrast", &[("contrast", ParamValue::Float(0.5))])]);
    let out = h.render(&doc);

    // Row 2 is the neutral ramp. A shadow should darken and a highlight lighten.
    let shadow_before = src.pixel(16, 2)[0];
    let shadow_after = out.pixel(16, 2)[0];
    let high_before = src.pixel(112, 2)[0];
    let high_after = out.pixel(112, 2)[0];

    assert!(
        shadow_after < shadow_before,
        "shadow went {shadow_before} -> {shadow_after}"
    );
    assert!(
        high_after > high_before,
        "highlight went {high_before} -> {high_after}"
    );
}

#[test]
fn vignette_darkens_corners_and_leaves_the_centre() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let doc = doc_with(&[("vignette", &[("amount", ParamValue::Float(0.8))])]);
    let out = h.render(&doc);

    let corner_before = src.pixel(2, 2)[0];
    let corner_after = out.pixel(2, 2)[0];
    assert!(
        corner_after < corner_before,
        "corner went {corner_before} -> {corner_after}"
    );

    let centre_before = src.pixel(64, 48);
    let centre_after = out.pixel(64, 48);
    assert!(
        centre_after[0].abs_diff(centre_before[0]) <= 4,
        "centre moved {centre_before:?} -> {centre_after:?}"
    );
}

#[test]
fn grain_perturbs_pixels_without_shifting_the_average() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let doc = doc_with(&[("grain", &[("strength", ParamValue::Float(1.0))])]);
    let out = h.render(&doc);

    let differing = src
        .pixels
        .iter()
        .zip(&out.pixels)
        .filter(|(a, b)| a != b)
        .count();
    assert!(differing > src.pixels.len() / 8, "grain barely fired");

    // Grain is a fluctuation, not an exposure change: the mean must hold.
    let mean = |img: &DecodedImage| -> f64 {
        img.pixels
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| p[0] as f64)
            .sum::<f64>()
            / (img.pixels.len() / 4) as f64
    };
    let shift = (mean(&out) - mean(&src)).abs();
    assert!(shift < 3.0, "grain shifted the mean by {shift:.2} levels");
}

#[test]
fn the_primaries_offset_wheel_shifts_everything() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    // Offset sits at 25 and has no master ring, both of which are Resolve's:
    // an achromatic offset is an exposure change, and there is a control for
    // that already. So the push goes into the three channels, measured from
    // where the panel says the wheel already is.
    let doc = doc_with(&[(
        "primaries",
        &[("offset", ParamValue::Wheel(Wheel::uniform(75.0)))],
    )]);
    let out = h.render(&doc);
    assert!(
        out.pixel(64, 2)[0] > src.pixel(64, 2)[0],
        "a positive offset should lift the image"
    );
}

#[test]
fn halation_only_glows_from_highlights() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let doc = doc_with(&[(
        "halation",
        &[
            ("strength", ParamValue::Float(1.0)),
            ("spread", ParamValue::Float(0.08)),
            ("threshold", ParamValue::Float(0.5)),
        ],
    )]);
    let out = h.render(&doc);

    // Deep shadow far from any highlight must be untouched; the ramp near
    // white must gain some glow.
    let shadow = out.pixel(2, 2)[0] as i32 - src.pixel(2, 2)[0] as i32;
    assert!(
        shadow.abs() <= 3,
        "halation leaked into the shadows by {shadow}"
    );

    let bright_delta: i32 = (100..120)
        .map(|x| out.pixel(x, 2)[0] as i32 - src.pixel(x, 2)[0] as i32)
        .sum();
    assert!(bright_delta > 0, "highlights gained no glow");
}

/// The M1 exit criterion, measured.
///
/// A deep stack, one slider moved: exactly one pass should run.
#[test]
fn editing_the_deepest_row_costs_one_pass() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };

    let build = |ev: f32| {
        let mut doc = doc_with(&[
            ("exposure", &[]),
            ("white_balance", &[]),
            ("contrast", &[]),
            ("curves", &[]),
            ("hsl", &[]),
            ("primaries", &[]),
            ("vignette", &[]),
            ("halation", &[]),
            ("grain", &[]),
        ]);
        doc.stack.rows[8]
            .params
            .set("strength", ParamValue::Float(ev));
        doc
    };

    // Nine rows, but six of them sit at their neutral values and are skipped
    // entirely — only the three look effects (vignette, halation, grain) ship
    // visible and therefore cost a pass. That skipping is what keeps a freshly
    // opened photo at zero passes now that every document carries nine pinned
    // panels.
    h.render(&build(0.1));
    assert_eq!(h.passes(), 3, "only the non-neutral rows should run");

    for i in 1..20 {
        h.render(&build(0.1 + i as f32 * 0.01));
        assert_eq!(h.passes(), 1, "iteration {i} re-ran more than the last row");
    }
}

#[test]
fn editing_an_early_row_reruns_the_rest() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let build = |ev: f32| {
        let mut doc = doc_with(&[("exposure", &[]), ("contrast", &[]), ("grain", &[])]);
        doc.stack.rows[0].params.set("ev", ParamValue::Float(ev));
        doc
    };
    h.render(&build(0.0));
    h.render(&build(0.5));
    // Rows 0..2 are all invalidated, but Contrast is at its neutral value and
    // costs nothing, so only Exposure and Grain actually execute.
    assert_eq!(
        h.passes(),
        2,
        "changing row 0 must re-run everything below it"
    );
}

/// The property the pinned panels depend on.
///
/// Opening a photo creates nine fixed panels. If each burned a full-screen
/// pass to reproduce its input, a brand new document would cost nine passes a
/// frame before the user touched anything, and the pass counter would stop
/// meaning "the work your edit costs".
#[test]
fn a_fresh_document_costs_no_passes_at_all() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let doc = pe_effects::new_document("photo.jpg");
    let out = h.render(&doc);

    assert_eq!(doc.stack.len(), pe_effects::PINNED_ROWS.len());
    assert_eq!(h.passes(), 0, "a new document should be free to render");
    assert!(
        out.max_channel_delta(&src).unwrap() <= TOLERANCE,
        "and should leave the photograph alone"
    );
}

#[test]
fn an_unchanged_stack_costs_nothing() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let doc = doc_with(&[("exposure", &[]), ("grain", &[])]);
    h.render(&doc);
    h.render(&doc);
    assert_eq!(h.passes(), 0, "re-rendering an unchanged stack did work");
}

#[test]
fn screen_blend_brightens_rather_than_replacing() {
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let mut doc = doc_with(&[("exposure", &[("ev", ParamValue::Float(-2.0))])]);
    doc.stack.rows[0].blend = BlendMode::Screen;
    let out = h.render(&doc);

    // Screen can only lighten. A darkening effect composited with Screen must
    // therefore never drop below the original.
    for x in (4..120).step_by(8) {
        let before = src.pixel(x, 2)[0];
        let after = out.pixel(x, 2)[0];
        assert!(
            after + 2 >= before,
            "screen darkened x={x}: {before} -> {after}"
        );
    }
}

#[test]
fn an_unknown_effect_passes_the_image_through() {
    // A document written by a newer build can name an effect this one has never
    // heard of. It must render, not crash, and not blank the image.
    let src = chart();
    let Some(mut h) = Harness::new(&src) else {
        return;
    };
    let mut doc = Document::from_path("test.jpg");
    doc.stack
        .push(StackRow::new(RowId(1), "dehaze_from_the_future"));

    let out = h.render(&doc);
    assert!(out.max_channel_delta(&src).unwrap() <= TOLERANCE);
}
