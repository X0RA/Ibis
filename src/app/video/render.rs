//! mpv renders into a texture in Slint's current GL context. All GL resources
//! are released by RenderingTeardown, before Slint destroys that context.
use super::*;
use glow::HasContext;
use libmpv2_sys as sys;
use std::{
    ffi::{CStr, c_void},
    num::NonZeroU32,
    ptr,
    rc::Rc,
};

type Loader<'a> = dyn Fn(&CStr) -> *const c_void + 'a;
unsafe extern "C" fn get_proc(ctx: *mut c_void, name: *const std::ffi::c_char) -> *mut c_void {
    // mpv resolves GL entry points synchronously during context creation.
    let loader = unsafe { &*ctx.cast::<&Loader<'_>>() };
    loader(unsafe { CStr::from_ptr(name) }).cast_mut()
}
struct Redraw {
    window: slint::Weak<AppWindow>,
    queued: Arc<AtomicBool>,
}
unsafe extern "C" fn redraw(ctx: *mut c_void) {
    let state = unsafe { &*ctx.cast::<Redraw>() };
    if !state.queued.swap(true, Ordering::AcqRel) {
        let queued = state.queued.clone();
        let _ = state.window.upgrade_in_event_loop(move |window| {
            queued.store(false, Ordering::Release);
            window.window().request_redraw();
        });
    }
}
struct Texture {
    gl: Rc<glow::Context>,
    texture: glow::NativeTexture,
    fbo: glow::NativeFramebuffer,
    size: (u32, u32),
}
impl Texture {
    unsafe fn new(gl: Rc<glow::Context>, size: (u32, u32)) -> Result<Self, String> {
        unsafe {
            let texture = gl.create_texture()?;
            let fbo = match gl.create_framebuffer() {
                Ok(fbo) => fbo,
                Err(e) => {
                    gl.delete_texture(texture);
                    return Err(e);
                }
            };
            let old_texture = gl.get_parameter_i32(glow::TEXTURE_BINDING_2D);
            let old_fbo = gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING);
            let old_buffer = gl.get_parameter_i32(glow::PIXEL_UNPACK_BUFFER_BINDING);
            let stores = [
                glow::UNPACK_ALIGNMENT,
                glow::UNPACK_ROW_LENGTH,
                glow::UNPACK_SKIP_PIXELS,
                glow::UNPACK_SKIP_ROWS,
            ];
            let saved = stores.map(|s| gl.get_parameter_i32(s));
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
            for (s, v) in stores.into_iter().zip([1, 0, 0, 0]) {
                gl.pixel_store_i32(s, v);
            }
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            for (p, v) in [
                (glow::TEXTURE_MIN_FILTER, glow::LINEAR),
                (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
                (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
            }
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                size.0 as i32,
                size.1 as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::DRAW_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            let complete =
                gl.check_framebuffer_status(glow::DRAW_FRAMEBUFFER) == glow::FRAMEBUFFER_COMPLETE;
            gl.bind_framebuffer(
                glow::DRAW_FRAMEBUFFER,
                NonZeroU32::new(old_fbo as u32).map(glow::NativeFramebuffer),
            );
            gl.bind_texture(
                glow::TEXTURE_2D,
                NonZeroU32::new(old_texture as u32).map(glow::NativeTexture),
            );
            gl.bind_buffer(
                glow::PIXEL_UNPACK_BUFFER,
                NonZeroU32::new(old_buffer as u32).map(glow::NativeBuffer),
            );
            for (s, v) in stores.into_iter().zip(saved) {
                gl.pixel_store_i32(s, v);
            }
            let result = Self {
                gl,
                texture,
                fbo,
                size,
            };
            if complete {
                Ok(result)
            } else {
                Err("Could not allocate video framebuffer".into())
            }
        }
    }
    fn image(&self) -> slint::Image {
        // The texture outlives this borrowed image; replacement/teardown clears
        // the UI image before deleting the texture, in the same GL callback.
        unsafe {
            slint::BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                self.texture.0,
                self.size.into(),
            )
            .build()
        }
    }
}
impl Drop for Texture {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_framebuffer(self.fbo);
            self.gl.delete_texture(self.texture);
        }
    }
}
struct Renderer {
    ctx: *mut sys::mpv_render_context,
    gl: Rc<glow::Context>,
    texture: Option<Texture>,
    _redraw: Box<Redraw>,
    // Own the mpv core until AFTER freeing its rendering context.
    _core: Arc<Mpv>,
}
impl Renderer {
    unsafe fn new(
        core: Arc<Mpv>,
        loader: &Loader<'_>,
        window: slint::Weak<AppWindow>,
    ) -> Result<Self, String> {
        unsafe {
            let gl = Rc::new(glow::Context::from_loader_function_cstr(|name| {
                loader(name)
            }));
            let mut init = sys::mpv_opengl_init_params {
                get_proc_address: Some(get_proc),
                get_proc_address_ctx: (&loader as *const &Loader<'_>).cast_mut().cast(),
            };
            let mut advanced: i32 = 1;
            let mut params = [
                sys::mpv_render_param {
                    type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
                    data: c"opengl".as_ptr().cast_mut().cast(),
                },
                sys::mpv_render_param {
                    type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                    data: (&mut init as *mut sys::mpv_opengl_init_params).cast(),
                },
                sys::mpv_render_param {
                    type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_ADVANCED_CONTROL,
                    data: (&mut advanced as *mut i32).cast(),
                },
                sys::mpv_render_param {
                    type_: 0,
                    data: ptr::null_mut(),
                },
            ];
            let mut ctx = ptr::null_mut();
            let code =
                sys::mpv_render_context_create(&mut ctx, core.ctx.as_ptr(), params.as_mut_ptr());
            if code < 0 {
                return Err(format!("mpv OpenGL setup failed ({code})"));
            }
            let mut redraw = Box::new(Redraw {
                window,
                queued: Arc::new(AtomicBool::new(false)),
            });
            sys::mpv_render_context_set_update_callback(
                ctx,
                Some(self::redraw),
                (&mut *redraw as *mut Redraw).cast(),
            );
            Ok(Self {
                ctx,
                gl,
                texture: None,
                _redraw: redraw,
                _core: core,
            })
        }
    }
    unsafe fn render(&mut self, window: &AppWindow) -> Result<(), String> {
        unsafe {
            let updated = sys::mpv_render_context_update(self.ctx);
            // Advanced control requires servicing updates even when the view is hidden.
            if !window.get_viewer_open() || !window.get_is_video() {
                if updated & sys::mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME as u64 != 0 {
                    let mut skip = 1i32;
                    let mut block = 0i32;
                    let mut params = [
                        sys::mpv_render_param {
                            type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_SKIP_RENDERING,
                            data: (&mut skip as *mut i32).cast(),
                        },
                        sys::mpv_render_param {
                            type_:
                                sys::mpv_render_param_type_MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                            data: (&mut block as *mut i32).cast(),
                        },
                        sys::mpv_render_param {
                            type_: 0,
                            data: ptr::null_mut(),
                        },
                    ];
                    sys::mpv_render_context_render(self.ctx, params.as_mut_ptr());
                }
                return Ok(());
            }
            let limit = self
                .gl
                .get_parameter_i32(glow::MAX_TEXTURE_SIZE)
                .clamp(1, 8192) as u32;
            let size = (
                window.get_video_texture_width().max(1.0) as u32,
                window.get_video_texture_height().max(1.0) as u32,
            );
            let factor = (limit as f64 / f64::from(size.0.max(size.1))).min(1.0);
            let size = (
                (size.0 as f64 * factor).max(1.0) as u32,
                (size.1 as f64 * factor).max(1.0) as u32,
            );
            let changed = self.texture.as_ref().is_none_or(|t| t.size != size);
            if changed {
                let texture = Texture::new(self.gl.clone(), size)?;
                window.set_video_texture(Default::default());
                self.texture = Some(texture);
            }
            if updated & sys::mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME as u64 == 0 && !changed
            {
                return Ok(());
            }
            let texture = self.texture.as_ref().unwrap();
            let old_fbo = self.gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING);
            let old_read = self.gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING);
            let mut viewport = [0; 4];
            self.gl
                .get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            let mut scissor = [0; 4];
            self.gl
                .get_parameter_i32_slice(glow::SCISSOR_BOX, &mut scissor);
            let blend = [
                glow::BLEND_SRC_RGB,
                glow::BLEND_DST_RGB,
                glow::BLEND_SRC_ALPHA,
                glow::BLEND_DST_ALPHA,
            ]
            .map(|parameter| self.gl.get_parameter_i32(parameter) as u32);
            let mut clear = [0.0; 4];
            self.gl
                .get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE, &mut clear);
            let mut fbo = sys::mpv_opengl_fbo {
                fbo: texture.fbo.0.get() as i32,
                w: size.0 as i32,
                h: size.1 as i32,
                internal_format: glow::RGBA8 as i32,
            };
            let mut flip = 0i32;
            let mut block = 0i32;
            let mut params = [
                sys::mpv_render_param {
                    type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO,
                    data: (&mut fbo as *mut sys::mpv_opengl_fbo).cast(),
                },
                sys::mpv_render_param {
                    type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y,
                    data: (&mut flip as *mut i32).cast(),
                },
                sys::mpv_render_param {
                    type_: sys::mpv_render_param_type_MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                    data: (&mut block as *mut i32).cast(),
                },
                sys::mpv_render_param {
                    type_: 0,
                    data: ptr::null_mut(),
                },
            ];
            let code = sys::mpv_render_context_render(self.ctx, params.as_mut_ptr());
            self.gl.bind_framebuffer(
                glow::DRAW_FRAMEBUFFER,
                NonZeroU32::new(old_fbo as u32).map(glow::NativeFramebuffer),
            );
            self.gl.bind_framebuffer(
                glow::READ_FRAMEBUFFER,
                NonZeroU32::new(old_read as u32).map(glow::NativeFramebuffer),
            );
            self.gl
                .viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
            self.gl
                .scissor(scissor[0], scissor[1], scissor[2], scissor[3]);
            self.gl.clear_color(clear[0], clear[1], clear[2], clear[3]);
            self.gl
                .blend_func_separate(blend[0], blend[1], blend[2], blend[3]);
            if code < 0 {
                return Err(format!("Video rendering failed ({code})"));
            }
            // Republish after navigation, even when the texture size is unchanged.
            if changed || window.get_video_texture().size().width == 0 {
                window.set_video_texture(texture.image());
            }
            Ok(())
        }
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            // Unregister while the callback storage is still alive.
            sys::mpv_render_context_set_update_callback(self.ctx, None, ptr::null_mut());
            sys::mpv_render_context_free(self.ctx);
        }
    }
}
pub(super) fn register(window: &AppWindow, core: Arc<Mpv>) -> Result<(), String> {
    let weak = window.as_weak();
    let mut renderer: Option<Renderer> = None;
    window
        .window()
        .set_rendering_notifier(move |state, api| match state {
            slint::RenderingState::RenderingSetup => {
                let result = match api {
                    slint::GraphicsAPI::NativeOpenGL { get_proc_address } => unsafe {
                        Renderer::new(core.clone(), get_proc_address, weak.clone())
                    },
                    _ => Err("Video playback requires OpenGL".into()),
                };
                match result {
                    Ok(value) => {
                        renderer = Some(value);
                        if let Some(w) = weak.upgrade() {
                            w.set_video_available(true);
                        }
                    }
                    Err(error) => {
                        if let Some(w) = weak.upgrade() {
                            w.set_video_available(false);
                            w.set_video_error(error.into());
                        }
                    }
                }
            }
            slint::RenderingState::BeforeRendering => {
                if let (Some(renderer), Some(window)) = (renderer.as_mut(), weak.upgrade())
                    && let Err(error) = unsafe { renderer.render(&window) }
                {
                    window.set_video_error(error.into());
                    window.set_video_loading(false);
                }
            }
            slint::RenderingState::RenderingTeardown => {
                if let Some(w) = weak.upgrade() {
                    w.set_video_texture(Default::default());
                    w.set_video_available(false);
                }
                renderer = None;
            }
            _ => {}
        })
        .map_err(|e| e.to_string())
}
