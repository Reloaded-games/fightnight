//! Procedural buildings. Each generator returns a [`Geom`] in a local frame
//! (origin at the ground centre of the footprint, door on the +Z side), which is
//! then rotated by quarter turns and dropped into the world with [`place`].

use super::collision::*;
use crate::math::*;
use crate::mesh::*;
use crate::rng::{hash3, Rng};
use std::f32::consts::{FRAC_PI_2, PI};

pub const FLOOR_Y: f32 = 0.28;
pub const STORY_H: f32 = 3.0;
pub const WALL_T: f32 = 0.28;
const DOOR_W: f32 = 1.5;
const DOOR_H: f32 = 2.35;

/// A raised entrance (stoop, porch, plinth): where its outer edge is, in the building's local frame.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    /// Centre of the landing's outer edge, at the landing's top height.
    pub edge: Vec3,
    /// Half the width of the steps that lead up to it.
    pub half_w: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Geom {
    pub mesh: MeshData,
    pub cols: Vec<Collider>,
    /// Interior floor positions where loot can lie.
    pub loot: Vec<Vec3>,
    /// (position, yaw) of chests. Yaw is the direction the lid faces (0 = +Z).
    pub chests: Vec<(Vec3, f32)>,
    pub door_out: Vec3,
    pub door_in: Vec3,
    /// Half extents of the footprint (x, z), excluding porches / overhangs.
    pub half: Vec2,
    pub height: f32,
    /// Local position of a spinning windmill hub, if any.
    pub hub: Option<Vec3>,
    /// The raised entrance in front of the door, if the building has one.
    pub entry: Option<Entry>,
}

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub wall: u32,
    pub wall_mat: u8,
    pub trim: u32,
    pub roof: u32,
    pub shutters: Option<u32>,
    pub porch: bool,
    pub chimney: bool,
    pub ridge_x: bool,
    pub floors: u32,
    pub logs: bool,
}

pub const WALLS: [u32; 9] = [0xF2E7C8, 0xF6DC8E, 0xA9D8EE, 0xB9E3C6, 0xF0AE96, 0xF4F3EE, 0xCFC4E8, 0xE8D2A6, 0x9FB6CF];
pub const ROOFS: [u32; 7] = [0xC24A3C, 0x2F8F8A, 0xD9772F, 0x3F5F94, 0x56596A, 0x4F8B4A, 0x8A4B80];
pub const SHUTTER: [u32; 5] = [0x3A7D8C, 0xB5463A, 0x3E6E42, 0x2F4F7F, 0xD9A23B];

pub fn random_style(rng: &mut Rng, floors: u32) -> Style {
    let brick = rng.chance(0.18);
    Style {
        wall: if brick { 0xB5523B } else { *rng.pick(&WALLS) },
        wall_mat: if brick { mat::BRICK } else { mat::PLASTER },
        trim: 0xF7F5EE,
        roof: *rng.pick(&ROOFS),
        shutters: if rng.chance(0.65) { Some(*rng.pick(&SHUTTER)) } else { None },
        porch: rng.chance(0.4),
        chimney: rng.chance(0.55),
        ridge_x: rng.chance(0.6),
        floors,
        logs: false,
    }
}

// ---------------------------------------------------------------------------------
// Builder plumbing
// ---------------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Op {
    u0: f32,
    u1: f32,
    v0: f32,
    v1: f32,
}

/// Rectangles (u0, u1, v0, v1) that tile a wall of length `len` and height `h` minus its openings.
fn wall_rects(len: f32, h: f32, ops: &[Op]) -> Vec<(f32, f32, f32, f32)> {
    let mut sorted: Vec<Op> = ops.to_vec();
    sorted.sort_by(|a, b| a.u0.partial_cmp(&b.u0).unwrap());
    let mut out = vec![];
    let mut cur = 0.0;
    for o in sorted {
        if o.u0 > cur + 1e-4 {
            out.push((cur, o.u0, 0.0, h));
        }
        if o.v0 > 1e-4 {
            out.push((o.u0, o.u1, 0.0, o.v0));
        }
        if o.v1 < h - 1e-4 {
            out.push((o.u0, o.u1, o.v1, h));
        }
        cur = o.u1;
    }
    if cur < len - 1e-4 {
        out.push((cur, len, 0.0, h));
    }
    out
}

/// A straight wall in plan view.
#[derive(Clone, Copy)]
struct Line {
    along_x: bool,
    a0: f32,
    a1: f32,
    /// Across-axis coordinate of the wall centre line.
    c: f32,
    /// Outward direction along the across axis (+1 / -1).
    out: f32,
}

impl Line {
    fn len(&self) -> f32 {
        self.a1 - self.a0
    }
    /// A box spanning `u0..u1` along the wall, `d0..d1` across (relative to the centre line) and `y0..y1`.
    fn bx(&self, u0: f32, u1: f32, d0: f32, d1: f32, y0: f32, y1: f32) -> (Vec3, Vec3) {
        if self.along_x {
            (Vec3::new(self.a0 + u0, y0, self.c + d0), Vec3::new(self.a0 + u1, y1, self.c + d1))
        } else {
            (Vec3::new(self.c + d0, y0, self.a0 + u0), Vec3::new(self.c + d1, y1, self.a0 + u1))
        }
    }
}

/// A stateless random number in [0, 1) from three integers.
fn hunit(a: i32, b: i32, c: i32) -> f32 {
    (hash3(a, b, c, 0xD00D) >> 8) as f32 / 16_777_216.0
}

/// A colour scaled by `k` (a darker or lighter shade of the same hue).
fn shade_hex(col: u32, k: f32) -> u32 {
    let c = |sh: u32| (((col >> sh) & 255) as f32 * k).clamp(0.0, 255.0) as u32;
    (c(16) << 16) | (c(8) << 8) | c(0)
}

struct B {
    mb: MeshBuilder,
    cols: Vec<Collider>,
    loot: Vec<Vec3>,
    chests: Vec<(Vec3, f32)>,
}

impl B {
    fn new() -> Self {
        Self { mb: MeshBuilder::new(), cols: vec![], loot: vec![], chests: vec![] }
    }

    fn bx(&mut self, min: Vec3, max: Vec3, col: u32, m: u8, collide: bool) {
        self.mb.mat(m).hex(col);
        self.mb.box_min_max(min, max);
        if collide {
            self.cols.push(Collider::aabb_box(min, max, Tag::Static));
        }
    }

    fn bx_faces(&mut self, min: Vec3, max: Vec3, col: u32, m: u8, faces: u8, collide: bool) {
        self.mb.mat(m).hex(col);
        self.mb.box_faces(min, max, faces);
        if collide {
            self.cols.push(Collider::aabb_box(min, max, Tag::Static));
        }
    }

    fn cyl(&mut self, c: Vec3, r: f32, h: f32, col: u32, m: u8, collide: bool) {
        self.mb.mat(m).hex(col);
        self.mb.cylinder(c, r, r, h, 12, true, true);
        if collide {
            self.cols.push(Collider::cyl(c.x, c.z, r, c.y, c.y + h, Tag::Static));
        }
    }

    /// Emit a wall with openings (mesh + colliders).
    fn wall(&mut self, l: Line, y0: f32, h: f32, ops: &[Op], col: u32, m: u8) {
        self.mb.ao(0.82, 1.0);
        for (u0, u1, v0, v1) in wall_rects(l.len(), h, ops) {
            let (a, b) = l.bx(u0, u1, -WALL_T / 2.0, WALL_T / 2.0, y0 + v0, y0 + v1);
            self.bx(a, b, col, m, true);
        }
        self.mb.ao(1.0, 1.0);
    }

    /// Frame trim around openings (door: jambs + header; windows: + sill and optional shutters).
    fn opening_trim(&mut self, l: Line, y0: f32, o: &Op, window: bool, trim: u32, shutter: Option<u32>) {
        let t = WALL_T / 2.0 + 0.05;
        let (a, b) = l.bx(o.u0 - 0.1, o.u0, -t, t, y0 + o.v0, y0 + o.v1 + 0.12);
        self.bx(a, b, trim, mat::FLAT, false);
        let (a, b) = l.bx(o.u1, o.u1 + 0.1, -t, t, y0 + o.v0, y0 + o.v1 + 0.12);
        self.bx(a, b, trim, mat::FLAT, false);
        let (a, b) = l.bx(o.u0 - 0.1, o.u1 + 0.1, -t, t, y0 + o.v1, y0 + o.v1 + 0.12);
        self.bx(a, b, trim, mat::FLAT, false);
        if window {
            let (a, b) = l.bx(o.u0 - 0.14, o.u1 + 0.14, -t, t + 0.1, y0 + o.v0 - 0.09, y0 + o.v0);
            self.bx(a, b, trim, mat::FLAT, false);
            // window cross bars (open window with mullions) – thin so you can still shoot through
            let (a, b) = l.bx((o.u0 + o.u1) * 0.5 - 0.025, (o.u0 + o.u1) * 0.5 + 0.025, -0.03, 0.03, y0 + o.v0, y0 + o.v1);
            self.bx(a, b, trim, mat::FLAT, false);
            // a cross bar at two thirds of the height makes six panes out of one, and a lintel with a small overhang
            let ym = y0 + o.v0 + (o.v1 - o.v0) * 0.62;
            let (a, b) = l.bx(o.u0, o.u1, -0.03, 0.03, ym - 0.02, ym + 0.02);
            self.bx(a, b, trim, mat::FLAT, false);
            let (a, b) = l.bx(o.u0 - 0.18, o.u1 + 0.18, -t - 0.05 * l.out.abs(), t + 0.05, y0 + o.v1 + 0.12, y0 + o.v1 + 0.19);
            self.bx(a, b, trim, mat::FLAT, false);
            if let Some(sc) = shutter {
                for (s0, s1) in [(o.u0 - 0.5, o.u0 - 0.12), (o.u1 + 0.12, o.u1 + 0.5)] {
                    let o_d = l.out * (WALL_T / 2.0 + 0.05);
                    let (d0, d1) = if l.out > 0.0 { (o_d, o_d + 0.06) } else { (o_d - 0.06, o_d) };
                    let (a, b) = l.bx(s0, s1, d0, d1, y0 + o.v0 - 0.05, y0 + o.v1 + 0.08);
                    self.bx(a, b, sc, mat::WOOD, false);
                }
            }
        }
    }

    /// A round rod from `p0` to `p1` (gutters, downspouts, poles).
    fn tube(&mut self, p0: Vec3, p1: Vec3, r: f32, col: u32, m: u8) {
        let d = p1 - p0;
        let len = d.length();
        if len < 1e-4 {
            return;
        }
        self.mb.mat(m).hex(col);
        self.mb.push_xf(Mat4::from_translation(p0) * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, d / len)));
        self.mb.cylinder(Vec3::ZERO, r, r, len, 6, false, true);
        self.mb.pop_xf();
    }

    /// A flat board of width `w` and thickness `t` from `p0` to `p1`, its faces looking along `normal`.
    fn board(&mut self, p0: Vec3, p1: Vec3, normal: Vec3, w: f32, t: f32, col: u32, m: u8) {
        let d = p1 - p0;
        let len = d.length();
        if len < 1e-4 {
            return;
        }
        let x = d / len;
        let n = (normal - x * normal.dot(x)).normalize_or_zero();
        if n == Vec3::ZERO {
            return;
        }
        let y = n.cross(x);
        self.mb.mat(m).hex(col);
        self.mb.push_xf(Mat4::from_cols(x.extend(0.0), y.extend(0.0), n.extend(0.0), p0.extend(1.0)));
        self.mb.box_min_max(Vec3::new(0.0, -w / 2.0, -t / 2.0), Vec3::new(len, w / 2.0, t / 2.0));
        self.mb.pop_xf();
    }

    /// A door standing ajar inside the opening `o`, hinged at its `u0` jamb: two raised panels on each face, a knob and a
    /// lock plate. Mesh only (the opening stays clear for walking and shooting).
    fn door_leaf(&mut self, l: Line, y0: f32, o: &Op, col: u32, knob: u32) {
        let to_world = |u: f32, d: f32| if l.along_x { Vec3::new(l.a0 + u, 0.0, l.c + d) } else { Vec3::new(l.c + d, 0.0, l.a0 + u) };
        let open = 1.2f32;
        let hinge = to_world(o.u0 + 0.05, -l.out * (WALL_T / 2.0 - 0.03)) + Vec3::Y * (y0 + o.v0 + 0.02);
        let dir = to_world(open.cos(), -l.out * open.sin()) - to_world(0.0, 0.0);
        let yaw = (-dir.z).atan2(dir.x);
        let (w, h) = (o.u1 - o.u0 - 0.1, o.v1 - o.v0 - 0.05);
        let dark = shade_hex(col, 0.82);
        self.mb.push_xf(Mat4::from_translation(hinge) * Mat4::from_rotation_y(yaw));
        self.mb.mat(mat::WOOD).hex(col);
        self.mb.box_min_max(Vec3::new(0.0, 0.0, -0.025), Vec3::new(w, h, 0.025));
        self.mb.hex(dark);
        for s in [-1.0f32, 1.0] {
            let z = s * 0.0265;
            for (y0p, y1p) in [(0.12, h * 0.46), (h * 0.5, h - 0.14)] {
                let (a, b) = (Vec3::new(0.14, y0p, z.min(z * 0.2)), Vec3::new(w - 0.14, y1p, z.max(z * 0.2)));
                self.mb.box_min_max(a, b);
            }
        }
        // knob on a plate and a dark keyhole
        self.mb.mat(mat::METAL).hex(knob).spec(0.9);
        for s in [-1.0f32, 1.0] {
            self.mb.sphere(Vec3::new(w - 0.14, h * 0.47, s * 0.062), 0.034, 0);
        }
        self.mb.hex(0x2b2e34);
        self.mb.box_center(Vec3::new(w - 0.14, h * 0.47 + 0.09, 0.0), Vec3::new(0.012, 0.03, 0.03));
        self.mb.pop_xf();
    }

    /// Dressing for a window opening `o`: a curtain rod with two tied-back curtains inside and, for some windows,
    /// a flower box below the sill. Everything is chosen by hashing `key`, so houses stay the same shape.
    fn window_dressing(&mut self, l: Line, y0: f32, o: &Op, key: i32, ground: bool) {
        let side = |a: f32, b: f32| if a < b { (a, b) } else { (b, a) };
        let h = hunit(key, 1, 7);
        // curtains inside: pastel cloth against the inner face
        let (d0, d1) = side(-l.out * (WALL_T / 2.0 + 0.0), -l.out * (WALL_T / 2.0 + 0.06));
        let cols = [0xf4e3b8u32, 0xe9b8c4, 0xb8d6e9, 0xc9e3b8, 0xf2f0e8];
        let cc = cols[(h * 5.0) as usize % cols.len()];
        for (u0, u1) in [(o.u0 + 0.0, o.u0 + 0.2), (o.u1 - 0.2, o.u1 - 0.0)] {
            let (a, b) = l.bx(u0, u1, d0, d1, y0 + o.v0 + 0.32, y0 + o.v1 + 0.02);
            self.bx(a, b, cc, mat::CLOTH, false);
        }
        let (a, b) = l.bx(o.u0 - 0.04, o.u1 + 0.04, d0, d1 - l.out * 0.02, y0 + o.v1 + 0.03, y0 + o.v1 + 0.06);
        self.bx(a, b, 0x6a4a2e, mat::WOOD, false);
        // flower box
        if ground && hunit(key, 2, 11) < 0.55 {
            let (e0, e1) = side(l.out * (WALL_T / 2.0 + 0.04), l.out * (WALL_T / 2.0 + 0.24));
            let (ylo, yhi) = (y0 + o.v0 - 0.3, y0 + o.v0 - 0.1);
            let (a, b) = l.bx(o.u0 - 0.06, o.u1 + 0.06, e0, e1, ylo, yhi);
            self.bx(a, b, 0x8a5a34, mat::WOOD, false);
            let (a, b) = l.bx(o.u0 - 0.04, o.u1 + 0.04, e0 + 0.02, e1 - 0.02, yhi, yhi + 0.03);
            self.bx(a, b, 0x4a3524, mat::FLAT, false);
            let bloom = [0xff6f91u32, 0xffd23f, 0xfdfbf0, 0xff8a3d, 0xc27bff][(hunit(key, 3, 13) * 5.0) as usize % 5];
            let n = 5;
            let dm = l.out * (WALL_T / 2.0 + 0.14);
            for k in 0..n {
                let u = o.u0 + 0.1 + (o.u1 - o.u0 - 0.2) * k as f32 / (n - 1) as f32;
                let pos = if l.along_x { Vec3::new(l.a0 + u, yhi + 0.07, l.c + dm) } else { Vec3::new(l.c + dm, yhi + 0.07, l.a0 + u) };
                self.mb.mat(mat::FOLIAGE).tinted(false).hex(0x4c9a3a);
                self.mb.sphere(pos - Vec3::Y * 0.02, 0.075, 0);
                self.mb.mat(mat::FLAT).hex(if k % 2 == 0 { bloom } else { 0xfdfbf0 }).tinted(false);
                self.mb.sphere(pos + Vec3::Y * 0.07, 0.05, 0);
            }
            self.mb.tinted(true);
        }
    }

    fn corner_posts(&mut self, hx: f32, hz: f32, y0: f32, y1: f32, col: u32) {
        for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let c = Vec3::new(sx * hx, 0.0, sz * hz);
            self.bx(Vec3::new(c.x - 0.17, y0, c.z - 0.17), Vec3::new(c.x + 0.17, y1, c.z + 0.17), col, mat::FLAT, false);
        }
    }

    /// Pick window centres along a wall of `len`, avoiding `avoid` intervals.
    fn window_slots(len: f32, avoid: &[(f32, f32)], spacing: f32) -> Vec<f32> {
        let n = ((len / spacing).floor() as i32).max(1);
        let mut out = vec![];
        for k in 0..n {
            let c = len * (k as f32 + 0.5) / n as f32;
            let ok = c > 1.1 && c < len - 1.1 && avoid.iter().all(|&(a, b)| c + 0.9 < a || c - 0.9 > b);
            if ok {
                out.push(c);
            }
        }
        out
    }

    fn finish(self, half: Vec2, height: f32, door_out: Vec3, door_in: Vec3) -> Geom {
        Geom { mesh: self.mb.finish(), cols: self.cols, loot: self.loot, chests: self.chests, door_out, door_in, half, height, hub: None, entry: None }
    }
}

