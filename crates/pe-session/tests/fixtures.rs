//! The fixtures the Swift tests decode.
//!
//! Committed, and checked here against what the code produces now. If a field
//! is added in Rust and not in Swift, or renamed on one side, one of the two
//! suites fails — which is the only thing standing between two halves of one
//! application and a silent divergence.
//!
//! Regenerate deliberately, having looked at the diff:
//!
//! ```text
//! PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures
//! ```

use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/apple/Fixtures")
        .canonicalize()
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/apple/Fixtures")
        })
}

fn check(name: &str, produced: String) {
    let path = fixture_dir().join(name);
    if std::env::var_os("PE_UPDATE_FIXTURES").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &produced).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "{} is missing — run with PE_UPDATE_FIXTURES=1",
            path.display()
        )
    });
    assert_eq!(
        committed.trim(),
        produced.trim(),
        "{} is out of date. Look at the change, then regenerate with \
         PE_UPDATE_FIXTURES=1 cargo test -p pe-session --test fixtures",
        path.display()
    );
}

#[test]
fn the_registry_fixture_is_current() {
    let json = serde_json::to_string_pretty(&pe_session::describe::registry()).unwrap();
    check("registry.json", json);
}

#[test]
fn the_snapshot_fixture_is_current() {
    // A session with something in it, so the fixture exercises rows,
    // parameters, undo labels and the colour settings rather than the empty
    // case. Deterministic: no clock, no GPU, no file.
    let mut s = pe_session::Session::new();
    s.open_test_chart(64, 64).unwrap();
    let id = s.add_effect("exposure").unwrap();
    s.set_float(id, "ev", 0.75).unwrap();
    let json = serde_json::to_string_pretty(&pe_session::describe::snapshot(&s)).unwrap();
    check("snapshot.json", json);
}

/// Curves, and what the engine evaluates them to.
///
/// The Swift editor draws a preview by reimplementing the evaluator, because
/// asking the engine to bake on every drag frame would put a C call and a
/// 256-float copy inside a gesture. This is what keeps the copy honest: if the
/// two ever disagree, the line the user drags is not the line that renders, and
/// it disagrees most where they are looking hardest.
#[test]
fn the_curve_sample_fixture_is_current() {
    use pe_core::Curve;

    // Chosen for the cases that separate a correct monotone interpolant from a
    // plausible one.
    let cases: Vec<(&str, Vec<[f32; 2]>)> = vec![
        ("identity", vec![[0.0, 0.0], [1.0, 1.0]]),
        ("flat", vec![[0.0, 0.5], [1.0, 0.5]]),
        // A lifted midtone: the ordinary case.
        ("lifted", vec![[0.0, 0.0], [0.5, 0.65], [1.0, 1.0]]),
        // The case Catmull-Rom gets wrong: a dragged highlight that would make
        // an overshooting spline bulge above 1.0 in the middle.
        (
            "pulled_highlight",
            vec![[0.0, 0.0], [0.7, 0.95], [0.85, 0.97], [1.0, 0.5]],
        ),
        // A local extremum, which must get a flat tangent.
        (
            "s_curve",
            vec![[0.0, 0.1], [0.35, 0.2], [0.65, 0.8], [1.0, 0.9]],
        ),
        // Points out of order: the evaluator sorts, and so must Swift.
        ("unsorted", vec![[1.0, 1.0], [0.25, 0.4], [0.0, 0.0]]),
        // Two points at the same x, which must not divide by zero.
        (
            "coincident_x",
            vec![[0.0, 0.0], [0.5, 0.3], [0.5, 0.8], [1.0, 1.0]],
        ),
        // Endpoints inside the unit square: outside the control range the
        // curve holds its endpoint rather than extrapolating.
        ("inset", vec![[0.2, 0.3], [0.8, 0.7]]),
    ];

    let mut out = serde_json::Map::new();
    for (name, points) in cases {
        let curve = Curve {
            points: points.clone(),
        };
        // Sampled at the LUT's own positions, so this is literally what the
        // shader will read.
        let samples: Vec<f32> = (0..256).map(|i| curve.sample(i as f32 / 255.0)).collect();
        out.insert(
            name.to_string(),
            serde_json::json!({ "points": points, "samples": samples }),
        );
    }

    let json = serde_json::to_string_pretty(&serde_json::Value::Object(out)).unwrap();
    check("curve_samples.json", json);
}

