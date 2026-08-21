//! The stage cache — why dragging a slider stays at 60fps regardless of how
//! deep the stack is.
//!
//! After every row renders, its output texture is kept. When the user changes
//! something, only rows from the first *changed* row onward need to re-run;
//! everything above it is already sitting in VRAM. Dragging the Grain slider at
//! position 9 of 12 re-runs four rows, not twelve.
//!
//! Without this the renderer re-runs the whole stack on every pointer move,
//! and the application feels progressively worse the more the user does to an
//! image — which is exactly backwards.
//!
//! This module is deliberately GPU-free so the invalidation logic, which is
//! where the bugs live, is testable without a device.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use pe_core::{Stack, StackRow};

/// A content hash of everything about a row that affects its output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RowFingerprint(pub u64);

/// Hash a row by its serialised form.
///
/// Serialisation rather than a hand-written `Hash` impl on purpose: parameters
/// are dynamically typed and contain floats, and a hand-rolled hash is the kind
/// of thing that silently stops covering a field the day someone adds one.
/// `ParamMap` is a `BTreeMap`, so the bytes are deterministic.
pub fn fingerprint(row: &StackRow) -> RowFingerprint {
    let mut h = DefaultHasher::new();
    match serde_json::to_string(row) {
        Ok(s) => s.hash(&mut h),
        // A row that cannot be serialised is a bug, but failing to render is a
        // worse outcome than failing to cache. Hash the id and treat the row as
        // permanently dirty.
        Err(_) => {
            row.id.0.hash(&mut h);
            u64::MAX.hash(&mut h);
        }
    }
    RowFingerprint(h.finish())
}

/// What needs to happen to bring the output up to date.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderPlan {
    /// Index of the first row whose cached output is no longer valid.
    /// Equals the stack length when nothing changed.
    pub first_dirty: usize,
    /// The cache slot to start rendering *from*. `None` means start from the
    /// decoded source image.
    pub reuse: Option<usize>,
    /// Indices that will actually execute, with disabled and fully transparent
    /// rows skipped.
    pub execute: Vec<usize>,
}

impl RenderPlan {
    /// True when the cached output is already correct and no GPU work is needed.
    pub fn is_up_to_date(&self) -> bool {
        self.execute.is_empty() && self.first_dirty == self.reuse.map_or(0, |r| r + 1)
    }
}

/// Identifies the inputs that invalidate *everything* when they change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderContext {
    /// Hash of the source image. A different photo invalidates every stage.
    pub source: u64,
    /// Preview dimensions. Interactive rendering happens at screen resolution,
    /// so a window resize invalidates the whole chain.
    pub width: u32,
    pub height: u32,
    /// Hash of the colour-management settings, which sit outside the stack but
    /// affect every stage.
    pub color: u64,
    /// Hash of the crop and straighten settings. They run before row zero, so
    /// a change to them makes every cached stage stale even when the output
    /// happens to come out the same size.
    pub geometry: u64,
    /// Which rectangle of the frame is being rendered. Panning or zooming
    /// makes every cached stage stale, because they were rendered for a
    /// different part of the picture.
    pub view: u64,
}

#[derive(Default)]
pub struct StageCache {
    /// Fingerprint of the row that produced each cached slot.
    slots: Vec<RowFingerprint>,
    context: Option<RenderContext>,
}