// ---------------------------------------------------------------------------------
// Roof
// ---------------------------------------------------------------------------------

/// Gable roof with overhang, matching colliders, gable-end infill and fascia.
fn gable_roof(b: &mut B, hx: f32, hz: f32, top_y: f32, rise: f32, ridge_x: bool, roof: u32, wall: u32, trim: u32, wall_mat: u8) {
    let ov = 0.6;
    let (ex, ez) = (hx + ov, hz + ov);
    // canonical frame: ridge along X; map (x,z) accordingly
    let map = |x: f32, y: f32, z: f32| if ridge_x { Vec3::new(x, y, z) } else { Vec3::new(z, y, x) };
    let (len_half, wid_half) = if ridge_x { (hx, hz) } else { (hz, hx) };
    let (elen, ewid) = if ridge_x { (ex, ez) } else { (ez, ex) };
    let slope_n = |s: f32| {
        let n = Vec3::new(0.0, ewid, s * rise).normalize();
        if ridge_x { n } else { Vec3::new(n.z, n.y, n.x) }
    };
    b.mb.mat(mat::SHINGLE).hex(roof);
    for s in [-1.0f32, 1.0] {
        let p = [map(-elen, top_y, s * ewid), map(elen, top_y, s * ewid), map(elen, top_y + rise, 0.0), map(-elen, top_y + rise, 0.0)];
        b.mb.quad_out(p, slope_n(s));
        // thickness: an underside + fascia lip
    }
    // ridge cap
    b.mb.mat(mat::FLAT).hex(roof);
    b.bx(map(-elen - 0.05, top_y + rise - 0.05, -0.12).min(map(elen + 0.05, top_y + rise + 0.1, 0.12)), map(-elen - 0.05, top_y + rise - 0.05, -0.12).max(map(elen + 0.05, top_y + rise + 0.1, 0.12)), roof, mat::FLAT, false);
    // underside of the overhang
    b.mb.mat(mat::PLASTER).hex(trim);
    b.mb.quad_out([map(-elen, top_y - 0.04, -ewid), map(elen, top_y - 0.04, -ewid), map(elen, top_y - 0.04, ewid), map(-elen, top_y - 0.04, ewid)], Vec3::NEG_Y);
    // fascia boards along the eaves
    for s in [-1.0f32, 1.0] {
        let a = map(-elen, top_y - 0.04, s * ewid - 0.06);
        let c = map(elen, top_y + 0.14, s * ewid + 0.06);
        b.bx(a.min(c), a.max(c), trim, mat::FLAT, false);
    }
    // gable ends in wall colour
    let y_at_wall = top_y + rise * (ov / ewid);
    b.mb.mat(wall_mat).hex(wall);
    for s in [-1.0f32, 1.0] {
        let x = s * len_half;
        let out = map(s, 0.0, 0.0);
        let bl = map(x, top_y, -wid_half);
        let br = map(x, top_y, wid_half);
        let tl = map(x, y_at_wall, wid_half);
        let tr = map(x, y_at_wall, -wid_half);
        b.mb.quad_out([bl, br, tl, tr], out);
        b.mb.tri_out(map(x, y_at_wall, -wid_half), map(x, y_at_wall, wid_half), map(x, top_y + rise, 0.0), out);
        // round attic vent
        let c = map(x + s * 0.02, top_y + rise * 0.5, 0.0);
        b.mb.mat(mat::FLAT).hex(trim);
        b.mb.push_xf(Mat4::from_translation(c) * if ridge_x { Mat4::from_rotation_z(FRAC_PI_2) } else { Mat4::from_rotation_x(FRAC_PI_2) });
        b.mb.cylinder(Vec3::new(0.0, -0.03, 0.0), 0.32, 0.32, 0.06, 10, true, true);
        b.mb.pop_xf();
        b.mb.mat(wall_mat).hex(wall);
    }
    // rake boards along the sloped edges of both gable ends
    for s in [-1.0f32, 1.0] {
        for t in [-1.0f32, 1.0] {
            b.board(map(s * elen, top_y - 0.03, t * ewid), map(s * elen, top_y + rise + 0.02, 0.0), map(s, 0.0, 0.0), 0.2, 0.06, trim, mat::FLAT);
        }
    }
    // gutters along both eaves, with a downspout running down the wall at one corner
    for t in [-1.0f32, 1.0] {
        let (p0, p1) = (map(-elen, top_y + 0.02, t * (ewid + 0.03)), map(elen, top_y + 0.02, t * (ewid + 0.03)));
        b.tube(p0, p1, 0.06, 0x9aa0a8, mat::METAL);
    }
    {
        let (x, z_eave, z_wall) = (len_half - 0.4, ewid + 0.03, wid_half + 0.09);
        let y_out = top_y - 0.14;
        b.tube(map(x, top_y + 0.02, z_eave), map(x, y_out, z_eave), 0.045, 0x9aa0a8, mat::METAL);
        b.tube(map(x, y_out, z_eave), map(x, y_out, z_wall), 0.045, 0x9aa0a8, mat::METAL);
        b.tube(map(x, y_out, z_wall), map(x, 0.05, z_wall), 0.045, 0x9aa0a8, mat::METAL);
    }
    // colliders: two wedges
    if ridge_x {
        b.cols.push(Collider::wedge(Vec3::new(-ex, top_y, 0.0), Vec3::new(ex, top_y + rise, ez), 3, Tag::Static));
        b.cols.push(Collider::wedge(Vec3::new(-ex, top_y, -ez), Vec3::new(ex, top_y + rise, 0.0), 1, Tag::Static));
    } else {
        b.cols.push(Collider::wedge(Vec3::new(0.0, top_y, -ez), Vec3::new(ex, top_y + rise, ez), 2, Tag::Static));
        b.cols.push(Collider::wedge(Vec3::new(-ex, top_y, -ez), Vec3::new(0.0, top_y + rise, ez), 0, Tag::Static));
    }
}

// ---------------------------------------------------------------------------------
// Interior furniture
// ---------------------------------------------------------------------------------

fn table(b: &mut B, c: Vec3, w: f32, d: f32, col: u32) {
    let top_y = 0.78;
    b.bx(Vec3::new(c.x - w / 2.0, c.y + top_y - 0.07, c.z - d / 2.0), Vec3::new(c.x + w / 2.0, c.y + top_y, c.z + d / 2.0), col, mat::WOOD, true);
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let p = Vec3::new(c.x + sx * (w / 2.0 - 0.08), c.y, c.z + sz * (d / 2.0 - 0.08));
        b.bx(Vec3::new(p.x - 0.04, p.y, p.z - 0.04), Vec3::new(p.x + 0.04, p.y + top_y - 0.07, p.z + 0.04), col, mat::WOOD, false);
    }
    b.loot.push(Vec3::new(c.x, c.y + top_y, c.z));
}

fn chair(b: &mut B, c: Vec3, yaw: f32, col: u32) {
    b.mb.push_xf(Mat4::from_translation(c) * Mat4::from_rotation_y(yaw));
    b.mb.mat(mat::WOOD).hex(col);
    b.mb.box_min_max(Vec3::new(-0.22, 0.42, -0.22), Vec3::new(0.22, 0.48, 0.22));
    b.mb.box_min_max(Vec3::new(-0.22, 0.48, 0.18), Vec3::new(0.22, 0.95, 0.22));
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        b.mb.box_min_max(Vec3::new(sx * 0.18 - 0.025, 0.0, sz * 0.18 - 0.025), Vec3::new(sx * 0.18 + 0.025, 0.42, sz * 0.18 + 0.025));
    }
    b.mb.pop_xf();
    b.cols.push(Collider::aabb_box(c + Vec3::new(-0.22, 0.0, -0.22), c + Vec3::new(0.22, 0.5, 0.22), Tag::Static));
}

fn bed(b: &mut B, c: Vec3, yaw: f32, blanket: u32) {
    // local: head toward -Z
    b.mb.push_xf(Mat4::from_translation(c) * Mat4::from_rotation_y(yaw));
    b.mb.mat(mat::WOOD).hex(0x8d6636);
    b.mb.box_min_max(Vec3::new(-0.55, 0.0, -1.0), Vec3::new(0.55, 0.32, 1.0));
    b.mb.box_min_max(Vec3::new(-0.58, 0.0, -1.05), Vec3::new(0.58, 0.85, -0.95));
    b.mb.mat(mat::CLOTH).hex(0xf4f1ea);
    b.mb.box_min_max(Vec3::new(-0.5, 0.32, -0.92), Vec3::new(0.5, 0.42, 0.0));
    b.mb.hex(0xfffdf7);
    b.mb.box_min_max(Vec3::new(-0.4, 0.42, -0.9), Vec3::new(0.4, 0.5, -0.5));
    b.mb.hex(blanket);
    b.mb.box_min_max(Vec3::new(-0.52, 0.32, -0.1), Vec3::new(0.52, 0.5, 0.98));
    b.mb.pop_xf();
    let (lo, hi) = (Vec3::new(-1.05, 0.0, -1.05), Vec3::new(1.05, 0.6, 1.05));
    let (a, bb) = rot_box(lo, hi, yaw, c);
    b.cols.push(Collider::aabb_box(a, bb, Tag::Static));
    b.loot.push(c + Vec3::new(0.0, 0.52, 0.2));
}

fn rot_box(lo: Vec3, hi: Vec3, yaw: f32, c: Vec3) -> (Vec3, Vec3) {
    let m = Mat4::from_translation(c) * Mat4::from_rotation_y(yaw);
    let mut bb = Aabb::EMPTY;
    for k in 0..8 {
        let p = Vec3::new(if k & 1 == 0 { lo.x } else { hi.x }, if k & 2 == 0 { lo.y } else { hi.y }, if k & 4 == 0 { lo.z } else { hi.z });
        bb.extend(m.transform_point3(p));
    }
    (bb.min, bb.max)
}

fn shelf(b: &mut B, c: Vec3, yaw: f32) {
    b.mb.push_xf(Mat4::from_translation(c) * Mat4::from_rotation_y(yaw));
    b.mb.mat(mat::WOOD).hex(0x7a5232);
    b.mb.box_min_max(Vec3::new(-0.85, 0.0, -0.2), Vec3::new(0.85, 1.9, 0.2));
    // shelves with colourful books
    let cols = [0xc0392b, 0x2980b9, 0xf1c40f, 0x27ae60, 0x8e44ad, 0xe67e22];
    for r in 0..4 {
        let y = 0.25 + r as f32 * 0.42;
        b.mb.mat(mat::FLAT);
        for k in 0..7 {
            b.mb.hex(cols[(k + r) % cols.len()]);
            let x = -0.7 + k as f32 * 0.22;
            b.mb.box_min_max(Vec3::new(x, y, -0.15), Vec3::new(x + 0.14, y + 0.3, 0.19));
        }
    }
    b.mb.pop_xf();
    let (a, bb) = rot_box(Vec3::new(-0.85, 0.0, -0.2), Vec3::new(0.85, 1.9, 0.2), yaw, c);
    b.cols.push(Collider::aabb_box(a, bb, Tag::Static));
}

fn sofa(b: &mut B, c: Vec3, yaw: f32, col: u32) {
    b.mb.push_xf(Mat4::from_translation(c) * Mat4::from_rotation_y(yaw));
    b.mb.mat(mat::CLOTH).hex(col);
    b.mb.box_min_max(Vec3::new(-1.0, 0.0, -0.45), Vec3::new(1.0, 0.45, 0.45));
    b.mb.box_min_max(Vec3::new(-1.0, 0.45, 0.25), Vec3::new(1.0, 0.95, 0.45));
    b.mb.box_min_max(Vec3::new(-1.0, 0.45, -0.45), Vec3::new(-0.8, 0.7, 0.25));
    b.mb.box_min_max(Vec3::new(0.8, 0.45, -0.45), Vec3::new(1.0, 0.7, 0.25));
    b.mb.pop_xf();
    let (a, bb) = rot_box(Vec3::new(-1.0, 0.0, -0.45), Vec3::new(1.0, 0.95, 0.45), yaw, c);
    b.cols.push(Collider::aabb_box(a, bb, Tag::Static));
}

