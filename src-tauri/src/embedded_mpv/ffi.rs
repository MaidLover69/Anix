//! The part of the libmpv client and render API the embedded player uses, loaded at run time
//! with `dlopen`, so the app starts (and falls back to the web player) when libmpv is missing.
//! Declarations follow `mpv/client.h`, `mpv/render.h` and `mpv/render_gl.h` (client API 2.x).

use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};

pub type Handle = *mut c_void;
pub type RenderContext = *mut c_void;

pub const FORMAT_NONE: c_int = 0;
pub const FORMAT_STRING: c_int = 1;
pub const FORMAT_FLAG: c_int = 3;
pub const FORMAT_DOUBLE: c_int = 5;

pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_END_FILE: c_int = 7;
pub const EVENT_FILE_LOADED: c_int = 8;
pub const EVENT_SEEK: c_int = 20;
pub const EVENT_PLAYBACK_RESTART: c_int = 21;
pub const EVENT_PROPERTY_CHANGE: c_int = 22;

pub const END_FILE_REASON_EOF: c_int = 0;
pub const END_FILE_REASON_ERROR: c_int = 4;

pub const RENDER_PARAM_INVALID: c_int = 0;
pub const RENDER_PARAM_API_TYPE: c_int = 1;
pub const RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
pub const RENDER_PARAM_OPENGL_FBO: c_int = 3;
pub const RENDER_PARAM_FLIP_Y: c_int = 4;
pub const RENDER_PARAM_X11_DISPLAY: c_int = 8;
pub const RENDER_PARAM_WL_DISPLAY: c_int = 9;

/// `mpv_render_context_update` flag: a new frame should be drawn.
pub const RENDER_UPDATE_FRAME: u64 = 1;

#[repr(C)]
pub struct Event {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct EventProperty {
    pub name: *const c_char,
    pub format: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct EventEndFile {
    pub reason: c_int,
    pub error: c_int,
}

#[repr(C)]
pub struct RenderParam {
    pub kind: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct OpenGlInitParams {
    pub get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    pub get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
pub struct OpenGlFbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

pub struct Lib {
    pub create: unsafe extern "C" fn() -> Handle,
    pub initialize: unsafe extern "C" fn(Handle) -> c_int,
    pub terminate_destroy: unsafe extern "C" fn(Handle),
    pub set_option_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    pub set_property_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    pub get_property_string: unsafe extern "C" fn(Handle, *const c_char) -> *mut c_char,
    pub free: unsafe extern "C" fn(*mut c_void),
    pub command: unsafe extern "C" fn(Handle, *mut *const c_char) -> c_int,
    pub observe_property: unsafe extern "C" fn(Handle, u64, *const c_char, c_int) -> c_int,
    pub wait_event: unsafe extern "C" fn(Handle, c_double) -> *mut Event,
    pub error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    pub render_context_create: unsafe extern "C" fn(*mut RenderContext, Handle, *mut RenderParam) -> c_int,
    pub render_context_set_update_callback:
        unsafe extern "C" fn(RenderContext, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    pub render_context_update: unsafe extern "C" fn(RenderContext) -> u64,
    pub render_context_render: unsafe extern "C" fn(RenderContext, *mut RenderParam) -> c_int,
    pub render_context_free: unsafe extern "C" fn(RenderContext),
    /// libavcodec's licence string ("LGPL version 2.1 or later" / "GPL version 2 or later").
    pub avcodec_license: Option<unsafe extern "C" fn() -> *const c_char>,
}

// The function pointers are plain C entry points of a library that is never unloaded.
unsafe impl Send for Lib {}
unsafe impl Sync for Lib {}

/// `dlopen` with the error text on failure.
pub fn open(path: &str) -> Result<*mut c_void, String> {
    let c_path = CString::new(path).map_err(|_| "Invalid library path".to_string())?;
    let handle = unsafe { libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
    if handle.is_null() {
        let reason = unsafe { libc::dlerror() };
        let reason = if reason.is_null() {
            "unknown error".to_string()
        } else {
            unsafe { CStr::from_ptr(reason) }.to_string_lossy().into_owned()
        };
        return Err(format!("{} could not be loaded: {}", path, reason));
    }
    Ok(handle)
}

pub fn symbol(handle: *mut c_void, name: &str) -> *mut c_void {
    let c_name = CString::new(name).expect("symbol names have no NUL");
    unsafe { libc::dlsym(handle, c_name.as_ptr()) }
}

impl Lib {
    pub fn load(path: &str) -> Result<Lib, String> {
        let handle = open(path)?;
        macro_rules! sym {
            ($name:literal) => {{
                let ptr = symbol(handle, $name);
                if ptr.is_null() {
                    return Err(format!("{} lacks {}", path, $name));
                }
                // SAFETY: the declaration above matches the libmpv 2.x prototype of this symbol.
                unsafe { std::mem::transmute::<*mut c_void, _>(ptr) }
            }};
        }
        let avcodec = symbol(handle, "avcodec_license");
        Ok(Lib {
            create: sym!("mpv_create"),
            initialize: sym!("mpv_initialize"),
            terminate_destroy: sym!("mpv_terminate_destroy"),
            set_option_string: sym!("mpv_set_option_string"),
            set_property_string: sym!("mpv_set_property_string"),
            get_property_string: sym!("mpv_get_property_string"),
            free: sym!("mpv_free"),
            command: sym!("mpv_command"),
            observe_property: sym!("mpv_observe_property"),
            wait_event: sym!("mpv_wait_event"),
            error_string: sym!("mpv_error_string"),
            render_context_create: sym!("mpv_render_context_create"),
            render_context_set_update_callback: sym!("mpv_render_context_set_update_callback"),
            render_context_update: sym!("mpv_render_context_update"),
            render_context_render: sym!("mpv_render_context_render"),
            render_context_free: sym!("mpv_render_context_free"),
            avcodec_license: if avcodec.is_null() {
                None
            } else {
                // SAFETY: `const char *avcodec_license(void)`.
                Some(unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> *const c_char>(avcodec) })
            },
        })
    }

    pub fn error_text(&self, code: c_int) -> String {
        let text = unsafe { (self.error_string)(code) };
        if text.is_null() {
            return format!("error {}", code);
        }
        unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
    }

    pub fn license(&self) -> Option<String> {
        let f = self.avcodec_license?;
        let text = unsafe { f() };
        (!text.is_null()).then(|| unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned())
    }
}
