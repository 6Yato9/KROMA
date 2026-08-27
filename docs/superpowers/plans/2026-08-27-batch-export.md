# Batch Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export every photograph in the set, on macOS, with its own edit.

**Architecture:** Most of it is already shared. `pe_session::export` has `unclaimed_export_path`, `export_name`, `same_file` and `would_overwrite_a_source`; `Session` has `export_current`; and the library — added last — holds the set and each photograph's parked edit. What is Windows-only is the **stepping**: `main.rs`'s `batch_export`/`batch_step`, one photograph per frame so the window stays alive.

**Tech Stack:** Rust, the `pe-ffi` C ABI, Swift 6 / SwiftUI, XCTest.

**Predecessors:** the library and filmstrip. Batch export is what a set is *for*, and it was unreachable until there was one.

---

## Why it is stepped

Exporting sixty photographs is sixty full-resolution renders. Done in a loop the
window is frozen for a minute with no way to tell whether it is working or hung,
and no way to stop it. One photograph per frame keeps the interface alive, gives
somewhere to show progress, and makes cancelling a matter of not asking for the
next one.

That shape has to survive the port: `step()` does one photograph and says
whether there is more.

## What `batch_step` gets right, and must keep getting right

Read `apps/windows/src/main.rs:672-800` before writing anything. The details
that are easy to lose:

- **Targets are snapshotted when the run starts.** A photograph taken out of the
  set part way through is still on disc and still worth exporting.
- **Decoded per photograph, not held.** The whole reason a set is navigable is
  that only one frame is in memory at a time; a batch that loaded them all would
  undo that in the one place it matters most.
- **The image is loaded *before* the document is chosen**, because a photograph
  that has never been opened has no document yet and the file is the only thing
  that can say what colour space it is in.
- **Three places an edit can be**: the live history for the photograph in hand,
  a parked history for one that has been visited, and the sidecar — or nothing
  at all, which means the defaults.
- **A collision counts as a failure, not a stop.** One photograph that would
  land on somebody's original should not abandon the other sixty-five, and the
  summary at the end says how many did not make it.
- **`taken` runs across the whole batch**, so two sources with the same stem do
  not write over each other.

---

## Task 1: The run is the engine's

**Files:**
- Modify: `crates/pe-session/src/export.rs`, `crates/pe-session/src/session.rs`

- [x] **Step 1: The state**

```rust
/// A batch export in progress.
///
/// Stepped rather than looped: sixty photographs is sixty full-resolution
/// renders, and a loop freezes the window for a minute with no way to tell
/// whether it is working or hung, and no way to stop.
pub struct Batch {
    /// Snapshotted when the run started. A photograph taken out of the set
    /// part way through is still on disc and still worth exporting.
    targets: Vec<PathBuf>,
    next: usize,
    dir: PathBuf,
    done: usize,
    failed: usize,
    export: Export,
    /// Names already used by this run, so two sources with the same stem do
    /// not write over each other.
    taken: HashSet<PathBuf>,
}
```

and on `Session`:

```rust
    /// Begin exporting every photograph in the set into `dir`.
    pub fn start_batch(&mut self, dir: PathBuf) -> Result<(), SessionError>

    /// Export one photograph. `Ok(true)` while there is more to do.
    pub fn step_batch(&mut self) -> Result<bool, SessionError>

    /// How far it has got: done, failed, total. `None` when none is running.
    pub fn batch_progress(&self) -> Option<(usize, usize, usize)>

    /// Stop, keeping whatever has already been written.
    pub fn cancel_batch(&mut self)
```

- [x] **Step 2: Tests**

These are the reason this feature was chosen while the screen is unavailable:
every one of them is checkable on disc.

