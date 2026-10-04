//! GPU-side copies of the static world content (props, building meshes, grass)
//! and the per-frame culling that turns them into a small number of draw calls.

use super::types::*;
use bytemuck::cast_slice;
use fn_core::camera::Frustum;
use fn_core::math::*;
use fn_core::mesh::{Instance, MeshData};
use fn_core::meshlib::MeshId;
use fn_core::world::props::*;
use fn_core::world::{World, CHUNKS, CHUNK_SIZE, WORLD_HALF};
use std::collections::HashMap;
use wgpu::util::DeviceExt;
use wgpu::*;

pub struct RProp {
    pub kind: PropKind,
    pub inst: Instance,
    pub center: Vec3,
    pub radius: f32,
    pub alive: bool,
}

pub struct StaticMeshGpu {
    pub vb: Buffer,
    pub ib: Buffer,
    pub index_count: u32,
    pub aabb: Aabb,
}

pub struct GrassChunk {
    pub grass: Option<(Buffer, u32)>,
    pub flowers: Option<(Buffer, u32)>,
    pub aabb: Aabb,
}

#[derive(Default)]
pub struct WorldGpu {
    pub props: Vec<Vec<RProp>>,
    /// Bounding boxes of each chunk's props (for chunk-level culling).
    pub prop_bounds: Vec<Aabb>,
    pub static_meshes: Vec<StaticMeshGpu>,
    pub grass: HashMap<usize, GrassChunk>,
    pub chunk_centers: Vec<Vec2>,
}

fn prop_radius(kind: PropKind, scale: f32) -> f32 {
    scale
        * match kind {
            PropKind::Pine => 5.0,
            PropKind::Oak => 4.2,
            PropKind::Birch => 3.8,
            PropKind::Palm => 4.6,
            PropKind::Bush => 1.6,
            PropKind::Boulder => 3.0,
            PropKind::Rock0 | PropKind::Rock1 | PropKind::Rock2 => 1.8,
            _ => 1.6,
        }
}

fn prop_center(p: &PropInst) -> Vec3 {
    let up = match p.kind {
        PropKind::Pine => 5.0,
        PropKind::Oak => 4.0,
        PropKind::Birch => 3.6,
        PropKind::Palm => 3.6,
        _ => 0.8,
    };
    p.pos + Vec3::Y * up * p.scale
}

pub fn build_world_gpu(device: &Device, world: &World) -> WorldGpu {
    let mut g = WorldGpu::default();
    let n = CHUNKS * CHUNKS;
    g.props = (0..n).map(|_| Vec::new()).collect();
    g.prop_bounds = vec![Aabb::EMPTY; n];
    for (ci, list) in world.chunk_props.iter().enumerate() {
        for p in list {
            let c = prop_center(p);
            let r = prop_radius(p.kind, p.scale);
            g.prop_bounds[ci].extend(c - Vec3::splat(r));
            g.prop_bounds[ci].extend(c + Vec3::splat(r));
            g.props[ci].push(RProp { kind: p.kind, inst: p.instance(), center: c, radius: r, alive: true });
        }
    }
    for cm in &world.chunk_meshes {
        g.static_meshes.push(upload_static(device, &cm.mesh, cm.aabb));
    }
    g.chunk_centers = (0..n).map(|i| fn_core::world::terrain_mesh::chunk_center(i % CHUNKS, i / CHUNKS)).collect();
    g
}

fn upload_static(device: &Device, m: &MeshData, aabb: Aabb) -> StaticMeshGpu {
    StaticMeshGpu {
        vb: device.create_buffer_init(&util::BufferInitDescriptor { label: Some("static-vb"), contents: cast_slice(&m.verts), usage: BufferUsages::VERTEX }),
        ib: device.create_buffer_init(&util::BufferInitDescriptor { label: Some("static-ib"), contents: cast_slice(&m.idx), usage: BufferUsages::INDEX }),
        index_count: m.idx.len() as u32,
        aabb,
    }
}

/// Instances to draw this frame grouped by (mesh), already appended to `out`.
pub struct PropPass {
    pub batches: Vec<Batch>,
}

