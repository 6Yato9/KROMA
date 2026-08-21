//! Crop, straighten, flip and resize, through the real pipeline.
//!
//! Geometry folds into the same map the colour transform already reads
//! through, which is what keeps the source sampled exactly once. The risk that
//! creates is a silent half-pixel offset: a map that is wrong by a fraction of
//! a texel still produces a picture that looks right, and quietly softens every
//! photograph that passes through the program. Most of what follows is aimed
//! squarely at that.

use pe_core::{AspectLock, Document, Geometry, Resize};
use pe_io::DecodedImage;
use pe_render::{EffectRenderer, GpuContext};

fn render(gpu: &GpuContext, src: &DecodedImage, doc: &Document) -> DecodedImage {
    let renderer = EffectRenderer::new(&gpu.device);
    let (w, h) = pe_render::export::output_size(doc, src.width, src.height);
    let pixels = pe_render::render_full(gpu, &renderer, src.width, src.height, &src.pixels, doc)
        .expect("export");
    DecodedImage::new(w, h, pixels).expect("decoded")
}

fn doc_with(geometry: Geometry) -> Document {
    let mut doc = Document::from_path("test.png");
    doc.geometry = geometry;
    doc
}

/// Four flat quadrants: red top-left, green top-right, blue bottom-left, white
/// bottom-right. Orientation is then readable from a single pixel, and any
/// resampling shows up as a blend along the two seams.
fn quadrants(w: u32, h: u32) -> DecodedImage {
    let mut pixels = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let left = x < w / 2;
            let top = y < h / 2;
            let c: [u8; 4] = match (top, left) {
                (true, true) => [220, 40, 40, 255],
                (true, false) => [40, 200, 60, 255],
                (false, true) => [50, 80, 230, 255],
                (false, false) => [235, 235, 235, 255],
            };
            pixels.extend_from_slice(&c);
        }
    }
    DecodedImage::new(w, h, pixels).expect("quadrants")
}

/// Which quadrant colour a pixel is, or None if it is a blend of two.
fn quadrant(img: &DecodedImage, x: u32, y: u32) -> Option<&'static str> {
    let p = img.pixel(x, y);
    let near = |c: [u8; 3]| {
        (p[0] as i32 - c[0] as i32).abs() <= 4
            && (p[1] as i32 - c[1] as i32).abs() <= 4
            && (p[2] as i32 - c[2] as i32).abs() <= 4
    };
    if near([220, 40, 40]) {
        Some("red")
    } else if near([40, 200, 60]) {
        Some("green")
    } else if near([50, 80, 230]) {
        Some("blue")
    } else if near([235, 235, 235]) {
        Some("white")
    } else {
        None
    }
}

fn corners(img: &DecodedImage) -> [Option<&'static str>; 4] {
    let (w, h) = (img.width, img.height);
    [
        quadrant(img, w / 4, h / 4),
        quadrant(img, w * 3 / 4, h / 4),
        quadrant(img, w / 4, h * 3 / 4),
        quadrant(img, w * 3 / 4, h * 3 / 4),
    ]
}

// ---------------------------------------------------------------------------
// The identity
// ---------------------------------------------------------------------------

/// The one that has to hold before any of the rest matters.
///
/// A document with no crop must read the source pixel for pixel. Half a texel
/// of drift would blur every picture that ever passes through the program, and
/// it would look like nothing at all until someone compared at 400%.
#[test]
fn a_document_with_no_crop_does_not_resample() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = quadrants(240, 160);
    let out = render(gpu, &src, &doc_with(Geometry::default()));

    assert_eq!((out.width, out.height), (240, 160));
    // Right up against the seams, where a fractional offset would show as a
    // blend of two quadrants rather than either of them.
    let mut worst = 0i32;
    for y in 0..160 {
        for x in 0..240 {
            for c in 0..3 {
                worst = worst.max((out.pixel(x, y)[c] as i32 - src.pixel(x, y)[c] as i32).abs());
            }
        }
    }
    assert!(
        worst <= 3,
        "an uncropped render drifted from the source by {worst} levels"
    );
}

// ---------------------------------------------------------------------------
// Crop
// ---------------------------------------------------------------------------

#[test]
fn a_crop_takes_the_rectangle_it_names() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = quadrants(240, 160);
    let out = render(
        gpu,
        &src,
        &doc_with(Geometry {
            // The right half.
            centre: [0.25, 0.0],
            size: [0.5, 1.0],
            ..Default::default()
        }),
    );

    assert_eq!((out.width, out.height), (120, 160));
    assert_eq!(
        corners(&out),
        [Some("green"), Some("green"), Some("white"), Some("white")],
        "the right half is green over white"
    );
}