/// The lattice's own geometry, and what the engine makes of it.
///
/// The Swift warper reimplements this so a vertex drag costs no round trip,
/// exactly as the curve editor reimplements the curve evaluator. This is what
/// keeps the copy honest.
#[test]
fn the_warp_sample_fixture_is_current() {
    use pe_core::Warp;

    let mut out = serde_json::Map::new();

    // Where every vertex of a few grid sizes sits, on both kinds of axis.
    let mut homes = serde_json::Map::new();
    for (cols, rows) in [(4u32, 4u32), (6, 6), (6, 4), (8, 12), (16, 16), (2, 2)] {
        let w = Warp::identity(cols, rows);
        for wrap in [true, false] {
            let mut points = Vec::new();
            for r in 0..rows {
                for c in 0..cols {
                    points.push(w.home(c, r, wrap));
                }
            }
            homes.insert(
                format!("{cols}x{rows}{}", if wrap { "_wrap" } else { "_clamp" }),
                serde_json::json!({ "cols": cols, "rows": rows, "wrap": wrap, "homes": points }),
            );
        }
    }
    out.insert("homes".into(), serde_json::Value::Object(homes));

    // And what a dragged lattice samples to between its vertices — the part
    // the shader reads, and the part a plausible reimplementation gets subtly
    // wrong at the seam.
    let mut sampled = serde_json::Map::new();
    let mut w = Warp::identity(6, 4);
    w.set(0, 0, [0.12, -0.2]);
    w.set(1, 2, [-0.3, 0.15]);
    w.set(5, 3, [0.4, 0.4]);
    for wrap in [true, false] {
        let mut values = Vec::new();
        for i in 0..32 {
            for j in 0..32 {
                let u = i as f32 / 31.0;
                let v = j as f32 / 31.0;
                values.push(w.sample(u, v, wrap));
            }
        }
        sampled.insert(
            if wrap { "wrap" } else { "clamp" }.into(),
            serde_json::json!({ "grid": 32, "values": values }),
        );
    }
    out.insert(
        "sampled".into(),
        serde_json::json!({
            "cols": w.cols(),
            "rows": w.rows(),
            "offsets": w.offsets(),
            "at": sampled,
        }),
    );

    let json = serde_json::to_string_pretty(&serde_json::Value::Object(out)).unwrap();
    check("warp_samples.json", json);
}

