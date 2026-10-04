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
use std::sync::OnceLock;
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

/// Rotation-independent spheres enclosing both LODs and the brick theme. Derive these
/// once from the rendered geometry; a species' gameplay size is not its canopy size.
fn prop_geometry_bounds() -> &'static [(f32, f32); PROP_KINDS.len()] {
    static BOUNDS: OnceLock<[(f32, f32); PROP_KINDS.len()]> = OnceLock::new();
    BOUNDS.get_or_init(|| PROP_KINDS.map(|kind| {
        let meshes = [kind.mesh(0), kind.mesh(1), fn_core::lego_models::mapped_mesh(kind.mesh(0), fn_core::game::GameMode::Lego)]
            .map(fn_core::meshlib::build_mesh);
        let mut bounds = Aabb::EMPTY;
        for mesh in &meshes {
            let b = mesh.bounds();
            bounds.extend(b.min);
            bounds.extend(b.max);
        }
        let up = bounds.center().y;
        let center = Vec3::Y * up;
        let radius = meshes.iter().flat_map(|m| &m.verts).map(|v| Vec3::from(v.pos).distance(center)).fold(0.0f32, f32::max);
        (up, radius)
    }))
}

fn prop_radius(kind: PropKind, scale: f32) -> f32 {
    // The mesh/shadow shaders apply at most 0.257m of world-space wind displacement.
    prop_geometry_bounds()[kind as usize].1 * scale + 0.3
}

fn prop_center(p: &PropInst) -> Vec3 {
    let up = prop_geometry_bounds()[p.kind as usize].0;
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

/// Cull and LOD-select props for one view. Instances are appended to `out`; the returned batches
/// index into it with `first` offset by `base`.
pub fn gather_props(
    g: &WorldGpu,
    mode: fn_core::game::GameMode,
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
                match p.kind.base() {
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
        batches.push(Batch { mesh: fn_core::lego_models::mapped_mesh(kind.mesh(lod), mode).idx(), first: base + out.len() as u32, count: list.len() as u32, shadow: false });
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

pub fn mesh_ids_for_grass(mode: fn_core::game::GameMode) -> (u16, u16) {
    (fn_core::lego_models::mapped_mesh(MeshId::GrassTuft, mode).idx(), fn_core::lego_models::mapped_mesh(MeshId::FlowerClump, mode).idx())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fn_core::game::GameMode;

    #[test]
    fn prop_spheres_enclose_every_lod_and_theme_after_rotation_and_wind() {
        for kind in PROP_KINDS {
            for mode in [GameMode::BattleRoyale, GameMode::Lego] {
                for lod in 0..2 {
                    let mesh = fn_core::meshlib::build_mesh(fn_core::lego_models::mapped_mesh(kind.mesh(lod), mode));
                    for scale in [0.55, 1.0, 1.6] {
                        for yaw in [0.0, 0.7, 1.8, 3.4] {
                            let p = PropInst { kind, pos: Vec3::new(17.0, 6.0, -91.0), yaw, scale, tint: [1.0; 3] };
                            let center = prop_center(&p);
                            let radius = prop_radius(kind, scale);
                            let aabb = Aabb::from_center_half(center, Vec3::splat(radius));
                            let xf = Mat4::from_translation(p.pos) * Mat4::from_rotation_y(yaw) * Mat4::from_scale(Vec3::splat(scale));
                            for v in &mesh.verts {
                                for wind in [Vec3::new(0.22, 0.0, 0.132), Vec3::new(-0.22, 0.0, -0.132)] {
                                    let point = xf.transform_point3(Vec3::from(v.pos)) + wind;
                                    assert!(point.distance(center) <= radius + 1e-4, "{kind:?} {mode:?} LOD {lod}: vertex outside sphere");
                                    assert!(aabb.contains(point), "{kind:?} {mode:?} LOD {lod}: vertex outside chunk bounds");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn canopy_at_frustum_edge_keeps_prop_and_chunk_visible() {
        for kind in [PropKind::PineSlim, PropKind::OakBroad, PropKind::BirchTall, PropKind::BushFlower] {
            let p = PropInst { kind, pos: Vec3::new(-12.0, 4.0, -25.0), yaw: 0.7, scale: 1.4, tint: [1.0; 3] };
            let center = prop_center(&p);
            let radius = prop_radius(kind, p.scale);
            let aabb = Aabb::from_center_half(center, Vec3::splat(radius));
            let xf = Mat4::from_translation(p.pos) * Mat4::from_rotation_y(p.yaw) * Mat4::from_scale(Vec3::splat(p.scale));
            for mode in [GameMode::BattleRoyale, GameMode::Lego] {
                for lod in 0..2 {
                    let mesh = fn_core::meshlib::build_mesh(fn_core::lego_models::mapped_mesh(kind.mesh(lod), mode));
                    for normal in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z] {
                        let edge = mesh.verts.iter().map(|v| normal.dot(xf.transform_point3(Vec3::from(v.pos)))).fold(f32::NEG_INFINITY, f32::max);
                        let frustum = Frustum { planes: [normal.extend(-edge); 5] };
                        assert!(frustum.intersects_sphere(center, radius), "{kind:?} {mode:?} LOD {lod}: canopy incorrectly culled");
                        assert!(frustum.intersects_aabb(&aabb), "{kind:?} {mode:?} LOD {lod}: chunk incorrectly culled");
                    }
                }
            }
        }
    }
}
