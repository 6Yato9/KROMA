//! Effect parameters.
//!
//! Dynamically typed rather than a struct per effect, because the document has
//! to round-trip effects this build may not know about — a file saved by a
//! newer version must open, keep its unknown rows intact, and save back without
//! silently dropping them.
//!
//! Backed by a `BTreeMap` specifically for deterministic key ordering. The
//! golden tests diff serialised documents, and a `HashMap` would make every
//! save produce a different byte sequence for identical content.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A colour wheel: per-channel offsets plus a master.
///
/// Resolve's wheels are four-valued — the three channels and the luminance ring
/// around the outside. Modelling the master separately rather than folding it
/// into the channels keeps "reset just the ring" possible.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wheel {
    pub rgb: [f32; 3],
    pub master: f32,
}

impl Default for Wheel {
    fn default() -> Self {
        Self {
            rgb: [0.0; 3],
            master: 0.0,
        }
    }
}

impl Wheel {
    pub fn is_neutral(&self) -> bool {
        self.rgb.iter().all(|c| *c == 0.0) && self.master == 0.0
    }
}

/// A curve as a list of control points in `[x, y]` order.
///
/// Stored as points rather than a baked LUT so the curve stays editable and
/// resolution-independent; the renderer bakes it to a texture on upload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Curve {
    pub points: Vec<[f32; 2]>,
}

impl Curve {
    /// A flat line down the middle.
    ///
    /// The identity for a *secondary* curve. Hue Vs Sat and its five siblings
    /// do not map a value onto itself the way a tone curve does — they answer
    /// "what should happen to this hue", and the answer that changes nothing
    /// is the same everywhere. Which is why their neutral is a level line and
    /// a tone curve's is a diagonal.
    pub fn flat() -> Self {
        Self {
            points: vec![[0.0, 0.5], [1.0, 0.5]],
        }
    }

    /// Whether this curve leaves a secondary alone.
    pub fn is_flat(&self) -> bool {
        self.points.iter().all(|p| (p[1] - 0.5).abs() < 1e-4)
    }
}

impl Default for Curve {
    fn default() -> Self {
        // Identity: a straight line from black to white.
        Self {
            points: vec![[0.0, 0.0], [1.0, 1.0]],
        }
    }
}

impl Curve {
    pub fn is_identity(&self) -> bool {
        self.points.len() == 2 && self.points[0] == [0.0, 0.0] && self.points[1] == [1.0, 1.0]
    }

    /// Points sorted by x, which is what any evaluator needs and what dragging
    /// a control point past its neighbour can violate.
    pub fn sorted(&self) -> Vec<[f32; 2]> {
        let mut p = self.points.clone();
        p.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
        p
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "lowercase")]
pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    /// A linear RGB triple, e.g. a tint colour.
    Rgb([f32; 3]),
    Wheel(Wheel),
    Curve(Curve),
    /// One of an effect's enumerated options, stored by key.
    Choice(String),
    /// A lattice of displacements — the Colour Warper's grids.
    Warp(crate::warp::Warp),
}