#[test]
fn a_crop_of_one_quadrant_is_flat() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = quadrants(240, 160);
    let out = render(
        gpu,
        &src,
        &doc_with(Geometry {
            centre: [-0.25, -0.25],
            size: [0.5, 0.5],
            ..Default::default()
        }),
    );
    assert_eq!((out.width, out.height), (120, 80));
    for c in corners(&out) {
        assert_eq!(c, Some("red"), "the top-left quadrant is all red");
    }
}

// ---------------------------------------------------------------------------
// Turns and flips
// ---------------------------------------------------------------------------

/// A quarter turn has to reach the pixels, not just the reported size.
#[test]
fn a_quarter_turn_clockwise_swaps_the_shape_and_moves_the_corners() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = quadrants(240, 160);
    let out = render(
        gpu,
        &src,
        &doc_with(Geometry {
            turns: 1,
            ..Default::default()
        }),
    );

    assert_eq!(
        (out.width, out.height),
        (160, 240),
        "a landscape frame turned on its side is a portrait file"
    );
    // Clockwise: the top-left of the picture ends up top-right.
    assert_eq!(
        corners(&out),
        [Some("blue"), Some("red"), Some("white"), Some("green")],
        "the quadrants did not rotate clockwise"
    );
}

#[test]
fn three_turns_is_one_the_other_way() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = quadrants(240, 160);
    let out = render(
        gpu,
        &src,
        &doc_with(Geometry {
            turns: 3,
            ..Default::default()
        }),
    );
    assert_eq!(
        corners(&out),
        [Some("green"), Some("white"), Some("red"), Some("blue")],
        "three turns clockwise is one anticlockwise"
    );
}

#[test]
fn a_horizontal_flip_swaps_left_and_right_and_leaves_top_and_bottom() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = quadrants(240, 160);
    let out = render(
        gpu,
        &src,
        &doc_with(Geometry {
            flip_h: true,
            ..Default::default()
        }),
    );
    assert_eq!(
        corners(&out),
        [Some("green"), Some("red"), Some("white"), Some("blue")]
    );
}

#[test]
fn a_vertical_flip_swaps_top_and_bottom() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = quadrants(240, 160);
    let out = render(
        gpu,
        &src,
        &doc_with(Geometry {
            flip_v: true,
            ..Default::default()
        }),
    );
    assert_eq!(
        corners(&out),
        [Some("blue"), Some("white"), Some("red"), Some("green")]
    );
}

// ---------------------------------------------------------------------------
// Straighten
// ---------------------------------------------------------------------------

/// Straightening pivots on the middle of the picture, and a crop shrunk to fit
/// has real image behind every pixel — no black corners.
#[test]
fn a_straightened_crop_is_full_of_photograph() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = quadrants(240, 160);
    let mut g = Geometry {
        angle: 8.0,
        ..Default::default()
    };
    g.shrink_to_fit(240, 160);
    let out = render(gpu, &src, &doc_with(g));

    // Every corner is still one of the four quadrant colours. A blank corner
    // would be black, which is none of them.
    for (i, c) in corners(&out).iter().enumerate() {
        assert!(
            c.is_some(),
            "corner {i} is not image content: {:?}",
            out.pixel(out.width / 4, out.height / 4)
        );
    }
    // And the quadrants are still in the order they started in — a rotation
    // this small must not reshuffle the picture.
    assert_eq!(
        corners(&out),
        [Some("red"), Some("green"), Some("blue"), Some("white")]
    );
}

/// Without shrinking, the corners of a straightened full frame have nothing
/// behind them, and they must read as blank rather than as the edge pixel
/// smeared outwards — which would look like real photograph.
#[test]
fn a_straightened_full_frame_has_blank_corners() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = quadrants(240, 160);
    let g = Geometry {
        angle: 20.0,
        ..Default::default()
    };
    assert!(!g.fits(240, 160), "the fixture is meant to overhang");
    let out = render(gpu, &src, &doc_with(g));

    let tl = out.pixel(1, 1);
    assert!(
        tl[0] < 12 && tl[1] < 12 && tl[2] < 12,
        "the overhanging corner should be blank, got {tl:?}"
    );
    // The middle is untouched by any of this.
    assert_eq!(quadrant(&out, 240 / 4, 160 / 4), Some("red"));
}

// ---------------------------------------------------------------------------
// Resize
// ---------------------------------------------------------------------------

