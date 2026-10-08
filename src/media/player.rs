use libloading::Library;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    path::{Path, PathBuf},
    ptr,
    sync::Arc,
};

type MpvHandle = c_void;
type MpvRenderContext = c_void;

type MpvCreate = unsafe extern "C" fn() -> *mut MpvHandle;
type MpvInitialize = unsafe extern "C" fn(*mut MpvHandle) -> c_int;
type MpvTerminateDestroy = unsafe extern "C" fn(*mut MpvHandle);
type MpvCommand = unsafe extern "C" fn(*mut MpvHandle, *const *const c_char) -> c_int;
type MpvGetProperty =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, c_int, *mut c_void) -> c_int;
type MpvSetProperty =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, c_int, *mut c_void) -> c_int;
type MpvSetOptionString =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
type MpvErrorString = unsafe extern "C" fn(c_int) -> *const c_char;

type MpvRenderContextCreate =
    unsafe extern "C" fn(*mut *mut MpvRenderContext, *mut MpvHandle, *mut MpvRenderParam) -> c_int;

type MpvRenderContextRender =
    unsafe extern "C" fn(*mut MpvRenderContext, *mut MpvRenderParam) -> c_int;

type MpvRenderContextSetUpdateCallback = unsafe extern "C" fn(
    *mut MpvRenderContext,
    Option<unsafe extern "C" fn(*mut c_void)>,
    *mut c_void,
);

type MpvRenderContextFree = unsafe extern "C" fn(*mut MpvRenderContext);

const MPV_FORMAT_FLAG: c_int = 3;
const MPV_FORMAT_DOUBLE: c_int = 5;

const MPV_RENDER_PARAM_INVALID: c_int = 0;
const MPV_RENDER_PARAM_API_TYPE: c_int = 1;
const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const MPV_RENDER_PARAM_OPENGL_FBO: c_int = 3;
const MPV_RENDER_PARAM_FLIP_Y: c_int = 4;

