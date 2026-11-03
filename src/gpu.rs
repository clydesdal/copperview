//! Draws the layers with the graphics card.
//!
//! Each layer is first drawn alone as a black-and-white picture, because
//! overlapping shapes on one layer mustn't look darker and cut-outs have to
//! bite only into their own layer. Those pictures are then mixed together with
//! their colors, which is where transparency and color mixing happen. The
//! pictures are kept until the view moves, so hovering stays instant.

use crate::board::Layer;
use eframe::egui;
use eframe::glow::{self, HasContext};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mix {
    /// See-through colors laid over each other.
    Normal,
    /// Colored light: red and blue make magenta, and everything goes toward white.
    Light,
    /// Colored inks on white paper: overlaps go darker.
    Ink,
}

/// From millimetres to the -1..1 range of the picture.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Xf {
    pub sx: f32,
    pub sy: f32,
    pub tx: f32,
    pub ty: f32,
}

#[derive(Clone)]
pub struct Item {
    pub layer: Arc<Layer>,
    pub xf: Xf,
    pub color: [f32; 3],
    pub alpha: f32,
    pub invert: bool,
    /// For showing differences: the same layer from the other board.
    pub other: Option<(Arc<Layer>, Xf)>,
    /// Left and right edges, 0..1 across the picture, for the swipe view.
    pub clip: Option<(f32, f32)>,
}

/// In the Differences view, what's in both sets steps back so changes stand out.
const KEEP_SEE_THROUGH: f32 = 0.45;
/// Light and ink need more of it, or there'd be too little left to mix.
const KEEP_MIXED: f32 = 0.8;
/// Light adds up, so each layer is held back a little to leave room to mix.
const LIGHT_DIM: f32 = 0.8;

const A_ONLY: [f32; 3] = [0.96, 0.33, 0.31];
const B_ONLY: [f32; 3] = [0.30, 0.86, 0.48];

struct Texture {
    tex: glow::Texture,
    size: [i32; 2],
    xf: Xf,
}

struct GpuMesh {
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    ebo: glow::Buffer,
    count: i32,
    picture: Option<Texture>,
}

struct Target {
    fbo: glow::Framebuffer,
    rb: glow::Renderbuffer,
    size: [i32; 2],
}

pub struct Gpu {
    pub(crate) fill: glow::Program,
    mix: glow::Program,
    empty: glow::VertexArray,
    pub(crate) resolve: glow::Framebuffer,
    target: Option<Target>,
    pub(crate) samples: i32,
    meshes: HashMap<u64, GpuMesh>,
    pub(crate) solid: Option<glow::Program>,
    pub(crate) scene: Option<crate::gpu3d::Scene>,
}

const FILL_VS: &str = r"
layout(location = 0) in vec3 a;
uniform vec4 u_xf;
out float v;
void main() {
    v = a.z;
    gl_Position = vec4(a.x * u_xf.x + u_xf.z, a.y * u_xf.y + u_xf.w, 0.0, 1.0);
}";

const FILL_FS: &str = r"
in float v;
out vec4 color;
void main() { color = vec4(v, 0.0, 0.0, 1.0); }";

const MIX_VS: &str = r"
out vec2 uv;
void main() {
    vec2 q = vec2(float((gl_VertexID & 1) * 4 - 1), float((gl_VertexID >> 1) * 4 - 1));
    uv = q * 0.5 + 0.5;
    gl_Position = vec4(q, 0.0, 1.0);
}";

