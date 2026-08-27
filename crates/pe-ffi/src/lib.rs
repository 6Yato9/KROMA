//! C ABI surface, for Swift.
//!
//! The whole engine, as forty-odd functions. Everything the Mac and the iPad
//! do to a photograph goes through here, which is what makes the engine/UI
//! firewall real rather than aspirational: a shell cannot reach past it.
//!
//! Rules for anything added here:
//!
//! 1. Never expose a Rust type across the boundary — only opaque pointers,
//!    primitives, and UTF-8 C strings.
//! 2. Every allocation handed out has a matching `pe_*_free`.
//! 3. Never unwind across the boundary. Every entry point catches panics; a
//!    panic crossing into Swift is undefined behaviour, not a crash report.
//! 4. Hot paths are typed scalars; cold paths are JSON.
//! 5. Nothing calls back into Swift. Swift drives and Rust answers.
//!
//! The last two are spelled out where the session begins, below.

use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use pe_core::Document;

/// Opaque handle to a document.
pub struct PeDocument {
    inner: Document,
}

/// Run `f`, converting any panic into `fallback` rather than unwinding into
/// the caller's frame.
fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

/// Parse a document from a JSON C string.
///
/// Returns null on any failure. The caller owns the result and must release it
/// with [`pe_document_free`].
///
/// # Safety
/// `json` must be a valid, NUL-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_document_from_json(json: *const c_char) -> *mut PeDocument {
    guard(ptr::null_mut(), || {
        if json.is_null() {
            return ptr::null_mut();
        }
        let Ok(s) = (unsafe { CStr::from_ptr(json) }).to_str() else {
            return ptr::null_mut();
        };
        match Document::from_json(s) {
            Ok(inner) => Box::into_raw(Box::new(PeDocument { inner })),
            Err(_) => ptr::null_mut(),
        }
    })
}

/// Serialise a document to JSON. Caller must release with [`pe_string_free`].
///
/// # Safety
/// `doc` must be a pointer returned by [`pe_document_from_json`] and not yet
/// freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_document_to_json(doc: *const PeDocument) -> *mut c_char {
    guard(ptr::null_mut(), || {
        let Some(doc) = (unsafe { doc.as_ref() }) else {
            return ptr::null_mut();
        };
        match doc.inner.to_json().ok().and_then(|s| CString::new(s).ok()) {
            Some(c) => c.into_raw(),
            None => ptr::null_mut(),
        }
    })
}

/// Number of rows in the document's stack, or `-1` for a null handle.
///
/// # Safety
/// `doc` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_document_row_count(doc: *const PeDocument) -> i64 {
    guard(-1, || {
        (unsafe { doc.as_ref() }).map_or(-1, |d| d.inner.stack.len() as i64)
    })
}

/// Release a document.
///
/// # Safety
/// `doc` must have come from [`pe_document_from_json`] and must not be used
/// afterwards. Passing null is allowed and does nothing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_document_free(doc: *mut PeDocument) {
    if !doc.is_null() {
        drop(unsafe { Box::from_raw(doc) });
    }
}

/// Release a string returned by this library.
///
/// # Safety
/// `s` must have come from this library and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// Semantic version of the engine, as a static C string. Never freed.
#[unsafe(no_mangle)]
pub extern "C" fn pe_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

// ---------------------------------------------------------------------------
// The session.
//
// Two rules on top of the three above.
//
// 4. Hot paths are typed scalars; cold paths are JSON. A slider drag must not
//    allocate a string. Structure is rare and shape-heavy, so it goes as JSON
//    where adding a field does not mean adding a function.
//
// 5. Nothing calls back into Swift. Swift drives and Rust answers. A callback
//    from a Rust worker thread into a Swift closure is a reentrancy bug with a
//    deadline on it.
// ---------------------------------------------------------------------------

use pe_session::Session;

/// Opaque handle to a session, plus the last thing that went wrong.
///
/// The message is kept here rather than returned, because every fallible entry
/// point returns `i32` and a status code with no text is a bug report nobody
/// can write.
pub struct PeSession {
    inner: Session,
    last_error: Option<String>,
}

/// What a panic was about, as far as it can be recovered.
///
/// Almost every panic payload is a `&str` or a `String`; anything else is a
/// deliberate `panic_any` and there is nothing useful to say about it.
fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "the engine panicked".to_string()
    }
}

/// Run `f` against a session, or return `fallback` for a null handle.
///
/// `Copy` because the fallback is wanted in two places — the null branch and
/// the panic branch — and every sentinel here is an integer, a `bool`, or a
/// null pointer.
///
/// The panic is caught here rather than in [`guard`] because here there is a
/// session to write the reason onto. Rule 3 stops the unwind; without this it
/// would also stop the explanation, and a caught panic reporting only a status
/// code is exactly the bug report nobody can write. A null handle is the one
/// case with nowhere to record anything — there is no session — so it is also
/// the one case where the sentinel arrives with `pe_session_last_error` unset,
/// which is how the two are told apart.
fn with<T: Copy>(s: *mut PeSession, fallback: T, f: impl FnOnce(&mut PeSession) -> T) -> T {
    let Some(session) = (unsafe { s.as_mut() }) else {
        return fallback;
    };
    match catch_unwind(AssertUnwindSafe(|| f(&mut *session))) {
        Ok(value) => value,
        Err(payload) => {
            session.last_error = Some(panic_text(payload));
            fallback
        }
    }
}

/// The same, for a call that returns a status code and may set an error.
fn status(
    s: *mut PeSession,
    f: impl FnOnce(&mut Session) -> Result<(), pe_session::SessionError>,
) -> i32 {
    with(s, -1, |s| match f(&mut s.inner) {
        Ok(()) => {
            s.last_error = None;
            0
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            -2
        }
    })
}

/// Borrow a C string as UTF-8, or `None` for a null or malformed one.
fn as_str<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}

/// Hand a string out. The caller releases it with [`pe_string_free`].
fn to_c(s: String) -> *mut c_char {
    CString::new(s)
        .map(|c| c.into_raw())
        .unwrap_or(ptr::null_mut())
}

/// Open a session. Caller must release it with [`pe_session_free`].
#[unsafe(no_mangle)]
pub extern "C" fn pe_session_new() -> *mut PeSession {
    guard(ptr::null_mut(), || {
        Box::into_raw(Box::new(PeSession {
            inner: Session::new(),
            last_error: None,
        }))
    })
}

/// Release a session.
///
/// Guarded, unlike the other frees: dropping a session tears down a wgpu
/// device, a surface and every pipeline hung off them, and that is the one
/// destructor here with enough machinery in it to panic. Rule 3 does not stop
/// applying because the call happens to be a free.
///
/// # Safety
/// `s` must have come from [`pe_session_new`] and must not be used afterwards.
/// Passing null is allowed and does nothing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_free(s: *mut PeSession) {
    guard((), || {
        if !s.is_null() {
            drop(unsafe { Box::from_raw(s) });
        }
    })
}

/// The last failure's message, or null if the last call succeeded.
/// Caller must release with [`pe_string_free`].
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_last_error(s: *mut PeSession) -> *mut c_char {
    with(s, ptr::null_mut(), |s| match s.last_error.clone() {
        Some(m) => to_c(m),
        None => ptr::null_mut(),
    })
}

/// # Safety
/// `s` and `path` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_support_dir(s: *mut PeSession, path: *const c_char) -> i32 {
    let Some(path) = as_str(path) else { return -1 };
    let path = path.to_string();
    status(s, move |s| {
        s.set_support_dir(path);
        Ok(())
    })
}

// ---- opening --------------------------------------------------------------

/// # Safety
/// `s` and `path` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_open_path(s: *mut PeSession, path: *const c_char) -> i32 {
    let Some(path) = as_str(path) else { return -1 };
    let path = path.to_string();
    status(s, move |s| s.open_path(path))
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_open_test_chart(
    s: *mut PeSession,
    width: u32,
    height: u32,
) -> i32 {
    status(s, move |s| s.open_test_chart(width, height))
}

/// Rows in the open document, or `-1` for a null handle.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_row_count(s: *mut PeSession) -> i64 {
    with(s, -1, |s| s.inner.row_count() as i64)
}

// ---- the set --------------------------------------------------------------
//
// The photographs a session has open, for the filmstrip. Only one of them is
// decoded — a 24-megapixel frame is 96 MB of RGBA, so a folder of two hundred
// would be twenty gigabytes — and the whole point of a strip is to make a set
// navigable without holding it. What crosses here is paths, three flags and a
// 128-pixel thumbnail. Never a frame.
//
// [`Session::library`] answers `None` until a set is opened and again for the
// built-in chart, which is not a file and therefore not a set of one. Every
// function below has an answer for that: the counts say zero, the readers say
// `-2` or null, and asking for thumbnails of nothing is a no-op rather than a
// failure. None of those is `-1`, which stays a null handle throughout.

/// Open a set of photographs, focused on the first. `paths_json` is a JSON
/// array of file paths — `["/a.jpg","/b.jpg"]`.
///
/// JSON because rule 4 puts cold paths there: a file name is not a scalar, and
/// there is no count of them known in advance, so the typed alternative is a
/// pointer-and-length pair of a type this ABI is not allowed to name.
///
/// Returns 0; `-1` for a null handle, for a null or non-UTF-8 `paths_json`, or
/// for JSON that is not an array of strings; `-2` if the session refused, with
/// the reason in [`pe_session_last_error`].
///
/// **An empty array is `-2`, not 0.** [`Session::open_paths`] refuses it, so
/// that no reader of a set ever has to cope with a set of no photographs, and
/// that judgement is passed through rather than re-made here. A malformed list
/// is `-1` and leaves the session exactly as it was — and, when there is a
/// session to write it on, says what was wrong in [`pe_session_last_error`].
///
/// # Safety
/// `s` and `paths_json` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_open_paths(
    s: *mut PeSession,
    paths_json: *const c_char,
) -> i32 {
    let Some(json) = as_str(paths_json) else {
        return -1;
    };
    let Ok(paths) = serde_json::from_str::<Vec<String>>(json) else {
        return with(s, -1, |s| {
            s.last_error = Some("open_paths wants a JSON array of paths".to_string());
            -1
        });
    };
    status(s, move |s| {
        s.open_paths(paths.into_iter().map(std::path::PathBuf::from).collect())
    })
}

/// Show a different photograph of the set, parking the current edit and taking
/// that one's.
///
/// Returns 0; `-1` for a null handle; `-2` with no set open, for an index past
/// the end, or for a photograph that will not decode. The reason is in
/// [`pe_session_last_error`], and in every one of those cases nothing has
/// moved: the set is still pointed where it was and the edit on screen is
/// still the one that was there.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_focus(s: *mut PeSession, index: u32) -> i32 {
    status(s, move |s| s.focus(index as usize))
}

/// How many photographs are in the set.
///
/// `-1` for a null handle; **0 with no set open**, which is the truth rather
/// than a failure — a session showing nothing, or showing the built-in chart,
/// has no photographs in it, and a strip of zero entries is exactly the right
/// thing to draw.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_entry_count(s: *mut PeSession) -> i32 {
    with(s, -1, |s| s.inner.library().map_or(0, |l| l.len() as i32))
}

/// The path of one photograph in the set. Caller must release it with
/// [`pe_string_free`].
///
/// Null for a null handle, with no set open, and for an index past the end.
/// The three are not told apart because there is nothing a strip would do
/// differently: an entry it cannot have is an entry it cannot draw.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_entry_path(s: *mut PeSession, index: u32) -> *mut c_char {
    with(s, ptr::null_mut(), |s| {
        match s.inner.library().and_then(|l| l.path(index as usize)) {
            Some(p) => to_c(p.display().to_string()),
            None => ptr::null_mut(),
        }
    })
}

/// The three marks a filmstrip draws on one entry: whether its edit has
/// anything in it to undo, whether its decode failed, and whether its
/// thumbnail has arrived.
///
/// Three bools in one call rather than three calls, because a strip asks all
/// three of every visible entry on every frame it draws.
///
/// Any out-pointer may be null. Returns 0; `-1` for a null handle; `-2` with
/// no set open or for an index past the end, in which case nothing is written.
///
/// # Safety
/// `s` must be valid or null; each non-null out-pointer must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_entry_flags(
    s: *mut PeSession,
    index: u32,
    out_edited: *mut bool,
    out_failed: *mut bool,
    out_has_thumb: *mut bool,
) -> i32 {
    with(s, -1, |s| {
        let Some(entry) = s
            .inner
            .library()
            .and_then(|l| l.entries().get(index as usize))
        else {
            return -2;
        };
        unsafe {
            if !out_edited.is_null() {
                out_edited.write(entry.edited());
            }
            if !out_failed.is_null() {
                out_failed.write(entry.failed);
            }
            if !out_has_thumb.is_null() {
                out_has_thumb.write(entry.thumb.is_some());
            }
        }
        0
    })
}

/// Which photograph of the set is the one on screen.
///
/// `-1` for a null handle; `-2` with no set open, because there is no index to
/// give and 0 would name an entry that does not exist.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_current_entry(s: *mut PeSession) -> i32 {
    with(s, -1, |s| {
        s.inner.library().map_or(-2, |l| l.current() as i32)
    })
}

/// Ask for the thumbnails of `from..to` that have not been asked for yet.
///
/// The range the strip is actually showing, not the whole set: opening a
/// folder of a thousand should not queue a thousand decodes before the first
/// one anybody can see. The decode happens on a worker thread and the pixels
/// arrive later, through [`pe_session_collect_thumbnails`]. Asking twice for
/// the same entry costs nothing; the second ask is dropped.
///
/// Indices past the end are ignored, and a `from` at or past `to` asks for
/// nothing. Returns 0, or `-1` for a null handle.
///
/// **With no set open this is a no-op that returns 0**, which is what
/// [`Session::request_thumbnails`] does: a session with no photographs answers
/// "give me your thumbnails" by having none, not by failing.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_request_thumbnails(
    s: *mut PeSession,
    from: u32,
    to: u32,
) -> i32 {
    with(s, -1, |s| {
        s.inner.request_thumbnails(from as usize..to as usize);
        0
    })
}

/// Take delivery of whatever the thumbnail worker has finished.
///
/// 1 if anything arrived — so the shell knows to upload it and repaint — 0 if
/// nothing did, `-1` for a null handle. With no set open nothing can arrive,
/// so 0.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_collect_thumbnails(s: *mut PeSession) -> i32 {
    with(s, -1, |s| i32::from(s.inner.collect_thumbnails()))
}

/// How big one entry's thumbnail is, in pixels.
///
/// RGBA, eight bits a channel, rows top to bottom, so the buffer
/// [`pe_session_thumbnail_data`] wants is `width * height * 4` bytes. The long
/// edge is [`pe_session::library::THUMB_EDGE`]; the short one follows the
/// photograph's proportions, so neither is worth assuming.
///
/// Either out-pointer may be null. Returns 0; `-1` for a null handle; `-2`
/// with no set open, for an index past the end, or for a thumbnail that has
/// not arrived yet — ask [`pe_session_request_thumbnails`] and then
/// [`pe_session_collect_thumbnails`] first. Nothing is written on a failure.
///
/// # Safety
/// `s` must be valid or null; each non-null out-pointer must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_thumbnail_shape(
    s: *mut PeSession,
    index: u32,
    out_w: *mut u32,
    out_h: *mut u32,
) -> i32 {
    with(s, -1, |s| {
        let Some(thumb) = thumbnail(s, index) else {
            return -2;
        };
        unsafe {
            if !out_w.is_null() {
                out_w.write(thumb.width);
            }
            if !out_h.is_null() {
                out_h.write(thumb.height);
            }
        }
        0
    })
}

/// Copy a thumbnail's RGBA bytes into `out`, returning how many were written,
/// or a negative number: `-1` for a null handle, a null `out`, no set open, an
/// index past the end, or a thumbnail that has not arrived; `-2` if `capacity`
/// is short of the `width * height * 4` [`pe_session_thumbnail_shape`]
/// reported.
///
/// Short is refused rather than truncated, for the same reason a scope's is:
/// 64 KB of pixels with the last rows missing is a plausible-looking
/// photograph that does not exist, and a strip full of them looks like a
/// decoder bug rather than a caller's arithmetic.
///
/// The buffer is the caller's, before and after — nothing is allocated here,
/// so rule 2 stays trivially satisfied and there is no `pe_*_free` to call.
///
/// # Safety
/// `s` must be valid or null. `out` must point to at least `capacity` writable
/// bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_thumbnail_data(
    s: *mut PeSession,
    index: u32,
    out: *mut u8,
    capacity: u32,
) -> i32 {
    with(s, -1, |s| {
        if out.is_null() {
            return -1;
        }
        let Some(thumb) = thumbnail(s, index) else {
            return -1;
        };
        let wanted = thumb.rgba.len();
        if (capacity as usize) < wanted {
            return -2;
        }
        unsafe { ptr::copy_nonoverlapping(thumb.rgba.as_ptr(), out, wanted) };
        wanted as i32
    })
}

/// One entry's thumbnail, if there is a set, the index is in it, and the
/// worker has delivered. The single `None` the two thumbnail functions above
/// both answer to, so that they cannot come to disagree about which entries
/// have pixels.
fn thumbnail(s: &PeSession, index: u32) -> Option<&pe_session::Thumbnail> {
    s.inner
        .library()?
        .entries()
        .get(index as usize)?
        .thumb
        .as_ref()
}

// ---- the screen -----------------------------------------------------------

