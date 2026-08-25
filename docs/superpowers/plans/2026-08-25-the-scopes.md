# The Scopes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure the graded frame in `pe-session`, get the counts across the C ABI, and draw waveform, parade, vectorscope and histogram on macOS.

**Architecture:** `pe-scopes` already computes everything and is fully tested; nothing about the measurements changes. What is missing is a path: the Windows shell renders its own scope frame and bins it in `apps/windows/src/preview.rs`, so none of it is reachable from Swift. This plan moves the *measuring* into `pe-session` — which already has `render_offscreen`, doing exactly the render-and-read-back this needs — and adds a bulk-copy path over the ABI.

**Tech Stack:** Rust, the `pe-ffi` C ABI (five functions added), Swift 6 / SwiftUI, XCTest.

**Predecessors:** the curve editor and both Colour Warper plans, all complete. All eight parameter kinds draw.

---

## Why the scopes are not a picture

`crates/pe-scopes` says it plainly in its own header: *they are counters, not
pictures. Turning a grid of counts into something on screen — the brightness
curve, the graticule, the colours — belongs to the panel that draws them, so
that the numbers stay testable without a display.*

That is also what makes them crossable. A waveform is a `[u32]`, and a `[u32]`
goes over a C ABI as a buffer copy. Rendering to an image in Rust and shipping
pixels would be easier and would put the drawing somewhere it cannot be tested
and cannot match the platform's own look.

## What is measured

All six come from one readback of the graded frame in **display-referred**
8-bit RGBA — what a colourist is asking a scope is "what will this look like on
the output", so the signal measured has to be the one going to the output.

| | shape | drawn as |
|---|---|---|
| `Histogram` | 4 planes × 256 | the levels histogram |
| `Histogram` (log) | 4 × 256 | behind the tone curves |
| `ColourSpread` | 2 × 256 | behind the secondary curves |
| `Waveform` | 4 × (columns × 256) | waveform, and parade |
| `Vectorscope` | 1 × (256 × 256) | the vectorscope |
| `Distribution` | 3 × (128 × 128) | the warper's three plots |

A waveform at 640 columns is 655,360 counts — 2.6 MB. That is the reason for a
`generation` counter: scopes are re-measured when the edit changes, not per
frame, and Swift skips the copy when the number has not moved.

## The bug this plan fixes

`Distribution` bins chromaticity over `0..XY_SPAN` where `XY_SPAN = 0.8`
(`crates/pe-scopes/src/warper.rs:45`). The plot it is drawn on spans
`PLOT_MIN..PLOT_SPAN` — −0.03 to 0.88 — and `plot_image` reads the grid at
*plot* fractions (`apps/windows/src/warper.rs`, `Plot::Chromaticity` computes
`(plot_value(u), plot_value(v))` for its colour and then calls
`sample_grid(grid, u, v)` for the haze).

Two different ranges, read as one. Solving `x / 0.8 == (x + 0.03) / 0.91` gives
`x ≈ 0.218` — the *only* chromaticity where the cloud sits where its colours
actually are. At `x = 0.64`, a saturated red, the haze is about 6% of the plot
width away from the pin a colourist would put on it.

That matters more than the number suggests. The cloud exists so you are aiming
at *this photograph's* greens rather than greens in general; a cloud that
disagrees with the coordinates drawn over it is worse than no cloud, because it
is confidently wrong. The hue-saturation and chroma-luma grids do not have the
problem — both they and their plots use 0..1 in their own terms.

## Scope

Measuring, the ABI, and the four scope panels on macOS. **Not** the backgrounds
— the histogram behind the curve editor and the haze behind the warper's three
plots — which are the next plan and are small once the data is reachable.

**Not** moving the Windows shell onto the new session path. Its `preview.rs`
already measures, at a moment tied to its own render loop; switching it is a
separate change with its own risk and no benefit to this one. Task 2's fix is
in `pe-scopes` and reaches Windows regardless.

---

## File Structure

**Created:**

| path | responsibility |
|---|---|
| `crates/pe-session/src/scopes.rs` | measuring, and the generation that says when to re-read |
| `apps/apple/KromaKit/Scopes.swift` | the counts, copied across once per generation |
| `apps/apple/KromaKit/Controls/ScopeViews.swift` | waveform, parade, vectorscope, histogram |
| `apps/apple/KromaKitTests/ScopesTests.swift` | |

**Modified:** `crates/pe-scopes/src/warper.rs`, `crates/pe-session/src/lib.rs`,
`session.rs`, `crates/pe-ffi/src/lib.rs`, `apps/apple/KromaKit/Engine.swift`,
`SessionStore.swift`, and wherever the macOS window lays out its panels.

