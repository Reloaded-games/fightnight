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
    /// The flight of stairs up to the second floor, if there is one (local bounds; it rises toward -Z).
    pub stairs: Option<(Vec3, Vec3)>,
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
    /// The point `u` along the wall, `d` across it (from the centre line) and at height `y`.
    fn at(&self, u: f32, d: f32, y: f32) -> Vec3 {
        if self.along_x {
            Vec3::new(self.a0 + u, y, self.c + d)
        } else {
            Vec3::new(self.c + d, y, self.a0 + u)
        }
    }
    /// The outward direction of the wall.
    fn out_dir(&self) -> Vec3 {
        if self.along_x {
            Vec3::new(0.0, 0.0, self.out)
        } else {
            Vec3::new(self.out, 0.0, 0.0)
        }
    }
    /// The across range (from the centre line, low to high) of something that stands `d0`..`d1` proud of the outer face.
    fn proud(&self, d0: f32, d1: f32) -> (f32, f32) {
        let f = WALL_T / 2.0;
        if self.out > 0.0 {
            (f + d0, f + d1)
        } else {
            (-f - d1, -f - d0)
        }
    }
    /// The faces of a box set against the outer face that can be seen: all but its back.
    fn outer_faces(&self) -> u8 {
        match (self.along_x, self.out > 0.0) {
            (true, true) => 0b011111,
            (true, false) => 0b101111,
            (false, true) => 0b111101,
            (false, false) => 0b111110,
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

/// The colour `a` taken a fraction `t` of the way to `b`.
fn mix_hex(a: u32, b: u32, t: f32) -> u32 {
    let c = |sh: u32| {
        let (x, y) = (((a >> sh) & 255) as f32, ((b >> sh) & 255) as f32);
        (x + (y - x) * t).round().clamp(0.0, 255.0) as u32
    };
    (c(16) << 16) | (c(8) << 8) | c(0)
}

// ---------------------------------------------------------------------------------
// Facade dressing: mesh only, never a collider, and never a random number (what varies follows from sizes and colours already drawn)

/// A band `out` proud of the outer face of the wall `l` between the heights `y0 + v0` and `y0 + v1`, broken where an opening of `ops`
/// crosses it. It runs on round the corners: a front or back band past the ends of its wall, a side band up to the faces of the front and back.
fn wall_band(b: &mut B, l: Line, y0: f32, v0: f32, v1: f32, ops: &[Op], out: f32, col: u32, m: u8) {
    let ext = if l.along_x { out } else { WALL_T };
    let mut cuts: Vec<(f32, f32)> = ops.iter().filter(|o| o.v0 < v1 && o.v1 > v0).map(|o| (o.u0, o.u1)).collect();
    cuts.sort_by(|a, c| a.0.total_cmp(&c.0));
    let (d0, d1) = l.proud(0.0, out);
    let mut u = -ext;
    let mut spans = vec![];
    for (c0, c1) in cuts {
        if c0 > u + 1e-3 {
            spans.push((u, c0));
        }
        u = u.max(c1);
    }
    if l.len() + ext > u + 1e-3 {
        spans.push((u, l.len() + ext));
    }
    for (u0, u1) in spans {
        let (a, c) = l.bx(u0, u1, d0, d1, y0 + v0, y0 + v1);
        b.bx_faces(a, c, col, m, l.outer_faces(), false);
    }
}

/// A flat pilaster `w` wide centred `u` along the wall `l`, standing `out` proud of its outer face from `y_lo` to `y_hi`.
fn pilaster(b: &mut B, l: Line, u: f32, w: f32, y_lo: f32, y_hi: f32, out: f32, col: u32, m: u8) {
    let (d0, d1) = l.proud(0.0, out);
    let (a, c) = l.bx(u - w / 2.0, u + w / 2.0, d0, d1, y_lo, y_hi);
    b.bx_faces(a, c, col, m, l.outer_faces(), false);
}

/// A striped shop awning on the outer face of the wall `l` over `u0`..`u1`: cloth from `y_top` at the wall sloping `drop` down to
/// `reach` out, a valance along its front edge, cheeks at its ends and two stays back to the wall.
fn awning(b: &mut B, l: Line, u0: f32, u1: f32, y_top: f32, drop: f32, reach: f32, col: u32) {
    let f = WALL_T / 2.0;
    let o = l.out_dir();
    let n = o * drop + Vec3::Y * reach;
    let back = |u: f32| l.at(u, l.out * (f + 0.01), y_top);
    let front = |u: f32, dy: f32| l.at(u, l.out * (f + reach), y_top - drop - dy);
    let stripes = ((u1 - u0) / 0.42).round().max(2.0) as i32;
    b.mb.mat(mat::CLOTH);
    for k in 0..stripes {
        let (ua, ub) = (u0 + (u1 - u0) * k as f32 / stripes as f32, u0 + (u1 - u0) * (k + 1) as f32 / stripes as f32);
        b.mb.hex(if k % 2 == 0 { col } else { 0xf7f2e6 });
        let p = [back(ua), back(ub), front(ub, 0.0), front(ua, 0.0)];
        b.mb.quad_out(p, n);
        b.mb.quad_out(p, -n);
        let v = [front(ua, 0.0), front(ub, 0.0), front(ub, 0.2), front(ua, 0.2)];
        b.mb.quad_out(v, o);
        b.mb.quad_out(v, -o);
    }
    b.mb.hex(col);
    let along = if l.along_x { Vec3::X } else { Vec3::Z };
    for (u, s) in [(u0, -1.0f32), (u1, 1.0)] {
        b.mb.tri_out(back(u), front(u, 0.0), l.at(u, l.out * (f + 0.01), y_top - drop), along * s);
        b.mb.tri_out(back(u), front(u, 0.0), l.at(u, l.out * (f + 0.01), y_top - drop), -along * s);
        b.tube(l.at(u + 0.08 * -s, l.out * (f + reach - 0.05), y_top - drop + 0.03), l.at(u + 0.08 * -s, l.out * f, y_top - drop - 0.35), 0.018, 0x2b2e34, mat::METAL);
    }
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
        Geom { mesh: self.mb.finish(), cols: self.cols, loot: self.loot, chests: self.chests, door_out, door_in, half, height, hub: None, entry: None, stairs: None }
    }
}

// ---------------------------------------------------------------------------------
// Stairs

/// Steps in the flight between the two floors of a house (0.19 m rise, 0.35 m run).
const STAIR_STEPS: i32 = 16;
/// Length of the top of the flight that stays open to the upper floor.
const STAIR_LANDING: f32 = 1.2;
/// Height of a handrail above the nose of the steps.
const STAIR_RAIL: f32 = 0.9;

/// One step of a flight: a solid block from the base of the flight up to its tread.
struct Step {
    min: Vec3,
    max: Vec3,
}

/// A straight flight of `n` steps over `x0..x1` that rises toward -Z, from its foot at `z_bot` to its head at `z_top`. The first tread
/// is one rise above `y_low` and the last is level with `y_high`; every step is a solid block down to `y_base`, in order from the foot.
fn flight_steps(x0: f32, x1: f32, z_top: f32, z_bot: f32, y_base: f32, y_low: f32, y_high: f32, n: i32) -> Vec<Step> {
    let run = (z_bot - z_top) / n as f32;
    (0..n)
        .map(|k| {
            let y = y_low + (y_high - y_low) * (k + 1) as f32 / n as f32;
            Step { min: Vec3::new(x0, y_base, z_bot - (k + 1) as f32 * run), max: Vec3::new(x1, y, z_bot - k as f32 * run) }
        })
        .collect()
}

/// Mesh for a flight: a riser block under each tread, and a thin tread board that overhangs the step below it.
fn flight_mesh(mb: &mut MeshBuilder, steps: &[Step], riser: u32, tread: u32, m: u8) {
    const BOARD: f32 = 0.05;
    for s in steps {
        mb.mat(m).hex(riser).ao(0.8, 1.0);
        mb.box_min_max(s.min, Vec3::new(s.max.x, s.max.y - BOARD, s.max.z));
        mb.hex(tread).ao(1.0, 1.0);
        mb.box_min_max(Vec3::new(s.min.x, s.max.y - BOARD, s.min.z), Vec3::new(s.max.x, s.max.y, s.max.z + BOARD));
    }
}

/// A guard rail standing on a floor along the straight line `a`..`c` (posts, a top and a middle bar). It stops people walking
/// off the edge but not bullets.
fn guard_rail(b: &mut B, a: Vec3, c: Vec3, col: u32) {
    const H: f32 = 1.0;
    let len = a.distance(c);
    let n = (len / 1.0).ceil().max(1.0) as i32;
    b.mb.mat(mat::WOOD).hex(col);
    for k in 0..=n {
        let p = a.lerp(c, k as f32 / n as f32);
        b.mb.box_min_max(Vec3::new(p.x - 0.04, a.y, p.z - 0.04), Vec3::new(p.x + 0.04, a.y + H, p.z + 0.04));
    }
    let (lo, hi) = (a.min(c), a.max(c));
    for (y0, y1, t) in [(a.y + H - 0.07, a.y + H + 0.02, 0.045), (a.y + 0.45, a.y + 0.5, 0.025)] {
        b.mb.box_min_max(Vec3::new(lo.x - t, y0, lo.z - t), Vec3::new(hi.x + t, y1, hi.z + t));
    }
    let mut col = Collider::aabb_box(Vec3::new(lo.x - 0.04, a.y + 0.3, lo.z - 0.04), Vec3::new(hi.x + 0.04, a.y + H, hi.z + 0.04), Tag::Static);
    col.blocks_bullets = false;
    b.cols.push(col);
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
        // (four boards around the outside: as one box it would cover the whole upper floor)
        let y = FLOOR_Y + STORY_H;
        let (y0, y1) = (y - 0.12, y + 0.12);
        b.bx(Vec3::new(-hx - 0.06, y0, hz), Vec3::new(hx + 0.06, y1, hz + 0.06), st.trim, mat::FLAT, false);
        b.bx(Vec3::new(-hx - 0.06, y0, -hz - 0.06), Vec3::new(hx + 0.06, y1, -hz), st.trim, mat::FLAT, false);
        b.bx(Vec3::new(-hx - 0.06, y0, -hz), Vec3::new(-hx, y1, hz), st.trim, mat::FLAT, false);
        b.bx(Vec3::new(hx, y0, -hz), Vec3::new(hx + 0.06, y1, hz), st.trim, mat::FLAT, false);
    }

    // --- stairs + upper floor
    let mut stairs_rect = None;
    let mut stairs = None;
    if floors == 2 {
        let l = 5.6f32;
        let x0 = -hx + WALL_T + 0.15;
        let (z_t, z_b) = (-hz + WALL_T + 0.1, -hz + WALL_T + 0.1 + l);
        let up_y = FLOOR_Y + STORY_H;
        let (ix0, ix1, iz0, iz1) = (-hx + WALL_T, hx - WALL_T, -hz + WALL_T, hz - WALL_T);
        // a real flight: one solid block per step, wall to wall on the left, open (with a handrail) on the right
        let sx1 = x0 + 1.3;
        let steps = flight_steps(ix0, sx1, z_t, z_b, FLOOR_Y, FLOOR_Y, up_y, STAIR_STEPS);
        flight_mesh(&mut b.mb, &steps, 0x8f6a3e, 0xb98b56, mat::WOOD);
        b.cols.extend(steps.iter().map(|s| Collider::aabb_box(s.min, s.max, Tag::Static)));
        stairs_rect = Some((x0, sx1, z_t, z_b));
        stairs = Some((Vec3::new(ix0, FLOOR_Y, z_t), Vec3::new(sx1, up_y, z_b)));
        // the upper floor stops short of the top of the flight (you step off sideways) and covers its foot
        let hole_z0 = z_t + STAIR_LANDING;
        let hole_z1 = z_b - 1.0;
        // handrail on the open side, from the foot up to where the stairwell guard takes over
        let rail_x = sx1 - 0.06;
        let (run, rise) = (l / STAIR_STEPS as f32, STORY_H / STAIR_STEPS as f32);
        let nose = |z: f32| FLOOR_Y + rise + (z_b - z) * rise / run;
        b.mb.mat(mat::WOOD).hex(0x6e4a2c);
        for k in (0..STAIR_STEPS).step_by(2) {
            let z = z_b - (k as f32 + 0.5) * run;
            if z < hole_z0 {
                break;
            }
            let y = FLOOR_Y + (k + 1) as f32 * rise;
            b.mb.box_min_max(Vec3::new(rail_x - 0.025, y, z - 0.025), Vec3::new(rail_x + 0.025, nose(z) + STAIR_RAIL, z + 0.025));
        }
        let (za, zb) = (z_b - 0.12, hole_z0);
        let slope = ((nose(zb) - nose(za)) / (za - zb)).atan();
        let len = ((za - zb).powi(2) + (nose(zb) - nose(za)).powi(2)).sqrt();
        b.mb.hex(0x7a5232);
        b.mb.with_xf(Mat4::from_translation(Vec3::new(rail_x, nose(za) + STAIR_RAIL, za)) * Mat4::from_rotation_x(slope), |mb| {
            mb.box_min_max(Vec3::new(-0.04, -0.025, -len), Vec3::new(0.04, 0.035, 0.0));
        });
        for z in [za - 0.04, zb + 0.04] {
            b.mb.box_min_max(Vec3::new(rail_x - 0.045, nose(z) - 0.1, z - 0.045), Vec3::new(rail_x + 0.045, nose(z) + STAIR_RAIL + 0.1, z + 0.045));
        }
        // upper slab with a hole above the stairs
        let slab_y0 = up_y - 0.3;
        let pieces = [(sx1, ix1, iz0, iz1), (ix0, sx1, hole_z1, iz1)];
        for (a0, a1, c0, c1) in pieces {
            if a1 - a0 > 0.05 && c1 - c0 > 0.05 {
                b.bx(Vec3::new(a0, slab_y0, c0), Vec3::new(a1, up_y, c1), 0xb98b56, mat::WOOD, true);
            }
        }
        // guard rails around the stairwell: along its open side and across its far end, with the way on at the top
        guard_rail(&mut b, Vec3::new(sx1 + 0.05, up_y, hole_z0), Vec3::new(sx1 + 0.05, up_y, hole_z1 + 0.05), 0x7a5232);
        guard_rail(&mut b, Vec3::new(sx1 + 0.05, up_y, hole_z1 + 0.05), Vec3::new(ix0, up_y, hole_z1 + 0.05), 0x7a5232);
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
    g.stairs = stairs;
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
    // a tiled plinth round the walls (under the shop windows, broken at the door) and pilasters up the corners to the parapet
    let door_cut = [Op { u0: ops[0].u0 - 0.12, u1: ops[0].u1 + 0.12, ..ops[0] }];
    for (line, cut) in [(front, &door_cut[..]), (back, &[][..]), (left, &[][..]), (right, &[][..])] {
        wall_band(&mut b, line, FLOOR_Y, 0.0, 0.55, cut, 0.05, shade_hex(accent, 0.72), mat::STONE);
    }
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let (x0, x1) = if sx > 0.0 { (hx - 0.24, hx + 0.08) } else { (-hx - 0.08, -hx + 0.24) };
        let (z0, z1) = if sz > 0.0 { (hz - 0.24, hz + 0.08) } else { (-hz - 0.08, -hz + 0.24) };
        b.bx_faces(Vec3::new(x0, FLOOR_Y, z0), Vec3::new(x1, top_y, z1), 0xf7f5ee, mat::PLASTER, 0b110011, false);
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
    // barn doors slid open either side of the opening (visual only): red boards in a white frame with a rail across the middle and
    // an X brace in each half
    for sx in [-1.0f32, 1.0] {
        let x0 = door_u - 1.8 + (if sx < 0.0 { -1.8 } else { 3.6 });
        let (f, y0, y1, w) = (WALL_T / 2.0, FLOOR_Y, FLOOR_Y + 3.7, 0.15);
        let (x1, ym) = (x0 + 1.8, (y0 + y1) / 2.0);
        let (a, c) = front.bx(x0, x1, f, f + 0.05, y0, y1);
        b.bx(a, c, shade_hex(red, 0.9), mat::WOOD, false);
        for (u0, u1, v0, v1) in [(x0, x0 + w, y0, y1), (x1 - w, x1, y0, y1), (x0 + w, x1 - w, y0, y0 + w), (x0 + w, x1 - w, y1 - w, y1), (x0 + w, x1 - w, ym - w / 2.0, ym + w / 2.0)] {
            let (a, c) = front.bx(u0, u1, f + 0.05, f + 0.09, v0, v1);
            b.bx_faces(a, c, 0xf5efe0, mat::WOOD, front.outer_faces(), false);
        }
        for (ya, yb) in [(y0 + w, ym - w / 2.0), (ym + w / 2.0, y1 - w)] {
            for (ua, ub) in [(x0 + w, x1 - w), (x1 - w, x0 + w)] {
                b.board(front.at(ua, f + 0.07, ya), front.at(ub, f + 0.07, yb), Vec3::Z, 0.13, 0.04, 0xf5efe0, mat::WOOD);
            }
        }
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
    // a handrail round the tops of the gallery's posts
    for k in 0..12 {
        let (a0, a1) = (k as f32 / 12.0 * TAU_F, (k + 1) as f32 / 12.0 * TAU_F);
        let (p0, p1) = (Vec3::new(a0.cos() * 2.45, total + 1.3, a0.sin() * 2.45), Vec3::new(a1.cos() * 2.45, total + 1.3, a1.sin() * 2.45));
        let mid = (p0 + p1) * 0.5;
        b.board(p0, p1, Vec3::new(mid.x, 0.0, mid.z), 0.07, 0.07, 0x3d3f46, mat::METAL);
    }
    // lamp room (emissive) behind glazing bars, on a dark base ring
    b.mb.mat(mat::EMISSIVE).hex(0xfff2b0);
    b.mb.cylinder(Vec3::new(0.0, total + 0.35, 0.0), 1.5, 1.5, 2.2, 10, false, false);
    b.mb.mat(mat::METAL).hex(0x3d3f46);
    b.mb.cylinder(Vec3::new(0.0, total + 0.35, 0.0), 1.56, 1.56, 0.32, 10, false, false);
    for k in 0..10 {
        let a = k as f32 / 10.0 * TAU_F;
        let p = Vec3::new(a.cos() * 1.5, 0.0, a.sin() * 1.5);
        b.board(p + Vec3::Y * (total + 0.6), p + Vec3::Y * (total + 2.55), p, 0.07, 0.08, 0x3d3f46, mat::METAL);
    }
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
// City towers
// ---------------------------------------------------------------------------------

/// Width of each of the two side-by-side lanes of a tower's switchback stair.
const TOWER_LANE: f32 = 1.3;
/// Thickness of the spine wall between the two lanes.
const TOWER_SPINE: f32 = 0.2;
/// Depth of the landing between a tower's back wall and the back end of its flights.
const TOWER_LANDING: f32 = 1.2;
/// Run of one step of a tower's flights (16 of them climb a storey in 5.6 m).
const TOWER_RUN: f32 = 0.35;
/// Thickness of a tower's floor slabs and of its roof.
const SLAB_T: f32 = 0.3;
/// Steps at the foot of a flight that stay under the slab it climbs to: under a 0.3 m slab the second step still has 2.3 m of
/// head room, the third would have less than 2.2 m.
const COVERED_STEPS: i32 = 2;
/// Height of the parapet round a tower's flat roof.
const PARAPET_H: f32 = 0.9;

/// One flight of a tower's stair, in the tower's frame: it fills its lane from `z_back` to `z_front` and climbs a storey, from the
/// floor at `y_low` to the one at `y_high`, toward the back (-Z) when `to_back`, toward the front otherwise.
#[derive(Clone, Copy, Debug)]
struct Flight {
    x0: f32,
    x1: f32,
    z_back: f32,
    z_front: f32,
    y_low: f32,
    y_high: f32,
    to_back: bool,
}

impl Flight {
    fn run(&self) -> f32 {
        (self.z_front - self.z_back) / STAIR_STEPS as f32
    }

    /// The opening (x0, x1, z0, z1) this flight needs in the slab it arrives at: all of its length but the steps at its foot.
    fn hole(&self) -> (f32, f32, f32, f32) {
        let covered = COVERED_STEPS as f32 * self.run();
        if self.to_back {
            (self.x0, self.x1, self.z_back, self.z_front - covered)
        } else {
            (self.x0, self.x1, self.z_back + covered, self.z_front)
        }
    }

    /// Its step blocks, from the foot up.
    fn steps(&self) -> Vec<Step> {
        let steps = flight_steps(self.x0, self.x1, self.z_back, self.z_front, self.y_low, self.y_low, self.y_high, STAIR_STEPS);
        if self.to_back {
            return steps;
        }
        // the same flight turned round to climb toward +Z
        let m = self.z_back + self.z_front;
        steps.into_iter().map(|s| Step { min: Vec3::new(s.min.x, s.min.y, m - s.max.z), max: Vec3::new(s.max.x, s.max.y, m - s.min.z) }).collect()
    }
}

/// The stair of a tower of `floors` storeys on a `hx` x `hz` half footprint: one flight per storey, from the ground floor up to the
/// roof. The flights alternate between a lane along the left wall, climbing toward the back, and the lane beside it (behind a
/// spine wall), climbing toward the front, so each one starts on the landing where the one below it ends.
fn tower_flights(hx: f32, hz: f32, floors: u32) -> Vec<Flight> {
    let x0 = -hx + WALL_T;
    let z_back = -hz + WALL_T + TOWER_LANDING;
    let z_front = z_back + STAIR_STEPS as f32 * TOWER_RUN;
    (0..floors)
        .map(|i| {
            let x = if i % 2 == 0 { x0 } else { x0 + TOWER_LANE + TOWER_SPINE };
            let y_low = FLOOR_Y + i as f32 * STORY_H;
            Flight { x0: x, x1: x + TOWER_LANE, z_back, z_front, y_low, y_high: y_low + STORY_H, to_back: i % 2 == 0 }
        })
        .collect()
}

/// Like [`flight_mesh`], but without the faces nobody sees (the undersides on the floor, and the backs that the next step up hides), for
/// buildings with a stair on every storey.
fn flight_mesh_lean(mb: &mut MeshBuilder, steps: &[Step], riser: u32, tread: u32, m: u8) {
    const BOARD: f32 = 0.05;
    for (k, s) in steps.iter().enumerate() {
        let back = if k + 1 == steps.len() { 0b100000 } else { 0 };
        mb.mat(m).hex(riser).ao(0.8, 1.0);
        mb.box_faces(s.min, Vec3::new(s.max.x, s.max.y - BOARD, s.max.z), 0b010011 | back);
        mb.hex(tread).ao(1.0, 1.0);
        mb.box_faces(Vec3::new(s.min.x, s.max.y - BOARD, s.min.z), Vec3::new(s.max.x, s.max.y, s.max.z + BOARD), 0b011111 | back);
    }
}

/// Emit one flight of a tower's stair: a solid block per step, and the steps' mesh (turned half a turn for a flight that climbs
/// toward the front, so the nosing of every tread still overhangs the step below it).
fn tower_flight(b: &mut B, f: &Flight, riser: u32, tread: u32) {
    let steps = flight_steps(f.x0, f.x1, f.z_back, f.z_front, f.y_low, f.y_low, f.y_high, STAIR_STEPS);
    let c = Vec3::new((f.x0 + f.x1) / 2.0, 0.0, (f.z_back + f.z_front) / 2.0);
    let turn = if f.to_back { Mat4::IDENTITY } else { Mat4::from_translation(c) * Mat4::from_rotation_y(PI) * Mat4::from_translation(-c) };
    b.mb.push_xf(turn);
    flight_mesh_lean(&mut b.mb, &steps, riser, tread, mat::STONE);
    b.mb.pop_xf();
    b.cols.extend(f.steps().iter().map(|s| Collider::aabb_box(s.min, s.max, Tag::Static)));
}

/// A handrail on posts up the open side of a flight, at `x` (mesh only, as in the houses).
fn stair_rail(b: &mut B, x: f32, f: &Flight, col: u32) {
    let rise = (f.y_high - f.y_low) / STAIR_STEPS as f32;
    let (foot, dz) = if f.to_back { (f.z_front, -1.0) } else { (f.z_back, 1.0) };
    let at = |k: f32| Vec3::new(x, f.y_low + (k + 1.0) * rise + STAIR_RAIL, foot + dz * (k + 0.5) * f.run());
    b.mb.mat(mat::METAL).hex(col);
    for k in (0..STAIR_STEPS).step_by(3) {
        let top = at(k as f32);
        b.mb.box_min_max(Vec3::new(x - 0.025, top.y - STAIR_RAIL, top.z - 0.025), Vec3::new(x + 0.025, top.y, top.z + 0.025));
    }
    b.tube(at(0.0), at((STAIR_STEPS - 1) as f32), 0.03, col, mat::METAL);
}

/// [`B::wall`] for a building of many storeys: the same boxes and colliders, without their top and bottom faces (those butt against the
/// storeys above and below, or lie under the sills and lintels of the openings).
fn lean_wall(b: &mut B, l: Line, y0: f32, h: f32, ops: &[Op], col: u32, m: u8) {
    b.mb.ao(0.82, 1.0);
    for (u0, u1, v0, v1) in wall_rects(l.len(), h, ops) {
        let (a, c) = l.bx(u0, u1, -WALL_T / 2.0, WALL_T / 2.0, y0 + v0, y0 + v1);
        b.bx_faces(a, c, col, m, 0b110011, true);
    }
    b.mb.ao(1.0, 1.0);
}

/// The frame of a city window: a sill, a lintel, a mullion and a transom (mesh only: the opening stays open to shoot and climb through).
fn tower_window(b: &mut B, l: Line, y0: f32, o: &Op, trim: u32) {
    let t = WALL_T / 2.0 + 0.04;
    let (s0, s1) = if l.out > 0.0 { (-t, t + 0.06) } else { (-t - 0.06, t) };
    let (a, c) = l.bx(o.u0 - 0.12, o.u1 + 0.12, s0, s1, y0 + o.v0 - 0.1, y0 + o.v0);
    b.bx(a, c, trim, mat::FLAT, false);
    let (a, c) = l.bx(o.u0 - 0.12, o.u1 + 0.12, -t, t, y0 + o.v1, y0 + o.v1 + 0.16);
    b.bx(a, c, trim, mat::FLAT, false);
    // flat jambs on the outer face from the sill up to the lintel, which close the frame (3.5 cm proud: only their fronts are worth
    // drawing, a building has dozens of windows)
    let (d0, d1) = l.proud(0.0, 0.035);
    let front = l.outer_faces() & !(if l.along_x { 0b001111 } else { 0b111100 });
    for (u0, u1) in [(o.u0 - 0.1, o.u0), (o.u1, o.u1 + 0.1)] {
        let (a, c) = l.bx(u0, u1, d0, d1, y0 + o.v0, y0 + o.v1);
        b.bx_faces(a, c, trim, mat::FLAT, front, false);
    }
    // (the bars leave out the end faces that butt against the sill, the lintel and the jambs)
    let um = (o.u0 + o.u1) * 0.5;
    let (a, c) = l.bx(um - 0.03, um + 0.03, -0.03, 0.03, y0 + o.v0, y0 + o.v1);
    b.bx_faces(a, c, trim, mat::FLAT, 0b110011, false);
    let ym = y0 + o.v0 + (o.v1 - o.v0) * 0.66;
    let (a, c) = l.bx(o.u0, o.u1, -0.03, 0.03, ym - 0.025, ym + 0.025);
    b.bx_faces(a, c, trim, mat::FLAT, if l.along_x { 0b111100 } else { 0b001111 }, false);
}

/// A low chest of drawers whose back is its local +Z side, with its collider.
fn cabinet(b: &mut B, c: Vec3, yaw: f32, col: u32) {
    b.mb.push_xf(Mat4::from_translation(c) * Mat4::from_rotation_y(yaw));
    b.mb.mat(mat::WOOD).hex(col);
    b.mb.box_min_max(Vec3::new(-0.8, 0.0, -0.25), Vec3::new(0.8, 0.95, 0.25));
    b.mb.hex(shade_hex(col, 0.8));
    for k in 0..3 {
        let y = 0.1 + k as f32 * 0.28;
        b.mb.box_min_max(Vec3::new(-0.72, y, -0.27), Vec3::new(0.72, y + 0.22, -0.25));
    }
    b.mb.pop_xf();
    let (a, bb) = rot_box(Vec3::new(-0.8, 0.0, -0.27), Vec3::new(0.8, 0.95, 0.25), yaw, c);
    b.cols.push(Collider::aabb_box(a, bb, Tag::Static));
}

/// Dress one floor of a tower's rooms (`room` = x0, x1, z0, z1, the floor at height `y`): a sofa against the right-hand wall, a cabinet
/// against the back wall, a rug and a ceiling lamp; a chest when `chest`; and one or two loot spots on clear floor. On the ground floor
/// `door_x` keeps the way in from the front door (`hz` is the half depth) free.
fn tower_room(b: &mut B, rng: &mut Rng, y: f32, room: (f32, f32, f32, f32), door_x: Option<f32>, chest: bool, hz: f32) {
    let (x0, x1, z0, z1) = room;
    let in_lane = |x: f32, z: f32, r: f32| door_x.is_some_and(|dx| (x - dx).abs() < 1.5 + r && z > hz - 3.2);
    let mid = Vec3::new((x0 + x1) * 0.5, y, (z0 + z1) * 0.5);
    rug(b, mid, (x1 - x0 - 1.6).min(3.0), (z1 - z0).min(2.6) * 0.8, *rng.pick(&[0xc0583e, 0x3f78a8, 0x5b8f4a, 0xb88a3b, 0x6b5a8f]));
    let ceiling = y + STORY_H - SLAB_T;
    b.bx(Vec3::new(mid.x - 0.45, ceiling - 0.04, mid.z - 0.45), Vec3::new(mid.x + 0.45, ceiling, mid.z + 0.45), 0xfff3d0, mat::EMISSIVE, false);
    if rng.chance(0.75) {
        let z = rng.range(z0 + 1.3, z1 - 1.3);
        if !in_lane(x1 - 0.5, z, 1.0) {
            sofa(b, Vec3::new(x1 - 0.5, y, z), FRAC_PI_2, *rng.pick(&[0x6b8fb8, 0xb8706b, 0x7ba66f, 0xc9a35a, 0x5a5f6a]));
        }
    }
    if rng.chance(0.6) {
        // (clear of the sofa, which stands up to 0.95 m out from the right-hand wall)
        let x = rng.range(x0 + 0.9, x1 - 1.85);
        if !in_lane(x, z0 + 0.3, 0.8) {
            cabinet(b, Vec3::new(x, y, z0 + 0.3), PI, *rng.pick(&[0x8a5a34, 0xb98b56, 0xe9e4d8, 0x5a5f6a]));
        }
    }
    let mut chest_at = None;
    if chest {
        let cands = [(Vec3::new(x1 - 0.6, y, z0 + 0.5), 0.0f32), (Vec3::new(x1 - 0.5, y, (z0 + z1) * 0.5), -FRAC_PI_2), (Vec3::new(x1 - 0.6, y, z1 - 0.5), PI), (Vec3::new(x0 + 0.6, y, z1 - 0.5), PI)];
        let fits = |p: &Vec3, yaw: f32| {
            // clear of furniture over the chest's footprint plus the room it needs to open
            let (rx, rz) = if (yaw.abs() - FRAC_PI_2).abs() < 0.1 { (0.4, 0.55) } else { (0.55, 0.4) };
            !in_lane(p.x, p.z, rx) && floor_clear(b, p.x, p.z, y, rx, rz)
        };
        if let Some(&(p, yaw)) = cands.iter().find(|(p, yaw)| fits(p, *yaw)) {
            b.chests.push((p, yaw));
            chest_at = Some(p);
        }
    }
    let want = 1 + rng.below(2);
    let mut spots: Vec<Vec3> = vec![];
    for _ in 0..16 {
        if spots.len() >= want {
            break;
        }
        let p = Vec3::new(rng.range(x0 + 0.4, x1 - 0.5), y, rng.range(z0 + 0.6, z1 - 0.6));
        let ok = !in_lane(p.x, p.z, 0.4) && floor_clear(b, p.x, p.z, y, 0.3, 0.3) && chest_at.is_none_or(|c| c.distance(p) > 1.3) && spots.iter().all(|q| q.distance(p) > 1.5);
        if ok {
            spots.push(p);
        }
    }
    if spots.is_empty() {
        // the middle of the room is never furnished, and is far from the door lane and the chest
        spots.push(mid);
    }
    b.loot.extend(spots);
}

/// A city tower block in the manner of Tilted Towers: `floors` storeys (clamped to 2..=6) of brick or plaster (`st` gives the colours)
/// under a flat roof with a parapet, windows on every side of every floor, tall shop windows and the front door (+Z) on the ground floor.
/// Inside, a switchback stair climbs from the ground floor to a hatch in the roof: its flights alternate between two lanes along the left
/// wall, each slab is open only above the flight that arrives at it (with guard rails round the opening), and the flight that leaves it
/// stands on solid slab. Every floor has one or two loot spots, upper floors sometimes a chest, the roof one spot. `w` x `d` is the outer
/// footprint, at least 8.5 m each way (9 to 13 m suits it best).
pub fn gen_tower(rng: &mut Rng, w: f32, d: f32, floors: u32, st: &Style) -> Geom {
    let mut b = B::new();
    let (hx, hz) = (w.max(8.5) / 2.0, d.max(8.5) / 2.0);
    let floors = floors.clamp(2, 6);
    let roof_y = FLOOR_Y + floors as f32 * STORY_H;
    let (ix0, ix1, iz0, iz1) = (-hx + WALL_T, hx - WALL_T, -hz + WALL_T, hz - WALL_T);
    let flights = tower_flights(hx, hz, floors);
    let (lane_a, lane_b) = (flights[0], flights[1]);
    let (z_back, z_front) = (lane_a.z_back, lane_a.z_front);
    // the stairwell takes the strip along the left wall: the rooms, and the front door, are right of it
    let room_x0 = lane_b.x1 + 0.8;
    let (lo, hi) = (room_x0 + DOOR_W / 2.0, ix1 - DOOR_W / 2.0 - 0.4);
    let door_x = rng.pick(&[0.0f32, (room_x0 + ix1) / 2.0]).clamp(lo, hi.max(lo));
    let door_u = door_x + hx;
    let (floor_col, floor_mat) = *rng.pick(&[(0xb98b56u32, mat::WOOD), (0xcfc7b8, mat::STONE), (0x9c7a56, mat::WOOD), (0xbfc4c9, mat::STONE)]);
    let door_col = *rng.pick(&[0x2f4f7fu32, 0xb5463a, 0x3e6e42, 0x2b2e34, 0x8a5a34]);
    let rail = 0x3d4048;

    // --- foundation and ground floor
    b.mb.ao(0.55, 1.0);
    b.bx(Vec3::new(-hx - 0.1, -1.6, -hz - 0.1), Vec3::new(hx + 0.1, FLOOR_Y, hz + 0.1), 0x9b9a95, mat::STONE, true);
    b.mb.ao(1.0, 1.0);
    b.bx_faces(Vec3::new(ix0, FLOOR_Y, iz0), Vec3::new(ix1, FLOOR_Y + 0.02, iz1), floor_col, floor_mat, 0b000100, false);

    // --- outer walls, storey by storey
    let front = Line { along_x: true, a0: -hx, a1: hx, c: hz - WALL_T / 2.0, out: 1.0 };
    let back = Line { along_x: true, a0: -hx, a1: hx, c: -hz + WALL_T / 2.0, out: -1.0 };
    let left = Line { along_x: false, a0: -hz + WALL_T, a1: hz - WALL_T, c: -hx + WALL_T / 2.0, out: -1.0 };
    let right = Line { along_x: false, a0: -hz + WALL_T, a1: hz - WALL_T, c: hx - WALL_T / 2.0, out: 1.0 };
    let door = Op { u0: door_u - DOOR_W / 2.0, u1: door_u + DOOR_W / 2.0, v0: 0.0, v1: DOOR_H };
    // on the storeys whose flight climbs along the left wall, that wall has no windows where the treads stand higher than a sill (0.9 m):
    // the solid steps right behind them would fill them up
    let low_steps = (0.9 / (STORY_H / STAIR_STEPS as f32)).floor();
    let stairwell = (z_back - left.a0, z_front - low_steps * TOWER_RUN - left.a0);
    // the shop storey is faced in stone (a brick tower's in pale dressed stone, a rendered one's in a stone tinted like its walls)
    let (base_col, base_mat) = if st.wall_mat == mat::BRICK { (0xbcb3a3u32, mat::STONE) } else { (mix_hex(st.wall, 0x857f77, 0.62), mat::STONE) };
    for fl in 0..floors {
        let y0 = FLOOR_Y + fl as f32 * STORY_H;
        for (li, line) in [front, back, left, right].into_iter().enumerate() {
            let street = fl == 0 && li == 0;
            let (mut ops, avoid) = if street {
                (vec![door], vec![(door.u0 - 0.4, door.u1 + 0.4)])
            } else if li == 2 && flights[fl as usize].to_back {
                (vec![], vec![stairwell])
            } else {
                (vec![], vec![])
            };
            // tall shop windows either side of the door, sash windows everywhere else
            let (half_w, v0, v1) = if street { (0.8, 0.6, 2.35) } else { (0.65, 0.9, 2.2) };
            let wins: Vec<Op> = B::window_slots(line.len(), &avoid, 3.3).into_iter().map(|c| Op { u0: c - half_w, u1: c + half_w, v0, v1 }).collect();
            ops.extend(wins.iter().copied());
            let (col, m) = if fl == 0 { (base_col, base_mat) } else { (st.wall, st.wall_mat) };
            lean_wall(&mut b, line, y0, STORY_H, &ops, col, m);
            for o in &wins {
                tower_window(&mut b, line, y0, o, st.trim);
            }
        }
    }
    // a stone plinth (split round the door), a band at every floor line and quoins up the corners
    let (py0, py1, plinth) = (FLOOR_Y, FLOOR_Y + 0.45, 0x8d8a84);
    b.bx(Vec3::new(-hx - 0.05, py0, -hz - 0.05), Vec3::new(hx + 0.05, py1, -hz), plinth, mat::STONE, false);
    b.bx(Vec3::new(-hx - 0.05, py0, -hz), Vec3::new(-hx, py1, hz), plinth, mat::STONE, false);
    b.bx(Vec3::new(hx, py0, -hz), Vec3::new(hx + 0.05, py1, hz), plinth, mat::STONE, false);
    for (x0, x1) in [(-hx - 0.05, door_x - DOOR_W / 2.0 - 0.1), (door_x + DOOR_W / 2.0 + 0.1, hx + 0.05)] {
        b.bx(Vec3::new(x0, py0, hz), Vec3::new(x1, py1, hz + 0.05), plinth, mat::STONE, false);
    }
    for fl in 1..=floors {
        // (four boards round the outside: the one at the roof line is the cornice under the parapet)
        let y = FLOOR_Y + fl as f32 * STORY_H;
        let (y0, y1, o) = if fl == floors { (y - 0.3, y, 0.12) } else { (y - 0.12, y + 0.12, 0.06) };
        b.bx(Vec3::new(-hx - o, y0, hz), Vec3::new(hx + o, y1, hz + o), st.trim, mat::FLAT, false);
        b.bx(Vec3::new(-hx - o, y0, -hz - o), Vec3::new(hx + o, y1, -hz), st.trim, mat::FLAT, false);
        b.bx(Vec3::new(-hx - o, y0, -hz), Vec3::new(-hx, y1, hz), st.trim, mat::FLAT, false);
        b.bx(Vec3::new(hx, y0, -hz), Vec3::new(hx + o, y1, hz), st.trim, mat::FLAT, false);
    }
    b.corner_posts(hx - 0.06, hz - 0.06, FLOOR_Y, roof_y - 0.3, st.trim);
    // pilasters between the bays of the storeys above the shops, with capitals under a frieze that runs round below the cornice
    // (nothing stands further out than the cornice, so the tower's bounds stay what they were)
    let pil_col = if st.wall_mat == mat::BRICK { shade_hex(st.wall, 1.12) } else { mix_hex(st.wall, 0xffffff, 0.3) };
    for line in [front, back, left, right] {
        let bays = ((line.len() / 3.3).floor() as i32).max(1);
        for k in 1..bays {
            let u = line.len() * k as f32 / bays as f32;
            pilaster(&mut b, line, u, 0.42, FLOOR_Y + STORY_H + 0.12, roof_y - 0.42, 0.1, pil_col, st.wall_mat);
            pilaster(&mut b, line, u, 0.56, roof_y - 0.42, roof_y - 0.3, 0.12, st.trim, mat::FLAT);
        }
        wall_band(&mut b, line, roof_y, -0.56, -0.46, &[], 0.05, st.trim, mat::FLAT);
    }
    // striped awnings over the shop windows, clear of the door's canopy and of the corners
    for c in B::window_slots(front.len(), &[(door.u0 - 0.4, door.u1 + 0.4)], 3.3) {
        let (mut u0, mut u1) = ((c - 0.95).max(0.3), (c + 0.95).min(front.len() - 0.3));
        if c < door_u {
            u1 = u1.min(door_u - 1.35);
        } else {
            u0 = u0.max(door_u + 1.35);
        }
        if u1 - u0 > 0.8 {
            awning(&mut b, front, u0, u1, FLOOR_Y + STORY_H - 0.2, 0.45, 0.8, st.roof);
        }
    }
    // the front door standing ajar, and a canopy over it on two stays
    b.opening_trim(front, FLOOR_Y, &door, false, st.trim, None);
    b.door_leaf(front, FLOOR_Y, &door, door_col, 0xe3c15a);
    let cy = FLOOR_Y + DOOR_H + 0.35;
    b.bx(Vec3::new(door_x - 1.25, cy, hz), Vec3::new(door_x + 1.25, cy + 0.12, hz + 0.9), st.roof, mat::METAL, false);
    for sx in [-1.0f32, 1.0] {
        b.tube(Vec3::new(door_x + sx * 1.1, cy + 0.06, hz + 0.85), Vec3::new(door_x + sx * 1.1, cy + 0.9, hz + 0.05), 0.025, 0x2b2e34, mat::METAL);
    }

    // --- the stairwell: a spine wall between the two lanes on every storey, one flight per storey
    for fl in 0..floors {
        let y0 = FLOOR_Y + fl as f32 * STORY_H;
        b.mb.ao(0.8, 1.0);
        b.bx(Vec3::new(lane_a.x1, y0, z_back), Vec3::new(lane_b.x0, y0 + STORY_H - SLAB_T, z_front), 0xe6e0d4, mat::PLASTER, true);
        b.mb.ao(1.0, 1.0);
    }
    for f in &flights {
        tower_flight(&mut b, f, 0x9a958c, 0xb8b2a6);
        if !f.to_back {
            // the right-hand lane is open to the rooms
            stair_rail(&mut b, f.x1 - 0.06, f, rail);
        }
    }

    // --- the floors above the ground and the roof: each slab is open only above the flight that arrives at it
    for fl in 1..=floors {
        let y = FLOOR_Y + fl as f32 * STORY_H;
        let roof = fl == floors;
        let arrive = flights[fl as usize - 1];
        let (hx0, hx1, hz0, hz1) = arrive.hole();
        for (a0, a1, c0, c1) in [(ix0, hx0, iz0, iz1), (hx1, ix1, iz0, iz1), (hx0, hx1, iz0, hz0), (hx0, hx1, hz1, iz1)] {
            if a1 - a0 > 0.05 && c1 - c0 > 0.05 {
                b.bx(Vec3::new(a0, y - SLAB_T, c0), Vec3::new(a1, y, c1), 0xf1ede2, mat::PLASTER, true);
                let (col, m) = if roof { (0x7d7f84, mat::STONE) } else { (floor_col, floor_mat) };
                b.bx_faces(Vec3::new(a0, y, c0), Vec3::new(a1, y + 0.02, c1), col, m, 0b110111, false);
                if roof {
                    // the joints between the roof's paving slabs, on one grid across all the pieces
                    b.mb.mat(mat::STONE).hex(0x696b70);
                    let (grid, yj, hw) = (1.5f32, y + 0.03, 0.025);
                    let mut x = ((a0 + hx) / grid).floor() * grid - hx + grid;
                    while x < a1 - 0.1 {
                        b.mb.quad_out([Vec3::new(x - hw, yj, c0), Vec3::new(x + hw, yj, c0), Vec3::new(x + hw, yj, c1), Vec3::new(x - hw, yj, c1)], Vec3::Y);
                        x += grid;
                    }
                    let mut z = ((c0 + hz) / grid).floor() * grid - hz + grid;
                    while z < c1 - 0.1 {
                        b.mb.quad_out([Vec3::new(a0, yj, z - hw), Vec3::new(a1, yj, z - hw), Vec3::new(a1, yj, z + hw), Vec3::new(a0, yj, z + hw)], Vec3::Y);
                        z += grid;
                    }
                }
            }
        }
        // guard rails across the foot end of the opening and down its open sides; the sides stop a metre short of the head of the
        // flight, so you can step off sideways as well as straight ahead. Indoors the left lane lies between the outer wall and the
        // spine and the right lane has the spine on its left; on the roof there is no spine.
        if arrive.to_back {
            let (ze, xr) = (hz1 + 0.05, hx1 + 0.05);
            guard_rail(&mut b, Vec3::new(hx0, y, ze), Vec3::new(if roof { xr } else { hx1 }, y, ze), rail);
            if roof {
                guard_rail(&mut b, Vec3::new(xr, y, ze), Vec3::new(xr, y, hz0 + 1.0), rail);
            }
        } else {
            let (ze, xl, xr) = (hz0 - 0.05, hx0 - 0.05, hx1 + 0.05);
            guard_rail(&mut b, Vec3::new(if roof { xl } else { hx0 }, y, ze), Vec3::new(xr, y, ze), rail);
            guard_rail(&mut b, Vec3::new(xr, y, ze), Vec3::new(xr, y, hz1 - 1.0), rail);
            if roof {
                guard_rail(&mut b, Vec3::new(xl, y, ze), Vec3::new(xl, y, hz1 - 1.0), rail);
            }
        }
    }

    // --- the roof: a parapet with a coping, an air-conditioning unit and a vent stack (clear of the ways off the hatch)
    for (k, (mn, mx)) in [
        (Vec3::new(-hx - 0.1, roof_y, -hz - 0.1), Vec3::new(hx + 0.1, roof_y + PARAPET_H, -hz + WALL_T)),
        (Vec3::new(-hx - 0.1, roof_y, hz - WALL_T), Vec3::new(hx + 0.1, roof_y + PARAPET_H, hz + 0.1)),
        (Vec3::new(-hx - 0.1, roof_y, -hz + WALL_T), Vec3::new(-hx + WALL_T, roof_y + PARAPET_H, hz - WALL_T)),
        (Vec3::new(hx - WALL_T, roof_y, -hz + WALL_T), Vec3::new(hx + 0.1, roof_y + PARAPET_H, hz - WALL_T)),
    ]
    .into_iter()
    .enumerate()
    {
        b.bx(mn, mx, st.wall, st.wall_mat, true);
        // the copings of the front and back parapets cover the corners; those of the sides fit between them
        let ez = if k < 2 { 0.05 } else { 0.0 };
        b.bx(Vec3::new(mn.x - 0.05, mx.y, mn.z - ez), Vec3::new(mx.x + 0.05, mx.y + 0.08, mx.z + ez), st.trim, mat::FLAT, false);
    }
    let roof_mid = (z_back + z_front) * 0.5;
    let ac = Vec3::new(ix1 - 0.75, roof_y, roof_mid);
    b.bx(Vec3::new(ac.x - 0.55, roof_y, ac.z - 0.7), Vec3::new(ac.x + 0.55, roof_y + 0.85, ac.z + 0.7), 0xb8bcc2, mat::METAL, true);
    b.bx(Vec3::new(ac.x - 0.4, roof_y + 0.85, ac.z - 0.45), Vec3::new(ac.x + 0.4, roof_y + 0.9, ac.z + 0.45), 0x4a4d57, mat::METAL, false);
    b.tube(Vec3::new(ac.x, roof_y, ac.z - 1.1), Vec3::new(ac.x, roof_y + 1.0, ac.z - 1.1), 0.09, 0x9aa0a8, mat::METAL);

    // --- rooms: a little furniture, loot on every floor, now and then a chest upstairs, and one spot on the roof
    let room = (room_x0, ix1, iz0, iz1);
    for fl in 0..floors {
        let y = FLOOR_Y + fl as f32 * STORY_H;
        let chest = fl > 0 && rng.chance(0.4);
        tower_room(&mut b, rng, y, room, (fl == 0).then_some(door_x), chest, hz);
    }
    b.loot.push(Vec3::new((room_x0 + ac.x - 0.55) * 0.5, roof_y, roof_mid));

    let door_out = Vec3::new(door_x, 0.0, hz + 2.4);
    let door_in = Vec3::new(door_x, FLOOR_Y, hz - 1.6);
    // (the rails round the hatch stand a little higher than the parapet)
    let mut g = b.finish(Vec2::new(hx, hz), roof_y + 1.05, door_out, door_in);
    g.stairs = Some((Vec3::new(lane_a.x0, lane_a.y_low, z_back), Vec3::new(lane_a.x1, lane_a.y_high, z_front)));
    g.entry = Some(Entry { edge: Vec3::new(door_x, FLOOR_Y, hz + 0.1), half_w: 1.0 });
    g
}

// ---------------------------------------------------------------------------------
// Industry
// ---------------------------------------------------------------------------------

/// A wall of corrugated sheet: the wall with its openings, and ribs pressed into its outer face (mesh only).
fn ribbed_wall(b: &mut B, l: Line, y0: f32, h: f32, ops: &[Op], col: u32, rib: u32) {
    b.wall(l, y0, h, ops, col, mat::METAL);
    // only the faces that can be seen: the outer one and the two edges
    let faces = match (l.along_x, l.out > 0.0) {
        (true, true) => 0b010011,
        (true, false) => 0b100011,
        (false, true) => 0b110001,
        (false, false) => 0b110010,
    };
    let (d0, d1) = if l.out > 0.0 { (WALL_T / 2.0, WALL_T / 2.0 + 0.04) } else { (-WALL_T / 2.0 - 0.04, -WALL_T / 2.0) };
    for (u0, u1, v0, v1) in wall_rects(l.len(), h, ops) {
        if v1 - v0 < 0.25 {
            continue;
        }
        let mut u = u0 + 0.2;
        while u < u1 - 0.12 {
            let (a, c) = l.bx(u - 0.05, u + 0.05, d0, d1, y0 + v0, y0 + v1);
            b.bx_faces(a, c, rib, mat::METAL, faces, false);
            u += 0.45;
        }
    }
}

/// A wooden crate standing on `min.y` (solid), with dark battens round its middle and its top edge.
fn crate_box(b: &mut B, min: Vec3, max: Vec3, col: u32) {
    b.bx(min, max, col, mat::WOOD, true);
    let (dark, e) = (shade_hex(col, 0.7), 0.03);
    let mid = (min.y + max.y) * 0.5;
    for (y0, y1) in [(mid - 0.06, mid + 0.06), (max.y - 0.1, max.y + 0.01)] {
        b.bx_faces(Vec3::new(min.x - e, y0, min.z - e), Vec3::new(max.x + e, y1, max.z + e), dark, mat::WOOD, 0b110011, false);
    }
}

/// An industrial hall, about 7 m tall: corrugated metal walls, a shallow pitched roof, a wide roll-up door on the front (+Z) that stays
/// open, a side door, strip windows high up, and stacks of crates (solid) either side of the aisle that runs back from the big door, with
/// three to five loot spots on the floor between them and often a chest at the end of the aisle. `w` (16 to 28 m) by `d` (12 to 20 m)
/// is the footprint; sizes outside those ranges are clamped into them.
pub fn gen_warehouse(rng: &mut Rng, w: f32, d: f32) -> Geom {
    let mut b = B::new();
    let (w, d) = (w.clamp(16.0, 28.0), d.clamp(12.0, 20.0));
    let (hx, hz) = (w / 2.0, d / 2.0);
    let h = 6.0;
    let top_y = FLOOR_Y + h;
    let (ix0, ix1, iz0, iz1) = (-hx + WALL_T, hx - WALL_T, -hz + WALL_T, hz - WALL_T);
    let skin = *rng.pick(&[0x9aa7b0u32, 0x7f9fb8, 0xc2c4bd, 0x88a184, 0xb86b4b, 0xd8c9a3]);
    let rib = shade_hex(skin, 0.84);
    let roof_col = *rng.pick(&[0x6c6f78u32, 0x8a8d93, 0x5a6f80]);
    let (steel, hazard) = (0x4a4e57, 0xe0b030);

    // --- concrete slab and floor
    b.mb.ao(0.55, 1.0);
    b.bx(Vec3::new(-hx - 0.1, -1.6, -hz - 0.1), Vec3::new(hx + 0.1, FLOOR_Y, hz + 0.1), 0x9d9e9f, mat::STONE, true);
    b.mb.ao(1.0, 1.0);
    b.bx_faces(Vec3::new(ix0, FLOOR_Y, iz0), Vec3::new(ix1, FLOOR_Y + 0.02, iz1), 0xa9aaa4, mat::STONE, 0b000100, false);

    // --- walls: the roll-up door in the front, a side door, strip windows high up all round
    let front = Line { along_x: true, a0: -hx, a1: hx, c: hz - WALL_T / 2.0, out: 1.0 };
    let back = Line { along_x: true, a0: -hx, a1: hx, c: -hz + WALL_T / 2.0, out: -1.0 };
    let left = Line { along_x: false, a0: -hz + WALL_T, a1: hz - WALL_T, c: -hx + WALL_T / 2.0, out: -1.0 };
    let right = Line { along_x: false, a0: -hz + WALL_T, a1: hz - WALL_T, c: hx - WALL_T / 2.0, out: 1.0 };
    let big_w = 5.0;
    let door_u = (w * *rng.pick(&[0.5f32, 0.38, 0.62])).clamp(big_w / 2.0 + 2.0, w - big_w / 2.0 - 2.0);
    let door_x = -hx + door_u;
    let big = Op { u0: door_u - big_w / 2.0, u1: door_u + big_w / 2.0, v0: 0.0, v1: 4.6 };
    let (side, other) = if rng.chance(0.5) { (left, right) } else { (right, left) };
    let sd_u = side.len() * 0.3;
    let side_door = Op { u0: sd_u - DOOR_W / 2.0, u1: sd_u + DOOR_W / 2.0, v0: 0.0, v1: DOOR_H };
    let strip = |len: f32, avoid: &[(f32, f32)]| -> Vec<Op> { B::window_slots(len, avoid, 4.2).into_iter().map(|c| Op { u0: c - 1.0, u1: c + 1.0, v0: 4.5, v1: 5.3 }).collect() };
    let walls = [
        (front, Some(big), strip(front.len(), &[(big.u0 - 0.6, big.u1 + 0.6)])),
        (back, None, strip(back.len(), &[])),
        (side, Some(side_door), strip(side.len(), &[(side_door.u0 - 0.6, side_door.u1 + 0.6)])),
        (other, None, strip(other.len(), &[])),
    ];
    for (line, door, wins) in &walls {
        let mut ops = wins.clone();
        ops.extend(door.iter().copied());
        ribbed_wall(&mut b, *line, FLOOR_Y, h, &ops, skin, rib);
        for o in wins {
            tower_window(&mut b, *line, FLOOR_Y, o, steel);
        }
    }
    b.corner_posts(hx - 0.06, hz - 0.06, FLOOR_Y, top_y, steel);
    // a concrete plinth round the foot of the walls (broken at the doors and their frames) and flashing along their tops
    for (line, door, _) in &walls {
        let cut: Vec<Op> = door.iter().map(|o| Op { u0: o.u0 - 0.12, u1: o.u1 + 0.12, ..*o }).collect();
        wall_band(&mut b, *line, FLOOR_Y, 0.0, 0.5, &cut, 0.09, 0x8f8d88, mat::STONE);
        wall_band(&mut b, *line, FLOOR_Y, h - 0.25, h, &[], 0.07, steel, mat::METAL);
    }
    // the roll-up door, rolled up into its drum inside above the opening, with its last few slats and bottom rail showing; hazard
    // stripes up the frame, a sign and lamps outside
    b.opening_trim(front, FLOOR_Y, &big, false, hazard, None);
    let t = WALL_T / 2.0 + 0.056;
    for u in [big.u0 - 0.1, big.u1] {
        for k in 0..6 {
            let y = FLOOR_Y + 0.15 + k as f32 * 0.32;
            let (a, c) = front.bx(u - 0.006, u + 0.106, -t, t, y, y + 0.16);
            b.bx_faces(a, c, 0x2b2e34, mat::FLAT, 0b110011, false);
        }
    }
    let inner = -WALL_T / 2.0;
    let (a, c) = front.bx(big.u0 - 0.25, big.u1 + 0.25, inner - 0.6, inner, FLOOR_Y + big.v1 + 0.05, FLOOR_Y + big.v1 + 0.65);
    b.bx(a, c, steel, mat::METAL, false);
    let (a, c) = front.bx(big.u0, big.u1, -0.06, 0.02, FLOOR_Y + big.v1 - 0.62, FLOOR_Y + big.v1);
    b.bx(a, c, shade_hex(skin, 0.92), mat::METAL, false);
    for k in 1..4 {
        let y = FLOOR_Y + big.v1 - k as f32 * 0.155;
        let (a, c) = front.bx(big.u0, big.u1, -0.072, 0.032, y - 0.018, y + 0.018);
        b.bx_faces(a, c, shade_hex(skin, 0.78), mat::METAL, 0b111100, false);
    }
    let (a, c) = front.bx(big.u0, big.u1, -0.08, 0.04, FLOOR_Y + big.v1 - 0.68, FLOOR_Y + big.v1 - 0.62);
    b.bx(a, c, steel, mat::METAL, false);
    for u in [big.u0 - 0.5, big.u1 + 0.5] {
        let (y, f) = (FLOOR_Y + 4.0, WALL_T / 2.0);
        b.tube(front.at(u, f, y + 0.3), front.at(u, f + 0.22, y + 0.12), 0.025, steel, mat::METAL);
        b.mb.mat(mat::METAL).hex(steel);
        b.mb.cylinder(front.at(u, f + 0.22, y - 0.02), 0.17, 0.05, 0.14, 8, false, true);
        b.mb.mat(mat::EMISSIVE).hex(0xffe9b0);
        b.mb.cylinder(front.at(u, f + 0.22, y - 0.03), 0.13, 0.13, 0.012, 8, true, false);
    }
    let (a, c) = front.bx(big.u0 + 0.5, big.u1 - 0.5, WALL_T / 2.0, WALL_T / 2.0 + 0.06, FLOOR_Y + 4.85, FLOOR_Y + 5.6);
    b.bx(a, c, *rng.pick(&[0xc24a3cu32, 0x2f8f8a, 0x3f5f94, 0xd9772f]), mat::FLAT, false);
    let (a, c) = front.bx(big.u0 + 0.7, big.u1 - 0.7, WALL_T / 2.0 + 0.06, WALL_T / 2.0 + 0.08, FLOOR_Y + 5.0, FLOOR_Y + 5.45);
    b.bx(a, c, 0xf4f1ea, mat::FLAT, false);
    // the side door, standing ajar, with a lamp over it
    b.opening_trim(side, FLOOR_Y, &side_door, false, steel, None);
    b.door_leaf(side, FLOOR_Y, &side_door, 0x5a6f80, 0xc9c9c9);
    let (s0, s1) = if side.out > 0.0 { (WALL_T / 2.0, WALL_T / 2.0 + 0.2) } else { (-WALL_T / 2.0 - 0.2, -WALL_T / 2.0) };
    let (a, c) = side.bx(sd_u - 0.2, sd_u + 0.2, s0, s1, FLOOR_Y + DOOR_H + 0.3, FLOOR_Y + DOOR_H + 0.45);
    b.bx(a, c, 0xffe29a, mat::EMISSIVE, false);

    // --- roof: two shallow slopes up to a ridge along the long side, with standing seams, a ridge cap, gutters and vents; inside, a
    // ceiling under steel trusses and lamps. The colliders (two wedges) stay within the footprint.
    let ridge_x = w >= d;
    let map = |x: f32, y: f32, z: f32| if ridge_x { Vec3::new(x, y, z) } else { Vec3::new(z, y, x) };
    let (len_h, wid_h) = if ridge_x { (hx, hz) } else { (hz, hx) };
    let rise = wid_h * 0.14;
    let ov = 0.3;
    let (el, ew) = (len_h + ov, wid_h + ov);
    let eave_y = top_y - rise * ov / wid_h;
    for s in [-1.0f32, 1.0] {
        let n = Vec3::new(0.0, wid_h, s * rise).normalize();
        let n = if ridge_x { n } else { Vec3::new(n.z, n.y, n.x) };
        let p = [map(-el, eave_y, s * ew), map(el, eave_y, s * ew), map(el, top_y + rise, 0.0), map(-el, top_y + rise, 0.0)];
        b.mb.mat(mat::METAL).hex(roof_col);
        b.mb.quad_out(p, n);
        b.mb.hex(shade_hex(roof_col, 0.7));
        b.mb.quad_out(p, -n);
        let seams = (2.0 * el / 1.2).round() as i32;
        for k in 0..=seams {
            let x = -el + 0.06 + (2.0 * el - 0.12) * k as f32 / seams as f32;
            b.board(map(x, eave_y + 0.03, s * ew), map(x, top_y + rise + 0.03, 0.0), n, 0.06, 0.05, shade_hex(roof_col, 0.85), mat::METAL);
        }
        // a gutter along the eave, with a downpipe at either end down the wall to a shoe above the plinth
        b.tube(map(-el, eave_y - 0.02, s * (ew + 0.03)), map(el, eave_y - 0.02, s * (ew + 0.03)), 0.07, 0x9aa0a8, mat::METAL);
        for t in [-1.0f32, 1.0] {
            let x = t * (len_h - 0.4);
            let (z_eave, z_wall, z_shoe) = (s * (ew + 0.03), s * (wid_h + 0.13), s * (wid_h + 0.24));
            let y_bend = eave_y - 0.3;
            for (p0, p1) in [
                (map(x, eave_y - 0.02, z_eave), map(x, y_bend, z_eave)),
                (map(x, y_bend, z_eave), map(x, y_bend - 0.12, z_wall)),
                (map(x, y_bend - 0.12, z_wall), map(x, FLOOR_Y + 0.75, z_wall)),
                (map(x, FLOOR_Y + 0.75, z_wall), map(x, FLOOR_Y + 0.6, z_shoe)),
            ] {
                b.tube(p0, p1, 0.055, 0x9aa0a8, mat::METAL);
            }
        }
        // skylights in every bay between the trusses: a pane in a frame, a little proud of the sheeting
        let bays = ((2.0 * len_h) / 4.5).round().max(2.0) as i32;
        let lift = |x: f32, w: f32, e: f32| map(x, top_y + rise - (top_y + rise - eave_y) * w / ew, s * w) + n * e;
        for k in 0..bays {
            let xc = -len_h + 2.0 * len_h * (k as f32 + 0.5) / bays as f32;
            let (w0, w1) = (0.3 * ew, 0.72 * ew);
            b.mb.mat(mat::METAL).hex(0xc9ccd0);
            b.mb.quad_out([lift(xc - 0.75, w0 - 0.08, 0.075), lift(xc + 0.75, w0 - 0.08, 0.075), lift(xc + 0.75, w1 + 0.08, 0.075), lift(xc - 0.75, w1 + 0.08, 0.075)], n);
            b.mb.mat(mat::GLASS).hex(0x56788a);
            b.mb.quad_out([lift(xc - 0.62, w0, 0.085), lift(xc + 0.62, w0, 0.085), lift(xc + 0.62, w1, 0.085), lift(xc - 0.62, w1, 0.085)], n);
        }
    }
    let (a, c) = (map(-el - 0.02, top_y + rise - 0.05, -0.2), map(el + 0.02, top_y + rise + 0.08, 0.2));
    b.bx(a.min(c), a.max(c), shade_hex(roof_col, 0.8), mat::METAL, false);
    b.mb.mat(mat::METAL).hex(skin);
    for s in [-1.0f32, 1.0] {
        b.mb.tri_out(map(s * len_h, top_y, -wid_h), map(s * len_h, top_y, wid_h), map(s * len_h, top_y + rise, 0.0), map(s, 0.0, 0.0));
    }
    for t in [-0.45f32, 0.45] {
        let v = map(t * len_h, top_y + rise, 0.0);
        b.mb.mat(mat::METAL).hex(0xb8bcc2);
        b.mb.cylinder(v - Vec3::Y * 0.05, 0.28, 0.28, 0.45, 10, false, false);
        b.mb.sphere(v + Vec3::Y * 0.42, 0.3, 1);
    }
    if ridge_x {
        b.cols.push(Collider::wedge(Vec3::new(-hx, top_y, 0.0), Vec3::new(hx, top_y + rise, hz), 3, Tag::Static));
        b.cols.push(Collider::wedge(Vec3::new(-hx, top_y, -hz), Vec3::new(hx, top_y + rise, 0.0), 1, Tag::Static));
    } else {
        b.cols.push(Collider::wedge(Vec3::new(0.0, top_y, -hz), Vec3::new(hx, top_y + rise, hz), 2, Tag::Static));
        b.cols.push(Collider::wedge(Vec3::new(-hx, top_y, -hz), Vec3::new(0.0, top_y + rise, hz), 0, Tag::Static));
    }
    b.bx(Vec3::new(ix0, top_y - 0.15, iz0), Vec3::new(ix1, top_y, iz1), 0xd8dadc, mat::METAL, true);
    let trusses = ((2.0 * len_h) / 4.5).round().max(2.0) as i32;
    for k in 1..trusses {
        let x = -len_h + 2.0 * len_h * k as f32 / trusses as f32;
        let (a, c) = (map(x - 0.1, top_y - 0.55, -wid_h + WALL_T), map(x + 0.1, top_y - 0.15, wid_h - WALL_T));
        b.bx(a.min(c), a.max(c), steel, mat::METAL, false);
        for t in [-0.5f32, 0.5] {
            let p = map(x, top_y - 0.55, t * wid_h);
            b.tube(p, p - Vec3::Y * 0.6, 0.012, 0x2b2e34, mat::METAL);
            b.mb.mat(mat::METAL).hex(0x5a5f6a);
            b.mb.cylinder(p - Vec3::Y * 0.85, 0.32, 0.08, 0.25, 8, false, false);
            b.mb.mat(mat::EMISSIVE).hex(0xfff1c4);
            b.mb.cylinder(p - Vec3::Y * 0.86, 0.26, 0.26, 0.02, 8, true, true);
        }
    }

    // --- inside: safety lines down the aisle, and crates stacked in rows either side of it
    for sx in [-1.0f32, 1.0] {
        let x = door_x + sx * 2.75;
        b.bx_faces(Vec3::new(x - 0.06, FLOOR_Y + 0.02, iz0 + 0.3), Vec3::new(x + 0.06, FLOOR_Y + 0.025, iz1 - 0.3), hazard, mat::FLAT, 0b000100, false);
    }
    let aisle = 2.6;
    // a stack reaches this far from its spot (the bottom crate 0.8 m, the ones on it a little less but set askew); none stands in the way
    // from the side door across to the aisle
    let stack_r = 0.86;
    let side_z = side.a0 + sd_u;
    let mut spots = vec![];
    let mut z = iz0 + 1.6;
    while z < iz1 - 3.0 {
        let mut x = ix0 + 1.4;
        while x < ix1 - 1.2 {
            let cross = (x - door_x) * side.out > 0.0 && (z - side_z).abs() < stack_r + DOOR_W / 2.0 + 0.1;
            if (x - door_x).abs() > aisle + 0.9 && !cross {
                spots.push((x, z));
            }
            x += 3.2;
        }
        z += 3.4;
    }
    rng.shuffle(&mut spots);
    let keep = (spots.len() * 2 / 3).max(spots.len().min(3));
    let crate_cols = [0xa8794au32, 0x9c6a3a, 0xb98b56, 0x8a5a34, 0x6f8fa6, 0x7f9a5c];
    for &(x, z) in &spots[..keep] {
        let mut y = FLOOR_Y;
        let mut s = rng.range(1.25, 1.6);
        for k in 0..1 + rng.below(3) {
            let (ox, oz) = if k == 0 { (0.0, 0.0) } else { (rng.range(-0.12, 0.12), rng.range(-0.12, 0.12)) };
            let top = y + s * rng.range(0.75, 0.95);
            crate_box(&mut b, Vec3::new(x + ox - s / 2.0, y, z + oz - s / 2.0), Vec3::new(x + ox + s / 2.0, top, z + oz + s / 2.0), *rng.pick(&crate_cols));
            y = top;
            s *= rng.range(0.78, 0.92);
        }
    }
    // a chest at the end of the aisle (or in a back corner), then loot on clear floor between the stacks
    if rng.chance(0.7) {
        let cands = [Vec3::new(door_x, FLOOR_Y, iz0 + 0.5), Vec3::new(ix0 + 0.7, FLOOR_Y, iz0 + 0.5), Vec3::new(ix1 - 0.7, FLOOR_Y, iz0 + 0.5)];
        if let Some(p) = cands.into_iter().find(|p| floor_clear(&b, p.x, p.z, FLOOR_Y, 0.55, 0.4)) {
            b.chests.push((p, 0.0));
        }
    }
    let want = 3 + rng.below(3);
    let mut loot: Vec<Vec3> = vec![];
    let clear = |b: &B, loot: &[Vec3], p: Vec3| {
        let by_door = (p.x - door_x).abs() < big_w / 2.0 && p.z > iz1 - 1.5;
        !by_door && floor_clear(b, p.x, p.z, FLOOR_Y, 0.35, 0.35) && b.chests.iter().all(|(c, _)| c.distance(p) > 1.3) && loot.iter().all(|q| q.distance(p) > 2.0)
    };
    for _ in 0..60 {
        if loot.len() >= want {
            break;
        }
        let p = Vec3::new(rng.range(ix0 + 0.6, ix1 - 0.6), FLOOR_Y, rng.range(iz0 + 0.6, iz1 - 0.6));
        if clear(&b, &loot, p) {
            loot.push(p);
        }
    }
    // the aisle is never stacked: top up there if the floor between the crates was too crowded
    for k in 0..8 {
        if loot.len() >= 3 {
            break;
        }
        let p = Vec3::new(door_x + if k % 2 == 0 { -1.2 } else { 1.2 }, FLOOR_Y, iz0 + 2.0 + (iz1 - iz0 - 4.5) * (k / 2) as f32 / 3.0);
        if clear(&b, &loot, p) {
            loot.push(p);
        }
    }
    b.loot.extend(loot);

    let mut g = b.finish(Vec2::new(hx, hz), top_y + rise + 0.75, Vec3::new(door_x, 0.0, hz + 3.0), Vec3::new(door_x, FLOOR_Y, hz - 2.5));
    g.entry = Some(Entry { edge: Vec3::new(door_x, FLOOR_Y, hz + 0.1), half_w: big_w / 2.0 - 0.2 });
    g
}

/// A 20-foot shipping container, shut: 6.1 m long (along X), 2.44 m wide and 2.6 m tall, with corrugated sides, end and roof, a steel
/// frame with corner castings, and the cargo doors with their locking bars at the +X end. `col` is its paint. One solid block.
pub fn gen_container(col: u32) -> Geom {
    let mut b = B::new();
    let (hx, hz, h) = (3.05f32, 1.22f32, 2.6f32);
    let (rib, frame) = (shade_hex(col, 0.84), shade_hex(col, 0.62));
    // the shell, a hair inside the frame
    b.mb.ao(0.7, 1.0);
    b.bx(Vec3::new(-hx + 0.02, 0.02, -hz + 0.04), Vec3::new(hx - 0.02, h - 0.03, hz - 0.04), col, mat::METAL, false);
    b.mb.ao(1.0, 1.0);
    b.cols.push(Collider::aabb_box(Vec3::new(-hx, 0.0, -hz), Vec3::new(hx, h, hz), Tag::Static));
    // corrugation: ribs down both long sides and the closed end, and across the roof
    let mut x = -hx + 0.3;
    while x < hx - 0.25 {
        for (z0, z1, faces) in [(hz - 0.04, hz - 0.005, 0b010011u8), (-hz + 0.005, -hz + 0.04, 0b100011)] {
            b.bx_faces(Vec3::new(x - 0.06, 0.16, z0), Vec3::new(x + 0.06, h - 0.12, z1), rib, mat::METAL, faces, false);
        }
        x += 0.3;
    }
    let mut z = -hz + 0.3;
    while z < hz - 0.25 {
        b.bx_faces(Vec3::new(-hx + 0.005, 0.16, z - 0.06), Vec3::new(-hx + 0.02, h - 0.12, z + 0.06), rib, mat::METAL, 0b110010, false);
        z += 0.3;
    }
    let mut x = -hx + 0.35;
    while x < hx - 0.3 {
        b.bx_faces(Vec3::new(x - 0.08, h - 0.03, -hz + 0.12), Vec3::new(x + 0.08, h - 0.005, hz - 0.12), rib, mat::METAL, 0b000111, false);
        x += 0.4;
    }
    // the frame: corner posts with castings, and rails along the top and bottom of every side
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let (x0, x1) = if sx > 0.0 { (hx - 0.12, hx) } else { (-hx, -hx + 0.12) };
        let (z0, z1) = if sz > 0.0 { (hz - 0.12, hz) } else { (-hz, -hz + 0.12) };
        b.bx(Vec3::new(x0, 0.0, z0), Vec3::new(x1, h, z1), frame, mat::METAL, false);
        for (y0, y1) in [(0.0, 0.15), (h - 0.15, h + 0.01)] {
            b.bx(Vec3::new(x0 - 0.01, y0, z0 - 0.01), Vec3::new(x1 + 0.01, y1, z1 + 0.01), shade_hex(frame, 0.8), mat::METAL, false);
        }
    }
    for s in [-1.0f32, 1.0] {
        let (z0, z1) = if s > 0.0 { (hz - 0.1, hz) } else { (-hz, -hz + 0.1) };
        let (x0, x1) = if s > 0.0 { (hx - 0.1, hx) } else { (-hx, -hx + 0.1) };
        for (y0, y1) in [(0.0, 0.16), (h - 0.12, h)] {
            b.bx(Vec3::new(-hx + 0.12, y0, z0), Vec3::new(hx - 0.12, y1, z1), frame, mat::METAL, false);
            b.bx(Vec3::new(x0, y0, -hz + 0.12), Vec3::new(x1, y1, hz - 0.12), frame, mat::METAL, false);
        }
    }
    // the cargo doors at +X: two leaves, each with two locking bars and their handles
    for (za, zb) in [(-hz + 0.12, -0.01), (0.01, hz - 0.12)] {
        b.bx(Vec3::new(hx - 0.03, 0.16, za), Vec3::new(hx - 0.005, h - 0.12, zb), shade_hex(col, 0.95), mat::METAL, false);
        for k in [0.28f32, 0.72] {
            let z = za + (zb - za) * k;
            b.tube(Vec3::new(hx + 0.015, 0.2, z), Vec3::new(hx + 0.015, h - 0.16, z), 0.022, 0x9aa0a8, mat::METAL);
            b.bx(Vec3::new(hx - 0.005, 1.05, z - 0.03), Vec3::new(hx + 0.06, 1.15, z + 0.2), 0x9aa0a8, mat::METAL, false);
        }
    }
    b.finish(Vec2::new(hx, hz), h, Vec3::ZERO, Vec3::ZERO)
}

/// A tall round factory chimney of height `h` (clamped to 12..=60 m): a tapering brick shaft on a square plinth, with steel bands round
/// it, a red-and-white top under a dark rim and a ladder up its front (+Z). One solid cylinder on a solid plinth.
pub fn gen_smokestack(h: f32) -> Geom {
    let h = h.clamp(12.0, 60.0);
    let mut b = B::new();
    let (base_h, r0, r1) = (2.4f32, 1.7f32, 1.15f32);
    // the plinth with its capping and a small iron door
    b.mb.ao(0.6, 1.0);
    b.bx(Vec3::new(-2.3, -1.0, -2.3), Vec3::new(2.3, base_h, 2.3), 0x8e4b3a, mat::BRICK, true);
    b.mb.ao(1.0, 1.0);
    // (the capping is solid too, or anyone up there would stand sunk into it)
    b.bx(Vec3::new(-2.45, base_h, -2.45), Vec3::new(2.45, base_h + 0.25, 2.45), 0x9b9a95, mat::STONE, true);
    b.bx(Vec3::new(-0.5, 0.0, 2.3), Vec3::new(0.5, 1.9, 2.34), 0x2b2e34, mat::METAL, false);
    // the shaft
    let y0 = base_h + 0.25;
    let r_at = |y: f32| r0 + (r1 - r0) * ((y - y0) / (h - y0)).clamp(0.0, 1.0);
    b.mb.mat(mat::BRICK).hex(0xa65a44).ao(0.8, 1.0);
    b.mb.cylinder(Vec3::new(0.0, y0, 0.0), r0, r1, h - y0, 16, false, false);
    b.mb.ao(1.0, 1.0);
    // (three drums that follow the taper, each as wide as the shaft at its foot)
    for k in 0..3 {
        let (ya, yb) = (y0 + (h - y0) * k as f32 / 3.0, y0 + (h - y0) * (k + 1) as f32 / 3.0);
        b.cols.push(Collider::cyl(0.0, 0.0, r_at(ya), ya, yb, Tag::Static));
    }
    // red and white stripes round the top, steel bands below them
    let stripes = h - 4.5;
    for k in 0..3 {
        let ya = stripes + k as f32 * 1.5;
        b.mb.mat(mat::PLASTER).hex(if k % 2 == 0 { 0xc43b32 } else { 0xf4f1ea });
        b.mb.cylinder(Vec3::new(0.0, ya, 0.0), r_at(ya) + 0.02, r_at(ya + 1.5) + 0.02, 1.5, 16, false, false);
    }
    b.mb.mat(mat::METAL).hex(0x3d3f46);
    let mut y = y0 + 3.0;
    while y < stripes - 1.0 {
        b.mb.cylinder(Vec3::new(0.0, y, 0.0), r_at(y) + 0.06, r_at(y + 0.18) + 0.06, 0.18, 16, false, false);
        y += 5.0;
    }
    // the rim and the dark mouth inside it
    b.mb.cylinder(Vec3::new(0.0, h, 0.0), r1 + 0.14, r1 + 0.14, 0.3, 16, true, false);
    b.mb.cylinder(Vec3::new(0.0, h + 0.3, 0.0), r1 + 0.14, r1 - 0.12, 0.0001, 16, false, false);
    b.mb.mat(mat::FLAT).hex(0x141416);
    b.mb.cylinder(Vec3::new(0.0, h + 0.2, 0.0), r1 - 0.12, r1 - 0.12, 0.1, 16, false, true);
    // a ladder up the front, following the taper
    let off = 0.22;
    for sx in [-0.25f32, 0.25] {
        b.tube(Vec3::new(sx, y0 + 0.3, r_at(y0 + 0.3) + off), Vec3::new(sx, h - 0.1, r_at(h - 0.1) + off), 0.03, 0x5a5f6a, mat::METAL);
    }
    let mut y = y0 + 0.6;
    while y < h - 0.2 {
        let z = r_at(y) + off;
        b.tube(Vec3::new(-0.25, y, z), Vec3::new(0.25, y, z), 0.02, 0x5a5f6a, mat::METAL);
        y += 0.5;
    }
    b.finish(Vec2::new(2.45, 2.45), h + 0.3, Vec3::ZERO, Vec3::ZERO)
}

/// Paint of the junkyard's car wrecks (faded further per wreck).
const WRECK_PAINT: [u32; 9] = [0xb5463a, 0x2f6fb0, 0xd9a23b, 0x3e7e4e, 0xe8e4da, 0x5a5f6a, 0x8a3b6e, 0x2b2e34, 0xc96f2a];

/// A junkyard pile of `n` (clamped to 1..=8) car wrecks crushed flat, in assorted faded colours, each lying a little askew on the one
/// below it. Every wreck is a solid block.
pub fn gen_wreck_stack(rng: &mut Rng, n: u32) -> Geom {
    let mut b = B::new();
    let mut y = 0.0f32;
    let mut half = Vec2::ZERO;
    for _ in 0..n.clamp(1, 8) {
        let paint = shade_hex(*rng.pick(&WRECK_PAINT), rng.range(0.7, 0.95));
        let (l, wd) = (rng.range(1.95, 2.25), rng.range(0.82, 0.92));
        let (body_h, cab_h) = (rng.range(0.36, 0.48), rng.range(0.1, 0.2));
        let yaw = rng.range(-0.2, 0.2) + if rng.chance(0.5) { PI } else { 0.0 };
        let c = Vec3::new(rng.range(-0.2, 0.2), y, rng.range(-0.12, 0.12));
        b.mb.push_xf(Mat4::from_translation(c) * Mat4::from_rotation_y(yaw));
        // the squashed body with a rusty sill and black bumpers, the cabin folded down onto it with its broken windows, and the tyres
        b.mb.mat(mat::METAL).hex(paint).ao(0.75, 1.0);
        b.mb.box_min_max(Vec3::new(-l, 0.05, -wd), Vec3::new(l, body_h, wd));
        b.mb.ao(1.0, 1.0);
        b.mb.hex(0x7a4a2e);
        b.mb.box_faces(Vec3::new(-l - 0.01, 0.05, -wd - 0.01), Vec3::new(l + 0.01, 0.16, wd + 0.01), 0b110011);
        b.mb.hex(0x2b2e34);
        for (x0, x1) in [(l - 0.02, l + 0.08), (-l - 0.08, -l + 0.02)] {
            b.mb.box_min_max(Vec3::new(x0, 0.1, -wd + 0.05), Vec3::new(x1, 0.25, wd - 0.05));
        }
        let cab0 = -l * rng.range(0.35, 0.55);
        b.mb.hex(shade_hex(paint, 0.85));
        b.mb.box_min_max(Vec3::new(cab0, body_h, -wd * 0.84), Vec3::new(cab0 + l * 0.9, body_h + cab_h, wd * 0.84));
        b.mb.mat(mat::GLASS).hex(0x2a3440);
        b.mb.box_faces(Vec3::new(cab0 + 0.08, body_h + 0.02, -wd * 0.85), Vec3::new(cab0 + l * 0.9 - 0.08, body_h + cab_h - 0.025, wd * 0.85), 0b110000);
        b.mb.mat(mat::FLAT).hex(0x1f2024);
        for sx in [-0.62f32, 0.62] {
            for sz in [-1.0f32, 1.0] {
                b.mb.push_xf(Mat4::from_translation(Vec3::new(sx * l, 0.17, sz * (wd - 0.02))) * Mat4::from_rotation_x(FRAC_PI_2));
                b.mb.cylinder(Vec3::new(0.0, -0.09, 0.0), 0.19, 0.19, 0.18, 8, true, true);
                b.mb.pop_xf();
            }
        }
        b.mb.pop_xf();
        let (lo, hi) = rot_box(Vec3::new(-l - 0.08, 0.0, -wd - 0.08), Vec3::new(l + 0.08, body_h + cab_h, wd + 0.08), yaw, c);
        b.cols.push(Collider::aabb_box(lo, hi, Tag::Static));
        half = half.max(Vec2::new(lo.x.abs().max(hi.x.abs()), lo.z.abs().max(hi.z.abs())));
        y += body_h + cab_h - 0.03;
    }
    b.finish(half, y + 0.03, Vec3::ZERO, Vec3::ZERO)
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
    // a wainscot round the foot of the ground-floor walls in a deeper tone under a rail of trim (broken at the door and its frame):
    // stone under a brick house, darker boards under a cabin
    let (base, base_mat) = if st.logs {
        (shade_hex(st.wall, 0.72), mat::WOOD)
    } else if st.wall_mat == mat::BRICK {
        (0xa39b8f, mat::STONE)
    } else {
        (shade_hex(st.wall, 0.8), st.wall_mat)
    };
    let door_cut = [Op { u0: door.u0 - 0.12, u1: door.u1 + 0.12, ..door }];
    for (line, cut) in [(front, &door_cut[..]), (back, &[][..]), (left, &[][..]), (right, &[][..])] {
        wall_band(b, line, FLOOR_Y, 0.0, 0.7, cut, 0.03, base, base_mat);
        wall_band(b, line, FLOOR_Y, 0.7, 0.77, cut, 0.055, st.trim, mat::FLAT);
    }
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

/// A flight of stairs in the world: its bounds and the direction it rises (0 +X, 1 +Z, 2 -X, 3 -Z, as for a wedge collider).
#[derive(Clone, Copy, Debug)]
pub struct Stairs {
    pub min: Vec3,
    pub max: Vec3,
    pub dir: u8,
}

/// The direction (as for a wedge collider) of the open side of a flight of stairs that rises toward `dir`: the side with the
/// handrail, where the upper floor is stepped onto. In a house's own frame the flight rises toward -Z and opens toward +X.
pub fn open_side(dir: u8) -> u8 {
    // +X(0) -> -Z(3) -> -X(2) -> +Z(1): the open side is the one a quarter turn clockwise of the rise (-Z -> +X)
    const ORDER: [u8; 4] = [0, 3, 2, 1];
    let i = ORDER.iter().position(|&d| d == dir & 3).unwrap();
    ORDER[(i + 3) % 4]
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
    /// The flight up to the second floor, if there is one.
    pub stairs: Option<Stairs>,
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
        stairs: g.stairs.map(|(lo, hi)| {
            let (a, b) = (tp(lo), tp(hi));
            Stairs { min: a.min(b), max: a.max(b), dir: rot_dir(3, rot) }
        }),
        steps: None,
    }
}

/// A drop below this from a landing to the terrain is a plain step; anything more needs a stair.
const STEP_UP: f32 = 0.4;

/// On a sloping lot the door side of a house can stand a metre or more above the terrain (the floor sits at the highest
/// point of the footprint). Run a flight of steps from the landing's edge down to the ground: a mesh and a solid block (to walk
/// up, step by step) for each tread. Without it the door is out of reach for anyone who cannot jump that high, and bots never jump on purpose.
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
    // the head of the flight is level with the landing and rises toward -Z (toward the door) in the building's frame
    let steps = flight_steps(cx - hw, cx + hw, ez, ez + len, base_l - 0.15, base_l, top_l, n);
    let mut mb = MeshBuilder::new();
    flight_mesh(&mut mb, &steps, 0x96958f, 0xb4b3ad, mat::STONE);
    placed.mesh.append(&mb.finish(), m);
    for s in &steps {
        let (a, b) = (m.transform_point3(s.min), m.transform_point3(s.max));
        placed.cols.push(Collider::aabb_box(a.min(b), a.max(b), tag));
    }
    let (a, b) = (m.transform_point3(Vec3::new(cx - hw, base_l, ez)), m.transform_point3(Vec3::new(cx + hw, top_l, ez + len)));
    placed.steps = Some(Aabb::new(a.min(b), a.max(b)));
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
            // (after the others, so they keep drawing the same numbers from `r`)
            ("city tower", gen_tower(&mut r, 11.0, 10.0, 3, &st)),
            ("warehouse", gen_warehouse(&mut r, 22.0, 16.0)),
            ("container", gen_container(0x2f6fb0)),
            ("smokestack", gen_smokestack(30.0)),
            ("wrecks", gen_wreck_stack(&mut r, 3)),
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

    /// Total area of the upward-facing triangles of `m` that lie flat at height `y`.
    fn top_area_at(m: &MeshData, y: f32) -> f32 {
        let mut area = 0.0;
        for t in m.idx.chunks(3) {
            let v: Vec<Vec3> = t.iter().map(|&i| Vec3::from(m.verts[i as usize].pos)).collect();
            let n = (v[1] - v[0]).cross(v[2] - v[0]);
            if v.iter().all(|p| (p.y - y).abs() < 0.005) && n.y > 0.0 {
                area += n.length() * 0.5;
            }
        }
        area
    }

    #[test]
    fn the_band_between_floors_does_not_cover_the_upper_floor() {
        // the trim band around the outside of a two-story house once was one box across the whole footprint, which put a
        // plain slab of trim colour 12 cm above the boards of the upper floor
        let mut r = Rng::new(5);
        let st = Style { floors: 2, ..random_style(&mut r, 2) };
        let (w, d) = (10.0, 11.0);
        let g = gen_house(&mut r, w, d, &st);
        let band_top = FLOOR_Y + STORY_H + 0.12;
        let boards = top_area_at(&g.mesh, FLOOR_Y + STORY_H);
        let band = top_area_at(&g.mesh, band_top);
        assert!(boards > 30.0, "the upper floor shows its boards: {boards} m2");
        assert!(band < 4.0, "the trim band is a thin ring, not a floor: {band} m2");
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

    // ----- city towers, warehouses and industrial decor

    use crate::game::actor::{HEIGHT, RADIUS};
    use crate::game::env::STEP_HEIGHT;
    use crate::game::movement::SNAP_DOWN;

    /// Towers of 2 to 5 storeys on a few footprints, with the number of storeys asked for and the half footprint.
    fn towers() -> Vec<(u32, f32, f32, Geom)> {
        let mut r = Rng::new(21);
        let mut out = vec![];
        for floors in 2..=6u32 {
            for (w, d) in [(8.5f32, 8.5f32), (11.0, 10.0), (13.0, 12.5), (9.5, 13.0), (12.0, 9.0)] {
                let st = random_style(&mut r, floors);
                out.push((floors, w / 2.0, d / 2.0, gen_tower(&mut r, w, d, floors, &st)));
            }
        }
        out
    }

    /// Height of the floor of storey `fl` of a tower (the roof is storey `floors`).
    fn storey_y(fl: u32) -> f32 {
        FLOOR_Y + fl as f32 * STORY_H
    }

    /// Walk a body the size of a player (RADIUS x HEIGHT) along the (x, z) waypoints of `path` from `start`, the way the movement
    /// code moves one: in short strides, standing on the highest collider top within STEP_HEIGHT of its feet (the terrain is y = 0).
    /// It fails if a solid pushes it back (a wall, a rail, a step that is too high), if it drops more than SNAP_DOWN (it walked into a
    /// hole), or if anything hangs less than 2 m above its feet. Returns where it ended up.
    fn walk(cols: &[Collider], start: Vec3, path: &[Vec2]) -> Result<Vec3, String> {
        let mut p = start;
        for &to in path {
            loop {
                let d = to - Vec2::new(p.x, p.z);
                let l = d.length();
                if l < 1e-4 {
                    break;
                }
                let c = Vec2::new(p.x, p.z) + d * (l.min(0.05) / l);
                if let Some(col) = cols.iter().find(|col| col.shape.push_out(c, RADIUS, p.y, p.y + HEIGHT, STEP_HEIGHT).is_some_and(|v| v.length() > 1e-3)) {
                    return Err(format!("blocked at {c:?} (feet {:.2}) by {:?}", p.y, col.shape));
                }
                let g = cols.iter().filter_map(|col| col.shape.top_at(c.x, c.y)).filter(|&t| t <= p.y + STEP_HEIGHT).fold(0.0f32, f32::max);
                if g < p.y - SNAP_DOWN {
                    return Err(format!("fell at {c:?} from {:.2} to {g:.2}", p.y));
                }
                if let Some(col) = cols.iter().find(|col| col.shape.top_at(c.x, c.y).is_some() && col.shape.bottom() > g + 0.05 && col.shape.bottom() < g + 2.0) {
                    return Err(format!("less than 2 m of head room at {c:?} (feet {g:.2}) under {:?}", col.shape));
                }
                p = Vec3::new(c.x, g, c.y);
            }
        }
        Ok(p)
    }

    /// The way from a tower's door to its roof: in, across the front landing to the foot of the first flight, then up every flight in
    /// turn, turning on the landings at its ends, and out from the hatch onto the open roof.
    fn tower_route(g: &Geom, flights: &[Flight]) -> Vec<Vec2> {
        let (back, front) = (flights[0].z_back - 0.6, flights[0].z_front + 0.57);
        let lane = |f: &Flight| (f.x0 + f.x1) / 2.0;
        let z_in = front.max(g.door_in.z);
        let mut path = vec![Vec2::new(g.door_in.x, g.door_in.z), Vec2::new(g.door_in.x, z_in), Vec2::new(lane(&flights[0]), z_in)];
        for f in flights {
            let (foot, head) = if f.to_back { (front, back) } else { (back, front) };
            path.push(Vec2::new(lane(f), foot));
            path.push(Vec2::new(lane(f), head));
        }
        let end = *path.last().unwrap();
        path.push(Vec2::new(g.door_in.x, end.y));
        path
    }

    #[test]
    fn a_tower_can_be_climbed_from_the_street_to_the_roof() {
        for (floors, hx, hz, g) in towers() {
            let flights = tower_flights(hx, hz, floors);
            assert_eq!(flights.len(), floors as usize);
            let top = walk(&g.cols, g.door_out, &tower_route(&g, &flights)).unwrap_or_else(|e| panic!("{floors} storeys, {hx}x{hz}: {e}"));
            assert!((top.y - storey_y(floors)).abs() < 0.01, "{floors} storeys, {hx}x{hz}: walked out onto the roof, y {}", top.y);
            // the bounds handed to the game are those of the first flight, which rises toward -Z like a house's
            let (lo, hi) = g.stairs.expect("a tower has stairs");
            assert!((lo.y - FLOOR_Y).abs() < 1e-4 && (hi.y - storey_y(1)).abs() < 1e-4 && (lo.z - flights[0].z_back).abs() < 1e-4 && flights[0].to_back);
        }
    }

    #[test]
    fn every_flight_of_a_tower_is_made_of_low_steps_with_head_room_over_them() {
        for (floors, hx, hz, g) in towers() {
            let name = format!("{floors} storeys, {hx}x{hz}");
            for (i, f) in tower_flights(hx, hz, floors).iter().enumerate() {
                let mut steps: Vec<Aabb> = g
                    .cols
                    .iter()
                    .map(|c| c.shape.aabb())
                    .filter(|bb| bb.min.x >= f.x0 - 0.01 && bb.max.x <= f.x1 + 0.01 && bb.min.z >= f.z_back - 0.01 && bb.max.z <= f.z_front + 0.01)
                    .filter(|bb| (bb.min.y - f.y_low).abs() < 0.01 && bb.max.y <= f.y_high + 0.01)
                    .collect();
                assert_eq!(steps.len(), STAIR_STEPS as usize, "{name}: flight {i}");
                steps.sort_by(|a, b| a.max.y.total_cmp(&b.max.y));
                let mut last = f.y_low;
                for s in &steps {
                    let rise = s.max.y - last;
                    assert!(rise > 0.05 && rise <= STEP_HEIGHT, "{name}: flight {i} has a rise of {rise}");
                    last = s.max.y;
                    // nothing over the tread for 2 m (the steps beside it only touch its edges)
                    let (x0, x1, z0, z1) = (s.min.x + 0.02, s.max.x - 0.02, s.min.z + 0.02, s.max.z - 0.02);
                    for c in &g.cols {
                        let bb = c.shape.aabb();
                        let over = bb.min.x < x1 && bb.max.x > x0 && bb.min.z < z1 && bb.max.z > z0;
                        assert!(
                            !(over && bb.max.y > s.max.y + 0.01 && bb.min.y < s.max.y + 2.0),
                            "{name}: flight {i}, step at {:.2}: only {:.2} m of head room under {:?}",
                            s.max.y,
                            bb.min.y - s.max.y,
                            c.shape
                        );
                    }
                    // a flight above the ground floor stands on solid slab
                    if i > 0 {
                        let (cx, cz) = ((s.min.x + s.max.x) / 2.0, (s.min.z + s.max.z) / 2.0);
                        let slab = g.cols.iter().any(|c| c.shape.aabb().max.y < f.y_low + 0.01 && c.shape.top_at(cx, cz).is_some_and(|t| (t - f.y_low).abs() < 0.01));
                        assert!(slab, "{name}: flight {i} is not standing on a slab at ({cx:.2}, {cz:.2})");
                    }
                }
                assert!((last - f.y_high).abs() < 0.01, "{name}: flight {i} ends level with the floor above");
                // the slab it arrives at is open above it
                let (hx0, hx1, hz0, hz1) = f.hole();
                let (cx, cz) = ((hx0 + hx1) / 2.0, (hz0 + hz1) / 2.0);
                assert!(!g.cols.iter().any(|c| c.shape.top_at(cx, cz).is_some_and(|t| (t - f.y_high).abs() < 0.01)), "{name}: flight {i} has no opening above it");
            }
        }
    }

    #[test]
    fn a_tower_keeps_to_its_footprint_and_has_loot_on_every_floor() {
        for (floors, hx, hz, g) in towers() {
            let name = format!("{floors} storeys, {hx}x{hz}");
            assert_eq!((g.half.x, g.half.y), (hx, hz));
            assert!(g.height > storey_y(floors) + PARAPET_H);
            assert!(g.mesh.verts.iter().all(|v| v.pos.iter().chain(&v.nrm).all(|c| c.is_finite())), "{name}: NaN");
            assert!(g.mesh.verts.len() < 20_000, "{name}: {} verts", g.mesh.verts.len());
            for c in &g.cols {
                let bb = c.shape.aabb();
                assert!(bb.min.x >= -hx - 0.15 && bb.max.x <= hx + 0.15 && bb.min.z >= -hz - 0.15 && bb.max.z <= hz + 0.15, "{name}: collider outside the footprint: {:?}", c.shape);
                assert!(bb.max.y <= g.height + 0.01, "{name}: collider above the roof: {:?}", c.shape);
            }
            // every spot lies indoors (or on the roof) on a floor, with nothing in it
            let lies_clear = |p: Vec3, rx: f32, rz: f32| {
                let ground = g.cols.iter().filter_map(|c| c.shape.top_at(p.x, p.z)).filter(|&t| t <= p.y + 0.01).fold(f32::MIN, f32::max);
                let free = !g.cols.iter().any(|c| {
                    let bb = c.shape.aabb();
                    bb.min.x < p.x + rx && bb.max.x > p.x - rx && bb.min.z < p.z + rz && bb.max.z > p.z - rz && bb.min.y < p.y + 0.5 && bb.max.y > p.y + 0.05
                });
                (ground - p.y).abs() < 0.01 && free
            };
            let inside = |p: Vec3| p.x.abs() < hx - WALL_T && p.z.abs() < hz - WALL_T;
            for &l in &g.loot {
                assert!(inside(l) && lies_clear(l, 0.25, 0.25), "{name}: loot spot {l:?} is not on clear floor indoors");
            }
            for &(c, yaw) in &g.chests {
                let (rx, rz) = if (yaw.abs() - FRAC_PI_2).abs() < 0.1 { (0.35, 0.5) } else { (0.5, 0.35) };
                assert!(inside(c) && lies_clear(c, rx, rz) && c.y > FLOOR_Y + 1.0, "{name}: chest at {c:?} is not clear on an upper floor");
            }
            for fl in 0..=floors {
                let n = g.loot.iter().filter(|l| (l.y - storey_y(fl)).abs() < 0.01).count();
                let want = if fl == floors { 1..=1 } else { 1..=2 };
                assert!(want.contains(&n), "{name}: {n} loot spots on storey {fl}");
            }
            assert_eq!(g.loot.len(), g.loot.iter().filter(|l| (0..=floors).any(|fl| (l.y - storey_y(fl)).abs() < 0.01)).count(), "{name}: loot off the floors");
        }
        // over many towers, some upper floors get a chest and none of the ground floors does
        let chests: usize = towers().iter().map(|t| t.3.chests.len()).sum();
        assert!(chests > 3, "{chests} chests in the towers");
    }

    #[test]
    fn a_towers_front_door_is_open_and_its_walls_are_not() {
        for (floors, hx, hz, g) in towers() {
            let name = format!("{floors} storeys, {hx}x{hz}");
            assert!(g.door_out.z > hz && g.door_in.z < hz - WALL_T, "{name}: the door is on the +Z side");
            let end = walk(&g.cols, g.door_out, &[Vec2::new(g.door_in.x, g.door_in.z)]).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!((end.y - FLOOR_Y).abs() < 0.01, "{name}: walked in onto the ground floor, y {}", end.y);
            // a ray at chest height through the doorway reaches the door_in point; beside the door, at knee height, the wall stops it
            let dir = Vec3::NEG_Z;
            let reach = g.door_out.z - g.door_in.z;
            let first_hit = |o: Vec3| g.cols.iter().filter_map(|c| c.shape.raycast(o, dir, 30.0)).map(|h| h.0).fold(f32::MAX, f32::min);
            assert!(first_hit(Vec3::new(g.door_out.x, FLOOR_Y + 1.3, g.door_out.z)) > reach, "{name}: something blocks the doorway");
            for dx in [-1.4f32, 1.4] {
                assert!(first_hit(Vec3::new(g.door_out.x + dx, FLOOR_Y + 0.4, g.door_out.z)) < reach, "{name}: the front wall beside the door must be solid");
            }
            let e = g.entry.expect("an entrance for the steps on sloping ground");
            assert!((e.edge.x - g.door_out.x).abs() < 1e-4 && e.edge.z >= hz);
        }
        // the number of storeys is clamped
        let mut r = Rng::new(4);
        let st = random_style(&mut r, 2);
        assert_eq!(tower_storeys(&gen_tower(&mut r, 10.0, 10.0, 0, &st)), 2);
        assert_eq!(tower_storeys(&gen_tower(&mut r, 10.0, 10.0, 9, &st)), 6);
    }

    #[test]
    fn the_furniture_of_a_tower_stands_clear_of_itself() {
        // (more towers than the others look at: a sofa and a cabinet once met in a corner now and then)
        let mut r = Rng::new(99);
        let mut pieces = 0;
        for floors in 2..=6u32 {
            for (w, d) in [(8.5f32, 8.5f32), (11.0, 10.0), (13.0, 12.5), (9.5, 13.0), (12.0, 9.0)] {
                for _ in 0..6 {
                    let st = random_style(&mut r, floors);
                    let g = gen_tower(&mut r, w, d, floors, &st);
                    let (hx, hz) = (g.half.x, g.half.y);
                    // low solids standing on a floor, between the stairwell and the outer walls
                    let room_x0 = tower_flights(hx, hz, floors)[1].x1 + 0.8;
                    let furniture: Vec<Aabb> = g
                        .cols
                        .iter()
                        .map(|c| c.shape.aabb())
                        .filter(|bb| bb.min.x > room_x0 - 0.01 && bb.max.x < hx - WALL_T + 0.01 && bb.min.z > -hz + WALL_T - 0.01 && bb.max.z < hz - WALL_T + 0.01)
                        .filter(|bb| bb.max.y - bb.min.y < 1.0 && (0..floors).any(|fl| (bb.min.y - storey_y(fl)).abs() < 0.01))
                        .collect();
                    pieces += furniture.len();
                    for (i, a) in furniture.iter().enumerate() {
                        for b in &furniture[i + 1..] {
                            assert!((a.max.min(b.max) - a.min.max(b.min)).min_element() <= 0.01, "{floors} storeys, {w}x{d}: {a:?} and {b:?} stand in each other");
                        }
                    }
                }
            }
        }
        assert!(pieces > 300, "{pieces} pieces of furniture in 150 towers");
    }

    #[test]
    fn a_towers_windows_are_not_filled_by_its_stairs() {
        // the steps are solid down to the floor, so a window in the wall a flight climbs along would be plugged by them: a ray at chest
        // height that gets into a window opening must carry on well into the building
        for (floors, hx, hz, g) in towers() {
            let name = format!("{floors} storeys, {hx}x{hz}");
            for fl in 0..floors {
                let y = storey_y(fl) + 1.6;
                let mut open = 0;
                for (dir, span) in [(Vec3::X, hz), (Vec3::NEG_X, hz), (Vec3::Z, hx), (Vec3::NEG_Z, hx)] {
                    let mut u = -span + 0.3;
                    while u < span - 0.3 {
                        let o = if dir.x != 0.0 { Vec3::new(-dir.x * (hx + 1.0), y, u) } else { Vec3::new(u, y, -dir.z * (hz + 1.0)) };
                        let t = g.cols.iter().filter(|c| c.blocks_bullets).filter_map(|c| c.shape.raycast(o, dir, 40.0)).map(|h| h.0).fold(f32::MAX, f32::min);
                        if t > 1.02 {
                            open += 1;
                            assert!(t > 1.0 + WALL_T + 0.4, "{name}: storey {fl}: the window at {o:?} is filled by something right behind it ({t:.2})");
                        }
                        u += 0.1;
                    }
                }
                assert!(open > 20, "{name}: storey {fl} has windows");
            }
        }
    }

    /// The number of storeys of a tower, from its roof height.
    fn tower_storeys(g: &Geom) -> u32 {
        ((g.height - 1.05 - FLOOR_Y) / STORY_H).round() as u32
    }

    #[test]
    fn a_tower_turns_and_moves_with_place() {
        let mut r = Rng::new(8);
        let st = random_style(&mut r, 4);
        let g = gen_tower(&mut r, 11.0, 11.0, 4, &st);
        for rot in 0..4u8 {
            let c = Vec3::new(-60.0, 3.0, 25.0);
            let p = place(&g, c, rot, Tag::Building(3));
            let s = p.stairs.expect("stairs survive placement");
            assert_eq!(s.dir, rot_dir(3, rot));
            assert!((s.max.y - s.min.y - STORY_H).abs() < 1e-3);
            let fwd = Mat3::from_rotation_y(rot as f32 * FRAC_PI_2) * Vec3::Z;
            assert!((p.door_out - c).dot(fwd) > g.half.y, "rot {rot}: door not on the front side");
            assert_eq!(p.loot.len(), g.loot.len());
        }
    }

    #[test]
    fn a_warehouse_has_an_open_door_crates_and_loot() {
        let mut r = Rng::new(31);
        for (w, d) in [(16.0f32, 12.0f32), (22.0, 16.0), (28.0, 20.0), (18.0, 20.0), (40.0, 5.0)] {
            for _ in 0..4 {
                let g = gen_warehouse(&mut r, w, d);
                let (hx, hz) = (g.half.x, g.half.y);
                let name = format!("warehouse {w}x{d}");
                assert!((hx * 2.0 - w.clamp(16.0, 28.0)).abs() < 1e-4 && (hz * 2.0 - d.clamp(12.0, 20.0)).abs() < 1e-4, "{name}: {hx}x{hz}");
                assert!(g.height > 6.5 && g.height < 9.0, "{name}: {} m tall", g.height);
                assert!(!g.mesh.is_empty() && g.mesh.verts.len() < 20_000, "{name}: {} verts", g.mesh.verts.len());
                assert!(g.mesh.verts.iter().all(|v| v.pos.iter().chain(&v.nrm).all(|c| c.is_finite())), "{name}: NaN");
                for c in &g.cols {
                    let bb = c.shape.aabb();
                    assert!(bb.min.x >= -hx - 0.15 && bb.max.x <= hx + 0.15 && bb.min.z >= -hz - 0.15 && bb.max.z <= hz + 0.15, "{name}: collider outside the footprint: {:?}", c.shape);
                }
                // in through the big door and on down the aisle to the back
                let end = walk(&g.cols, g.door_out, &[Vec2::new(g.door_in.x, g.door_in.z), Vec2::new(g.door_in.x, -hz + 2.0)]).unwrap_or_else(|e| panic!("{name}: {e}"));
                assert!((end.y - FLOOR_Y).abs() < 0.01, "{name}: on the floor, y {}", end.y);
                // ... and in through the side door (found with rays at chest height along both side walls) and across to the aisle
                let gap = |sx: f32, z: f32| {
                    let o = Vec3::new(sx * (hx + 1.0), FLOOR_Y + 1.3, z);
                    g.cols.iter().filter_map(|c| c.shape.raycast(o, Vec3::new(-sx, 0.0, 0.0), 30.0)).map(|h| h.0).fold(f32::MAX, f32::min) > 1.0 + WALL_T + 0.05
                };
                let doors: Vec<(f32, Vec<f32>)> = [-1.0f32, 1.0].iter().map(|&sx| (sx, (0..(hz * 40.0) as i32).map(|k| -hz + 0.025 + k as f32 * 0.05).filter(|&z| gap(sx, z)).collect())).collect();
                let (sx, zs) = doors.iter().find(|d| !d.1.is_empty()).unwrap_or_else(|| panic!("{name}: no side door"));
                assert!(doors.iter().filter(|d| !d.1.is_empty()).count() == 1 && (zs[zs.len() - 1] - zs[0] - DOOR_W).abs() < 0.1, "{name}: one side door, {zs:?}");
                let zc = (zs[0] + zs[zs.len() - 1]) / 2.0;
                let route = [Vec2::new(sx * (hx - 1.0), zc), Vec2::new(g.door_in.x, zc)];
                let end = walk(&g.cols, Vec3::new(sx * (hx + 1.5), 0.0, zc), &route).unwrap_or_else(|e| panic!("{name}, side door: {e}"));
                assert!((end.y - FLOOR_Y).abs() < 0.01, "{name}: in by the side door, y {}", end.y);
                // crates to fight round: solid blocks standing on the floor, away from the walls
                let indoors = |bb: Aabb| bb.min.x > -hx + WALL_T + 0.1 && bb.max.x < hx - WALL_T - 0.1 && bb.min.z > -hz + WALL_T + 0.1 && bb.max.z < hz - WALL_T - 0.1;
                let crates = g.cols.iter().filter(|c| (c.shape.bottom() - FLOOR_Y).abs() < 0.01 && c.shape.aabb().max.y < FLOOR_Y + 2.0 && indoors(c.shape.aabb())).count();
                assert!(crates >= 3, "{name}: {crates} crates");
                assert!((3..=5).contains(&g.loot.len()), "{name}: {} loot spots", g.loot.len());
                for l in &g.loot {
                    assert!(l.x.abs() < hx - WALL_T && l.z.abs() < hz - WALL_T && (l.y - FLOOR_Y).abs() < 1e-4, "{name}: loot at {l:?}");
                    assert!(floor_clear_cols(&g.cols, l.x, l.z, l.y, 0.3), "{name}: loot at {l:?} lies inside a crate");
                }
                for (c, _) in &g.chests {
                    assert!(floor_clear_cols(&g.cols, c.x, c.z, c.y, 0.4), "{name}: chest at {c:?}");
                }
            }
        }
    }

    #[test]
    fn facade_dressing_stands_no_further_out_than_the_cornice_or_the_eaves() {
        // a building's mesh bounds decide where grass may grow, the nav grid round it and the bots' notion of indoors, so the pilasters,
        // awnings and window jambs of a tower and the plinth, downpipes and lamps of a warehouse keep within what was already there:
        // the coping of the parapet (and the door's canopy in front), the roof's overhang and its gutters
        for (floors, hx, hz, g) in towers() {
            let bb = g.mesh.bounds();
            assert!(bb.min.x >= -hx - 0.151 && bb.max.x <= hx + 0.151 && bb.min.z >= -hz - 0.151 && bb.max.z <= hz + 0.901, "{floors} storeys, {hx}x{hz}: {bb:?}");
        }
        let mut r = Rng::new(31);
        for (w, d) in [(16.0f32, 12.0f32), (22.0, 16.0), (28.0, 20.0), (18.0, 20.0)] {
            let g = gen_warehouse(&mut r, w, d);
            let (bb, hx, hz) = (g.mesh.bounds(), g.half.x, g.half.y);
            assert!(bb.min.x >= -hx - 0.41 && bb.max.x <= hx + 0.41 && bb.min.z >= -hz - 0.41 && bb.max.z <= hz + 0.41, "warehouse {w}x{d}: {bb:?}");
        }
    }

    /// True if no collider rises out of the floor at `y` within `r` of (x, z).
    fn floor_clear_cols(cols: &[Collider], x: f32, z: f32, y: f32, r: f32) -> bool {
        !cols.iter().any(|c| {
            let bb = c.shape.aabb();
            bb.min.x < x + r && bb.max.x > x - r && bb.min.z < z + r && bb.max.z > z - r && bb.min.y < y + 0.5 && bb.max.y > y + 0.05
        })
    }

    #[test]
    fn industrial_decor_is_solid_and_sane() {
        let mut r = Rng::new(9);
        let mut cases = vec![("container".to_string(), gen_container(0xc0392b)), ("smokestack".into(), gen_smokestack(24.0)), ("tall smokestack".into(), gen_smokestack(55.0))];
        for n in 1..=6 {
            cases.push((format!("{n} wrecks"), gen_wreck_stack(&mut r, n)));
        }
        for (name, g) in &cases {
            assert!(!g.mesh.is_empty() && !g.cols.is_empty(), "{name}: empty");
            assert!(g.mesh.verts.len() < 20_000, "{name}: {} verts", g.mesh.verts.len());
            assert!(g.mesh.verts.iter().all(|v| v.pos.iter().chain(&v.nrm).all(|c| c.is_finite())), "{name}: NaN");
            assert!(g.loot.is_empty() && g.chests.is_empty(), "{name}: decoration holds no loot");
            let bb = g.mesh.bounds();
            for c in &g.cols {
                let cb = c.shape.aabb();
                assert!(cb.min.is_finite() && cb.max.is_finite() && cb.max.y > cb.min.y + 0.1, "{name}: {:?}", c.shape);
                assert!(cb.max.x <= g.half.x + 0.01 && cb.min.x >= -g.half.x - 0.01 && cb.max.z <= g.half.y + 0.01 && cb.min.z >= -g.half.y - 0.01, "{name}: collider outside its half extents");
                assert!(cb.max.y <= g.height + 0.01 && cb.max.y <= bb.max.y + 0.05, "{name}: collider above the mesh");
            }
        }
        // the container is the size of a real one, and every wreck in a pile is a block of its own on the one below
        let cb = cases[0].1.cols[0].shape.aabb();
        assert!(((cb.max - cb.min) - Vec3::new(6.1, 2.6, 2.44)).abs().max_element() < 0.01, "{:?}", cb.max - cb.min);
        assert!(cases[2].1.height > cases[1].1.height + 25.0);
        // the smokestack's plinth is solid up to the top of its capping, so anyone up there stands on it rather than in it
        let stack = &cases[1].1;
        let capping = stack.mesh.verts.iter().filter(|v| v.pos[0] > 1.9 && v.pos[2] > 1.9).map(|v| v.pos[1]).fold(f32::MIN, f32::max);
        let top = stack.cols.iter().filter_map(|c| c.shape.top_at(2.0, 2.0)).fold(f32::MIN, f32::max);
        assert!((top - capping).abs() < 0.02, "the plinth is solid to {top}, its capping is at {capping}");
        let piles: Vec<&Geom> = cases[3..].iter().map(|c| &c.1).collect();
        for (k, g) in piles.iter().enumerate() {
            let n = (k + 1) as f32;
            assert_eq!(g.cols.len(), k + 1);
            assert!(g.height > 0.4 * n && g.height < 0.7 * n + 0.1, "{n} flattened wrecks pile {} m high", g.height);
            let mut tops: Vec<(f32, f32)> = g.cols.iter().map(|c| (c.shape.bottom(), c.shape.aabb().max.y)).collect();
            tops.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert!(tops.windows(2).all(|p| (p[1].0 - p[0].1).abs() < 0.05), "each wreck lies on the one below: {tops:?}");
        }
    }
}
