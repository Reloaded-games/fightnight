//! Terrain meshing and painting. The island is split into square chunks that
//! share one vertex grid layout; each chunk has three LOD index sets (cell stride
//! 1, 2 and 4) plus skirts that hide cracks between chunks of different LOD.

use super::heightmap::Heightmap;
use super::{CELL, CHUNKS, CHUNK_CELLS, WORLD_HALF};
use crate::math::*;
use crate::mesh::{mat, to_srgb8, Vertex};
use crate::noise::*;

pub const LOD_STRIDES: [usize; 3] = [1, 2, 4];
const SKIRT_DEPTH: f32 = 4.0;

#[derive(Clone, Debug)]
pub struct ChunkInfo {
    pub cx: usize,
    pub cz: usize,
    pub aabb: Aabb,
    /// First index / index count per LOD into `TerrainMesh::indices`.
    pub lod_first: [u32; 3],
    pub lod_count: [u32; 3],
    /// Offset of this chunk's first vertex in `TerrainMesh::verts`.
    pub base_vertex: u32,
}

pub struct TerrainMesh {
    pub verts: Vec<Vertex>,
    /// 16-bit indices local to each chunk (add `base_vertex`).
    pub indices: Vec<u16>,
    pub chunks: Vec<ChunkInfo>,
    pub verts_per_chunk: usize,
}

/// Colours the ground: grass with painterly variation, beaches, rock, snow.
#[derive(Clone)]
pub struct TerrainPainter {
    pub seed: u32,
}

