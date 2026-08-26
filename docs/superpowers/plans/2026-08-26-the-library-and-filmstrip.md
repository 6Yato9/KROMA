# The Library and Filmstrip Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open a set of photographs on macOS, move between them from a filmstrip, and keep each one's edit while you do.

**Architecture:** `apps/windows/src/library.rs` already holds the whole model and is almost entirely portable — the only egui in it is that a thumbnail is a `TextureHandle` and `collect` takes a `Context`. It moves to `pe-session` with thumbnails as raw bytes, each shell uploading its own. `Library::switch` — park the outgoing edit, take the incoming one — is the piece both shells call, so the hard part exists once.

**Tech Stack:** Rust, the `pe-ffi` C ABI, Swift 6 / SwiftUI, XCTest.

**Predecessors:** the crop tool. This is the sixth time a Windows-only module has moved to a shared crate so the Mac could reach it; the pattern is established.

---

## What a library is, and what it is not

From `library.rs`'s own header: only one photograph is decoded at a time. A
24-megapixel frame is 96 MB of RGBA, so a folder of two hundred would be twenty
gigabytes — **the whole reason a filmstrip exists is to make a set navigable
without holding it.** What the library keeps per photo is a path, a few
kilobytes of edit, and a 128-pixel thumbnail.

The division of labour is the part to get right:

- the **library** owns the *edits* — a parked `History` per photograph, so
  switching away and back does not quietly throw away an undo stack;
- the **session** owns the *pixels* — one photograph, decoded;
- `Library::switch` moves the edit and the caller loads the pixels. It is
  already written and already right.

Thumbnails decode on a worker thread and arrive over a channel. The alternative
is decoding all of them when a folder opens, which for two hundred JPEGs is half
a minute of frozen window before anything can be done — and the first thing
anybody does is click the photograph they were looking for, which needs none of
the others.

## What has to change to move it

`Entry::thumb` is an `egui::TextureHandle` and `Library::collect` takes an
`egui::Context`. Both become raw RGBA bytes; each shell turns bytes into its own
texture. Everything else — paths, parked histories, the worker, the channel,
`scan`, `focus`, `add`, `remove`, `switch`, `take_current` — is already
platform-independent and moves unchanged.

---

## Task 1: The library moves to the engine

**Files:**
- Create: `crates/pe-session/src/library.rs`
- Modify: `crates/pe-session/src/lib.rs`, `apps/windows/src/library.rs`, `main.rs`

- [ ] **Step 1: Move it**

`apps/windows/src/library.rs` becomes `crates/pe-session/src/library.rs`,
**keeping every doc comment** — they carry the reasoning about memory, about why
one worker rather than a pool, and about why the whole `History` is parked
rather than just the document.

Two changes, and only two:

```rust
pub struct Entry {
    pub path: PathBuf,
    parked: Option<(History, RowIdGenerator)>,
    /// The thumbnail as RGBA bytes, `THUMB_EDGE` on its longest side.
    ///
    /// Bytes rather than a texture: a texture belongs to a graphics context
    /// and there are two shells with two of those. Each uploads its own.
    pub thumb: Option<Thumbnail>,
    requested: bool,
    pub failed: bool,
}

pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
```

and `collect(&mut self) -> bool`, with no `Context`.

The Windows shell keeps its own `Entry.texture: Option<TextureHandle>` cache
beside the library, filling it from `thumb` after `collect` returns true. That
is a few lines in `apps/windows/src/library.rs`, which becomes a thin adapter,
or in `filmstrip.rs` — put it wherever it reads better and say which.

- [ ] **Step 2: Tests it did not have**

It moves into a crate with tests, so give it some. The worker makes timing a
consideration — **do not sleep and hope.** Poll `collect` in a loop with a
deadline and fail with a clear message if nothing arrives, or make the worker
injectable so a test can run it synchronously; say which you chose.