/// The chromaticity plot's mapping, and a set of pins through it.
///
/// The Swift editor reimplements the mapping so a pin drag costs no round
/// trip, exactly as the curve and lattice editors do. This is what keeps the
/// copy honest.
#[test]
fn the_pin_sample_fixture_is_current() {
    use pe_core::pins::{PLOT_MIN, PLOT_SPAN, Pin, Pins, plot_fraction, plot_value};

    // The mapping, at points that matter: both frame edges, the white point, a
    // fresh pin's home, and the greenest corner of the locus.
    let probes: Vec<f32> = vec![
        PLOT_MIN, 0.0, 0.15, 0.3127, 0.33, 0.35, 0.5, 0.7347, 0.8338, PLOT_SPAN,
    ];
    let fractions: Vec<[f32; 2]> = probes.iter().map(|v| [*v, plot_fraction(*v)]).collect();
    // And back, so a reimplementation that is right in one direction and wrong
    // in the other is caught.
    let values: Vec<[f32; 2]> = (0..=20)
        .map(|i| {
            let t = i as f32 / 20.0;
            [t, plot_value(t)]
        })
        .collect();

    // A set of pins, including one dragged, one with exposure only, and one
    // left alone — the three states `is_neutral` has to tell apart.
    let mut pins = Pins::default();
    pins.add(Pin::placed([0.33, 0.35]));
    let mut dragged = Pin::placed([0.20, 0.65]);
    dragged.to = [0.28, 0.55];
    dragged.chroma_range = 0.12;
    pins.add(dragged);
    let mut lifted = Pin::placed([0.45, 0.40]);
    lifted.exposure = 0.75;
    lifted.tonal_low = 0.2;
    lifted.tonal_pivot = 0.6;
    pins.add(lifted);

    let packed: Vec<Vec<f32>> = pins.iter().map(|p| p.pack().to_vec()).collect();
    let neutral: Vec<bool> = pins.iter().map(Pin::is_neutral).collect();

    let json = serde_json::to_string_pretty(&serde_json::json!({
        "plot": { "min": PLOT_MIN, "span": PLOT_SPAN },
        "fractions": fractions,
        "values": values,
        "pins": pins,
        "packed": packed,
        "neutral": neutral,
        "max_pins": pe_core::pins::MAX_PINS,
    }))
    .unwrap();
    check("pin_samples.json", json);
}

/// The vectorscope's graticule, and where pixels of those same colours land.
///
/// The Swift panel draws the colour-bar boxes and the skin line by
/// reimplementing `pe_scopes::waveform::position`, because that projection is
/// `pub` Rust with no C ABI and a handful of multiplications is not worth a
/// round trip through the engine to place six boxes. This is what keeps the
/// copy honest, exactly as `curve_samples.json` does for the curve evaluator.
///
/// `cell` is not computed from `position`: it is found by putting one pixel of
/// that colour through `Vectorscope::from_display` and seeing which bin lights
/// up. That is what makes the fixture worth having rather than a restatement
/// of the formula — a box can only be checked against somewhere the pixels
/// actually reach if the fixture went there the way the pixels do.
#[test]
fn the_scope_graticule_fixture_is_current() {
    use pe_scopes::{SKIN, TARGETS, VECTOR_SIZE, Vectorscope, waveform::position};

    fn cell(rgb: [u8; 3]) -> [usize; 2] {
        let v = Vectorscope::from_display(&[rgb[0], rgb[1], rgb[2], 255]);
        let lit = v
            .bins()
            .iter()
            .position(|c| *c > 0)
            .expect("one pixel lands in one bin");
        [lit % VECTOR_SIZE, lit / VECTOR_SIZE]
    }

    let targets: Vec<serde_json::Value> = TARGETS
        .iter()
        .map(|(name, rgb)| {
            serde_json::json!({
                "name": name,
                "rgb": rgb,
                "position": position(*rgb),
                "cell": cell(*rgb),
            })
        })
        .collect();

    let json = serde_json::to_string_pretty(&serde_json::json!({
        "size": VECTOR_SIZE,
        "targets": targets,
        "skin": {
            "rgb": SKIN,
            "position": position(SKIN),
            "cell": cell(SKIN),
        },
    }))
    .unwrap();
    check("scope_graticule.json", json);
}

/// The ten curves of the `curves` effect, read out of the registry.
///
/// Read rather than typed: a list edited by hand when a curve is added is a
/// list that will not be, and the one thing worse than a curve with no
/// backdrop is a test that says every curve has one because it never heard of
/// the new curve.
fn curve_keys() -> Vec<&'static str> {
    pe_effects::by_key("curves")
        .expect("the curves effect is in the registry")
        .params
        .iter()
        .filter(|p| matches!(p.kind, pe_effects::ParamKind::Curve { .. }))
        .map(|p| p.key)
        .collect()
}

