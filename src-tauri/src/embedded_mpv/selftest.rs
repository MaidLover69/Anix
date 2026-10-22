//! The hidden hardware decoding self-test: a second mpv with full GL interop (`hwdec=auto-safe`)
//! decodes a tiny clip of four colour bands (`resources/hwdec-selftest.mp4`, made for Anix)
//! into an offscreen framebuffer of the player's own GL context while nothing plays, and the
//! read-back pixels must show those colours. Only then does "Automatic" use full interop.
//! Everything here runs on the GTK main thread.

use super::ffi::{self, Handle, Lib, OpenGlFbo, OpenGlInitParams, RenderContext, RenderParam};
use super::layer::{display_param, get_proc_address, GetProc, GL_FRAMEBUFFER_BINDING};
use super::policy::{self, SelfTest};
use gtk::glib;
use gtk::prelude::*;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr::null_mut;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

static CLIP: &[u8] = include_bytes!("../../resources/hwdec-selftest.mp4");

const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_RGBA: u32 = 0x1908;
const GL_RGBA8: i32 = 0x8058;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
const GL_NEAREST: i32 = 0x2600;
const GL_VENDOR: u32 = 0x1F00;
const GL_RENDERER: u32 = 0x1F01;
const GL_VERSION: u32 = 0x1F02;

/// Frames drawn before the pixels are read (the first ones may still be the black clear).
const FRAMES: u32 = 8;
const GIVE_UP_AFTER: Duration = Duration::from_secs(8);

/// The GL entry points the player layer needs, from the context's loader.
#[derive(Clone, Copy)]
pub(super) struct Gl {
    pub get_integerv: unsafe extern "C" fn(u32, *mut c_int),
    /// For the self-test and the black-picture watch; a loader that hands out extension
    /// functions only leaves them out, and both are then skipped (copy-back stays).
    pub extra: Option<GlExtra>,
}

#[derive(Clone, Copy)]
pub(super) struct GlExtra {
    get_integerv: unsafe extern "C" fn(u32, *mut c_int),
    pub read_pixels: unsafe extern "C" fn(c_int, c_int, c_int, c_int, u32, u32, *mut c_void),
    get_string: unsafe extern "C" fn(u32) -> *const u8,
    gen_framebuffers: unsafe extern "C" fn(c_int, *mut u32),
    delete_framebuffers: unsafe extern "C" fn(c_int, *const u32),
    bind_framebuffer: unsafe extern "C" fn(u32, u32),
    framebuffer_texture_2d: unsafe extern "C" fn(u32, u32, u32, u32, c_int),
    check_framebuffer_status: unsafe extern "C" fn(u32) -> u32,
    gen_textures: unsafe extern "C" fn(c_int, *mut u32),
    delete_textures: unsafe extern "C" fn(c_int, *const u32),
    bind_texture: unsafe extern "C" fn(u32, u32),
    tex_image_2d: unsafe extern "C" fn(u32, c_int, c_int, c_int, c_int, c_int, u32, u32, *const c_void),
    tex_parameteri: unsafe extern "C" fn(u32, u32, c_int),
}

macro_rules! gl_fn {
    ($get:expr, $name:literal) => {{
        let ptr = unsafe { $get(concat!($name, "\0").as_ptr() as *const c_char) };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: the field's type is the GL prototype of this entry point.
        unsafe { std::mem::transmute::<*mut c_void, _>(ptr) }
    }};
}

impl Gl {
    pub fn load(get: GetProc) -> Option<Gl> {
        Some(Gl { get_integerv: gl_fn!(get, "glGetIntegerv"), extra: GlExtra::load(get) })
    }
}

impl GlExtra {
    fn load(get: GetProc) -> Option<GlExtra> {
        macro_rules! f {
            ($name:literal) => {
                gl_fn!(get, $name)
            };
        }
        Some(GlExtra {
            get_integerv: f!("glGetIntegerv"),
            read_pixels: f!("glReadPixels"),
            get_string: f!("glGetString"),
            gen_framebuffers: f!("glGenFramebuffers"),
            delete_framebuffers: f!("glDeleteFramebuffers"),
            bind_framebuffer: f!("glBindFramebuffer"),
            framebuffer_texture_2d: f!("glFramebufferTexture2D"),
            check_framebuffer_status: f!("glCheckFramebufferStatus"),
            gen_textures: f!("glGenTextures"),
            delete_textures: f!("glDeleteTextures"),
            bind_texture: f!("glBindTexture"),
            tex_image_2d: f!("glTexImage2D"),
            tex_parameteri: f!("glTexParameteri"),
        })
    }

    fn string(&self, name: u32) -> String {
        let text = unsafe { (self.get_string)(name) };
        if text.is_null() {
            return String::new();
        }
        unsafe { CStr::from_ptr(text.cast()) }.to_string_lossy().into_owned()
    }