fn rug(b: &mut B, c: Vec3, w: f32, d: f32, col: u32) {
    b.bx(Vec3::new(c.x - w / 2.0, c.y, c.z - d / 2.0), Vec3::new(c.x + w / 2.0, c.y + 0.025, c.z + d / 2.0), col, mat::CLOTH, false);
    b.bx(Vec3::new(c.x - w / 2.0 + 0.15, c.y + 0.025, c.z - d / 2.0 + 0.15), Vec3::new(c.x + w / 2.0 - 0.15, c.y + 0.03, c.z + d / 2.0 - 0.15), 0xf1e7c8, mat::CLOTH, false);
}

fn counter(b: &mut B, c: Vec3, w: f32, yaw: f32) {
    b.mb.push_xf(Mat4::from_translation(c) * Mat4::from_rotation_y(yaw));
    b.mb.mat(mat::WOOD).hex(0xe9e4d8);
    b.mb.box_min_max(Vec3::new(-w / 2.0, 0.0, -0.32), Vec3::new(w / 2.0, 0.9, 0.32));
    b.mb.hex(0x4b4f58).mat(mat::STONE);
    b.mb.box_min_max(Vec3::new(-w / 2.0 - 0.02, 0.9, -0.36), Vec3::new(w / 2.0 + 0.02, 0.96, 0.36));
    b.mb.pop_xf();
    let (a, bb) = rot_box(Vec3::new(-w / 2.0, 0.0, -0.36), Vec3::new(w / 2.0, 0.96, 0.36), yaw, c);
    b.cols.push(Collider::aabb_box(a, bb, Tag::Static));
    b.loot.push(c + Vec3::new(0.0, 0.96, 0.0));
}

/// True if no solid already in the building (furniture, walls) rises above the floor `y` within the `rx` x `rz` half-extents
/// around (x, z): somewhere an item can lie or a chest can stand.
fn floor_clear(b: &B, x: f32, z: f32, y: f32, rx: f32, rz: f32) -> bool {
    !b.cols.iter().any(|c| {
        let bb = c.shape.aabb();
        bb.min.x < x + rx && bb.max.x > x - rx && bb.min.z < z + rz && bb.max.z > z - rz && bb.min.y < y + 0.5 && bb.max.y > y + 0.05
    })
}

/// Furnish the ground-floor interior of a house. Keeps the door lane and stairs clear.
fn furnish(b: &mut B, rng: &mut Rng, hx: f32, hz: f32, door_x: f32, stairs: Option<(f32, f32, f32, f32)>) {
    let iy = FLOOR_Y;
    let (ix0, ix1, iz0, iz1) = (-hx + WALL_T + 0.1, hx - WALL_T - 0.1, -hz + WALL_T + 0.1, hz - WALL_T - 0.1);
    let blocked = |x: f32, z: f32, r: f32| -> bool {
        // door lane: from door inward
        if (x - door_x).abs() < 1.5 + r && z > hz - 3.2 {
            return true;
        }
        if let Some((sx0, sx1, sz0, sz1)) = stairs {
            if x > sx0 - r - 0.4 && x < sx1 + r + 0.4 && z > sz0 - r - 0.4 && z < sz1 + r + 0.6 {
                return true;
            }
        }
        false
    };
    // rug in the middle
    let rc = Vec3::new(0.0, iy, -hz * 0.1);
    if !blocked(rc.x, rc.z, 1.2) {
        rug(b, rc, (hx * 0.9).min(3.0), (hz * 0.7).min(2.4), *rng.pick(&[0xc0583e, 0x3f78a8, 0x5b8f4a, 0xb88a3b]));
    }
    // items along the back wall: bed / shelf / counter / sofa
    let mut x = ix0 + 1.0;
    let back_z = iz0 + 0.5;
    let mut placed = 0;
    while x < ix1 - 0.9 && placed < 3 {
        if blocked(x, back_z, 1.0) {
            x += 1.8;
            continue;
        }
        match rng.below(4) {
            0 => {
                bed(b, Vec3::new(x + 0.2, iy, iz0 + 1.1), 0.0, *rng.pick(&[0xd35d4f, 0x4f7fd3, 0x58b57a, 0xd3a24f]));
                x += 1.9;
            }
            1 => {
                shelf(b, Vec3::new(x, iy, iz0 + 0.25), 0.0);
                x += 1.9;
            }
            2 => {
                counter(b, Vec3::new(x, iy, iz0 + 0.4), 1.8, 0.0);
                x += 2.0;
            }
            _ => {
                sofa(b, Vec3::new(x, iy, iz0 + 0.6), 0.0, *rng.pick(&[0x6b8fb8, 0xb8706b, 0x7ba66f, 0xc9a35a]));
                x += 2.2;
            }
        }
        placed += 1;
    }
    // table + chairs somewhere on the side away from the door lane
    let tx = if door_x > 0.0 { -hx * 0.45 } else { hx * 0.45 };
    let tc = Vec3::new(tx, iy, hz * 0.1);
    if !blocked(tc.x, tc.z, 1.0) && hz > 3.2 {
        table(b, tc, 1.4, 0.9, 0xb98b56);
        chair(b, tc + Vec3::new(-0.2, 0.0, 0.7), PI, 0xa07a48);
        chair(b, tc + Vec3::new(0.3, 0.0, -0.7), 0.0, 0xa07a48);
    }
    // a couple of loose floor loot spots
    for _ in 0..2 {
        let p = Vec3::new(rng.range(ix0 + 0.6, ix1 - 0.6), iy, rng.range(iz0 + 1.8, iz1 - 1.8));
        // not inside a bed, a sofa or a shelf
        if !blocked(p.x, p.z, 0.6) && floor_clear(b, p.x, p.z, iy, 0.3, 0.3) {
            b.loot.push(p);
        }
    }
}

// ---------------------------------------------------------------------------------
// Houses
// ---------------------------------------------------------------------------------

/// A cottage / two-story house. `w` x `d` is the outer footprint.
pub fn gen_house(rng: &mut Rng, w: f32, d: f32, st: &Style) -> Geom {
    let mut b = B::new();
    let (hx, hz) = (w / 2.0, d / 2.0);
    let floors = if st.floors >= 2 && d >= 9.6 { 2 } else { 1 };
    let top_y = FLOOR_Y + floors as f32 * STORY_H;
    let door_u = (w * *rng.pick(&[0.3f32, 0.5, 0.7])).clamp(1.3, w - 1.3);
    let door_x = -hx + door_u;

    // --- foundation & floor
    b.mb.ao(0.55, 1.0);
    b.bx(Vec3::new(-hx - 0.1, -1.6, -hz - 0.1), Vec3::new(hx + 0.1, FLOOR_Y, hz + 0.1), 0x9b9a95, mat::STONE, true);
    b.mb.ao(1.0, 1.0);
    b.bx_faces(Vec3::new(-hx + WALL_T, FLOOR_Y, -hz + WALL_T), Vec3::new(hx - WALL_T, FLOOR_Y + 0.02, hz - WALL_T), 0xb98b56, mat::WOOD, 0b000100, false);

    let front = Line { along_x: true, a0: -hx, a1: hx, c: hz - WALL_T / 2.0, out: 1.0 };
    let back = Line { along_x: true, a0: -hx, a1: hx, c: -hz + WALL_T / 2.0, out: -1.0 };
    let left = Line { along_x: false, a0: -hz + WALL_T, a1: hz - WALL_T, c: -hx + WALL_T / 2.0, out: -1.0 };
    let right = Line { along_x: false, a0: -hz + WALL_T, a1: hz - WALL_T, c: hx - WALL_T / 2.0, out: 1.0 };

    for fl in 0..floors {
        let y0 = FLOOR_Y + fl as f32 * STORY_H;
        let (win_v0, win_v1) = (0.95, 2.15);
        // front: door on floor 0
        let mut ops: Vec<Op> = vec![];
        let mut avoid = vec![];
        if fl == 0 {
            ops.push(Op { u0: door_u - DOOR_W / 2.0, u1: door_u + DOOR_W / 2.0, v0: 0.0, v1: DOOR_H });
            avoid.push((door_u - DOOR_W / 2.0 - 0.3, door_u + DOOR_W / 2.0 + 0.3));
        }
        let mut win_ops: Vec<Op> = vec![];
        for c in B::window_slots(front.len(), &avoid, 3.4) {
            let o = Op { u0: c - 0.65, u1: c + 0.65, v0: win_v0, v1: win_v1 };
            ops.push(o);
            win_ops.push(o);
        }
        b.wall(front, y0, STORY_H, &ops, st.wall, st.wall_mat);
        if fl == 0 {
            let door = ops[0];
            b.opening_trim(front, y0, &door, false, st.trim, None);
        }
        for o in &win_ops {
            b.opening_trim(front, y0, o, true, st.trim, st.shutters);
        }
        // back
        let mut bops = vec![];
        for c in B::window_slots(back.len(), &[], 3.6) {
            bops.push(Op { u0: c - 0.65, u1: c + 0.65, v0: win_v0, v1: win_v1 });
        }
        b.wall(back, y0, STORY_H, &bops, st.wall, st.wall_mat);
        for o in &bops {
            b.opening_trim(back, y0, o, true, st.trim, st.shutters);
        }
        // sides (shorter: between front and back)
        for side in [left, right] {
            let mut sops = vec![];
            for c in B::window_slots(side.len(), &[], 3.8) {
                sops.push(Op { u0: c - 0.65, u1: c + 0.65, v0: win_v0, v1: win_v1 });
            }
            b.wall(side, y0, STORY_H, &sops, st.wall, st.wall_mat);
            for o in &sops {
                b.opening_trim(side, y0, o, true, st.trim, st.shutters);
            }
        }
        // base board
        b.corner_posts(hx - 0.06, hz - 0.06, y0, y0 + STORY_H, st.trim);
    }
    // exterior band between floors
    if floors == 2 {
        let y = FLOOR_Y + STORY_H;
        b.bx(Vec3::new(-hx - 0.06, y - 0.12, -hz - 0.06), Vec3::new(hx + 0.06, y + 0.12, hz + 0.06), st.trim, mat::FLAT, false);
    }

    // --- stairs + upper floor
    let mut stairs_rect = None;
    if floors == 2 {
        let l = 5.6f32;
        let x0 = -hx + WALL_T + 0.15;
        let (z_t, z_b) = (-hz + WALL_T + 0.1, -hz + WALL_T + 0.1 + l);
        let up_y = FLOOR_Y + STORY_H;
        b.mb.mat(mat::WOOD).hex(0xa57a47);
        // visual: wedge ramp, plus a stringer
        b.mb.wedge(Vec3::new(x0, FLOOR_Y, z_t), Vec3::new(x0 + 1.3, up_y, z_b), 3);
        b.cols.push(Collider::wedge(Vec3::new(x0, FLOOR_Y, z_t), Vec3::new(x0 + 1.3, up_y, z_b), 3, Tag::Static));
        b.bx(Vec3::new(x0 + 1.3, FLOOR_Y, z_t), Vec3::new(x0 + 1.36, up_y + 1.0, z_t + 0.06), 0x7a5232, mat::WOOD, false);
        stairs_rect = Some((x0, x0 + 1.3, z_t, z_b));
        // upper slab with a hole above the stairs
        let hole_z1 = z_b - 1.7;
        let (sx0, sx1) = (x0 - 0.05, x0 + 1.35);
        let slab_y0 = up_y - 0.3;
        let (ix0, ix1, iz0, iz1) = (-hx + WALL_T, hx - WALL_T, -hz + WALL_T, hz - WALL_T);
        let pieces = [
            (ix0, sx0, iz0, iz1),
            (sx1, ix1, iz0, iz1),
            (sx0, sx1, hole_z1, iz1),
        ];
        for (a0, a1, c0, c1) in pieces {
            if a1 - a0 > 0.05 && c1 - c0 > 0.05 {
                b.bx(Vec3::new(a0, slab_y0, c0), Vec3::new(a1, up_y, c1), 0xb98b56, mat::WOOD, true);
            }
        }
        // upper floor furnishings: a bed and loot
        b.loot.push(Vec3::new(hx * 0.4, up_y, hz * 0.2));
        b.loot.push(Vec3::new(-hx * 0.1, up_y, -hz * 0.3));
        if rng.chance(0.5) {
            b.chests.push((Vec3::new(hx - WALL_T - 0.5, up_y, -hz * 0.2), -FRAC_PI_2));
        }
    }

    // --- ceiling + roof
    b.bx(Vec3::new(-hx, top_y - 0.2, -hz), Vec3::new(hx, top_y, hz), 0xf1ede2, mat::PLASTER, true);
    let ridge_x = if (w - d).abs() < 1.0 { st.ridge_x } else { w >= d };
    let rise = (if ridge_x { d } else { w }) * 0.3 + 0.4;
    gable_roof(&mut b, hx, hz, top_y, rise, ridge_x, st.roof, st.wall, st.trim, st.wall_mat);
    if st.chimney {
        let cx = if rng.chance(0.5) { hx * 0.45 } else { -hx * 0.45 };
        let cz = if ridge_x { -hz * 0.5 } else { cx * 0.0 + hz * 0.2 };
        let cx = if ridge_x { cx } else { -hx * 0.55 };
        b.bx(Vec3::new(cx - 0.4, top_y - 0.3, cz - 0.4), Vec3::new(cx + 0.4, top_y + rise + 0.9, cz + 0.4), 0xa8553f, mat::BRICK, true);
        b.bx(Vec3::new(cx - 0.5, top_y + rise + 0.9, cz - 0.5), Vec3::new(cx + 0.5, top_y + rise + 1.05, cz + 0.5), 0x5c5c60, mat::STONE, false);
    }

    // --- stoop / porch
    // (the stoop is as wide as the porch in front of it: otherwise the corners beside it are pits when the ground falls away)
    let (px0, px1) = ((door_x - 1.9).max(-hx + 0.2), (door_x + 1.9).min(hx - 0.2));
    let (sx0, sx1) = if st.porch { (px0, px1) } else { (door_x - 1.0, door_x + 1.0) };
    b.bx(Vec3::new(sx0, -1.0, hz), Vec3::new(sx1, 0.14, hz + 1.0), 0xa8a7a2, mat::STONE, true);
    if st.porch {
        b.bx(Vec3::new(px0, -1.0, hz + 1.0), Vec3::new(px1, FLOOR_Y, hz + 2.6), 0xa88556, mat::WOOD, true);
        for px in [px0 + 0.12, px1 - 0.12] {
            b.bx(Vec3::new(px - 0.1, FLOOR_Y, hz + 2.4), Vec3::new(px + 0.1, FLOOR_Y + 2.7, hz + 2.6), st.trim, mat::FLAT, true);
        }
        b.bx(Vec3::new(px0 - 0.1, FLOOR_Y + 2.7, hz - 0.1), Vec3::new(px1 + 0.1, FLOOR_Y + 2.88, hz + 2.8), st.roof, mat::SHINGLE, true);
    }

    house_details(&mut b, hx, hz, door_u, floors, st);

    // --- interior
    furnish(&mut b, rng, hx, hz, door_x, stairs_rect);
    if rng.chance(0.35) && floors == 1 {
        // the back corner away from the door; if furniture got there first, the other corner or a side wall
        let sx = if door_x > 0.0 { -hx + WALL_T + 0.55 } else { hx - WALL_T - 0.55 };
        let back = -hz + WALL_T + 0.45;
        let candidates = [
            (Vec3::new(sx, FLOOR_Y, back), 0.0),
            (Vec3::new(-sx, FLOOR_Y, back), 0.0),
            (Vec3::new(hx - WALL_T - 0.5, FLOOR_Y, -hz * 0.2), -FRAC_PI_2),
            (Vec3::new(-hx + WALL_T + 0.5, FLOOR_Y, -hz * 0.2), FRAC_PI_2),
        ];
        let fits = |p: &Vec3, yaw: f32| {
            // clear of the door lane, and of furniture over the chest's footprint plus the room it needs to open
            let (rx, rz) = if yaw == 0.0 { (0.55, 0.4) } else { (0.4, 0.55) };
            let lane = (p.x - door_x).abs() < 1.5 + rx && p.z > hz - 3.2;
            !lane && floor_clear(&b, p.x, p.z, FLOOR_Y, rx, rz)
        };
        if let Some(&(p, yaw)) = candidates.iter().find(|(p, yaw)| fits(p, *yaw)) {
            b.chests.push((p, yaw));
        }
    }

    let door_out = Vec3::new(door_x, 0.0, hz + 2.6);
    let door_in = Vec3::new(door_x, FLOOR_Y, hz - 1.6);
    let mut g = b.finish(Vec2::new(hx, hz), top_y + rise, door_out, door_in);
    g.loot.truncate(8);
    g.entry = Some(if st.porch { Entry { edge: Vec3::new(door_x, FLOOR_Y, hz + 2.6), half_w: 1.0 } } else { Entry { edge: Vec3::new(door_x, 0.14, hz + 1.0), half_w: 1.0 } });
    g
}

