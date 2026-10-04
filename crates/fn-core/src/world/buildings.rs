//! Procedural buildings. Each generator returns a [`Geom`] in a local frame
//! (origin at the ground centre of the footprint, door on the +Z side), which is
//! then rotated by quarter turns and dropped into the world with [`place`].

use super::collision::*;
use crate::math::*;
use crate::mesh::*;
use crate::rng::Rng;
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
    // doorway (decorative arch on the front)
    b.bx(Vec3::new(-0.7, 0.0, 2.6), Vec3::new(0.7, 2.3, 3.2), 0x6a4a2e, mat::WOOD, false);
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
    // door
    b.bx(Vec3::new(-0.7, 0.0, 3.1), Vec3::new(0.7, 2.2, 3.5), 0x5a3c28, mat::WOOD, false);
    b.finish(Vec2::new(3.5, 3.5), total + 4.4, Vec3::new(0.0, 0.0, 6.0), Vec3::new(0.0, 0.0, 4.0))
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
    // platform + tank
    b.bx(Vec3::new(-3.0, leg_h, -3.0), Vec3::new(3.0, leg_h + 0.3, 3.0), 0x575961, mat::METAL, true);
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
    b.finish(Vec2::new(2.7, 2.7), 14.0, Vec3::new(0.0, 0.0, 4.5), Vec3::new(0.0, 0.0, 3.0))
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
    }
    b.finish(Vec2::new(hx, hz), top + 0.7, Vec3::new(0.0, 0.0, 6.0), Vec3::new(0.0, 0.0, 3.0))
}

pub fn gen_fountain() -> Geom {
    let mut b = B::new();
    b.cyl(Vec3::new(0.0, -0.5, 0.0), 3.2, 1.0, 0xbfc3c8, mat::STONE, true);
    b.cyl(Vec3::new(0.0, 0.5, 0.0), 2.6, 0.05, 0x3db4e8, mat::EMISSIVE, false);
    b.cyl(Vec3::new(0.0, 0.5, 0.0), 0.5, 1.8, 0xd5d8dc, mat::STONE, false);
    b.cyl(Vec3::new(0.0, 2.2, 0.0), 1.3, 0.2, 0xd5d8dc, mat::STONE, false);
    b.cyl(Vec3::new(0.0, 2.4, 0.0), 1.1, 0.05, 0x3db4e8, mat::EMISSIVE, false);
    b.cyl(Vec3::new(0.0, 2.4, 0.0), 0.2, 1.0, 0xd5d8dc, mat::STONE, false);
    b.finish(Vec2::new(3.3, 3.3), 3.6, Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 4.0))
}

pub fn gen_well() -> Geom {
    let mut b = B::new();
    b.mb.mat(mat::STONE).hex(0x9a9a96);
    b.mb.cylinder(Vec3::new(0.0, -0.5, 0.0), 1.1, 1.1, 1.5, 12, true, false);
    b.cols.push(Collider::cyl(0.0, 0.0, 1.1, -0.5, 1.0, Tag::Static));
    b.cyl(Vec3::new(0.0, 0.95, 0.0), 0.9, 0.05, 0x1d5f8a, mat::EMISSIVE, false);
    for sx in [-1.0f32, 1.0] {
        b.bx(Vec3::new(sx * 1.0 - 0.08, 0.5, -0.08), Vec3::new(sx * 1.0 + 0.08, 2.6, 0.08), 0x6a4a2e, mat::WOOD, false);
    }
    b.mb.mat(mat::SHINGLE).hex(0x8a4b3a);
    b.mb.wedge(Vec3::new(-1.3, 2.6, -1.0), Vec3::new(0.0, 3.3, 1.0), 0);
    b.mb.wedge(Vec3::new(0.0, 2.6, -1.0), Vec3::new(1.3, 3.3, 1.0), 2);
    b.finish(Vec2::new(1.3, 1.3), 3.3, Vec3::new(0.0, 0.0, 3.0), Vec3::new(0.0, 0.0, 2.0))
}

pub fn gen_lamp_post(col: u32) -> Geom {
    let mut b = B::new();
    b.mb.mat(mat::METAL).hex(0x33363d);
    b.mb.cylinder(Vec3::ZERO, 0.14, 0.09, 4.2, 6, true, false);
    b.mb.cylinder(Vec3::new(0.0, 0.0, 0.0), 0.22, 0.22, 0.25, 6, true, false);
    b.bx(Vec3::new(-0.04, 4.1, -0.04), Vec3::new(0.7, 4.2, 0.04), 0x33363d, mat::METAL, false);
    b.mb.mat(mat::EMISSIVE).hex(col);
    b.mb.box_min_max(Vec3::new(0.45, 3.82, -0.14), Vec3::new(0.85, 4.1, 0.14));
    b.cols.push(Collider::cyl(0.0, 0.0, 0.18, 0.0, 4.2, Tag::Static));
    b.finish(Vec2::new(0.3, 0.3), 4.3, Vec3::ZERO, Vec3::ZERO)
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
        ]
    }

    #[test]
    fn every_generator_produces_geometry() {
        for (name, g) in all_geoms() {
            assert!(!g.mesh.is_empty(), "{name} has no mesh");
            assert!(g.mesh.verts.iter().all(|v| v.pos.iter().all(|c| c.is_finite())), "{name} NaN");
            assert!(g.mesh.verts.len() < 20_000, "{name}: {} verts", g.mesh.verts.len());
            assert!(!g.cols.is_empty() || name == "fence", "{name} has no colliders");
            let bb = g.mesh.bounds();
            assert!(bb.max.y > if name == "dock" || name == "fence" { 0.15 } else { 1.0 }, "{name} too short");
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