```rust
    #[test]
    fn a_batch_writes_one_file_per_photograph() { }

    /// The edit follows the photograph, whether it is the one in hand, one
    /// visited and parked, or one never opened with a sidecar beside it.
    #[test]
    fn each_photograph_is_exported_with_its_own_edit() { }

    /// Two sources called the same thing in different folders must not write
    /// over one another.
    #[test]
    fn two_photographs_with_the_same_name_get_different_files() { }

    /// One collision does not abandon the run.
    #[test]
    fn a_photograph_that_would_land_on_an_original_is_counted_and_skipped() { }

    #[test]
    fn a_photograph_that_will_not_decode_is_counted_and_skipped() { }

    /// Taken out of the set half way through, still exported.
    #[test]
    fn a_photograph_removed_mid_run_is_still_written() { }

    #[test]
    fn cancelling_keeps_what_was_already_written() { }

    #[test]
    fn a_batch_with_no_set_is_refused() { }
```

`each_photograph_is_exported_with_its_own_edit` is the one that matters — make
it fail if the document lookup falls back to the defaults, by giving the
photographs visibly different edits and reading the written pixels back.

- [x] **Step 3: Verify and commit**

Baseline **747 Rust passed, 0 failed, 1 ignored**. Report the real number.
Run `cargo check --workspace --all-targets --exclude pe-windows --target x86_64-unknown-linux-gnu` and `cargo clippy --workspace --all-targets`.

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add -A && git commit -m "A batch is the engine's to run, one photograph at a time"
```

---

## Task 2: The run crosses

**Files:**
- Modify: `crates/pe-ffi/src/lib.rs`

```rust
pe_session_start_batch(s, dir: *const c_char) -> i32
pe_session_step_batch(s) -> i32            // 1 more to do, 0 finished, negative on refusal
pe_session_batch_progress(s, out_done: *mut u32, out_failed: *mut u32,
                          out_total: *mut u32) -> i32   // -2 when none is running
pe_session_cancel_batch(s) -> i32
```

Follow the sentinel rule the library functions established: **`-1` means the
request never reached the session; `-2` means the session looked at it and
refused.** Test against files on disc, not only return codes.

---

## Task 3: Swift runs one

**Files:**
- Modify: `apps/apple/KromaKit/Engine.swift`, `SessionStore.swift`
- Modify: `apps/apple/PhotoEditor/PhotoEditorApp.swift` (a menu item and a folder panel)
- Create: `apps/apple/KromaKit/Controls/BatchProgress.swift`

- [x] **Step 1: Drive it**

`store.startBatch(_:)` and a step per frame from wherever the render loop
already ticks — the same place `renderIfNeeded` and `collectThumbnails` are
called. **Do not step in a `body`**: a full-resolution render inside a view
update is a frozen window with extra steps.

Progress as done/failed/total, and a cancel. When it finishes, say what
happened — `n exported`, or `n exported, m failed` — because a run that
silently stops is indistinguishable from one that crashed.

- [x] **Step 2: Tests**

Against a real engine and a temporary directory, the way `LibraryTests` makes
its fixtures: start a batch over three photographs, step it to completion, and
assert the files exist and the progress counted them. That is checkable without
a screen, which is the point.

---

## Task 4: Write it down

`apps/apple/README.md`: a batch is stepped, one photograph per frame, for the
reason above; the edit follows the photograph from whichever of the three places
it lives in; and a collision is counted rather than fatal.

---

## Verification

| check | command | expected |
|---|---|---|
| Rust | `cargo test --workspace --no-fail-fast` | 0 failed |
| Swift | `xcodebuild test -scheme KromaKitTests` | 0 failed |
| non-Apple | `cargo check --workspace --all-targets --exclude pe-windows --target x86_64-unknown-linux-gnu` | clean |
| Windows shell | `cargo clippy --workspace --all-targets` | clean |
| app | `xcodebuild build -scheme PhotoEditor` | BUILD SUCCEEDED, no warnings |

`each_photograph_is_exported_with_its_own_edit` is the test that matters. A
batch that exports sixty photographs with the wrong sixty edits is worse than
one that refuses to run, because the files look right until somebody opens them.