/// Every curve the registry declares must have an answer. A curve added later
/// with no backdrop would silently draw against nothing, which reads as "this
/// photograph has no colours there".
///
/// Here rather than in `pe-scopes` beside `Backdrop` itself, because the
/// question is about two crates at once — what `pe-effects` declares and what
/// `pe-scopes` answers — and this suite is the one that already sees both. It
/// also puts the list in the same place as the fixture that ships it to Swift,
/// so there is one source for both rather than two that can disagree.
#[test]
fn every_registered_curve_has_a_backdrop() {
    use pe_scopes::backdrop::Backdrop;

    let keys = curve_keys();
    assert_eq!(keys.len(), 10, "the curves effect grew or shrank: {keys:?}");
    for key in keys {
        assert_ne!(
            Backdrop::behind(key),
            Backdrop::Nothing,
            "{key} has no backdrop"
        );
    }
    assert_eq!(Backdrop::behind("not_a_curve"), Backdrop::Nothing);
}

/// What belongs behind each curve, the window a tone plot spans, and one
/// histogram traced.
///
/// The mapping is shared knowledge with one right answer per curve, and the
/// window is two numbers that already appear in three places. Both cross to
/// Swift here rather than being typed a fourth time. The trace crosses because
/// Swift smooths the same bins on its own — a seven-tap filter written twice
/// agrees until the day one copy is tidied, so the two are compared bin for
/// bin instead of trusted.
#[test]
fn the_backdrop_fixture_is_current() {
    use pe_core::parametric::{LOG_BLACK, LOG_WHITE};
    use pe_scopes::backdrop::Backdrop;

    let mapping: serde_json::Map<String, serde_json::Value> = curve_keys()
        .iter()
        .map(|k| {
            (
                k.to_string(),
                serde_json::json!(format!("{:?}", Backdrop::behind(k))),
            )
        })
        .collect();

    // An input with the features that separate a correct smoothing from a
    // plausible one. The ends are the part worth shipping: a filter that reads
    // off the end of the array either clamps, wraps or shortens its window,
    // and all three agree everywhere in the middle.
    let mut bins = [0u32; pe_scopes::BINS];
    bins[0] = 500; // hard against the low end
    bins[pe_scopes::BINS - 1] = 300; // and the high
    bins[64] = 1000; // a lone spike, with empty runs either side
    for b in bins.iter_mut().skip(150).take(30) {
        *b = 200; // a plateau, which must come back flat
    }
    let peak = *bins.iter().max().unwrap() as f32;

    let json = serde_json::to_string_pretty(&serde_json::json!({
        "window": { "black": LOG_BLACK, "white": LOG_WHITE },
        "behind": mapping,
        "bins": bins.to_vec(),
        "peak": peak,
        "traced": pe_scopes::trace(&bins, peak),
    }))
    .unwrap();
    check("backdrop_samples.json", json);
}