---

## Task 1: The session can measure what it just rendered

**Files:**
- Create: `crates/pe-session/src/scopes.rs`
- Modify: `crates/pe-session/src/lib.rs`, `crates/pe-session/src/session.rs`

- [ ] **Step 1: Write the failing tests**

Add to `crates/pe-session/src/session.rs`'s test module. These need a GPU;
check how the existing render tests guard for one — there is a
`SessionError::NoGpu` and the offscreen tests must already handle a machine
without a device. **Follow whatever that existing guard is** rather than
inventing a skip.

```rust
    #[test]
    fn measuring_bins_the_frame_that_was_graded() {
        let mut s = chart_session();
        // The test chart is a colour target: it must produce counts in more
        // than one bin, or the scope is measuring a blank.
        s.measure_scopes(160, 120).unwrap();
        let scopes = s.scopes().expect("measured");
        assert!(scopes.histogram.total > 0);
        assert_eq!(
            scopes.histogram.total,
            160 * 120,
            "every pixel should be counted exactly once"
        );
        let occupied = scopes.histogram.luma.iter().filter(|c| **c > 0).count();
        assert!(occupied > 1, "a colour chart binned into one level");
        assert_eq!(scopes.waveform.columns(), 160);
        assert_eq!(scopes.waveform.rows(), 120);
    }

    #[test]
    fn the_generation_moves_only_when_something_was_measured() {
        let mut s = chart_session();
        assert_eq!(s.scope_generation(), 0, "nothing measured yet");
        s.measure_scopes(64, 64).unwrap();
        let first = s.scope_generation();
        assert!(first > 0);
        s.measure_scopes(64, 64).unwrap();
        assert!(
            s.scope_generation() > first,
            "a second measurement should be tellable from the first"
        );
    }

    #[test]
    fn measuring_with_nothing_open_is_refused() {
        let mut s = Session::new();
        assert!(s.measure_scopes(64, 64).is_err());
        assert!(s.scopes().is_none());
    }

    #[test]
    fn an_edit_does_not_silently_leave_stale_scopes_behind() {
        // The counts describe a particular grade. Handing back numbers measured
        // before an edit would draw a scope of a picture that is no longer on
        // screen, which is the one thing a scope must never do.
        let mut s = chart_session();
        s.measure_scopes(64, 64).unwrap();
        assert!(s.scopes().is_some());
        let row = s.add_effect("exposure").unwrap();
        s.set_float(row, "exposure", 1.5).unwrap();
        assert!(
            s.scopes().is_none(),
            "an edit should discard the measurement it invalidated"
        );
    }
```

That last test is a design decision, not just a check: **an edit drops the
measurement.** The alternative — keeping it and letting the shell decide — puts
the "are these numbers still true" question in every caller, and the shell that
forgets to ask draws a lie. Dropping it means `scopes()` returning `None` is
the shell's cue to re-measure.

- [ ] **Step 2: Run and watch them fail**

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && cargo test -p pe-session scope 2>&1 | LC_ALL=C grep -aE "^error|^test result:"
```

Expected: `no method named 'measure_scopes'`.

- [ ] **Step 3: Write the module**

Create `crates/pe-session/src/scopes.rs`:

```rust
//! Measuring the graded frame.
//!
//! `pe-scopes` does the counting and is tested on its own. What lives here is
//! the *when*: which frame is measured, at what size, and how a shell knows
//! the numbers it is holding still describe the picture on screen.
//!
//! The measurement is dropped by any edit. The alternative — keeping it and
//! letting each shell decide whether it is still true — puts that question in
//! every caller, and the caller that forgets to ask draws a scope of a
//! photograph that is no longer there.

use pe_scopes::{ColourSpread, Histogram, Vectorscope, warper::Distribution, waveform::Waveform};

/// Everything measured from one frame.
///
/// One struct rather than six, because they are all binned in a single pass
/// over the same pixels and are all invalidated at the same moment. Splitting
/// them would mean six copies of the "has this changed" question.
#[derive(Clone, Debug)]
pub struct Scopes {
    pub histogram: Histogram,
    /// The same frame binned in the curve's own domain, for drawing behind the
    /// curve editor.
    pub log_histogram: Histogram,
    /// Where the frame's hues and saturations sit, for the secondary curves. A
    /// tone histogram behind a Hue Vs Sat curve would put every peak in the
    /// wrong place.
    pub colour: ColourSpread,
    pub waveform: Waveform,
    pub vectorscope: Vectorscope,
    /// Where the frame's colours sit on each of the Colour Warper's three
    /// plots. Without it the warper is a diagram of colour in general rather
    /// than a tool aimed at the photograph in front of you.
    pub warper: Distribution,
}

