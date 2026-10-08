use crate::media::player::MpvRenderHandle;

use eframe::{
    egui,
    egui_glow::CallbackFn,
    glow::{self, HasContext},
};

use std::{
    num::NonZeroU32,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct GlResources {
    texture: Option<glow::Texture>,
    framebuffer: Option<glow::Framebuffer>,
    width: i32,
    height: i32,
}

pub struct VideoSurface {
    render: MpvRenderHandle,
    resources: Arc<Mutex<GlResources>>,
}

impl VideoSurface {
    pub fn new(render: MpvRenderHandle) -> Self {
        Self {
            render,
            resources: Arc::new(Mutex::new(GlResources::default())),
        }
    }

    pub fn paint(&self, ui: &egui::Ui, rect: egui::Rect) {
        let render = self.render;
        let resources = Arc::clone(&self.resources);

        let callback = CallbackFn::new(move |info, painter| {
            let width = (info.viewport.width() * info.pixels_per_point)
                .round()
                .max(1.0) as i32;

            let height = (info.viewport.height() * info.pixels_per_point)
                .round()
                .max(1.0) as i32;

            let Ok(mut resources) = resources.lock() else {
                return;
            };

            let gl = painter.gl();

            unsafe {
                let previous_draw = gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING);

                let previous_read = gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING);

                if let Err(error) = ensure_resources(&mut resources, gl, width, height) {
                    eprintln!("Video OpenGL surface error: {error}");
                    return;
                }

                let Some(framebuffer) = resources.framebuffer else {
                    return;
                };

                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));

                gl.viewport(0, 0, width, height);
                gl.clear_color(0.0, 0.0, 0.0, 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT);

                let framebuffer_id = gl.get_parameter_i32(glow::FRAMEBUFFER_BINDING);

                if let Err(error) = render.render_to_fbo(framebuffer_id, width, height) {
                    eprintln!("libmpv render error: {error}");
                }

                gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(framebuffer));

                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, framebuffer_from_id(previous_draw));

                let pixels_per_point = info.pixels_per_point;
                let screen_height = info.screen_size_px[1] as i32;

                let dst_x0 = (info.viewport.left() * pixels_per_point).round() as i32;

                let dst_x1 = (info.viewport.right() * pixels_per_point).round() as i32;

                // egui coordinates start at the top-left. OpenGL framebuffer
                // coordinates start at the bottom-left.
                let dst_y0 =
                    screen_height - (info.viewport.bottom() * pixels_per_point).round() as i32;

                let dst_y1 =
                    screen_height - (info.viewport.top() * pixels_per_point).round() as i32;

                gl.blit_framebuffer(
                    0,
                    0,
                    width,
                    height,
                    dst_x0,
                    dst_y0,
                    dst_x1,
                    dst_y1,
                    glow::COLOR_BUFFER_BIT,
                    glow::LINEAR,
                );

                gl.bind_framebuffer(glow::READ_FRAMEBUFFER, framebuffer_from_id(previous_read));

                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, framebuffer_from_id(previous_draw));
            }
        });

        ui.painter().add(egui::PaintCallback {
            rect,
            callback: Arc::new(callback),
        });
    }

    pub fn destroy(&self, gl: &glow::Context) {
        let Ok(mut resources) = self.resources.lock() else {
            return;
        };

        unsafe {
            if let Some(framebuffer) = resources.framebuffer.take() {
                gl.delete_framebuffer(framebuffer);
            }

            if let Some(texture) = resources.texture.take() {
                gl.delete_texture(texture);
            }
        }

        resources.width = 0;
        resources.height = 0;
    }
}

fn framebuffer_from_id(id: i32) -> Option<glow::Framebuffer> {
    if id <= 0 {
        None
    } else {
        NonZeroU32::new(id as u32).map(glow::NativeFramebuffer)
    }
}

fn ensure_resources(
    resources: &mut GlResources,
    gl: &glow::Context,
    width: i32,
    height: i32,
) -> Result<(), String> {
    unsafe {
        if resources.texture.is_none() {
            resources.texture = Some(
                gl.create_texture()
                    .map_err(|error| format!("Could not create video texture: {error}"))?,
            );
        }

        if resources.framebuffer.is_none() {
            resources.framebuffer = Some(
                gl.create_framebuffer()
                    .map_err(|error| format!("Could not create video framebuffer: {error}"))?,
            );
        }

        if resources.width == width
            && resources.height == height
            && resources.width > 0
            && resources.height > 0
        {
            return Ok(());
        }

        let texture = resources
            .texture
            .ok_or_else(|| "Video texture unavailable.".to_string())?;

        let framebuffer = resources
            .framebuffer
            .ok_or_else(|| "Video framebuffer unavailable.".to_string())?;

        gl.bind_texture(glow::TEXTURE_2D, Some(texture));

        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );

        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );

        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );

        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );

        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            width,
            height,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );

        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));

        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );

        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);

        if status != glow::FRAMEBUFFER_COMPLETE {
            return Err(format!("Video framebuffer is incomplete: 0x{status:04X}"));
        }

        resources.width = width;
        resources.height = height;

        Ok(())
    }
}
