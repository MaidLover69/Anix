//! The GTK side: a GL area under the webview, inside a `GtkOverlay` that takes the webview's
//! place in the window. Everything here runs on the GTK main thread.

use super::ffi::{self, Handle, Lib, OpenGlFbo, OpenGlInitParams, RenderContext, RenderParam};
use super::policy::{self, BlackWatch};
use super::selftest::Gl;
use gtk::glib;
use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
use once_cell::sync::OnceCell;
use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_int, c_void};
use std::ptr::null_mut;

pub(super) type GetProc = unsafe extern "C" fn(*const c_char) -> *mut c_void;

pub(super) const GL_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
const GL_RGBA: u32 = 0x1908;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
/// How often the drawn picture is sampled for the black-picture watch.
const BLACK_CHECK_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

static GL_GET_PROC: OnceCell<GetProc> = OnceCell::new();
static FIRST_FRAME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

thread_local! {
    static AREA: RefCell<Option<gtk::GLArea>> = const { RefCell::new(None) };
    static CONTEXT: Cell<RenderContext> = const { Cell::new(null_mut()) };
    static GL: Cell<Option<Gl>> = const { Cell::new(None) };
    static BLACK: RefCell<(Option<std::time::Instant>, BlackWatch)> = RefCell::new((None, BlackWatch::default()));
}

pub(super) unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    match GL_GET_PROC.get() {
        Some(get) => get(name),
        None => null_mut(),
    }
}

/// GDK uses EGL on Wayland (and on X11 when asked to) and GLX otherwise; the loader must
/// match the context GDK made current.
fn gl_loader() -> Result<GetProc, String> {
    if let Ok(egl) = ffi::open("libEGL.so.1") {
        let current = ffi::symbol(egl, "eglGetCurrentContext");
        let get = ffi::symbol(egl, "eglGetProcAddress");
        if !current.is_null() && !get.is_null() {
            let current: unsafe extern "C" fn() -> *mut c_void = unsafe { std::mem::transmute(current) };
            if !unsafe { current() }.is_null() {
                tracing::info!("embedded mpv draws through EGL");
                return Ok(unsafe { std::mem::transmute::<*mut c_void, GetProc>(get) });
            }
        }
    }
    let glx = ffi::open("libGL.so.1")?;
    let get = ffi::symbol(glx, "glXGetProcAddressARB");
    if get.is_null() {
        return Err("No OpenGL loader was found".to_string());
    }
    tracing::info!("embedded mpv draws through GLX");
    Ok(unsafe { std::mem::transmute::<*mut c_void, GetProc>(get) })
}

/// The native display mpv needs for hardware decoding interop, as a render parameter.
pub(super) fn display_param(area: &gtk::GLArea) -> Option<RenderParam> {
    let display = WidgetExt::display(area);
    let raw: *mut gtk::gdk::ffi::GdkDisplay = display.to_glib_none().0;
    let (getter, kind) = match display.type_().name() {
        "GdkWaylandDisplay" => ("gdk_wayland_display_get_wl_display", ffi::RENDER_PARAM_WL_DISPLAY),
        "GdkX11Display" => ("gdk_x11_display_get_xdisplay", ffi::RENDER_PARAM_X11_DISPLAY),
        _ => return None,
    };
    let get = ffi::symbol(libc::RTLD_DEFAULT, getter);
    if get.is_null() {
        return None;
    }
    let get: unsafe extern "C" fn(*mut gtk::gdk::ffi::GdkDisplay) -> *mut c_void = unsafe { std::mem::transmute(get) };
    let native = unsafe { get(raw) };
    (!native.is_null()).then_some(RenderParam { kind, data: native })
}

unsafe extern "C" fn on_update(_ctx: *mut c_void) {
    // Called on an mpv thread, which must not touch GTK or mpv: hand over to the main loop.
    glib::MainContext::default().invoke(|| {
        AREA.with(|area| {
            if let Some(area) = area.borrow().as_ref() {
                area.queue_render();
            }
        })
    });
}

fn create_context(area: &gtk::GLArea, lib: &'static Lib, handle: Handle) -> Result<(), String> {
    area.make_current();
    if let Some(error) = area.error() {
        return Err(format!("OpenGL is not available: {}", error));
    }
    let get = match GL_GET_PROC.get() {
        Some(get) => *get,
        None => *GL_GET_PROC.get_or_init(|| gl_loader().unwrap_or(no_proc)),
    };
    let gl = Gl::load(get).ok_or("OpenGL functions could not be loaded")?;
    GL.with(|cell| cell.set(Some(gl)));

    let mut init = OpenGlInitParams { get_proc_address, get_proc_address_ctx: null_mut() };
    let mut params = vec![
        RenderParam { kind: ffi::RENDER_PARAM_API_TYPE, data: c"opengl".as_ptr() as *mut c_void },
        RenderParam { kind: ffi::RENDER_PARAM_OPENGL_INIT_PARAMS, data: &mut init as *mut _ as *mut c_void },
    ];
    params.extend(display_param(area));
    params.push(RenderParam { kind: ffi::RENDER_PARAM_INVALID, data: null_mut() });

    let mut context: RenderContext = null_mut();
    let code = unsafe { (lib.render_context_create)(&mut context, handle, params.as_mut_ptr()) };
    if code < 0 || context.is_null() {
        return Err(format!("mpv could not draw with OpenGL: {}", lib.error_text(code)));
    }
    unsafe { (lib.render_context_set_update_callback)(context, Some(on_update), null_mut()) };
    CONTEXT.with(|cell| cell.set(context));
    super::selftest::schedule(area.clone(), lib, gl);
    Ok(())
}