impl Scopes {
    /// Bin one frame of display-referred 8-bit RGBA.
    pub fn measure(pixels: &[u8], width: usize, height: usize) -> Self {
        Self {
            histogram: Histogram::from_display(pixels),
            log_histogram: Histogram::from_display_log(pixels),
            colour: ColourSpread::from_display(pixels),
            waveform: Waveform::from_display(pixels, width, height),
            vectorscope: Vectorscope::from_display(pixels),
            warper: Distribution::from_display(pixels),
        }
    }
}
```

Add `pub mod scopes;` to `crates/pe-session/src/lib.rs` and re-export `Scopes`
if that is the file's habit — check how `export` and `surface` are exposed and
match it.

In `session.rs`, hold the measurement and its generation beside the document,
and add:

```rust
    /// Render the current grade at `width` by `height` and bin it.
    ///
    /// A separate, smaller render than the preview: 640 by 480 is three hundred
    /// thousand pixels, a 1.2 MB readback and a couple of milliseconds to bin,
    /// and the counts do not get better from more of them. The preview's own
    /// size is driven by the window and would make this cost whatever the user
    /// last dragged their corner to.
    pub fn measure_scopes(&mut self, width: u32, height: u32) -> Result<(), SessionError> {
        let pixels = self.render_offscreen(width, height)?;
        self.scopes = Some(Scopes::measure(&pixels, width as usize, height as usize));
        self.scope_generation += 1;
        Ok(())
    }

    /// The last measurement, if one has been taken since the last edit.
    ///
    /// `None` means "measure before you draw" rather than "there are no
    /// scopes" — see the module comment on why an edit throws them away.
    pub fn scopes(&self) -> Option<&Scopes> {
        self.scopes.as_ref()
    }

    /// Which measurement this is. Zero before the first, and strictly
    /// increasing after — a shell holding a copy compares this to know whether
    /// to copy again, which for a 2.6 MB waveform is worth doing.
    pub fn scope_generation(&self) -> u64 {
        self.scope_generation
    }
```

Then find the one place every mutation funnels through — `set_param` and its
neighbours all call into the history — and clear `self.scopes` there. **Do not
sprinkle `self.scopes = None` through the setters**; find the choke point, and
if there genuinely is not one, say so in your report rather than adding twenty
lines that will be forgotten by the twenty-first setter.

- [ ] **Step 4: Run the tests, then everything**

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | LC_ALL=C grep -aE "^error|^warning"; cargo test --workspace --no-fail-fast 2>&1 | LC_ALL=C grep -aE "^test result:|FAILED"
```

Baseline **645 passed, 0 failed**. Four new tests; report the real total.

- [ ] **Step 5: Commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add crates && git commit -m "The session measures what it renders, and forgets it when the grade moves"
```

---

## Task 2: The cloud sits where its colours are

**Files:**
- Modify: `crates/pe-scopes/src/warper.rs`
- Modify: `apps/windows/src/warper.rs`

- [ ] **Step 1: Write the failing test**

Add to the test module in `crates/pe-scopes/src/warper.rs`:

```rust
    /// The chromaticity grid is drawn on a plot spanning `PLOT_MIN` to
    /// `PLOT_SPAN`, and the drawing reads it at plot fractions. Binning it over
    /// a different range put every colour in the wrong cell — they agreed only
    /// at x = 0.218, and at a saturated red the cloud sat about six per cent of
    /// the plot away from the pin a colourist would put on it.
    ///
    /// A cloud that disagrees with the coordinates drawn over it is worse than
    /// no cloud: it is confidently wrong, and it exists precisely so you are
    /// aiming at this photograph's colours rather than colours in general.
    #[test]
    fn the_chromaticity_grid_is_binned_over_the_plot_it_is_drawn_on() {
        use pe_core::pins::{plot_fraction, PLOT_MIN, PLOT_SPAN};

        // A pixel of a known chromaticity, and where it lands.
        // sRGB pure red has xy = (0.64, 0.33).
        let pixels = [255u8, 0, 0, 255];
        let d = Distribution::from_display(&pixels);

        let cell = |grid: &[u32]| -> Option<(usize, usize)> {
            grid.iter()
                .position(|c| *c > 0)
                .map(|i| (i % GRID, i / GRID))
        };
        let (col, row) = cell(&d.chromaticity).expect("red was not counted at all");

        let want_col = (plot_fraction(0.64) * GRID as f32) as usize;
        assert!(
            col.abs_diff(want_col) <= 1,
            "red binned at column {col}, but the plot draws x = 0.64 at {want_col}"
        );

        // And the two ends of the plot are reachable, which they are not if the
        // grid covers a narrower range than the plot.
        let _ = (PLOT_MIN, PLOT_SPAN, row);
    }
