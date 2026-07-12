//! C FFI for MATBOX engine.
//! Exposes a simple C API: create/destroy an engine, eval a string, get last result.

use std::ffi::CStr;
use std::os::raw::c_char;

use matbox_core::Engine;

/// Opaque handle to a MATBOX engine.
pub struct MatboxEngine(Engine);

/// Create a new MATBOX engine instance. Returns opaque pointer.
/// # Safety
/// The returned pointer must be freed with `matbox_free`.
#[no_mangle]
pub unsafe extern "C" fn matbox_new() -> *mut MatboxEngine {
    Box::into_raw(Box::new(MatboxEngine(Engine::new())))
}

/// Destroy a MATBOX engine instance.
/// # Safety
/// `engine` must be a valid pointer returned by `matbox_new`.
#[no_mangle]
pub unsafe extern "C" fn matbox_free(engine: *mut MatboxEngine) {
    if !engine.is_null() {
        drop(Box::from_raw(engine));
    }
}

/// Evaluate a MATLAB expression string. Returns 0 on success, -1 on error.
/// The result is stored internally and can be retrieved with matbox_get_result.
/// # Safety
/// `engine` must be a valid pointer from `matbox_new`; `code` must be a valid C string.
#[no_mangle]
pub unsafe extern "C" fn matbox_eval(engine: *mut MatboxEngine, code: *const c_char) -> i32 {
    if engine.is_null() || code.is_null() { return -1; }
    let engine = &mut *engine;
    let cstr = CStr::from_ptr(code);
    let s = match cstr.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    match engine.0.eval(s) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::os::raw::c_char;

    #[test]
    fn test_create_free() {
        let e = unsafe { matbox_new() };
        assert!(!e.is_null());
        unsafe { matbox_free(e); }
    }

    #[test]
    fn test_eval_basic() {
        let e = unsafe { matbox_new() };
        let code = CString::new("1 + 2").unwrap();
        let r = unsafe { matbox_eval(e, code.as_ptr()) };
        assert_eq!(r, 0);
        unsafe { matbox_free(e); }
    }
}