/// Adopt a `CAMetalLayer`.
///
/// # Safety
/// `layer` must be a live `CAMetalLayer` that outlives the attachment, and
/// [`pe_session_detach_layer`] must be called before it goes away.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_attach_layer(
    s: *mut PeSession,
    layer: *mut std::ffi::c_void,
    width: u32,
    height: u32,
) -> i32 {
    status(s, move |s| unsafe { s.attach_layer(layer, width, height) })
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_resize(s: *mut PeSession, width: u32, height: u32) -> i32 {
    status(s, move |s| {
        s.resize_layer(width, height);
        Ok(())
    })
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_detach_layer(s: *mut PeSession) -> i32 {
    status(s, |s| {
        s.detach_layer();
        Ok(())
    })
}

/// Draw the current state into the attached layer and present it.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_render(s: *mut PeSession) -> i32 {
    status(s, |s| s.present())
}

/// Show this rectangle of the frame. `size` is the fraction of the whole
/// picture that is visible, so 1.0 is fitted and 0.25 is four times in.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_view(s: *mut PeSession, x: f32, y: f32, size: f32) -> i32 {
    status(s, move |s| {
        s.set_view(x, y, size);
        Ok(())
    })
}

/// Whether anything has changed since the last present. The display link asks
/// this before doing any work; an editor that redraws 120 times a second while
/// nothing moves is a laptop with a warm keyboard.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_needs_render(s: *mut PeSession) -> bool {
    with(s, false, |s| s.inner.needs_render())
}

/// Passes the last frame executed. See `Snapshot::passes`.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_last_passes(s: *mut PeSession) -> i64 {
    with(s, -1, |s| s.inner.last_passes() as i64)
}

/// Drive the autosave debounce. Called from the display link.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_tick(s: *mut PeSession) -> i32 {
    status(s, |s| s.tick())
}

// ---- the document ---------------------------------------------------------

/// The whole UI-visible state. Caller must release with [`pe_string_free`].
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_snapshot_json(s: *mut PeSession) -> *mut c_char {
    with(s, ptr::null_mut(), |s| {
        match serde_json::to_string(&pe_session::describe::snapshot(&s.inner)) {
            Ok(j) => to_c(j),
            Err(_) => ptr::null_mut(),
        }
    })
}

/// Bumped by every mutation. Compare before decoding the snapshot.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_snapshot_version(s: *mut PeSession) -> u64 {
    with(s, 0, |s| s.inner.snapshot_version())
}

/// Returns the new row's id, or negative on failure.
///
/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_add_effect(s: *mut PeSession, key: *const c_char) -> i64 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    with(s, -1, |s| match s.inner.add_effect(&key) {
        Ok(id) => {
            s.last_error = None;
            id.0 as i64
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            -2
        }
    })
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_remove_row(s: *mut PeSession, row: u64) -> i32 {
    status(s, move |s| s.remove_row(pe_core::RowId(row)))
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_move_row(s: *mut PeSession, row: u64, to: u32) -> i32 {
    status(s, move |s| s.move_row(pe_core::RowId(row), to as usize))
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_row_enabled(s: *mut PeSession, row: u64, on: bool) -> i32 {
    status(s, move |s| s.set_row_enabled(pe_core::RowId(row), on))
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_row_opacity(
    s: *mut PeSession,
    row: u64,
    value: f32,
) -> i32 {
    status(s, move |s| s.set_row_opacity(pe_core::RowId(row), value))
}

// ---- parameters, the hot path ---------------------------------------------

/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_float(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    value: f32,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| s.set_float(pe_core::RowId(row), &key, value))
}

/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_bool(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    value: bool,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| s.set_bool(pe_core::RowId(row), &key, value))
}

/// # Safety
/// `s`, `key` and `value` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_choice(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    value: *const c_char,
) -> i32 {
    let (Some(key), Some(value)) = (as_str(key), as_str(value)) else {
        return -1;
    };
    let (key, value) = (key.to_string(), value.to_string());
    status(s, move |s| s.set_choice(pe_core::RowId(row), &key, &value))
}

/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_rgb(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    r: f32,
    g: f32,
    b: f32,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| s.set_rgb(pe_core::RowId(row), &key, [r, g, b]))
}

/// A four-way colour wheel: three channels and the ring around the outside.
///
/// Seven arguments rather than a struct, because rule 1 says only primitives
/// cross this boundary and a wheel is four floats however it is packaged.
///
/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_wheel(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    master: f32,
    r: f32,
    g: f32,
    b: f32,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| {
        s.set_wheel(pe_core::RowId(row), &key, master, [r, g, b])
    })
}

/// Replace a curve with `count` control points, as `2 * count` floats — x, y,
/// x, y. A flat array rather than JSON because this is a drag path: a curve
/// being dragged sends its points on every frame, and a parse per frame to
/// carry twenty numbers is work nobody needs done.
///
/// # Safety
/// `s` and `key` must be valid or null. `xy` must point to at least
/// `2 * count` readable floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_curve(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    xy: *const f32,
    count: u32,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    if xy.is_null() {
        return -1;
    }
    let key = key.to_string();
    // Copied out before the closure, because `status` may catch a panic and
    // the borrowed slice must not outlive the call the caller made.
    let flat = unsafe { std::slice::from_raw_parts(xy, count as usize * 2) };
    let points: Vec<[f32; 2]> = flat.as_chunks::<2>().0.to_vec();
    status(s, move |s| s.set_curve(pe_core::RowId(row), &key, &points))
}

/// Move one vertex of a lattice. `dx` and `dy` are a displacement in axis
/// units, not a position.
///
/// Typed scalars rather than JSON because this is a drag path: a vertex being
/// dragged sends its offset on every frame.
///
/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_warp_vertex(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    col: u32,
    vertex_row: u32,
    dx: f32,
    dy: f32,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| {
        s.set_warp_vertex(pe_core::RowId(row), &key, col, vertex_row, [dx, dy])
    })
}

/// Put a lattice back to identity, keeping its grid size.
///
/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_clear_warp(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| s.clear_warp(pe_core::RowId(row), &key))
}

/// Place a pin at a chromaticity, returning its index, or a negative number on
/// failure — `-1` for a bad argument, `-2` for a refusal whose reason is in
/// [`pe_session_last_error`].
///
/// The odd one out: this call answers with an index rather than a status, so
/// it cannot use the `status` helper, and failure has to arrive in the same
/// integer as the answer. A negative index is the sentinel, and the reason is
/// recorded exactly where `status` would have put it.
///
/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_add_pin(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    x: f32,
    y: f32,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    with(s, -1, move |s| {
        match s.inner.add_pin(pe_core::RowId(row), &key, [x, y]) {
            Ok(i) => {
                s.last_error = None;
                i as i32
            }
            Err(e) => {
                s.last_error = Some(e.to_string());
                -2
            }
        }
    })
}

/// Drag a pin. Only `to` moves — `at` is where the colour is.
///
/// Typed scalars rather than JSON because this is a drag path: a pin being
/// dragged sends its chromaticity on every frame.
///
/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_move_pin(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    index: u32,
    x: f32,
    y: f32,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| {
        s.move_pin(pe_core::RowId(row), &key, index as usize, [x, y])
    })
}

/// The five controls that shape a pin, set together.
///
/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pe_session_set_pin_shape(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    index: u32,
    chroma_range: f32,
    tonal_low: f32,
    tonal_high: f32,
    tonal_pivot: f32,
    exposure: f32,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| {
        s.set_pin_shape(
            pe_core::RowId(row),
            &key,
            index as usize,
            chroma_range,
            tonal_low,
            tonal_high,
            tonal_pivot,
            exposure,
        )
    })
}

/// Take a pin away.
///
/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_remove_pin(
    s: *mut PeSession,
    row: u64,
    key: *const c_char,
    index: u32,
) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| {
        s.remove_pin(pe_core::RowId(row), &key, index as usize)
    })
}

// ---- geometry -------------------------------------------------------------

/// The value of `aspect` that asks for the source photograph's own
/// proportions.
///
/// [`pe_core::AspectLock`] has three arms and `aspect` is one float, because
/// the alternative on a drag path is an enum across the ABI plus a second
/// parameter to carry its payload. So: a positive number is a fixed
/// width-to-height ratio, this one value is the source's own proportions, and
/// every other value at or below zero is free. It is negative precisely
/// because no ratio computed from two positive edges can collide with it, and
/// it is named here — and in the generated header — rather than left as a bare
/// `-1.0` in somebody's comment.
pub const PE_ASPECT_ORIGINAL: f32 = -1.0;

/// Read the `aspect` parameter. See [`PE_ASPECT_ORIGINAL`].
///
/// Infinity and NaN fall through to free rather than becoming a ratio no crop
/// can hold.
fn aspect_lock(aspect: f32) -> pe_core::AspectLock {
    if aspect == PE_ASPECT_ORIGINAL {
        pe_core::AspectLock::Original
    } else if aspect > 0.0 && aspect.is_finite() {
        pe_core::AspectLock::Ratio { w: aspect, h: 1.0 }
    } else {
        pe_core::AspectLock::Free
    }
}

/// Write the `aspect` parameter back.
///
/// A ratio crosses as the single number `w / h`, so a lock a document spells
/// `16:9` reads back here as `1.777…`. That is all the crop arithmetic ever
/// wanted; the snapshot carries `aspect_w` and `aspect_h` separately for
/// anything that needs to *print* the lock rather than apply it.
fn aspect_value(lock: pe_core::AspectLock) -> f32 {
    match lock {
        pe_core::AspectLock::Free => 0.0,
        pe_core::AspectLock::Original => PE_ASPECT_ORIGINAL,
        // The same guard `AspectLock::ratio` uses, so a malformed lock on a
        // document from disk crosses as a finite number rather than infinity —
        // which would come back in as free and quietly drop the lock.
        pe_core::AspectLock::Ratio { w, h } => w / h.max(1e-6),
    }
}

/// Set the crop, straighten and flips, and write back what was actually
/// stored.
///
/// **The values written back are frequently not the ones passed in, and that
/// is the point of this function.** The engine corrects what it is given:
/// quarter-turns are taken modulo four, a locked aspect re-shapes the crop,
/// and the crop is then slid — and, if it still will not fit anywhere,
/// shrunk — back inside the straightened source. What comes back is what the
/// document now holds, so no shell ever needs a second copy of `apply_aspect`,
/// `slide_to_fit` and `shrink_to_fit` to keep honest. A caller that ignores
/// the out-parameters and goes on drawing what it asked for is drawing a
/// rectangle the renderer does not produce, and that rectangle will jump to
/// the real one the moment the drag ends and the snapshot is read again.
///
/// `cx` and `cy` are the crop's centre as an offset from the middle of the
/// source, in units of the source's own width and height; `w` and `h` are its
/// size as a fraction of the source; `angle` is degrees, positive
/// anticlockwise; `turns` is quarter-turns clockwise. `aspect` is a positive
/// width-to-height ratio, [`PE_ASPECT_ORIGINAL`] for the source's own
/// proportions, or anything else at or below zero for free.
///
/// Every out-pointer may be null; pass nulls when only the status is wanted.
/// The two flips have no out-parameter because nothing corrects them — they
/// are stored exactly as given.
///
/// Nine in, seven out, all primitives: this is a drag path, and a JSON parse
/// per frame to carry seven numbers is work nobody needs done.
///
/// Returns 0, `-1` for a null handle, or `-2` with nothing open.
///
/// # Safety
/// `s` must be valid or null; each non-null out-pointer must be writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pe_session_set_geometry(
    s: *mut PeSession,
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
    angle: f32,
    turns: u32,
    flip_h: bool,
    flip_v: bool,
    aspect: f32,
    out_cx: *mut f32,
    out_cy: *mut f32,
    out_w: *mut f32,
    out_h: *mut f32,
    out_angle: *mut f32,
    out_turns: *mut u32,
    out_aspect: *mut f32,
) -> i32 {
    let want = pe_core::Geometry {
        centre: [cx, cy],
        size: [w, h],
        angle,
        // 256 is a multiple of four, so narrowing first would give the same
        // answer; taking the remainder first says why.
        turns: (turns % 4) as u8,
        flip_h,
        flip_v,
        aspect: aspect_lock(aspect),
    };
    with(s, -1, move |s| match s.inner.set_geometry(want) {
        Ok(g) => {
            s.last_error = None;
            unsafe {
                if !out_cx.is_null() {
                    out_cx.write(g.centre[0]);
                }
                if !out_cy.is_null() {
                    out_cy.write(g.centre[1]);
                }
                if !out_w.is_null() {
                    out_w.write(g.size[0]);
                }
                if !out_h.is_null() {
                    out_h.write(g.size[1]);
                }
                if !out_angle.is_null() {
                    out_angle.write(g.angle);
                }
                if !out_turns.is_null() {
                    out_turns.write(g.turns as u32);
                }
                if !out_aspect.is_null() {
                    out_aspect.write(aspect_value(g.aspect));
                }
            }
            0
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            -2
        }
    })
}

/// Put the crop, straighten and flips back to the whole frame.
///
/// [`pe_session_set_geometry`] with the default would do the same thing, but
/// "back to the original" is something a user asks for directly, and a shell
/// should not have to spell out nine arguments — seven of them zero — to say
/// it. Nothing is written back because there is nothing to correct: the answer
/// is always the whole frame.
///
/// Returns 0, `-1` for a null handle, or `-2` with nothing open.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_reset_geometry(s: *mut PeSession) -> i32 {
    status(s, |s| {
        s.set_geometry(pe_core::Geometry::default()).map(|_| ())
    })
}

/// Show the whole straightened source in the viewer rather than the crop.
///
/// While the crop tool is open the viewer has to show what is being cut away,
/// or there is nothing to see outside the rectangle and nothing to drag back
/// into. A flag rather than a frame: the frame is `Geometry::enclosing` and the
/// engine computes it, so no shell has to know how — and so the two shells
/// cannot disagree about what "the whole straightened source" means.
///
/// Not an edit. It is not in the history, the document is untouched, and an
/// export renders the document either way.
///
/// Returns 0, or `-1` for a null handle. Nothing needs to be open: a flag about
/// the window outlives whichever photograph is in it.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_cropping(s: *mut PeSession, cropping: bool) -> i32 {
    status(s, move |s| {
        s.set_cropping(cropping);
        Ok(())
    })
}

/// Where the crop sits inside the frame the viewer is showing, as `u0`, `v0`,
/// `u1`, `v1` — min x, min y, max x, max y in that frame's uv.
///
/// This is `Geometry::crop_uv_in` against the frame
/// [`pe_session_set_cropping`] selects, and it exists so no shell has to hold a
/// second copy of it. With the crop tool closed the crop *is* the frame and the
/// answer is the whole of it; with the tool open it is the rectangle the
/// overlay draws.
///
/// Every out-pointer may be null. Returns 0, `-1` for a null handle, or `-2`
/// with nothing open.
///
/// # Safety
/// `s` must be valid or null; each non-null out-pointer must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_crop_in_frame(
    s: *mut PeSession,
    out_u0: *mut f32,
    out_v0: *mut f32,
    out_u1: *mut f32,
    out_v1: *mut f32,
) -> i32 {
    with(s, -1, move |s| match s.inner.crop_in_frame() {
        Ok(rect) => {
            s.last_error = None;
            unsafe { write_rect(rect, out_u0, out_v0, out_u1, out_v1) };
            0
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            -2
        }
    })
}

/// Move the crop to a rectangle of the frame being shown, and write back where
/// it actually landed.
///
/// **The values written back are frequently not the ones passed in, and that is
/// the point of this function** — the same contract [`pe_session_set_geometry`]
/// has, and the same corrections: a locked aspect re-shapes the crop, and it is
/// slid, then shrunk, back inside the straightened source. A caller that
/// ignores the out-parameters and goes on drawing what it asked for is drawing a
/// rectangle the renderer does not produce.
///
/// The rectangle goes in and comes back in the frame [`pe_session_crop_in_frame`]
/// reads, so a drag is: read once, then propose and draw the answer.
///
/// Every out-pointer may be null. Returns 0, `-1` for a null handle, or `-2`
/// with nothing open.
///
/// # Safety
/// `s` must be valid or null; each non-null out-pointer must be writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pe_session_set_crop_in_frame(
    s: *mut PeSession,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    out_u0: *mut f32,
    out_v0: *mut f32,
    out_u1: *mut f32,
    out_v1: *mut f32,
) -> i32 {
    with(s, -1, move |s| {
        match s.inner.set_crop_in_frame([u0, v0, u1, v1]) {
            Ok(rect) => {
                s.last_error = None;
                unsafe { write_rect(rect, out_u0, out_v0, out_u1, out_v1) };
                0
            }
            Err(e) => {
                s.last_error = Some(e.to_string());
                -2
            }
        }
    })
}

/// The four out-parameters the two crop-in-frame calls share.
///
/// # Safety
/// Each non-null pointer must be writable.
unsafe fn write_rect(
    rect: [f32; 4],
    out_u0: *mut f32,
    out_v0: *mut f32,
    out_u1: *mut f32,
    out_v1: *mut f32,
) {
    for (out, value) in [out_u0, out_v0, out_u1, out_v1].into_iter().zip(rect) {
        if !out.is_null() {
            unsafe { out.write(value) };
        }
    }
}

// ---- comparing ------------------------------------------------------------

/// No comparison: the graded picture, whole.
///
/// The three `mode` values [`pe_session_set_compare`] and
/// [`pe_session_compare`] cross, named here — and in the generated header —
/// rather than left as bare integers in somebody's comment. The numbering is
/// part of the ABI: add to the end.
pub const PE_COMPARE_OFF: u32 = 0;
/// One picture with a seam: ungraded to the left of it, graded to the right.
pub const PE_COMPARE_WIPE: u32 = 1;
/// Two half-size pictures with a real gap: ungraded left, graded right.
pub const PE_COMPARE_SIDE: u32 = 2;

