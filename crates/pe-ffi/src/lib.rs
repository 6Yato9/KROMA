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
}