const MIX_FS: &str = r"
in vec2 uv;
out vec4 color;
uniform sampler2D u_a;
uniform sampler2D u_b;
uniform vec3 u_color;
uniform float u_alpha;
uniform float u_invert;
uniform int u_mode; // 0 normal and light, 1 ink, 2 difference, 3 difference in ink
uniform float u_keep; // how strongly what's in both sets shows in a difference
uniform vec3 u_a_only;
uniform vec3 u_b_only;
void main() {
    float a = texture(u_a, uv).r;
    if (u_invert > 0.5) a = 1.0 - a;
    if (u_mode >= 2) {
        float b = texture(u_b, uv).r;
        if (u_invert > 0.5) b = 1.0 - b;
        float both = min(a, b);
        float oa = a - both;
        float ob = b - both;
        float keep = both * u_alpha * u_keep;
        if (u_mode == 3) {
            vec3 ink = mix(vec3(1.0), u_color, keep);
            color = vec4(ink * mix(vec3(1.0), u_a_only, oa) * mix(vec3(1.0), u_b_only, ob), 1.0);
            return;
        }
        color = vec4(u_color * keep + u_a_only * oa + u_b_only * ob, keep + oa + ob);
        return;
    }
    float c = a * u_alpha;
    if (u_mode == 1) {
        color = vec4(mix(vec3(1.0), u_color, c), 1.0);
    } else {
        color = vec4(u_color * c, c);
    }
}";

pub(crate) unsafe fn program(gl: &glow::Context, vs: &str, fs: &str) -> Result<glow::Program, String> {
    unsafe {
        let head = if gl.version().is_embedded {
            "#version 300 es\nprecision highp float;\nprecision highp sampler2D;\n"
        } else {
            "#version 330 core\n"
        };
        let program = gl.create_program()?;
        for (kind, body) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
            let shader = gl.create_shader(kind)?;
            gl.shader_source(shader, &format!("{head}{body}"));
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                return Err(gl.get_shader_info_log(shader));
            }
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(gl.get_program_info_log(program));
        }
        Ok(program)
    }
}