/// The comparison an integer names, or `None` for one this ABI has no mode
/// for. The `None` is the whole reason this is a function and not a cast.
fn compare_mode(mode: u32) -> Option<pe_session::Compare> {
    match mode {
        PE_COMPARE_OFF => Some(pe_session::Compare::Off),
        PE_COMPARE_WIPE => Some(pe_session::Compare::Wipe),
        PE_COMPARE_SIDE => Some(pe_session::Compare::Side),
        _ => None,
    }
}

/// And back again, for [`pe_session_compare`].
fn compare_value(mode: pe_session::Compare) -> u32 {
    match mode {
        pe_session::Compare::Off => PE_COMPARE_OFF,
        pe_session::Compare::Wipe => PE_COMPARE_WIPE,
        pe_session::Compare::Side => PE_COMPARE_SIDE,
    }
}

/// Hold the graded picture up against the ungraded one, or stop.
///
/// `mode` is **0 off, 1 wipe, 2 side** — [`PE_COMPARE_OFF`],
/// [`PE_COMPARE_WIPE`], [`PE_COMPARE_SIDE`]. The engine composites the two
/// pictures itself, because it owns the textures; the seam and the labels are
/// the shell's to draw over the top.
///
/// **A value that is not one of the three is refused, not quietly treated as
/// off.** A shell that grew a fourth way of comparing and sends `3` gets `-2`
/// and a message saying so; showing no comparison instead would be
/// indistinguishable, from the shell's side, from the feature being broken —
/// and the one thing that could tell it otherwise is this return value.
/// Nothing moves on a refusal: the mode and the seam are still where they
/// were, and the next frame is the one that was already being drawn.
///
/// `wipe` is where the seam sits, as a fraction of the frame's width from the
/// left, and only [`PE_COMPARE_WIPE`] draws it. Out of range it is clamped
/// rather than refused: 0.0 and 1.0 are places a user drags to, and past
/// either end is what dragging against the edge of a window produces.
///
/// **The fraction is kept whatever the mode is**, so that cycling
/// off → wipe → side → off with one button puts the seam back where the user
/// left it rather than at the left edge. It is also the one thing a caller can
/// throw away here — pass 0 while turning a comparison off and the next wipe
/// starts from nothing — so a cycling button reads [`pe_session_compare`] and
/// hands back the fraction it gives.
///
/// Not an edit: it is not in the history, the document is untouched, and an
/// export renders the document either way. Nothing needs to be open, as with
/// [`pe_session_set_cropping`] — a property of the window outlives whichever
/// photograph is in it.
///
/// Returns 0; `-1` for a null handle; `-2` for a mode this ABI has no
/// comparison for, with the reason in [`pe_session_last_error`].
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_compare(s: *mut PeSession, mode: u32, wipe: f32) -> i32 {
    with(s, -1, move |s| {
        let Some(compare) = compare_mode(mode) else {
            s.last_error = Some(format!("{mode} is not a comparison: 0 off, 1 wipe, 2 side"));
            return -2;
        };
        s.inner.set_compare(compare, wipe);
        s.last_error = None;
        0
    })
}

/// Which comparison the viewer is showing, and where its seam sits.
///
/// `out_mode` is one of the `PE_COMPARE_*` values; `out_wipe` is the fraction
/// [`pe_session_set_compare`] stored, clamped, **whatever the mode is**. With
/// a comparison off that is the seam the next wipe will start from, which is
/// what lets a cycling button hand the fraction back instead of flattening it
/// to zero. Before anything has been set it is 0.5: a first wipe begins in the
/// middle.
///
/// Two answers in one call rather than two functions, because the control that
/// wants either wants both: the button draws its state from the mode and the
/// seam is drawn from the fraction, on the same frame.
///
/// Either out-pointer may be null. Returns 0, or `-1` for a null handle. There
/// is no `-2`: a window property has an answer with nothing open, and reading
/// it is not something the session can refuse.
///
/// # Safety
/// `s` must be valid or null; each non-null out-pointer must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_compare(
    s: *mut PeSession,
    out_mode: *mut u32,
    out_wipe: *mut f32,
) -> i32 {
    with(s, -1, move |s| {
        unsafe {
            if !out_mode.is_null() {
                out_mode.write(compare_value(s.inner.compare()));
            }
            if !out_wipe.is_null() {
                out_wipe.write(s.inner.wipe());
            }
        }
        0
    })
}

// ---- history --------------------------------------------------------------

/// Bracket a drag so it becomes one undo step rather than three hundred.
///
/// # Safety
/// `s` and `label` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_begin_interaction(
    s: *mut PeSession,
    label: *const c_char,
) -> i32 {
    let label = as_str(label).unwrap_or("Edit").to_string();
    status(s, move |s| {
        s.begin_interaction(label);
        Ok(())
    })
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_end_interaction(s: *mut PeSession) -> i32 {
    status(s, |s| {
        s.end_interaction();
        Ok(())
    })
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_can_undo(s: *mut PeSession) -> bool {
    with(s, false, |s| s.inner.can_undo())
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_can_redo(s: *mut PeSession) -> bool {
    with(s, false, |s| s.inner.can_redo())
}

/// `1` if it moved, `0` if there was nothing to undo, negative on failure.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_undo(s: *mut PeSession) -> i32 {
    with(s, -1, |s| match s.inner.undo() {
        Ok(moved) => moved as i32,
        Err(e) => {
            s.last_error = Some(e.to_string());
            -2
        }
    })
}

/// `1` if it moved, `0` if there was nothing to redo, negative on failure.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_redo(s: *mut PeSession) -> i32 {
    with(s, -1, |s| match s.inner.redo() {
        Ok(moved) => moved as i32,
        Err(e) => {
            s.last_error = Some(e.to_string());
            -2
        }
    })
}

// ---- persistence and export -----------------------------------------------

/// Write a `.peproj` beside the photograph, returning its path. Null on
/// failure, with the reason in [`pe_session_last_error`]. Caller must release
/// the path with [`pe_string_free`].
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_save_sidecar(s: *mut PeSession) -> *mut c_char {
    with(s, ptr::null_mut(), |s| match s.inner.save_sidecar() {
        Ok(p) => {
            s.last_error = None;
            to_c(p.display().to_string())
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            ptr::null_mut()
        }
    })
}

/// # Safety
/// `s` and `path` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_load_sidecar(s: *mut PeSession, path: *const c_char) -> i32 {
    let Some(path) = as_str(path) else { return -1 };
    let path = path.to_string();
    status(s, move |s| s.load_sidecar(path))
}

/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_revert(s: *mut PeSession) -> i32 {
    status(s, |s| s.revert())
}

/// Write the work in progress now, throttle or no throttle.
///
/// The tick respects the debounce, which is right sixty times a second and
/// wrong exactly once: when the photograph is being left, and the thing that
/// would have triggered the write is about to stop being on screen.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_flush_autosave(s: *mut PeSession) -> i32 {
    status(s, |s| s.write_autosave())
}

// The grade in hand: copy a look off one photograph and put it on another.
//
// The clipboard is the session's, not a shell's, so what crosses here is three
// verbs and a question — never the stack itself. A stack on the wire would be a
// second copy of the document's own shape, in JSON, that both sides would have
// to keep agreeing about.

/// Copy this photograph's grade.
///
/// Returns 0; `-1` for a null handle; `-2` with nothing open, the reason on
/// [`pe_session_last_error`].
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_copy_grade(s: *mut PeSession) -> i32 {
    status(s, |s| s.copy_grade())
}

/// Whether a grade has been copied, which is what a Paste item is greyed by.
///
/// `1` yes, `0` no — and `0` for a null handle too, because a session that is
/// not there has copied nothing. There is nothing here a caller could do
/// differently for the two, which is why this one is not a status code.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_has_grade(s: *mut PeSession) -> i32 {
    with(s, 0, |s| i32::from(s.inner.has_grade()))
}

/// Put the copied grade on this photograph, as one undo step.
///
/// Returns 0; `-1` for a null handle; `-2` with nothing open or nothing
/// copied — the two read differently on [`pe_session_last_error`].
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_paste_grade(s: *mut PeSession) -> i32 {
    status(s, |s| s.paste_grade())
}

/// Put the copied grade on every *other* photograph in the set.
///
/// Returns how many took it, which a shell says out loud — `0` is a real
/// answer for a set of one. `-1` for a null handle; `-2` with no set open or
/// nothing copied, the reason on [`pe_session_last_error`].
///
/// Not this photograph: [`pe_session_paste_grade`] is how it gets the grade,
/// and doing both from one call would make the count a lie.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_paste_grade_to_all(s: *mut PeSession) -> i32 {
    with(s, -1, |s| match s.inner.paste_grade_to_all() {
        Ok(n) => {
            s.last_error = None;
            // A set larger than two billion photographs is not a thing, and a
            // saturating cast is a smaller lie than a wrapped one.
            i32::try_from(n).unwrap_or(i32::MAX)
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            -2
        }
    })
}

/// # Safety
/// `s` and `format` must be valid or null. `format` is one of `jpeg`, `png`,
/// `png16`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_set_export(
    s: *mut PeSession,
    format: *const c_char,
    quality: u8,
) -> i32 {
    let name = as_str(format).unwrap_or("jpeg").to_string();
    status(s, move |s| {
        s.set_export(pe_session::export::Format::from_name(&name), quality);
        Ok(())
    })
}

/// Export, returning the path written. Null on failure; the reason is in
/// [`pe_session_last_error`], and "refused" there is not a bug. Caller must
/// release the path with [`pe_string_free`].
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_export(s: *mut PeSession) -> *mut c_char {
    with(s, ptr::null_mut(), |s| match s.inner.export_current() {
        Ok(p) => {
            s.last_error = None;
            to_c(p.display().to_string())
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            ptr::null_mut()
        }
    })
}

// ---- a batch --------------------------------------------------------------
//
// Every photograph in the set, each with its own edit, into one folder chosen
// rather than beside each original. The run belongs to the session — see
// [`pe_session::Session::step_batch`] for which of the three places an edit can
// be is looked in — and what crosses here is a directory, a step, three counts
// and a cancel. Never a frame, and never a list.
//
// **The stepping is the caller's, and that is deliberate.** Sixty photographs
// is sixty full-resolution renders; a loop inside the engine would freeze the
// window for a minute with no way to tell whether it was working or hung, and
// no way to stop it. One step per frame keeps the interface alive, gives
// somewhere to show progress, and makes cancelling a matter of not asking for
// the next one.

/// Begin exporting every photograph in the set into `dir`, in whichever format
/// [`pe_session_set_export`] was last given.
///
/// The format is taken now rather than per photograph, so changing it halfway
/// cannot leave a folder half JPEG and half PNG.
///
/// Returns 0; `-1` for a null handle or a null or non-UTF-8 `dir`; `-2` if the
/// session refused, with the reason in [`pe_session_last_error`].
///
/// **With no set open this is `-2`, not 0.** A session showing nothing, or
/// showing the built-in chart, has no photographs to run over, and answering 0
/// would report a successful run of nought files. Starting a second run
/// replaces the first, counts and all.
///
/// # Safety
/// `s` and `dir` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_start_batch(s: *mut PeSession, dir: *const c_char) -> i32 {
    let Some(dir) = as_str(dir) else { return -1 };
    let dir = std::path::PathBuf::from(dir);
    status(s, move |s| s.start_batch(dir))
}

/// Export one photograph, and say whether there is more to do.
///
/// **`1` there is more, `0` there is no more, negative it was refused.** Three
/// answers, not two, and not the usual 0-is-success status: the loop condition
/// is `> 0`, never `!= 0`. A caller that reads `0` as a failure abandons a run
/// on its last step and leaves the rest of the folder unwritten; a caller that
/// reads a negative as "finished" reports `n exported` for a run that never
/// started. `1` is the odd one out precisely so that neither mistake is
/// silent.
///
/// `-1` for a null handle; `-2` if the session refused, with the reason in
/// [`pe_session_last_error`]. A refusal means the engine has no device to
/// render with, which ends the whole run rather than costing it one
/// photograph. A photograph that merely cannot be written — a collision with
/// somebody's original, a file that will not decode — is *not* a refusal: it
/// is counted in `failed` by [`pe_session_batch_progress`] and stepped past,
/// because one collision should not abandon the other sixty-five.
///
/// **`0` with no run in progress at all**, since there is equally nothing more
/// to do. [`pe_session_batch_progress`] is what tells a run that finished from
/// one that was never started, and it is the thing to ask before reporting a
/// summary.
///
/// Call it once a frame from wherever the render loop already ticks, not in a
/// loop and never inside a view update: a full-resolution render there is a
/// frozen window with extra steps.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_step_batch(s: *mut PeSession) -> i32 {
    with(s, -1, |s| match s.inner.step_batch() {
        Ok(more) => {
            s.last_error = None;
            i32::from(more)
        }
        Err(e) => {
            s.last_error = Some(e.to_string());
            -2
        }
    })
}

/// How far the run has got: done, failed, total.
///
/// Three counts in one call, because a progress bar wants all three on every
/// frame it draws. Any out-pointer may be null. Returns 0; `-1` for a null
/// handle; `-2` when no run has been started, in which case nothing is
/// written. The two counts do not have to add up to the third until the run is
/// over, which is the point of giving all three.
///
/// **A finished run is still a run.** Its counts stay readable until it is
/// cancelled or another begins, because the summary — `n exported`, or
/// `n exported, m failed` — is read *after* the step that answered 0. A run
/// that silently stopped is indistinguishable from one that crashed.
///
/// `-2` here leaves [`pe_session_last_error`] alone rather than writing a
/// reason onto it, as the other readers of the set do. "No batch is running"
/// is the ordinary state of a session, this is asked once a frame to decide
/// whether to draw a progress bar at all, and a message per frame would bury
/// whatever real failure was sitting there.
///
/// # Safety
/// `s` must be valid or null; each non-null out-pointer must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_batch_progress(
    s: *mut PeSession,
    out_done: *mut u32,
    out_failed: *mut u32,
    out_total: *mut u32,
) -> i32 {
    with(s, -1, |s| {
        let Some((done, failed, total)) = s.inner.batch_progress() else {
            return -2;
        };
        unsafe {
            if !out_done.is_null() {
                out_done.write(done as u32);
            }
            if !out_failed.is_null() {
                out_failed.write(failed as u32);
            }
            if !out_total.is_null() {
                out_total.write(total as u32);
            }
        }
        0
    })
}

/// Stop the run, keeping whatever has already been written.
///
/// Nothing is taken back. Half a folder of exports is the state somebody asked
/// for when they pressed cancel; deleting the files they had already waited
/// for would be the surprising answer. Also how the counts of a *finished* run
/// are put away once the summary has been shown.
///
/// Returns 0, or `-1` for a null handle. Cancelling when nothing is running
/// succeeds and does nothing: a shell that cancels because a window is closing
/// does not know whether a run is on, and there is nothing it would do
/// differently if it did.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_cancel_batch(s: *mut PeSession) -> i32 {
    status(s, |s| {
        s.cancel_batch();
        Ok(())
    })
}

// ---- what is remembered between runs --------------------------------------
//
// The handful of things that belong to the person rather than to any one
// picture: the effects they have starred and the set that was open. See
// [`pe_session::Settings`] for why they live in the engine and not in a shell —
// a star means the same in both, and so does the set you left open.
//
// The two lists cross as JSON, by rule 4. Neither is a scalar, neither has a
// count known in advance, and both are read once at launch rather than per
// frame: the typed alternative is a pointer-and-length pair of a type this ABI
// is not allowed to name.

/// Whether an effect is starred. `1` yes, `0` no; `-1` for a null handle or a
/// null or non-UTF-8 `key`.
///
/// No `-2`. The session cannot refuse this question — a key it has never heard
/// of is simply not starred — so there is nothing for a refusal to mean.
///
/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_is_favourite(s: *mut PeSession, key: *const c_char) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    with(s, -1, |s| i32::from(s.inner.is_favourite(&key)))
}

/// Star or unstar an effect, and write the change out.
///
/// Returns 0; `-1` for a null handle or a null or non-UTF-8 `key`. No `-2`, for
/// the reason [`pe_session_is_favourite`] gives, and none for the write either:
/// settings that cannot be written are not an error anybody is told about — see
/// [`pe_session::Settings::save`].
///
/// # Safety
/// `s` and `key` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_toggle_favourite(s: *mut PeSession, key: *const c_char) -> i32 {
    let Some(key) = as_str(key) else { return -1 };
    let key = key.to_string();
    status(s, move |s| {
        s.toggle_favourite(&key);
        Ok(())
    })
}

/// Every starred effect, as a JSON array of keys — `["grain","halation"]` — in
/// the order they were starred. Caller must release with [`pe_string_free`].
///
/// Null only for a null handle. Nothing starred is `[]`, which is a list and
/// not a failure: a browser with no favourites draws no favourites heading, and
/// that is a different thing from a browser that could not ask.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_favourites_json(s: *mut PeSession) -> *mut c_char {
    with(s, ptr::null_mut(), |s| {
        match serde_json::to_string(&s.inner.settings().favourites) {
            Ok(j) => to_c(j),
            Err(_) => ptr::null_mut(),
        }
    })
}

