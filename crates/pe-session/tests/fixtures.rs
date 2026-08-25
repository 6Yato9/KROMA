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

/// What belongs behind each curve, and the window a tone plot spans.
///
/// The mapping is shared knowledge with one right answer per curve, and the
/// window is two numbers that already appear in three places. Both cross to
/// Swift here rather than being typed a fourth time.
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

    let json = serde_json::to_string_pretty(&serde_json::json!({
        "window": { "black": LOG_BLACK, "white": LOG_WHITE },
        "behind": mapping,
    }))
    .unwrap();
    check("backdrop_samples.json", json);
}