```

Work out the `row` assertion yourself from how `bump` stores v — the grid may
store it downwards while the plot reads upwards, and the existing
`sample_grid` comment says it does. **Assert on it too**; getting x right and y
upside down is exactly the kind of half-fix this test exists to prevent.

- [ ] **Step 2: Run and watch it fail**

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && cargo test -p pe-scopes chromaticity 2>&1 | LC_ALL=C grep -aE "^error|panicked|^test result:"
```

Expected: it fails, naming a column about eight cells off. If `pe-scopes` does
not yet depend on `pe-core`, add it — `pe-core` is the crate everything else
already depends on, and the plot range has lived there since the pins work.

- [ ] **Step 3: Fix the binning**

Replace `x / XY_SPAN as f64` and its y counterpart with
`pe_core::pins::plot_fraction`, so the grid is binned in exactly the
coordinates it is read in. Delete `XY_SPAN` and check for other users first:

```bash
cd "/Volumes/Projects/Programming/photo editor" && LC_ALL=C grep -rna "XY_SPAN" crates apps
```

Update the doc comment: the range is no longer "0.8 fits the visible region
without spending half the grid on impossible colours" but "the plot's own
range, because the drawing reads this grid at plot fractions and two ranges
read as one put every colour in the wrong cell".

- [ ] **Step 4: Check the Windows drawing still lines up**

`apps/windows/src/warper.rs` has an ignored test that writes the plots to PNGs
— `write_the_plots_out`, `#[ignore]`. Run it and **look at the chromaticity
plot**: the cloud should now sit inside the locus rather than shifted toward
the top-right corner.

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && cargo test -p pe-windows write_the_plots_out -- --ignored --nocapture 2>&1 | tail -5
```

Report where it wrote them. If you cannot view an image, say so plainly rather
than claiming it looks right — the test above is the assertion that matters and
the PNG is a courtesy.

- [ ] **Step 5: Verify and commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | LC_ALL=C grep -aE "^error|^warning"; cargo test --workspace --no-fail-fast 2>&1 | LC_ALL=C grep -aE "^test result:|FAILED"
```

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add crates apps && git commit -m "The cloud is binned on the plot it is drawn on"
```

---

## Task 3: The counts cross the boundary

Bulk data, so neither of the ABI's usual shapes fits: a waveform is 2.6 MB and
will not be JSON, and it is not a scalar either. It crosses as a buffer copy
into memory the caller owns, which keeps rule 2 — every allocation has a
`pe_*_free` — trivially satisfied by never allocating.

Every scope is described the same way: **`planes × height × width` `u32`,
row-major**. That one layout covers all six, and the waveform's own
`bins[channel][column * LEVELS + level]` is exactly it with `height = columns`
and `width = LEVELS`.

**Files:**
- Modify: `crates/pe-ffi/src/lib.rs`

- [ ] **Step 1: Write the kind enum and the five functions**

```rust
/// Which measurement to read. The numbering is part of the ABI: add to the end.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PeScope {
    Histogram = 0,
    /// The same frame binned in the curve's own domain.
    LogHistogram = 1,
    /// Hue and saturation spread, for behind the secondary curves.
    ColourSpread = 2,
    Waveform = 3,
    Vectorscope = 4,
    WarperChromaticity = 5,
    WarperHueSat = 6,
    WarperChromaLuma = 7,
}
```

```rust
/// Render the current grade at `width` by `height` and bin it.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_measure(
    s: *mut PeSession,
    width: u32,
    height: u32,
) -> i32 {
    status(s, move |s| s.measure_scopes(width, height))
}

/// Which measurement the session is holding: 0 before the first, and strictly
/// increasing after. An edit throws the measurement away and the number stops
/// advancing until the next `pe_session_measure`, so a caller that compares
/// this before copying 2.6 MB of waveform will not copy the same numbers twice.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_scope_generation(s: *mut PeSession) -> u64 {
    with(s, 0, |s| s.inner.scope_generation())
}