/// The palette and the track ramps.
///
/// Both shells draw from one set of numbers. Before this crate existed the
/// Windows greys were written at each call site and had already drifted — the
/// viewer surround, the filmstrip and the status bar were three shades of what
/// was meant to be one colour. A second copy in Swift is that mistake again,
/// so the numbers cross here and the Swift side asserts against them.
#[test]
fn the_theme_fixture_is_current() {
    use pe_theme::{CHANNEL_AXES, Ramp, Rgb8, colour, ramp_for};

    let hex = |c: Rgb8| format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b);

    let mut palette = serde_json::Map::new();
    for &(name, c) in colour::ALL {
        palette.insert(name.to_string(), serde_json::json!(hex(*c)));
    }

    // Which ramp each parameter of each registered effect gets, so a key
    // renamed on one side and not the other is caught.
    //
    // Spelled by `Ramp::tag`, not by `{:?}`. The derived `Debug` wrote a
    // saturation ramp as `Sat(Rgb8 { r: 35, g: 228, b: 235 })` — a private
    // field layout, leaking into a string two languages are held to, and one
    // that would change if anyone added a field to `Rgb8`.
    let mut ramps = serde_json::Map::new();
    for effect in pe_effects::all() {
        for p in effect.params {
            let r = ramp_for(effect.key, p.key);
            if !r.is_plain() {
                ramps.insert(
                    format!("{}.{}", effect.key, p.key),
                    serde_json::json!(r.tag()),
                );
            }
        }
    }

    // And what each ramp actually paints, sampled — a table that agrees on
    // *which* ramp and disagrees on its colours is no use.
    let mut sampled = serde_json::Map::new();
    for ramp in [
        Ramp::Temp,
        Ramp::Tint,
        Ramp::Hue,
        Ramp::Chroma,
        Ramp::Luma,
        Ramp::HueAround(28.0),
    ] {
        let steps: Vec<String> = (0..=16).map(|i| hex(ramp.at(i as f32 / 16.0))).collect();
        sampled.insert(ramp.tag(), serde_json::json!(steps));
    }

    // The three wheel sliders' axes. They are reached as a constant rather
    // than through `ramp_for`, so nothing above would notice if the Mac's copy
    // of them said something else.
    let axes: Vec<String> = CHANNEL_AXES.iter().map(Ramp::tag).collect();

    // The icon strip: each tool, in the strip's order, with the pinned effects
    // it draws in the order it draws them.
    //
    // Here rather than in a fixture of its own because this file is what holds
    // the two shells to one inspector — the palette, the ramps, the wheel axes
    // and now which panel a pinned effect appears on are the same kind of
    // shared answer, and a Swift copy of any of them is the same mistake.
    //
    // What it buys: a pinned effect belonging to no section of the Colour tab
    // is drawn nowhere at all. `pe_effects::tab` refuses that on the Rust side;
    // this is what refuses it on the Swift one, so a twelfth pinned effect
    // given no home fails both suites rather than silently vanishing from the
    // interface.
    let tabs: Vec<serde_json::Value> = pe_effects::Tab::ALL
        .iter()
        .map(|t| serde_json::json!({ "name": t.name() }))
        .collect();
    let sections: Vec<serde_json::Value> = pe_effects::Section::ALL
        .iter()
        .map(|s| {
            serde_json::json!({
                "title": s.title(),
                "starts_open": s.starts_open(),
                "effects": s.effects(),
            })
        })
        .collect();

    let json = serde_json::to_string_pretty(&serde_json::json!({
        "palette": palette,
        "ramps": ramps,
        "sampled": sampled,
        "axes": axes,
        "tabs": tabs,
        "sections": sections,
    }))
    .unwrap();
    check("theme.json", json);
}

/// Every pinned effect appears in exactly one section of the Colour tab,
/// checked against the registry rather than against the list `pe-effects` was
/// written with.
///
/// The same shape as `every_registered_curve_has_a_backdrop` above, and here
/// for the same reason: `pe_effects::tab`'s own suite already asks this, but
/// this is the suite that also writes the fixture Swift reads, so the thing
/// shipped across is the thing that was checked.
#[test]
fn the_colour_tab_has_a_home_for_every_pinned_effect() {
    use pe_effects::Section;

    for key in pe_effects::PINNED_ROWS {
        assert!(Section::of(key).is_some(), "{key} is in no section");
    }
    // And the sections between them claim every pinned effect, once each, and
    // nothing else — which is the half that catches a section left holding a
    // key after the row it named stopped being pinned.
    let claimed: Vec<&str> = Section::ALL
        .iter()
        .flat_map(|s| s.effects())
        .copied()
        .collect();
    let mut sorted = claimed.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), claimed.len(), "a section claims a repeat");
    let mut pinned = pe_effects::PINNED_ROWS.to_vec();
    pinned.sort_unstable();
    assert_eq!(
        sorted, pinned,
        "the Colour tab and the pinned rows disagree"
    );
}