```rust
    /// Switching away and back keeps the undo stack. The whole `History` is
    /// parked rather than just the document precisely so this holds.
    #[test]
    fn an_edit_survives_a_trip_to_another_photograph_and_back() { }

    /// A photograph never opened gets a fresh document — or the edit saved
    /// beside it, which is the whole point of writing one.
    #[test]
    fn a_photograph_never_opened_takes_the_edit_saved_beside_it() { }

    #[test]
    fn a_folder_scan_takes_only_the_extensions_the_dialogs_offer() { }

    /// Asking twice does not decode twice.
    #[test]
    fn a_thumbnail_is_only_requested_once() { }

    #[test]
    fn removing_the_current_photograph_leaves_a_sensible_one_current() { }

    #[test]
    fn removing_the_last_photograph_leaves_an_empty_library() { }
```

- [ ] **Step 3: Verify and commit**

Baseline **712 Rust passed, 0 failed, 1 ignored**. Report the real number.

Run the non-Apple check: `cargo check --workspace --all-targets --exclude pe-windows --target x86_64-unknown-linux-gnu`.

**The Windows shell must still build.** `cargo clippy --workspace --all-targets` covers it. It is 2,700 lines of `main.rs` driving this type; if the move needs more than the thumbnail adapter to keep it compiling, say what and why.

```bash
cd "/Volumes/Projects/Programming/photo editor" && git add -A && git commit -m "The set of photographs is the engine's to keep"
```

---

## Task 2: A session with more than one photograph

**Files:**
- Modify: `crates/pe-session/src/session.rs`

- [ ] **Step 1: The session holds a library**

`Session` currently holds one `Photo`. It gains a `Library`, and the switch
becomes one call rather than something each shell orchestrates:

```rust
    /// Open a set of photographs, focused on the first.
    pub fn open_paths(&mut self, paths: Vec<PathBuf>) -> Result<(), SessionError>

    /// Show a different photograph, parking the current edit and taking that
    /// one's.
    ///
    /// The edit is `Library::switch`'s business and the pixels are this
    /// function's. Both shells were orchestrating that pair by hand; one of
    /// them had to forget eventually.
    pub fn focus(&mut self, index: usize) -> Result<(), SessionError>

    pub fn library(&self) -> &Library
    pub fn request_thumbnails(&mut self, range: Range<usize>)
    /// Take delivery of whatever the worker finished. True if anything did.
    pub fn collect_thumbnails(&mut self) -> bool
```

`Session::open` (one path) stays and becomes `open_paths` with one element, so
nothing that exists today changes behaviour.

**The autosave must follow the focus.** Parking an edit and moving on without
writing it is how a crash loses the edit for every photograph but the last —
check what `autosave.rs` does on the single-photograph path and make the switch
honour it. Say what you found.

- [ ] **Step 2: Tests**

```rust
    #[test]
    fn a_session_opens_a_set_and_shows_the_first() { }

    #[test]
    fn focusing_another_photograph_swaps_the_pixels_and_the_edit() { }

    /// The edit that was parked is written, not merely remembered — a crash
    /// after switching should not lose it.
    #[test]
    fn switching_away_saves_the_edit_it_parked() { }

    #[test]
    fn focusing_past_the_end_is_refused() { }

    #[test]
    fn a_one_photograph_session_still_behaves_as_it_did() { }
```

- [ ] **Step 3: Verify and commit**

---

## Task 3: The library crosses

**Files:**
- Modify: `crates/pe-ffi/src/lib.rs`

- [ ] **Step 1: The surface**

Counts and scalars as typed calls; names as UTF-8 with a `pe_string_free`, as
the existing string functions do; thumbnails as a **buffer copy into memory the
caller owns**, exactly as the scopes do — read `pe_session_scope_data` and
follow it, including refusing a short buffer rather than truncating.