/// How big a scope is, and what to divide its counts by.
///
/// Every scope is `planes * height * width` `u32`, row-major. `peak` is the
/// largest count in that data. Any out-pointer may be null.
///
/// `total` is the number of pixels measured — except for a **waveform**, where
/// it is how many image rows fed each column. That is the natural full scale
/// for a waveform cell, and unlike the peak it does not move as the picture is
/// graded, so the display does not flicker under the user's hand. The Windows
/// shell normalises against exactly this; see its `intensity`.
///
/// Returns 0, or -1 with nothing measured.
///
/// # Safety
/// `s` must be valid or null; each non-null out-pointer must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_scope_shape(
    s: *mut PeSession,
    kind: PeScope,
    planes: *mut u32,
    width: *mut u32,
    height: *mut u32,
    total: *mut u32,
    peak: *mut u32,
) -> i32 {
    /* ... */
}

/// Copy a scope's counts into `out`, returning how many were written, or a
/// negative number: -1 with nothing measured, -2 if `capacity` is short of
/// what `pe_session_scope_shape` reported.
///
/// Short rather than truncating, because a half-copied waveform draws a
/// plausible picture of a frame that does not exist.
///
/// # Safety
/// `s` must be valid or null. `out` must point to at least `capacity`
/// writable `u32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_scope_data(
    s: *mut PeSession,
    kind: PeScope,
    out: *mut u32,
    capacity: u32,
) -> i32 {
    /* ... */
}

/// The fraction of pixels above diffuse white, which is what a clipping
/// warning is actually about. Negative with nothing measured.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_over_white_fraction(s: *mut PeSession) -> f32 {
    with(s, -1.0, |s| {
        s.inner
            .scopes()
            .map_or(-1.0, |sc| sc.histogram.over_white_fraction() as f32)
    })
}
```

Write the two elided bodies. The plane order is the one the drawing needs and
must be documented in the enum's comment: **red, green, blue, luma** for a
histogram and a waveform; **hue, saturation** for a colour spread; one plane
for the rest.

`peak` deserves a look rather than a guess. `Histogram::peak()` and
`Vectorscope::peak()` already exist — **read them.** If either excludes
something (a histogram that ignored bin 0 would be doing so because a black-
point spike otherwise flattens everything else to nothing), then use the type's
own method and say so in the doc comment; if they are a plain maximum, compute
it uniformly. Report which you found.

- [ ] **Step 2: Write the tests**

Add to `mod tests` in `crates/pe-ffi/src/lib.rs`:

```rust
    #[test]
    fn a_scope_crosses_as_a_buffer_the_caller_owns() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        assert_eq!(unsafe { pe_session_scope_generation(s) }, 0);
        assert_eq!(unsafe { pe_session_measure(s, 64, 64) }, 0);
        assert!(unsafe { pe_session_scope_generation(s) } > 0);

        let (mut planes, mut w, mut h, mut total, mut peak) = (0u32, 0u32, 0u32, 0u32, 0u32);
        assert_eq!(
            unsafe {
                pe_session_scope_shape(
                    s, PeScope::Histogram, &mut planes, &mut w, &mut h,
                    &mut total, &mut peak,
                )
            },
            0
        );
        assert_eq!((planes, w, h), (4, 256, 1));
        assert_eq!(total, 64 * 64, "every pixel counted exactly once");
        assert!(peak > 0);

        let n = (planes * w * h) as usize;
        let mut out = vec![0u32; n];
        assert_eq!(
            unsafe { pe_session_scope_data(s, PeScope::Histogram, out.as_mut_ptr(), n as u32) },
            n as i32
        );
        assert_eq!(out.iter().take(256).sum::<u32>(), 64 * 64, "the red plane");
        assert_eq!(out.iter().max().copied(), Some(peak));

        // Short is refused rather than truncated: a half-copied scope draws a
        // plausible picture of a frame that does not exist.
        assert_eq!(
            unsafe { pe_session_scope_data(s, PeScope::Histogram, out.as_mut_ptr(), 10) },
            -2
        );
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn every_scope_reports_a_shape_that_matches_what_it_copies() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        assert_eq!(unsafe { pe_session_measure(s, 64, 48) }, 0);
        for kind in [
            PeScope::Histogram, PeScope::LogHistogram, PeScope::ColourSpread,
            PeScope::Waveform, PeScope::Vectorscope, PeScope::WarperChromaticity,
            PeScope::WarperHueSat, PeScope::WarperChromaLuma,
        ] {
            let (mut planes, mut w, mut h) = (0u32, 0u32, 0u32);
            assert_eq!(
                unsafe {
                    pe_session_scope_shape(
                        s, kind, &mut planes, &mut w, &mut h,
                        std::ptr::null_mut(), std::ptr::null_mut(),
                    )
                },
                0
            );
            let n = (planes * w * h) as usize;
            assert!(n > 0);
            let mut out = vec![0u32; n];
            assert_eq!(
                unsafe { pe_session_scope_data(s, kind, out.as_mut_ptr(), n as u32) },
                n as i32,
                "{:?} copied a different number than it reported", kind as i32
            );
        }
        // The waveform's columns follow the measured width, not the image.
        let (mut planes, mut w, mut h) = (0u32, 0u32, 0u32);
        unsafe {
            pe_session_scope_shape(
                s, PeScope::Waveform, &mut planes, &mut w, &mut h,
                std::ptr::null_mut(), std::ptr::null_mut(),
            )
        };
        assert_eq!((planes, w, h), (4, 256, 64), "planes, levels, columns");
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn reading_a_scope_before_measuring_is_refused() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        let mut out = [0u32; 8];
        assert_eq!(
            unsafe { pe_session_scope_data(s, PeScope::Histogram, out.as_mut_ptr(), 8) },
            -1
        );
        assert!(unsafe { pe_session_over_white_fraction(s) } < 0.0);
        unsafe { pe_session_free(s) };
    }
