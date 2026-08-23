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