/// Downscaling averages pixels, and averaging pixels is only meaningful in
/// linear light.
///
/// Half black and half white averages to 0.5 in linear, which is 188 in sRGB,
/// not 128. Getting 128 would mean the reduction happened in the encoding
/// rather than in the light — the mistake that makes every downscaled image
/// come out too dark.
#[test]
fn downscaling_averages_in_linear_light() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let mut pixels = Vec::new();
    for y in 0..256u32 {
        for x in 0..256u32 {
            let on = ((x / 2) + (y / 2)) % 2 == 0;
            let v = if on { 255u8 } else { 0u8 };
            pixels.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let src = DecodedImage::new(256, 256, pixels).unwrap();

    let mut doc = Document::from_path("test.png");
    doc.resize = Resize::LongEdge { pixels: 32 };
    let out = render(gpu, &src, &doc);

    assert_eq!((out.width, out.height), (32, 32));
    for y in [8u32, 16, 24] {
        for x in [8u32, 16, 24] {
            let v = out.pixel(x, y)[0] as i32;
            assert!(
                (v - 188).abs() <= 6,
                "a black-and-white checkerboard averaged to {v}, not 188 — \
                 the reduction is not happening in linear light"
            );
        }
    }
}

#[test]
fn resizing_applies_after_the_crop() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = quadrants(240, 160);
    let mut doc = doc_with(Geometry {
        centre: [0.25, 0.0],
        size: [0.5, 1.0],
        ..Default::default()
    });
    doc.resize = Resize::LongEdge { pixels: 80 };
    let out = render(gpu, &src, &doc);

    // The crop is 120x160; 80 on the long edge takes it to 60x80.
    assert_eq!((out.width, out.height), (60, 80));
    assert_eq!(quadrant(&out, 30, 20), Some("green"));
    assert_eq!(quadrant(&out, 30, 60), Some("white"));
}

// ---------------------------------------------------------------------------
// Composition
// ---------------------------------------------------------------------------

/// Crop, turn, flip and resize all at once, because each of them rewrites the
/// same map and the order they are applied in is easy to get wrong in a way
/// that only shows when more than one is set.
#[test]
fn everything_at_once_still_lands_where_it_should() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let src = quadrants(240, 160);
    let mut doc = doc_with(Geometry {
        // The top half, turned clockwise and mirrored.
        centre: [0.0, -0.25],
        size: [1.0, 0.5],
        turns: 1,
        flip_h: true,
        aspect: AspectLock::Free,
        ..Default::default()
    });
    doc.resize = Resize::LongEdge { pixels: 120 };
    let out = render(gpu, &src, &doc);

    // The crop is 240x80; turned it is 80x240; at 120 on the long edge, 40x120.
    assert_eq!((out.width, out.height), (40, 120));
    // The top half is red on the left, green on the right. Turned clockwise
    // that becomes red over green, and the horizontal flip mirrors an axis the
    // split does not run along, so it leaves the order alone.
    assert_eq!(quadrant(&out, 20, 30), Some("red"));
    assert_eq!(quadrant(&out, 20, 90), Some("green"));
}

/// Geometry runs before the stack, so an effect sees the cropped frame — which
/// is why a vignette darkens the corners of the photograph the user is making
/// rather than the corners of the sensor.
#[test]
fn effects_see_the_cropped_frame() {
    let Some(gpu) = pe_golden::shared_gpu() else {
        return;
    };
    let mut pixels = Vec::new();
    for _ in 0..200u32 {
        for _ in 0..200u32 {
            pixels.extend_from_slice(&[180, 180, 180, 255]);
        }
    }
    let src = DecodedImage::new(200, 200, pixels).unwrap();

    // A vignette on the top-left quarter of the frame.
    let mut doc = doc_with(Geometry {
        centre: [-0.25, -0.25],
        size: [0.5, 0.5],
        ..Default::default()
    });
    let def = pe_effects::by_key("vignette").expect("registered");
    let mut row = pe_core::StackRow::new(pe_core::RowId(0), "vignette");
    row.params = def.default_params();
    doc.stack.push(row);

    let out = render(gpu, &src, &doc);
    assert_eq!((out.width, out.height), (100, 100));

    // Darkest at the cropped frame's own corners, lightest at its own centre.
    // If the vignette were anchored to the sensor instead, the crop would be a
    // corner of it and the gradient would run diagonally across the result.
    let centre = out.pixel(50, 50)[0] as i32;
    let tl = out.pixel(3, 3)[0] as i32;
    let br = out.pixel(96, 96)[0] as i32;
    assert!(centre > tl + 4, "the crop's centre should be the brightest");
    assert!(
        (tl - br).abs() <= 4,
        "opposite corners of the crop should darken alike ({tl} vs {br}) — \
         the vignette is anchored to the source, not to the crop"
    );
}