    /// Vendor, renderer and version: changes with the GPU or its driver.
    fn driver(&self) -> String {
        format!("{} | {} | {}", self.string(GL_VENDOR), self.string(GL_RENDERER), self.string(GL_VERSION))
    }
}

/// Runs the self-test once nothing plays, if it is due for this driver.
pub(super) fn schedule(area: gtk::GLArea, lib: &'static Lib, gl: Gl) {
    let Some(gl) = gl.extra else {
        tracing::warn!("The GL loader lacks core functions; no decoding self-test, copy-back is used");
        if super::SELFTEST_DUE.swap(false, Ordering::SeqCst) {
            super::switch_hwdec(super::policy::hwdec_value(super::policy::HwdecMode::Copy, false));
        }
        return;
    };
    // The caller made the player's context current.
    super::gl_ready(&gl.driver());
    glib::timeout_add_local(Duration::from_secs(4), move || {
        if !super::SELFTEST_DUE.load(Ordering::SeqCst) {
            return glib::ControlFlow::Break;
        }
        if super::PLAYING.load(Ordering::SeqCst) || !area.is_realized() {
            return glib::ControlFlow::Continue;
        }
        super::SELFTEST_DUE.store(false, Ordering::SeqCst);
        area.make_current();
        let driver = gl.driver();
        let mpv = super::MPV_VERSION.get().cloned().unwrap_or_default();
        if super::selftest_needed(&driver, &mpv) {
            if let Err(e) = Test::start(&area, lib, gl, driver.clone(), mpv.clone()) {
                tracing::warn!("The hardware decoding self-test could not run: {}", e);
                super::selftest_finished(result(false, driver, mpv));
            }
        }
        glib::ControlFlow::Break
    });
}

fn result(ok: bool, driver: String, mpv: String) -> SelfTest {
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    SelfTest { ok, driver, mpv, app: env!("CARGO_PKG_VERSION").to_string(), at }
}

struct Test {
    lib: &'static Lib,
    gl: GlExtra,
    handle: Handle,
    context: RenderContext,
    fbo: u32,
    texture: u32,
    clip: std::path::PathBuf,
    frames: u32,
    started: Instant,
    driver: String,
    mpv: String,
}

impl Test {
    fn start(area: &gtk::GLArea, lib: &'static Lib, gl: GlExtra, driver: String, mpv: String) -> Result<(), String> {
        let dir = super::cache_dir().ok_or("The cache folder is unknown")?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let clip = dir.join("hwdec-selftest.mp4");
        std::fs::write(&clip, CLIP).map_err(|e| e.to_string())?;

        let handle = unsafe { (lib.create)() };
        if handle.is_null() {
            return Err("mpv_create failed".to_string());
        }
        let mut test =
            Test { lib, gl, handle, context: null_mut(), fbo: 0, texture: 0, clip, frames: 0, started: Instant::now(), driver, mpv };
        if let Err(e) = test.setup(area) {
            test.close(area);
            return Err(e);
        }
        let area = area.clone();
        glib::timeout_add_local(Duration::from_millis(40), move || match test.tick(&area) {
            Some(ok) => {
                let (driver, mpv) = (test.driver.clone(), test.mpv.clone());
                test.close(&area);
                super::selftest_finished(result(ok, driver, mpv));
                glib::ControlFlow::Break
            }
            None => glib::ControlFlow::Continue,
        });
        Ok(())
    }

    fn option(&self, name: &str, value: &str) -> Result<(), String> {
        let (n, v) = (CString::new(name).unwrap(), CString::new(value).unwrap());
        let code = unsafe { (self.lib.set_option_string)(self.handle, n.as_ptr(), v.as_ptr()) };
        if code < 0 { Err(format!("{}: {}", name, self.lib.error_text(code))) } else { Ok(()) }
    }