unsafe extern "C" fn no_proc(_name: *const c_char) -> *mut c_void {
    null_mut()
}

fn render(area: &gtk::GLArea, lib: &'static Lib) -> glib::Propagation {
    let context = CONTEXT.with(Cell::get);
    let Some(gl) = GL.with(Cell::get) else { return glib::Propagation::Stop };
    if context.is_null() {
        return glib::Propagation::Stop;
    }
    let mut fbo: c_int = 0;
    unsafe { (gl.get_integerv)(GL_FRAMEBUFFER_BINDING, &mut fbo) };
    let scale = area.scale_factor();
    let mut target = OpenGlFbo {
        fbo,
        w: area.allocated_width() * scale,
        h: area.allocated_height() * scale,
        internal_format: 0,
    };
    // GtkGLArea's framebuffer has its origin at the bottom left.
    let mut flip: c_int = 1;
    let mut params = [
        RenderParam { kind: ffi::RENDER_PARAM_OPENGL_FBO, data: &mut target as *mut _ as *mut c_void },
        RenderParam { kind: ffi::RENDER_PARAM_FLIP_Y, data: &mut flip as *mut _ as *mut c_void },
        RenderParam { kind: ffi::RENDER_PARAM_INVALID, data: null_mut() },
    ];
    let code = unsafe {
        (lib.render_context_update)(context);
        (lib.render_context_render)(context, params.as_mut_ptr())
    };
    if !FIRST_FRAME.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::info!("embedded mpv drew its first frame: {}x{} into framebuffer {} ({})", target.w, target.h, fbo, code);
    }
    watch_for_black(&gl, target.w, target.h);
    glib::Propagation::Stop
}

/// With full interop, samples the drawn video now and then; a picture that stays black while
/// the playhead moves switches this session to copy-back.
fn watch_for_black(gl: &Gl, w: c_int, h: c_int) {
    use std::sync::atomic::Ordering::Relaxed;
    let Some(gl) = gl.extra else { return };
    if !super::INTEROP.load(Relaxed) || !super::PLAYING.load(Relaxed) || w < 8 || h < 8 {
        return;
    }
    BLACK.with(|cell| {
        let mut cell = cell.borrow_mut();
        let now = std::time::Instant::now();
        if cell.0.is_some_and(|last| now.duration_since(last) < BLACK_CHECK_EVERY) {
            return;
        }
        let started = *cell.0.get_or_insert(now);
        cell.0 = Some(now);
        let [left, right, top, bottom] = *super::MARGINS.lock().unwrap_or_else(|p| p.into_inner());
        let (x0, x1) = (left * w as f64, (1.0 - right) * w as f64);
        // GL rows count from the bottom.
        let (y0, y1) = (bottom * h as f64, (1.0 - top) * h as f64);
        let mut samples = Vec::with_capacity(25);
        for fy in [0.1, 0.3, 0.5, 0.7, 0.9] {
            for fx in [0.1, 0.3, 0.5, 0.7, 0.9] {
                let mut pixel = [0u8; 4];
                let (x, y) = ((x0 + (x1 - x0) * fx) as c_int, (y0 + (y1 - y0) * fy) as c_int);
                unsafe { (gl.read_pixels)(x, y, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, pixel.as_mut_ptr().cast()) };
                samples.push([pixel[0], pixel[1], pixel[2]]);
            }
        }
        let time_pos = f64::from_bits(super::TIME_POS.load(Relaxed));
        let elapsed = now.duration_since(started).as_secs_f64();
        if cell.1.observe(elapsed, policy::all_black(&samples), time_pos) {
            tracing::warn!("Black picture at {:.1} s with full interop, samples: {:?}", time_pos, samples);
            super::interop_drew_black();
        }
    });
}

/// Puts the GL area under the main webview and makes the webview's background transparent.
pub fn attach(window: &tauri::WebviewWindow, lib: &'static Lib, handle: Handle) -> Result<(), String> {
    let vbox = window.default_vbox().map_err(|e| e.to_string())?;
    let children = vbox.children();
    let index = children
        .iter()
        .position(|child| child.type_().name() == "WebKitWebView")
        .ok_or("The webview was not found in the window")?;
    let webview = children[index].clone();

    let area = gtk::GLArea::new();
    area.set_auto_render(false);
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.connect_realize(move |area| {
        if let Err(e) = create_context(area, lib, handle) {
            super::render_failed(e);
        }
    });
    area.connect_unrealize(move |area| {
        area.make_current();
        let context = CONTEXT.with(|cell| cell.replace(null_mut()));
        if !context.is_null() {
            unsafe { (lib.render_context_free)(context) };
        }
    });
    area.connect_render(move |area, _| render(area, lib));

    let overlay = gtk::Overlay::new();
    vbox.remove(&webview);
    overlay.add(&area);
    overlay.add_overlay(&webview);
    vbox.pack_start(&overlay, true, true, 0);
    vbox.reorder_child(&overlay, index as i32);
    overlay.show_all();
    webview.grab_focus();
    AREA.with(|cell| *cell.borrow_mut() = Some(area));

    AsRef::<tauri::Webview<_>>::as_ref(window)
        .set_background_color(Some(tauri::window::Color(0, 0, 0, 0)))
        .map_err(|e| e.to_string())
}