/// A log cabin: darker wood, steeper roof, stone chimney. Used at the lodge.
pub fn gen_cabin(rng: &mut Rng, w: f32, d: f32) -> Geom {
    let st = Style {
        wall: *rng.pick(&[0x9a6a3e, 0xa9794a, 0x8a5c36]),
        wall_mat: mat::WOOD,
        trim: 0x5d3f27,
        roof: *rng.pick(&[0x4a3a30, 0x5a4636, 0x3e5a3a]),
        shutters: Some(*rng.pick(&[0x2f6b4a, 0x9b3a2f, 0x2f4f7f])),
        porch: true,
        chimney: true,
        ridge_x: true,
        floors: 1,
        logs: true,
    };
    gen_house(rng, w, d, &st)
}

// ---------------------------------------------------------------------------------
// Shop
// ---------------------------------------------------------------------------------

pub fn gen_shop(rng: &mut Rng, w: f32, d: f32) -> Geom {
    let mut b = B::new();
    let (hx, hz) = (w / 2.0, d / 2.0);
    let wall = *rng.pick(&[0xE8D2A6, 0xC4D8E8, 0xE9B9A0, 0xBFE0C9, 0xF2E3B5]);
    let accent = *rng.pick(&[0xC24A3C, 0x2F8F8A, 0x3F5F94, 0xD9772F]);
    let h = 3.6;
    let top_y = FLOOR_Y + h;
    b.mb.ao(0.55, 1.0);
    b.bx(Vec3::new(-hx - 0.1, -1.6, -hz - 0.1), Vec3::new(hx + 0.1, FLOOR_Y, hz + 0.1), 0x9b9a95, mat::STONE, true);
    b.mb.ao(1.0, 1.0);
    b.bx_faces(Vec3::new(-hx + WALL_T, FLOOR_Y, -hz + WALL_T), Vec3::new(hx - WALL_T, FLOOR_Y + 0.02, hz - WALL_T), 0xcfc7b8, mat::STONE, 0b000100, false);
    let front = Line { along_x: true, a0: -hx, a1: hx, c: hz - WALL_T / 2.0, out: 1.0 };
    let back = Line { along_x: true, a0: -hx, a1: hx, c: -hz + WALL_T / 2.0, out: -1.0 };
    let left = Line { along_x: false, a0: -hz + WALL_T, a1: hz - WALL_T, c: -hx + WALL_T / 2.0, out: -1.0 };
    let right = Line { along_x: false, a0: -hz + WALL_T, a1: hz - WALL_T, c: hx - WALL_T / 2.0, out: 1.0 };
    let door_u = w * 0.5;
    let mut ops = vec![Op { u0: door_u - 0.9, u1: door_u + 0.9, v0: 0.0, v1: 2.5 }];
    let big = [(1.1, door_u - 1.3), (door_u + 1.3, w - 1.1)];
    let mut wins = vec![];
    for (a, c) in big {
        if c - a > 1.0 {
            let o = Op { u0: a, u1: c, v0: 0.8, v1: 2.6 };
            ops.push(o);
            wins.push(o);
        }
    }
    b.wall(front, FLOOR_Y, h, &ops, wall, mat::PLASTER);
    b.opening_trim(front, FLOOR_Y, &ops[0], false, 0xf7f5ee, None);
    for o in &wins {
        b.opening_trim(front, FLOOR_Y, o, true, accent, None);
    }
    let bops: Vec<Op> = B::window_slots(back.len(), &[], 4.0).into_iter().map(|c| Op { u0: c - 0.7, u1: c + 0.7, v0: 1.1, v1: 2.3 }).collect();
    b.wall(back, FLOOR_Y, h, &bops, wall, mat::PLASTER);
    for o in &bops {
        b.opening_trim(back, FLOOR_Y, o, true, accent, None);
    }
    for side in [left, right] {
        let sops: Vec<Op> = B::window_slots(side.len(), &[], 4.0).into_iter().map(|c| Op { u0: c - 0.7, u1: c + 0.7, v0: 1.1, v1: 2.3 }).collect();
        b.wall(side, FLOOR_Y, h, &sops, wall, mat::PLASTER);
        for o in &sops {
            b.opening_trim(side, FLOOR_Y, o, true, accent, None);
        }
    }
    // flat roof with parapet and cornice
    b.bx(Vec3::new(-hx, top_y - 0.2, -hz), Vec3::new(hx, top_y, hz), 0x8a8d93, mat::STONE, true);
    let par = 0.5;
    b.bx(Vec3::new(-hx - 0.1, top_y, -hz - 0.1), Vec3::new(hx + 0.1, top_y + par, -hz + 0.2), wall, mat::PLASTER, true);
    b.bx(Vec3::new(-hx - 0.1, top_y, hz - 0.2), Vec3::new(hx + 0.1, top_y + par, hz + 0.1), wall, mat::PLASTER, true);
    b.bx(Vec3::new(-hx - 0.1, top_y, -hz + 0.2), Vec3::new(-hx + 0.2, top_y + par, hz - 0.2), wall, mat::PLASTER, true);
    b.bx(Vec3::new(hx - 0.2, top_y, -hz + 0.2), Vec3::new(hx + 0.1, top_y + par, hz - 0.2), wall, mat::PLASTER, true);
    b.bx(Vec3::new(-hx - 0.15, top_y + par, -hz - 0.15), Vec3::new(hx + 0.15, top_y + par + 0.1, hz + 0.15), accent, mat::FLAT, false);
    // striped awning over the front
    let aw_y = FLOOR_Y + 2.9;
    let stripes = ((w + 0.6) / 0.6).floor() as i32;
    for k in 0..stripes {
        let x0 = -hx - 0.3 + k as f32 * ((w + 0.6) / stripes as f32);
        let x1 = x0 + (w + 0.6) / stripes as f32;
        let col = if k % 2 == 0 { accent } else { 0xf7f2e6 };
        b.mb.mat(mat::CLOTH).hex(col);
        let (p0, p1, p2, p3) = (Vec3::new(x0, aw_y, hz), Vec3::new(x1, aw_y, hz), Vec3::new(x1, aw_y - 0.45, hz + 1.7), Vec3::new(x0, aw_y - 0.45, hz + 1.7));
        b.mb.quad_out([p0, p1, p2, p3], Vec3::new(0.0, 0.6, 0.5));
        b.mb.quad_out([p0, p1, p2, p3], Vec3::new(0.0, -0.6, -0.5));
    }
    for sx in [-hx + 0.1, hx - 0.1] {
        b.bx(Vec3::new(sx - 0.05, 0.0, hz + 1.55), Vec3::new(sx + 0.05, aw_y - 0.4, hz + 1.65), 0x4a4d57, mat::METAL, false);
    }
    b.bx(Vec3::new(-hx - 0.3, 0.0, hz), Vec3::new(hx + 0.3, 0.12, hz + 1.8), 0xb5b3ad, mat::STONE, true);
    // fruit stands under the awning, planters beside the door and a unit on the roof
    for (k, sx) in [-1.0f32, 1.0].into_iter().enumerate() {
        let x = sx * (hx - 1.7);
        let (a, c) = (Vec3::new(x - 0.7, 0.12, hz + 0.45), Vec3::new(x + 0.7, 0.72, hz + 1.15));
        b.bx(a, c, 0x9c6a3a, mat::WOOD, false);
        b.mb.mat(mat::WOOD).hex(0x7a5232);
        b.mb.box_min_max(Vec3::new(x - 0.74, 0.72, hz + 0.41), Vec3::new(x + 0.74, 0.78, hz + 1.19));
        let fruit = [[0xd33a2a, 0xf0a030, 0x7ac143], [0xf0c030, 0xd33a2a, 0xa6d85a]][k];
        b.mb.mat(mat::FLAT).tinted(false);
        for row in 0..3 {
            for j in 0..6 {
                let (fx, fz) = (x - 0.55 + j as f32 * 0.22, hz + 0.58 + row as f32 * 0.22);
                b.mb.hex(fruit[(j + row) % 3]);
                b.mb.sphere(Vec3::new(fx, 0.86, fz), 0.1, 0);
            }
        }
        b.mb.tinted(true);
        // a little price board
        b.bx(Vec3::new(x - 0.35, 0.78, hz + 1.2), Vec3::new(x + 0.35, 1.05, hz + 1.23), 0x2b2e34, mat::FLAT, false);
    }
    for sx in [-1.0f32, 1.0] {
        let x = sx * 1.55;
        b.bx(Vec3::new(x - 0.35, 0.12, hz + 0.3), Vec3::new(x + 0.35, 0.55, hz + 0.75), 0x8a5a34, mat::WOOD, false);
        b.mb.mat(mat::FOLIAGE).tinted(false).hex(0x4c9a3a);
        b.mb.blob(Vec3::new(x, 0.75, hz + 0.52), Vec3::new(0.34, 0.3, 0.3), 1, 0.15, 3, true);
        b.mb.mat(mat::FLAT).hex(if sx < 0.0 { 0xff6f91 } else { 0xffd23f });
        for k in 0..6 {
            let a = k as f32 * 2.399;
            b.mb.sphere(Vec3::new(x + a.cos() * 0.2, 0.8 + 0.12 * (k % 3) as f32, hz + 0.52 + a.sin() * 0.18), 0.06, 0);
        }
        b.mb.tinted(true);
    }
    b.bx(Vec3::new(hx * 0.3 - 0.7, top_y + par + 0.1, -hz * 0.3 - 0.6), Vec3::new(hx * 0.3 + 0.7, top_y + par + 0.75, -hz * 0.3 + 0.6), 0xb8bcc2, mat::METAL, false);
    b.bx(Vec3::new(hx * 0.3 - 0.55, top_y + par + 0.75, -hz * 0.3 - 0.45), Vec3::new(hx * 0.3 + 0.55, top_y + par + 0.8, -hz * 0.3 + 0.45), 0x4a4d57, mat::METAL, false);
    // shop sign
    b.bx(Vec3::new(-1.6, top_y - 0.1, hz + 0.02), Vec3::new(1.6, top_y + 0.45, hz + 0.1), accent, mat::FLAT, false);
    b.bx(Vec3::new(-1.4, top_y + 0.02, hz + 0.1), Vec3::new(1.4, top_y + 0.33, hz + 0.13), 0xfff7e0, mat::FLAT, false);
    // interior: shelves & counter
    let iy = FLOOR_Y;
    for k in 0..3 {
        let x = -hx + 1.4 + k as f32 * 2.2;
        if x < hx - 1.2 {
            shelf(&mut b, Vec3::new(x, iy, -hz + WALL_T + 0.3), 0.0);
        }
    }
    counter(&mut b, Vec3::new(if rng.chance(0.5) { -hx * 0.4 } else { hx * 0.4 }, iy, hz * 0.2), 2.6, 0.0);
    let (ix, iz) = (hx * 0.0, -hz * 0.1);
    let _ = (ix, iz);
    // loose loot not on the counter's footprint, and the chest in whichever back corner the shelves left free
    for p in [Vec3::new(rng.range(-hx + 1.5, hx - 1.5), iy, rng.range(-hz * 0.3, hz * 0.3)), Vec3::new(rng.range(-hx + 1.5, hx - 1.5), iy, -hz + 2.0)] {
        if floor_clear(&b, p.x, p.z, iy, 0.3, 0.3) {
            b.loot.push(p);
        }
    }
    let corner = Vec3::new(hx - WALL_T - 0.6, iy, -hz + WALL_T + 0.5);
    if let Some(p) = [corner, Vec3::new(-corner.x, iy, corner.z)].into_iter().find(|p| floor_clear(&b, p.x, p.z, iy, 0.55, 0.4)) {
        b.chests.push((p, 0.0));
    }
    let door_x = -hx + door_u;
    let mut g = b.finish(Vec2::new(hx, hz), top_y + par, Vec3::new(door_x, 0.0, hz + 2.4), Vec3::new(door_x, FLOOR_Y, hz - 1.6));
    g.entry = Some(Entry { edge: Vec3::new(door_x, 0.12, hz + 1.8), half_w: 1.0 });
    g
}

// ---------------------------------------------------------------------------------
// Barn
// ---------------------------------------------------------------------------------