/// Cull and LOD-select props for one view. Instances are appended to `out`; the returned batches
/// index into it with `first` offset by `base`.
pub fn gather_props(
    g: &WorldGpu,
    frustum: &Frustum,
    cam: Vec3,
    max_dist_scale: f32,
    shadow_pass: bool,
    shadow_range: f32,
    out: &mut Vec<Instance>,
    base: u32,
) -> Vec<Batch> {
    // per (kind, lod) lists
    let mut lists: Vec<Vec<Instance>> = vec![Vec::new(); PROP_KINDS.len() * 2];
    let cam_xz = Vec2::new(cam.x, cam.z);
    for (ci, chunk) in g.props.iter().enumerate() {
        if chunk.is_empty() {
            continue;
        }
        let center = g.chunk_centers[ci];
        let dchunk = center.distance(cam_xz) - CHUNK_SIZE * 0.75;
        let limit = if shadow_pass { shadow_range } else { 520.0 * max_dist_scale };
        if dchunk > limit {
            continue;
        }
        if !frustum.intersects_aabb(&g.prop_bounds[ci]) {
            continue;
        }
        for p in chunk {
            if !p.alive {
                continue;
            }
            let d = p.center.distance(cam);
            let maxd = if shadow_pass { shadow_range } else { p.kind.draw_distance() * max_dist_scale };
            if d - p.radius > maxd {
                continue;
            }
            if shadow_pass && !p.kind.casts_shadow() {
                continue;
            }
            if !frustum.intersects_sphere(p.center, p.radius) {
                continue;
            }
            let lod = if shadow_pass {
                1
            } else {
                match p.kind {
                    PropKind::Pine | PropKind::Oak | PropKind::Birch | PropKind::Palm => (d > 120.0 * max_dist_scale) as usize,
                    PropKind::Bush => (d > 55.0) as usize,
                    _ => 0,
                }
            };
            lists[p.kind as usize * 2 + lod].push(p.inst);
        }
    }
    let mut batches = vec![];
    for (i, list) in lists.iter().enumerate() {
        if list.is_empty() {
            continue;
        }
        let kind = PROP_KINDS[i / 2];
        let lod = i % 2;
        batches.push(Batch { mesh: kind.mesh(lod).idx(), first: base + out.len() as u32, count: list.len() as u32, shadow: false });
        out.extend_from_slice(list);
    }
    batches
}

/// Generate ground cover for the chunks around the camera (a couple per call) and drop far ones.
pub fn update_ground_cover(device: &Device, g: &mut WorldGpu, world: &World, cam: Vec3, radius: f32) {
    let cam_xz = Vec2::new(cam.x, cam.z);
    let mut generated = 0;
    let mut wanted: Vec<(f32, usize)> = vec![];
    for ci in 0..CHUNKS * CHUNKS {
        let d = g.chunk_centers[ci].distance(cam_xz);
        if d < radius + CHUNK_SIZE * 0.7 && !g.grass.contains_key(&ci) {
            wanted.push((d, ci));
        }
    }
    wanted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (_, ci) in wanted {
        if generated >= 2 {
            break;
        }
        let (cx, cz) = (ci % CHUNKS, ci / CHUNKS);
        let (grass, flowers) = fn_core::world::props::scatter_ground_cover(world, cx, cz);
        let x0 = -WORLD_HALF + cx as f32 * CHUNK_SIZE;
        let z0 = -WORLD_HALF + cz as f32 * CHUNK_SIZE;
        let mut aabb = Aabb::EMPTY;
        aabb.extend(Vec3::new(x0, -5.0, z0));
        aabb.extend(Vec3::new(x0 + CHUNK_SIZE, 90.0, z0 + CHUNK_SIZE));
        let mk = |v: &Vec<Instance>, label: &'static str| -> Option<(Buffer, u32)> {
            if v.is_empty() {
                None
            } else {
                Some((device.create_buffer_init(&util::BufferInitDescriptor { label: Some(label), contents: cast_slice(v), usage: BufferUsages::VERTEX }), v.len() as u32))
            }
        };
        g.grass.insert(ci, GrassChunk { grass: mk(&grass, "grass"), flowers: mk(&flowers, "flowers"), aabb });
        generated += 1;
    }
    let far = radius * 1.7 + CHUNK_SIZE;
    let centers = &g.chunk_centers;
    g.grass.retain(|ci, _| centers[*ci].distance(cam_xz) < far);
}

pub fn mesh_ids_for_grass() -> (u16, u16) {
    (MeshId::GrassTuft.idx(), MeshId::FlowerClump.idx())
}
