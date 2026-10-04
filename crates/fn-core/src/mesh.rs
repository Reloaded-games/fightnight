//! Procedural mesh generation. All game geometry (terrain, trees, houses,
//! characters, weapons...) is authored in code with `MeshBuilder`, so the game
//! ships without a single model file.
//!
//! Conventions: right handed, Y up, counter-clockwise front faces. Colours are
//! authored in sRGB; the shaders convert them to linear.

use crate::math::*;
use crate::rng::hash3;
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;

/// Material ids stored in `Vertex::attr[1]`; the shader switches on these to add
/// procedural detail (wood grain, shingles, wind sway ...).
pub mod mat {
    pub const FLAT: u8 = 0;
    pub const FOLIAGE: u8 = 1;
    pub const GRASS: u8 = 2;
    pub const WOOD: u8 = 3;
    pub const BRICK: u8 = 4;
    pub const SHINGLE: u8 = 5;
    pub const METAL: u8 = 6;
    pub const GLASS: u8 = 7;
    pub const TERRAIN: u8 = 8;
    pub const EMISSIVE: u8 = 9;
    pub const SKIN: u8 = 10;
    pub const CLOTH: u8 = 11;
    pub const STONE: u8 = 12;
    pub const PLASTER: u8 = 13;
    pub const ASPHALT: u8 = 14;
    pub const BARK: u8 = 15;
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    /// sRGB colour, alpha unused (kept for alignment / future use).
    pub col: [u8; 4],
    /// [ambient occlusion, material id, wind sway weight, specular]
    pub attr: [u8; 4],
}

/// Per instance data for instanced draws. `m0..m2` are the rows of a 3x4
/// affine transform.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Instance {
    pub m0: [f32; 4],
    pub m1: [f32; 4],
    pub m2: [f32; 4],
    /// Colour multiplier (rgb, linear-ish) and alpha.
    pub color: [f32; 4],
    /// x: flash/emissive add, y: opacity (1 = solid, < 1 dissolves), z/w: free.
    pub params: [f32; 4],
}

impl Instance {
    pub fn from_mat4(m: Mat4, color: [f32; 4]) -> Self {
        let c = m.to_cols_array_2d();
        Self {
            m0: [c[0][0], c[1][0], c[2][0], c[3][0]],
            m1: [c[0][1], c[1][1], c[2][1], c[3][1]],
            m2: [c[0][2], c[1][2], c[2][2], c[3][2]],
            color,
            params: [0.0, 1.0, 0.0, 0.0],
        }
    }
    pub fn at(pos: Vec3, yaw: f32, scale: f32, color: [f32; 4]) -> Self {
        Self::from_mat4(
            Mat4::from_translation(pos) * Mat4::from_rotation_y(yaw) * Mat4::from_scale(Vec3::splat(scale)),
            color,
        )
    }
    pub fn with_params(mut self, p: [f32; 4]) -> Self {
        self.params = p;
        self
    }
}

/// A camera-facing (or axis-stretched) billboard particle for the GPU.
/// `a` = (position.xyz, size), `b` = (axis.xyz, length), `color` = premultiply-ready
/// rgba, `c` = (shape, rotation, glow, seed). Shapes: 0 glow, 1 smoke, 2 spark,
/// 3 ring, 4 beam, 5 tracer, 6 star/flash.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Particle {
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub color: [f32; 4],
    pub c: [f32; 4],
}

pub mod shape {
    pub const GLOW: f32 = 0.0;
    pub const SMOKE: f32 = 1.0;
    pub const SPARK: f32 = 2.0;
    pub const RING: f32 = 3.0;
    pub const BEAM: f32 = 4.0;
    pub const TRACER: f32 = 5.0;
    pub const STAR: f32 = 6.0;
}

impl Particle {
    pub fn billboard(pos: Vec3, size: f32, color: [f32; 4], shape: f32, rot: f32, seed: f32) -> Self {
        Self { a: [pos.x, pos.y, pos.z, size], b: [0.0, 1.0, 0.0, 0.0], color, c: [shape, rot, 0.0, seed] }
    }
    /// Stretched quad from `pos` along `axis` for `length` metres (beams/tracers) or centred (sparks).
    pub fn stretched(pos: Vec3, axis: Vec3, length: f32, width: f32, color: [f32; 4], shape: f32, seed: f32) -> Self {
        Self { a: [pos.x, pos.y, pos.z, width], b: [axis.x, axis.y, axis.z, length], color, c: [shape, 0.0, 0.0, seed] }
    }
}

