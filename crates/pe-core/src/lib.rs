//! The document model: what an edit *is*, independent of how it is rendered.
//!
//! No GPU, no I/O, no UI. This crate could be compiled for a machine with no
//! graphics hardware at all and still be fully testable — which is the point,
//! because it means the rules about what a valid edit looks like are verified
//! without needing a device.
//!
//! The central idea lives in [`stack`]: **every row carries its own
//! `enabled`, `opacity`, `blend` and `key`**. That mirrors a Resolve node's
//! anatomy, and it means masking, blend modes and partial-strength effects are
//! properties that already exist rather than features to be added to each
//! effect one at a time.

pub mod curve;
pub mod document;
pub mod history;
pub mod parametric;
pub mod params;
pub mod stack;

pub use document::{ColorSettings, Document, DocumentError, Metadata, SCHEMA_VERSION, Source};
pub use history::History;
pub use params::{Curve, ParamMap, ParamValue, Wheel};
pub use stack::{BlendMode, Key, KeyAdjust, RowId, Stack, StackRow, WindowShape};

/// Hands out row identifiers that are unique within a document.
///
/// Monotonic and never reused, so a stale reference from the UI or an undo
/// stack can be detected as missing rather than silently pointing at a
/// different row that happens to have taken the same id.
#[derive(Debug, Default, Clone)]
pub struct RowIdGenerator {
    next: u64,
}

impl RowIdGenerator {
    /// Start after the highest id already present, so ids remain unique when
    /// resuming work on a loaded document.
    pub fn resuming(doc: &Document) -> Self {
        let next = doc.stack.iter().map(|r| r.id.0).max().map_or(0, |m| m + 1);
        Self { next }
    }

    /// Hand out the next unused id.
    ///
    /// Named `allocate` rather than `next` so it is not mistaken for an
    /// iterator; this never terminates and never yields `None`.
    pub fn allocate(&mut self) -> RowId {
        let id = RowId(self.next);
        self.next += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_unique() {
        let mut g = RowIdGenerator::default();
        let ids: Vec<_> = (0..100).map(|_| g.allocate()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
    }

    #[test]
    fn resuming_avoids_colliding_with_loaded_rows() {
        let mut doc = Document::from_path("a.jpg");
        doc.stack.push(StackRow::new(RowId(4), "exposure"));
        doc.stack.push(StackRow::new(RowId(9), "grain"));

        let mut g = RowIdGenerator::resuming(&doc);
        let fresh = g.allocate();
        assert_eq!(fresh, RowId(10));
        assert!(doc.stack.get(fresh).is_none());
    }

    #[test]
    fn resuming_on_an_empty_document_starts_at_zero() {
        let doc = Document::from_path("a.jpg");
        assert_eq!(RowIdGenerator::resuming(&doc).allocate(), RowId(0));
    }
}
