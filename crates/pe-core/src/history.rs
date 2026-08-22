//! Undo/redo.
//!
//! Snapshot-based, deliberately. A document is a few kilobytes of JSON-shaped
//! data, so cloning one per edit costs nothing measurable, and snapshots are
//! impossible to get subtly wrong in the way that inverse-operation command
//! stacks are. If profiling ever says otherwise, the interface here is narrow
//! enough to swap.
//!
//! The one thing it does need to be clever about is **coalescing**: dragging a
//! slider produces hundreds of edits per second, and each must not become its
//! own undo step. Edits carry a [`CoalesceKey`], and consecutive edits sharing
//! one collapse into a single entry.

use crate::document::Document;

/// Identifies a run of edits that should collapse into one undo step.
///
/// `None` never coalesces — use it for discrete actions like adding or deleting
/// a row.
pub type CoalesceKey = Option<String>;

struct Entry {
    doc: Document,
    label: String,
    coalesce: CoalesceKey,
}

pub struct History {
    /// See [`Self::revision`].
    revision: u64,
    /// Snapshots *before* each edit, oldest first.
    past: Vec<Entry>,
    /// Snapshots undone, most recently undone last.
    future: Vec<Entry>,
    current: Document,
    limit: usize,
}

impl History {
    pub fn new(doc: Document) -> Self {
        Self {
            revision: 0,
            past: Vec::new(),
            future: Vec::new(),
            current: doc,
            limit: 200,
        }
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    /// How many times the document has changed.
    ///
    /// A counter rather than a hash of the document, because the question
    /// anything asks of it — "is this still what I last saw?" — is answered
    /// exactly by a counter and only probably by a hash, and the autosave
    /// consults it on every frame. Undo and redo count as changes, because
    /// from the outside they are.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn document(&self) -> &Document {
        &self.current
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Label of the edit that [`History::undo`] would reverse.
    pub fn undo_label(&self) -> Option<&str> {
        self.past.last().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.future.last().map(|e| e.label.as_str())
    }

    /// Apply an edit, recording the prior state.
    ///
    /// `coalesce`: consecutive edits with the same `Some(key)` merge into one
    /// undo step. Pass `None` for discrete actions.
    pub fn edit<F>(&mut self, label: impl Into<String>, coalesce: CoalesceKey, f: F)
    where
        F: FnOnce(&mut Document),
    {
        let label = label.into();
        let merges = coalesce.is_some()
            && self
                .past
                .last()
                .is_some_and(|prev| prev.coalesce == coalesce);

        if !merges {
            self.past.push(Entry {
                doc: self.current.clone(),
                label,
                coalesce,
            });
            if self.past.len() > self.limit {
                self.past.remove(0);
            }
        }
        // Any new edit invalidates the redo branch.
        self.future.clear();
        f(&mut self.current);
        self.revision += 1;
    }

    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.past.pop() else {
            return false;
        };
        let restored = std::mem::replace(&mut self.current, entry.doc);
        self.future.push(Entry {
            doc: restored,
            label: entry.label,
            coalesce: entry.coalesce,
        });
        self.revision += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(entry) = self.future.pop() else {
            return false;
        };
        let restored = std::mem::replace(&mut self.current, entry.doc);
        self.past.push(Entry {
            doc: restored,
            label: entry.label,
            coalesce: entry.coalesce,
        });
        self.revision += 1;
        true
    }

    /// End a coalescing run — call on pointer-up so the next drag of the same
    /// slider starts a fresh undo step.
    pub fn break_coalescing(&mut self) {
        if let Some(last) = self.past.last_mut() {
            last.coalesce = None;
        }
    }
}

#[cfg(test)]
mod tests {

    /// The revision has to move for *every* change, including the ones that
    /// undo other changes. Anything watching it — the autosave, for one — is
    /// asking "is this still what I last saw", and an undo means it is not.
    #[test]
    fn every_change_moves_the_revision_including_undo() {
        let mut h = History::new(Document::from_path("photo.jpg"));
        let start = h.revision();
        h.edit("one", None, |d| d.stack.rows.clear());
        let after_edit = h.revision();
        assert!(after_edit > start, "an edit did not move the revision");

        assert!(h.undo());
        assert!(h.revision() > after_edit, "an undo did not move it");
        let after_undo = h.revision();
        assert!(h.redo());
        assert!(h.revision() > after_undo, "a redo did not move it");
    }