    fn setup(&mut self, area: &gtk::GLArea) -> Result<(), String> {
        for (name, value) in [
            ("vo", "libmpv"),
            ("hwdec", "auto-safe"),
            ("ao", "null"),
            ("audio", "no"),
            ("config", "no"),
            ("load-scripts", "no"),
            ("terminal", "no"),
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("input-default-bindings", "no"),
        ] {
            self.option(name, value)?;
        }
        let code = unsafe { (self.lib.initialize)(self.handle) };
        if code < 0 {
            return Err(format!("mpv_initialize: {}", self.lib.error_text(code)));
        }

        area.make_current();
        let mut init = OpenGlInitParams { get_proc_address, get_proc_address_ctx: null_mut() };
        let mut params = vec![
            RenderParam { kind: ffi::RENDER_PARAM_API_TYPE, data: c"opengl".as_ptr() as *mut c_void },
            RenderParam { kind: ffi::RENDER_PARAM_OPENGL_INIT_PARAMS, data: &mut init as *mut _ as *mut c_void },
        ];
        params.extend(display_param(area));
        params.push(RenderParam { kind: ffi::RENDER_PARAM_INVALID, data: null_mut() });
        let code = unsafe { (self.lib.render_context_create)(&mut self.context, self.handle, params.as_mut_ptr()) };
        if code < 0 || self.context.is_null() {
            return Err(format!("mpv could not draw with OpenGL: {}", self.lib.error_text(code)));
        }

        let gl = self.gl;
        let (w, h) = policy::SELFTEST_SIZE;
        let mut previous: c_int = 0;
        unsafe {
            (gl.get_integerv)(GL_FRAMEBUFFER_BINDING, &mut previous);
            (gl.gen_textures)(1, &mut self.texture);
            (gl.bind_texture)(GL_TEXTURE_2D, self.texture);
            (gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            (gl.tex_parameteri)(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
            (gl.tex_image_2d)(GL_TEXTURE_2D, 0, GL_RGBA8, w, h, 0, GL_RGBA, GL_UNSIGNED_BYTE, std::ptr::null());
            (gl.bind_texture)(GL_TEXTURE_2D, 0);
            (gl.gen_framebuffers)(1, &mut self.fbo);
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
            (gl.framebuffer_texture_2d)(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, self.texture, 0);
            let complete = (gl.check_framebuffer_status)(GL_FRAMEBUFFER) == GL_FRAMEBUFFER_COMPLETE;
            (gl.bind_framebuffer)(GL_FRAMEBUFFER, previous as u32);
            if !complete {
                return Err("The offscreen framebuffer is not usable".to_string());
            }
        }

        let path = CString::new(self.clip.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let mut args = [c"loadfile".as_ptr(), path.as_ptr(), std::ptr::null()];
        let code = unsafe { (self.lib.command)(self.handle, args.as_mut_ptr()) };
        if code < 0 {
            return Err(format!("loadfile: {}", self.lib.error_text(code)));
        }
        Ok(())
    }

    /// `Some(passed)` once there is a verdict.
    fn tick(&mut self, area: &gtk::GLArea) -> Option<bool> {
        // Nobody else reads this mpv's events; keep its queue empty.
        while unsafe { (*(self.lib.wait_event)(self.handle, 0.0)).event_id } != 0 {}
        if self.started.elapsed() > GIVE_UP_AFTER {
            tracing::warn!("The hardware decoding self-test drew no picture in time");
            return Some(false);
        }
        area.make_current();
        if unsafe { (self.lib.render_context_update)(self.context) } & ffi::RENDER_UPDATE_FRAME == 0 {
            return None;
        }
        let gl = self.gl;
        let (w, h) = policy::SELFTEST_SIZE;
        let mut previous: c_int = 0;
        let mut target = OpenGlFbo { fbo: self.fbo as c_int, w, h, internal_format: 0 };
        let mut flip: c_int = 0;
        let mut params = [
            RenderParam { kind: ffi::RENDER_PARAM_OPENGL_FBO, data: &mut target as *mut _ as *mut c_void },
            RenderParam { kind: ffi::RENDER_PARAM_FLIP_Y, data: &mut flip as *mut _ as *mut c_void },
            RenderParam { kind: ffi::RENDER_PARAM_INVALID, data: null_mut() },
        ];
        unsafe {
            (gl.get_integerv)(GL_FRAMEBUFFER_BINDING, &mut previous);
            (self.lib.render_context_render)(self.context, params.as_mut_ptr());
        }
        self.frames += 1;
        let verdict = (self.frames >= FRAMES).then(|| {
            // The bands are vertical and the points sit on the middle row, so the
            // framebuffer's row order does not matter.
            let samples: Vec<[u8; 3]> = policy::SELFTEST_POINTS
                .iter()
                .map(|&(x, y)| {
                    let mut pixel = [0u8; 4];
                    unsafe {
                        (gl.bind_framebuffer)(GL_FRAMEBUFFER, self.fbo);
                        (gl.read_pixels)(x, y, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, pixel.as_mut_ptr().cast());
                    }
                    [pixel[0], pixel[1], pixel[2]]
                })
                .collect();
            tracing::info!("Hardware decoding self-test pixels: {:?}", samples);
            policy::colours_match(&samples, &policy::SELFTEST_COLOURS, 60)
        });
        unsafe { (gl.bind_framebuffer)(GL_FRAMEBUFFER, previous as u32) };
        verdict
    }

    fn close(&mut self, area: &gtk::GLArea) {
        area.make_current();
        unsafe {
            if !self.context.is_null() {
                (self.lib.render_context_free)(self.context);
                self.context = null_mut();
            }
            (self.lib.terminate_destroy)(self.handle);
            (self.gl.delete_framebuffers)(1, &self.fbo);
            (self.gl.delete_textures)(1, &self.texture);
        }
        let _ = std::fs::remove_file(&self.clip);
        area.queue_render();
    }
}