/// The spectral locus: the boundary drawn, and what sits at a chromaticity.
///
/// The Swift warper draws the horseshoe and fills the plot behind it by
/// reimplementing `pe_color::locus`, exactly as the curve, lattice and pin
/// editors reimplement their own geometry — a polyline and a per-texel colour
/// are not worth a round trip through the engine, and the plot is rebuilt on
/// every slider drag. This is what keeps the copy honest.
///
/// All three questions cross, because a reimplementation can get any one of
/// them right and the others wrong in ways that look plausible on screen:
///
/// - `curve` is every drawn point, not a subsample. A Swift copy that
///   subdivided fifteen times instead of sixteen, or that rounded the line of
///   purples off with the rest of the spline, would still pass a fixture that
///   only checked the tabulated points it interpolates *between*.
/// - `inside` is the answer the plot dims by, and the probes below are picked
///   at the places two plausible implementations disagree: either side of the
///   line of purples, the greenest vertex, and the corners of the square that
///   are no colour at all.
/// - `colour` is `None` at y of zero and a colour everywhere else, including
///   outside the horseshoe — a Swift copy that drew black there would give the
///   plot a surround instead of a continuous field.
#[test]
fn the_locus_fixture_is_current() {
    use pe_color::locus::{LOCUS, SUBDIVISIONS, colour_at, curve, inside};

    // Named, so a failure says which question moved rather than which index.
    let probes: Vec<(&str, [f32; 2])> = vec![
        ("d65", [0.3127, 0.3290]),
        ("equal_energy", [0.33, 0.33]),
        ("srgb_red", [0.640, 0.330]),
        ("srgb_green", [0.300, 0.600]),
        ("srgb_blue", [0.150, 0.060]),
        // Outside sRGB, inside the horseshoe: the clip-towards-white case.
        ("wide_green", [0.10, 0.75]),
        ("wide_cyan", [0.10, 0.40]),
        // 520 nm, a hair inside its own vertex.
        ("spectral_green", [0.0794, 0.8187]),
        // Either side of the line of purples at x = 0.45, where the chord sits
        // at y = 0.133.
        ("magenta_above_purples", [0.45, 0.20]),
        ("magenta_below_purples", [0.45, 0.10]),
        // No colour, but still drawn.
        ("far_corner", [0.8, 0.8]),
        ("origin_corner", [0.05, 0.05]),
        ("under_the_locus", [0.6, 0.05]),
        ("negative_z", [0.5, 0.6]),
        // And the one place there is nothing to answer.
        ("no_luminance", [0.4, 0.0]),
    ];

    let probed: Vec<serde_json::Value> = probes
        .iter()
        .map(|(name, [x, y])| {
            serde_json::json!({
                "name": name,
                "xy": [x, y],
                "inside": inside(*x, *y),
                "colour": colour_at(*x, *y),
            })
        })
        .collect();

    let json = serde_json::to_string_pretty(&serde_json::json!({
        "locus": LOCUS.as_slice(),
        "subdivisions": SUBDIVISIONS,
        "curve": curve(),
        "probes": probed,
    }))
    .unwrap();
    check("locus.json", json);
}

/// The formats a picker offers, with the strings each side needs.
///
/// `name` is what crosses the FFI, `label` is what the reader sees, and
/// `takes_quality` is whether the quality row is live. Three strings that have
/// to agree across two shells, so they are generated from the engine rather
/// than typed a second time in Swift.
#[test]
fn the_export_formats_fixture_is_current() {
    use pe_session::export::Format;

    let formats: Vec<serde_json::Value> = Format::ALL
        .iter()
        .map(|f| {
            serde_json::json!({
                "name": f.name(),
                "label": f.label(),
                "extension": f.extension(),
                "takes_quality": f.takes_quality(),
            })
        })
        .collect();

    let json = serde_json::to_string_pretty(&serde_json::json!({
        "formats": formats,
        // What a session starts at, so the Swift side can assert the panel's
        // opening state rather than assuming it.
        "default_format": Format::default().name(),
    }))
    .unwrap();
    check("export_formats.json", json);
}