pub fn gen_barn(rng: &mut Rng) -> Geom {
    let mut b = B::new();
    let (w, d) = (11.0f32, 14.0f32);
    let (hx, hz) = (w / 2.0, d / 2.0);
    let h = 5.0;
    let top_y = FLOOR_Y + h;
    let red = 0xA8402F;
    b.mb.ao(0.55, 1.0);
    b.bx(Vec3::new(-hx - 0.1, -1.6, -hz - 0.1), Vec3::new(hx + 0.1, FLOOR_Y, hz + 0.1), 0x8d8c88, mat::STONE, true);
    b.mb.ao(1.0, 1.0);
    b.bx_faces(Vec3::new(-hx + WALL_T, FLOOR_Y, -hz + WALL_T), Vec3::new(hx - WALL_T, FLOOR_Y + 0.02, hz - WALL_T), 0xa5845a, mat::WOOD, 0b000100, false);
    let front = Line { along_x: true, a0: -hx, a1: hx, c: hz - WALL_T / 2.0, out: 1.0 };
    let back = Line { along_x: true, a0: -hx, a1: hx, c: -hz + WALL_T / 2.0, out: -1.0 };
    let left = Line { along_x: false, a0: -hz + WALL_T, a1: hz - WALL_T, c: -hx + WALL_T / 2.0, out: -1.0 };
    let right = Line { along_x: false, a0: -hz + WALL_T, a1: hz - WALL_T, c: hx - WALL_T / 2.0, out: 1.0 };
    // big double door in the front wall (opening 3.4 wide, 3.6 high), small windows up high
    let door_u = w * 0.5;
    let ops = vec![Op { u0: door_u - 1.8, u1: door_u + 1.8, v0: 0.0, v1: 3.7 }, Op { u0: 1.2, u1: 2.2, v0: 3.2, v1: 4.1 }, Op { u0: w - 2.2, u1: w - 1.2, v0: 3.2, v1: 4.1 }];
    b.wall(front, FLOOR_Y, h, &ops, red, mat::WOOD);
    b.opening_trim(front, FLOOR_Y, &ops[0], false, 0xf5efe0, None);
    // white X-braced barn doors flanking the opening (visual only)
    for sx in [-1.0f32, 1.0] {
        let x0 = door_u - 1.8 + (if sx < 0.0 { -1.8 } else { 3.6 });
        let (a, c) = front.bx(x0 + 0.0, x0 + 1.8, WALL_T / 2.0, WALL_T / 2.0 + 0.05, FLOOR_Y, FLOOR_Y + 3.7);
        b.bx(a, c, 0xf5efe0, mat::WOOD, false);
    }
    b.opening_trim(front, FLOOR_Y, &ops[1], true, 0xf5efe0, None);
    b.opening_trim(front, FLOOR_Y, &ops[2], true, 0xf5efe0, None);
    b.wall(back, FLOOR_Y, h, &[Op { u0: 4.0, u1: 5.0, v0: 3.0, v1: 4.0 }], red, mat::WOOD);
    for side in [left, right] {
        let sops: Vec<Op> = [3.0f32, 8.0].iter().map(|&c| Op { u0: c, u1: c + 1.0, v0: 2.6, v1: 3.6 }).collect();
        b.wall(side, FLOOR_Y, h, &sops, red, mat::WOOD);
        for o in &sops {
            b.opening_trim(side, FLOOR_Y, o, true, 0xf5efe0, None);
        }
    }
    b.corner_posts(hx - 0.06, hz - 0.06, FLOOR_Y, FLOOR_Y + h, 0xf5efe0);
    b.bx(Vec3::new(-hx, top_y - 0.2, -hz), Vec3::new(hx, top_y, hz), 0x6a4a2e, mat::WOOD, true);
    gable_roof(&mut b, hx, hz, top_y, 3.2, false, 0x6c6f78, red, 0xf5efe0, mat::WOOD);
    // hay loft door high in the front gable, with the hoist beam above it, and a cupola with a weather vane on the ridge
    let loft_y = top_y + 0.5;
    b.bx(Vec3::new(-0.8, loft_y, hz + 0.01), Vec3::new(0.8, loft_y + 1.5, hz + 0.06), 0x3a1f18, mat::WOOD, false);
    for (x0, x1) in [(-0.9f32, -0.8f32), (0.8, 0.9)] {
        b.bx(Vec3::new(x0, loft_y - 0.1, hz + 0.0), Vec3::new(x1, loft_y + 1.6, hz + 0.1), 0xf5efe0, mat::WOOD, false);
    }
    b.bx(Vec3::new(-0.95, loft_y + 1.5, hz + 0.0), Vec3::new(0.95, loft_y + 1.65, hz + 0.1), 0xf5efe0, mat::WOOD, false);
    b.bx(Vec3::new(-0.8, loft_y + 0.72, hz + 0.0), Vec3::new(0.8, loft_y + 0.78, hz + 0.1), 0xf5efe0, mat::WOOD, false);
    b.bx(Vec3::new(-0.06, top_y + 2.45, hz), Vec3::new(0.06, top_y + 2.55, hz + 0.55), 0x6a4a2e, mat::WOOD, false);
    b.tube(Vec3::new(0.0, top_y + 2.5, hz + 0.5), Vec3::new(0.0, top_y + 1.9, hz + 0.5), 0.02, 0xc9b27a, mat::CLOTH);
    b.bx(Vec3::new(-0.18, top_y + 1.6, hz + 0.35), Vec3::new(0.18, top_y + 1.9, hz + 0.55), 0xb5a07a, mat::WOOD, false);
    let cu = top_y + 3.2;
    b.bx(Vec3::new(-0.55, cu - 0.05, -0.55), Vec3::new(0.55, cu + 0.9, 0.55), 0xf5efe0, mat::WOOD, false);
    b.bx(Vec3::new(-0.3, cu + 0.2, 0.54), Vec3::new(0.3, cu + 0.7, 0.58), 0x3a1f18, mat::WOOD, false);
    b.mb.mat(mat::METAL).hex(0x6c6f78);
    b.mb.cylinder(Vec3::new(0.0, cu + 0.9, 0.0), 0.85, 0.0, 0.7, 4, true, false);
    b.tube(Vec3::new(0.0, cu + 1.55, 0.0), Vec3::new(0.0, cu + 2.5, 0.0), 0.03, 0x2b2e34, mat::METAL);
    b.bx(Vec3::new(-0.5, cu + 2.1, -0.02), Vec3::new(0.5, cu + 2.25, 0.02), 0x2b2e34, mat::METAL, false);
    b.mb.mat(mat::METAL).hex(0x2b2e34);
    b.mb.sphere(Vec3::new(0.0, cu + 2.55, 0.0), 0.07, 1);
    // hay bales and crates inside
    for (x, z) in [(-3.0, -4.0), (-3.0, -2.6), (-1.6, -4.0), (3.2, -3.0), (3.2, 0.5)] {
        let (a, c) = (Vec3::new(x - 0.7, FLOOR_Y, z - 0.6), Vec3::new(x + 0.7, FLOOR_Y + 1.2, z + 0.6));
        b.bx(a, c, 0xe2b94a, mat::FLAT, true);
        b.loot.push(Vec3::new(x, FLOOR_Y + 1.2, z));
    }
    b.loot.push(Vec3::new(0.0, FLOOR_Y, -2.0));
    b.loot.push(Vec3::new(-3.0, FLOOR_Y, 2.0));
    b.chests.push((Vec3::new(0.0, FLOOR_Y, -hz + WALL_T + 0.5), 0.0));
    b.chests.push((Vec3::new(hx - WALL_T - 0.5, FLOOR_Y, 2.5), -FRAC_PI_2));
    let _ = rng;
    let mut g = b.finish(Vec2::new(hx, hz), top_y + 3.2, Vec3::new(0.0, 0.0, hz + 3.0), Vec3::new(0.0, FLOOR_Y, hz - 2.0));
    g.entry = Some(Entry { edge: Vec3::new(0.0, FLOOR_Y, hz + 0.1), half_w: 1.7 });
    g
}

// ---------------------------------------------------------------------------------
// Landmarks
// ---------------------------------------------------------------------------------

pub fn gen_windmill() -> Geom {
    let mut b = B::new();
    let h = 11.0;
    // tapered octagonal tower
    b.mb.mat(mat::PLASTER).hex(0xf1e9d6).ao(0.55, 1.0);
    b.mb.cylinder(Vec3::new(0.0, -1.0, 0.0), 3.5, 2.4, h + 1.0, 8, true, true);
    b.cols.push(Collider::cyl(0.0, 0.0, 3.0, -1.0, h, Tag::Static));
    // stone base ring
    b.cyl(Vec3::new(0.0, -1.0, 0.0), 3.75, 1.7, 0x8f8f8a, mat::STONE, false);
    // doorway (decorative arch on the front) with a frame, a lintel, a step and a lantern
    b.bx(Vec3::new(-0.7, 0.0, 2.6), Vec3::new(0.7, 2.3, 3.2), 0x6a4a2e, mat::WOOD, false);
    for sx in [-1.0f32, 1.0] {
        b.bx(Vec3::new(sx * 0.82 - 0.1, 0.0, 2.55), Vec3::new(sx * 0.82 + 0.1, 2.5, 3.3), 0xf1e9d6, mat::STONE, false);
    }
    b.bx(Vec3::new(-0.95, 2.3, 2.55), Vec3::new(0.95, 2.55, 3.3), 0xf1e9d6, mat::STONE, false);
    b.bx(Vec3::new(-1.1, -0.1, 3.0), Vec3::new(1.1, 0.14, 3.7), 0xa8a7a2, mat::STONE, false);
    b.bx(Vec3::new(0.95, 1.7, 3.1), Vec3::new(1.15, 2.0, 3.3), 0xffe29a, mat::EMISSIVE, false);
    // brick bands round the tower
    b.mb.mat(mat::STONE).hex(0xc9b9a0);
    for y in [3.2f32, 8.0] {
        let r = 3.5 - (y + 1.0) / (h + 1.0) * 1.1 + 0.05;
        b.mb.cylinder(Vec3::new(0.0, y, 0.0), r, r, 0.22, 8, false, false);
    }
    // wooden band + cap
    b.mb.mat(mat::SHINGLE).hex(0x8a4b3a).ao(0.7, 1.0);
    b.mb.cylinder(Vec3::new(0.0, h, 0.0), 2.9, 0.0, 3.4, 8, true, false);
    b.mb.mat(mat::FLAT).hex(0x5b4026);
    b.mb.cylinder(Vec3::new(0.0, h - 0.3, 0.0), 2.65, 2.65, 0.35, 8, true, true);
    // window slits
    b.mb.hex(0x303030);
    for k in 0..4 {
        let a = k as f32 * PI / 2.0 + PI / 8.0;
        b.bx(Vec3::new(a.sin() * 2.55 - 0.2, 6.0, a.cos() * 2.55 - 0.2), Vec3::new(a.sin() * 2.55 + 0.2, 7.4, a.cos() * 2.55 + 0.2), 0x303030, mat::FLAT, false);
    }
    let mut g = b.finish(Vec2::new(3.6, 3.6), h + 3.4, Vec3::new(0.0, 0.0, 6.0), Vec3::new(0.0, 0.0, 4.0));
    g.hub = Some(Vec3::new(0.0, h - 0.8, 3.2));
    g
}

pub fn gen_lighthouse() -> Geom {
    let mut b = B::new();
    // striped tapered tower
    let segs = 6;
    let total = 20.0;
    for i in 0..segs {
        let t0 = i as f32 / segs as f32;
        let t1 = (i + 1) as f32 / segs as f32;
        let (r0, r1) = (3.4 - 1.4 * t0, 3.4 - 1.4 * t1);
        b.mb.mat(mat::PLASTER).hex(if i % 2 == 0 { 0xf4f1ea } else { 0xc43b32 }).ao(0.6, 1.0);
        b.mb.cylinder(Vec3::new(0.0, t0 * total - if i == 0 { 1.0 } else { 0.0 }, 0.0), r0, r1, total / segs as f32 + if i == 0 { 1.0 } else { 0.0 }, 14, i == 0, false);
    }
    b.cols.push(Collider::cyl(0.0, 0.0, 3.1, -1.0, total, Tag::Static));
    // gallery
    b.cyl(Vec3::new(0.0, total, 0.0), 2.6, 0.35, 0x3d3f46, mat::METAL, false);
    b.mb.mat(mat::FLAT).hex(0x3d3f46);
    for k in 0..12 {
        let a = k as f32 / 12.0 * TAU_F;
        b.bx(Vec3::new(a.cos() * 2.45 - 0.04, total + 0.35, a.sin() * 2.45 - 0.04), Vec3::new(a.cos() * 2.45 + 0.04, total + 1.3, a.sin() * 2.45 + 0.04), 0x3d3f46, mat::METAL, false);
    }
    // lamp room (emissive)
    b.mb.mat(mat::EMISSIVE).hex(0xfff2b0);
    b.mb.cylinder(Vec3::new(0.0, total + 0.35, 0.0), 1.5, 1.5, 2.2, 10, false, false);
    b.mb.mat(mat::METAL).hex(0xc43b32);
    b.mb.cylinder(Vec3::new(0.0, total + 2.55, 0.0), 1.9, 0.0, 1.8, 10, true, false);
    // door with a stone frame and a step, round windows up the tower, a finial on the lantern roof
    b.bx(Vec3::new(-0.7, 0.0, 3.1), Vec3::new(0.7, 2.2, 3.5), 0x5a3c28, mat::WOOD, false);
    for sx in [-1.0f32, 1.0] {
        b.bx(Vec3::new(sx * 0.85 - 0.12, 0.0, 3.0), Vec3::new(sx * 0.85 + 0.12, 2.45, 3.55), 0xe6e1d6, mat::STONE, false);
    }
    b.bx(Vec3::new(-1.0, 2.2, 3.0), Vec3::new(1.0, 2.5, 3.55), 0xe6e1d6, mat::STONE, false);
    b.bx(Vec3::new(-1.3, -0.1, 3.4), Vec3::new(1.3, 0.16, 4.2), 0xa8a7a2, mat::STONE, false);
    for (k, y) in [5.5f32, 9.5, 13.5, 17.0].into_iter().enumerate() {
        let t = (y / total).clamp(0.0, 1.0);
        let r = 3.4 - 1.4 * t;
        let a = k as f32 * 1.9;
        b.mb.mat(mat::GLASS).hex(0x2a3b4a);
        b.mb.push_xf(Mat4::from_translation(Vec3::new(a.sin() * r, y, a.cos() * r)) * Mat4::from_rotation_y(a));
        b.mb.push_xf(Mat4::from_rotation_x(FRAC_PI_2));
        b.mb.cylinder(Vec3::new(0.0, -0.1, 0.0), 0.34, 0.34, 0.12, 10, true, true);
        b.mb.pop_xf();
        b.mb.pop_xf();
    }
    b.mb.mat(mat::METAL).hex(0x3d3f46);
    b.mb.sphere(Vec3::new(0.0, total + 4.5, 0.0), 0.22, 1);
    b.finish(Vec2::new(3.5, 3.5), total + 4.6, Vec3::new(0.0, 0.0, 6.0), Vec3::new(0.0, 0.0, 4.0))
}

const TAU_F: f32 = std::f32::consts::TAU;