/// The set that was open when this last ran, as
/// `{"paths":["/a.jpg","/b.jpg"],"index":1}`. Caller must release with
/// [`pe_string_free`].
///
/// **Only the photographs that are still there** — see
/// [`pe_session::Session::remembered_session`], which drops the ones that have
/// gone and looks the one that was showing up again by name in what survived.
///
/// An empty `paths` with an `index` of nought is the answer for a first run and
/// for a set whose files have all gone. The index is not a position in an empty
/// list and nothing should treat it as one: [`pe_session_open_paths`] refuses
/// an empty set, so a shell that passes this straight back gets `-2` rather
/// than a set of no photographs.
///
/// **What this does not promise is that the photographs will decode.** They
/// exist; that is all `is_file` can say. A file that is there and will not read
/// makes [`pe_session_open_paths`] answer `-2`, and what to do about that is
/// the shell's to decide — the engine has no window to say it in.
///
/// Null only for a null handle.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_remembered_session_json(s: *mut PeSession) -> *mut c_char {
    with(s, ptr::null_mut(), |s| {
        let (paths, index) = s.inner.remembered_session();
        let described = serde_json::json!({
            "paths": paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>(),
            "index": index,
        });
        match serde_json::to_string(&described) {
            Ok(j) => to_c(j),
            Err(_) => ptr::null_mut(),
        }
    })
}

// ---- scopes ---------------------------------------------------------------

/// Which measurement to read. The numbering is part of the ABI: add to the end.
///
/// Every scope crosses as `planes * height * width` `u32`, row-major, and the
/// plane order is part of the contract because the drawing depends on it:
/// **red, green, blue, luma** for a histogram and a waveform, **hue,
/// saturation** for a colour spread, and a single plane for the rest.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PeScope {
    /// Four planes — red, green, blue, luma — of 256 levels, one row each.
    Histogram = 0,
    /// The same frame binned in the curve's own domain. Same four planes.
    LogHistogram = 1,
    /// Hue and saturation spread, for behind the secondary curves. Two
    /// planes — hue, then saturation — of 256 bins, one row each.
    ColourSpread = 2,
    /// Four planes — red, green, blue, luma — of one row per image column,
    /// each row 256 levels wide. The columns follow the width passed to
    /// [`pe_session_measure`], not the size of the photograph.
    Waveform = 3,
    /// One plane, 256 by 256, y increasing downwards for drawing.
    Vectorscope = 4,
    /// The Colour Warper's chromaticity cloud: one plane, 128 by 128.
    WarperChromaticity = 5,
    /// The Colour Warper's hue/saturation cloud: one plane, 128 by 128.
    WarperHueSat = 6,
    /// The Colour Warper's chroma/luma cloud: one plane, 128 by 128.
    WarperChromaLuma = 7,
}

/// One measurement, in the shape the ABI describes it: `planes` slices of
/// `height * width` counts, laid end to end.
///
/// Borrowed rather than copied, so that asking for the shape of a 2.6 MB
/// waveform costs nothing until the counts are actually asked for.
struct ScopeView<'a> {
    /// In the order [`PeScope`] documents.
    planes: Vec<&'a [u32]>,
    /// Row length — the fastest-moving axis.
    width: u32,
    /// Rows per plane.
    height: u32,
    /// What the counts are read against; see [`pe_session_scope_shape`].
    total: u32,
}

impl ScopeView<'_> {
    fn len(&self) -> usize {
        self.planes.iter().map(|p| p.len()).sum()
    }

    /// The largest count anywhere in the data.
    ///
    /// Computed here rather than taken from `Histogram::peak`,
    /// `Vectorscope::peak` or `Distribution::peaks`: all three are plain
    /// maxima over exactly the planes this hands out, so one uniform rule
    /// gives the same answer for all eight scopes and cannot drift from what
    /// [`pe_session_scope_data`] copies. If one of them ever starts excluding
    /// a bin — a histogram ignoring bin 0 so a black-point spike stops
    /// flattening everything else — that scope must switch to its own method
    /// here, and this comment is the place to say so.
    fn peak(&self) -> u32 {
        self.planes
            .iter()
            .flat_map(|p| p.iter())
            .copied()
            .max()
            .unwrap_or(0)
    }
}

fn scope_view(sc: &pe_session::Scopes, kind: PeScope) -> ScopeView<'_> {
    use pe_scopes::{BINS, Channel, LEVELS, VECTOR_SIZE};

    fn histogram(h: &pe_scopes::Histogram) -> ScopeView<'_> {
        ScopeView {
            planes: vec![&h.red[..], &h.green[..], &h.blue[..], &h.luma[..]],
            width: pe_scopes::BINS as u32,
            height: 1,
            total: h.total,
        }
    }
    // A warper grid holds no count of its own — black has no chromaticity and
    // is never binned — so the pixels measured come from the histogram, which
    // counts every one of them.
    fn grid(g: &[u32], total: u32) -> ScopeView<'_> {
        ScopeView {
            planes: vec![g],
            width: pe_scopes::warper::GRID as u32,
            height: pe_scopes::warper::GRID as u32,
            total,
        }
    }

    match kind {
        PeScope::Histogram => histogram(&sc.histogram),
        PeScope::LogHistogram => histogram(&sc.log_histogram),
        PeScope::ColourSpread => ScopeView {
            planes: vec![&sc.colour.hue[..], &sc.colour.saturation[..]],
            width: BINS as u32,
            height: 1,
            total: sc.colour.total,
        },
        PeScope::Waveform => ScopeView {
            planes: Channel::ALL
                .iter()
                .map(|c| sc.waveform.channel(*c))
                .collect(),
            width: LEVELS as u32,
            height: sc.waveform.columns() as u32,
            total: sc.waveform.rows() as u32,
        },
        PeScope::Vectorscope => ScopeView {
            planes: vec![sc.vectorscope.bins()],
            width: VECTOR_SIZE as u32,
            height: VECTOR_SIZE as u32,
            total: sc.vectorscope.total(),
        },
        PeScope::WarperChromaticity => grid(&sc.warper.chromaticity, sc.histogram.total),
        PeScope::WarperHueSat => grid(&sc.warper.hue_sat, sc.histogram.total),
        PeScope::WarperChromaLuma => grid(&sc.warper.chroma_luma, sc.histogram.total),
    }
}

/// Render the current grade at `width` by `height` and bin it.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_measure(s: *mut PeSession, width: u32, height: u32) -> i32 {
    status(s, move |s| s.measure_scopes(width, height))
}

/// Which measurement the session is holding, or 0 for none.
///
/// Zero before the first measurement and again after an edit throws one away,
/// so this one call answers both questions a caller has: is there anything to
/// read, and is it the same as last time. Compare it before copying 2.6 MB of
/// waveform and you will copy neither the same numbers twice nor stale ones
/// once.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_scope_generation(s: *mut PeSession) -> u64 {
    with(s, 0, |s| s.inner.scope_generation())
}

/// How big a scope is, and what to divide its counts by.
///
/// Every scope is `planes * height * width` `u32`, row-major, in the plane
/// order [`PeScope`] documents. `peak` is the largest count in that data. Any
/// out-pointer may be null, and a null `peak` is worth passing when it is not
/// wanted: it is the only field that costs a walk over the counts.
///
/// `total` is the number of pixels measured — except for a **waveform**, where
/// it is how many image rows fed each column. That is the natural full scale
/// for a waveform cell, and unlike the peak it does not move as the picture is
/// graded, so the display does not flicker under the user's hand. The Windows
/// shell normalises against exactly this; see its `intensity`.
///
/// Returns 0, or -1 with nothing measured.
///
/// # Safety
/// `s` must be valid or null; each non-null out-pointer must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_scope_shape(
    s: *mut PeSession,
    kind: PeScope,
    planes: *mut u32,
    width: *mut u32,
    height: *mut u32,
    total: *mut u32,
    peak: *mut u32,
) -> i32 {
    with(s, -1, |s| {
        let Some(view) = s.inner.scopes().map(|sc| scope_view(sc, kind)) else {
            return -1;
        };
        unsafe {
            if !planes.is_null() {
                planes.write(view.planes.len() as u32);
            }
            if !width.is_null() {
                width.write(view.width);
            }
            if !height.is_null() {
                height.write(view.height);
            }
            if !total.is_null() {
                total.write(view.total);
            }
            if !peak.is_null() {
                peak.write(view.peak());
            }
        }
        0
    })
}

/// Copy a scope's counts into `out`, returning how many were written, or a
/// negative number: -1 with nothing measured or a null `out`, -2 if `capacity`
/// is short of what [`pe_session_scope_shape`] reported.
///
/// Short rather than truncating, because a half-copied waveform draws a
/// plausible picture of a frame that does not exist.
///
/// # Safety
/// `s` must be valid or null. `out` must point to at least `capacity`
/// writable `u32`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_scope_data(
    s: *mut PeSession,
    kind: PeScope,
    out: *mut u32,
    capacity: u32,
) -> i32 {
    with(s, -1, |s| {
        if out.is_null() {
            return -1;
        }
        let Some(view) = s.inner.scopes().map(|sc| scope_view(sc, kind)) else {
            return -1;
        };
        let wanted = view.len();
        if (capacity as usize) < wanted {
            return -2;
        }
        let mut written = 0usize;
        for plane in &view.planes {
            // Rule 2 stays trivially satisfied: nothing is allocated here, so
            // there is nothing for a `pe_*_free` to release. The buffer is the
            // caller's, before and after.
            unsafe { ptr::copy_nonoverlapping(plane.as_ptr(), out.add(written), plane.len()) };
            written += plane.len();
        }
        written as i32
    })
}

/// Screen pixels per image pixel, for the zoom readout and the 100% button.
///
/// **Not the view's `size`**, which is a fraction of the frame and reads 1 for
/// any fitted view. This is what a person means by 100%: one image pixel to one
/// screen pixel. Negative with nothing open or no layer attached — there is no
/// viewport to measure against, and a made-up 1.0 would be a readout that looks
/// right and is not.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_view_scale(s: *mut PeSession) -> f32 {
    with(s, -1.0, |s| s.inner.view_scale().unwrap_or(-1.0))
}

/// The fraction of pixels above diffuse white, which is what a clipping
/// warning is actually about. Negative with nothing measured.
///
/// # Safety
/// `s` must be valid or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_session_over_white_fraction(s: *mut PeSession) -> f32 {
    with(s, -1.0, |s| {
        s.inner
            .scopes()
            .map_or(-1.0, |sc| sc.histogram.over_white_fraction() as f32)
    })
}

// ---- the registry ---------------------------------------------------------

