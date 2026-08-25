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