impl StageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of valid cached stages.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Throw everything away. Called when the source image or output size
    /// changes.
    pub fn clear(&mut self) {
        self.slots.clear();
        self.context = None;
    }

    /// Discard cached stages from `index` onward.
    pub fn invalidate_from(&mut self, index: usize) {
        self.slots.truncate(index);
    }

    /// Work out the minimum work needed to render `stack` in `context`.
    ///
    /// `inert` decides which rows can be skipped entirely. It is a parameter
    /// rather than a method on the row because that policy needs the effect
    /// registry — whether the parameters sit at their neutral values — and
    /// invalidation logic has no business knowing about effects. Keeping the
    /// two apart is also what lets these tests use synthetic effect names.
    pub fn plan(
        &mut self,
        stack: &Stack,
        context: RenderContext,
        inert: impl Fn(&StackRow) -> bool,
    ) -> RenderPlan {
        if self.context != Some(context) {
            self.clear();
            self.context = Some(context);
        }

        // Longest prefix whose fingerprints still match what we cached.
        let mut first_dirty = 0;
        while first_dirty < stack.len()
            && first_dirty < self.slots.len()
            && self.slots[first_dirty] == fingerprint(&stack.rows[first_dirty])
        {
            first_dirty += 1;
        }

        // Anything cached beyond the matching prefix is stale.
        self.invalidate_from(first_dirty);

        let execute = (first_dirty..stack.len())
            .filter(|i| !inert(&stack.rows[*i]))
            .collect();

        RenderPlan {
            first_dirty,
            reuse: first_dirty.checked_sub(1),
            execute,
        }
    }

    /// Record that row `index` rendered successfully.
    ///
    /// Must be called in order; storing out of sequence would let a later stage
    /// be reused while an earlier one is missing.
    pub fn store(&mut self, index: usize, row: &StackRow) {
        debug_assert!(
            index <= self.slots.len(),
            "stage {index} stored before {} exists",
            self.slots.len()
        );
        let fp = fingerprint(row);
        if index < self.slots.len() {
            self.slots[index] = fp;
        } else {
            self.slots.push(fp);
        }
    }

    /// Record every row a plan executed, plus the no-ops it skipped.
    ///
    /// No-op rows still occupy a cache slot: their "output" is their input, and
    /// giving them a slot keeps indices aligned with the stack so that
    /// re-enabling one invalidates from the right place.
    pub fn store_plan(&mut self, stack: &Stack, plan: &RenderPlan) {
        for i in plan.first_dirty..stack.len() {
            self.store(i, &stack.rows[i]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pe_core::{BlendMode, ParamValue, RowId, StackRow};

    const CTX: RenderContext = RenderContext {
        source: 1,
        width: 1920,
        height: 1080,
        color: 7,
        geometry: 11,
        view: 0,
    };

    /// Cropping runs before row zero, so a change to it invalidates the whole
    /// chain — including the case that would otherwise slip through, where a
    /// rotation leaves the output exactly the same size.
    #[test]
    fn changing_the_crop_invalidates_every_stage() {
        let stack = stack_of(3);
        let mut cache = StageCache::new();
        let first = cache.plan(&stack, CTX, |_| false);
        cache.store_plan(&stack, &first);
        assert_eq!(cache.len(), 3);

        let moved = RenderContext {
            geometry: 12,
            ..CTX
        };
        let plan = cache.plan(&stack, moved, |_| false);
        assert_eq!(plan.first_dirty, 0);
        assert_eq!(plan.reuse, None);
    }

    fn stack_of(n: u64) -> Stack {
        let mut s = Stack::default();
        for i in 0..n {
            s.push(StackRow::new(RowId(i), format!("effect{i}")));
        }
        s
    }

    /// Render once so the cache is warm, then return it.
    fn warmed(stack: &Stack) -> StageCache {
        let mut c = StageCache::new();
        let plan = c.plan(stack, CTX, StackRow::is_noop);
        c.store_plan(stack, &plan);
        c
    }

    fn set_param(stack: &mut Stack, id: u64, v: f32) {
        stack
            .get_mut(RowId(id))
            .unwrap()
            .params
            .set("x", ParamValue::Float(v));
    }

    #[test]
    fn a_cold_cache_renders_everything_from_the_source() {
        let stack = stack_of(5);
        let mut c = StageCache::new();
        let plan = c.plan(&stack, CTX, StackRow::is_noop);
        assert_eq!(plan.first_dirty, 0);
        assert_eq!(plan.reuse, None);
        assert_eq!(plan.execute, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn an_unchanged_stack_needs_no_work() {
        let stack = stack_of(5);
        let mut c = warmed(&stack);
        let plan = c.plan(&stack, CTX, StackRow::is_noop);
        assert!(plan.execute.is_empty());
        assert_eq!(plan.first_dirty, 5);
        assert!(plan.is_up_to_date());
    }

    /// The headline behaviour: touching a deep row is cheap.
    #[test]
    fn editing_the_last_row_reruns_only_that_row() {
        let mut stack = stack_of(12);
        let mut c = warmed(&stack);

        set_param(&mut stack, 11, 0.5);
        let plan = c.plan(&stack, CTX, StackRow::is_noop);

        assert_eq!(plan.first_dirty, 11);
        assert_eq!(plan.reuse, Some(10), "stage 10 should be reused from VRAM");
        assert_eq!(plan.execute, vec![11]);
    }

    #[test]
    fn editing_a_middle_row_reruns_it_and_everything_below() {
        let mut stack = stack_of(12);
        let mut c = warmed(&stack);

        set_param(&mut stack, 8, 0.5);
        let plan = c.plan(&stack, CTX, StackRow::is_noop);

        assert_eq!(plan.first_dirty, 8);
        assert_eq!(plan.reuse, Some(7));
        assert_eq!(plan.execute, vec![8, 9, 10, 11]);
    }

    #[test]
    fn editing_the_first_row_reruns_everything() {
        let mut stack = stack_of(6);
        let mut c = warmed(&stack);

        set_param(&mut stack, 0, 0.5);
        let plan = c.plan(&stack, CTX, StackRow::is_noop);

        assert_eq!(plan.first_dirty, 0);
        assert_eq!(plan.reuse, None);
        assert_eq!(plan.execute.len(), 6);
    }

    #[test]
    fn opacity_and_blend_changes_invalidate_too() {
        // These live on the row rather than in params, so they are the easiest
        // fields for a fingerprint to accidentally miss.
        for mutate in [
            (|s: &mut Stack| s.get_mut(RowId(3)).unwrap().opacity = 0.5) as fn(&mut Stack),
            |s: &mut Stack| s.get_mut(RowId(3)).unwrap().blend = BlendMode::Screen,
            |s: &mut Stack| s.get_mut(RowId(3)).unwrap().label = Some("x".into()),
        ] {
            let mut stack = stack_of(6);
            let mut c = warmed(&stack);
            mutate(&mut stack);
            assert_eq!(c.plan(&stack, CTX, StackRow::is_noop).first_dirty, 3);
        }
    }

    #[test]
    fn disabling_a_row_invalidates_from_it_and_skips_it() {
        let mut stack = stack_of(6);
        let mut c = warmed(&stack);

        stack.get_mut(RowId(2)).unwrap().enabled = false;
        let plan = c.plan(&stack, CTX, StackRow::is_noop);

        assert_eq!(plan.first_dirty, 2);
        assert_eq!(plan.execute, vec![3, 4, 5], "row 2 should be skipped");
    }

    #[test]
    fn a_fully_transparent_row_is_skipped() {
        let mut stack = stack_of(4);
        stack.get_mut(RowId(1)).unwrap().opacity = 0.0;
        let mut c = StageCache::new();
        assert_eq!(
            c.plan(&stack, CTX, StackRow::is_noop).execute,
            vec![0, 2, 3]
        );
    }

    #[test]
    fn re_enabling_a_row_invalidates_from_the_right_place() {
        // Regression guard for the subtle one: if skipped rows did not occupy a
        // cache slot, indices would drift and this would invalidate from the
        // wrong row.
        let mut stack = stack_of(6);
        stack.get_mut(RowId(2)).unwrap().enabled = false;
        let mut c = warmed(&stack);

        stack.get_mut(RowId(2)).unwrap().enabled = true;
        let plan = c.plan(&stack, CTX, StackRow::is_noop);

        assert_eq!(plan.first_dirty, 2);
        assert_eq!(plan.execute, vec![2, 3, 4, 5]);
    }

    #[test]
    fn appending_a_row_reuses_the_whole_existing_chain() {
        let mut stack = stack_of(8);
        let mut c = warmed(&stack);

        stack.push(StackRow::new(RowId(99), "grain"));
        let plan = c.plan(&stack, CTX, StackRow::is_noop);

        assert_eq!(plan.first_dirty, 8);
        assert_eq!(plan.reuse, Some(7));
        assert_eq!(plan.execute, vec![8]);
    }

    #[test]
    fn removing_a_row_invalidates_from_its_index() {
        let mut stack = stack_of(8);
        let mut c = warmed(&stack);

        stack.remove(RowId(5));
        let plan = c.plan(&stack, CTX, StackRow::is_noop);

        assert_eq!(plan.first_dirty, 5);
        assert_eq!(plan.execute, vec![5, 6]);
    }

    #[test]
    fn reordering_invalidates_from_the_earlier_of_the_two_positions() {
        let mut stack = stack_of(10);
        let mut c = warmed(&stack);

        stack.reorder(RowId(7), 2);
        let plan = c.plan(&stack, CTX, StackRow::is_noop);

        assert_eq!(plan.first_dirty, 2);
        assert_eq!(plan.reuse, Some(1));
    }

    #[test]
    fn a_resize_invalidates_everything() {
        let stack = stack_of(6);
        let mut c = warmed(&stack);

        let resized = RenderContext { width: 1280, ..CTX };
        let plan = c.plan(&stack, resized, StackRow::is_noop);

        assert_eq!(plan.first_dirty, 0, "preview renders at screen resolution");
        assert_eq!(plan.execute.len(), 6);
    }

    #[test]
    fn a_different_source_image_invalidates_everything() {
        let stack = stack_of(6);
        let mut c = warmed(&stack);
        let plan = c.plan(
            &stack,
            RenderContext { source: 2, ..CTX },
            StackRow::is_noop,
        );
        assert_eq!(plan.first_dirty, 0);
    }

    #[test]
    fn panning_or_zooming_invalidates_everything() {
        // The cached stages were rendered for a different rectangle of the
        // photograph, so none of them can be reused.
        let stack = stack_of(6);
        let mut c = warmed(&stack);
        let plan = c.plan(&stack, RenderContext { view: 99, ..CTX }, StackRow::is_noop);
        assert_eq!(plan.first_dirty, 0);
        assert_eq!(plan.execute.len(), 6);
    }

    #[test]
    fn changing_colour_management_invalidates_everything() {
        let stack = stack_of(6);
        let mut c = warmed(&stack);
        let plan = c.plan(&stack, RenderContext { color: 8, ..CTX }, StackRow::is_noop);
        assert_eq!(plan.first_dirty, 0);
    }

    #[test]
    fn truncating_the_stack_drops_orphaned_slots() {
        let mut stack = stack_of(10);
        let mut c = warmed(&stack);
        assert_eq!(c.len(), 10);

        for id in 5..10 {
            stack.remove(RowId(id));
        }
        c.plan(&stack, CTX, StackRow::is_noop);
        assert_eq!(c.len(), 5, "stale slots beyond the stack must be dropped");
    }

    #[test]
    fn fingerprints_are_stable_across_identical_rows() {
        let a = StackRow::new(RowId(1), "exposure");
        let b = StackRow::new(RowId(1), "exposure");
        assert_eq!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn fingerprints_distinguish_parameter_values() {
        let mut a = StackRow::new(RowId(1), "exposure");
        let mut b = StackRow::new(RowId(1), "exposure");
        a.params.set("ev", ParamValue::Float(0.5));
        b.params.set("ev", ParamValue::Float(0.6));
        assert_ne!(fingerprint(&a), fingerprint(&b));
    }

    #[test]
    fn a_drag_of_many_small_edits_never_grows_the_work() {
        // Simulates a pointer drag: 200 edits to the deepest row of a 12-row
        // stack should each cost exactly one row of rendering.
        let mut stack = stack_of(12);
        let mut c = warmed(&stack);
        for i in 0..200 {
            set_param(&mut stack, 11, i as f32 / 200.0);
            let plan = c.plan(&stack, CTX, StackRow::is_noop);
            assert_eq!(plan.execute, vec![11], "iteration {i}");
            c.store_plan(&stack, &plan);
        }
    }
}