```

`PeScope` needs `Debug` for that message, or use `kind as i32` as written.

- [ ] **Step 3: Verify and commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && cargo fmt --all && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | LC_ALL=C grep -aE "^error|^warning"; cargo test --workspace --no-fail-fast 2>&1 | LC_ALL=C grep -aE "^test result:|FAILED"
```

Confirm the generated header picked the enum up:

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && cbindgen --config cbindgen.toml --crate pe-ffi -o /dev/stdout 2>/dev/null | LC_ALL=C grep -a -A12 "PeScope"
```

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add crates && git commit -m "Six measurements cross as counts, not as pictures"
```

---

## Task 4: Swift holds the counts

**Files:**
- Create: `apps/apple/KromaKit/Scopes.swift`
- Modify: `apps/apple/KromaKit/Engine.swift`, `SessionStore.swift`
- Create: `apps/apple/KromaKitTests/ScopesTests.swift`

- [ ] **Step 1: Write the failing tests**

```swift
import XCTest

final class ScopesTests: XCTestCase {
    private func measured() throws -> (Session, Scopes) {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        try session.measureScopes(width: 64, height: 48)
        return (session, try XCTUnwrap(try session.scopes()))
    }

    func testAHistogramComesBackWithEveryPixelCounted() throws {
        let (_, scopes) = try measured()
        XCTAssertEqual(scopes.histogram.total, 64 * 48)
        XCTAssertEqual(scopes.histogram.red.count, 256)
        XCTAssertEqual(scopes.histogram.luma.count, 256)
        XCTAssertEqual(scopes.histogram.red.reduce(0, +), 64 * 48)
        XCTAssertGreaterThan(scopes.histogram.peak, 0)
        // A colour chart binned into one level would mean the measurement is
        // reading a blank rather than the picture.
        XCTAssertGreaterThan(scopes.histogram.luma.filter { $0 > 0 }.count, 1)
    }

    func testAWaveformIsShapedColumnsByLevels() throws {
        let (_, scopes) = try measured()
        XCTAssertEqual(scopes.waveform.columns, 64)
        XCTAssertEqual(scopes.waveform.levels, 256)
        XCTAssertEqual(scopes.waveform.plane(.luma).count, 64 * 256)
        // Each column holds exactly as many samples as the frame had rows.
        let column0 = (0..<256).map { scopes.waveform.at(.luma, column: 0, level: $0) }
        XCTAssertEqual(column0.reduce(0, +), 48)
    }

    func testAVectorscopeIsSquare() throws {
        let (_, scopes) = try measured()
        XCTAssertEqual(scopes.vectorscope.width, 256)
        XCTAssertEqual(scopes.vectorscope.height, 256)
        XCTAssertEqual(scopes.vectorscope.counts.count, 256 * 256)
    }

    func testTheGenerationSaysWhenToReadAgain() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        XCTAssertEqual(session.scopeGeneration(), 0)
        try session.measureScopes(width: 64, height: 48)
        let first = session.scopeGeneration()
        XCTAssertGreaterThan(first, 0)
        try session.measureScopes(width: 64, height: 48)
        XCTAssertGreaterThan(session.scopeGeneration(), first)
    }

    func testAnEditThrowsTheMeasurementAway() throws {
        // The counts describe a particular grade. Drawing numbers measured
        // before an edit would show a scope of a picture that is not on screen.
        let (session, _) = try measured()
        let row = try session.addEffect("exposure")
        try session.setFloat(row: row, key: "exposure", value: 1.5)
        XCTAssertNil(try session.scopes(), "stale counts survived an edit")
    }

    func testReadingBeforeMeasuringGivesNothingRatherThanZeroes() throws {
        let session = try XCTUnwrap(Session())
        try session.openTestChart(width: 64, height: 64)
        XCTAssertNil(try session.scopes())
    }
}
```