/// Every effect and every parameter, as JSON. Called once at launch; the whole
/// inspector is generated from it. Caller must release with [`pe_string_free`].
#[unsafe(no_mangle)]
pub extern "C" fn pe_registry_json() -> *mut c_char {
    guard(ptr::null_mut(), || {
        match serde_json::to_string(&pe_session::describe::registry()) {
            Ok(j) => to_c(j),
            Err(_) => ptr::null_mut(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"{
        "schema_version": 1,
        "source": {"kind":"path","path":"a.jpg"},
        "stack": [
            {"id":1,"effect":"exposure"},
            {"id":2,"effect":"grain"}
        ]
    }"#;

    fn cstr(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    /// The session as the shell sees it: JSON out, parsed, string freed.
    fn snapshot(s: *mut PeSession) -> serde_json::Value {
        let json = unsafe { pe_session_snapshot_json(s) };
        let text = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(json) };
        serde_json::from_str(&text).unwrap()
    }

    /// The `hue_sat` lattice of a row, as the object a `Warp` serialises to.
    fn warp_of(s: *mut PeSession, row: u64) -> serde_json::Value {
        let snap = snapshot(s);
        let param = snap["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row)
            .map(|r| r["params"]["hue_sat"].clone())
            .expect("the row that was just written to");
        assert_eq!(param["t"], "warp");
        param["v"].clone()
    }

    /// The `pins` set of a row, as the bare list a `Pins` serialises to.
    fn pins_of(s: *mut PeSession, row: u64) -> Vec<serde_json::Value> {
        let snap = snapshot(s);
        let param = snap["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row)
            .map(|r| r["params"]["pins"].clone())
            .expect("the row that was just written to");
        assert_eq!(param["t"], "pins");
        param["v"].as_array().expect("a pin set is a list").clone()
    }

    #[test]
    fn a_document_round_trips_across_the_boundary() {
        let json = cstr(DOC);
        let doc = unsafe { pe_document_from_json(json.as_ptr()) };
        assert!(!doc.is_null());

        assert_eq!(unsafe { pe_document_row_count(doc) }, 2);

        let out = unsafe { pe_document_to_json(doc) };
        assert!(!out.is_null());
        let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_owned();
        assert!(text.contains("exposure"));

        unsafe { pe_string_free(out) };
        unsafe { pe_document_free(doc) };
    }

    #[test]
    fn malformed_json_returns_null_rather_than_panicking() {
        let json = cstr("{ not json");
        assert!(unsafe { pe_document_from_json(json.as_ptr()) }.is_null());
    }

    #[test]
    fn null_inputs_are_handled() {
        assert!(unsafe { pe_document_from_json(ptr::null()) }.is_null());
        assert!(unsafe { pe_document_to_json(ptr::null()) }.is_null());
        assert_eq!(unsafe { pe_document_row_count(ptr::null()) }, -1);
        // Freeing null must be a no-op, not a crash.
        unsafe { pe_document_free(ptr::null_mut()) };
        unsafe { pe_string_free(ptr::null_mut()) };
    }

    #[test]
    fn a_document_from_the_future_is_refused_not_misread() {
        let json = cstr(r#"{"schema_version":99,"source":{"kind":"path","path":"a.jpg"}}"#);
        assert!(unsafe { pe_document_from_json(json.as_ptr()) }.is_null());
    }

    #[test]
    fn version_is_a_valid_c_string() {
        let v = unsafe { CStr::from_ptr(pe_version()) }.to_str().unwrap();
        assert_eq!(v, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn a_session_opens_a_chart_and_reports_its_rows() {
        let s = pe_session_new();
        assert!(!s.is_null());
        assert_eq!(unsafe { pe_session_open_test_chart(s, 64, 64) }, 0);
        assert!(unsafe { pe_session_row_count(s) } > 0);
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn every_entry_point_survives_a_null_handle() {
        // A null here is a Swift bug, and a crash inside Rust tells nobody
        // anything useful about where it was. Each of these returns its
        // failure value instead.
        assert_eq!(unsafe { pe_session_row_count(ptr::null_mut()) }, -1);
        assert_eq!(
            unsafe { pe_session_open_test_chart(ptr::null_mut(), 8, 8) },
            -1
        );
        assert!(unsafe { pe_session_snapshot_json(ptr::null_mut()) }.is_null());
        assert_eq!(unsafe { pe_session_snapshot_version(ptr::null_mut()) }, 0);
        assert_eq!(unsafe { pe_session_undo(ptr::null_mut()) }, -1);
        let list = cstr("[\"/a.png\"]");
        assert_eq!(
            unsafe { pe_session_open_paths(ptr::null_mut(), list.as_ptr()) },
            -1
        );
        assert_eq!(unsafe { pe_session_focus(ptr::null_mut(), 0) }, -1);
        assert_eq!(unsafe { pe_session_entry_count(ptr::null_mut()) }, -1);
        assert!(unsafe { pe_session_entry_path(ptr::null_mut(), 0) }.is_null());
        assert_eq!(
            unsafe {
                pe_session_entry_flags(
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
            -1
        );
        assert_eq!(unsafe { pe_session_current_entry(ptr::null_mut()) }, -1);
        assert_eq!(
            unsafe { pe_session_request_thumbnails(ptr::null_mut(), 0, 1) },
            -1
        );
        assert_eq!(
            unsafe { pe_session_collect_thumbnails(ptr::null_mut()) },
            -1
        );
        assert_eq!(
            unsafe {
                pe_session_thumbnail_shape(ptr::null_mut(), 0, ptr::null_mut(), ptr::null_mut())
            },
            -1
        );
        let mut out = [0u8; 4];
        assert_eq!(
            unsafe { pe_session_thumbnail_data(ptr::null_mut(), 0, out.as_mut_ptr(), 4) },
            -1
        );
        let dir = cstr("/out");
        assert_eq!(
            unsafe { pe_session_start_batch(ptr::null_mut(), dir.as_ptr()) },
            -1
        );
        // A step is the one of these whose 0 means something else entirely —
        // "no more to do" — so a null handle must not be able to produce it.
        assert_eq!(unsafe { pe_session_step_batch(ptr::null_mut()) }, -1);
        assert_eq!(
            unsafe {
                pe_session_batch_progress(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
            -1
        );
        assert_eq!(unsafe { pe_session_cancel_batch(ptr::null_mut()) }, -1);
        assert_eq!(
            unsafe { pe_session_set_compare(ptr::null_mut(), PE_COMPARE_WIPE, 0.5) },
            -1
        );
        assert_eq!(
            unsafe { pe_session_compare(ptr::null_mut(), ptr::null_mut(), ptr::null_mut()) },
            -1
        );
        unsafe { pe_session_free(ptr::null_mut()) };
    }

    #[test]
    fn a_panic_is_caught_and_says_what_it_was() {
        // Rule 3 is that nothing unwinds into Swift. The corollary is that the
        // reason has to survive the catch, or the shell is left holding a
        // sentinel and no way to report what happened.
        let s = pe_session_new();
        let caught = with(s, -1i32, |_| panic!("the engine fell over"));
        assert_eq!(caught, -1);

        let msg = unsafe { pe_session_last_error(s) };
        assert!(!msg.is_null(), "a caught panic left no explanation");
        let text = unsafe { CStr::from_ptr(msg) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(msg) };
        assert!(text.contains("fell over"), "unhelpful panic text: {text}");

        // And the session is still usable afterwards rather than poisoned.
        assert_eq!(unsafe { pe_session_open_test_chart(s, 32, 32) }, 0);
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_null_handle_is_told_apart_from_a_panic_by_having_no_message() {
        // Both return the same sentinel, so the message is what distinguishes
        // them: there is no session to write one onto when the handle is null.
        assert!(unsafe { pe_session_last_error(ptr::null_mut()) }.is_null());
    }

    #[test]
    fn a_parameter_the_effect_does_not_have_is_refused_with_a_message() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        let key = cstr("exposure");
        let row = unsafe { pe_session_add_effect(s, key.as_ptr()) };
        assert!(row >= 0);

        let bad = cstr("not_a_parameter");
        assert_ne!(
            unsafe { pe_session_set_float(s, row as u64, bad.as_ptr(), 1.0) },
            0
        );

        let msg = unsafe { pe_session_last_error(s) };
        assert!(
            !msg.is_null(),
            "a failure with no message is a failure nobody can report"
        );
        let text = unsafe { CStr::from_ptr(msg) }.to_str().unwrap().to_owned();
        assert!(
            text.contains("not_a_parameter"),
            "unhelpful message: {text}"
        );
        unsafe { pe_string_free(msg) };
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_drag_bracketed_by_an_interaction_is_one_undo_step() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        let key = cstr("exposure");
        let row = unsafe { pe_session_add_effect(s, key.as_ptr()) } as u64;
        let ev = cstr("ev");
        let label = cstr("Exposure");

        unsafe { pe_session_begin_interaction(s, label.as_ptr()) };
        for i in 1..60 {
            unsafe { pe_session_set_float(s, row, ev.as_ptr(), i as f32 * 0.01) };
        }
        unsafe { pe_session_end_interaction(s) };

        // One undo puts the whole drag back — not one frame of it. Fifty-nine
        // undo steps would mean the coalescing bracket did nothing.
        assert_eq!(unsafe { pe_session_undo(s) }, 1);

        let json = unsafe { pe_session_snapshot_json(s) };
        let text = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(json) };
        let snap: serde_json::Value = serde_json::from_str(&text).unwrap();
        let ev = snap["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row)
            .and_then(|r| r["params"]["ev"]["v"].as_f64())
            .expect("the exposure row is still there");
        assert_eq!(ev, 0.0, "one undo left the drag partly applied: ev is {ev}");

        // And a second undo takes the row away, so the drag really was one step.
        assert_eq!(unsafe { pe_session_undo(s) }, 1);
        assert!(!unsafe { pe_session_can_undo(s) });
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_wheel_crosses_the_boundary_as_four_numbers() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        // primaries is pinned, so a fresh document already has its wheels.
        let json = unsafe { pe_session_snapshot_json(s) };
        let text = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(json) };
        let snap: serde_json::Value = serde_json::from_str(&text).unwrap();
        let row = snap["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["effect"] == "primaries")
            .and_then(|r| r["id"].as_u64())
            .expect("primaries is a pinned row");

        let key = cstr("lift");
        assert_eq!(
            unsafe { pe_session_set_wheel(s, row, key.as_ptr(), 0.25, 0.1, 0.2, 0.3) },
            0
        );

        let json = unsafe { pe_session_snapshot_json(s) };
        let text = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(json) };
        let snap: serde_json::Value = serde_json::from_str(&text).unwrap();
        let lift = snap["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row)
            .map(|r| &r["params"]["lift"])
            .unwrap();
        assert_eq!(lift["t"], "wheel");
        assert_eq!(lift["v"]["master"], 0.25);
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_curve_crosses_the_boundary_as_a_flat_array() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        let json = unsafe { pe_session_snapshot_json(s) };
        let text = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(json) };
        let snap: serde_json::Value = serde_json::from_str(&text).unwrap();
        let row = snap["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["effect"] == "curves")
            .and_then(|r| r["id"].as_u64())
            .expect("curves is a pinned row");

        let key = cstr("luma");
        let xy: [f32; 6] = [0.0, 0.0, 0.5, 0.7, 1.0, 1.0];
        assert_eq!(
            unsafe { pe_session_set_curve(s, row, key.as_ptr(), xy.as_ptr(), 3) },
            0
        );

        let json = unsafe { pe_session_snapshot_json(s) };
        let text = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(json) };
        let snap: serde_json::Value = serde_json::from_str(&text).unwrap();
        let luma = snap["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == row)
            .map(|r| &r["params"]["luma"])
            .unwrap();
        assert_eq!(luma["t"], "curve");
        // `pe_core::Curve` is `#[serde(transparent)]`, so `v` is the point
        // list itself rather than an object wrapping it. Read back as f32,
        // because the point was stored as one and 0.7 widened to f64 is
        // 0.699999988079071 — a difference in the printing, not in the value.
        let y = luma["v"][1][1].as_f64().expect("the y of the middle point") as f32;
        assert_eq!(y, 0.7);
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_null_point_list_is_refused_rather_than_dereferenced() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        let key = cstr("luma");
        assert_eq!(
            unsafe { pe_session_set_curve(s, 0, key.as_ptr(), std::ptr::null(), 3) },
            -1
        );
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_vertex_crosses_the_boundary_and_a_bad_one_is_refused() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };

        // The warper's row id is not knowable from out here without asking.
        let snap = snapshot(s);
        let row = snap["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["effect"] == "colour_warper")
            .and_then(|r| r["id"].as_u64())
            .expect("the warper is a pinned row");

        let key = cstr("hue_sat");
        assert_eq!(
            unsafe { pe_session_set_warp_vertex(s, row, key.as_ptr(), 2, 3, 0.25, -0.1) },
            0
        );

        // A status of 0 does not prove the document moved; read it back.
        // `Warp` serialises as a keyed object whose offsets are row-major, so
        // the vertex at column 2 of row 3 of a 6 by 6 grid is index 20. Read
        // as f32: the offset was stored as one, and -0.1 widened to f64 is
        // -0.10000000149011612 — a difference in the printing, not the value.
        let hue_sat = warp_of(s, row);
        assert_eq!(hue_sat["cols"], 6);
        let v = &hue_sat["offsets"][3 * 6 + 2];
        assert_eq!(v[0].as_f64().unwrap() as f32, 0.25);
        assert_eq!(v[1].as_f64().unwrap() as f32, -0.1);

        // Out of range is refused, not silently dropped.
        assert_eq!(
            unsafe { pe_session_set_warp_vertex(s, row, key.as_ptr(), 99, 0, 0.1, 0.1) },
            -2
        );
        assert_eq!(unsafe { pe_session_clear_warp(s, row, key.as_ptr()) }, 0);

        let hue_sat = warp_of(s, row);
        assert_eq!(hue_sat["cols"], 6, "clearing kept the grid");
        assert!(
            hue_sat["offsets"]
                .as_array()
                .unwrap()
                .iter()
                .all(|o| o[0] == 0.0 && o[1] == 0.0),
            "the lattice was not put back to identity"
        );

        unsafe { pe_session_free(s) };
    }

    #[test]
    fn pins_cross_the_boundary_and_bad_ones_are_refused() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };

        // The warper's row id is not knowable from out here without asking.
        let snap = snapshot(s);
        let row = snap["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["effect"] == "colour_warper")
            .and_then(|r| r["id"].as_u64())
            .expect("the warper is a pinned row");

        let key = cstr("pins");

        assert_eq!(
            unsafe { pe_session_add_pin(s, row, key.as_ptr(), 0.33, 0.35) },
            0
        );
        assert_eq!(
            unsafe { pe_session_add_pin(s, row, key.as_ptr(), 0.20, 0.65) },
            1
        );
        assert_eq!(
            unsafe { pe_session_move_pin(s, row, key.as_ptr(), 1, 0.28, 0.55) },
            0
        );
        assert_eq!(
            unsafe { pe_session_set_pin_shape(s, row, key.as_ptr(), 1, 0.12, 0.2, 0.9, 0.6, 0.75) },
            0
        );

        // A pin that is not there is refused, not ignored.
        assert_eq!(
            unsafe { pe_session_move_pin(s, row, key.as_ptr(), 9, 0.1, 0.1) },
            -2
        );
        assert_eq!(
            unsafe { pe_session_remove_pin(s, row, key.as_ptr(), 9) },
            -2
        );

        // And the document actually changed — a status code alone proves
        // nothing. `Pins` is `#[serde(transparent)]`, so `v` is the list of
        // pins itself rather than an object wrapping it. Read back as f32:
        // 0.75 widens to f64 exactly but 0.12 does not, and the value that was
        // stored was an f32 either way.
        let pins = pins_of(s, row);
        assert_eq!(pins.len(), 2);
        assert_eq!(pins[1]["to"][0].as_f64().unwrap() as f32, 0.28);
        assert_eq!(pins[1]["to"][1].as_f64().unwrap() as f32, 0.55);
        assert_eq!(pins[1]["at"][0].as_f64().unwrap() as f32, 0.20);
        assert_eq!(pins[1]["chroma_range"].as_f64().unwrap() as f32, 0.12);
        assert_eq!(pins[1]["exposure"].as_f64().unwrap() as f32, 0.75);

        assert_eq!(unsafe { pe_session_remove_pin(s, row, key.as_ptr(), 0) }, 0);
        let pins = pins_of(s, row);
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0]["to"][0].as_f64().unwrap() as f32, 0.28);

        unsafe { pe_session_free(s) };
    }

    /// The `geometry` block of the snapshot — the document's own view of the
    /// crop, which is what the out-parameters have to agree with.
    fn geometry_of(s: *mut PeSession) -> serde_json::Value {
        snapshot(s)["geometry"].clone()
    }

    /// Everything the seven out-parameters can hold, so a test can ask for all
    /// of them without seven `let mut`s each time.
    #[derive(Default)]
    struct Out {
        cx: f32,
        cy: f32,
        w: f32,
        h: f32,
        angle: f32,
        turns: u32,
        aspect: f32,
    }

    impl Out {
        /// What the engine stored, rebuilt from what it wrote back. The flips
        /// have no out-parameter, so they are passed in here.
        fn geometry(&self, flip_h: bool, flip_v: bool) -> pe_core::Geometry {
            pe_core::Geometry {
                centre: [self.cx, self.cy],
                size: [self.w, self.h],
                angle: self.angle,
                turns: self.turns as u8,
                flip_h,
                flip_v,
                aspect: pe_core::AspectLock::Free,
            }
        }
    }

    #[test]
    fn a_geometry_crosses_and_comes_back_corrected() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };

        // A crop hanging off the top corner, and five quarter-turns, which is
        // one.
        let mut o = Out::default();
        assert_eq!(
            unsafe {
                pe_session_set_geometry(
                    s,
                    0.9,
                    0.9,
                    0.5,
                    0.5,
                    0.0,
                    5,
                    true,
                    false,
                    0.0,
                    &mut o.cx,
                    &mut o.cy,
                    &mut o.w,
                    &mut o.h,
                    &mut o.angle,
                    &mut o.turns,
                    &mut o.aspect,
                )
            },
            0
        );

        // The engine corrected. Not "a status of 0 came back" — the numbers
        // themselves are different ones.
        assert_eq!(o.turns, 1, "five quarter-turns is one");
        assert_ne!(
            (o.cx, o.cy),
            (0.9, 0.9),
            "the crop was left hanging off the edge"
        );
        assert!(
            o.geometry(true, false).fits(64, 64),
            "what came back is still outside the source: {:?}",
            o.geometry(true, false)
        );

        // And what came back is what the document holds, to the bit. Read as
        // f32 both sides: the values were stored as f32, and widening them to
        // f64 for JSON changes the printing, not the number.
        let g = geometry_of(s);
        assert_eq!(g["centre"][0].as_f64().unwrap() as f32, o.cx);
        assert_eq!(g["centre"][1].as_f64().unwrap() as f32, o.cy);
        assert_eq!(g["size"][0].as_f64().unwrap() as f32, o.w);
        assert_eq!(g["size"][1].as_f64().unwrap() as f32, o.h);
        assert_eq!(g["angle"].as_f64().unwrap() as f32, o.angle);
        assert_eq!(g["turns"], 1);
        // The flips are never corrected, which is why they have no
        // out-parameter; they still have to arrive.
        assert_eq!(g["flip_h"], true);
        assert_eq!(g["flip_v"], false);
        assert_eq!(g["aspect"], "free");

        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_null_out_pointer_is_allowed() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };

        // A caller that does not care what was stored passes nulls and gets a
        // status code. A 0.4 crop straightened by 12 degrees fits as it
        // stands, so there is nothing to correct and the document should hold
        // exactly what was asked for.
        assert_eq!(
            unsafe {
                pe_session_set_geometry(
                    s,
                    0.0,
                    0.0,
                    0.4,
                    0.4,
                    12.0,
                    0,
                    false,
                    true,
                    0.0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
            0
        );

        let g = geometry_of(s);
        assert_eq!(g["angle"].as_f64().unwrap() as f32, 12.0);
        assert_eq!(g["size"][0].as_f64().unwrap() as f32, 0.4);
        assert_eq!(g["size"][1].as_f64().unwrap() as f32, 0.4);
        assert_eq!(g["flip_v"], true);

        unsafe { pe_session_free(s) };
    }

    #[test]
    fn resetting_puts_it_back_to_the_whole_frame() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };

        let mut o = Out::default();
        assert_eq!(
            unsafe {
                pe_session_set_geometry(
                    s,
                    0.1,
                    -0.05,
                    0.3,
                    0.3,
                    20.0,
                    2,
                    true,
                    true,
                    PE_ASPECT_ORIGINAL,
                    &mut o.cx,
                    &mut o.cy,
                    &mut o.w,
                    &mut o.h,
                    &mut o.angle,
                    &mut o.turns,
                    &mut o.aspect,
                )
            },
            0
        );
        assert_ne!(geometry_of(s)["angle"], 0.0, "nothing was set to reset");

        assert_eq!(unsafe { pe_session_reset_geometry(s) }, 0);

        let g = geometry_of(s);
        assert_eq!(g["centre"][0].as_f64().unwrap() as f32, 0.0);
        assert_eq!(g["centre"][1].as_f64().unwrap() as f32, 0.0);
        assert_eq!(g["size"][0].as_f64().unwrap() as f32, 1.0);
        assert_eq!(g["size"][1].as_f64().unwrap() as f32, 1.0);
        assert_eq!(g["angle"].as_f64().unwrap() as f32, 0.0);
        assert_eq!(g["turns"], 0);
        assert_eq!(g["flip_h"], false);
        assert_eq!(g["flip_v"], false);
        assert_eq!(g["aspect"], "free", "the lock is part of the whole frame");

        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_locked_ratio_reshapes_the_crop_and_comes_back_as_one() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };

        // 2:1 against a square crop of a square source: the height gives way,
        // because `apply_aspect` never grows the crop.
        let mut o = Out::default();
        assert_eq!(
            unsafe {
                pe_session_set_geometry(
                    s,
                    0.0,
                    0.0,
                    0.8,
                    0.8,
                    0.0,
                    0,
                    false,
                    false,
                    2.0,
                    &mut o.cx,
                    &mut o.cy,
                    &mut o.w,
                    &mut o.h,
                    &mut o.angle,
                    &mut o.turns,
                    &mut o.aspect,
                )
            },
            0
        );
        assert_eq!((o.w, o.h), (0.8, 0.4), "the lock did not re-shape the crop");
        assert_eq!(o.aspect, 2.0);

        let g = geometry_of(s);
        assert_eq!(g["aspect"], "ratio");
        assert_eq!(g["aspect_w"].as_f64().unwrap() as f32, 2.0);
        assert_eq!(g["aspect_h"].as_f64().unwrap() as f32, 1.0);
        assert_eq!(g["size"][1].as_f64().unwrap() as f32, o.h);

        unsafe { pe_session_free(s) };
    }

    /// The third arm has to survive the round trip. If it came back as the
    /// number the source's proportions happen to work out to, the next frame
    /// of the drag would hand that number back in as a fixed ratio and the
    /// lock would quietly stop being "Original" — a control changing its own
    /// value behind the user's back.
    #[test]
    fn an_original_lock_comes_back_as_original_and_not_as_a_ratio() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };

        let mut o = Out::default();
        assert_eq!(
            unsafe {
                pe_session_set_geometry(
                    s,
                    0.0,
                    0.0,
                    0.8,
                    0.8,
                    0.0,
                    0,
                    false,
                    false,
                    PE_ASPECT_ORIGINAL,
                    &mut o.cx,
                    &mut o.cy,
                    &mut o.w,
                    &mut o.h,
                    &mut o.angle,
                    &mut o.turns,
                    &mut o.aspect,
                )
            },
            0
        );
        assert_eq!(o.aspect, PE_ASPECT_ORIGINAL);

        let g = geometry_of(s);
        assert_eq!(g["aspect"], "original");
        assert!(g["aspect_w"].is_null(), "original carries no ratio");

        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_geometry_with_nothing_open_is_refused_and_a_null_handle_is_told_apart() {
        // Nothing open: -2, with a message to go with it.
        let s = pe_session_new();
        assert_eq!(
            unsafe {
                pe_session_set_geometry(
                    s,
                    0.0,
                    0.0,
                    1.0,
                    1.0,
                    0.0,
                    0,
                    false,
                    false,
                    0.0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
            -2
        );
        assert_eq!(unsafe { pe_session_reset_geometry(s) }, -2);
        let err = unsafe { pe_session_last_error(s) };
        assert!(
            !err.is_null(),
            "-2 without a message is a bug report nobody can write"
        );
        unsafe { pe_string_free(err) };
        unsafe { pe_session_free(s) };

        // A null handle: -1, and nowhere to have recorded anything.
        let mut o = Out::default();
        assert_eq!(
            unsafe {
                pe_session_set_geometry(
                    ptr::null_mut(),
                    0.0,
                    0.0,
                    1.0,
                    1.0,
                    0.0,
                    0,
                    false,
                    false,
                    0.0,
                    &mut o.cx,
                    &mut o.cy,
                    &mut o.w,
                    &mut o.h,
                    &mut o.angle,
                    &mut o.turns,
                    &mut o.aspect,
                )
            },
            -1
        );
        assert_eq!(o.w, 0.0, "a refused call wrote to the caller's memory");
        assert_eq!(unsafe { pe_session_reset_geometry(ptr::null_mut()) }, -1);
    }

    /// The four out-parameters of the two crop-in-frame calls, read back.
    #[derive(Default)]
    struct Rect {
        u0: f32,
        v0: f32,
        u1: f32,
        v1: f32,
    }

    impl Rect {
        fn read(s: *mut PeSession) -> (i32, Rect) {
            let mut r = Rect::default();
            let code =
                unsafe { pe_session_crop_in_frame(s, &mut r.u0, &mut r.v0, &mut r.u1, &mut r.v1) };
            (code, r)
        }

        fn set(s: *mut PeSession, want: [f32; 4]) -> (i32, Rect) {
            let mut r = Rect::default();
            let code = unsafe {
                pe_session_set_crop_in_frame(
                    s, want[0], want[1], want[2], want[3], &mut r.u0, &mut r.v0, &mut r.u1,
                    &mut r.v1,
                )
            };
            (code, r)
        }

        fn is(&self, want: [f32; 4], slop: f32) -> bool {
            (self.u0 - want[0]).abs() < slop
                && (self.v0 - want[1]).abs() < slop
                && (self.u1 - want[2]).abs() < slop
                && (self.v1 - want[3]).abs() < slop
        }
    }

    /// The question the ABI could not answer before: where is the crop in the
    /// frame you are showing me. Closed, the crop is the frame; open, it is the
    /// middle half of the whole source.
    #[test]
    fn the_crop_crosses_as_a_rectangle_of_the_frame_being_shown() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        assert_eq!(
            unsafe {
                pe_session_set_geometry(
                    s,
                    0.0,
                    0.0,
                    0.5,
                    0.5,
                    0.0,
                    0,
                    false,
                    false,
                    0.0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
            0
        );

        let (code, r) = Rect::read(s);
        assert_eq!(code, 0);
        assert!(
            r.is([0.0, 0.0, 1.0, 1.0], 1e-3),
            "with the tool closed the crop does not fill the frame it is shown in"
        );

        assert_eq!(unsafe { pe_session_set_cropping(s, true) }, 0);
        let (code, r) = Rect::read(s);
        assert_eq!(code, 0);
        assert!(
            r.is([0.25, 0.25, 0.75, 0.75], 2e-3),
            "a centred half crop is not the middle of the whole source"
        );

        assert_eq!(unsafe { pe_session_set_cropping(s, false) }, 0);
        let (_, r) = Rect::read(s);
        assert!(
            r.is([0.0, 0.0, 1.0, 1.0], 1e-3),
            "closing the tool left the frame open"
        );

        unsafe { pe_session_free(s) };
    }

    /// And the correction contract, on this call as on `set_geometry`: what
    /// comes back is where the crop landed, not what was asked for.
    #[test]
    fn a_crop_set_in_the_frame_comes_back_corrected() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        assert_eq!(
            unsafe {
                pe_session_set_geometry(
                    s,
                    0.0,
                    0.0,
                    0.5,
                    0.5,
                    0.0,
                    0,
                    false,
                    false,
                    0.0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
            0
        );
        assert_eq!(unsafe { pe_session_set_cropping(s, true) }, 0);

        // Dragged off the top left corner of the frame, which is a crop the
        // renderer cannot produce.
        let (code, r) = Rect::set(s, [-0.4, -0.4, 0.1, 0.1]);
        assert_eq!(code, 0);
        assert!(
            r.u0 >= -1e-3 && r.v0 >= -1e-3,
            "the crop was left hanging off the frame: {} {}",
            r.u0,
            r.v0
        );
        assert!(
            r.is([0.0, 0.0, 0.5, 0.5], 2e-3),
            "slid back to the corner is [0, 0, 0.5, 0.5]; got {} {} {} {}",
            r.u0,
            r.v0,
            r.u1,
            r.v1
        );

        // And it is what the reading call now answers.
        let (_, again) = Rect::read(s);
        assert!(
            again.is([r.u0, r.v0, r.u1, r.v1], 1e-6),
            "the answer written back is not the answer that can be read back"
        );
        // It is an edit, so it is in the history like any other crop.
        let g = geometry_of(s);
        assert!(unsafe { pe_session_can_undo(s) });
        let width = g["size"][0].as_f64().unwrap() as f32;
        assert!(
            (width - 0.5).abs() < 2e-3,
            "the move resized the crop: {width}"
        );

        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_crop_in_the_frame_with_nothing_open_is_refused_and_a_null_handle_is_told_apart() {
        // Nothing open: -2, with a message. The flag itself is not refused —
        // it is a property of the window, not of a photograph.
        let s = pe_session_new();
        assert_eq!(unsafe { pe_session_set_cropping(s, true) }, 0);
        assert_eq!(Rect::read(s).0, -2);
        assert_eq!(Rect::set(s, [0.0, 0.0, 1.0, 1.0]).0, -2);
        let err = unsafe { pe_session_last_error(s) };
        assert!(
            !err.is_null(),
            "-2 without a message is a bug report nobody can write"
        );
        unsafe { pe_string_free(err) };
        unsafe { pe_session_free(s) };

        // A null handle: -1, and nothing written to the caller's memory.
        let mut r = Rect {
            u1: 1.0,
            ..Default::default()
        };
        assert_eq!(
            unsafe {
                pe_session_crop_in_frame(
                    ptr::null_mut(),
                    &mut r.u0,
                    &mut r.v0,
                    &mut r.u1,
                    &mut r.v1,
                )
            },
            -1
        );
        assert_eq!(
            unsafe {
                pe_session_set_crop_in_frame(
                    ptr::null_mut(),
                    0.0,
                    0.0,
                    1.0,
                    1.0,
                    &mut r.u0,
                    &mut r.v0,
                    &mut r.u1,
                    &mut r.v1,
                )
            },
            -1
        );
        assert_eq!(r.u1, 1.0, "a refused call wrote to the caller's memory");
        assert_eq!(
            unsafe { pe_session_set_cropping(ptr::null_mut(), true) },
            -1
        );
    }

    #[test]
    fn the_registry_crosses_the_boundary_whole() {
        let json = pe_registry_json();
        assert!(!json.is_null());
        let text = unsafe { CStr::from_ptr(json) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(json) };
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            parsed["effects"].as_array().unwrap().len(),
            pe_effects::all().len()
        );
    }

    #[test]
    fn a_scope_crosses_as_a_buffer_the_caller_owns() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        assert_eq!(unsafe { pe_session_scope_generation(s) }, 0);
        assert_eq!(unsafe { pe_session_measure(s, 64, 64) }, 0);
        assert!(unsafe { pe_session_scope_generation(s) } > 0);

        let (mut planes, mut w, mut h, mut total, mut peak) = (0u32, 0u32, 0u32, 0u32, 0u32);
        assert_eq!(
            unsafe {
                pe_session_scope_shape(
                    s,
                    PeScope::Histogram,
                    &mut planes,
                    &mut w,
                    &mut h,
                    &mut total,
                    &mut peak,
                )
            },
            0
        );
        assert_eq!((planes, w, h), (4, 256, 1));
        assert_eq!(total, 64 * 64, "every pixel counted exactly once");
        assert!(peak > 0);

        let n = (planes * w * h) as usize;
        let mut out = vec![0u32; n];
        assert_eq!(
            unsafe { pe_session_scope_data(s, PeScope::Histogram, out.as_mut_ptr(), n as u32) },
            n as i32
        );
        assert_eq!(out.iter().take(256).sum::<u32>(), 64 * 64, "the red plane");
        assert_eq!(out.iter().max().copied(), Some(peak));

        // Short is refused rather than truncated: a half-copied scope draws a
        // plausible picture of a frame that does not exist.
        assert_eq!(
            unsafe { pe_session_scope_data(s, PeScope::Histogram, out.as_mut_ptr(), 10) },
            -2
        );
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn every_scope_reports_a_shape_that_matches_what_it_copies() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        assert_eq!(unsafe { pe_session_measure(s, 64, 48) }, 0);
        for kind in [
            PeScope::Histogram,
            PeScope::LogHistogram,
            PeScope::ColourSpread,
            PeScope::Waveform,
            PeScope::Vectorscope,
            PeScope::WarperChromaticity,
            PeScope::WarperHueSat,
            PeScope::WarperChromaLuma,
        ] {
            let (mut planes, mut w, mut h) = (0u32, 0u32, 0u32);
            assert_eq!(
                unsafe {
                    pe_session_scope_shape(
                        s,
                        kind,
                        &mut planes,
                        &mut w,
                        &mut h,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    )
                },
                0
            );
            let n = (planes * w * h) as usize;
            assert!(n > 0);
            let mut out = vec![0u32; n];
            assert_eq!(
                unsafe { pe_session_scope_data(s, kind, out.as_mut_ptr(), n as u32) },
                n as i32,
                "{:?} copied a different number than it reported",
                kind as i32
            );
        }
        // The waveform's columns follow the measured width, not the image.
        let (mut planes, mut w, mut h) = (0u32, 0u32, 0u32);
        unsafe {
            pe_session_scope_shape(
                s,
                PeScope::Waveform,
                &mut planes,
                &mut w,
                &mut h,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!((planes, w, h), (4, 256, 64), "planes, levels, columns");
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn reading_a_scope_before_measuring_is_refused() {
        let s = pe_session_new();
        unsafe { pe_session_open_test_chart(s, 64, 64) };
        let mut out = [0u32; 8];
        assert_eq!(
            unsafe { pe_session_scope_data(s, PeScope::Histogram, out.as_mut_ptr(), 8) },
            -1
        );
        assert!(unsafe { pe_session_over_white_fraction(s) } < 0.0);
        unsafe { pe_session_free(s) };
    }

    // ---- the set ---------------------------------------------------------

    /// A real photograph on disc, because everything a set knows is about
    /// paths and a thumbnail has to be decoded from a file that exists.
    fn photo_at(dir: &std::path::Path, name: &str, width: u32, height: u32) -> std::path::PathBuf {
        let path = dir.join(name);
        pe_io::save_png(
            &pe_io::test_chart(width, height),
            &path,
            &pe_color::space::SRGB,
        )
        .expect("the temporary directory is writable");
        path
    }

    /// The JSON list `pe_session_open_paths` takes.
    fn paths_json(paths: &[&std::path::Path]) -> CString {
        let list: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        cstr(&serde_json::to_string(&list).unwrap())
    }

    /// One entry's path, as the shell reads it: string out, copied, freed.
    fn entry_path(s: *mut PeSession, index: u32) -> Option<String> {
        let p = unsafe { pe_session_entry_path(s, index) };
        if p.is_null() {
            return None;
        }
        let text = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(p) };
        Some(text)
    }

    /// The last failure's message, or `None` if the last call succeeded.
    fn last_error(s: *mut PeSession) -> Option<String> {
        let m = unsafe { pe_session_last_error(s) };
        if m.is_null() {
            return None;
        }
        let text = unsafe { CStr::from_ptr(m) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(m) };
        Some(text)
    }

    /// Poll until the worker has delivered entry `index`'s thumbnail, or given
    /// up on it. A real thread, so this waits to a deadline rather than
    /// sleeping for a guessed interval and hoping.
    fn wait_for_thumbnail(s: *mut PeSession, index: u32) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            unsafe { pe_session_collect_thumbnails(s) };
            let (mut has_thumb, mut failed) = (false, false);
            assert_eq!(
                unsafe {
                    pe_session_entry_flags(s, index, ptr::null_mut(), &mut failed, &mut has_thumb)
                },
                0
            );
            if has_thumb || failed {
                assert!(!failed, "the worker could not read a file just written");
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the thumbnail worker delivered nothing in thirty seconds"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn a_set_crosses_as_paths_and_an_index() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 96, 32);
        let c = photo_at(tmp.path(), "c.png", 32, 32);

        let s = pe_session_new();
        let support = cstr(&tmp.path().join("support").display().to_string());
        unsafe { pe_session_set_support_dir(s, support.as_ptr()) };
        let list = paths_json(&[&a, &b, &c]);
        assert_eq!(unsafe { pe_session_open_paths(s, list.as_ptr()) }, 0);

        assert_eq!(unsafe { pe_session_entry_count(s) }, 3);
        assert_eq!(unsafe { pe_session_current_entry(s) }, 0);
        assert_eq!(entry_path(s, 0), Some(a.display().to_string()));
        assert_eq!(entry_path(s, 2), Some(c.display().to_string()));
        assert_eq!(
            entry_path(s, 3),
            None,
            "an entry past the end is not a path"
        );

        // The document, not just the return code: the first photograph is the
        // one showing, and only it has been decoded.
        let snap = snapshot(s);
        assert_eq!(snap["name"], "a.png");
        assert_eq!(snap["width"], 64);
        assert_eq!(snap["height"], 64);

        // And focusing another moves both the index and the pixels.
        assert_eq!(unsafe { pe_session_focus(s, 1) }, 0);
        assert_eq!(unsafe { pe_session_current_entry(s) }, 1);
        let snap = snapshot(s);
        assert_eq!(snap["name"], "b.png");
        assert_eq!(
            snap["width"], 96,
            "the index moved but the photograph did not"
        );
        assert_eq!(snap["height"], 32);

        // Past the end is refused, and refused without moving.
        assert_eq!(unsafe { pe_session_focus(s, 3) }, -2);
        assert!(last_error(s).is_some(), "a refusal nobody can report");
        assert_eq!(unsafe { pe_session_current_entry(s) }, 1);
        assert_eq!(snapshot(s)["name"], "b.png");

        unsafe { pe_session_free(s) };
    }

    #[test]
    fn the_strip_is_told_which_photographs_have_been_edited() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);

        let s = pe_session_new();
        let support = cstr(&tmp.path().join("support").display().to_string());
        unsafe { pe_session_set_support_dir(s, support.as_ptr()) };
        let list = paths_json(&[&a, &b]);
        assert_eq!(unsafe { pe_session_open_paths(s, list.as_ptr()) }, 0);

        // Sharpen rather than exposure: exposure is one of the pinned rows a
        // fresh document already carries, so adding it proves nothing.
        let key = cstr("sharpen");
        let row = unsafe { pe_session_add_effect(s, key.as_ptr()) };
        assert!(row >= 0);
        let amount = cstr("amount");
        assert_eq!(
            unsafe { pe_session_set_float(s, row as u64, amount.as_ptr(), 1.5) },
            0
        );

        // The edit is only parked once it stops being the one on screen.
        assert_eq!(unsafe { pe_session_focus(s, 1) }, 0);

        let (mut edited, mut failed, mut has_thumb) = (false, true, true);
        assert_eq!(
            unsafe { pe_session_entry_flags(s, 0, &mut edited, &mut failed, &mut has_thumb) },
            0
        );
        assert!(edited, "the sharpened photograph is not marked as edited");
        assert!(!failed);
        assert!(!has_thumb, "a thumbnail arrived that was never asked for");

        let (mut edited, mut failed, mut has_thumb) = (true, true, true);
        assert_eq!(
            unsafe { pe_session_entry_flags(s, 1, &mut edited, &mut failed, &mut has_thumb) },
            0
        );
        assert!(!edited, "the untouched photograph is marked as edited");
        assert!(!failed);
        assert!(!has_thumb);

        // Every out-pointer may be null, and past the end writes nothing.
        assert_eq!(
            unsafe {
                pe_session_entry_flags(s, 0, ptr::null_mut(), ptr::null_mut(), ptr::null_mut())
            },
            0
        );
        let mut untouched = true;
        assert_eq!(
            unsafe {
                pe_session_entry_flags(s, 2, &mut untouched, ptr::null_mut(), ptr::null_mut())
            },
            -2
        );
        assert!(untouched, "a refused call wrote to an out-parameter anyway");

        // And the parked edit really is the sharpen, not just a flag.
        assert_eq!(unsafe { pe_session_focus(s, 0) }, 0);
        let snap = snapshot(s);
        let sharpen = snap["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["effect"] == "sharpen")
            .expect("the parked edit came back without its row");
        assert_eq!(sharpen["params"]["amount"]["v"], 1.5);

        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_thumbnail_crosses_as_a_buffer_the_caller_owns() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 320, 240);

        let s = pe_session_new();
        let list = paths_json(&[&a]);
        assert_eq!(unsafe { pe_session_open_paths(s, list.as_ptr()) }, 0);

        // Nothing arrives that was never asked for, and nothing to read yet.
        assert_eq!(unsafe { pe_session_collect_thumbnails(s) }, 0);
        let (mut w, mut h) = (0u32, 0u32);
        assert_eq!(
            unsafe { pe_session_thumbnail_shape(s, 0, &mut w, &mut h) },
            -2,
            "a thumbnail that has not arrived reported a shape"
        );
        let mut probe = [0u8; 4];
        assert_eq!(
            unsafe { pe_session_thumbnail_data(s, 0, probe.as_mut_ptr(), 4) },
            -1
        );

        assert_eq!(unsafe { pe_session_request_thumbnails(s, 0, 1) }, 0);
        wait_for_thumbnail(s, 0);

        assert_eq!(
            unsafe { pe_session_thumbnail_shape(s, 0, &mut w, &mut h) },
            0
        );
        assert_eq!(
            (w, h),
            (pe_session::library::THUMB_EDGE, 96),
            "128 on the long edge, and the short one in the photograph's own proportions"
        );

        let n = (w * h * 4) as usize;
        let mut out = vec![0u8; n];
        assert_eq!(
            unsafe { pe_session_thumbnail_data(s, 0, out.as_mut_ptr(), n as u32) },
            n as i32
        );
        // The pixels, not just the count: a test chart is not black, and every
        // pixel of it is opaque.
        let (pixels, rest) = out.as_chunks::<4>();
        assert!(rest.is_empty(), "a thumbnail is a whole number of pixels");
        assert!(
            pixels.iter().all(|px| px[3] == 255),
            "the alpha channel did not survive the copy"
        );
        assert!(
            pixels.iter().any(|px| px[0..3] != [0, 0, 0]),
            "the buffer came back as 64 KB of black"
        );

        // Short is refused rather than truncated: 64 KB of pixels with the
        // last rows missing is a plausible photograph that does not exist.
        let mut short = vec![0xABu8; n];
        assert_eq!(
            unsafe { pe_session_thumbnail_data(s, 0, short.as_mut_ptr(), (n - 1) as u32) },
            -2
        );
        assert!(
            short.iter().all(|b| *b == 0xAB),
            "a refused copy wrote into the buffer anyway"
        );

        // A null buffer and an index past the end are the same nothing.
        assert_eq!(
            unsafe { pe_session_thumbnail_data(s, 0, ptr::null_mut(), n as u32) },
            -1
        );
        assert_eq!(
            unsafe { pe_session_thumbnail_data(s, 1, out.as_mut_ptr(), n as u32) },
            -1
        );
        assert_eq!(
            unsafe { pe_session_thumbnail_shape(s, 1, &mut w, &mut h) },
            -2
        );

        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_range_asks_only_for_what_is_in_it() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 48);
        let b = photo_at(tmp.path(), "b.png", 64, 48);
        let c = photo_at(tmp.path(), "c.png", 64, 48);

        let s = pe_session_new();
        let list = paths_json(&[&a, &b, &c]);
        assert_eq!(unsafe { pe_session_open_paths(s, list.as_ptr()) }, 0);

        // An inverted range, and one entirely past the end, ask for nothing —
        // which is how a strip scrolled off its own set behaves.
        assert_eq!(unsafe { pe_session_request_thumbnails(s, 2, 1) }, 0);
        assert_eq!(unsafe { pe_session_request_thumbnails(s, 7, 9) }, 0);
        assert_eq!(unsafe { pe_session_collect_thumbnails(s) }, 0);

        assert_eq!(unsafe { pe_session_request_thumbnails(s, 1, 2) }, 0);
        wait_for_thumbnail(s, 1);
        assert_eq!(
            unsafe { pe_session_collect_thumbnails(s) },
            0,
            "something arrived twice"
        );

        // Only the one asked for. The other two are still paths.
        for (index, expected) in [(0u32, false), (1, true), (2, false)] {
            let mut has_thumb = !expected;
            assert_eq!(
                unsafe {
                    pe_session_entry_flags(
                        s,
                        index,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        &mut has_thumb,
                    )
                },
                0
            );
            assert_eq!(has_thumb, expected, "entry {index}");
        }
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_malformed_or_empty_list_is_refused_and_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 32, 32);

        let s = pe_session_new();
        let good = paths_json(&[&a]);
        assert_eq!(unsafe { pe_session_open_paths(s, good.as_ptr()) }, 0);
        let before = snapshot(s);

        // Not a C string at all.
        assert_eq!(unsafe { pe_session_open_paths(s, ptr::null()) }, -1);
        // Not JSON.
        assert_eq!(
            unsafe { pe_session_open_paths(s, cstr("{ not json").as_ptr()) },
            -1
        );
        let text = last_error(s).expect("a malformed list left no explanation");
        assert!(text.contains("array of paths"), "unhelpful message: {text}");
        // JSON, but not an array of strings.
        assert_eq!(
            unsafe { pe_session_open_paths(s, cstr("[1,2,3]").as_ptr()) },
            -1
        );
        assert_eq!(
            unsafe { pe_session_open_paths(s, cstr("\"/a.png\"").as_ptr()) },
            -1
        );
        // An array of no photographs at all: the session's own refusal, with a
        // message, rather than a set of nothing for every reader to cope with.
        assert_eq!(unsafe { pe_session_open_paths(s, cstr("[]").as_ptr()) }, -2);
        assert!(last_error(s).is_some(), "an empty set was refused silently");
        // A path that is not a photograph: refused, with the reason.
        let missing = paths_json(&[&tmp.path().join("nope.png")]);
        assert_eq!(unsafe { pe_session_open_paths(s, missing.as_ptr()) }, -2);
        let text = last_error(s).expect("a failed decode left no explanation");
        assert!(text.contains("nope.png"), "unhelpful message: {text}");

        // Through all of that the session is exactly where it started.
        assert_eq!(unsafe { pe_session_entry_count(s) }, 1);
        assert_eq!(unsafe { pe_session_current_entry(s) }, 0);
        let after = snapshot(s);
        assert_eq!(after["name"], before["name"]);
        assert_eq!(after["version"], before["version"], "a refusal edited");
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_session_with_no_set_answers_every_question_about_one() {
        // A `Library` is built around its paths and the support directory, so
        // it cannot exist before a set is opened — and the built-in chart is
        // not a file, so it is not a set of one either. Each of these has an
        // answer rather than a sentinel that means "null handle".
        for open_a_chart in [false, true] {
            let s = pe_session_new();
            if open_a_chart {
                assert_eq!(unsafe { pe_session_open_test_chart(s, 32, 32) }, 0);
            }

            assert_eq!(unsafe { pe_session_entry_count(s) }, 0);
            assert_eq!(unsafe { pe_session_current_entry(s) }, -2);
            assert_eq!(entry_path(s, 0), None);
            let mut untouched = true;
            assert_eq!(
                unsafe {
                    pe_session_entry_flags(s, 0, &mut untouched, ptr::null_mut(), ptr::null_mut())
                },
                -2
            );
            assert!(untouched);
            assert_eq!(unsafe { pe_session_focus(s, 0) }, -2);
            assert!(last_error(s).is_some());

            // Asking a session with no photographs for their thumbnails is
            // answered by there being none, not by a failure.
            assert_eq!(unsafe { pe_session_request_thumbnails(s, 0, 10) }, 0);
            assert_eq!(unsafe { pe_session_collect_thumbnails(s) }, 0);

            let (mut w, mut h) = (7u32, 7u32);
            assert_eq!(
                unsafe { pe_session_thumbnail_shape(s, 0, &mut w, &mut h) },
                -2
            );
            assert_eq!((w, h), (7, 7), "a refused shape wrote anyway");
            let mut out = [0u8; 8];
            assert_eq!(
                unsafe { pe_session_thumbnail_data(s, 0, out.as_mut_ptr(), 8) },
                -1
            );
            unsafe { pe_session_free(s) };
        }
    }

    #[test]
    fn opening_one_photograph_the_old_way_is_still_a_set_of_one() {
        // `pe_session_open_path` predates the set and must keep behaving as it
        // did, which now means: a set of exactly one, focused on it.
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 48, 48);

        let s = pe_session_new();
        let path = cstr(&a.display().to_string());
        assert_eq!(unsafe { pe_session_open_path(s, path.as_ptr()) }, 0);
        assert_eq!(unsafe { pe_session_entry_count(s) }, 1);
        assert_eq!(unsafe { pe_session_current_entry(s) }, 0);
        assert_eq!(entry_path(s, 0), Some(a.display().to_string()));
        // Focusing the one already showing is not a reason to fail.
        assert_eq!(unsafe { pe_session_focus(s, 0) }, 0);
        assert_eq!(unsafe { pe_session_focus(s, 1) }, -2);

        // And opening the chart afterwards puts the set away, because a chart
        // has no file for a strip to be a strip of.
        assert_eq!(unsafe { pe_session_open_test_chart(s, 32, 32) }, 0);
        assert_eq!(unsafe { pe_session_entry_count(s) }, 0);
        assert_eq!(unsafe { pe_session_current_entry(s) }, -2);
        unsafe { pe_session_free(s) };
    }

    // ---- a batch of them -------------------------------------------------

    /// A session over a set on disc, writing PNGs into a folder beside it.
    ///
    /// PNG rather than JPEG because the assertions are about files arriving
    /// under the names the format decides, and there is nothing here that a
    /// lossy encode would make truer.
    fn batch_session(tmp: &std::path::Path, paths: &[&std::path::Path]) -> *mut PeSession {
        let s = pe_session_new();
        let support = cstr(&tmp.join("support").display().to_string());
        assert_eq!(
            unsafe { pe_session_set_support_dir(s, support.as_ptr()) },
            0
        );
        let list = paths_json(paths);
        assert_eq!(unsafe { pe_session_open_paths(s, list.as_ptr()) }, 0);
        let png = cstr("png");
        assert_eq!(unsafe { pe_session_set_export(s, png.as_ptr(), 95) }, 0);
        s
    }

    /// The three counts as a shell reads them: `None` when no run has been
    /// started, and a panic for anything that is not one of the two answers
    /// the function documents.
    fn batch_progress(s: *mut PeSession) -> Option<(u32, u32, u32)> {
        let (mut done, mut failed, mut total) = (u32::MAX, u32::MAX, u32::MAX);
        match unsafe { pe_session_batch_progress(s, &mut done, &mut failed, &mut total) } {
            0 => Some((done, failed, total)),
            -2 => None,
            other => panic!("a progress read answered {other}"),
        }
    }

    /// Step to the end the way a frame loop does — `> 0` is more to do — and
    /// say how many steps it took. Anything negative is a refusal and ends the
    /// test, not the loop.
    fn run_batch(s: *mut PeSession) -> usize {
        let mut steps = 0;
        loop {
            let more = unsafe { pe_session_step_batch(s) };
            assert!(more >= 0, "the run was refused: {:?}", last_error(s));
            steps += 1;
            assert!(steps < 64, "a batch that will not finish");
            if more == 0 {
                return steps;
            }
        }
    }

    #[test]
    fn a_batch_crosses_and_writes_one_file_per_photograph() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);
        let c = photo_at(tmp.path(), "c.png", 64, 64);
        let out = tmp.path().join("out");
        std::fs::create_dir(&out).expect("the temporary directory is writable");

        let s = batch_session(tmp.path(), &[&a, &b, &c]);
        assert_eq!(batch_progress(s), None, "a run nobody has started");

        let dir = cstr(&out.display().to_string());
        assert_eq!(unsafe { pe_session_start_batch(s, dir.as_ptr()) }, 0);
        assert_eq!(batch_progress(s), Some((0, 0, 3)), "nothing has run yet");

        assert_eq!(run_batch(s), 3, "one step per photograph, no more");

        // On disc, because that is the only thing that proves a batch ran: a
        // return code says a step happened, not that a file arrived.
        for name in ["a_KROMA.png", "b_KROMA.png", "c_KROMA.png"] {
            assert!(out.join(name).exists(), "{name} was not written");
        }

        // And the summary is still readable *after* the step that answered 0,
        // which is when a shell reports it. A finished run that answered
        // "nothing running" here would be indistinguishable from one that
        // crashed on its first photograph.
        assert_eq!(batch_progress(s), Some((3, 0, 3)));
        assert_eq!(
            unsafe { pe_session_step_batch(s) },
            0,
            "stepping a finished run found more to do"
        );

        unsafe { pe_session_free(s) };
    }

    /// One photograph that cannot be written is a count, not a refusal: `-2`
    /// would stop the run, and the other sixty-five are still worth writing.
    #[test]
    fn a_collision_crosses_as_a_failed_count_rather_than_a_refusal() {
        // Contrived deliberately, and not far-fetched: a folder exported once
        // already, exported into again.
        let tmp = tempfile::tempdir().unwrap();
        let sunset = photo_at(tmp.path(), "sunset.png", 64, 64);
        let already = photo_at(tmp.path(), "sunset_KROMA.png", 64, 64);
        let untouched = std::fs::read(&already).unwrap();

        let s = batch_session(tmp.path(), &[&sunset, &already]);
        let dir = cstr(&tmp.path().display().to_string());
        assert_eq!(unsafe { pe_session_start_batch(s, dir.as_ptr()) }, 0);

        // `run_batch` asserts that no step went negative, which is half of
        // what this test is about.
        assert_eq!(run_batch(s), 2, "the collision abandoned the run");

        assert_eq!(
            std::fs::read(&already).unwrap(),
            untouched,
            "an original was written over"
        );
        assert_eq!(
            batch_progress(s),
            Some((1, 1, 2)),
            "the collision was not counted, or it was counted as a success"
        );
        assert!(
            tmp.path().join("sunset_KROMA_KROMA.png").exists(),
            "one collision abandoned the photograph after it"
        );
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn cancelling_keeps_what_was_written_and_puts_the_counts_away() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);
        let c = photo_at(tmp.path(), "c.png", 64, 64);
        let out = tmp.path().join("out");
        std::fs::create_dir(&out).expect("the temporary directory is writable");

        let s = batch_session(tmp.path(), &[&a, &b, &c]);
        let dir = cstr(&out.display().to_string());
        assert_eq!(unsafe { pe_session_start_batch(s, dir.as_ptr()) }, 0);
        assert_eq!(
            unsafe { pe_session_step_batch(s) },
            1,
            "two photographs left is more to do"
        );
        assert_eq!(batch_progress(s), Some((1, 0, 3)));

        assert_eq!(unsafe { pe_session_cancel_batch(s) }, 0);
        assert_eq!(batch_progress(s), None, "a cancelled run is not a run");
        assert!(
            out.join("a_KROMA.png").exists(),
            "cancelling took back what had already been written"
        );
        assert!(
            !out.join("b_KROMA.png").exists(),
            "the run carried on past the cancel"
        );

        // A step afterwards is 0 — there is nothing more to do — rather than a
        // refusal, and cancelling again is a no-op rather than a failure.
        assert_eq!(unsafe { pe_session_step_batch(s) }, 0);
        assert_eq!(unsafe { pe_session_cancel_batch(s) }, 0);
        assert!(
            !out.join("b_KROMA.png").exists(),
            "a step after a cancel restarted the run"
        );
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_batch_with_no_set_is_refused_and_told_apart_from_never_arriving() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = cstr(&tmp.path().display().to_string());

        // The built-in chart is not a set of one: there is no file for a run
        // to be a run over.
        for open_a_chart in [false, true] {
            let s = pe_session_new();
            if open_a_chart {
                assert_eq!(unsafe { pe_session_open_test_chart(s, 32, 32) }, 0);
            }

            // `-1`: the request never reached the session, and there is no
            // message because nothing looked at it.
            assert_eq!(unsafe { pe_session_start_batch(s, ptr::null()) }, -1);
            assert!(
                last_error(s).is_none(),
                "a request that never arrived left a message behind"
            );

            // `-2`: the session looked at it and refused, and said why.
            assert_eq!(unsafe { pe_session_start_batch(s, dir.as_ptr()) }, -2);
            assert!(last_error(s).is_some(), "a refusal nobody can report");

            // Nothing is running afterwards, and a refused read writes nothing
            // over the caller's variables.
            let mut done = 7u32;
            assert_eq!(
                unsafe {
                    pe_session_batch_progress(s, &mut done, ptr::null_mut(), ptr::null_mut())
                },
                -2
            );
            assert_eq!(done, 7, "a refused progress read wrote anyway");
            assert_eq!(
                unsafe { pe_session_step_batch(s) },
                0,
                "a step with no run is nothing to do, not a refusal"
            );
            unsafe { pe_session_free(s) };
        }
    }

    // ---- comparing -------------------------------------------------------

    /// The frame the comparison tests read back. Small: every assertion below
    /// is about bands of it, not about detail.
    const CW: u32 = 64;
    const CH: u32 = 64;

    /// A chart with an edit that is obvious in bytes — three stops — put there
    /// the way a shell would, one call across the boundary at a time.
    fn graded_session() -> *mut PeSession {
        let s = pe_session_new();
        assert_eq!(unsafe { pe_session_open_test_chart(s, CW, CH) }, 0);
        let exposure = cstr("exposure");
        let row = unsafe { pe_session_add_effect(s, exposure.as_ptr()) };
        assert!(row > 0, "exposure is a registered effect: {row}");
        let ev = cstr("ev");
        assert_eq!(
            unsafe { pe_session_set_float(s, row as u64, ev.as_ptr(), 3.0) },
            0
        );
        s
    }

    /// The composited frame, as bytes.
    ///
    /// The one thing here that does not cross the ABI, because there is
    /// nothing to cross it with and there should not be: a shell gets its
    /// pixels by attaching a layer and presenting to it, which needs a screen.
    /// This is the same offscreen render `pe_session_measure` bins, and it is
    /// the only way to assert that a comparison was *composited* rather than
    /// merely accepted — which is the whole of what these two functions are
    /// for.
    fn frame(s: *mut PeSession) -> Vec<u8> {
        unsafe { &mut *s }
            .inner
            .render_offscreen(CW, CH)
            .expect("the chart renders")
    }

    /// Columns `from..to` of a `CW`-wide RGBA8 frame, every row of them.
    fn cols(pixels: &[u8], from: u32, to: u32) -> Vec<u8> {
        let stride = CW as usize * 4;
        let (from, to) = (from as usize * 4, to as usize * 4);
        pixels
            .chunks_exact(stride)
            .flat_map(|row| row[from..to].iter().copied())
            .collect()
    }

    /// The mode and the seam, as a shell reads them back.
    fn compare_of(s: *mut PeSession) -> (u32, f32) {
        let (mut mode, mut wipe) = (u32::MAX, f32::NAN);
        assert_eq!(unsafe { pe_session_compare(s, &mut mode, &mut wipe) }, 0);
        (mode, wipe)
    }

    fn set_compare(s: *mut PeSession, mode: u32, wipe: f32) -> i32 {
        unsafe { pe_session_set_compare(s, mode, wipe) }
    }

    #[test]
    fn a_comparison_crosses_as_a_mode_and_a_fraction_and_is_composited() {
        let s = graded_session();
        let after = frame(s);

        // All of one picture at either end, and the two ends are not the same
        // picture: three stops apart.
        assert_eq!(set_compare(s, PE_COMPARE_WIPE, 1.0), 0);
        let ungraded = frame(s);
        assert_ne!(ungraded, after, "a wipe at everything is still the grade");
        assert_eq!(set_compare(s, PE_COMPARE_WIPE, 0.0), 0);
        assert_eq!(frame(s), after, "a wipe at nothing hid some of the grade");

        // And in between, one picture with a seam: each half is exactly what
        // the whole of it would have been there, which is what says the
        // fraction crossed as a position rather than as a flag.
        assert_eq!(set_compare(s, PE_COMPARE_WIPE, 0.5), 0);
        let wiped = frame(s);
        assert_eq!(
            cols(&wiped, 0, CW / 2),
            cols(&ungraded, 0, CW / 2),
            "the left of the seam is not the ungraded frame"
        );
        assert_eq!(
            cols(&wiped, CW / 2, CW),
            cols(&after, CW / 2, CW),
            "the right of the seam is not the graded frame"
        );

        // Side by side is a different shape, not a seam somewhere else: two
        // half-size pictures, so the corner is the viewer's surround, which no
        // picture reaches any more.
        assert_eq!(set_compare(s, PE_COMPARE_SIDE, 0.5), 0);
        let side = frame(s);
        assert_ne!(side, wiped, "2 drew the same frame as 1");
        assert!(
            side[0] < 60,
            "the top left corner is not the surround: {:?}",
            &side[..4]
        );

        // And 0 is the frame the session was drawing before any of this.
        assert_eq!(set_compare(s, PE_COMPARE_OFF, 0.5), 0);
        assert_eq!(frame(s), after, "off was still compositing something");
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_mode_this_abi_has_no_comparison_for_is_refused_rather_than_treated_as_off() {
        let s = graded_session();
        assert_eq!(set_compare(s, PE_COMPARE_WIPE, 0.25), 0);
        assert_eq!(last_error(s), None, "a call that worked left a message");
        let wiped = frame(s);

        // A shell that grows a fourth way of comparing and sends 3 is told.
        // Quietly showing no comparison would look, from the far side of the
        // boundary, exactly like the feature not working — and this return
        // value is the only thing that could have told it otherwise.
        for unknown in [3u32, 4, u32::MAX] {
            assert_eq!(set_compare(s, unknown, 0.5), -2, "{unknown} was accepted");
            let message = last_error(s).expect("a refusal nobody can report");
            assert!(
                message.contains(&unknown.to_string()),
                "the message does not say what was refused: {message}"
            );
            // Nothing moved, in the state or in the picture.
            assert_eq!(
                compare_of(s),
                (PE_COMPARE_WIPE, 0.25),
                "a refused mode changed the comparison"
            );
            assert_eq!(frame(s), wiped, "a refused mode changed the picture");
        }

        // The other sentinel means something else entirely: the request never
        // reached a session, so there is nowhere for a message to have been
        // written and nothing looked at it.
        assert_eq!(
            unsafe { pe_session_set_compare(ptr::null_mut(), PE_COMPARE_WIPE, 0.5) },
            -1
        );
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn the_seam_survives_a_round_trip_through_off() {
        let s = graded_session();
        let after = frame(s);

        assert_eq!(set_compare(s, PE_COMPARE_WIPE, 0.25), 0);
        let quarter = frame(s);
        assert_ne!(quarter, after, "a quarter of a wipe drew no before");
        assert_eq!(compare_of(s), (PE_COMPARE_WIPE, 0.25));

        // One button, three presses, back where it started — each press
        // handing back the fraction it just read rather than a zero of its
        // own, which is the whole reason the fraction is readable.
        for mode in [PE_COMPARE_SIDE, PE_COMPARE_OFF] {
            let (_, wipe) = compare_of(s);
            assert_eq!(set_compare(s, mode, wipe), 0);
        }
        assert_eq!(
            compare_of(s),
            (PE_COMPARE_OFF, 0.25),
            "the seam was not kept for next time"
        );
        assert_eq!(frame(s), after, "off drew a before anyway");

        let (_, wipe) = compare_of(s);
        assert_eq!(set_compare(s, PE_COMPARE_WIPE, wipe), 0);
        assert_eq!(
            frame(s),
            quarter,
            "the seam came back somewhere other than where it was left"
        );
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_seam_dragged_off_the_frame_lands_on_the_edge_rather_than_being_refused() {
        // Both ends are places a user drags to, and past either end is what
        // dragging against the edge of a window produces. Each of these
        // renders as well as reading back: past either end the scissor would
        // be negative or wider than the target, and wgpu rejects both rather
        // than shrugging.
        let s = graded_session();
        for (asked, want) in [(-3.0_f32, 0.0_f32), (4.0, 1.0), (f32::NAN, 0.0)] {
            assert_eq!(set_compare(s, PE_COMPARE_WIPE, asked), 0);
            let (mode, wipe) = compare_of(s);
            assert_eq!((mode, wipe), (PE_COMPARE_WIPE, want), "a wipe of {asked}");
            frame(s);
        }
        unsafe { pe_session_free(s) };
    }

    #[test]
    fn a_comparison_is_a_property_of_the_window_and_needs_no_out_parameters() {
        // Nothing open, and every question about a comparison still has an
        // answer: the same bargain `pe_session_set_cropping` makes, because a
        // window outlives whichever photograph is in it.
        let s = pe_session_new();
        assert_eq!(
            compare_of(s),
            (PE_COMPARE_OFF, 0.5),
            "a fresh session is comparing something, or starts its first wipe \
             somewhere other than the middle"
        );
        assert_eq!(set_compare(s, PE_COMPARE_SIDE, 0.75), 0);
        assert_eq!(last_error(s), None, "a call that worked left a message");
        assert_eq!(compare_of(s), (PE_COMPARE_SIDE, 0.75));

        // Either half may be the only half wanted, and neither may be.
        let mut mode = u32::MAX;
        assert_eq!(
            unsafe { pe_session_compare(s, &mut mode, ptr::null_mut()) },
            0
        );
        assert_eq!(mode, PE_COMPARE_SIDE);
        let mut wipe = f32::NAN;
        assert_eq!(
            unsafe { pe_session_compare(s, ptr::null_mut(), &mut wipe) },
            0
        );
        assert_eq!(wipe, 0.75);
        assert_eq!(
            unsafe { pe_session_compare(s, ptr::null_mut(), ptr::null_mut()) },
            0
        );
        unsafe { pe_session_free(s) };
    }

    // ---- what is remembered between runs ---------------------------------

    /// A JSON string out of the ABI, as the shell reads it: parsed, freed.
    fn json_out(p: *mut c_char) -> Option<serde_json::Value> {
        if p.is_null() {
            return None;
        }
        let text = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_owned();
        unsafe { pe_string_free(p) };
        Some(serde_json::from_str(&text).unwrap())
    }

    fn favourites(s: *mut PeSession) -> Vec<String> {
        let json = json_out(unsafe { pe_session_favourites_json(s) }).expect("a list, not null");
        serde_json::from_value(json).expect("a JSON array of keys")
    }

    fn remembered(s: *mut PeSession) -> (Vec<String>, usize) {
        let json = json_out(unsafe { pe_session_remembered_session_json(s) })
            .expect("an object, not null");
        (
            serde_json::from_value(json["paths"].clone()).expect("paths is an array of strings"),
            serde_json::from_value(json["index"].clone()).expect("index is a number"),
        )
    }

    /// A star is written down when it is made, and the next session reads it —
    /// which is the whole reason any of this crosses the boundary.
    #[test]
    fn a_star_crosses_and_the_next_session_finds_it() {
        let tmp = tempfile::tempdir().unwrap();
        let support = cstr(&tmp.path().join("support").display().to_string());
        let grain = cstr("grain");

        let s = pe_session_new();
        assert_eq!(
            unsafe { pe_session_set_support_dir(s, support.as_ptr()) },
            0
        );
        assert_eq!(unsafe { pe_session_is_favourite(s, grain.as_ptr()) }, 0);
        assert_eq!(favourites(s), Vec::<String>::new());

        assert_eq!(unsafe { pe_session_toggle_favourite(s, grain.as_ptr()) }, 0);
        assert_eq!(unsafe { pe_session_is_favourite(s, grain.as_ptr()) }, 1);
        assert_eq!(favourites(s), ["grain"]);
        unsafe { pe_session_free(s) };

        // The next launch, which is the only thing this feature is for.
        let again = pe_session_new();
        assert_eq!(
            unsafe { pe_session_set_support_dir(again, support.as_ptr()) },
            0
        );
        assert_eq!(
            unsafe { pe_session_is_favourite(again, grain.as_ptr()) },
            1,
            "the star did not survive the session it was made in"
        );
        assert_eq!(favourites(again), ["grain"]);

        // And the same gesture takes it off again.
        assert_eq!(
            unsafe { pe_session_toggle_favourite(again, grain.as_ptr()) },
            0
        );
        assert_eq!(unsafe { pe_session_is_favourite(again, grain.as_ptr()) }, 0);
        assert_eq!(favourites(again), Vec::<String>::new());
        unsafe { pe_session_free(again) };
    }

    /// The set that was open comes back with which one was showing, and the
    /// shell can hand the paths straight back to `pe_session_open_paths`.
    #[test]
    fn the_set_that_was_open_crosses_as_paths_and_an_index() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);
        let c = photo_at(tmp.path(), "c.png", 64, 64);
        let support = cstr(&tmp.path().join("support").display().to_string());

        let s = pe_session_new();
        assert_eq!(
            unsafe { pe_session_set_support_dir(s, support.as_ptr()) },
            0
        );
        assert_eq!(
            remembered(s),
            (Vec::new(), 0),
            "a first run remembers a set it never had"
        );

        let list = paths_json(&[&a, &b, &c]);
        assert_eq!(unsafe { pe_session_open_paths(s, list.as_ptr()) }, 0);
        assert_eq!(unsafe { pe_session_focus(s, 2) }, 0);
        unsafe { pe_session_free(s) };

        let again = pe_session_new();
        assert_eq!(
            unsafe { pe_session_set_support_dir(again, support.as_ptr()) },
            0
        );
        let (paths, index) = remembered(again);
        assert_eq!(
            paths,
            [
                a.display().to_string(),
                b.display().to_string(),
                c.display().to_string()
            ]
        );
        assert_eq!(paths[index], c.display().to_string());

        // The round trip the shell actually makes: the list comes back out and
        // goes straight in again.
        let back = cstr(&serde_json::to_string(&paths).unwrap());
        assert_eq!(unsafe { pe_session_open_paths(again, back.as_ptr()) }, 0);
        assert_eq!(unsafe { pe_session_entry_count(again) }, 3);
        assert_eq!(unsafe { pe_session_focus(again, index as u32) }, 0);
        assert_eq!(snapshot(again)["name"], "c.png");
        unsafe { pe_session_free(again) };
    }

    /// A photograph that has gone is left out, and the one that was showing is
    /// still the one that comes back — by name, not by the position it used to
    /// hold.
    #[test]
    fn a_photograph_that_has_gone_is_left_out_of_what_comes_back() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let b = photo_at(tmp.path(), "b.png", 64, 64);
        let c = photo_at(tmp.path(), "c.png", 64, 64);
        let support = cstr(&tmp.path().join("support").display().to_string());

        let s = pe_session_new();
        assert_eq!(
            unsafe { pe_session_set_support_dir(s, support.as_ptr()) },
            0
        );
        let list = paths_json(&[&a, &b, &c]);
        assert_eq!(unsafe { pe_session_open_paths(s, list.as_ptr()) }, 0);
        assert_eq!(unsafe { pe_session_focus(s, 2) }, 0);
        unsafe { pe_session_free(s) };

        // Deleted between the two runs, from in front of the one that was
        // showing — which renumbers everything after it.
        std::fs::remove_file(&a).unwrap();

        let again = pe_session_new();
        assert_eq!(
            unsafe { pe_session_set_support_dir(again, support.as_ptr()) },
            0
        );
        let (paths, index) = remembered(again);
        assert_eq!(paths, [b.display().to_string(), c.display().to_string()]);
        assert_eq!(
            paths[index],
            c.display().to_string(),
            "the deletion slid the answer onto the wrong photograph"
        );
        unsafe { pe_session_free(again) };
    }

    /// Everything the last run had is gone: an empty list, an index of nought,
    /// and `pe_session_open_paths` refusing it rather than a set of none.
    #[test]
    fn a_set_that_has_entirely_gone_comes_back_empty_and_is_refused_on_the_way_in() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let support = cstr(&tmp.path().join("support").display().to_string());

        let s = pe_session_new();
        assert_eq!(
            unsafe { pe_session_set_support_dir(s, support.as_ptr()) },
            0
        );
        let list = paths_json(&[&a]);
        assert_eq!(unsafe { pe_session_open_paths(s, list.as_ptr()) }, 0);
        unsafe { pe_session_free(s) };

        std::fs::remove_file(&a).unwrap();

        let again = pe_session_new();
        assert_eq!(
            unsafe { pe_session_set_support_dir(again, support.as_ptr()) },
            0
        );
        let (paths, index) = remembered(again);
        assert!(paths.is_empty());
        assert_eq!(index, 0);

        let empty = cstr("[]");
        assert_eq!(
            unsafe { pe_session_open_paths(again, empty.as_ptr()) },
            -2,
            "a set of no photographs was opened"
        );
        assert!(last_error(again).is_some(), "a refusal with no reason");
        unsafe { pe_session_free(again) };
    }

    /// A remembered photograph that is *there* and will not *decode* is the
    /// case the engine does not answer, and the shells do. `is_file` is all
    /// `remembered_session` can say, so the path comes back and opening it is
    /// `-2` — which is what `apps/windows/src/main.rs`'s `open_something` and
    /// `SessionStore.openRemembered` are each built on.
    #[test]
    fn a_remembered_photograph_that_will_not_decode_still_comes_back() {
        let tmp = tempfile::tempdir().unwrap();
        let a = photo_at(tmp.path(), "a.png", 64, 64);
        let support = cstr(&tmp.path().join("support").display().to_string());

        let s = pe_session_new();
        assert_eq!(
            unsafe { pe_session_set_support_dir(s, support.as_ptr()) },
            0
        );
        let list = paths_json(&[&a]);
        assert_eq!(unsafe { pe_session_open_paths(s, list.as_ptr()) }, 0);
        unsafe { pe_session_free(s) };

        // Still there, still named the same, and no longer a photograph.
        std::fs::write(&a, b"not a PNG any more").unwrap();

        let again = pe_session_new();
        assert_eq!(
            unsafe { pe_session_set_support_dir(again, support.as_ptr()) },
            0
        );
        let (paths, _) = remembered(again);
        assert_eq!(
            paths,
            [a.display().to_string()],
            "a file that exists was dropped for not decoding, which is not \
             something the engine can know"
        );

        let back = cstr(&serde_json::to_string(&paths).unwrap());
        assert_eq!(
            unsafe { pe_session_open_paths(again, back.as_ptr()) },
            -2,
            "a file of rubbish decoded"
        );
        assert!(
            last_error(again).is_some(),
            "the shell has to be able to say what went wrong"
        );
        unsafe { pe_session_free(again) };
    }

    /// The sentinels, and the rule they follow: `-1` never reached a session.
    #[test]
    fn what_is_remembered_answers_a_null_handle_without_touching_it() {
        let grain = cstr("grain");
        assert_eq!(
            unsafe { pe_session_is_favourite(ptr::null_mut(), grain.as_ptr()) },
            -1
        );
        assert_eq!(
            unsafe { pe_session_toggle_favourite(ptr::null_mut(), grain.as_ptr()) },
            -1
        );
        assert!(unsafe { pe_session_favourites_json(ptr::null_mut()) }.is_null());
        assert!(unsafe { pe_session_remembered_session_json(ptr::null_mut()) }.is_null());

        // And a null key is the same failure with a session behind it: nothing
        // is starred and nothing is written.
        let s = pe_session_new();
        assert_eq!(unsafe { pe_session_is_favourite(s, ptr::null()) }, -1);
        assert_eq!(unsafe { pe_session_toggle_favourite(s, ptr::null()) }, -1);
        assert_eq!(favourites(s), Vec::<String>::new());

        // A session with nowhere to write still answers both questions, which
        // is the arrangement `Settings` is built on: no support directory is
        // not an error, it is a run that does not outlive itself.
        assert_eq!(unsafe { pe_session_toggle_favourite(s, grain.as_ptr()) }, 0);
        assert_eq!(unsafe { pe_session_is_favourite(s, grain.as_ptr()) }, 1);
        assert_eq!(remembered(s), (Vec::new(), 0));
        unsafe { pe_session_free(s) };
    }
}
