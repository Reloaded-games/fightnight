//! Plain data shared between the game layer and the renderer.

use bytemuck::{Pod, Zeroable};
use fn_core::camera::Camera;
use fn_core::mesh::{Instance, Particle};
use glam::{Vec3, Vec4};

pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const CASCADES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    /// 1 or 4.
    pub msaa: u32,
    pub shadow_size: u32,
    pub shadows: bool,
    pub render_scale: f32,
    pub bloom: bool,
    /// Draw distance multiplier for props and grass.
    pub detail: f32,
}

impl Quality {
    pub fn low() -> Self {
        Self { msaa: 1, shadow_size: 1024, shadows: true, render_scale: 0.75, bloom: false, detail: 0.6 }
    }
    pub fn medium() -> Self {
        Self { msaa: 4, shadow_size: 1024, shadows: true, render_scale: 1.0, bloom: true, detail: 0.85 }
    }
    pub fn high() -> Self {
        Self { msaa: 4, shadow_size: 2048, shadows: true, render_scale: 1.0, bloom: true, detail: 1.0 }
    }
    pub fn from_name(n: &str) -> Self {
        match n {
            "low" => Self::low(),
            "medium" => Self::medium(),
            _ => Self::high(),
        }
    }
}

/// A contiguous run of instances drawn with one mesh.
#[derive(Clone, Copy, Debug)]
pub struct Batch {
    pub mesh: u16,
    pub first: u32,
    pub count: u32,
    pub shadow: bool,
}

pub struct FrameInput<'a> {
    pub time: f32,
    pub camera: Camera,
    pub sun_dir: Vec3,
    /// centre.x, centre.z, radius, strength
    pub storm: Vec4,
    pub storm_time: f32,
    /// 0..1: how strongly the post tint for being inside the storm applies.
    pub in_storm: f32,
    pub damage: f32,
    pub vignette: f32,
    pub wind: f32,
    pub batches: &'a [Batch],
    pub instances: &'a [Instance],
    pub ghost_batches: &'a [Batch],
    pub ghost_instances: &'a [Instance],
    pub particles: &'a [Particle],
    pub particles_add: &'a [Particle],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GlobalsU {
    pub view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub cascade_vp: [[[f32; 4]; 4]; 3],
    pub cam_pos: [f32; 4],
    pub cam_right: [f32; 4],
    pub cam_up: [f32; 4],
    pub cam_fwd: [f32; 4],
    pub sun_dir: [f32; 4],
    pub sun_color: [f32; 4],
    pub sky_color: [f32; 4],
    pub ground_color: [f32; 4],
    pub fog_params: [f32; 4],
    pub storm: [f32; 4],
    pub cascade_splits: [f32; 4],
    pub shadow_info: [f32; 4],
    pub screen: [f32; 4],
    pub post: [f32; 4],
    pub world: [f32; 4],
    pub misc: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ShadowU {
    pub vp: [[f32; 4]; 4],
    pub params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct PostU {
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub c: [f32; 4],
    pub d: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct MeshRange {
    pub first_index: u32,
    pub index_count: u32,
    pub base_vertex: i32,
}

/// Counters exposed to the debug overlay.
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub draw_calls: u32,
    pub triangles: u64,
    pub chunks_drawn: u32,
    pub instances: u32,
}