#[repr(C)]
struct MpvRenderParam {
    type_: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct MpvOpenGlInitParams {
    get_proc_address: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void>,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct MpvOpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

type ProcAddressLoader = dyn Fn(&CStr) -> *const c_void + Send + Sync + 'static;

struct ProcAddressBridge {
    loader: Arc<ProcAddressLoader>,
}

struct RenderUpdateBridge {
    callback: Arc<dyn Fn() + Send + Sync + 'static>,
}

unsafe extern "C" fn mpv_get_proc_address(
    context: *mut c_void,
    name: *const c_char,
) -> *mut c_void {
    if context.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    let bridge = unsafe { &*(context as *const ProcAddressBridge) };
    let name = unsafe { CStr::from_ptr(name) };

    (bridge.loader)(name) as *mut c_void
}

unsafe extern "C" fn mpv_render_update(context: *mut c_void) {
    if context.is_null() {
        return;
    }

    let bridge = unsafe { &*(context as *const RenderUpdateBridge) };

    (bridge.callback)();
}

struct MpvApi {
    create: MpvCreate,
    initialize: MpvInitialize,
    terminate_destroy: MpvTerminateDestroy,
    command: MpvCommand,
    get_property: MpvGetProperty,
    set_property: MpvSetProperty,
    set_option_string: MpvSetOptionString,
    error_string: MpvErrorString,

    render_context_create: MpvRenderContextCreate,
    render_context_render: MpvRenderContextRender,
    render_context_set_update_callback: MpvRenderContextSetUpdateCallback,
    render_context_free: MpvRenderContextFree,
}

#[derive(Clone, Copy)]
pub struct MpvRenderHandle {
    context: usize,
    render: MpvRenderContextRender,
    error_string: MpvErrorString,
}

impl MpvRenderHandle {
    pub fn render_to_fbo(&self, framebuffer: i32, width: i32, height: i32) -> Result<(), String> {
        if self.context == 0 {
            return Err("libmpv render context is NULL.".into());
        }

        let mut fbo = MpvOpenGlFbo {
            fbo: framebuffer,
            w: width.max(1),
            h: height.max(1),
            internal_format: 0,
        };

        // OpenGL framebuffer coordinates are vertically inverted relative
        // to the image orientation expected by the egui surface.
        let mut flip_y: c_int = 1;

        let mut params = [
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_OPENGL_FBO,
                data: (&mut fbo as *mut MpvOpenGlFbo).cast(),
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_FLIP_Y,
                data: (&mut flip_y as *mut c_int).cast(),
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];

        let result =
            unsafe { (self.render)(self.context as *mut MpvRenderContext, params.as_mut_ptr()) };

        if result >= 0 {
            Ok(())
        } else {
            Err(format!(
                "mpv_render_context_render: {}",
                error_string(self.error_string, result)
            ))
        }
    }
}

pub struct MpvPlayer {
    handle: *mut MpvHandle,
    render_context: *mut MpvRenderContext,

    api: MpvApi,
    _library: Library,

    proc_address_bridge: Option<Box<ProcAddressBridge>>,
    render_update_bridge: Option<Box<RenderUpdateBridge>>,

    loaded: bool,
    initialized: bool,
}

impl MpvPlayer {
    pub fn new() -> Self {
        match Self::try_new() {
            Ok(player) => player,
            Err(error) => {
                panic!("Could not create libmpv context: {error}");
            }
        }
    }

    fn try_new() -> Result<Self, String> {
        let dll_path = Self::dll_path()?;

        let library = unsafe {
            Library::new(&dll_path)
                .map_err(|e| format!("Could not load {}: {e}", dll_path.display()))?
        };

        unsafe {
            let create: MpvCreate = *library
                .get::<MpvCreate>(b"mpv_create\0")
                .map_err(|e| format!("Missing mpv_create: {e}"))?;

            let initialize: MpvInitialize = *library
                .get::<MpvInitialize>(b"mpv_initialize\0")
                .map_err(|e| format!("Missing mpv_initialize: {e}"))?;

            let terminate_destroy: MpvTerminateDestroy = *library
                .get::<MpvTerminateDestroy>(b"mpv_terminate_destroy\0")
                .map_err(|e| format!("Missing mpv_terminate_destroy: {e}"))?;

            let command: MpvCommand = *library
                .get::<MpvCommand>(b"mpv_command\0")
                .map_err(|e| format!("Missing mpv_command: {e}"))?;

            let get_property: MpvGetProperty = *library
                .get::<MpvGetProperty>(b"mpv_get_property\0")
                .map_err(|e| format!("Missing mpv_get_property: {e}"))?;

            let set_property: MpvSetProperty = *library
                .get::<MpvSetProperty>(b"mpv_set_property\0")
                .map_err(|e| format!("Missing mpv_set_property: {e}"))?;

            let set_option_string: MpvSetOptionString = *library
                .get::<MpvSetOptionString>(b"mpv_set_option_string\0")
                .map_err(|e| format!("Missing mpv_set_option_string: {e}"))?;

            let error_string: MpvErrorString = *library
                .get::<MpvErrorString>(b"mpv_error_string\0")
                .map_err(|e| format!("Missing mpv_error_string: {e}"))?;

            let render_context_create: MpvRenderContextCreate = *library
                .get::<MpvRenderContextCreate>(b"mpv_render_context_create\0")
                .map_err(|e| format!("Missing mpv_render_context_create: {e}"))?;

            let render_context_render: MpvRenderContextRender = *library
                .get::<MpvRenderContextRender>(b"mpv_render_context_render\0")
                .map_err(|e| format!("Missing mpv_render_context_render: {e}"))?;

            let render_context_set_update_callback: MpvRenderContextSetUpdateCallback = *library
                .get::<MpvRenderContextSetUpdateCallback>(
                    b"mpv_render_context_set_update_callback\0",
                )
                .map_err(|e| format!("Missing mpv_render_context_set_update_callback: {e}"))?;

            let render_context_free: MpvRenderContextFree = *library
                .get::<MpvRenderContextFree>(b"mpv_render_context_free\0")
                .map_err(|e| format!("Missing mpv_render_context_free: {e}"))?;

            let api = MpvApi {
                create,
                initialize,
                terminate_destroy,
                command,
                get_property,
                set_property,
                set_option_string,
                error_string,
                render_context_create,
                render_context_render,
                render_context_set_update_callback,
                render_context_free,
            };

            let handle = (api.create)();

            if handle.is_null() {
                return Err("mpv_create() returned NULL.".into());
            }

            Ok(Self {
                handle,
                render_context: ptr::null_mut(),

                api,
                _library: library,

                proc_address_bridge: None,
                render_update_bridge: None,

                loaded: false,
                initialized: false,
            })
        }
    }

    pub fn initialize_render(
        &mut self,
        get_proc_address: Arc<ProcAddressLoader>,
        request_repaint: Arc<dyn Fn() + Send + Sync + 'static>,
    ) -> Result<(), String> {
        if self.handle.is_null() {
            return Err("libmpv handle is NULL.".into());
        }

        if self.initialized {
            return Ok(());
        }

        self.set_option_string("vo", "libmpv")?;
        self.set_option_string("aid", "no")?;
        self.set_option_string("keep-open", "yes")?;

        let result = unsafe { (self.api.initialize)(self.handle) };

        if result < 0 {
            return Err(format!(
                "mpv_initialize() failed: {}",
                Self::error_from_api(&self.api, result)
            ));
        }

        self.initialized = true;

        let mut proc_bridge = Box::new(ProcAddressBridge {
            loader: get_proc_address,
        });

        let mut gl_init = MpvOpenGlInitParams {
            get_proc_address: Some(mpv_get_proc_address),
            get_proc_address_ctx: (&mut *proc_bridge as *mut ProcAddressBridge).cast(),
        };

        let api_type = b"opengl\0";

        let mut params = [
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_API_TYPE,
                data: api_type.as_ptr() as *mut c_void,
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: (&mut gl_init as *mut MpvOpenGlInitParams).cast(),
            },
            MpvRenderParam {
                type_: MPV_RENDER_PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];

        let mut render_context: *mut MpvRenderContext = ptr::null_mut();

        let result = unsafe {
            (self.api.render_context_create)(&mut render_context, self.handle, params.as_mut_ptr())
        };

        if result < 0 || render_context.is_null() {
            return Err(format!(
                "mpv_render_context_create() failed: {}",
                Self::error_from_api(&self.api, result)
            ));
        }

        let mut update_bridge = Box::new(RenderUpdateBridge {
            callback: request_repaint,
        });

        unsafe {
            (self.api.render_context_set_update_callback)(
                render_context,
                Some(mpv_render_update),
                (&mut *update_bridge as *mut RenderUpdateBridge).cast(),
            );
        }

        self.render_context = render_context;
        self.proc_address_bridge = Some(proc_bridge);
        self.render_update_bridge = Some(update_bridge);

        Ok(())
    }

    pub fn render_handle(&self) -> Option<MpvRenderHandle> {
        if self.render_context.is_null() {
            return None;
        }

        Some(MpvRenderHandle {
            context: self.render_context as usize,
            render: self.api.render_context_render,
            error_string: self.api.error_string,
        })
    }

    pub fn load(&mut self, path: &Path) -> Result<(), String> {
        if !self.initialized || self.render_context.is_null() {
            return Err("libmpv render context has not been initialized.".into());
        }

        let path_string = path.to_string_lossy().into_owned();

        self.command(&["loadfile", &path_string, "replace"])?;

        self.loaded = true;

        self.set_paused(true)?;

        Ok(())
    }

    pub fn set_paused(&self, paused: bool) -> Result<(), String> {
        if !self.initialized {
            return Err("libmpv is not initialized.".into());
        }

        let name = CString::new("pause").unwrap();
        let mut value: c_int = if paused { 1 } else { 0 };

        let result = unsafe {
            (self.api.set_property)(
                self.handle,
                name.as_ptr(),
                MPV_FORMAT_FLAG,
                (&mut value as *mut c_int).cast(),
            )
        };

        self.check(result, "set pause")
    }

    pub fn paused(&self) -> Result<bool, String> {
        if !self.initialized {
            return Err("libmpv is not initialized.".into());
        }

        let name = CString::new("pause").unwrap();
        let mut value: c_int = 0;

        let result = unsafe {
            (self.api.get_property)(
                self.handle,
                name.as_ptr(),
                MPV_FORMAT_FLAG,
                (&mut value as *mut c_int).cast(),
            )
        };

        self.check(result, "get pause")?;

        Ok(value != 0)
    }

    pub fn position(&self) -> Result<f64, String> {
        self.get_double_property("time-pos")
    }

    pub fn duration(&self) -> Result<f64, String> {
        self.get_double_property("duration")
    }

    pub fn seek_absolute(&self, seconds: f64) -> Result<(), String> {
        let seconds_string = format!("{seconds:.6}");

        self.command(&["seek", &seconds_string, "absolute+exact"])
    }

    pub fn stop(&mut self) {
        if self.loaded && self.initialized && !self.handle.is_null() {
            let _ = self.command(&["stop"]);
        }

        self.loaded = false;
    }

    pub fn is_running(&mut self) -> bool {
        self.loaded && self.initialized && !self.handle.is_null() && !self.render_context.is_null()
    }

    fn set_option_string(&self, name: &str, value: &str) -> Result<(), String> {
        let name = CString::new(name).map_err(|_| "Invalid mpv option name.".to_string())?;

        let value = CString::new(value).map_err(|_| "Invalid mpv option value.".to_string())?;

        let result =
            unsafe { (self.api.set_option_string)(self.handle, name.as_ptr(), value.as_ptr()) };

        self.check(result, "set mpv option")
    }

    fn command(&self, args: &[&str]) -> Result<(), String> {
        if !self.initialized {
            return Err("libmpv is not initialized.".into());
        }

        if self.handle.is_null() {
            return Err("libmpv handle is NULL.".into());
        }

        let strings: Vec<CString> = args
            .iter()
            .map(|value| {
                CString::new(*value)
                    .map_err(|_| format!("mpv argument contains an embedded NUL: {value:?}"))
            })
            .collect::<Result<_, _>>()?;

        let mut pointers: Vec<*const c_char> = strings.iter().map(|value| value.as_ptr()).collect();

        pointers.push(ptr::null());

        let result = unsafe { (self.api.command)(self.handle, pointers.as_ptr()) };

        self.check(result, args.first().copied().unwrap_or("command"))
    }

    fn get_double_property(&self, property: &str) -> Result<f64, String> {
        if !self.initialized {
            return Err("libmpv is not initialized.".into());
        }

        let name =
            CString::new(property).map_err(|_| format!("Invalid mpv property name: {property}"))?;

        let mut value = 0.0_f64;

        let result = unsafe {
            (self.api.get_property)(
                self.handle,
                name.as_ptr(),
                MPV_FORMAT_DOUBLE,
                (&mut value as *mut f64).cast(),
            )
        };

        self.check(result, &format!("get {property}"))?;

        Ok(value)
    }

    fn check(&self, result: c_int, operation: &str) -> Result<(), String> {
        if result >= 0 {
            return Ok(());
        }

        Err(format!(
            "{operation}: {}",
            Self::error_from_api(&self.api, result)
        ))
    }

    fn error_from_api(api: &MpvApi, code: c_int) -> String {
        error_string(api.error_string, code)
    }

    fn dll_path() -> Result<PathBuf, String> {
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

fn error_string(error_string: MpvErrorString, code: c_int) -> String {
    unsafe {
        let ptr = error_string(code);

        if ptr.is_null() {
            return format!("mpv error {code}");
        }

        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        if !self.render_context.is_null() {
            unsafe {
                (self.api.render_context_set_update_callback)(
                    self.render_context,
                    None,
                    ptr::null_mut(),
                );

                (self.api.render_context_free)(self.render_context);
            }

            self.render_context = ptr::null_mut();
        }

        self.render_update_bridge = None;
        self.proc_address_bridge = None;

        if !self.handle.is_null() {
            unsafe {
                (self.api.terminate_destroy)(self.handle);
            }

            self.handle = ptr::null_mut();
        }
    }
}
