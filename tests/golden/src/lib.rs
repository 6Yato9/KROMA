//! Golden-image testing.
//!
//! Every effect gets a reference render committed to the repo, and CI diffs
//! against it. This is the highest-value test infrastructure in a project this
//! shader-heavy: without it, a one-line change to a shader six months from now
//! silently alters the output of every look already saved, and you find out by
//! accident. With it, you find out in thirty seconds.
//!
//! # Regenerating
//!
//! ```text
//! PE_UPDATE_GOLDEN=1 cargo test -p pe-golden
//! ```
//!
//! Review the resulting diff *visually* before committing it. A golden test
//! blindly regenerated is a golden test deleted.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use pe_io::DecodedImage;
use pe_render::GpuContext;

/// One GPU device shared by every test in the process.
///
/// Each test acquiring its own device meant thirty-odd wgpu devices per run.
/// That is wasteful, and on this hardware it is also flaky: teardown
/// occasionally hangs, the test binary stays alive, and the *next* build then
/// fails to link because the executable is locked. Sharing one device removed
/// both problems and made the suite several times faster.
///
/// Returns `None` when there is no GPU, so tests skip rather than fail.
pub fn shared_gpu() -> Option<&'static GpuContext> {
    static GPU: OnceLock<Option<GpuContext>> = OnceLock::new();
    GPU.get_or_init(|| match GpuContext::new_blocking() {
        Ok(g) => {
            eprintln!("GPU: {}", g.describe());
            Some(g)
        }
        Err(e) => {
            eprintln!("no GPU available, skipping GPU tests: {e}");
            None
        }
    })
    .as_ref()
}

/// Directory holding the committed reference images.
pub fn refs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("refs")
}

/// Directory for failure artefacts. Git-ignored.
pub fn out_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("out")
}

fn updating() -> bool {
    std::env::var("PE_UPDATE_GOLDEN").is_ok_and(|v| v != "0")
}

/// Compare `actual` against the committed reference called `name`.
///
/// `tolerance` is the largest acceptable per-channel difference, 0..255. Use 0
/// for anything computed on the CPU; a small value is appropriate only for GPU
/// output, where different vendors' rounding legitimately differs in the last
/// bit.
///
/// Panics with a readable message on mismatch, after writing the actual image
/// and a difference map next to it so the failure can be inspected.
pub fn assert_matches(name: &str, actual: &DecodedImage, tolerance: u8) {
    let ref_path = refs_dir().join(format!("{name}.png"));

    if updating() || !ref_path.exists() {
        std::fs::create_dir_all(refs_dir()).expect("create refs dir");
        pe_io::save_png(actual, &ref_path).expect("write reference");
        if !updating() {
            panic!(
                "no reference for {name:?}; one has been written to {}.\n\
                 Inspect it, then commit it if it is correct.",
                ref_path.display()
            );
        }
        return;
    }

    let expected = pe_io::load(&ref_path)
        .unwrap_or_else(|e| panic!("could not read reference {}: {e}", ref_path.display()));

    if expected.size() != actual.size() {
        dump_failure(name, actual, None);
        panic!(
            "{name}: size changed, reference is {:?} but render is {:?}",
            expected.size(),
            actual.size()
        );
    }

    let delta = actual
        .max_channel_delta(&expected)
        .expect("sizes already checked");

    if delta > tolerance {
        let diff = difference_map(&expected, actual);
        dump_failure(name, actual, Some(&diff));
        panic!(
            "{name}: output drifted by {delta} (tolerance {tolerance}).\n\
             Wrote {}/{name}.actual.png and {name}.diff.png.\n\
             If the change is intentional, re-run with PE_UPDATE_GOLDEN=1 — \
             after looking at the diff.",
            out_dir().display()
        );
    }
}

fn dump_failure(name: &str, actual: &DecodedImage, diff: Option<&DecodedImage>) {
    if std::fs::create_dir_all(out_dir()).is_err() {
        return;
    }
    let _ = pe_io::save_png(actual, out_dir().join(format!("{name}.actual.png")));
    if let Some(d) = diff {
        let _ = pe_io::save_png(d, out_dir().join(format!("{name}.diff.png")));
    }
}

/// An amplified absolute difference, so a two-level drift is actually visible.
pub fn difference_map(a: &DecodedImage, b: &DecodedImage) -> DecodedImage {
    let pixels = a
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.pixels.as_chunks::<4>().0.iter())
        .flat_map(|(pa, pb)| {
            let amp = |i: usize| pa[i].abs_diff(pb[i]).saturating_mul(16);
            [amp(0), amp(1), amp(2), 255]
        })
        .collect();
    DecodedImage {
        width: a.width,
        height: a.height,
        pixels,
    }
}

/// Render a full image through the CPU reference pipeline.
///
/// The oracle. The GPU is expected to agree with this to within a bit or two,
/// and any disagreement beyond that is a shader bug.
pub fn render_reference(
    src: &DecodedImage,
    pipeline: &pe_color::Pipeline,
    steps: &[pe_color::WorkingSpace],
) -> DecodedImage {
    let pixels = src
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let rgb = [
                p[0] as f64 / 255.0,
                p[1] as f64 / 255.0,
                p[2] as f64 / 255.0,
            ];
            let out = pipeline.apply_chain(rgb, steps, |_, _, px| px);
            [
                encode_u8(out[0]),
                encode_u8(out[1]),
                encode_u8(out[2]),
                p[3],
            ]
        })
        .collect();
    DecodedImage {
        width: src.width,
        height: src.height,
        pixels,
    }
}

fn encode_u8(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Round-trip every pixel out to `via` and back, staying in `f64` throughout.
///
/// The float-domain counterpart to [`render_reference`]. Used to demonstrate
/// that the pipeline itself is lossless, separately from what 8-bit
/// quantisation does to it.
pub fn round_trip_delta_f64(
    src: &DecodedImage,
    via: pe_color::ColorSpace,
    steps: &[pe_color::WorkingSpace],
) -> f64 {
    let there = pe_color::Pipeline::new(pe_color::space::SRGB, via);
    let back = pe_color::Pipeline::new(via, pe_color::space::SRGB);
    src.iter_rgb()
        .map(|rgb| {
            let mid = there.apply_chain(rgb, steps, |_, _, px| px);
            let out = back.apply_chain(mid, steps, |_, _, px| px);
            (0..3)
                .map(|i| (out[i] - rgb[i]).abs())
                .fold(0.0f64, f64::max)
        })
        .fold(0.0f64, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_difference_map_of_identical_images_is_black() {
        let a = pe_io::test_chart(16, 16);
        let d = difference_map(&a, &a);
        assert!(
            d.pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3] == [0, 0, 0])
        );
    }

    #[test]
    fn the_difference_map_amplifies_small_drifts() {
        let a = pe_io::test_chart(8, 8);
        let mut b = a.clone();
        b.pixels[0] = b.pixels[0].wrapping_add(1);
        let d = difference_map(&a, &b);
        assert_eq!(d.pixels[0], 16, "a one-level drift should be visible");
    }
}
