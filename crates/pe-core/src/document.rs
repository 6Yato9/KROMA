//! The document — the whole edit, as data.
//!
//! The stack *is* the document. There is no other representation of an edit,
//! and nothing here is ever destructive to the source image.
//!
//! Format rules, decided at M0 and treated as a compatibility commitment from
//! the first save:
//!
//! 1. **JSON, not binary.** Diffable and readable, which is worth days of
//!    debugging. The file is kilobytes regardless.
//! 2. **`schema_version` from commit one**, so a migration path exists before
//!    it is needed rather than after.
//! 3. **Unknown fields are preserved**, so a file touched by a newer build and
//!    saved by an older one does not lose data.

use serde::{Deserialize, Serialize};

use crate::geometry::{Geometry, Resize};
use crate::stack::Stack;

/// Bumped whenever the on-disk shape changes incompatibly. Migrations live in
/// [`migrate`].
pub const SCHEMA_VERSION: u32 = 1;

/// Where the pixels come from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Source {
    /// A path relative to the document, or absolute. Smaller files, but breaks
    /// if the user reorganises their library.
    Path { path: String },
    /// The original bytes, base64'd into the document. Survives anything, at
    /// the cost of size.
    Embedded { format: String, data: String },
}

/// The colour-management settings — Resolve's project-settings panel, per
/// document.
///
/// Spaces are stored by name rather than by value so that a document does not
/// bake in a numeric matrix that a later, more accurate derivation would
/// contradict.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorSettings {
    /// What the source file is. `"sRGB"` for most JPEGs.
    pub input: String,
    /// What we render to for display and export.
    pub output: String,
}

impl Default for ColorSettings {
    fn default() -> Self {
        Self {
            input: "sRGB".into(),
            output: "sRGB".into(),
        }
    }
}