impl ParamValue {
    pub fn as_float(&self) -> Option<f32> {
        match self {
            ParamValue::Float(v) => Some(*v),
            ParamValue::Int(v) => Some(*v as f32),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ParamValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_warp(&self) -> Option<&crate::warp::Warp> {
        match self {
            ParamValue::Warp(w) => Some(w),
            _ => None,
        }
    }

    pub fn as_wheel(&self) -> Option<&Wheel> {
        match self {
            ParamValue::Wheel(w) => Some(w),
            _ => None,
        }
    }

    pub fn as_curve(&self) -> Option<&Curve> {
        match self {
            ParamValue::Curve(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_choice(&self) -> Option<&str> {
        match self {
            ParamValue::Choice(s) => Some(s),
            _ => None,
        }
    }

    /// Short type tag, used in error messages when a document supplies the
    /// wrong kind of value for a parameter.
    pub fn type_name(&self) -> &'static str {
        match self {
            ParamValue::Float(_) => "float",
            ParamValue::Int(_) => "int",
            ParamValue::Bool(_) => "bool",
            ParamValue::Rgb(_) => "rgb",
            ParamValue::Wheel(_) => "wheel",
            ParamValue::Curve(_) => "curve",
            ParamValue::Choice(_) => "choice",
            ParamValue::Warp(_) => "warp",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParamMap(pub BTreeMap<String, ParamValue>);

impl ParamMap {
    pub fn get(&self, key: &str) -> Option<&ParamValue> {
        self.0.get(key)
    }

    pub fn set(&mut self, key: impl Into<String>, value: ParamValue) {
        self.0.insert(key.into(), value);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Read a float, falling back to `default` when absent or the wrong type.
    ///
    /// Effects use this rather than indexing so that a malformed or
    /// partially-written document degrades to sensible values instead of
    /// panicking mid-render.
    pub fn float_or(&self, key: &str, default: f32) -> f32 {
        self.get(key)
            .and_then(ParamValue::as_float)
            .unwrap_or(default)
    }

    pub fn bool_or(&self, key: &str, default: bool) -> bool {
        self.get(key)
            .and_then(ParamValue::as_bool)
            .unwrap_or(default)
    }

    pub fn choice_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.get(key)
            .and_then(ParamValue::as_choice)
            .unwrap_or(default)
    }
}

impl FromIterator<(String, ParamValue)> for ParamMap {
    fn from_iter<T: IntoIterator<Item = (String, ParamValue)>>(iter: T) -> Self {
        ParamMap(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_params_fall_back_to_defaults() {
        let p = ParamMap::default();
        assert_eq!(p.float_or("exposure", 0.0), 0.0);
        assert!(p.bool_or("preserve_luma", true));
        assert_eq!(p.choice_or("mode", "rgb"), "rgb");
    }

    #[test]
    fn wrong_typed_params_fall_back_rather_than_panic() {
        // A hand-edited or newer-version document can contain anything.
        let mut p = ParamMap::default();
        p.set("exposure", ParamValue::Choice("nonsense".into()));
        assert_eq!(p.float_or("exposure", 0.5), 0.5);
    }

    #[test]
    fn ints_read_as_floats() {
        let mut p = ParamMap::default();
        p.set("iterations", ParamValue::Int(3));
        assert_eq!(p.float_or("iterations", 0.0), 3.0);
    }

    #[test]
    fn serialisation_key_order_is_stable() {
        let mut a = ParamMap::default();
        a.set("zebra", ParamValue::Float(1.0));
        a.set("alpha", ParamValue::Float(2.0));
        a.set("middle", ParamValue::Float(3.0));

        let mut b = ParamMap::default();
        b.set("middle", ParamValue::Float(3.0));
        b.set("alpha", ParamValue::Float(2.0));
        b.set("zebra", ParamValue::Float(1.0));

        // Insertion order differs; serialised bytes must not.
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }

    #[test]
    fn param_values_round_trip_through_json() {
        let values = vec![
            ParamValue::Float(0.5),
            ParamValue::Int(-3),
            ParamValue::Bool(true),
            ParamValue::Rgb([0.1, 0.2, 0.3]),
            ParamValue::Wheel(Wheel {
                rgb: [0.01, -0.02, 0.03],
                master: 0.05,
            }),
            ParamValue::Curve(Curve {
                points: vec![[0.0, 0.0], [0.5, 0.6], [1.0, 1.0]],
            }),
            ParamValue::Choice("screen".into()),
        ];
        for v in values {
            let json = serde_json::to_string(&v).unwrap();
            let back: ParamValue = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back, "json was {json}");
        }
    }

    #[test]
    fn default_curve_is_the_identity() {
        assert!(Curve::default().is_identity());
    }

    #[test]
    fn sorted_fixes_dragged_past_neighbours() {
        let c = Curve {
            points: vec![[0.0, 0.0], [0.8, 0.4], [0.3, 0.9], [1.0, 1.0]],
        };
        let xs: Vec<f32> = c.sorted().iter().map(|p| p[0]).collect();
        assert_eq!(xs, vec![0.0, 0.3, 0.8, 1.0]);
    }

    #[test]
    fn default_wheel_is_neutral() {
        assert!(Wheel::default().is_neutral());
    }
}