#[inline]
fn mix3(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

impl TerrainPainter {
    /// Autumn-ness in [0,1] for a location (drives grass tint and tree species).
    pub fn autumn(&self, x: f32, z: f32) -> f32 {
        smoothstep(0.6, 0.72, fbm01(x / 210.0 + 90.0, z / 210.0 - 40.0, 3, self.seed ^ 0xA7))
    }
    /// Forest density mask in [0,1] (0 = open meadow).
    pub fn forest(&self, x: f32, z: f32) -> f32 {
        smoothstep(0.43, 0.58, fbm01(x / 120.0 - 33.0, z / 120.0 + 12.0, 4, self.seed ^ 0xF0))
    }

    pub fn color(&self, x: f32, z: f32, h: f32, slope: f32) -> Vec3 {
        let s = self.seed;
        let n1 = fbm01(x / 38.0, z / 38.0, 3, s ^ 0x101);
        let n2 = fbm01(x / 11.0 + 50.0, z / 11.0, 2, s ^ 0x102);
        let big = fbm01(x / 210.0 + 7.0, z / 210.0 - 3.0, 3, s ^ 0x103);
        let g_light = hex_v(0x91BE62);
        let g_mid = hex_v(0x6CA34F);
        let g_dark = hex_v(0x487D42);
        let mut c = mix3(g_mid, g_light, n1 * 1.2 - 0.1);
        c = mix3(c, g_dark, smoothstep(0.55, 0.85, n2) * 0.5);
        // sun-bleached meadows on the higher, broader hills
        c = mix3(c, hex_v(0xB6BF69), smoothstep(0.6, 0.85, big) * 0.5);
        // autumn grove tint
        let aut = self.autumn(x, z);
        c = mix3(c, mix3(hex_v(0xD9A441), hex_v(0xC9772F), n1), aut * 0.7);
        // dirt on mid slopes, rock on steep slopes
        c = mix3(c, hex_v(0x9B7A4F), smoothstep(0.34, 0.6, slope) * 0.6);
        let rock = mix3(hex_v(0x8F9097), hex_v(0x6E7076), n2);
        c = mix3(c, rock, smoothstep(0.62, 0.95, slope));
        // snow caps
        let snow_h = 41.0 + (n1 - 0.5) * 6.0;
        c = mix3(c, hex_v(0xF2F7FF), smoothstep(snow_h, snow_h + 4.0, h) * (1.0 - smoothstep(0.7, 1.1, slope) * 0.6));
        // beaches and the sea floor
        let sand = mix3(hex_v(0xF1DFA6), hex_v(0xE3CD8C), n2);
        let sand_w = smoothstep(3.4, 2.0, h + (n1 - 0.5) * 0.9);
        c = mix3(c, sand, sand_w);
        if h < 0.0 {
            let wet = hex_v(0xB9A777);
            c = mix3(c, wet, smoothstep(0.2, -1.0, h));
            c = mix3(c, hex_v(0x6C8A8E), smoothstep(-1.5, -8.0, h));
        }
        c
    }
}

#[inline]
fn hex_v(rgb: u32) -> Vec3 {
    crate::mesh::hex(rgb)
}

pub fn build_terrain_mesh(hm: &Heightmap, painter: &TerrainPainter) -> TerrainMesh {
    let side = CHUNK_CELLS + 1;
    let grid_verts = side * side;
    let skirt_verts = side * 4;
    let verts_per_chunk = grid_verts + skirt_verts;
    let mut verts: Vec<Vertex> = Vec::with_capacity(verts_per_chunk * CHUNKS * CHUNKS);
    let mut indices: Vec<u16> = Vec::new();
    let mut chunks = Vec::with_capacity(CHUNKS * CHUNKS);

    for cz in 0..CHUNKS {
        for cx in 0..CHUNKS {
            let base_vertex = verts.len() as u32;
            let mut bb = Aabb::EMPTY;
            let (gi0, gj0) = (cx * CHUNK_CELLS, cz * CHUNK_CELLS);
            for j in 0..side {
                for i in 0..side {
                    let (gi, gj) = (gi0 + i, gj0 + j);
                    let x = hm.world_x(gi);
                    let z = hm.world_x(gj);
                    let h = hm.get(gi, gj);
                    let n = hm.smooth_normal(gi, gj);
                    let slope = (1.0 - n.y * n.y).max(0.0).sqrt() / n.y.max(1e-3);
                    let c = painter.color(x, z, h, slope);
                    bb.extend(Vec3::new(x, h, z));
                    verts.push(Vertex { pos: [x, h, z], nrm: n.to_array(), col: to_srgb8(c), attr: [255, mat::TERRAIN, 0, 0] });
                }
            }
            // skirts: copies of border vertices pushed down. Order: N(z=0), S(z=max), W(x=0), E(x=max)
            let border = |k: usize, e: usize| -> (usize, usize) {
                match e {
                    0 => (k, 0),
                    1 => (k, side - 1),
                    2 => (0, k),
                    _ => (side - 1, k),
                }
            };
            for e in 0..4 {
                for k in 0..side {
                    let (i, j) = border(k, e);
                    let src = verts[base_vertex as usize + j * side + i];
                    let mut v = src;
                    v.pos[1] -= SKIRT_DEPTH;
                    bb.extend(Vec3::from(v.pos));
                    verts.push(v);
                }
            }

            let mut lod_first = [0u32; 3];
            let mut lod_count = [0u32; 3];
            for (l, &stride) in LOD_STRIDES.iter().enumerate() {
                lod_first[l] = indices.len() as u32;
                let steps = CHUNK_CELLS / stride;
                for sj in 0..steps {
                    for si in 0..steps {
                        let (i, j) = (si * stride, sj * stride);
                        let v00 = (j * side + i) as u16;
                        let v10 = (j * side + i + stride) as u16;
                        let v01 = ((j + stride) * side + i) as u16;
                        let v11 = ((j + stride) * side + i + stride) as u16;
                        indices.extend_from_slice(&[v00, v01, v11, v00, v11, v10]);
                    }
                }
                // skirts at this LOD
                for e in 0..4 {
                    let outward = match e {
                        0 => Vec3::NEG_Z,
                        1 => Vec3::Z,
                        2 => Vec3::NEG_X,
                        _ => Vec3::X,
                    };
                    let mut k = 0;
                    while k + stride <= CHUNK_CELLS {
                        let (ia, ja) = border(k, e);
                        let (ib, jb) = border(k + stride, e);
                        let ta = (ja * side + ia) as u16;
                        let tb = (jb * side + ib) as u16;
                        let ba = (grid_verts + e * side + k) as u16;
                        let bb_ = (grid_verts + e * side + k + stride) as u16;
                        for tri in [[ta, tb, ba], [tb, bb_, ba]] {
                            let p = |v: u16| Vec3::from(verts[base_vertex as usize + v as usize].pos);
                            let n = (p(tri[1]) - p(tri[0])).cross(p(tri[2]) - p(tri[0]));
                            if n.dot(outward) >= 0.0 {
                                indices.extend_from_slice(&tri);
                            } else {
                                indices.extend_from_slice(&[tri[0], tri[2], tri[1]]);
                            }
                        }
                        k += stride;
                    }
                }
                lod_count[l] = indices.len() as u32 - lod_first[l];
            }
            chunks.push(ChunkInfo { cx, cz, aabb: bb, lod_first, lod_count, base_vertex });
        }
    }
    TerrainMesh { verts, indices, chunks, verts_per_chunk }
}

/// World-space centre of a chunk.
pub fn chunk_center(cx: usize, cz: usize) -> Vec2 {
    let s = CHUNK_CELLS as f32 * CELL;
    Vec2::new(-WORLD_HALF + (cx as f32 + 0.5) * s, -WORLD_HALF + (cz as f32 + 0.5) * s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::gen::*;

    #[test]
    fn terrain_mesh_is_upward_facing_and_watertight_count() {
        let base = BaseTerrain::new(5);
        let layout = plan_layout(&base);
        let hm = build_heightmap(&base, &layout);
        let painter = TerrainPainter { seed: 5 };
        let tm = build_terrain_mesh(&hm, &painter);
        assert_eq!(tm.chunks.len(), CHUNKS * CHUNKS);
        let side = CHUNK_CELLS + 1;
        assert_eq!(tm.verts.len(), tm.chunks.len() * (side * side + side * 4));
        // Every non-skirt triangle in LOD0 faces up.
        let c = &tm.chunks[CHUNKS * 7 + 9];
        let first = c.lod_first[0] as usize;
        let grid_tris = CHUNK_CELLS * CHUNK_CELLS * 2;
        for t in 0..grid_tris {
            let idx = &tm.indices[first + t * 3..first + t * 3 + 3];
            let p = |i: u16| Vec3::from(tm.verts[c.base_vertex as usize + i as usize].pos);
            let n = (p(idx[1]) - p(idx[0])).cross(p(idx[2]) - p(idx[0]));
            assert!(n.y > 0.0, "terrain triangle faces down");
        }
        // Skirts face outward (horizontal normal component outward).
        let skirt_first = first + grid_tris * 3;
        let skirt_end = (c.lod_first[0] + c.lod_count[0]) as usize;
        assert!(skirt_end > skirt_first);
        // LOD counts shrink.
        assert!(c.lod_count[1] < c.lod_count[0] && c.lod_count[2] < c.lod_count[1]);
        // Index values within the chunk's range.
        assert!(tm.indices.iter().all(|&i| (i as usize) < tm.verts_per_chunk));
        // Heights in mesh match heightmap exactly.
        let v = &tm.verts[c.base_vertex as usize + 5 * side + 7];
        let (gi, gj) = (c.cx * CHUNK_CELLS + 7, c.cz * CHUNK_CELLS + 5);
        assert!((v.pos[1] - hm.get(gi, gj)).abs() < 1e-5);
    }

    #[test]
    fn painter_colors_are_sane() {
        let p = TerrainPainter { seed: 3 };
        for k in 0..200 {
            let c = p.color(k as f32 * 7.3, k as f32 * 3.1, (k % 50) as f32 - 5.0, (k % 10) as f32 * 0.1);
            assert!(c.cmpge(Vec3::splat(-0.01)).all() && c.cmple(Vec3::splat(1.01)).all(), "{c:?}");
        }
        // beaches are sandy (red > blue), grass is green-dominant
        let sand = p.color(0.0, 0.0, 1.2, 0.0);
        assert!(sand.x > sand.z);
        let grass = p.color(10.0, 10.0, 12.0, 0.05);
        assert!(grass.y > grass.x && grass.y > grass.z, "{grass:?}");
    }
}