pub fn gen_water_tower() -> Geom {
    let mut b = B::new();
    let leg_h = 9.0;
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        b.bx(Vec3::new(sx * 2.4 - 0.18, -0.5, sz * 2.4 - 0.18), Vec3::new(sx * 2.4 + 0.18, leg_h, sz * 2.4 + 0.18), 0x6e7078, mat::METAL, true);
    }
    // cross braces
    b.mb.mat(mat::METAL).hex(0x6e7078);
    for y in [2.5f32, 5.5] {
        b.bx(Vec3::new(-2.5, y, -2.5), Vec3::new(2.5, y + 0.1, -2.3), 0x6e7078, mat::METAL, false);
        b.bx(Vec3::new(-2.5, y, 2.3), Vec3::new(2.5, y + 0.1, 2.5), 0x6e7078, mat::METAL, false);
        b.bx(Vec3::new(-2.5, y, -2.5), Vec3::new(-2.3, y + 0.1, 2.5), 0x6e7078, mat::METAL, false);
        b.bx(Vec3::new(2.3, y, -2.5), Vec3::new(2.5, y + 0.1, 2.5), 0x6e7078, mat::METAL, false);
    }
    // X braces between the legs on all four faces
    for k in 0..4 {
        let (c, sn) = ((k as f32 * FRAC_PI_2).cos(), (k as f32 * FRAC_PI_2).sin());
        let rot = |x: f32, z: f32| Vec3::new(x * c - z * sn, 0.0, x * sn + z * c);
        for (y0, y1) in [(0.4f32, 5.0f32), (5.0, 8.8)] {
            let (a, d) = (rot(-2.4, 2.4), rot(2.4, 2.4));
            b.tube(a + Vec3::Y * y0, d + Vec3::Y * y1, 0.05, 0x6e7078, mat::METAL);
            b.tube(d + Vec3::Y * y0, a + Vec3::Y * y1, 0.05, 0x6e7078, mat::METAL);
        }
    }
    // a ladder up the front, and the legs sit on concrete footings
    for sx in [-0.28f32, 0.28] {
        b.tube(Vec3::new(sx, 0.0, 2.75), Vec3::new(sx, leg_h + 1.1, 2.75), 0.035, 0x6e7078, mat::METAL);
    }
    for k in 0..24 {
        let y = 0.4 + k as f32 * 0.4;
        b.bx(Vec3::new(-0.3, y, 2.72), Vec3::new(0.3, y + 0.04, 2.78), 0x6e7078, mat::METAL, false);
    }
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        b.bx(Vec3::new(sx * 2.4 - 0.4, -0.5, sz * 2.4 - 0.4), Vec3::new(sx * 2.4 + 0.4, 0.15, sz * 2.4 + 0.4), 0xa8a7a2, mat::STONE, false);
    }
    // platform + tank
    b.bx(Vec3::new(-3.0, leg_h, -3.0), Vec3::new(3.0, leg_h + 0.3, 3.0), 0x575961, mat::METAL, true);
    // railing round the platform
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        b.tube(Vec3::new(sx * 2.92, leg_h + 0.3, sz * 2.92), Vec3::new(sx * 2.92, leg_h + 1.3, sz * 2.92), 0.04, 0x6e7078, mat::METAL);
    }
    for (a, d) in [((-2.92f32, -2.92f32), (2.92f32, -2.92f32)), ((2.92, -2.92), (2.92, 2.92)), ((2.92, 2.92), (-2.92, 2.92)), ((-2.92, 2.92), (-2.92, -2.92))] {
        for y in [leg_h + 0.75, leg_h + 1.3] {
            b.tube(Vec3::new(a.0, y, a.1), Vec3::new(d.0, y, d.1), 0.03, 0x6e7078, mat::METAL);
        }
    }
    b.cyl(Vec3::new(0.0, leg_h + 0.3, 0.0), 2.7, 4.2, 0x4f86c6, mat::METAL, true);
    b.mb.mat(mat::METAL).hex(0x3d4f6b);
    for y in [1.0f32, 2.2, 3.4] {
        b.mb.cylinder(Vec3::new(0.0, leg_h + 0.3 + y, 0.0), 2.74, 2.74, 0.12, 14, false, false);
    }
    b.mb.hex(0x4f5560);
    b.mb.cylinder(Vec3::new(0.0, leg_h + 4.5, 0.0), 2.9, 0.0, 1.8, 14, true, false);
    b.finish(Vec2::new(3.0, 3.0), leg_h + 6.3, Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 3.0))
}

pub fn gen_silo() -> Geom {
    let mut b = B::new();
    b.cyl(Vec3::new(0.0, -1.0, 0.0), 2.6, 13.0, 0xc9ccd1, mat::METAL, true);
    b.mb.mat(mat::METAL).hex(0x8c9097);
    for y in [2.0f32, 5.0, 8.0, 11.0] {
        b.mb.cylinder(Vec3::new(0.0, y, 0.0), 2.65, 2.65, 0.14, 14, false, false);
    }
    b.mb.hex(0x7c8087);
    b.mb.cylinder(Vec3::new(0.0, 12.0, 0.0), 2.62, 0.0, 1.9, 14, true, false);
    // a ladder with a safety cage up the front, a small door and a roof vent
    for sx in [-0.3f32, 0.3] {
        b.tube(Vec3::new(sx, 0.0, 2.72), Vec3::new(sx, 12.4, 2.72), 0.035, 0x6e7078, mat::METAL);
    }
    for k in 0..30 {
        b.bx(Vec3::new(-0.32, 0.4 + k as f32 * 0.4, 2.68), Vec3::new(0.32, 0.44 + k as f32 * 0.4, 2.74), 0x6e7078, mat::METAL, false);
    }
    b.bx(Vec3::new(-0.45, 0.0, 2.56), Vec3::new(0.45, 1.5, 2.66), 0x5a3c28, mat::METAL, false);
    b.mb.mat(mat::METAL).hex(0x3d3f46);
    b.mb.cylinder(Vec3::new(0.0, 13.8, 0.0), 0.35, 0.35, 0.5, 8, false, true);
    b.mb.sphere(Vec3::new(0.0, 14.4, 0.0), 0.2, 1);
    b.finish(Vec2::new(2.7, 2.7), 14.8, Vec3::new(0.0, 0.0, 4.5), Vec3::new(0.0, 0.0, 3.0))
}

/// Gas station: a canopy on pillars with two pumps and a small shop.
pub fn gen_gas_canopy() -> Geom {
    let mut b = B::new();
    let (hx, hz) = (7.0f32, 4.5f32);
    let top = 5.0;
    b.bx(Vec3::new(-hx, top, -hz), Vec3::new(hx, top + 0.7, hz), 0xf2f0ea, mat::FLAT, true);
    b.bx(Vec3::new(-hx - 0.05, top + 0.45, -hz - 0.05), Vec3::new(hx + 0.05, top + 0.7, hz + 0.05), 0xc43b32, mat::FLAT, false);
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        b.bx(Vec3::new(sx * 5.8 - 0.25, 0.0, sz * 3.6 - 0.25), Vec3::new(sx * 5.8 + 0.25, top, sz * 3.6 + 0.25), 0xe9e7e0, mat::FLAT, true);
    }
    // concrete pad
    b.bx(Vec3::new(-hx - 1.0, -0.5, -hz - 1.0), Vec3::new(hx + 1.0, 0.1, hz + 1.0), 0x9d9e9f, mat::ASPHALT, true);
    // pumps
    for px in [-2.5f32, 2.5] {
        b.bx(Vec3::new(px - 0.45, 0.1, -0.35), Vec3::new(px + 0.45, 1.5, 0.35), 0xd8d8d6, mat::METAL, true);
        b.bx(Vec3::new(px - 0.45, 1.5, -0.35), Vec3::new(px + 0.45, 1.75, 0.35), 0xc43b32, mat::FLAT, false);
        b.bx(Vec3::new(px - 0.3, 0.8, 0.35), Vec3::new(px + 0.3, 1.2, 0.4), 0x2c3a4a, mat::EMISSIVE, false);
        b.bx(Vec3::new(px - 0.7, 0.1, -0.7), Vec3::new(px + 0.7, 0.3, 0.7), 0xb8b8b4, mat::STONE, false);
        b.loot.push(Vec3::new(px, 0.1, 1.4));
        // hose and nozzle holster on the side of each pump
        b.tube(Vec3::new(px - 0.45, 1.2, 0.0), Vec3::new(px - 0.65, 0.7, 0.0), 0.03, 0x2b2e34, mat::CLOTH);
        b.bx(Vec3::new(px - 0.7, 0.55, -0.08), Vec3::new(px - 0.6, 0.85, 0.08), 0x2b2e34, mat::METAL, false);
    }
    // canopy underside lights and a tall price sign at the corner of the pad
    for lx in [-4.5f32, 0.0, 4.5] {
        b.bx(Vec3::new(lx - 0.5, top - 0.06, -0.15), Vec3::new(lx + 0.5, top, 0.15), 0xfff3c4, mat::EMISSIVE, false);
    }
    b.tube(Vec3::new(hx + 0.45, 0.0, hz + 0.6), Vec3::new(hx + 0.45, 4.6, hz + 0.6), 0.1, 0x575961, mat::METAL);
    b.bx(Vec3::new(hx - 0.05, 4.6, hz + 0.45), Vec3::new(hx + 0.95, 6.0, hz + 0.75), 0xf2f0ea, mat::FLAT, false);
    b.bx(Vec3::new(hx - 0.05, 5.6, hz + 0.75), Vec3::new(hx + 0.95, 6.0, hz + 0.78), 0xc43b32, mat::FLAT, false);
    for k in 0..3 {
        b.bx(Vec3::new(hx + 0.05, 4.7 + k as f32 * 0.3, hz + 0.75), Vec3::new(hx + 0.85, 4.9 + k as f32 * 0.3, hz + 0.78), 0x2c3a4a, mat::EMISSIVE, false);
    }
    b.finish(Vec2::new(hx, hz), top + 0.7, Vec3::new(0.0, 0.0, 6.0), Vec3::new(0.0, 0.0, 3.0))
}

pub fn gen_fountain() -> Geom {
    let mut b = B::new();
    // basin: a stone drum with a rolled rim and eight finials, filled with glowing water
    b.cyl(Vec3::new(0.0, -0.5, 0.0), 3.2, 1.0, 0xbfc3c8, mat::STONE, true);
    b.mb.mat(mat::STONE).hex(0xd4d7dc).ao(0.8, 1.0);
    b.mb.cylinder(Vec3::new(0.0, 0.38, 0.0), 3.3, 3.3, 0.16, 20, false, true);
    for k in 0..8 {
        let a = k as f32 / 8.0 * TAU_F;
        b.mb.sphere(Vec3::new(a.cos() * 3.2, 0.62, a.sin() * 3.2), 0.17, 1);
        b.mb.cylinder(Vec3::new(a.cos() * 3.2, 0.5, a.sin() * 3.2), 0.12, 0.12, 0.08, 6, false, false);
    }
    b.cyl(Vec3::new(0.0, 0.54, 0.0), 2.6, 0.04, 0x3db4e8, mat::EMISSIVE, false);
    // pedestal with collars, an upper bowl, a spire and a crown
    b.cyl(Vec3::new(0.0, 0.5, 0.0), 0.5, 1.8, 0xd5d8dc, mat::STONE, false);
    b.mb.mat(mat::STONE).hex(0xc2c6cc);
    for y in [0.9f32, 1.9] {
        b.mb.cylinder(Vec3::new(0.0, y, 0.0), 0.62, 0.62, 0.14, 12, false, true);
    }
    b.cyl(Vec3::new(0.0, 2.2, 0.0), 1.3, 0.2, 0xd5d8dc, mat::STONE, false);
    b.mb.mat(mat::STONE).hex(0xe2e4e8);
    b.mb.cylinder(Vec3::new(0.0, 2.3, 0.0), 1.38, 1.38, 0.1, 16, false, true);
    b.cyl(Vec3::new(0.0, 2.4, 0.0), 1.1, 0.05, 0x3db4e8, mat::EMISSIVE, false);
    b.cyl(Vec3::new(0.0, 2.4, 0.0), 0.2, 1.0, 0xd5d8dc, mat::STONE, false);
    b.mb.mat(mat::STONE).hex(0xe2e4e8);
    b.mb.sphere(Vec3::new(0.0, 3.45, 0.0), 0.26, 2);
    // jets: arcs of bright droplets from the crown down into the bowl
    b.mb.mat(mat::EMISSIVE).hex(0xcff0ff);
    for k in 0..6 {
        let a = k as f32 / 6.0 * TAU_F + 0.3;
        for j in 1..=5 {
            let t = j as f32 / 5.0;
            let (d, y) = (0.15 + 0.85 * t, 3.5 - 1.0 * t * t);
            b.mb.sphere(Vec3::new(a.cos() * d, y, a.sin() * d), 0.07 - 0.008 * t, 0);
        }
    }
    b.finish(Vec2::new(3.3, 3.3), 3.8, Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 4.0))
}

pub fn gen_well() -> Geom {
    let mut b = B::new();
    b.mb.mat(mat::STONE).hex(0x9a9a96);
    b.mb.cylinder(Vec3::new(0.0, -0.5, 0.0), 1.1, 1.1, 1.5, 12, true, false);
    b.cols.push(Collider::cyl(0.0, 0.0, 1.1, -0.5, 1.0, Tag::Static));
    // a darker stone course and a coping ring
    b.mb.hex(0x858580);
    b.mb.cylinder(Vec3::new(0.0, 0.3, 0.0), 1.12, 1.12, 0.12, 12, false, false);
    b.mb.hex(0xb5b5b0);
    b.mb.cylinder(Vec3::new(0.0, 0.96, 0.0), 1.2, 1.2, 0.1, 12, false, true);
    b.cyl(Vec3::new(0.0, 0.95, 0.0), 0.9, 0.05, 0x1d5f8a, mat::EMISSIVE, false);
    for sx in [-1.0f32, 1.0] {
        b.bx(Vec3::new(sx * 1.0 - 0.08, 0.5, -0.08), Vec3::new(sx * 1.0 + 0.08, 2.6, 0.08), 0x6a4a2e, mat::WOOD, false);
    }
    // windlass: a roller between the posts, a crank handle, rope and bucket over the water
    b.tube(Vec3::new(-1.05, 2.0, 0.0), Vec3::new(1.05, 2.0, 0.0), 0.1, 0x8a5a34, mat::WOOD);
    b.tube(Vec3::new(1.2, 2.0, 0.0), Vec3::new(1.2, 1.55, 0.0), 0.035, 0x4a4e57, mat::METAL);
    b.bx(Vec3::new(1.12, 1.5, -0.16), Vec3::new(1.28, 1.58, 0.16), 0x8a5a34, mat::WOOD, false);
    b.tube(Vec3::new(0.0, 2.0, 0.1), Vec3::new(0.0, 1.3, 0.1), 0.02, 0xc9b27a, mat::CLOTH);
    b.mb.mat(mat::WOOD).hex(0x8a5a34);
    b.mb.cylinder(Vec3::new(0.0, 1.02, 0.1), 0.2, 0.17, 0.3, 8, false, false);
    b.mb.hex(0x4a4e57).mat(mat::METAL);
    b.mb.cylinder(Vec3::new(0.0, 1.12, 0.1), 0.205, 0.205, 0.03, 8, false, false);
    b.mb.mat(mat::SHINGLE).hex(0x8a4b3a);
    b.mb.wedge(Vec3::new(-1.3, 2.6, -1.0), Vec3::new(0.0, 3.3, 1.0), 0);
    b.mb.wedge(Vec3::new(0.0, 2.6, -1.0), Vec3::new(1.3, 3.3, 1.0), 2);
    // ridge board
    b.bx(Vec3::new(-1.35, 3.28, -0.06), Vec3::new(1.35, 3.38, 0.06), 0x6a4a2e, mat::WOOD, false);
    b.finish(Vec2::new(1.3, 1.3), 3.3, Vec3::new(0.0, 0.0, 3.0), Vec3::new(0.0, 0.0, 2.0))
}