    /// And it must not move when nothing happened, or the autosave writes on
    /// every frame forever.
    #[test]
    fn a_refused_undo_does_not_move_the_revision() {
        let mut h = History::new(Document::from_path("photo.jpg"));
        let before = h.revision();
        assert!(!h.undo());
        assert!(!h.redo());
        assert_eq!(h.revision(), before);
    }
    use super::*;
    use crate::params::ParamValue;
    use crate::stack::{RowId, StackRow};

    fn doc() -> Document {
        Document::from_path("a.jpg")
    }

    fn set_ev(d: &mut Document, v: f32) {
        if d.stack.is_empty() {
            d.stack.push(StackRow::new(RowId(1), "exposure"));
        }
        d.stack
            .get_mut(RowId(1))
            .unwrap()
            .params
            .set("ev", ParamValue::Float(v));
    }

    fn ev_of(h: &History) -> Option<f32> {
        h.document()
            .stack
            .get(RowId(1))
            .and_then(|r| r.params.get("ev"))
            .and_then(ParamValue::as_float)
    }

    #[test]
    fn undo_restores_the_previous_state() {
        let mut h = History::new(doc());
        h.edit("Set exposure", None, |d| set_ev(d, 1.0));
        assert_eq!(ev_of(&h), Some(1.0));
        assert!(h.undo());
        assert_eq!(ev_of(&h), None);
    }

    #[test]
    fn redo_reapplies() {
        let mut h = History::new(doc());
        h.edit("Set exposure", None, |d| set_ev(d, 1.0));
        h.undo();
        assert!(h.redo());
        assert_eq!(ev_of(&h), Some(1.0));
    }

    #[test]
    fn nothing_to_undo_on_a_fresh_history() {
        let mut h = History::new(doc());
        assert!(!h.can_undo());
        assert!(!h.undo());
    }

    #[test]
    fn a_slider_drag_collapses_into_one_undo_step() {
        let mut h = History::new(doc());
        let key = Some("row1.ev".to_string());
        for i in 1..=50 {
            h.edit("Exposure", key.clone(), |d| set_ev(d, i as f32 / 50.0));
        }
        assert_eq!(ev_of(&h), Some(1.0));
        assert!(h.undo());
        // One undo returns to before the whole drag, not to step 49.
        assert_eq!(ev_of(&h), None);
        assert!(!h.can_undo());
    }

    #[test]
    fn different_controls_do_not_coalesce_together() {
        let mut h = History::new(doc());
        h.edit("Exposure", Some("row1.ev".into()), |d| set_ev(d, 0.5));
        h.edit("Contrast", Some("row1.contrast".into()), |d| {
            d.stack
                .get_mut(RowId(1))
                .unwrap()
                .params
                .set("contrast", ParamValue::Float(0.2));
        });
        h.undo();
        assert_eq!(ev_of(&h), Some(0.5));
    }

    #[test]
    fn breaking_coalescing_starts_a_new_step() {
        let mut h = History::new(doc());
        let key = Some("row1.ev".to_string());
        h.edit("Exposure", key.clone(), |d| set_ev(d, 0.3));
        h.break_coalescing();
        h.edit("Exposure", key.clone(), |d| set_ev(d, 0.9));

        assert!(h.undo());
        assert_eq!(ev_of(&h), Some(0.3), "second drag should undo on its own");
    }

    #[test]
    fn a_new_edit_discards_the_redo_branch() {
        let mut h = History::new(doc());
        h.edit("a", None, |d| set_ev(d, 1.0));
        h.undo();
        assert!(h.can_redo());
        h.edit("b", None, |d| set_ev(d, 2.0));
        assert!(!h.can_redo());
    }

    #[test]
    fn history_is_bounded() {
        let mut h = History::new(doc()).with_limit(10);
        for i in 0..100 {
            h.edit(format!("edit {i}"), None, |d| set_ev(d, i as f32));
        }
        let mut undos = 0;
        while h.undo() {
            undos += 1;
        }
        assert_eq!(undos, 10);
    }

    #[test]
    fn labels_describe_the_pending_undo() {
        let mut h = History::new(doc());
        h.edit("Add Grain", None, |d| {
            d.stack.push(StackRow::new(RowId(9), "grain"))
        });
        assert_eq!(h.undo_label(), Some("Add Grain"));
        h.undo();
        assert_eq!(h.redo_label(), Some("Add Grain"));
    }
}
