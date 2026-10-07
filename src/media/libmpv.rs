use libloading::{Library, Symbol};
use std::{
    ffi::{CStr, c_char, c_int},
    path::PathBuf,
};

type MpvClientApiVersion = unsafe extern "C" fn() -> u64;
type MpvCreate = unsafe extern "C" fn() -> *mut std::ffi::c_void;
type MpvInitialize = unsafe extern "C" fn(*mut std::ffi::c_void) -> c_int;
type MpvTerminateDestroy = unsafe extern "C" fn(*mut std::ffi::c_void);
type MpvErrorString = unsafe extern "C" fn(c_int) -> *const c_char;

pub struct LibMpv {
    _library: Library,
}

impl LibMpv {
    pub fn smoke_test() -> Result<String, String> {
        let dll_path = Self::dll_path()?;

        // SAFETY:
        // We control the DLL path and immediately resolve known libmpv
        // C API symbols from the verified vendored DLL.
        let library = unsafe {
            Library::new(&dll_path)
                .map_err(|e| format!("Could not load {}: {e}", dll_path.display()))?
        };

        unsafe {
            let api_version: Symbol<MpvClientApiVersion> = library
                .get(b"mpv_client_api_version\0")
                .map_err(|e| format!("Missing mpv_client_api_version: {e}"))?;

            let mpv_create: Symbol<MpvCreate> = library
                .get(b"mpv_create\0")
                .map_err(|e| format!("Missing mpv_create: {e}"))?;

            let mpv_initialize: Symbol<MpvInitialize> = library
                .get(b"mpv_initialize\0")
                .map_err(|e| format!("Missing mpv_initialize: {e}"))?;

            let mpv_terminate_destroy: Symbol<MpvTerminateDestroy> = library
                .get(b"mpv_terminate_destroy\0")
                .map_err(|e| format!("Missing mpv_terminate_destroy: {e}"))?;

            let mpv_error_string: Symbol<MpvErrorString> = library
                .get(b"mpv_error_string\0")
                .map_err(|e| format!("Missing mpv_error_string: {e}"))?;

            let version = api_version();

            let major = version >> 16;
            let minor = version & 0xffff;

            let handle = mpv_create();

            if handle.is_null() {
                return Err("mpv_create() returned NULL.".into());
            }

            let result = mpv_initialize(handle);

            if result < 0 {
                let error_ptr = mpv_error_string(result);

                let error = if error_ptr.is_null() {
                    format!("error code {result}")
                } else {
                    CStr::from_ptr(error_ptr).to_string_lossy().into_owned()
                };

                mpv_terminate_destroy(handle);

                return Err(format!("mpv_initialize() failed: {error}"));
            }

            mpv_terminate_destroy(handle);

            Ok(format!(
                "libmpv loaded successfully - client API {}.{}",
                major, minor
            ))
        }
    }

    fn dll_path() -> Result<PathBuf, String> {
        // During cargo run, CARGO_MANIFEST_DIR is our project root.
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("vendor")
            .join("mpv")
            .join("libmpv-2.dll");

        if !path.exists() {
            return Err(format!("Vendored libmpv DLL not found: {}", path.display()));
        }

        Ok(path)
    }
}