pub fn gen_lamp_post(col: u32) -> Geom {
    let mut b = B::new();
    // stepped base, a tapering pole with collars, a curved arm and a hexagonal lantern
    b.mb.mat(mat::METAL).hex(0x2e3138).ao(0.7, 1.0);
    b.mb.cylinder(Vec3::ZERO, 0.26, 0.2, 0.28, 8, true, false);
    b.mb.cylinder(Vec3::new(0.0, 0.28, 0.0), 0.16, 0.16, 0.1, 8, false, true);
    b.mb.hex(0x33363d);
    b.mb.cylinder(Vec3::new(0.0, 0.38, 0.0), 0.11, 0.075, 3.8, 8, false, false);
    b.mb.hex(0x4a4e57);
    for y in [1.4f32, 3.7] {
        b.mb.cylinder(Vec3::new(0.0, y, 0.0), 0.095, 0.095, 0.08, 8, false, true);
    }
    let arm = [(0.0f32, 4.12f32), (0.12, 4.3), (0.36, 4.36), (0.56, 4.28)];
    for w in arm.windows(2) {
        b.tube(Vec3::new(w[0].0, w[0].1, 0.0), Vec3::new(w[1].0, w[1].1, 0.0), 0.04, 0x33363d, mat::METAL);
    }
    let lx = 0.6;
    b.mb.mat(mat::METAL).hex(0x2e3138);
    b.mb.cylinder(Vec3::new(lx, 4.06, 0.0), 0.2, 0.04, 0.2, 6, true, false);
    b.mb.cylinder(Vec3::new(lx, 3.72, 0.0), 0.1, 0.12, 0.05, 6, true, false);
    b.mb.mat(mat::EMISSIVE).hex(col);
    b.mb.cylinder(Vec3::new(lx, 3.77, 0.0), 0.12, 0.17, 0.29, 6, false, false);
    b.cols.push(Collider::cyl(0.0, 0.0, 0.18, 0.0, 4.2, Tag::Static));
    b.finish(Vec2::new(0.3, 0.3), 4.4, Vec3::ZERO, Vec3::ZERO)
}

/// A park bench with a slatted back, turned by `yaw` (so it can face the fountain from a diagonal). The collider is a
/// plain cylinder so that a turned bench does not leave an invisible box around it.
pub fn gen_bench(yaw: f32) -> Geom {
    let mut b = B::new();
    b.mb.push_xf(Mat4::from_rotation_y(yaw));
    // iron ends, wooden slats for the seat and the back, armrests
    for sx in [-1.0f32, 1.0] {
        let x = sx * 0.78;
        b.bx(Vec3::new(x - 0.04, 0.0, -0.22), Vec3::new(x + 0.04, 0.46, -0.16), 0x2e3138, mat::METAL, false);
        b.bx(Vec3::new(x - 0.04, 0.0, 0.16), Vec3::new(x + 0.04, 0.46, 0.22), 0x2e3138, mat::METAL, false);
        b.bx(Vec3::new(x - 0.04, 0.46, -0.24), Vec3::new(x + 0.04, 0.52, 0.24), 0x2e3138, mat::METAL, false);
        b.bx(Vec3::new(x - 0.04, 0.46, 0.2), Vec3::new(x + 0.04, 0.98, 0.26), 0x2e3138, mat::METAL, false);
        b.bx(Vec3::new(x - 0.05, 0.66, -0.2), Vec3::new(x + 0.05, 0.7, 0.2), 0x2e3138, mat::METAL, false);
    }
    for k in 0..4 {
        let z = -0.2 + k as f32 * 0.13;
        b.bx(Vec3::new(-0.85, 0.5, z), Vec3::new(0.85, 0.55, z + 0.1), 0x9c6a3a, mat::WOOD, false);
    }
    for k in 0..3 {
        let y = 0.62 + k as f32 * 0.15;
        b.bx(Vec3::new(-0.85, y, 0.2), Vec3::new(0.85, y + 0.1, 0.25), 0x9c6a3a, mat::WOOD, false);
    }
    b.mb.pop_xf();
    b.cols.push(Collider::cyl(0.0, 0.0, 0.62, 0.0, 0.6, Tag::Static));
    b.finish(Vec2::new(0.9, 0.9), 1.0, Vec3::ZERO, Vec3::ZERO)
}

/// A stone planter with a shrub and flowers.
pub fn gen_planter(bloom: u32) -> Geom {
    let mut b = B::new();
    b.cyl(Vec3::ZERO, 0.62, 0.55, 0xc9c6bd, mat::STONE, true);
    b.mb.mat(mat::STONE).hex(0xdedbd2);
    b.mb.cylinder(Vec3::new(0.0, 0.52, 0.0), 0.68, 0.68, 0.1, 10, false, true);
    b.mb.mat(mat::FLAT).hex(0x4a3524);
    b.mb.cylinder(Vec3::new(0.0, 0.62, 0.0), 0.56, 0.56, 0.02, 10, false, true);
    b.mb.mat(mat::FOLIAGE).tinted(false).hex(0x4c9a3a).ao(0.6, 1.0);
    b.mb.blob(Vec3::new(0.0, 0.95, 0.0), Vec3::new(0.55, 0.42, 0.55), 1, 0.15, 5, true);
    b.mb.hex(0x5db04a);
    b.mb.blob(Vec3::new(0.2, 1.1, 0.1), Vec3::new(0.32, 0.28, 0.32), 1, 0.15, 9, true);
    b.mb.mat(mat::FLAT).hex(bloom).ao(1.0, 1.0);
    for k in 0..9 {
        let a = k as f32 * 2.399;
        let v = 0.2 + 0.6 * ((k * 5 % 9) as f32 / 9.0);
        let r = (1.0 - v * v).sqrt();
        b.mb.sphere(Vec3::new(a.cos() * r * 0.5, 0.95 + v * 0.42, a.sin() * r * 0.5), 0.07, 0);
    }
    b.finish(Vec2::new(0.7, 0.7), 1.4, Vec3::ZERO, Vec3::ZERO)
}

pub fn gen_dock(len: f32) -> Geom {
    let mut b = B::new();
    b.bx(Vec3::new(-1.1, 0.0, 0.0), Vec3::new(1.1, 0.18, len), 0x9d7a4c, mat::WOOD, true);
    let n = (len / 2.4) as i32;
    for k in 0..=n {
        let z = k as f32 * 2.4;
        for sx in [-1.0f32, 1.0] {
            b.bx(Vec3::new(sx * 1.0 - 0.1, -2.0, z - 0.1), Vec3::new(sx * 1.0 + 0.1, 0.9, z + 0.1), 0x6a4a2e, mat::WOOD, false);
        }
    }
    b.finish(Vec2::new(1.1, len * 0.5), 1.0, Vec3::ZERO, Vec3::ZERO)
}

/// A straight picket fence along +X from the origin.
pub fn gen_fence(len: f32, col: u32) -> Geom {
    let mut b = B::new();
    let n = (len / 0.45) as i32;
    for k in 0..=n {
        let x = k as f32 * (len / n.max(1) as f32);
        b.bx(Vec3::new(x - 0.04, 0.0, -0.03), Vec3::new(x + 0.04, 0.95, 0.03), col, mat::WOOD, false);
    }
    b.bx(Vec3::new(0.0, 0.25, -0.05), Vec3::new(len, 0.35, 0.05), col, mat::WOOD, false);
    b.bx(Vec3::new(0.0, 0.65, -0.05), Vec3::new(len, 0.75, 0.05), col, mat::WOOD, false);
    b.cols.push(Collider { shape: Shape::Box { min: Vec3::new(0.0, 0.0, -0.06), max: Vec3::new(len, 0.95, 0.06) }, tag: Tag::Static, blocks_bullets: false });
    b.finish(Vec2::new(len * 0.5, 0.1), 1.0, Vec3::ZERO, Vec3::ZERO)
}

// ---------------------------------------------------------------------------------
// Details
// ---------------------------------------------------------------------------------

/// A porch rail between two points at the same height: a top and a bottom rail with balusters.
fn porch_rail(b: &mut B, p0: Vec3, p1: Vec3, col: u32) {
    let (lo, hi) = (p0.min(p1), p0.max(p1));
    let len = (hi - lo).length();
    if len < 0.3 {
        return;
    }
    let along_x = (hi.x - lo.x) > (hi.z - lo.z);
    let (t, y) = (0.035, p0.y);
    let bx = |b: &mut B, a0: f32, a1: f32, y0: f32, y1: f32, half: f32| {
        if along_x {
            b.bx(Vec3::new(a0, y0, lo.z - half), Vec3::new(a1, y1, lo.z + half), col, mat::WOOD, false);
        } else {
            b.bx(Vec3::new(lo.x - half, y0, a0), Vec3::new(lo.x + half, y1, a1), col, mat::WOOD, false);
        }
    };
    let (a0, a1) = if along_x { (lo.x, hi.x) } else { (lo.z, hi.z) };
    bx(b, a0, a1, y + 0.9, y + 0.97, t + 0.015);
    bx(b, a0, a1, y + 0.2, y + 0.25, t);
    let n = (len / 0.27).floor() as i32;
    for k in 0..=n {
        let a = a0 + (a1 - a0) * k as f32 / n.max(1) as f32;
        bx(b, a - 0.02, a + 0.02, y + 0.25, y + 0.9, 0.02);
    }
}

/// Everything that dresses a house from outside without touching its walls or colliders: the front door standing ajar,
/// a porch lantern and rails, curtains and flower boxes in the windows.
fn house_details(b: &mut B, hx: f32, hz: f32, door_u: f32, floors: u32, st: &Style) {
    let front = Line { along_x: true, a0: -hx, a1: hx, c: hz - WALL_T / 2.0, out: 1.0 };
    let back = Line { along_x: true, a0: -hx, a1: hx, c: -hz + WALL_T / 2.0, out: -1.0 };
    let left = Line { along_x: false, a0: -hz + WALL_T, a1: hz - WALL_T, c: -hx + WALL_T / 2.0, out: -1.0 };
    let right = Line { along_x: false, a0: -hz + WALL_T, a1: hz - WALL_T, c: hx - WALL_T / 2.0, out: 1.0 };
    let door_x = -hx + door_u;
    let seed = (hx * 10.0) as i32 * 131 + (hz * 10.0) as i32;
    // the door and a lantern beside it
    let door_cols = [0x8a5a34u32, 0xb5463a, 0x3a7d8c, 0x2f4f7f, 0x3e6e42, 0xd9a23b];
    let door_col = if st.logs { 0x5d3f27 } else { door_cols[(hunit(seed, 5, 17) * 6.0) as usize % 6] };
    let door = Op { u0: door_u - DOOR_W / 2.0, u1: door_u + DOOR_W / 2.0, v0: 0.0, v1: DOOR_H };
    b.door_leaf(front, FLOOR_Y, &door, door_col, 0xe3c15a);
    let lu = if door_u + 1.2 < front.len() - 0.5 { door_u + 1.25 } else { door_u - 1.25 };
    let (a, c) = front.bx(lu - 0.07, lu + 0.07, WALL_T / 2.0, WALL_T / 2.0 + 0.1, FLOOR_Y + 1.95, FLOOR_Y + 2.25);
    b.bx(a, c, 0x2b2e34, mat::METAL, false);
    let (a, c) = front.bx(lu - 0.055, lu + 0.055, WALL_T / 2.0 + 0.01, WALL_T / 2.0 + 0.09, FLOOR_Y + 2.0, FLOOR_Y + 2.2);
    b.bx(a, c, 0xffe29a, mat::EMISSIVE, false);
    // a mailbox on a post at the edge of the lot, on whichever side of the door has more room
    let mx = if door_x > 0.0 { door_x - 3.1 } else { door_x + 3.1 };
    let mx = mx.clamp(-hx - 1.5, hx + 1.5);
    // (the bots' notion of "inside the building" is the mesh bounds minus the eaves, so nothing may stick out past the stoop)
    let mz = hz + if st.porch { 1.75 } else { 0.78 };
    let post_col = 0x6a4a2e;
    b.bx(Vec3::new(mx - 0.04, 0.0, mz - 0.04), Vec3::new(mx + 0.04, 1.05, mz + 0.04), post_col, mat::WOOD, false);
    let box_col = [0x2f4f7f, 0xb5463a, 0x3e6e42, 0x4a4e57][(hunit(seed, 9, 19) * 4.0) as usize % 4];
    b.mb.mat(mat::METAL).hex(box_col).spec(0.5);
    b.mb.push_xf(Mat4::from_translation(Vec3::new(mx, 1.2, mz)) * Mat4::from_rotation_z(FRAC_PI_2) * Mat4::from_rotation_x(FRAC_PI_2));
    b.mb.cylinder(Vec3::new(0.0, -0.22, 0.0), 0.11, 0.11, 0.44, 8, true, true);
    b.mb.pop_xf();
    b.bx(Vec3::new(mx + 0.11, 1.18, mz - 0.03), Vec3::new(mx + 0.13, 1.34, mz + 0.0), 0xd33a2a, mat::FLAT, false);
    // potted shrubs either side of the door when there is no porch
    if !st.porch {
        for sx in [-1.0f32, 1.0] {
            let x = door_x + sx * 1.45;
            if x.abs() < hx - 0.6 {
                b.mb.mat(mat::STONE).hex(0xc2663a);
                b.mb.cylinder(Vec3::new(x, 0.14, hz + 0.55), 0.22, 0.17, 0.34, 8, true, true);
                b.mb.mat(mat::FOLIAGE).tinted(false).hex(0x4c9a3a);
                b.mb.blob(Vec3::new(x, 0.7, hz + 0.55), Vec3::new(0.3, 0.3, 0.3), 1, 0.15, 3, true);
                b.mb.mat(mat::FLAT).hex(if sx < 0.0 { 0xff6f91 } else { 0xfdfbf0 });
                for k in 0..4 {
                    let a = k as f32 * 2.399 + 0.5;
                    b.mb.sphere(Vec3::new(x + a.cos() * 0.2, 0.78 + 0.1 * (k % 2) as f32, hz + 0.55 + a.sin() * 0.2), 0.055, 0);
                }
                b.mb.tinted(true);
            }
        }
    }
    // porch rails: along the front with a gap for the steps, and down both sides
    if st.porch {
        let (px0, px1) = ((door_x - 1.9).max(-hx + 0.2), (door_x + 1.9).min(hx - 0.2));
        let (y, zf) = (FLOOR_Y, hz + 2.5);
        porch_rail(b, Vec3::new(px0 + 0.22, y, zf), Vec3::new(door_x - 1.1, y, zf), st.trim);
        porch_rail(b, Vec3::new(door_x + 1.1, y, zf), Vec3::new(px1 - 0.22, y, zf), st.trim);
        porch_rail(b, Vec3::new(px0 + 0.12, y, hz + 1.05), Vec3::new(px0 + 0.12, y, zf - 0.12), st.trim);
        porch_rail(b, Vec3::new(px1 - 0.12, y, hz + 1.05), Vec3::new(px1 - 0.12, y, zf - 0.12), st.trim);
    }
    // a pendant lamp over the table's spot on each floor: a glowing shade you can see through the windows
    let tx = if door_x > 0.0 { -hx * 0.45 } else { hx * 0.45 };
    for fl in 0..floors {
        let ceil = FLOOR_Y + (fl + 1) as f32 * STORY_H - if fl + 1 == floors { 0.2 } else { 0.3 };
        let z = hz * 0.1;
        b.tube(Vec3::new(tx, ceil, z), Vec3::new(tx, ceil - 0.5, z), 0.012, 0x2b2e34, mat::METAL);
        b.mb.mat(mat::FLAT).hex(0xf3e6c4).ao(1.0, 1.0);
        b.mb.cylinder(Vec3::new(tx, ceil - 0.78, z), 0.26, 0.07, 0.28, 8, false, false);
        b.mb.mat(mat::EMISSIVE).hex(0xffe9b0);
        b.mb.cylinder(Vec3::new(tx, ceil - 0.79, z), 0.2, 0.2, 0.02, 8, true, true);
    }
    // curtains and flower boxes: same slots as the walls
    for fl in 0..floors {
        let y0 = FLOOR_Y + fl as f32 * STORY_H;
        let avoid: Vec<(f32, f32)> = if fl == 0 { vec![(door_u - DOOR_W / 2.0 - 0.3, door_u + DOOR_W / 2.0 + 0.3)] } else { vec![] };
        for (li, (line, spacing, av)) in [(front, 3.4f32, avoid.as_slice()), (back, 3.6, &[][..]), (left, 3.8, &[][..]), (right, 3.8, &[][..])].into_iter().enumerate() {
            for (k, c) in B::window_slots(line.len(), av, spacing).into_iter().enumerate() {
                let o = Op { u0: c - 0.65, u1: c + 0.65, v0: 0.95, v1: 2.15 };
                b.window_dressing(line, y0, &o, seed * 7 + li as i32 * 31 + k as i32 * 3 + fl as i32 * 101, fl == 0 && li == 0);
            }
        }
    }
}