#[derive(Clone, Debug, Default)]
pub struct MeshData {
    pub verts: Vec<Vertex>,
    pub idx: Vec<u32>,
}

impl MeshData {
    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }
    pub fn tri_count(&self) -> usize {
        self.idx.len() / 3
    }
    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for v in &self.verts {
            b.extend(Vec3::from(v.pos));
        }
        b
    }
    /// Append another mesh, transformed by `m`.
    pub fn append(&mut self, other: &MeshData, m: Mat4) {
        let base = self.verts.len() as u32;
        let nm = normal_matrix(m);
        for v in &other.verts {
            let mut w = *v;
            w.pos = m.transform_point3(Vec3::from(v.pos)).to_array();
            w.nrm = (nm * Vec3::from(v.nrm)).normalize_or_zero().to_array();
            self.verts.push(w);
        }
        self.idx.extend(other.idx.iter().map(|i| i + base));
    }
}

fn normal_matrix(m: Mat4) -> Mat3 {
    let m3 = Mat3::from_mat4(m);
    if m3.determinant().abs() < 1e-12 {
        Mat3::IDENTITY
    } else {
        m3.inverse().transpose()
    }
}

#[inline]
pub fn to_srgb8(c: Vec3) -> [u8; 4] {
    [
        (c.x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        (c.y.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        (c.z.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        255,
    ]
}

/// 0xRRGGBB to a colour vector.
#[inline]
pub fn hex(rgb: u32) -> Vec3 {
    Vec3::new(((rgb >> 16) & 255) as f32 / 255.0, ((rgb >> 8) & 255) as f32 / 255.0, (rgb & 255) as f32 / 255.0)
}

/// Small 3D value noise for displacing blobs.
fn noise3(p: Vec3, seed: u32) -> f32 {
    let f = p.floor();
    let t = p - f;
    let t = t * t * (Vec3::splat(3.0) - 2.0 * t);
    let (x, y, z) = (f.x as i32, f.y as i32, f.z as i32);
    let h = |dx: i32, dy: i32, dz: i32| (hash3(x + dx, y + dy, z + dz, seed) >> 8) as f32 / 16_777_216.0;
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = l(h(0, 0, 0), h(1, 0, 0), t.x);
    let x10 = l(h(0, 1, 0), h(1, 1, 0), t.x);
    let x01 = l(h(0, 0, 1), h(1, 0, 1), t.x);
    let x11 = l(h(0, 1, 1), h(1, 1, 1), t.x);
    l(l(x00, x10, t.y), l(x01, x11, t.y), t.z) * 2.0 - 1.0
}

pub struct MeshBuilder {
    pub mesh: MeshData,
    xf: Mat4,
    nm: Mat3,
    stack: Vec<(Mat4, Mat3)>,
    col: [u8; 4],
    mat: u8,
    sway: u8,
    spec: u8,
    ao_lo: f32,
    ao_hi: f32,
}

impl Default for MeshBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MeshBuilder {
    pub fn new() -> Self {
        Self {
            mesh: MeshData::default(),
            xf: Mat4::IDENTITY,
            nm: Mat3::IDENTITY,
            stack: Vec::new(),
            col: [200, 200, 200, 255],
            mat: mat::FLAT,
            sway: 0,
            spec: 0,
            ao_lo: 1.0,
            ao_hi: 1.0,
        }
    }

    pub fn finish(self) -> MeshData {
        self.mesh
    }

    // ----- state -----------------------------------------------------------
    pub fn color(&mut self, c: Vec3) -> &mut Self {
        self.col = to_srgb8(c);
        self
    }
    pub fn hex(&mut self, rgb: u32) -> &mut Self {
        self.col = to_srgb8(hex(rgb));
        self
    }
    pub fn mat(&mut self, m: u8) -> &mut Self {
        self.mat = m;
        self
    }
    pub fn sway(&mut self, s: f32) -> &mut Self {
        self.sway = (s.clamp(0.0, 1.0) * 255.0) as u8;
        self
    }
    pub fn spec(&mut self, s: f32) -> &mut Self {
        self.spec = (s.clamp(0.0, 1.0) * 255.0) as u8;
        self
    }
    /// Ambient-occlusion factors (1 = unoccluded) applied from bottom to top of
    /// each primitive; gives cheap contact shading without SSAO.
    pub fn ao(&mut self, lo: f32, hi: f32) -> &mut Self {
        self.ao_lo = lo;
        self.ao_hi = hi;
        self
    }
    /// Reset material state to plain flat shading.
    pub fn reset_style(&mut self) -> &mut Self {
        self.mat = mat::FLAT;
        self.sway = 0;
        self.spec = 0;
        self.ao_lo = 1.0;
        self.ao_hi = 1.0;
        self
    }

    pub fn push_xf(&mut self, m: Mat4) -> &mut Self {
        self.stack.push((self.xf, self.nm));
        self.xf *= m;
        self.nm = normal_matrix(self.xf);
        self
    }
    pub fn pop_xf(&mut self) -> &mut Self {
        if let Some((x, n)) = self.stack.pop() {
            self.xf = x;
            self.nm = n;
        }
        self
    }
    pub fn with_xf(&mut self, m: Mat4, f: impl FnOnce(&mut Self)) -> &mut Self {
        self.push_xf(m);
        f(self);
        self.pop_xf();
        self
    }

    // ----- raw access ------------------------------------------------------
    /// Add a vertex given in local space; `ao` is a multiplier in [0,1].
    pub fn vert(&mut self, p: Vec3, n: Vec3, ao: f32) -> u32 {
        let wp = self.xf.transform_point3(p);
        let wn = (self.nm * n).normalize_or_zero();
        self.mesh.verts.push(Vertex {
            pos: wp.to_array(),
            nrm: wn.to_array(),
            col: self.col,
            attr: [(ao.clamp(0.0, 1.0) * 255.0) as u8, self.mat, self.sway, self.spec],
        });
        (self.mesh.verts.len() - 1) as u32
    }
    /// Add a vertex with an explicit colour (for gradients).
    pub fn vert_c(&mut self, p: Vec3, n: Vec3, ao: f32, c: Vec3) -> u32 {
        let i = self.vert(p, n, ao);
        self.mesh.verts[i as usize].col = to_srgb8(c);
        i
    }
    pub fn set_sway(&mut self, v: u32, sway: f32) {
        self.mesh.verts[v as usize].attr[2] = (sway.clamp(0.0, 1.0) * 255.0) as u8;
    }

    /// Add a triangle; the winding is flipped if needed so that it agrees with
    /// the vertex normals.
    pub fn tri(&mut self, a: u32, b: u32, c: u32) {
        let v = &self.mesh.verts;
        let pa = Vec3::from(v[a as usize].pos);
        let pb = Vec3::from(v[b as usize].pos);
        let pc = Vec3::from(v[c as usize].pos);
        let n = (pb - pa).cross(pc - pa);
        let vn = Vec3::from(v[a as usize].nrm) + Vec3::from(v[b as usize].nrm) + Vec3::from(v[c as usize].nrm);
        if n.dot(vn) >= 0.0 {
            self.mesh.idx.extend_from_slice(&[a, b, c]);
        } else {
            self.mesh.idx.extend_from_slice(&[a, c, b]);
        }
    }
    pub fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.tri(a, b, c);
        self.tri(a, c, d);
    }

    /// A flat-shaded quad from four local-space corners (any winding).
    pub fn quad_flat(&mut self, p: [Vec3; 4], ao: [f32; 4]) {
        let n = (p[1] - p[0]).cross(p[3] - p[0]).normalize_or_zero();
        let n = if n == Vec3::ZERO { (p[2] - p[0]).cross(p[3] - p[1]).normalize_or_zero() } else { n };
        let i: Vec<u32> = (0..4).map(|k| self.vert(p[k], n, ao[k])).collect();
        self.quad(i[0], i[1], i[2], i[3]);
    }

    /// Flat quad whose normal is forced to point along `outward` (corner order is fixed up).
    pub fn quad_out(&mut self, p: [Vec3; 4], outward: Vec3) {
        let n = (p[1] - p[0]).cross(p[3] - p[0]);
        let n = if n.length_squared() < 1e-12 { (p[2] - p[0]).cross(p[3] - p[1]) } else { n };
        if n.dot(outward) >= 0.0 {
            self.quad_flat(p, [1.0; 4]);
        } else {
            self.quad_flat([p[3], p[2], p[1], p[0]], [1.0; 4]);
        }
    }

    /// Flat triangle whose normal is forced to point along `outward`.
    pub fn tri_out(&mut self, a: Vec3, b: Vec3, c: Vec3, outward: Vec3) {
        if (b - a).cross(c - a).dot(outward) >= 0.0 {
            self.tri_flat(a, b, c);
        } else {
            self.tri_flat(a, c, b);
        }
    }

    /// A flat-shaded triangle from three local-space corners.
    pub fn tri_flat(&mut self, a: Vec3, b: Vec3, c: Vec3) {
        let n = (b - a).cross(c - a).normalize_or_zero();
        let (ia, ib, ic) = (self.vert(a, n, 1.0), self.vert(b, n, 1.0), self.vert(c, n, 1.0));
        self.tri(ia, ib, ic);
    }

    // ----- primitives -------------------------------------------------------
    pub fn box_center(&mut self, c: Vec3, half: Vec3) {
        self.box_min_max(c - half, c + half);
    }

    pub fn box_min_max(&mut self, min: Vec3, max: Vec3) {
        self.box_faces(min, max, 0b111111);
    }

    /// Box with a per-face mask: bit0 +X, bit1 -X, bit2 +Y, bit3 -Y, bit4 +Z, bit5 -Z.
    pub fn box_faces(&mut self, min: Vec3, max: Vec3, faces: u8) {
        let (lo, hi) = (self.ao_lo, self.ao_hi);
        let corner = |x: bool, y: bool, z: bool| Vec3::new(if x { max.x } else { min.x }, if y { max.y } else { min.y }, if z { max.z } else { min.z });
        let ao_y = |y: bool| if y { hi } else { lo };
        // (normal, 4 corners)
        let defs: [(Vec3, [(bool, bool, bool); 4]); 6] = [
            (Vec3::X, [(true, false, false), (true, true, false), (true, true, true), (true, false, true)]),
            (Vec3::NEG_X, [(false, false, true), (false, true, true), (false, true, false), (false, false, false)]),
            (Vec3::Y, [(false, true, false), (false, true, true), (true, true, true), (true, true, false)]),
            (Vec3::NEG_Y, [(false, false, true), (false, false, false), (true, false, false), (true, false, true)]),
            (Vec3::Z, [(true, false, true), (true, true, true), (false, true, true), (false, false, true)]),
            (Vec3::NEG_Z, [(false, false, false), (false, true, false), (true, true, false), (true, false, false)]),
        ];
        for (fi, (n, cs)) in defs.iter().enumerate() {
            if faces & (1 << fi) == 0 {
                continue;
            }
            let ids: Vec<u32> = cs
                .iter()
                .map(|&(x, y, z)| {
                    let ao = if fi == 3 { lo * 0.8 } else { ao_y(y) };
                    self.vert(corner(x, y, z), *n, ao)
                })
                .collect();
            self.quad(ids[0], ids[1], ids[2], ids[3]);
        }
    }

    /// Vertical cylinder / truncated cone along +Y starting at `base`.
    /// `r0` is the bottom radius, `r1` the top radius.
    pub fn cylinder(&mut self, base: Vec3, r0: f32, r1: f32, h: f32, seg: u32, bottom_cap: bool, top_cap: bool) {
        let seg = seg.max(3);
        let slope = (r0 - r1) / h.max(1e-4);
        let (lo, hi) = (self.ao_lo, self.ao_hi);
        let mut ring_b = Vec::new();
        let mut ring_t = Vec::new();
        for i in 0..seg {
            let a = i as f32 / seg as f32 * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            let n = Vec3::new(c, slope, s).normalize();
            ring_b.push(self.vert(base + Vec3::new(c * r0, 0.0, s * r0), n, lo));
            ring_t.push(self.vert(base + Vec3::new(c * r1, h, s * r1), n, hi));
        }
        for i in 0..seg as usize {
            let j = (i + 1) % seg as usize;
            if r1 > 1e-5 {
                self.quad(ring_b[i], ring_b[j], ring_t[j], ring_t[i]);
            } else {
                self.tri(ring_b[i], ring_b[j], ring_t[i]);
            }
        }
        if bottom_cap && r0 > 1e-5 {
            let c = self.vert(base, Vec3::NEG_Y, lo * 0.8);
            let ring: Vec<u32> = (0..seg)
                .map(|i| {
                    let a = i as f32 / seg as f32 * std::f32::consts::TAU;
                    self.vert(base + Vec3::new(a.cos() * r0, 0.0, a.sin() * r0), Vec3::NEG_Y, lo * 0.8)
                })
                .collect();
            for i in 0..seg as usize {
                self.tri(c, ring[i], ring[(i + 1) % seg as usize]);
            }
        }
        if top_cap && r1 > 1e-5 {
            let c = self.vert(base + Vec3::Y * h, Vec3::Y, hi);
            let ring: Vec<u32> = (0..seg)
                .map(|i| {
                    let a = i as f32 / seg as f32 * std::f32::consts::TAU;
                    self.vert(base + Vec3::new(a.cos() * r1, h, a.sin() * r1), Vec3::Y, hi)
                })
                .collect();
            for i in 0..seg as usize {
                self.tri(c, ring[i], ring[(i + 1) % seg as usize]);
            }
        }
    }

    /// Cone pointing up with a base cap.
    pub fn cone(&mut self, base: Vec3, r: f32, h: f32, seg: u32) {
        self.cylinder(base, r, 0.0, h, seg, true, false);
    }

    /// Icosphere with smooth normals.
    pub fn sphere(&mut self, c: Vec3, r: f32, subdiv: u32) {
        self.blob(c, Vec3::splat(r), subdiv, 0.0, 0, true);
    }

    /// Ellipsoid with optional noise displacement. `smooth` chooses between
    /// smooth normals (foliage) and faceted ones (rocks).
    pub fn blob(&mut self, c: Vec3, radii: Vec3, subdiv: u32, noise_amp: f32, seed: u32, smooth: bool) {
        let (dirs, tris) = icosphere(subdiv);
        let pos: Vec<Vec3> = dirs
            .iter()
            .map(|d| {
                let k = if noise_amp > 0.0 { 1.0 + noise_amp * noise3(*d * 1.7 + Vec3::splat(seed as f32 * 0.37), seed) } else { 1.0 };
                c + *d * radii * k
            })
            .collect();
        let (lo, hi) = (self.ao_lo, self.ao_hi);
        let ao_of = |n: Vec3| lo + (hi - lo) * (n.y * 0.5 + 0.5);
        if smooth {
            // Smooth normals from the displaced geometry.
            let mut nrm = vec![Vec3::ZERO; pos.len()];
            for t in &tris {
                let n = (pos[t[1] as usize] - pos[t[0] as usize]).cross(pos[t[2] as usize] - pos[t[0] as usize]);
                for &i in t {
                    nrm[i as usize] += n;
                }
            }
            let ids: Vec<u32> = (0..pos.len())
                .map(|i| {
                    let n = nrm[i].normalize_or_zero();
                    let n = if n == Vec3::ZERO { dirs[i] } else { n };
                    // Local space positions: undo the builder's own transform by
                    // letting `vert` apply it.
                    self.vert(pos[i], n, ao_of(dirs[i]))
                })
                .collect();
            for t in &tris {
                self.tri(ids[t[0] as usize], ids[t[1] as usize], ids[t[2] as usize]);
            }
        } else {
            for t in &tris {
                let (a, b, cc) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
                let n = (b - a).cross(cc - a).normalize_or_zero();
                let mid = (dirs[t[0] as usize] + dirs[t[1] as usize] + dirs[t[2] as usize]).normalize_or_zero();
                let n = if n.dot(mid) < 0.0 { -n } else { n };
                let ao = ao_of(mid);
                let (ia, ib, ic) = (self.vert(a, n, ao), self.vert(b, n, ao), self.vert(cc, n, ao));
                self.tri(ia, ib, ic);
            }
        }
    }

    /// A ramp / wedge occupying `min..max`. The slope rises toward `dir`:
    /// 0 = +X, 1 = +Z, 2 = -X, 3 = -Z. The high edge reaches `max.y`.
    pub fn wedge(&mut self, min: Vec3, max: Vec3, dir: u8) {
        let (lo, hi) = (self.ao_lo, self.ao_hi);
        // Build in a canonical "rises toward +X" frame then rotate corners.
        let c = (min + max) * 0.5;
        let h = (max - min) * 0.5;
        let rot = |p: Vec3| -> Vec3 {
            // p is in canonical frame relative to centre with half sizes (hx,hy,hz)
            // map canonical +X to the requested direction.
            match dir & 3 {
                0 => p,
                1 => Vec3::new(-p.z, p.y, p.x),  // +X -> +Z
                2 => Vec3::new(-p.x, p.y, -p.z), // +X -> -X
                _ => Vec3::new(p.z, p.y, -p.x),  // +X -> -Z
            }
        };
        // Canonical half extents along the rise axis / lateral axis.
        let (hx, hz) = if dir & 1 == 0 { (h.x, h.z) } else { (h.z, h.x) };
        let hy = h.y;
        let p = |x: f32, y: f32, z: f32| c + rot(Vec3::new(x * hx, y * hy, z * hz));
        let nr = |n: Vec3| rot(n);
        let slope_n = nr(Vec3::new(-hy, hx, 0.0).normalize());
        // bottom (-Y)
        {
            let n = Vec3::NEG_Y;
            let i: Vec<u32> = [(-1., -1., -1.), (1., -1., -1.), (1., -1., 1.), (-1., -1., 1.)]
                .iter()
                .map(|&(x, y, z)| self.vert(p(x, y, z), n, lo * 0.8))
                .collect();
            self.quad(i[0], i[1], i[2], i[3]);
        }
        // back wall (+X face, full height)
        {
            let n = nr(Vec3::X);
            let i: Vec<u32> = [(1., -1., -1.), (1., 1., -1.), (1., 1., 1.), (1., -1., 1.)]
                .iter()
                .map(|&(x, y, z)| self.vert(p(x, y, z), n, if y > 0.0 { hi } else { lo }))
                .collect();
            self.quad(i[0], i[1], i[2], i[3]);
        }
        // sloped top
        {
            let i: Vec<u32> = [(-1., -1., -1.), (1., 1., -1.), (1., 1., 1.), (-1., -1., 1.)]
                .iter()
                .map(|&(x, y, z)| self.vert(p(x, y, z), slope_n, if y > 0.0 { hi } else { lo }))
                .collect();
            self.quad(i[0], i[1], i[2], i[3]);
        }
        // two triangular sides (+Z / -Z in canonical frame)
        for s in [-1.0f32, 1.0] {
            let n = nr(Vec3::new(0.0, 0.0, s));
            let a = self.vert(p(-1.0, -1.0, s), n, lo);
            let b = self.vert(p(1.0, -1.0, s), n, lo);
            let cc = self.vert(p(1.0, 1.0, s), n, hi);
            self.tri(a, b, cc);
        }
    }

    /// Triangular prism (gable roof) over `min..max` whose ridge runs along
    /// the X axis (`ridge_x = true`) or Z axis, at height `max.y`, eaves at
    /// `min.y`, with an overhang already included in min/max.
    pub fn gable(&mut self, min: Vec3, max: Vec3, ridge_x: bool) {
        let (lo, hi) = (self.ao_lo, self.ao_hi);
        let c = (min + max) * 0.5;
        let h = (max - min) * 0.5;
        // canonical: ridge along X; lateral axis Z.
        let (hx, hz) = if ridge_x { (h.x, h.z) } else { (h.z, h.x) };
        let hy = h.y;
        let rot = |p: Vec3| if ridge_x { p } else { Vec3::new(p.z, p.y, p.x) };
        let p = |x: f32, y: f32, z: f32| c + rot(Vec3::new(x * hx, y * hy, z * hz));
        let nr = |n: Vec3| rot(n);
        // two sloped planes
        for s in [-1.0f32, 1.0] {
            let n = nr(Vec3::new(0.0, hz, s * hy).normalize());
            let pts = [(-1.0, -1.0, s), (1.0, -1.0, s), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0)];
            let i: Vec<u32> = pts.iter().map(|&(x, y, z)| self.vert(p(x, y, z), n, if y > 0.0 { hi } else { lo })).collect();
            self.quad(i[0], i[1], i[2], i[3]);
        }
        // underside
        {
            let n = Vec3::NEG_Y;
            let i: Vec<u32> = [(-1., -1., -1.), (-1., -1., 1.), (1., -1., 1.), (1., -1., -1.)]
                .iter()
                .map(|&(x, y, z)| self.vert(p(x, y, z), n, lo * 0.7))
                .collect();
            self.quad(i[0], i[1], i[2], i[3]);
        }
        // gable end triangles
        for s in [-1.0f32, 1.0] {
            let n = nr(Vec3::new(s, 0.0, 0.0));
            let a = self.vert(p(s, -1.0, -1.0), n, lo);
            let b = self.vert(p(s, -1.0, 1.0), n, lo);
            let cc = self.vert(p(s, 1.0, 0.0), n, hi);
            self.tri(a, b, cc);
        }
    }

    /// Camera-less flat grid helper used by terrain: not transformed by state.
    pub fn raw_triangle_list(&mut self, verts: &[Vertex], idx: &[u32]) {
        let base = self.mesh.verts.len() as u32;
        self.mesh.verts.extend_from_slice(verts);
        self.mesh.idx.extend(idx.iter().map(|i| i + base));
    }
}

/// Unit icosphere: returns (unit directions, triangle indices).
pub fn icosphere(subdiv: u32) -> (Vec<Vec3>, Vec<[u32; 3]>) {
    let t = (1.0 + 5.0f32.sqrt()) / 2.0;
    let mut v: Vec<Vec3> = vec![
        Vec3::new(-1.0, t, 0.0),
        Vec3::new(1.0, t, 0.0),
        Vec3::new(-1.0, -t, 0.0),
        Vec3::new(1.0, -t, 0.0),
        Vec3::new(0.0, -1.0, t),
        Vec3::new(0.0, 1.0, t),
        Vec3::new(0.0, -1.0, -t),
        Vec3::new(0.0, 1.0, -t),
        Vec3::new(t, 0.0, -1.0),
        Vec3::new(t, 0.0, 1.0),
        Vec3::new(-t, 0.0, -1.0),
        Vec3::new(-t, 0.0, 1.0),
    ]
    .into_iter()
    .map(|p| p.normalize())
    .collect();
    let mut f: Vec<[u32; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..subdiv {
        let mut cache: HashMap<(u32, u32), u32> = HashMap::new();
        let mut nf = Vec::with_capacity(f.len() * 4);
        let mut mid = |a: u32, b: u32, v: &mut Vec<Vec3>| -> u32 {
            let key = if a < b { (a, b) } else { (b, a) };
            *cache.entry(key).or_insert_with(|| {
                v.push(((v[a as usize] + v[b as usize]) * 0.5).normalize());
                (v.len() - 1) as u32
            })
        };
        for t in &f {
            let a = mid(t[0], t[1], &mut v);
            let b = mid(t[1], t[2], &mut v);
            let c = mid(t[2], t[0], &mut v);
            nf.push([t[0], a, c]);
            nf.push([t[1], b, a]);
            nf.push([t[2], c, b]);
            nf.push([a, b, c]);
        }
        f = nf;
    }
    (v, f)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triangle's geometric normal must agree with its vertex normals.
    fn assert_outward(m: &MeshData, what: &str) {
        assert!(!m.is_empty(), "{what}: empty mesh");
        for t in m.idx.chunks(3) {
            let (a, b, c) = (&m.verts[t[0] as usize], &m.verts[t[1] as usize], &m.verts[t[2] as usize]);
            let (pa, pb, pc) = (Vec3::from(a.pos), Vec3::from(b.pos), Vec3::from(c.pos));
            let n = (pb - pa).cross(pc - pa);
            if n.length_squared() < 1e-12 {
                continue; // degenerate
            }
            let vn = Vec3::from(a.nrm) + Vec3::from(b.nrm) + Vec3::from(c.nrm);
            assert!(n.dot(vn) > 0.0, "{what}: triangle winding disagrees with normals");
        }
        for v in &m.verts {
            let l = Vec3::from(v.nrm).length();
            assert!((l - 1.0).abs() < 1e-3, "{what}: non-unit normal {l}");
        }
    }

    #[test]
    fn box_is_closed_and_outward() {
        let mut b = MeshBuilder::new();
        b.box_min_max(Vec3::splat(-1.0), Vec3::splat(1.0));
        let m = b.finish();
        assert_eq!(m.verts.len(), 24);
        assert_eq!(m.tri_count(), 12);
        assert_outward(&m, "box");
        // Each triangle must face away from the centre.
        for t in m.idx.chunks(3) {
            let (pa, pb, pc) = (Vec3::from(m.verts[t[0] as usize].pos), Vec3::from(m.verts[t[1] as usize].pos), Vec3::from(m.verts[t[2] as usize].pos));
            let n = (pb - pa).cross(pc - pa);
            let centroid = (pa + pb + pc) / 3.0;
            assert!(n.dot(centroid) > 0.0);
        }
    }

    #[test]
    fn primitives_are_well_formed() {
        let mut b = MeshBuilder::new();
        b.cylinder(Vec3::ZERO, 1.0, 0.5, 2.0, 8, true, true);
        assert_outward(&b.finish(), "cylinder");
        let mut b = MeshBuilder::new();
        b.cone(Vec3::ZERO, 1.0, 2.0, 8);
        assert_outward(&b.finish(), "cone");
        let mut b = MeshBuilder::new();
        b.sphere(Vec3::ZERO, 1.0, 2);
        let m = b.finish();
        assert_outward(&m, "sphere");
        assert_eq!(m.tri_count(), 320);
        let mut b = MeshBuilder::new();
        b.blob(Vec3::ZERO, Vec3::new(1.0, 0.7, 1.2), 1, 0.3, 4, false);
        assert_outward(&b.finish(), "faceted blob");
        let mut b = MeshBuilder::new();
        b.blob(Vec3::ZERO, Vec3::new(1.0, 0.7, 1.2), 1, 0.3, 4, true);
        assert_outward(&b.finish(), "smooth blob");
        for d in 0..4u8 {
            let mut b = MeshBuilder::new();
            b.wedge(Vec3::new(-2.0, 0.0, -1.0), Vec3::new(2.0, 3.0, 1.0), d);
            assert_outward(&b.finish(), "wedge");
        }
        for rx in [true, false] {
            let mut b = MeshBuilder::new();
            b.gable(Vec3::new(-3.0, 0.0, -2.0), Vec3::new(3.0, 1.5, 2.0), rx);
            assert_outward(&b.finish(), "gable");
        }
    }

    #[test]
    fn wedge_geometry_rises_toward_dir() {
        for (d, axis_pos) in [(0u8, Vec3::X), (1, Vec3::Z), (2, Vec3::NEG_X), (3, Vec3::NEG_Z)] {
            let mut b = MeshBuilder::new();
            b.wedge(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 3.0, 2.0), d);
            let m = b.finish();
            // Top-most vertices must lie on the +dir side.
            for v in &m.verts {
                let p = Vec3::from(v.pos);
                if p.y > 2.9 {
                    assert!(p.dot(axis_pos) > 1.9, "dir {d}: high vertex not on high side {p:?}");
                }
                if p.dot(axis_pos) > 1.9 && v.nrm[1].abs() < 0.01 && Vec3::from(v.nrm).dot(axis_pos) > 0.9 {
                    // back wall vertex: fine either height
                }
            }
        }
    }

    #[test]
    fn transform_stack_applies_to_positions_and_normals() {
        let mut b = MeshBuilder::new();
        b.push_xf(Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)) * Mat4::from_scale(Vec3::new(1.0, 4.0, 1.0)));
        b.box_min_max(Vec3::splat(-1.0), Vec3::splat(1.0));
        b.pop_xf();
        let m = b.finish();
        let bb = m.bounds();
        assert!((bb.min - Vec3::new(9.0, -4.0, -1.0)).length() < 1e-4);
        assert!((bb.max - Vec3::new(11.0, 4.0, 1.0)).length() < 1e-4);
        assert_outward(&m, "scaled box");
    }

    #[test]
    fn append_transforms_mesh() {
        let mut b = MeshBuilder::new();
        b.box_min_max(Vec3::ZERO, Vec3::ONE);
        let src = b.finish();
        let mut dst = MeshData::default();
        dst.append(&src, Mat4::from_translation(Vec3::new(5.0, 0.0, 0.0)));
        dst.append(&src, Mat4::from_translation(Vec3::new(-5.0, 0.0, 0.0)));
        assert_eq!(dst.verts.len(), 48);
        assert_eq!(dst.idx.len(), 72);
        assert_outward(&dst, "appended");
    }

    #[test]
    fn instance_matrix_rows() {
        let i = Instance::at(Vec3::new(1.0, 2.0, 3.0), 0.0, 2.0, [1.0; 4]);
        assert_eq!(i.m0, [2.0, 0.0, 0.0, 1.0]);
        assert_eq!(i.m1, [0.0, 2.0, 0.0, 2.0]);
        assert_eq!(i.m2, [0.0, 0.0, 2.0, 3.0]);
        assert_eq!(std::mem::size_of::<Instance>(), 80);
        assert_eq!(std::mem::size_of::<Vertex>(), 32);
    }
}
