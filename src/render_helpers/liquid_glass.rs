use std::rc::Rc;

use anyhow::{ensure, Context as _};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::gles::{ffi, link_program, GlesError, GlesRenderer, GlesTexture};
use smithay::backend::renderer::{ContextId, Renderer as _, Texture as _};
use smithay::gpu_span_location;
use smithay::utils::{Buffer, Size};

use crate::render_helpers::shaders::Shaders;

#[derive(Debug)]
pub struct LiquidGlass {
    program: LiquidGlassProgram,
    renderer_context_id: ContextId<GlesTexture>,
    textures: Vec<GlesTexture>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquidGlassOptions {
    pub inset_px: f32,
    pub border_radius_px: f32,
    pub edge_width_px: f32,
    pub edge_softness_px: f32,
    pub max_warp_px: f32,
    pub interior_warp_px: f32,
    pub white_tint: f32,
    pub edge_highlight: f32,
}

impl From<niri_config::LiquidGlass> for LiquidGlassOptions {
    fn from(config: niri_config::LiquidGlass) -> Self {
        Self {
            inset_px: config.inset_px as f32,
            border_radius_px: config.border_radius_px as f32,
            edge_width_px: config.edge_width_px as f32,
            edge_softness_px: config.edge_softness_px as f32,
            max_warp_px: config.max_warp_px as f32,
            interior_warp_px: config.interior_warp_px as f32,
            white_tint: config.white_tint as f32,
            edge_highlight: config.edge_highlight as f32,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiquidGlassProgram(Rc<LiquidGlassProgramInner>);

#[derive(Debug)]
struct LiquidGlassProgramInner {
    program: ffi::types::GLuint,
    uniform_tex: ffi::types::GLint,
    uniform_rect_size: ffi::types::GLint,
    uniform_inset_px: ffi::types::GLint,
    uniform_border_radius_px: ffi::types::GLint,
    uniform_edge_width_px: ffi::types::GLint,
    uniform_edge_softness_px: ffi::types::GLint,
    uniform_max_warp_px: ffi::types::GLint,
    uniform_interior_warp_px: ffi::types::GLint,
    uniform_white_tint: ffi::types::GLint,
    uniform_edge_highlight: ffi::types::GLint,
    attrib_vert: ffi::types::GLint,
}

impl LiquidGlassProgram {
    pub fn compile(renderer: &mut GlesRenderer) -> anyhow::Result<Self> {
        renderer
            .with_context(move |gl| unsafe {
                let program = link_program(
                    gl,
                    include_str!("shaders/blur.vert"),
                    include_str!("shaders/liquid_glass.frag"),
                )
                .context("error compiling liquid_glass shader")?;

                Ok(Self(Rc::new(LiquidGlassProgramInner {
                    program,
                    uniform_tex: gl.GetUniformLocation(program, c"tex".as_ptr()),
                    uniform_rect_size: gl.GetUniformLocation(program, c"rect_size".as_ptr()),
                    uniform_inset_px: gl.GetUniformLocation(program, c"inset_px".as_ptr()),
                    uniform_border_radius_px: gl
                        .GetUniformLocation(program, c"border_radius_px".as_ptr()),
                    uniform_edge_width_px: gl
                        .GetUniformLocation(program, c"edge_width_px".as_ptr()),
                    uniform_edge_softness_px: gl
                        .GetUniformLocation(program, c"edge_softness_px".as_ptr()),
                    uniform_max_warp_px: gl
                        .GetUniformLocation(program, c"max_warp_px".as_ptr()),
                    uniform_interior_warp_px: gl
                        .GetUniformLocation(program, c"interior_warp_px".as_ptr()),
                    uniform_white_tint: gl
                        .GetUniformLocation(program, c"white_tint".as_ptr()),
                    uniform_edge_highlight: gl
                        .GetUniformLocation(program, c"edge_highlight".as_ptr()),
                    attrib_vert: gl.GetAttribLocation(program, c"vert".as_ptr()),
                })))
            })
            .context("error making GL context current")?
    }

    pub fn destroy(self, renderer: &mut GlesRenderer) -> Result<(), GlesError> {
        renderer.with_context(move |gl| unsafe {
            gl.DeleteProgram(self.0.program);
        })
    }
}

impl LiquidGlass {
    pub fn new(renderer: &mut GlesRenderer) -> Option<Self> {
        let program = Shaders::get(renderer).liquid_glass.clone()?;
        Some(Self {
            program,
            renderer_context_id: renderer.context_id(),
            textures: Vec::new(),
        })
    }

    pub fn prepare_textures(
        &mut self,
        mut create_texture: impl FnMut(Fourcc, Size<i32, Buffer>) -> Result<GlesTexture, GlesError>,
        source: &GlesTexture,
    ) -> anyhow::Result<()> {
        let size = source.size();

        if let Some(output) = self.textures.first_mut() {
            if output.size() != size {
                self.textures.clear();
            } else if !output.is_unique_reference() {
                self.textures.clear();
            }
        }

        if self.textures.is_empty() {
            let texture: GlesTexture =
                create_texture(Fourcc::Abgr8888, size).context("error creating texture")?;
            self.textures.push(texture);
        }

        Ok(())
    }

    pub fn render(
        &mut self,
        renderer: &mut GlesRenderer,
        source: &GlesTexture,
        options: LiquidGlassOptions,
    ) -> anyhow::Result<GlesTexture> {
        let _span = tracy_client::span!("LiquidGlass::render");
        trace!("rendering liquid glass");

        ensure!(
            renderer.context_id() == self.renderer_context_id,
            "wrong renderer"
        );

        ensure!(!self.textures.is_empty(), "textures not prepared");

        let destination = self.textures[0].clone();

        let size = source.size();
        ensure!(
            destination.size() == size,
            "destination texture has wrong size"
        );



        renderer.with_profiled_context(gpu_span_location!("LiquidGlass::render"), |gl| unsafe {
            while gl.GetError() != ffi::NO_ERROR {}

            gl.Disable(ffi::BLEND);
            gl.Disable(ffi::SCISSOR_TEST);

            gl.ActiveTexture(ffi::TEXTURE0);

            let mut fbo = 0;
            gl.GenFramebuffers(1, &mut fbo);
            gl.BindFramebuffer(ffi::DRAW_FRAMEBUFFER, fbo);

            let program = &self.program.0;
            gl.UseProgram(program.program);
            gl.Uniform1i(program.uniform_tex, 0);

            gl.Uniform2f(program.uniform_rect_size, size.w as f32, size.h as f32);
            gl.Uniform1f(program.uniform_inset_px, options.inset_px);
            gl.Uniform1f(program.uniform_border_radius_px, options.border_radius_px);
            gl.Uniform1f(program.uniform_edge_width_px, options.edge_width_px);
            gl.Uniform1f(program.uniform_edge_softness_px, options.edge_softness_px);
            gl.Uniform1f(program.uniform_max_warp_px, options.max_warp_px);
            gl.Uniform1f(program.uniform_interior_warp_px, options.interior_warp_px);
            gl.Uniform1f(program.uniform_white_tint, options.white_tint);
            gl.Uniform1f(program.uniform_edge_highlight, options.edge_highlight);

            let vertices: [f32; 12] = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0];
            gl.EnableVertexAttribArray(program.attrib_vert as u32);
            gl.BindBuffer(ffi::ARRAY_BUFFER, 0);
            gl.VertexAttribPointer(
                program.attrib_vert as u32,
                2,
                ffi::FLOAT,
                ffi::FALSE,
                0,
                vertices.as_ptr().cast(),
            );

            gl.Viewport(0, 0, size.w, size.h);

            gl.FramebufferTexture2D(
                ffi::DRAW_FRAMEBUFFER,
                ffi::COLOR_ATTACHMENT0,
                ffi::TEXTURE_2D,
                destination.tex_id(),
                0,
            );

            gl.BindTexture(ffi::TEXTURE_2D, source.tex_id());
            gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MIN_FILTER, ffi::LINEAR as i32);
            gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MAG_FILTER, ffi::LINEAR as i32);
            gl.TexParameteri(
                ffi::TEXTURE_2D,
                ffi::TEXTURE_WRAP_S,
                ffi::CLAMP_TO_EDGE as i32,
            );
            gl.TexParameteri(
                ffi::TEXTURE_2D,
                ffi::TEXTURE_WRAP_T,
                ffi::CLAMP_TO_EDGE as i32,
            );

            gl.DrawArrays(ffi::TRIANGLES, 0, 6);

            gl.DisableVertexAttribArray(program.attrib_vert as u32);

            gl.BindFramebuffer(ffi::DRAW_FRAMEBUFFER, 0);
            gl.DeleteFramebuffers(1, &fbo);
        })?;

        Ok(destination)
    }
}