```rust
pe_session_open_paths(s, paths_json: *const c_char) -> i32
pe_session_focus(s, index: u32) -> i32
pe_session_entry_count(s) -> i32
pe_session_entry_path(s, index: u32) -> *mut c_char     // NULL out of range
pe_session_entry_flags(s, index: u32, out_edited: *mut bool,
                       out_failed: *mut bool, out_has_thumb: *mut bool) -> i32
pe_session_current_entry(s) -> i32
pe_session_request_thumbnails(s, from: u32, to: u32) -> i32
pe_session_collect_thumbnails(s) -> i32                 // 1 if anything arrived
pe_session_thumbnail_shape(s, index: u32, out_w: *mut u32, out_h: *mut u32) -> i32
pe_session_thumbnail_data(s, index: u32, out: *mut u8, capacity: u32) -> i32
```

`open_paths` takes JSON because a list of strings is a cold path and the ABI's
rule is that cold paths take JSON. A file name is not a scalar and there is no
count of them known in advance.

- [ ] **Step 2: Tests**, following `a_scope_crosses_as_a_buffer_the_caller_owns`
— assert on the document, not only on return codes.

- [ ] **Step 3: Verify and commit**

---

## Task 4: Swift holds the set

**Files:**
- Modify: `apps/apple/KromaKit/Engine.swift`, `SessionStore.swift`
- Create: `apps/apple/KromaKit/Library.swift`, `KromaKitTests/LibraryTests.swift`

- [ ] **Step 1: The model**

`LibraryEntry` — path, display name, edited, failed — and the current index.
Thumbnails are copied per entry and cached as `CGImage`, **rebuilt only when
`collectThumbnails` says something arrived**, the way `Scopes` uses its
generation. A 128-pixel thumbnail is 64 KB; two hundred of them is 13 MB, which
is worth not copying on every body evaluation.

`store.focus(_:)`, `store.openPaths(_:)`, `store.requestThumbnails(_:)`.

- [ ] **Step 2: Tests**, against a real engine and a temporary directory of
real images — write a few with `pe-io`'s own encoder from a Rust fixture step,
or the smallest valid PNGs you can construct in Swift. Say which.

- [ ] **Step 3: Verify and commit**

---

## Task 5: The filmstrip

**Files:**
- Create: `apps/apple/KromaKit/Controls/Filmstrip.swift`
- Modify: `apps/apple/PhotoEditor/ContentView.swift`

- [ ] **Step 1: Draw it**

**Down the left, not across the bottom.** `filmstrip.rs` says why: the window is
wider than it is tall and a photograph is not, so a horizontal strip costs
height — the dimension the picture wants.

A thumbnail per entry, the current one marked, a name under each, and a dot or
a mark on the ones that have been edited. Clicking focuses. Only visible
thumbnails are requested — that is what `request(range:)` is for, and asking for
all of them defeats the entire design.

Shown only when the set has more than one photograph, which is what
`main.rs:284` does.

- [ ] **Step 2: Test what a render can show**

Use the `ImageRenderer` approach the last four plans established and **prove
each test discriminates by breaking the thing it names.** Worth pinning: the
current entry is marked differently from the rest; an entry with no thumbnail
yet still occupies its place rather than collapsing; only the visible range is
requested.

- [ ] **Step 3: Verify and commit**

---

## Task 6: Write it down

`apps/apple/README.md`: the set of photographs is the engine's, one is decoded
at a time, thumbnails are bytes because a texture belongs to a graphics context
and there are two of those, and the filmstrip is vertical for the reason
`filmstrip.rs` gives.

Then the whole tree, and commit.

---

## Verification

| check | command | expected |
|---|---|---|
| Rust | `cargo test --workspace --no-fail-fast` | 0 failed |
| Swift | `xcodebuild test -scheme KromaKitTests` | 0 failed |
| non-Apple | `cargo check --workspace --all-targets --exclude pe-windows --target x86_64-unknown-linux-gnu` | clean |
| Windows shell | `cargo clippy --workspace --all-targets` | clean |
| format / lint | `cargo fmt --all --check`, `clippy -D warnings` | silent |
| app | `xcodebuild build -scheme PhotoEditor` | BUILD SUCCEEDED, no warnings |

`an_edit_survives_a_trip_to_another_photograph_and_back` is the test that
matters. Everything else here is navigation; that one is the promise that moving
between photographs does not cost you your work.