impl ColorSettings {
    /// Resolve the names into real colour spaces.
    ///
    /// Unknown names fall back to sRGB rather than erroring: a document
    /// referencing a space this build has not learned about yet should still
    /// open and show something sensible.
    pub fn pipeline(&self) -> pe_color::Pipeline {
        pe_color::Pipeline::new(
            pe_color::space::by_name(&self.input).unwrap_or(pe_color::space::SRGB),
            pe_color::space::by_name(&self.output).unwrap_or(pe_color::space::SRGB),
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Metadata {
    pub title: Option<String>,
    pub note: Option<String>,
    /// Free-form tags. Not used at M0; reserved so the library work at M4 does
    /// not need a schema bump.
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: u32,
    pub source: Source,
    #[serde(default)]
    pub color: ColorSettings,
    /// Crop, straighten and flip. Applied before the stack, so "the frame"
    /// means the cropped frame everywhere downstream.
    #[serde(default)]
    pub geometry: Geometry,
    /// How big the exported file should be. The last thing applied.
    #[serde(default)]
    pub resize: Resize,
    #[serde(default)]
    pub stack: Stack,
    #[serde(default)]
    pub metadata: Metadata,
    /// Anything this build did not recognise, kept verbatim so a round trip
    /// through an older version is lossless.
    #[serde(flatten, default)]
    pub unknown: serde_json::Map<String, serde_json::Value>,
}

impl Document {
    pub fn new(source: Source) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            source,
            color: ColorSettings::default(),
            geometry: Geometry::default(),
            resize: Resize::default(),
            stack: Stack::default(),
            metadata: Metadata::default(),
            unknown: Default::default(),
        }
    }

    pub fn from_path(path: impl Into<String>) -> Self {
        Self::new(Source::Path { path: path.into() })
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(s: &str) -> Result<Self, DocumentError> {
        let mut value: serde_json::Value = serde_json::from_str(s)?;
        let version = value
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .ok_or(DocumentError::MissingSchemaVersion)? as u32;
        if version > SCHEMA_VERSION {
            return Err(DocumentError::FromTheFuture {
                found: version,
                supported: SCHEMA_VERSION,
            });
        }
        migrate(&mut value, version)?;
        Ok(serde_json::from_value(value)?)
    }
}

/// Bring an older document up to [`SCHEMA_VERSION`] in place.
///
/// Empty at v1 by definition. It exists now so that the *call site* is already
/// wired up — retrofitting migration into a loader after files exist in the
/// wild is where compatibility bugs come from.
fn migrate(_value: &mut serde_json::Value, from: u32) -> Result<(), DocumentError> {
    match from {
        1 => Ok(()),
        0 => Err(DocumentError::UnsupportedVersion(from)),
        _ => Err(DocumentError::UnsupportedVersion(from)),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("document has no schema_version field")]
    MissingSchemaVersion,
    #[error("document schema v{found} is newer than this build supports (v{supported})")]
    FromTheFuture { found: u32, supported: u32 },
    #[error("document schema v{0} is not supported")]
    UnsupportedVersion(u32),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ParamValue;
    use crate::stack::{BlendMode, RowId, StackRow};

    fn sample() -> Document {
        let mut doc = Document::from_path("DSCF1234.JPG");
        let mut row = StackRow::new(RowId(1), "exposure");
        row.params.set("ev", ParamValue::Float(0.35));
        doc.stack.push(row);

        let mut grain = StackRow::new(RowId(2), "grain");
        grain.opacity = 0.4;
        grain.blend = BlendMode::Screen;
        doc.stack.push(grain);
        doc
    }

    #[test]
    fn round_trips_through_json() {
        let doc = sample();
        let json = doc.to_json().unwrap();
        let back = Document::from_json(&json).unwrap();
        assert_eq!(doc, back);
    }

    #[test]
    fn serialisation_is_byte_stable() {
        // Two identical documents must produce identical bytes, or the golden
        // tests and any future diffing are worthless.
        assert_eq!(sample().to_json().unwrap(), sample().to_json().unwrap());
    }

    #[test]
    fn a_newer_schema_is_refused_rather_than_misread() {
        let json = r#"{"schema_version": 99, "source": {"kind":"path","path":"a.jpg"}}"#;
        assert!(matches!(
            Document::from_json(json),
            Err(DocumentError::FromTheFuture { found: 99, .. })
        ));
    }

    #[test]
    fn a_missing_schema_version_is_an_error() {
        let json = r#"{"source": {"kind":"path","path":"a.jpg"}}"#;
        assert!(matches!(
            Document::from_json(json),
            Err(DocumentError::MissingSchemaVersion)
        ));
    }

    #[test]
    fn unknown_top_level_fields_survive_a_round_trip() {
        // Simulates opening a file written by a newer build.
        let json = r#"{
            "schema_version": 1,
            "source": {"kind":"path","path":"a.jpg"},
            "future_feature": {"strength": 0.5}
        }"#;
        let doc = Document::from_json(json).unwrap();
        assert!(doc.unknown.contains_key("future_feature"));
        let back = doc.to_json().unwrap();
        assert!(
            back.contains("future_feature"),
            "newer-version data was dropped: {back}"
        );
    }

    #[test]
    fn rows_keep_their_blend_and_opacity_across_a_save() {
        let doc = sample();
        let back = Document::from_json(&doc.to_json().unwrap()).unwrap();
        let grain = back.stack.get(RowId(2)).unwrap();
        assert_eq!(grain.blend, BlendMode::Screen);
        assert_eq!(grain.opacity, 0.4);
    }

    #[test]
    fn defaults_are_filled_in_for_a_minimal_document() {
        let json = r#"{"schema_version":1,"source":{"kind":"path","path":"a.jpg"}}"#;
        let doc = Document::from_json(json).unwrap();
        assert_eq!(doc.color.input, "sRGB");
        assert!(doc.stack.is_empty());
    }

    #[test]
    fn a_row_omitting_optional_fields_still_loads_enabled_and_opaque() {
        let json = r#"{
            "schema_version":1,
            "source":{"kind":"path","path":"a.jpg"},
            "stack":[{"id":7,"effect":"exposure"}]
        }"#;
        let doc = Document::from_json(json).unwrap();
        let row = doc.stack.get(RowId(7)).unwrap();
        assert!(row.enabled);
        assert_eq!(row.opacity, 1.0);
        assert_eq!(row.blend, BlendMode::Normal);
    }

    #[test]
    fn colour_settings_resolve_to_a_pipeline() {
        let doc = sample();
        let p = doc.color.pipeline();
        assert_eq!(p.input.name, "sRGB");
        assert_eq!(p.output.name, "sRGB");
    }

    #[test]
    fn an_unknown_colour_space_falls_back_to_srgb() {
        let cs = ColorSettings {
            input: "Imaginary Space".into(),
            output: "sRGB".into(),
        };
        assert_eq!(cs.pipeline().input.name, "sRGB");
    }
}