Adjust `addEffect` / `setFloat` spellings to whatever `Session` actually has —
read `EngineTests.swift` first.

- [ ] **Step 2: Write the implementation**

Create `apps/apple/KromaKit/Scopes.swift`. The shape:

```swift
/// One frame's measurements, copied out of the engine.
///
/// The engine counts; this holds the counts and nothing else. Turning them
/// into a picture — the brightness curve, the graticule, the colours — belongs
/// to the views, so the numbers stay testable without a display. That is the
/// same division `pe-scopes` states in its own header, and it is what lets
/// these cross a C ABI as plain buffers.
public struct Scopes: Sendable {
    public let histogram: Levels
    public let logHistogram: Levels
    public let colour: ColourSpread
    public let waveform: WaveformCounts
    public let vectorscope: Plane
    public let warper: WarperClouds
    public let generation: UInt64
}
```

with `Levels` holding four `[UInt32]` planes plus `total` and `peak`, `Plane`
holding one plus its dimensions, and `WaveformCounts` adding
`at(_:column:level:)`. Design the exact spelling as you write the tests
green — the requirement is that a view can ask for a plane and a normaliser
without arithmetic at the call site.

On `Session` (in `Engine.swift`), one private helper does every copy:

```swift
    /// Copy one scope out of the engine.
    ///
    /// Two calls: ask the shape, then fill a buffer of exactly that size. The
    /// engine refuses a short buffer rather than truncating, so a mismatch is
    /// an error here rather than a plausible picture of a frame that does not
    /// exist.
    private func scopePlane(_ kind: PeScope) throws -> (planes: Int, width: Int, height: Int,
                                                        total: UInt32, peak: UInt32,
                                                        counts: [UInt32])
```

`scopes()` returns `nil` when `pe_session_scope_generation` is 0 — that is the
engine saying "measure before you draw", not "there are no scopes".

Mirror `measureScopes` and a cached `scopes` on `SessionStore`, re-measuring
when the generation says to. **A 2.6 MB waveform must not be copied on every
SwiftUI body evaluation** — the generation exists precisely to prevent that,
and a store that copies unconditionally will make the app stutter while a
slider is dragged.

- [ ] **Step 3: Run and commit**