impl Gpu {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        unsafe {
            Ok(Gpu {
                fill: program(gl, FILL_VS, FILL_FS)?,
                mix: program(gl, MIX_VS, MIX_FS)?,
                empty: gl.create_vertex_array()?,
                resolve: gl.create_framebuffer()?,
                target: None,
                samples: gl.get_parameter_i32(glow::MAX_SAMPLES).clamp(0, 8),
                meshes: HashMap::new(),
                solid: None,
                scene: None,
            })
        }
    }

    pub(crate) fn mesh_of(&self, id: u64) -> Option<(glow::VertexArray, i32)> {
        self.meshes.get(&id).map(|m| (m.vao, m.count))
    }

    pub(crate) fn upload(&mut self, gl: &glow::Context, layer: &Layer) -> Result<(), String> {
        if self.meshes.contains_key(&layer.id) {
            return Ok(());
        }
        unsafe {
            let vao = gl.create_vertex_array()?;
            let vbo = gl.create_buffer()?;
            let ebo = gl.create_buffer()?;
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(&layer.mesh.verts), glow::STATIC_DRAW);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ebo));
            gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, as_bytes(&layer.mesh.idx), glow::STATIC_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, 12, 0);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
            self.meshes.insert(
                layer.id,
                GpuMesh { vao, vbo, ebo, count: layer.mesh.idx.len() as i32, picture: None },
            );
        }
        Ok(())
    }

    fn ensure_target(&mut self, gl: &glow::Context, size: [i32; 2]) -> Result<(), String> {
        if self.target.as_ref().is_some_and(|t| t.size == size) {
            return Ok(());
        }
        unsafe {
            if let Some(old) = self.target.take() {
                gl.delete_framebuffer(old.fbo);
                gl.delete_renderbuffer(old.rb);
            }
            let fbo = gl.create_framebuffer()?;
            let rb = gl.create_renderbuffer()?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rb));
            gl.renderbuffer_storage_multisample(glow::RENDERBUFFER, self.samples, glow::R8, size[0], size[1]);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::RENDERBUFFER, Some(rb));
            if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
                return Err("The graphics card refused the drawing surface.".to_string());
            }
            self.target = Some(Target { fbo, rb, size });
        }
        Ok(())
    }

    /// Makes sure this layer's black-and-white picture matches the view.
    fn picture(&mut self, gl: &glow::Context, layer: &Layer, xf: Xf, size: [i32; 2]) -> Result<glow::Texture, String> {
        self.upload(gl, layer)?;
        let (fill, resolve) = (self.fill, self.resolve);
        if let Some(mesh) = self.meshes.get(&layer.id)
            && let Some(p) = &mesh.picture
            && p.size == size
            && p.xf == xf
        {
            return Ok(p.tex);
        }
        self.ensure_target(gl, size)?;
        let target_fbo = self.target.as_ref().map(|t| t.fbo);
        let mesh = self.meshes.get_mut(&layer.id).ok_or("The layer wasn't loaded")?;
        unsafe {
            let tex = match mesh.picture.take() {
                Some(p) if p.size == size => p.tex,
                old => {
                    if let Some(old) = old {
                        gl.delete_texture(old.tex);
                    }
                    let tex = gl.create_texture()?;
                    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
                    gl.tex_storage_2d(glow::TEXTURE_2D, 1, glow::R8, size[0], size[1]);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
                    gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
                    tex
                }
            };
            gl.bind_framebuffer(glow::FRAMEBUFFER, target_fbo);
            gl.viewport(0, 0, size[0], size[1]);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::CULL_FACE);
            gl.color_mask(true, true, true, true);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.use_program(Some(fill));
            let loc = gl.get_uniform_location(fill, "u_xf");
            gl.uniform_4_f32(loc.as_ref(), xf.sx, xf.sy, xf.tx, xf.ty);
            gl.bind_vertex_array(Some(mesh.vao));
            gl.draw_elements(glow::TRIANGLES, mesh.count, glow::UNSIGNED_INT, 0);
            gl.bind_vertex_array(None);

            // Smooths the edges and keeps the result in the layer's picture.
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(resolve));
            gl.framebuffer_texture_2d(glow::DRAW_FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, target_fbo);
            gl.blit_framebuffer(0, 0, size[0], size[1], 0, 0, size[0], size[1], glow::COLOR_BUFFER_BIT, glow::NEAREST);
            mesh.picture = Some(Texture { tex, size, xf });
            Ok(tex)
        }
    }

    fn forget_unused(&mut self, gl: &glow::Context, keep: &HashSet<u64>) {
        self.meshes.retain(|id, mesh| {
            if keep.contains(id) {
                return true;
            }
            unsafe {
                gl.delete_vertex_array(mesh.vao);
                gl.delete_buffer(mesh.vbo);
                gl.delete_buffer(mesh.ebo);
                if let Some(p) = &mesh.picture {
                    gl.delete_texture(p.tex);
                }
            }
            false
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn paint(
        &mut self,
        gl: &glow::Context,
        info: &egui::PaintCallbackInfo,
        fbo: Option<glow::Framebuffer>,
        items: &[Item],
        mix: Mix,
        keep: &HashSet<u64>,
    ) -> Result<(), String> {
        self.forget_unused(gl, keep);
        // Back on the flat view: the solid board's memory can go.
        self.drop_scene(gl);
        let vp = info.viewport_in_pixels();
        let size = [vp.width_px, vp.height_px];
        if size[0] <= 0 || size[1] <= 0 {
            return Ok(());
        }
        let mut pictures = Vec::with_capacity(items.len());
        for item in items {
            let a = self.picture(gl, &item.layer, item.xf, size)?;
            let b = match &item.other {
                Some((layer, xf)) => Some(self.picture(gl, layer, *xf, size)?),
                None => None,
            };
            pictures.push((a, b));
        }

        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, fbo);
            gl.viewport(vp.left_px, vp.from_bottom_px, vp.width_px, vp.height_px);
            let clip = info.clip_rect_in_pixels();
            gl.enable(glow::SCISSOR_TEST);
            gl.enable(glow::BLEND);
            gl.blend_equation(glow::FUNC_ADD);
            gl.use_program(Some(self.mix));
            gl.bind_vertex_array(Some(self.empty));
            let u = |name: &str| gl.get_uniform_location(self.mix, name);
            gl.uniform_1_i32(u("u_a").as_ref(), 0);
            gl.uniform_1_i32(u("u_b").as_ref(), 1);
            gl.uniform_3_f32_slice(u("u_a_only").as_ref(), &A_ONLY);
            gl.uniform_3_f32_slice(u("u_b_only").as_ref(), &B_ONLY);
            for (item, (a, b)) in items.iter().zip(pictures) {
                let (mut left, mut width) = (clip.left_px, clip.width_px);
                if let Some((lo, hi)) = item.clip {
                    let x0 = vp.left_px + (lo * vp.width_px as f32) as i32;
                    let x1 = vp.left_px + (hi * vp.width_px as f32) as i32;
                    left = left.max(x0);
                    width = (x1.min(clip.left_px + clip.width_px) - left).max(0);
                }
                gl.scissor(left, clip.from_bottom_px, width, clip.height_px);
                // The mixing choice holds in the Differences view too.
                let diff = b.is_some();
                let mode = match mix {
                    Mix::Normal => {
                        gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
                        if diff { 2 } else { 0 }
                    }
                    Mix::Light => {
                        gl.blend_func(glow::ONE, glow::ONE);
                        if diff { 2 } else { 0 }
                    }
                    Mix::Ink => {
                        gl.blend_func(glow::ZERO, glow::SRC_COLOR);
                        if diff { 3 } else { 1 }
                    }
                };
                gl.uniform_1_i32(u("u_mode").as_ref(), mode);
                let keep = if mix == Mix::Normal { KEEP_SEE_THROUGH } else { KEEP_MIXED };
                gl.uniform_1_f32(u("u_keep").as_ref(), keep);
                let dim = if mix == Mix::Light && !diff { LIGHT_DIM } else { 1.0 };
                gl.uniform_3_f32_slice(u("u_color").as_ref(), &item.color.map(|c| c * dim));
                gl.uniform_1_f32(u("u_alpha").as_ref(), item.alpha);
                gl.uniform_1_f32(u("u_invert").as_ref(), if item.invert { 1.0 } else { 0.0 });
                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D, Some(a));
                gl.active_texture(glow::TEXTURE1);
                gl.bind_texture(glow::TEXTURE_2D, Some(b.unwrap_or(a)));
                gl.active_texture(glow::TEXTURE0);
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
            }
            gl.bind_vertex_array(None);
            gl.use_program(None);
            gl.disable(glow::SCISSOR_TEST);
        }
        Ok(())
    }
}

pub(crate) fn as_bytes<T: Copy>(values: &[T]) -> &[u8] {
    // Plain numbers laid out one after another, as the graphics card expects.
    unsafe { std::slice::from_raw_parts(values.as_ptr().cast(), std::mem::size_of_val(values)) }
}

/// A piece of the window that the graphics card will draw layers into.
pub fn callback(
    rect: egui::Rect,
    gpu: Arc<Mutex<Option<Result<Gpu, String>>>>,
    items: Vec<Item>,
    mix: Mix,
    keep: Arc<HashSet<u64>>,
    report: Arc<Mutex<Option<String>>>,
) -> egui::PaintCallback {
    let run = eframe::egui_glow::CallbackFn::new(move |info, painter| {
        let gl = painter.gl();
        let Ok(mut slot) = gpu.lock() else { return };
        let state = slot.get_or_insert_with(|| Gpu::new(gl));
        let outcome = match state {
            Ok(gpu) => gpu.paint(gl, &info, painter.intermediate_fbo(), &items, mix, &keep),
            Err(error) => Err(error.clone()),
        };
        if let (Err(error), Ok(mut report)) = (outcome, report.lock()) {
            *report = Some(error);
        }
    });
    egui::PaintCallback { rect, callback: Arc::new(run) }
}