// ---------------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------------

fn rot_dir(dir: u8, k: u8) -> u8 {
    // +X(0) -> -Z(3) -> -X(2) -> +Z(1) -> +X(0) for one CCW quarter turn about +Y
    const ORDER: [u8; 4] = [0, 3, 2, 1];
    let i = ORDER.iter().position(|&d| d == dir & 3).unwrap();
    ORDER[(i + k as usize) % 4]
}

/// A building placed in the world.
#[derive(Clone, Debug)]
pub struct Placed {
    pub mesh: MeshData,
    pub cols: Vec<Collider>,
    pub loot: Vec<Vec3>,
    pub chests: Vec<(Vec3, f32)>,
    pub door_out: Vec3,
    pub door_in: Vec3,
    pub aabb: Aabb,
    pub footprint_half: Vec2,
    pub hub: Option<Vec3>,
    pub rot: u8,
    pub height: f32,
    /// World-space bounds of the steps added in front of a raised entrance (see [`add_entrance_steps`]).
    pub steps: Option<Aabb>,
}

/// Rotate by `rot` quarter turns about +Y and translate to `pos` (ground height at pos.y).
pub fn place(g: &Geom, pos: Vec3, rot: u8, tag: Tag) -> Placed {
    let ang = rot as f32 * FRAC_PI_2;
    let m = Mat4::from_translation(pos) * Mat4::from_rotation_y(ang);
    let mut mesh = MeshData::default();
    mesh.append(&g.mesh, m);
    let mut aabb = Aabb::EMPTY;
    for v in &mesh.verts {
        aabb.extend(Vec3::from(v.pos));
    }
    let tp = |p: Vec3| m.transform_point3(p);
    let cols = g
        .cols
        .iter()
        .map(|c| {
            let shape = match c.shape {
                Shape::Box { min, max } => {
                    let (a, b) = (tp(min), tp(max));
                    Shape::Box { min: a.min(b), max: a.max(b) }
                }
                Shape::Wedge { min, max, dir } => {
                    let (a, b) = (tp(min), tp(max));
                    Shape::Wedge { min: a.min(b), max: a.max(b), dir: rot_dir(dir, rot) }
                }
                Shape::Cyl { cx, cz, r, y0, y1 } => {
                    let p = tp(Vec3::new(cx, 0.0, cz));
                    Shape::Cyl { cx: p.x, cz: p.z, r, y0: y0 + pos.y, y1: y1 + pos.y }
                }
            };
            Collider { shape, tag, blocks_bullets: c.blocks_bullets }
        })
        .collect();
    Placed {
        mesh,
        cols,
        loot: g.loot.iter().map(|p| tp(*p)).collect(),
        chests: g.chests.iter().map(|(p, y)| (tp(*p), y + ang)).collect(),
        door_out: tp(g.door_out),
        door_in: tp(g.door_in),
        aabb,
        footprint_half: if rot % 2 == 0 { g.half } else { Vec2::new(g.half.y, g.half.x) },
        hub: g.hub.map(tp),
        rot,
        height: g.height,
        steps: None,
    }
}

/// A drop below this from a landing to the terrain is a plain step; anything more needs a stair.
const STEP_UP: f32 = 0.4;

/// On a sloping lot the door side of a house can stand a metre or more above the terrain (the floor sits at the highest
/// point of the footprint). Run a flight of steps from the landing's edge down to the ground: stepped mesh plus a walkable
/// wedge. Without it the door is out of reach for anyone who cannot jump that high, and bots never jump on purpose.
/// `ground(x, z)` is the terrain height; `placed` and `pos`/`rot` are the building as placed by [`place`].
pub fn add_entrance_steps(placed: &mut Placed, g: &Geom, pos: Vec3, rot: u8, tag: Tag, ground: &dyn Fn(f32, f32) -> f32) {
    let Some(entry) = g.entry else { return };
    let m = Mat4::from_translation(pos) * Mat4::from_rotation_y(rot as f32 * FRAC_PI_2);
    let height_at = |x: f32, z: f32| {
        let p = m.transform_point3(Vec3::new(x, 0.0, z));
        ground(p.x, p.z)
    };
    let (cx, hw, ez) = (entry.edge.x, entry.half_w, entry.edge.z);
    let top = pos.y + entry.edge.y;
    // lowest terrain across the width of the steps, `d` metres beyond the landing's edge
    let lowest = |d: f32| [-hw, 0.0, hw].iter().map(|dx| height_at(cx + dx, ez + d)).fold(f32::MAX, f32::min);
    if top - lowest(0.3) < STEP_UP {
        return;
    }
    // a gentle climb (about 0.42 m per metre); longer while the ground keeps falling away
    const RISE_PER_M: f32 = 0.42;
    let mut len = ((top - lowest(0.3)) / RISE_PER_M).max(1.2);
    for _ in 0..8 {
        let need = ((top - lowest(len)) / RISE_PER_M).clamp(1.2, 10.0);
        if need <= len + 0.05 {
            break;
        }
        len = need;
    }
    // the stair stands on the lowest ground under its whole run, so nothing shows underneath
    let runs = (len / 0.5).ceil().max(1.0) as i32;
    let base_w = (0..=runs).map(|k| lowest(len * k as f32 / runs as f32)).fold(f32::MAX, f32::min).min(top - 0.3) - 0.05;
    let (top_l, base_l) = (entry.edge.y, base_w - pos.y);
    let n = (((top_l - base_l) / 0.19).ceil() as i32).clamp(2, 14);
    let mut mb = MeshBuilder::new();
    mb.mat(mat::STONE).hex(0xa8a7a2);
    for j in 0..n {
        let (z0, z1) = (ez + len * j as f32 / n as f32, ez + len * (j + 1) as f32 / n as f32);
        // the step's tread sits at the height of the walkable slope at its middle
        let y = top_l - (j as f32 + 0.5) * (top_l - base_l) / n as f32;
        mb.box_min_max(Vec3::new(cx - hw, base_l - 0.15, z0), Vec3::new(cx + hw, y, z1));
    }
    placed.mesh.append(&mb.finish(), m);
    // the slope rises toward -Z (toward the door) in the building's frame
    let (lo, hi) = (Vec3::new(cx - hw, base_l, ez), Vec3::new(cx + hw, top_l, ez + len));
    let (a, b) = (m.transform_point3(lo), m.transform_point3(hi));
    let (min, max) = (a.min(b), a.max(b));
    placed.cols.push(Collider::wedge(min, max, rot_dir(3, rot), tag));
    placed.steps = Some(Aabb::new(min, max));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_geoms() -> Vec<(&'static str, Geom)> {
        let mut r = Rng::new(5);
        let st = random_style(&mut r, 2);
        vec![
            ("house1", gen_house(&mut r, 9.0, 8.0, &Style { floors: 1, ..st })),
            ("house2", gen_house(&mut r, 10.0, 11.0, &st)),
            ("cabin", gen_cabin(&mut r, 8.0, 7.0)),
            ("shop", gen_shop(&mut r, 11.0, 8.0)),
            ("barn", gen_barn(&mut r)),
            ("windmill", gen_windmill()),
            ("lighthouse", gen_lighthouse()),
            ("tower", gen_water_tower()),
            ("silo", gen_silo()),
            ("gas", gen_gas_canopy()),
            ("fountain", gen_fountain()),
            ("well", gen_well()),
            ("lamp", gen_lamp_post(0xfff0b0)),
            ("dock", gen_dock(12.0)),
            ("fence", gen_fence(6.0, 0xf5f2ea)),
        ]
    }

    #[test]
    fn every_generator_produces_geometry() {
        for (name, g) in all_geoms() {
            assert!(!g.mesh.is_empty(), "{name} has no mesh");
            assert!(g.mesh.verts.iter().all(|v| v.pos.iter().all(|c| c.is_finite())), "{name} NaN");
            assert!(g.mesh.verts.len() < 20_000, "{name}: {} verts", g.mesh.verts.len());
            if name.starts_with("house") || name == "cabin" || name == "shop" || name == "barn" {
                assert!(g.mesh.verts.len() < 13_000, "{name}: {} verts: keep a building cheap, hundreds are drawn from one chunk mesh", g.mesh.verts.len());
            }
            assert!(!g.cols.is_empty() || name == "fence", "{name} has no colliders");
            let bb = g.mesh.bounds();
            assert!(bb.max.y > if name == "dock" || name == "fence" { 0.15 } else { 1.0 }, "{name} too short");
        }
    }

    #[test]
    fn dressing_stays_close_to_the_walls() {
        // the bots decide whether a point is "inside" a building from its mesh bounds minus the eaves, so gutters,
        // mailboxes and stands must not reach far past the roof, the porch or the awning
        for (name, g) in all_geoms() {
            if !(name.starts_with("house") || name == "cabin" || name == "shop" || name == "barn") {
                continue;
            }
            let bb = g.mesh.bounds();
            assert!(bb.min.x >= -g.half.x - 0.9 && bb.max.x <= g.half.x + 0.9, "{name}: x {:?}", (bb.min.x, bb.max.x));
            assert!(bb.min.z >= -g.half.y - 0.9 && bb.max.z <= g.half.y + 2.85, "{name}: z {:?}", (bb.min.z, bb.max.z));
        }
    }

    #[test]
    fn walls_have_door_gap_and_are_solid_elsewhere() {
        let mut r = Rng::new(11);
        let st = Style { floors: 1, porch: false, ..random_style(&mut r, 1) };
        let g = gen_house(&mut r, 9.0, 8.0, &st);
        // Walk a vertical ray through the door position at chest height: should be free.
        let o = Vec3::new(g.door_out.x, 1.2, g.door_out.z);
        let dir = Vec3::new(0.0, 0.0, -1.0);
        let mut hit_any = false;
        for c in &g.cols {
            if let Some((t, _)) = c.shape.raycast(o, dir, 12.0) {
                // the first hit must be beyond the front wall plane (inside the house)
                if t < 2.0 {
                    hit_any = true;
                }
            }
        }
        assert!(!hit_any, "door opening must be clear at chest height");
        // Through a solid part of the front wall, it must hit.
        let solid_x = if g.door_out.x > 0.0 { -g.half.x + 0.5 } else { g.half.x - 0.5 };
        let o2 = Vec3::new(solid_x, 1.2, 12.0);
        let hit = g.cols.iter().filter_map(|c| c.shape.raycast(o2, dir, 30.0)).map(|h| h.0).fold(f32::MAX, f32::min);
        assert!(hit < 30.0, "front wall must block a ray away from the door");
    }

    #[test]
    fn placement_rotates_everything_consistently() {
        let mut r = Rng::new(3);
        let st = random_style(&mut r, 1);
        let g = gen_house(&mut r, 9.0, 8.0, &st);
        for rot in 0..4u8 {
            let p = place(&g, Vec3::new(100.0, 5.0, -40.0), rot, Tag::Building(7));
            // door_out must be outside the footprint along the rotated front direction
            let c = Vec3::new(100.0, 5.0, -40.0);
            let d = p.door_out - c;
            let fwd = Mat3::from_rotation_y(rot as f32 * FRAC_PI_2) * Vec3::Z;
            assert!(d.dot(fwd) > g.half.y, "rot {rot}: door not on front side");
            // all collider boxes inside (aabb + margin)
            let bb = p.aabb.expanded(0.5);
            for col in &p.cols {
                let cb = col.shape.aabb();
                assert!(bb.contains(cb.min) && bb.contains(cb.max), "rot {rot}: collider outside the building aabb");
                assert_eq!(col.tag, Tag::Building(7));
            }
            // loot spots lie within the footprint
            for l in &p.loot {
                assert!((l.x - c.x).abs() < p.footprint_half.x + 0.1 && (l.z - c.z).abs() < p.footprint_half.y + 0.1, "rot {rot}: loot outside");
            }
        }
    }

    #[test]
    fn rot_dir_cycles() {
        assert_eq!(rot_dir(0, 1), 3);
        assert_eq!(rot_dir(3, 1), 2);
        assert_eq!(rot_dir(0, 4), 0);
        // consistent with rotating the vector
        for d in 0..4u8 {
            for k in 0..4u8 {
                let v = [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z][d as usize];
                let r = Mat3::from_rotation_y(k as f32 * FRAC_PI_2) * v;
                let nd = rot_dir(d, k);
                let nv = [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z][nd as usize];
                assert!((r - nv).length() < 1e-4, "d{d} k{k}: {r:?} vs {nv:?}");
            }
        }
    }
}