Baseline **97 Swift tests**. Six new; report the real number, and build the app
target too.

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add apps/apple && git commit -m "Swift holds the counts, and copies them only when they change"
```

---

## Task 5: Four scopes, drawn

**Files:**
- Create: `apps/apple/KromaKit/Controls/ScopeViews.swift`
- Modify: wherever the macOS window lays out its panels

Almost all the judgement in a scope is in the *display* — how a cell's count
becomes a brightness, what the graticule says, where the reference lines sit.
None of that is arithmetic anyone can check without looking at it, which is why
`pe-scopes` keeps it out of the measuring and why it lives here.

- [ ] **Step 1: The three rules that are not obvious**

**Draw an image, not shapes.** A waveform is 640 columns by 256 levels. As
SwiftUI geometry that is a hundred and sixty thousand rectangles a frame; as a
`CGImage` built once per measurement it is one draw. Build the image from the
counts in a `[UInt8]` and hand it to `Image(decorative:scale:)`, rebuilt only
when the generation changes — the same reason the Windows shell uploads a
texture rather than emitting quads.

**A waveform cell normalises against the number of image rows, not the observed
peak.** A cell's count is bounded by how many image rows fed its column, so
that is the natural full scale — and unlike the peak it does not move as the
picture is graded, so the display does not flicker under the user's hand. The
ABI reports it as `total` for `PeScope::Waveform`; see Task 3.

**Then a square root.** A flat sky puts a whole column in one cell and a
gradient spreads it over two hundred; on a linear scale the gradient is one
two-hundredth as bright as the sky, which is to say invisible. Every hardware
scope applies a curve here for the same reason.

Write these as a small testable function rather than inline in a view:

```swift
/// How a cell's count becomes a brightness.
public static func intensity(count: UInt32, fullScale: Int) -> Double
```

with tests: zero is zero, full scale is one, and **a count of one per cent of
full scale is brighter than one per cent** — that last is the whole point of
the curve, and a linear implementation passes the first two.

- [ ] **Step 2: The four views**

| view | from | drawn |
|---|---|---|
| Waveform | `waveform`, luma plane | one panel, columns across, levels up |
| Parade | `waveform`, R/G/B planes | three panels side by side — same counts, different reading |
| Vectorscope | `vectorscope` | the square, plus the graticule |
| Histogram | `histogram` | four channels, additive |

Levels run **up**: level 0 at the bottom. Getting that upside down draws a
plausible waveform of a photograph nobody took.

The waveform's reference lines — black, the quarters, white — and the
vectorscope's colour-bar boxes and skin line are what a scope is *read
against*. The Windows shell draws its graticule "by running the same
projection the pixels went through, so a box can never end up somewhere the
pixels cannot reach"; `pe_scopes::waveform::position` and `TARGETS` and `SKIN`
are that projection, and they are `pub`. **They have no C ABI.** Either add one
small function for the six target positions, or compute them in Swift from the
same published RGB triples and test them against a fixture — pick one, say
which, and do not eyeball six box positions.

Channel tints are additive, so where all three overlap the result is white.

- [ ] **Step 3: Put them on screen**

Add a scopes panel to the macOS window, showing several at once — the whole
point is reading one against another. Follow how the existing panels are laid
out and toggled; do not invent a new mechanism.

The store must call `measureScopes` when the panel is visible and the
measurement is stale, and **not** when it is hidden: it is a full extra render
plus a 1.2 MB readback, and paying for it behind a closed panel is the kind of
cost nobody attributes correctly later.

- [ ] **Step 4: Verify and commit**

Report the test count and build the app target. `** BUILD SUCCEEDED **`, no new
Swift warnings — two are pre-existing: cargo's `block v0.1.6` note and the
AppIntents metadata note.

SwiftUI type-check timeouts are likely; split into named `@ViewBuilder`
functions and say what you split.

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add apps/apple && git commit -m "Waveform, parade, vectorscope and histogram, read against their graticules"
```

---

## Task 6: Say what the scopes are

- [ ] **Step 1: Update the docs**

`README.md`'s M6 row and `apps/apple/README.md` both need the scopes added to
what the macOS app does. In "What is deliberately absent", the histogram behind
the curve editor and the haze behind the warper plots are **still** absent —
this plan makes them reachable, the next one draws them. Say that rather than
deleting the paragraph.

Record the measuring contract where a reader would look for it: the session
measures on demand, an edit throws the measurement away, and `scopes()`
returning nothing means "measure first" rather than "no scopes". That is the
rule that stops a shell drawing a scope of a photograph that is no longer on
screen.

- [ ] **Step 2: Record the chromaticity fix**

`docs/resolve-parameters.md`'s Colour Warper section already explains the plot
range. Add that the colour cloud is binned in those same coordinates, and why
it has to be: it was binned over a different range until this plan, and the two
agreed at exactly one chromaticity.

- [ ] **Step 3: Verify the whole tree and commit**

```bash
cd "/Volumes/Projects/Programming/photo editor" && source "$HOME/.cargo/env" && export CARGO_TARGET_DIR="/Users/abdellah/Desktop/Programming/Kroma build" && export CARGO_INCREMENTAL=0 && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | LC_ALL=C grep -aE "^error|^warning"; cargo test --workspace --no-fail-fast 2>&1 | LC_ALL=C grep -aE "^test result: FAILED|^error"; echo "rust done"
```

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add -A && git commit -m "The scopes, written down"
```

---

## Verification

| check | command | expected |
|---|---|---|
| Rust | `cargo test --workspace --no-fail-fast` | 0 failed |
| Swift | `xcodebuild test -scheme KromaKitTests` | 0 failed |
| format | `cargo fmt --all --check` | silent |
| lint | `cargo clippy --workspace --all-targets -- -D warnings` | silent |
| app | `xcodebuild build -scheme PhotoEditor` | BUILD SUCCEEDED, no warnings |

Two tests carry the weight.
`the_chromaticity_grid_is_binned_over_the_plot_it_is_drawn_on` proves a real
misplacement is gone from both shells.
`every_scope_reports_a_shape_that_matches_what_it_copies` is what stops a scope
being copied at one size and read at another — the failure mode that draws a
convincing picture of a frame that does not exist.
