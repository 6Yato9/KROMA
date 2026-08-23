//! C ABI surface, for Swift.
//!
//! Stubbed at M0 deliberately: the *shape* of the boundary is what needs to
//! exist now, so the Mac app has something to link against and so the
//! engine/UI firewall is real rather than aspirational from day one. The
//! surface fills out as the Mac port approaches.
//!
//! Rules for anything added here:
//!
//! 1. Never expose a Rust type across the boundary — only opaque pointers,
//!    primitives, and UTF-8 C strings.
//! 2. Every allocation handed out has a matching `pe_*_free`.
//! 3. Never unwind across the boundary. Every entry point catches panics; a
//!    panic crossing into Swift is undefined behaviour, not a crash report.

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

/// Prove a `CAMetalLayer` can be drawn into. Fills it with orange.
///
/// Temporary — replaced by `pe_session_attach_layer` in Task 11. It exists so
/// the riskiest assumption in the port is tested before anything is built on
/// it.
///
/// # Safety
/// `layer` must be a live `CAMetalLayer`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pe_spike_attach_and_clear(
    layer: *mut std::ffi::c_void,
    width: u32,
    height: u32,
) -> i32 {
    guard(-99, || {
        if layer.is_null() {
            return -1;
        }
        let instance = pe_render::GpuContext::create_instance();
        // Adapter must come from the same instance the surface will belong to,
        // and must be told about the surface so a machine with more than one
        // GPU picks one that can present to it.
        let probe = match unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(layer))
        } {
            Ok(s) => s,
            Err(_) => return -2,
        };
        let Ok(gpu) = pollster::block_on(pe_render::GpuContext::from_instance(
            &instance,
            Some(&probe),
        )) else {
            return -3;
        };
        drop(probe);
        let attached = match unsafe {
            pe_session::Attached::new(&instance, &gpu.adapter, &gpu.device, layer, width, height)
        } {
            Ok(a) => a,
            Err(_) => return -4,
        };
        match attached.present_clear(&gpu.device, &gpu.queue, [0.85, 0.45, 0.10, 1.0]) {
            Ok(()) => 0,
            Err(_) => -5,
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
}
