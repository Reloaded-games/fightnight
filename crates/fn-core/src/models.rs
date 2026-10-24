//! Hand-built models for everything that moves or can be picked up: characters
//! (as rigid parts posed by `game::rig`), weapons, consumables, chests, the battle
//! bus, the glider and the building pieces. All geometry is generated here.
//!
//! Conventions: Y is up, `-Z` is "forward" (the direction a character faces and a
//! weapon points), `+X` is the character's right-hand side.

use crate::math::*;
use crate::mesh::*;
use std::f32::consts::{FRAC_PI_2, PI};

/// Body proportions shared by the meshes and the rig.
pub mod dims {
    /// Pelvis centre above the feet while standing.
    pub const HIP_Y: f32 = 0.93;
    pub const HIP_X: f32 = 0.105;
    pub const HIP_DROP: f32 = 0.03;
    pub const THIGH: f32 = 0.43;
    pub const SHIN: f32 = 0.40;
    pub const SHOULDER_X: f32 = 0.25;
    /// Shoulder joint above the pelvis centre.
    pub const SHOULDER_Y: f32 = 0.50;
    pub const UPPER_ARM: f32 = 0.29;
    pub const FOREARM: f32 = 0.27;
    pub const NECK_Y: f32 = 0.575;
    pub const HEAD_C: f32 = 0.12;
}

fn rot_x(a: f32) -> Mat4 {
    Mat4::from_rotation_x(a)
}

/// Cylinder along -Z: `z0` (towards the grip) to `z1` (towards the muzzle, z1 < z0).
fn barrel(b: &mut MeshBuilder, x: f32, y: f32, z0: f32, z1: f32, r0: f32, r1: f32, seg: u32) {
    b.push_xf(Mat4::from_translation(Vec3::new(x, y, z0)) * rot_x(-FRAC_PI_2));
    b.cylinder(Vec3::ZERO, r0, r1, z0 - z1, seg, true, true);
    b.pop_xf();
}

fn grey(v: f32) -> Vec3 {
    Vec3::splat(v)
}

// =======================================================================================
// Characters
// =======================================================================================

const CHEST_C: Vec3 = Vec3::new(0.0, 0.345, 0.0);
const CHEST_R: Vec3 = Vec3::new(0.235, 0.21, 0.145);
const WAIST_C: Vec3 = Vec3::new(0.0, 0.11, 0.0);
const WAIST_R: Vec3 = Vec3::new(0.175, 0.14, 0.125);

/// Z of the front (-Z) surface of the torso at sideways offset `x` and height `y` (0 when outside the body).
fn torso_front_z(x: f32, y: f32) -> f32 {
    let ell = |c: Vec3, r: Vec3| {
        let (dx, dy) = ((x - c.x) / r.x, (y - c.y) / r.y);
        let k = 1.0 - dx * dx - dy * dy;
        if k > 0.0 {
            c.z - r.z * k.sqrt()
        } else {
            0.0
        }
    };
    ell(CHEST_C, CHEST_R).min(ell(WAIST_C, WAIST_R))
}

/// A box with rounded vertical edges: a plus-shaped pair of boxes with a cylinder in each corner.
fn rounded_prism(b: &mut MeshBuilder, c: Vec3, half: Vec3, r: f32, seg: u32) {
    let r = r.min(half.x).min(half.z) * 0.999;
    b.box_faces(c - Vec3::new(half.x, half.y, half.z - r), c + Vec3::new(half.x, half.y, half.z - r), 0b001111);
    b.box_faces(c - Vec3::new(half.x - r, half.y, half.z), c + Vec3::new(half.x - r, half.y, half.z), 0b111100);
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            b.cylinder(c + Vec3::new(sx * (half.x - r), -half.y, sz * (half.z - r)), r, r, half.y * 2.0, seg, false, true);
        }
    }
}

pub fn char_torso() -> MeshData {
    use dims::*;
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH);
    // waist and belly
    b.color(grey(0.94)).ao(0.7, 1.0);
    b.blob(WAIST_C, WAIST_R, 2, 0.0, 0, true);
    // chest and shoulders: one broad ellipsoid
    b.color(grey(1.0)).ao(0.8, 1.0);
    b.blob(CHEST_C, CHEST_R, 2, 0.0, 0, true);
    // shoulder caps
    b.color(grey(0.97));
    for s in [-1.0f32, 1.0] {
        b.sphere(Vec3::new(s * SHOULDER_X * 0.97, SHOULDER_Y, 0.0), 0.082, 1);
    }
    // collar band + neck
    b.color(grey(0.72)).ao(0.8, 1.0);
    b.cylinder(Vec3::new(0.0, SHOULDER_Y + 0.02, 0.0), 0.092, 0.075, 0.05, 12, false, true);
    b.mat(mat::SKIN).color(grey(1.0));
    b.cylinder(Vec3::new(0.0, SHOULDER_Y + 0.05, 0.0), 0.06, 0.055, NECK_Y - SHOULDER_Y - 0.03, 10, false, false);
    // chest pockets with flaps (a darker shade of the shirt)
    b.mat(mat::CLOTH).color(grey(0.8)).ao(0.8, 1.0);
    for s in [-1.0f32, 1.0] {
        let (x, y) = (s * 0.105, 0.285);
        let z = torso_front_z(x, y) - 0.004;
        b.push_xf(Mat4::from_translation(Vec3::new(x, y, z)) * rot_x(0.12));
        b.box_center(Vec3::ZERO, Vec3::new(0.05, 0.036, 0.011));
        b.pop_xf();
    }
    // zipper line down the front, with a pull tab
    b.mat(mat::METAL).tinted(false).hex(0x30333a).ao(1.0, 1.0);
    for k in 0..9 {
        let y = 0.07 + k as f32 * 0.043;
        b.box_center(Vec3::new(0.0, y, torso_front_z(0.0, y) - 0.004), Vec3::new(0.007, 0.021, 0.006));
    }
    b.hex(0xd9d4c4);
    b.box_center(Vec3::new(0.0, 0.47, torso_front_z(0.0, 0.47) - 0.008), Vec3::new(0.012, 0.016, 0.006));
    // pocket buttons
    b.hex(0xe9e4d4);
    for s in [-1.0f32, 1.0] {
        let (x, y) = (s * 0.105, 0.262);
        b.box_center(Vec3::new(x, y, torso_front_z(x, y) - 0.017), Vec3::new(0.009, 0.009, 0.004));
    }
    b.finish()
}

/// Accent-coloured trim worn on the torso: shoulder pads, a chest band and the collar. Tinted
/// with the outfit's accent colour so every character gets a second colour.
pub fn char_trim() -> MeshData {
    use dims::*;
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.8, 1.0);
    // shoulder pads: a cap with a darker rim
    for s in [-1.0f32, 1.0] {
        b.color(grey(1.0));
        b.blob(Vec3::new(s * (SHOULDER_X + 0.012), SHOULDER_Y + 0.045, 0.0), Vec3::new(0.088, 0.062, 0.095), 2, 0.0, 0, true);
        b.color(grey(0.74));
        b.push_xf(Mat4::from_translation(Vec3::new(s * (SHOULDER_X + 0.02), SHOULDER_Y + 0.0, 0.0)) * Mat4::from_rotation_z(-s * 0.35));
        b.cylinder(Vec3::new(0.0, -0.01, 0.0), 0.086, 0.09, 0.03, 12, false, false);
        b.pop_xf();
    }
    // chest band
    b.color(grey(0.95));
    b.push_xf(Mat4::from_scale(Vec3::new(1.0, 1.0, 0.64)));
    b.cylinder(Vec3::new(0.0, 0.215, 0.0), 0.222, 0.228, 0.05, 16, false, false);
    b.pop_xf();
    // collar, with a standing back
    b.color(grey(0.9));
    b.cylinder(Vec3::new(0.0, SHOULDER_Y + 0.0, 0.0), 0.097, 0.082, 0.06, 14, false, false);
    // a chest badge: a small diamond with a light centre
    let (bx, by) = (0.1f32, 0.375f32);
    let bz = torso_front_z(bx, by) - 0.006;
    b.color(grey(0.9));
    b.push_xf(Mat4::from_translation(Vec3::new(bx, by, bz)) * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_4));
    b.box_center(Vec3::ZERO, Vec3::new(0.026, 0.026, 0.008));
    b.pop_xf();
    b.tinted(false).mat(mat::FLAT).hex(0xfaf6ea);
    b.push_xf(Mat4::from_translation(Vec3::new(bx, by, bz - 0.006)) * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_4));
    b.box_center(Vec3::ZERO, Vec3::new(0.013, 0.013, 0.004));
    b.pop_xf();
    b.finish()
}

pub fn char_pelvis() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.7, 1.0);
    b.blob(Vec3::new(0.0, -0.04, 0.0), Vec3::new(0.2, 0.125, 0.125), 2, 0.0, 0, true);
    // belt
    b.color(grey(0.28)).ao(1.0, 1.0);
    b.push_xf(Mat4::from_scale(Vec3::new(1.0, 1.0, 0.68)));
    b.cylinder(Vec3::new(0.0, 0.03, 0.0), 0.19, 0.19, 0.055, 14, false, false);
    b.pop_xf();
    b.mat(mat::METAL).tinted(false).color(Vec3::new(0.86, 0.78, 0.5));
    b.box_center(Vec3::new(0.0, 0.058, -0.13), Vec3::new(0.04, 0.026, 0.009));
    b.color(Vec3::new(0.3, 0.28, 0.24));
    b.box_center(Vec3::new(0.0, 0.058, -0.1395), Vec3::new(0.022, 0.012, 0.003));
    // belt pouches on the hips (leather, a darker shade than the belt)
    b.mat(mat::CLOTH).hex(0x4a3a2c).ao(0.8, 1.0);
    for s in [-1.0f32, 1.0] {
        b.box_center(Vec3::new(s * 0.152, 0.0, -0.062), Vec3::new(0.034, 0.05, 0.036));
        b.hex(0x6a5338);
        b.box_center(Vec3::new(s * 0.152, 0.042, -0.062), Vec3::new(0.037, 0.014, 0.039));
        b.hex(0x4a3a2c);
    }
    // the back of the belt carries a small buckle strap
    b.color(grey(0.2));
    b.box_center(Vec3::new(0.0, 0.03, 0.128), Vec3::new(0.05, 0.026, 0.006));
    b.finish()
}

pub fn char_head() -> MeshData {
    use dims::*;
    let mut b = MeshBuilder::new();
    let c = Vec3::new(0.0, HEAD_C + 0.01, 0.0);
    b.mat(mat::SKIN).color(grey(1.0)).ao(0.85, 1.0);
    b.blob(c, Vec3::new(0.15, 0.165, 0.155), 3, 0.0, 0, true);
    // jaw / chin
    b.blob(c + Vec3::new(0.0, -0.06, -0.02), Vec3::new(0.115, 0.09, 0.12), 2, 0.0, 0, true);
    // nose and ears
    b.sphere(c + Vec3::new(0.0, -0.012, -0.152), 0.03, 1);
    b.sphere(c + Vec3::new(0.0, -0.034, -0.14), 0.017, 0);
    for s in [-1.0f32, 1.0] {
        b.sphere(c + Vec3::new(s * 0.152, -0.005, 0.01), 0.036, 1);
    }
    // rosy cheeks (a warm shade of whatever the skin is)
    b.color(Vec3::new(1.0, 0.82, 0.8)).ao(1.0, 1.0);
    for s in [-1.0f32, 1.0] {
        b.push_xf(Mat4::from_translation(c + Vec3::new(s * 0.088, -0.04, -0.128)) * Mat4::from_rotation_y(s * 0.55));
        b.blob(Vec3::ZERO, Vec3::new(0.028, 0.02, 0.012), 1, 0.0, 0, true);
        b.pop_xf();
    }
    // eyes (kept untinted so they stay white and dark whatever the skin tone)
    b.tinted(false).mat(mat::FLAT);
    for s in [-1.0f32, 1.0] {
        // white, a little rounder than a plain box: a wide box plus a taller inner one
        b.color(grey(0.99));
        b.box_center(c + Vec3::new(s * 0.062, 0.025, -0.1435), Vec3::new(0.034, 0.025, 0.012));
        b.box_center(c + Vec3::new(s * 0.062, 0.025, -0.1435), Vec3::new(0.028, 0.031, 0.0115));
        // iris, pupil and a glint
        b.color(Vec3::new(0.16, 0.3, 0.5));
        b.box_center(c + Vec3::new(s * 0.062 - s * 0.004, 0.021, -0.152), Vec3::new(0.019, 0.023, 0.007));
        b.color(Vec3::new(0.04, 0.05, 0.08));
        b.box_center(c + Vec3::new(s * 0.062 - s * 0.004, 0.021, -0.1578), Vec3::new(0.0095, 0.013, 0.004));
        b.color(grey(1.0));
        b.box_center(c + Vec3::new(s * 0.062 - s * 0.009, 0.03, -0.1612), Vec3::new(0.0045, 0.0045, 0.002));
        // upper lid line and brow, tilted so the outer end rises
        b.color(Vec3::new(0.12, 0.09, 0.08));
        b.box_center(c + Vec3::new(s * 0.062, 0.05, -0.1468), Vec3::new(0.037, 0.005, 0.006));
        b.push_xf(Mat4::from_translation(c + Vec3::new(s * 0.064, 0.074, -0.1465)) * Mat4::from_rotation_z(-s * 0.07));
        b.color(Vec3::new(0.22, 0.15, 0.1));
        b.box_center(Vec3::ZERO, Vec3::new(0.04, 0.0085, 0.008));
        b.pop_xf();
    }
    // smile: a shallow curve of small segments with a lighter lip line under it
    b.color(Vec3::new(0.5, 0.17, 0.17));
    for k in -3..=3i32 {
        let t = k as f32 / 3.0;
        b.box_center(c + Vec3::new(t * 0.036, -0.084 + t * t * 0.01, -0.1405), Vec3::new(0.0068, 0.0048, 0.008));
    }
    b.color(Vec3::new(0.86, 0.46, 0.42));
    b.box_center(c + Vec3::new(0.0, -0.095, -0.138), Vec3::new(0.014, 0.0035, 0.006));
    b.finish()
}

/// Part of an ellipsoid shell around `c`, open at the bottom. Azimuth `t` is 0 at the front (-Z) and grows toward
/// +X; each column runs from polar angle `from(t)` to `to(t)` (0 is straight up, PI/2 the equator). Used for hair
/// lines, caps and helmets whose lower rim follows the head.
#[allow(clippy::too_many_arguments)]
fn dome(b: &mut MeshBuilder, c: Vec3, r: Vec3, rings: u32, seg: u32, from: &dyn Fn(f32) -> f32, to: &dyn Fn(f32) -> f32, ao_lo: f32) {
    let mut ids: Vec<Vec<u32>> = Vec::new();
    for i in 0..=seg {
        let t = (i % seg) as f32 / seg as f32 * TAU_F;
        let (st, ct) = t.sin_cos();
        let (f0, f1) = (from(t), to(t));
        let mut col = Vec::new();
        for j in 0..=rings {
            let phi = f0 + (f1 - f0) * j as f32 / rings as f32;
            let (sp, cp) = phi.sin_cos();
            let d = Vec3::new(sp * st, cp, -sp * ct);
            let n = Vec3::new(d.x / r.x, d.y / r.y, d.z / r.z).normalize_or_zero();
            let ao = ao_lo + (1.0 - ao_lo) * ((cp * 0.5 + 0.5).clamp(0.0, 1.0));
            col.push(b.vert(c + d * r, n, ao));
        }
        ids.push(col);
    }
    for i in 0..seg as usize {
        for j in 0..rings as usize {
            b.quad(ids[i][j], ids[i + 1][j], ids[i + 1][j + 1], ids[i][j + 1]);
        }
    }
}

/// Head centre and size (what `char_head` is built from), for hair and hats to wrap.
const HEAD_CTR: Vec3 = Vec3::new(0.0, dims::HEAD_C + 0.01, 0.0);
const HEAD_RAD: Vec3 = Vec3::new(0.15, 0.165, 0.155);

/// A hairline: low at the back, high on the forehead.
fn hairline(front: f32, back: f32) -> impl Fn(f32) -> f32 {
    move |t: f32| {
        let k = (t * 0.5).sin().powi(2);
        front + (back - front) * k
    }
}

pub fn hair(style: u8) -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.7, 1.0);
    let c = HEAD_CTR;
    let shell = HEAD_RAD + Vec3::splat(0.012);
    let top = |_t: f32| 0.0;
    match style {
        1 => {
            // short crop: a close shell with a low hairline, a swept fringe, sideburns and a tuft
            b.color(grey(1.0));
            dome(&mut b, c, shell, 7, 22, &top, &hairline(0.95, 1.9), 0.7);
            b.color(grey(0.93));
            for (k, x) in [-0.075f32, -0.025, 0.03, 0.082].iter().enumerate() {
                b.push_xf(Mat4::from_translation(c + Vec3::new(*x, 0.105 - (k % 2) as f32 * 0.008, -0.114)) * Mat4::from_rotation_z(0.2 - k as f32 * 0.12));
                b.blob(Vec3::ZERO, Vec3::new(0.046, 0.034, 0.03), 1, 0.08, 11 + k as u32, true);
                b.pop_xf();
            }
            b.color(grey(0.86));
            for s in [-1.0f32, 1.0] {
                b.blob(c + Vec3::new(s * 0.148, 0.01, -0.03), Vec3::new(0.016, 0.05, 0.038), 1, 0.05, 21, true);
            }
            b.color(grey(1.0));
            b.blob(c + Vec3::new(0.0, 0.155, -0.02), Vec3::new(0.07, 0.034, 0.09), 1, 0.12, 23, true);
        }
        2 => {
            // long hair falling over the shoulders, with a swept fringe
            b.color(grey(1.0));
            dome(&mut b, c, shell, 7, 22, &top, &hairline(0.95, 2.05), 0.7);
            b.color(grey(0.9));
            b.blob(c + Vec3::new(0.0, -0.1, 0.07), Vec3::new(0.135, 0.2, 0.085), 2, 0.04, 9, true);
            for s in [-1.0f32, 1.0] {
                b.color(grey(0.85));
                b.blob(c + Vec3::new(s * 0.128, -0.06, 0.0), Vec3::new(0.032, 0.14, 0.07), 1, 0.04, 14, true);
                b.color(grey(1.0));
                b.push_xf(Mat4::from_translation(c + Vec3::new(s * 0.06, 0.105, -0.114)) * Mat4::from_rotation_z(s * 0.5));
                b.blob(Vec3::ZERO, Vec3::new(0.068, 0.034, 0.03), 1, 0.06, 17, true);
                b.pop_xf();
            }
        }
        3 => {
            // ponytail: swept-back cap, a tie in a contrasting colour and a swishing tail
            b.color(grey(1.0));
            dome(&mut b, c, shell, 7, 22, &top, &hairline(0.9, 1.85), 0.7);
            b.color(grey(0.95));
            b.push_xf(Mat4::from_translation(c + Vec3::new(0.0, 0.108, -0.114)));
            b.blob(Vec3::ZERO, Vec3::new(0.105, 0.032, 0.03), 1, 0.06, 15, true);
            b.pop_xf();
            let base = c + Vec3::new(0.0, 0.03, 0.15);
            b.color(grey(1.0));
            b.sphere(base, 0.05, 1);
            let mut p = base + Vec3::new(0.0, -0.01, 0.04);
            for (k, (dy, dz, r)) in [(-0.04f32, 0.045f32, 0.048f32), (-0.07, 0.03, 0.044), (-0.085, 0.01, 0.038), (-0.075, -0.015, 0.03)].iter().enumerate() {
                p += Vec3::new(0.0, *dy, *dz);
                b.color(grey(1.0 - k as f32 * 0.05));
                b.blob(p, Vec3::new(*r, *r * 1.35, *r), 1, 0.05, 31 + k as u32, true);
            }
            // tie
            b.tinted(false).mat(mat::FLAT).hex(0xf2f0e8);
            b.push_xf(Mat4::from_translation(base + Vec3::new(0.0, -0.01, 0.045)) * rot_x(0.3));
            b.cylinder(Vec3::new(0.0, -0.015, 0.0), 0.034, 0.034, 0.03, 8, false, false);
            b.pop_xf();
        }
        _ => {}
    }
    b.finish()
}

pub fn headgear(kind: u8) -> MeshData {
    let mut b = MeshBuilder::new();
    let c = HEAD_CTR;
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.7, 1.0);
    let top = |_t: f32| 0.0;
    match kind {
        1 => {
            // baseball cap: a six-panel dome, a curved bill (three flat segments), a button and a logo patch
            let shell = HEAD_RAD + Vec3::new(0.016, 0.02, 0.018);
            dome(&mut b, c, shell, 7, 24, &top, &hairline(1.0, 1.5), 0.7);
            b.color(grey(0.82));
            for (a, x) in [(-0.62f32, -0.085f32), (0.0, 0.0), (0.62, 0.085)] {
                b.push_xf(Mat4::from_translation(c + Vec3::new(x, 0.088, -0.168 - a.abs() * 0.04)) * Mat4::from_rotation_y(a) * rot_x(0.14));
                b.box_center(Vec3::ZERO, Vec3::new(0.055, 0.007, 0.07));
                b.pop_xf();
            }
            b.color(grey(0.6)).spec(0.1);
            b.sphere(c + Vec3::new(0.0, 0.192, 0.0), 0.02, 1);
            // a light logo patch on the front
            b.tinted(false).mat(mat::FLAT).hex(0xf4f0e2);
            b.push_xf(Mat4::from_translation(c + Vec3::new(0.0, 0.1, -0.1595)) * rot_x(-0.5));
            b.box_center(Vec3::ZERO, Vec3::new(0.03, 0.022, 0.004));
            b.pop_xf();
        }
        2 => {
            // beanie: a snug dome, a ribbed turn-up band with a light stripe, a fluffy bobble
            let shell = HEAD_RAD + Vec3::new(0.014, 0.022, 0.016);
            let edge = hairline(1.22, 1.72);
            dome(&mut b, c, shell, 7, 24, &top, &|t| edge(t) - 0.3, 0.7);
            b.color(grey(0.72));
            let band = shell + Vec3::splat(0.008);
            dome(&mut b, c, band, 3, 24, &|t| edge(t) - 0.3, &edge, 0.7);
            // ribs
            b.color(grey(0.6));
            for k in 0..16 {
                let t = k as f32 / 16.0 * TAU_F;
                let phi = edge(t) - 0.15;
                let (sp, cp) = phi.sin_cos();
                let p = c + Vec3::new(sp * t.sin() * band.x, cp * band.y, -sp * t.cos() * band.z);
                b.push_xf(Mat4::from_translation(p) * Mat4::from_rotation_y(-t));
                b.box_center(Vec3::new(0.0, 0.0, -0.004), Vec3::new(0.005, 0.034, 0.005));
                b.pop_xf();
            }
            b.mat(mat::CLOTH).tinted(true).color(grey(1.0));
            b.blob(c + Vec3::new(0.0, 0.215, 0.0), Vec3::splat(0.048), 1, 0.14, 5, false);
        }
        3 => {
            // helmet: a metal shell with a ridge, a brow plate, ear guards, a rim band and a chin strap
            b.mat(mat::METAL).spec(0.7);
            let shell = HEAD_RAD + Vec3::new(0.024, 0.026, 0.026);
            dome(&mut b, c, shell, 8, 26, &top, &hairline(0.82, 1.95), 0.65);
            // a crest along the top, following the curve of the shell
            b.color(grey(0.72));
            for k in 0..9 {
                let phi = -1.3 + k as f32 * 0.29;
                let p = c + Vec3::new(0.0, phi.cos() * (shell.y + 0.006), -phi.sin() * (shell.z + 0.006));
                b.push_xf(Mat4::from_translation(p) * rot_x(-phi));
                b.box_center(Vec3::ZERO, Vec3::new(0.017, 0.012, 0.0345));
                b.pop_xf();
            }
            b.color(grey(0.62));
            b.push_xf(Mat4::from_translation(c + Vec3::new(0.0, 0.068, -0.172)) * rot_x(0.22));
            b.box_center(Vec3::ZERO, Vec3::new(0.1, 0.008, 0.05));
            b.pop_xf();
            // rounded ear guards with a rivet
            for s in [-1.0f32, 1.0] {
                b.color(grey(0.82));
                b.blob(c + Vec3::new(s * 0.166, -0.005, 0.016), Vec3::new(0.022, 0.05, 0.056), 1, 0.0, 0, true);
                b.tinted(false).mat(mat::METAL).hex(0xd9d9dc);
                b.sphere(c + Vec3::new(s * 0.186, 0.005, 0.016), 0.011, 1);
                b.mat(mat::METAL).tinted(true);
            }
            // dark chin strap
            b.tinted(false).mat(mat::CLOTH).hex(0x2a2c31);
            for s in [-1.0f32, 1.0] {
                b.box_center(c + Vec3::new(s * 0.158, -0.082, -0.01), Vec3::new(0.008, 0.075, 0.012));
            }
        }
        4 => {
            // wide hat: a dished brim with an upturned rim, a pinched crown, a band and a feather
            b.cylinder(c + Vec3::new(0.0, 0.07, 0.0), 0.2, 0.27, 0.012, 18, true, true);
            b.cylinder(c + Vec3::new(0.0, 0.074, 0.0), 0.27, 0.285, 0.026, 18, false, false);
            b.cylinder(c + Vec3::new(0.0, 0.075, 0.0), 0.15, 0.125, 0.14, 14, false, true);
            b.color(grey(0.8));
            b.blob(c + Vec3::new(0.0, 0.215, 0.0), Vec3::new(0.12, 0.022, 0.12), 1, 0.0, 0, true);
            b.color(grey(0.55));
            b.cylinder(c + Vec3::new(0.0, 0.082, 0.0), 0.153, 0.151, 0.034, 14, false, false);
            // feather
            b.tinted(false).mat(mat::FLAT).hex(0xf4ede0);
            b.push_xf(Mat4::from_translation(c + Vec3::new(0.12, 0.1, -0.08)) * Mat4::from_rotation_z(-0.35) * rot_x(0.2));
            b.blob(Vec3::new(0.0, 0.08, 0.0), Vec3::new(0.012, 0.085, 0.03), 1, 0.0, 0, true);
            b.pop_xf();
        }
        _ => {}
    }
    b.finish()
}

pub fn char_arm_up() -> MeshData {
    use dims::*;
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.75, 1.0);
    b.cylinder(Vec3::new(0.0, -UPPER_ARM, 0.0), 0.056, 0.068, UPPER_ARM, 10, false, false);
    b.sphere(Vec3::new(0.0, 0.0, 0.0), 0.07, 1);
    b.sphere(Vec3::new(0.0, -UPPER_ARM, 0.0), 0.058, 2);
    // sleeve seam below the shoulder pad and a patch on the elbow
    b.color(grey(0.84));
    b.cylinder(Vec3::new(0.0, -0.1, 0.0), 0.0675, 0.0665, 0.014, 10, false, false);
    b.color(grey(0.78));
    b.blob(Vec3::new(0.0, -UPPER_ARM + 0.01, 0.045), Vec3::new(0.04, 0.045, 0.02), 1, 0.0, 0, true);
    b.finish()
}

pub fn char_arm_low() -> MeshData {
    use dims::*;
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.75, 1.0);
    b.cylinder(Vec3::new(0.0, -FOREARM, 0.0), 0.047, 0.058, FOREARM, 10, false, false);
    // cuff
    b.color(grey(0.72));
    b.cylinder(Vec3::new(0.0, -FOREARM, 0.0), 0.054, 0.054, 0.045, 10, false, true);
    // a dark strap just above the cuff
    b.tinted(false).hex(0x2b2e34);
    b.cylinder(Vec3::new(0.0, -FOREARM + 0.06, 0.0), 0.0535, 0.0545, 0.02, 10, false, false);
    b.finish()
}

pub fn char_hand() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::SKIN).color(grey(1.0)).ao(0.8, 1.0);
    // palm and a curled block of fingers with three dark grooves between them
    b.blob(Vec3::new(0.0, -0.045, 0.0), Vec3::new(0.05, 0.05, 0.04), 2, 0.0, 0, true);
    b.blob(Vec3::new(0.0, -0.088, -0.012), Vec3::new(0.047, 0.03, 0.02), 1, 0.0, 0, true);
    b.tinted(true).color(grey(0.72));
    for k in 0..3 {
        b.box_center(Vec3::new(-0.023 + k as f32 * 0.023, -0.092, -0.0305), Vec3::new(0.0014, 0.022, 0.0035));
    }
    // thumb pointing forward along the grip
    b.color(grey(1.0));
    b.push_xf(Mat4::from_translation(Vec3::new(0.0, -0.058, -0.04)) * rot_x(-0.5));
    b.blob(Vec3::ZERO, Vec3::new(0.0165, 0.0165, 0.034), 1, 0.0, 0, true);
    b.pop_xf();
    b.finish()
}

pub fn char_leg_up() -> MeshData {
    use dims::*;
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.7, 1.0);
    b.cylinder(Vec3::new(0.0, -THIGH, 0.0), 0.078, 0.098, THIGH, 12, false, false);
    b.sphere(Vec3::new(0.0, 0.0, 0.0), 0.098, 1);
    b.sphere(Vec3::new(0.0, -THIGH, 0.0), 0.08, 2);
    // a stitched seam down the front and a hip pocket flap
    b.color(grey(0.8)).ao(0.8, 1.0);
    b.box_center(Vec3::new(0.0, -0.2, -0.0865), Vec3::new(0.007, 0.15, 0.004));
    b.push_xf(Mat4::from_translation(Vec3::new(0.0, -0.1, -0.09)) * rot_x(0.05));
    b.box_center(Vec3::ZERO, Vec3::new(0.038, 0.032, 0.009));
    b.pop_xf();
    b.finish()
}

pub fn char_leg_low() -> MeshData {
    use dims::*;
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.7, 1.0);
    b.cylinder(Vec3::new(0.0, -SHIN, 0.0), 0.062, 0.078, SHIN, 12, false, false);
    // knee pad: a padded plate at the front, darker than the trousers
    b.color(grey(0.55)).ao(0.9, 1.0);
    b.blob(Vec3::new(0.0, -0.02, -0.058), Vec3::new(0.062, 0.058, 0.032), 2, 0.0, 0, true);
    // turn-up at the ankle
    b.color(grey(0.82)).ao(0.8, 1.0);
    b.cylinder(Vec3::new(0.0, -SHIN, 0.0), 0.0675, 0.0675, 0.04, 12, false, false);
    b.finish()
}

pub fn char_boot() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.6, 1.0);
    // shaft
    b.cylinder(Vec3::new(0.0, -0.065, 0.0), 0.083, 0.078, 0.145, 12, false, true);
    // foot
    b.blob(Vec3::new(0.0, -0.045, -0.05), Vec3::new(0.075, 0.055, 0.15), 2, 0.0, 0, true);
    // toe cap
    b.color(grey(0.8));
    b.blob(Vec3::new(0.0, -0.062, -0.158), Vec3::new(0.068, 0.04, 0.062), 1, 0.0, 0, true);
    // sole
    b.color(grey(0.3)).ao(1.0, 1.0);
    b.box_min_max(Vec3::new(-0.074, -0.098, -0.2), Vec3::new(0.074, -0.068, 0.09));
    // light rubber rim and laces (untinted)
    b.tinted(false).mat(mat::FLAT).hex(0xe9e4d6);
    b.box_min_max(Vec3::new(-0.0755, -0.075, -0.2015), Vec3::new(0.0755, -0.068, 0.0915));
    b.hex(0xf2eee2);
    for k in 0..3 {
        b.box_center(Vec3::new(0.0, -0.01 - k as f32 * 0.022, -0.0875 - k as f32 * 0.016), Vec3::new(0.026, 0.004, 0.005));
    }
    b.hex(0x2b2e34);
    b.box_center(Vec3::new(0.0, -0.012, 0.0795), Vec3::new(0.02, 0.03, 0.006));
    b.finish()
}

pub fn char_backpack() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.6, 1.0);
    // main body with rounded vertical edges
    rounded_prism(&mut b, Vec3::new(0.0, 0.3, 0.185), Vec3::new(0.14, 0.17, 0.074), 0.045, 8);
    // domed lid
    b.color(grey(0.9)).ao(0.8, 1.0);
    b.push_xf(Mat4::from_translation(Vec3::new(0.0, 0.47, 0.185)) * Mat4::from_rotation_z(FRAC_PI_2));
    b.cylinder(Vec3::new(0.0, -0.14, 0.0), 0.078, 0.078, 0.28, 10, true, true);
    b.pop_xf();
    // side pockets and a back pocket (a darker shade)
    b.color(grey(0.8)).ao(0.7, 1.0);
    for s in [-1.0f32, 1.0] {
        rounded_prism(&mut b, Vec3::new(s * 0.162, 0.2, 0.19), Vec3::new(0.026, 0.07, 0.048), 0.02, 6);
    }
    rounded_prism(&mut b, Vec3::new(0.0, 0.2, 0.268), Vec3::new(0.1, 0.075, 0.02), 0.016, 6);
    // cream stripes across the back and a zip line above the pocket
    b.tinted(false).mat(mat::FLAT).hex(0xf1e9d2).ao(1.0, 1.0);
    b.box_center(Vec3::new(0.0, 0.345, 0.2605), Vec3::new(0.1385, 0.011, 0.002));
    b.box_center(Vec3::new(0.0, 0.385, 0.2605), Vec3::new(0.1385, 0.011, 0.002));
    b.hex(0x2b2e34);
    b.box_center(Vec3::new(0.0, 0.282, 0.2895), Vec3::new(0.095, 0.004, 0.002));
    b.hex(0xd9d4c4);
    b.box_center(Vec3::new(0.03, 0.282, 0.291), Vec3::new(0.012, 0.009, 0.003));
    // rolled mat under the pack, with two straps
    b.mat(mat::CLOTH).hex(0x59606c);
    b.push_xf(Mat4::from_translation(Vec3::new(0.0, 0.07, 0.19)) * Mat4::from_rotation_z(FRAC_PI_2));
    b.cylinder(Vec3::new(0.0, -0.15, 0.0), 0.048, 0.048, 0.3, 10, true, true);
    b.pop_xf();
    b.hex(0xc9a56a);
    for s in [-1.0f32, 1.0] {
        b.push_xf(Mat4::from_translation(Vec3::new(s * 0.09, 0.07, 0.19)) * Mat4::from_rotation_z(FRAC_PI_2));
        b.cylinder(Vec3::new(0.0, -0.012, 0.0), 0.0505, 0.0505, 0.024, 10, false, false);
        b.pop_xf();
    }
    // straps over the shoulders and a waist strap
    b.mat(mat::CLOTH).hex(0x2e3138);
    for s in [-1.0f32, 1.0] {
        b.box_center(Vec3::new(s * 0.1, 0.38, 0.0), Vec3::new(0.018, 0.17, 0.1));
        b.hex(0xcfc9b8);
        b.box_center(Vec3::new(s * 0.1, 0.4, -0.0), Vec3::new(0.0195, 0.014, 0.1015));
        b.hex(0x2e3138);
    }
    b.box_center(Vec3::new(0.0, 0.1, 0.12), Vec3::new(0.15, 0.015, 0.04));
    b.finish()
}

/// The glider: a domed canopy with six gores over a hand bar. Origin at the hand bar; the
/// canopy sits about a metre above it.
pub fn glider() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).ao(0.8, 1.0);
    let segs = 12usize;
    let rings = 5usize;
    let (rad, rise) = (2.05f32, 0.72f32);
    let ctr = Vec3::new(0.0, 1.35, 0.0);
    // dome as an indexed grid so adjacent gores can alternate colour
    for g in 0..segs {
        let a0 = g as f32 / segs as f32 * TAU_F;
        let a1 = (g + 1) as f32 / segs as f32 * TAU_F;
        let tone = if g % 2 == 0 { 1.0 } else { 0.78 };
        b.color(grey(tone));
        for r in 0..rings {
            let t0 = r as f32 / rings as f32;
            let t1 = (r + 1) as f32 / rings as f32;
            let pt = |a: f32, t: f32| {
                let rr = rad * (1.0 - t * 0.92);
                let y = rise * (1.0 - (1.0 - t).powi(2));
                ctr + Vec3::new(a.cos() * rr, y - rise * 0.2 * (1.0 - t), a.sin() * rr)
            };
            let p = [pt(a0, t0), pt(a1, t0), pt(a1, t1), pt(a0, t1)];
            let mid = (p[0] + p[1] + p[2] + p[3]) * 0.25;
            b.quad_out(p, (mid - ctr + Vec3::Y * 0.6).normalize());
            // underside
            let q = [p[3], p[2], p[1], p[0]];
            b.quad_out(q, -(mid - ctr + Vec3::Y * 0.6).normalize());
        }
    }
    // stitched seams between the gores (light, whatever the canopy colour) and a patch at the crown
    let pt = |a: f32, t: f32| {
        let rr = rad * (1.0 - t * 0.92);
        let y = rise * (1.0 - (1.0 - t).powi(2));
        ctr + Vec3::new(a.cos() * rr, y - rise * 0.2 * (1.0 - t), a.sin() * rr)
    };
    b.tinted(false).mat(mat::FLAT).hex(0xf3efe4);
    for g in 0..segs {
        let a = g as f32 / segs as f32 * TAU_F;
        let w = Vec3::new(-a.sin(), 0.0, a.cos()) * 0.02;
        for r in 0..rings {
            let (t0, t1) = (r as f32 / rings as f32, (r + 1) as f32 / rings as f32);
            let (p0, p1) = (pt(a, t0), pt(a, t1));
            let n = ((p0 + p1) * 0.5 - ctr + Vec3::Y * 0.6).normalize();
            let o = n * 0.012;
            b.quad_out([p0 - w + o, p0 + w + o, p1 + w + o, p1 - w + o], n);
        }
    }
    b.mat(mat::CLOTH).tinted(true).color(grey(0.6));
    {
        let top = pt(0.0, 1.0);
        for k in 0..8 {
            let (a0, a1) = (k as f32 / 8.0 * TAU_F, (k + 1) as f32 / 8.0 * TAU_F);
            let up = Vec3::Y * 0.014;
            b.tri_out(top + up, pt(a0, 0.93) + up, pt(a1, 0.93) + up, Vec3::Y);
        }
    }
    // rim ring
    b.tinted(true).mat(mat::CLOTH).color(grey(0.5));
    for g in 0..segs {
        let a0 = g as f32 / segs as f32 * TAU_F;
        let a1 = (g + 1) as f32 / segs as f32 * TAU_F;
        let p0 = ctr + Vec3::new(a0.cos() * rad, -rise * 0.2, a0.sin() * rad);
        let p1 = ctr + Vec3::new(a1.cos() * rad, -rise * 0.2, a1.sin() * rad);
        b.quad_out([p0, p1, p1 + Vec3::Y * 0.05, p0 + Vec3::Y * 0.05], p0 - ctr);
    }
    // lines to the hand bar
    b.tinted(false).mat(mat::FLAT).color(Vec3::new(0.85, 0.85, 0.88));
    for k in 0..6 {
        let a = k as f32 / 6.0 * TAU_F;
        let top = ctr + Vec3::new(a.cos() * rad * 0.92, -rise * 0.2, a.sin() * rad * 0.92);
        let bot = Vec3::new(a.cos() * 0.05, 0.0, a.sin() * 0.05);
        let d = top - bot;
        let len = d.length();
        b.push_xf(Mat4::from_translation(bot) * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, d / len)));
        b.cylinder(Vec3::ZERO, 0.006, 0.006, len, 4, false, false);
        b.pop_xf();
    }
    b.mat(mat::METAL).tinted(false).color(Vec3::new(0.3, 0.32, 0.36));
    b.push_xf(Mat4::from_rotation_z(FRAC_PI_2));
    b.cylinder(Vec3::new(0.0, -0.3, 0.0), 0.016, 0.016, 0.6, 6, true, true);
    b.pop_xf();
    b.finish()
}

const TAU_F: f32 = std::f32::consts::TAU;

/// Thicken a weapon model (toy-like proportions read better at third-person distance). Lengths
/// along the barrel are kept so grips, foregrips and muzzles stay where the rig expects them.
fn chunky(mut m: MeshData, k: f32) -> MeshData {
    for v in &mut m.verts {
        v.pos[0] *= k;
        v.pos[1] *= k;
    }
    m
}

// =======================================================================================
// Weapons (origin at the grip, barrel along -Z)
//
// Metal parts keep their own colour; the furniture (stock, handguard, grip, magazine) is built in a
// light neutral and takes the instance tint, so a weapon's rarity shows as a colour on the gun
// instead of turning the whole thing into one dark silhouette.
// =======================================================================================

const GUNMETAL: u32 = 0x555b66;
const DARK: u32 = 0x2b2e34;
const LIGHT_METAL: u32 = 0xaab2be;
const WOOD: u32 = 0x8a5630;
const ACCENT: u32 = 0xe6e9ee;

/// Following parts are metal in a fixed colour.
fn metal(b: &mut MeshBuilder, col: u32) {
    b.mat(mat::METAL).tinted(false).hex(col).spec(0.7).ao(0.75, 1.0);
}

/// Following parts are furniture in the rarity colour.
fn accent(b: &mut MeshBuilder) {
    b.mat(mat::FLAT).tinted(true).hex(ACCENT).spec(0.12).ao(0.7, 1.0);
}

fn bxc(b: &mut MeshBuilder, x: f32, y: f32, z: f32, hx: f32, hy: f32, hz: f32) {
    b.box_center(Vec3::new(x, y, z), Vec3::new(hx, hy, hz));
}

/// A grip or magazine: a box tilted about X so its top leans toward -Z for a positive angle.
fn grip_box(b: &mut MeshBuilder, c: Vec3, half: Vec3, tilt: f32) {
    b.push_xf(Mat4::from_translation(c) * rot_x(tilt));
    b.box_center(Vec3::ZERO, half);
    b.pop_xf();
}

/// Ribbed side panels on a grip (drawn in the grip's own tilted frame).
fn grip_ribs(b: &mut MeshBuilder, c: Vec3, half: Vec3, tilt: f32, n: i32) {
    b.push_xf(Mat4::from_translation(c) * rot_x(tilt));
    for k in 0..n {
        let y = -half.y * 0.75 + k as f32 * (half.y * 1.5 / (n - 1).max(1) as f32);
        for s in [-1.0f32, 1.0] {
            b.box_center(Vec3::new(s * (half.x + 0.0005), y, 0.0), Vec3::new(0.0022, 0.0035, half.z * 0.8));
        }
    }
    b.pop_xf();
}

/// Picatinny teeth along the top of a rail.
fn rail_teeth(b: &mut MeshBuilder, y: f32, z0: f32, z1: f32, n: i32, hx: f32) {
    for k in 0..n {
        let z = z0 + (z1 - z0) * k as f32 / (n - 1).max(1) as f32;
        bxc(b, 0.0, y, z, hx, 0.004, 0.007);
    }
}

/// A curved magazine: stacked boxes that lean further forward as they go down.
fn curved_mag(b: &mut MeshBuilder, top: Vec3, hx: f32, hz: f32, seg_h: f32, segs: u32, tilt0: f32, dtilt: f32) {
    let mut p = top;
    let mut a = tilt0;
    for _ in 0..segs {
        let down = Vec3::new(0.0, -a.cos(), -a.sin());
        grip_box(b, p + down * seg_h * 0.5, Vec3::new(hx, seg_h * 0.5 + 0.002, hz), a);
        p += down * seg_h;
        a += dtilt;
    }
}

/// A trigger guard and trigger below the receiver around `z`.
fn trigger(b: &mut MeshBuilder, y: f32, z: f32, len: f32) {
    bxc(b, 0.0, y - 0.016, z - len * 0.5, 0.005, 0.0035, len * 0.5);
    bxc(b, 0.0, y - 0.004, z - len, 0.005, 0.0125, 0.0035);
    b.hex(LIGHT_METAL);
    bxc(b, 0.0, y - 0.005, z - 0.012, 0.0035, 0.011, 0.0035);
}

pub fn weapon_pistol() -> MeshData {
    let mut b = MeshBuilder::new();
    metal(&mut b, GUNMETAL);
    // slide with a lighter top rib, serrations and an ejection port
    bxc(&mut b, 0.0, 0.045, -0.085, 0.017, 0.022, 0.125);
    b.hex(LIGHT_METAL);
    bxc(&mut b, 0.0, 0.069, -0.08, 0.011, 0.004, 0.11);
    b.hex(DARK);
    for k in 0..5 {
        for s in [-1.0f32, 1.0] {
            bxc(&mut b, s * 0.0175, 0.045, 0.012 + k as f32 * 0.0095 - 0.03, 0.0022, 0.017, 0.0026);
        }
    }
    bxc(&mut b, 0.0178, 0.056, -0.075, 0.0018, 0.009, 0.026);
    barrel(&mut b, 0.0, 0.043, -0.2, -0.235, 0.009, 0.009, 8);
    b.hex(LIGHT_METAL);
    barrel(&mut b, 0.0, 0.043, -0.2, -0.205, 0.0125, 0.0125, 8);
    // frame, grip, magazine floor plate
    accent(&mut b);
    bxc(&mut b, 0.0, 0.01, -0.07, 0.016, 0.014, 0.1);
    grip_box(&mut b, Vec3::new(0.0, -0.045, 0.012), Vec3::new(0.018, 0.058, 0.026), -0.2);
    b.hex(DARK).tinted(false).mat(mat::FLAT);
    grip_ribs(&mut b, Vec3::new(0.0, -0.045, 0.012), Vec3::new(0.018, 0.058, 0.026), -0.2, 6);
    metal(&mut b, DARK);
    grip_box(&mut b, Vec3::new(0.0, -0.1, 0.0235), Vec3::new(0.02, 0.006, 0.029), -0.2);
    // trigger guard and trigger
    trigger(&mut b, 0.0, -0.03, 0.04);
    // sights
    b.hex(DARK);
    bxc(&mut b, 0.0, 0.08, -0.192, 0.004, 0.007, 0.005);
    bxc(&mut b, -0.009, 0.078, 0.03, 0.0035, 0.006, 0.005);
    bxc(&mut b, 0.009, 0.078, 0.03, 0.0035, 0.006, 0.005);
    chunky(b.finish(), 1.4)
}

pub fn weapon_smg() -> MeshData {
    let mut b = MeshBuilder::new();
    metal(&mut b, GUNMETAL);
    // receiver with a top rail
    bxc(&mut b, 0.0, 0.025, -0.14, 0.025, 0.04, 0.2);
    b.hex(LIGHT_METAL);
    bxc(&mut b, 0.0, 0.069, -0.13, 0.014, 0.006, 0.18);
    rail_teeth(&mut b, 0.078, -0.28, 0.03, 9, 0.011);
    // vents along the shroud and an ejection port
    b.hex(DARK);
    for k in 0..5 {
        for s in [-1.0f32, 1.0] {
            bxc(&mut b, s * 0.0255, 0.03, -0.2 - k as f32 * 0.028, 0.002, 0.012, 0.009);
        }
    }
    bxc(&mut b, 0.0258, 0.04, -0.02, 0.002, 0.012, 0.03);
    // barrel and a fat suppressor
    barrel(&mut b, 0.0, 0.03, -0.32, -0.36, 0.012, 0.012, 8);
    b.hex(0x3a3d45);
    barrel(&mut b, 0.0, 0.03, -0.36, -0.49, 0.022, 0.022, 10);
    b.hex(LIGHT_METAL);
    barrel(&mut b, 0.0, 0.03, -0.49, -0.5, 0.0235, 0.0235, 10);
    barrel(&mut b, 0.0, 0.03, -0.36, -0.37, 0.0235, 0.0235, 10);
    // extended magazine, grip and a vertical foregrip
    accent(&mut b);
    curved_mag(&mut b, Vec3::new(0.0, -0.015, -0.1), 0.017, 0.024, 0.06, 2, 0.04, 0.1);
    b.hex(DARK);
    bxc(&mut b, 0.0, -0.138, -0.108, 0.019, 0.006, 0.027);
    accent(&mut b);
    grip_box(&mut b, Vec3::new(0.0, -0.06, 0.04), Vec3::new(0.02, 0.06, 0.025), -0.25);
    b.hex(DARK).tinted(false).mat(mat::FLAT);
    grip_ribs(&mut b, Vec3::new(0.0, -0.06, 0.04), Vec3::new(0.02, 0.06, 0.025), -0.25, 6);
    accent(&mut b);
    bxc(&mut b, 0.0, -0.062, -0.2, 0.012, 0.042, 0.014);
    b.hex(DARK).tinted(false).mat(mat::FLAT);
    bxc(&mut b, 0.0, -0.1, -0.2, 0.0135, 0.004, 0.0155);
    metal(&mut b, DARK);
    trigger(&mut b, 0.0, 0.02, 0.05);
    // folding wire stock with a butt pad
    metal(&mut b, GUNMETAL);
    for y in [0.045f32, 0.005] {
        bxc(&mut b, 0.0, y, 0.15, 0.005, 0.005, 0.09);
    }
    bxc(&mut b, 0.0, 0.025, 0.235, 0.005, 0.025, 0.006);
    accent(&mut b);
    bxc(&mut b, 0.0, 0.025, 0.248, 0.018, 0.04, 0.0095);
    // red dot sight
    metal(&mut b, DARK);
    bxc(&mut b, 0.0, 0.092, -0.1, 0.016, 0.018, 0.03);
    bxc(&mut b, 0.0, 0.112, -0.1, 0.019, 0.004, 0.034);
    b.mat(mat::GLASS).tinted(false).color(Vec3::new(0.35, 0.65, 0.9));
    bxc(&mut b, 0.0, 0.098, -0.131, 0.012, 0.011, 0.002);
    b.mat(mat::EMISSIVE).tinted(false).color(Vec3::new(1.0, 0.15, 0.1));
    bxc(&mut b, 0.0, 0.098, -0.133, 0.0028, 0.0028, 0.002);
    chunky(b.finish(), 1.4)
}

pub fn weapon_ar() -> MeshData {
    let mut b = MeshBuilder::new();
    metal(&mut b, GUNMETAL);
    // receiver, a darker upper receiver and a charging handle
    bxc(&mut b, 0.0, 0.03, -0.1, 0.027, 0.045, 0.2);
    b.hex(DARK);
    bxc(&mut b, 0.0, 0.064, -0.1, 0.024, 0.018, 0.17);
    b.hex(LIGHT_METAL);
    bxc(&mut b, 0.0, 0.1, 0.07, 0.008, 0.008, 0.016);
    // ejection port and magazine release on the right
    b.hex(DARK);
    bxc(&mut b, 0.0275, 0.052, -0.05, 0.002, 0.014, 0.04);
    bxc(&mut b, 0.0275, 0.0, -0.1, 0.002, 0.008, 0.012);
    // long top rail with teeth
    b.hex(LIGHT_METAL);
    bxc(&mut b, 0.0, 0.082, -0.24, 0.015, 0.007, 0.33);
    rail_teeth(&mut b, 0.093, -0.54, 0.08, 18, 0.012);
    // handguard in the rarity colour with vent slots and a rail under it
    accent(&mut b);
    bxc(&mut b, 0.0, 0.025, -0.45, 0.028, 0.034, 0.13);
    b.hex(DARK).tinted(false).mat(mat::FLAT);
    for k in 0..5 {
        for s in [-1.0f32, 1.0] {
            bxc(&mut b, s * 0.0285, 0.028, -0.36 - k as f32 * 0.04, 0.002, 0.014, 0.011);
        }
    }
    metal(&mut b, LIGHT_METAL);
    bxc(&mut b, 0.0, -0.012, -0.45, 0.012, 0.004, 0.12);
    // barrel, gas block, flash hider
    metal(&mut b, DARK);
    barrel(&mut b, 0.0, 0.028, -0.55, -0.72, 0.012, 0.012, 8);
    bxc(&mut b, 0.0, 0.036, -0.6, 0.013, 0.02, 0.016);
    b.hex(LIGHT_METAL);
    barrel(&mut b, 0.0, 0.028, -0.72, -0.77, 0.019, 0.017, 8);
    b.hex(DARK);
    for k in 0..3 {
        barrel(&mut b, 0.0, 0.028, -0.725 - k as f32 * 0.014, -0.73 - k as f32 * 0.014, 0.0205, 0.0205, 8);
    }
    // front sight post and a rear flip sight
    bxc(&mut b, 0.0, 0.1, -0.6, 0.005, 0.02, 0.006);
    bxc(&mut b, 0.0, 0.112, -0.6, 0.012, 0.003, 0.006);
    bxc(&mut b, 0.0, 0.108, 0.0, 0.01, 0.012, 0.01);
    // stock: an adjustable tube, the cheek riser and the butt pad
    accent(&mut b);
    bxc(&mut b, 0.0, 0.025, 0.27, 0.022, 0.05, 0.09);
    bxc(&mut b, 0.0, 0.07, 0.26, 0.014, 0.012, 0.075);
    b.hex(DARK).tinted(false).mat(mat::FLAT);
    bxc(&mut b, 0.0, 0.025, 0.365, 0.024, 0.056, 0.012);
    for k in 0..4 {
        bxc(&mut b, 0.0, 0.0 + k as f32 * 0.02 - 0.012, 0.3, 0.0225, 0.004, 0.06);
    }
    // pistol grip and a curved magazine
    accent(&mut b);
    grip_box(&mut b, Vec3::new(0.0, -0.05, 0.02), Vec3::new(0.02, 0.06, 0.025), -0.28);
    b.hex(DARK).tinted(false).mat(mat::FLAT);
    grip_ribs(&mut b, Vec3::new(0.0, -0.05, 0.02), Vec3::new(0.02, 0.06, 0.025), -0.28, 6);
    metal(&mut b, 0x3c4048);
    curved_mag(&mut b, Vec3::new(0.0, -0.012, -0.14), 0.02, 0.031, 0.05, 3, 0.04, 0.09);
    b.hex(DARK);
    bxc(&mut b, 0.0, -0.17, -0.168, 0.0215, 0.006, 0.034);
    trigger(&mut b, 0.0, 0.0, 0.055);
    // red dot sight on the rail
    metal(&mut b, DARK);
    bxc(&mut b, 0.0, 0.113, -0.2, 0.014, 0.016, 0.028);
    bxc(&mut b, 0.0, 0.132, -0.2, 0.017, 0.0035, 0.032);
    b.mat(mat::GLASS).tinted(false).color(Vec3::new(0.35, 0.65, 0.9));
    bxc(&mut b, 0.0, 0.117, -0.229, 0.011, 0.012, 0.002);
    b.mat(mat::EMISSIVE).tinted(false).color(Vec3::new(1.0, 0.15, 0.1));
    bxc(&mut b, 0.0, 0.117, -0.2315, 0.0028, 0.0028, 0.002);
    // rarity stripe along the receiver (glows in the rarity colour)
    b.mat(mat::EMISSIVE).tinted(true).color(Vec3::new(0.9, 0.9, 0.9));
    bxc(&mut b, 0.0, 0.0, -0.1, 0.0285, 0.007, 0.15);
    chunky(b.finish(), 1.4)
}

pub fn weapon_shotgun() -> MeshData {
    let mut b = MeshBuilder::new();
    metal(&mut b, GUNMETAL);
    // receiver with a loading port and an ejection port
    bxc(&mut b, 0.0, 0.025, -0.06, 0.026, 0.04, 0.12);
    b.hex(DARK);
    bxc(&mut b, 0.0, -0.014, -0.08, 0.015, 0.002, 0.03);
    bxc(&mut b, 0.0268, 0.035, -0.06, 0.002, 0.013, 0.035);
    // barrel with a rib, magazine tube and its cap
    barrel(&mut b, 0.0, 0.04, -0.15, -0.78, 0.016, 0.016, 8);
    barrel(&mut b, 0.0, 0.0, -0.17, -0.7, 0.014, 0.014, 8);
    b.hex(LIGHT_METAL);
    bxc(&mut b, 0.0, 0.0605, -0.46, 0.004, 0.003, 0.31);
    barrel(&mut b, 0.0, 0.04, -0.78, -0.8, 0.02, 0.02, 8);
    barrel(&mut b, 0.0, 0.0, -0.7, -0.715, 0.017, 0.017, 8);
    b.sphere(Vec3::new(0.0, 0.066, -0.77), 0.007, 1);
    // pump forearm in wood with grooves and rarity end caps
    b.mat(mat::WOOD).tinted(false).hex(WOOD).spec(0.1);
    bxc(&mut b, 0.0, 0.0, -0.33, 0.026, 0.026, 0.09);
    b.hex(0x6a4024);
    for k in 0..5 {
        bxc(&mut b, 0.0, -0.0265, -0.28 - k as f32 * 0.025, 0.02, 0.0015, 0.005);
    }
    accent(&mut b);
    bxc(&mut b, 0.0, 0.0, -0.4175, 0.0275, 0.0275, 0.0095);
    bxc(&mut b, 0.0, 0.0, -0.2425, 0.0275, 0.0275, 0.0055);
    // wooden stock with a rarity butt pad
    b.mat(mat::WOOD).tinted(false).hex(WOOD).spec(0.1);
    bxc(&mut b, 0.0, 0.015, 0.24, 0.022, 0.05, 0.13);
    bxc(&mut b, 0.0, 0.058, 0.2, 0.015, 0.012, 0.09);
    accent(&mut b);
    bxc(&mut b, 0.0, 0.015, 0.378, 0.024, 0.056, 0.012);
    // wooden grip
    b.mat(mat::WOOD).tinted(false).hex(WOOD).spec(0.1);
    grip_box(&mut b, Vec3::new(0.0, -0.05, 0.04), Vec3::new(0.019, 0.055, 0.024), -0.3);
    b.mat(mat::FLAT).hex(0x6a4024);
    grip_ribs(&mut b, Vec3::new(0.0, -0.05, 0.04), Vec3::new(0.019, 0.055, 0.024), -0.3, 5);
    metal(&mut b, DARK);
    trigger(&mut b, 0.0, 0.0, 0.05);
    // red shells in a side holder
    b.mat(mat::FLAT).tinted(false).hex(0xd33a2a);
    for k in 0..3 {
        bxc(&mut b, 0.0285, 0.035, -0.02 - k as f32 * 0.03, 0.006, 0.014, 0.011);
    }
    b.hex(0xd9b44a);
    for k in 0..3 {
        bxc(&mut b, 0.0285, 0.02, -0.02 - k as f32 * 0.03, 0.0062, 0.004, 0.0112);
    }
    chunky(b.finish(), 1.4)
}

pub fn weapon_sniper() -> MeshData {
    let mut b = MeshBuilder::new();
    metal(&mut b, GUNMETAL);
    // receiver and ejection port
    bxc(&mut b, 0.0, 0.03, -0.1, 0.024, 0.04, 0.17);
    b.hex(DARK);
    bxc(&mut b, 0.0245, 0.04, -0.12, 0.002, 0.012, 0.045);
    // long barrel with a ported muzzle brake
    barrel(&mut b, 0.0, 0.035, -0.26, -1.0, 0.012, 0.01, 8);
    b.hex(LIGHT_METAL);
    barrel(&mut b, 0.0, 0.035, -1.0, -1.06, 0.018, 0.018, 8);
    b.hex(DARK);
    for k in 0..3 {
        bxc(&mut b, 0.0, 0.035, -1.015 - k as f32 * 0.016, 0.0195, 0.0035, 0.004);
        bxc(&mut b, 0.0, 0.035, -1.015 - k as f32 * 0.016, 0.0035, 0.0195, 0.004);
    }
    // stock in the rarity colour: a forend, the main stock with a cheek riser, the butt pad
    accent(&mut b);
    bxc(&mut b, 0.0, 0.0, 0.2, 0.022, 0.05, 0.14);
    bxc(&mut b, 0.0, 0.07, 0.24, 0.015, 0.015, 0.09);
    bxc(&mut b, 0.0, 0.03, -0.2, 0.02, 0.025, 0.12);
    grip_box(&mut b, Vec3::new(0.0, -0.05, 0.035), Vec3::new(0.019, 0.055, 0.024), -0.3);
    b.hex(DARK).tinted(false).mat(mat::FLAT);
    grip_ribs(&mut b, Vec3::new(0.0, -0.05, 0.035), Vec3::new(0.019, 0.055, 0.024), -0.3, 5);
    bxc(&mut b, 0.0, 0.0, 0.337, 0.0235, 0.052, 0.009);
    // scope: tube, objective bell, eyepiece, rings, turrets
    metal(&mut b, DARK);
    barrel(&mut b, 0.0, 0.1, 0.0, -0.36, 0.03, 0.03, 12);
    barrel(&mut b, 0.0, 0.1, 0.03, 0.0, 0.036, 0.03, 12);
    barrel(&mut b, 0.0, 0.1, -0.36, -0.4, 0.03, 0.04, 12);
    b.hex(LIGHT_METAL);
    for z in [-0.05f32, -0.27] {
        bxc(&mut b, 0.0, 0.066, z, 0.009, 0.018, 0.012);
        barrel(&mut b, 0.0, 0.1, z + 0.011, z - 0.011, 0.0325, 0.0325, 12);
    }
    b.hex(DARK);
    bxc(&mut b, 0.0, 0.14, -0.16, 0.008, 0.01, 0.008);
    bxc(&mut b, 0.034, 0.1, -0.16, 0.01, 0.008, 0.008);
    // lenses
    b.mat(mat::GLASS).tinted(false).color(Vec3::new(0.25, 0.55, 0.85));
    barrel(&mut b, 0.0, 0.1, 0.034, 0.03, 0.026, 0.026, 12);
    barrel(&mut b, 0.0, 0.1, -0.402, -0.406, 0.034, 0.034, 12);
    // bolt handle and magazine
    metal(&mut b, LIGHT_METAL);
    bxc(&mut b, 0.04, 0.035, -0.02, 0.02, 0.006, 0.006);
    b.sphere(Vec3::new(0.065, 0.035, -0.02), 0.011, 1);
    metal(&mut b, 0x3c4048);
    bxc(&mut b, 0.0, -0.025, -0.09, 0.014, 0.025, 0.03);
    // a folded bipod under the forend
    metal(&mut b, DARK);
    bxc(&mut b, 0.0, -0.003, -0.5, 0.012, 0.01, 0.014);
    for s in [-1.0f32, 1.0] {
        bxc(&mut b, s * 0.012, -0.03, -0.46, 0.004, 0.004, 0.06);
    }
    chunky(b.finish(), 1.4)
}

pub fn weapon_rocket() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).tinted(false).hex(0x586b4d).ao(0.7, 1.0).spec(0.4);
    // main tube resting over the shoulder; the grip hangs below the middle
    barrel(&mut b, 0.0, 0.09, 0.45, -0.55, 0.085, 0.085, 14);
    // flared rear and front rings
    b.hex(DARK);
    barrel(&mut b, 0.0, 0.09, 0.62, 0.45, 0.115, 0.085, 14);
    barrel(&mut b, 0.0, 0.09, -0.55, -0.62, 0.095, 0.095, 14);
    b.hex(0x44553b);
    for z in [0.32f32, -0.05, -0.38] {
        barrel(&mut b, 0.0, 0.09, z + 0.015, z - 0.015, 0.0885, 0.0885, 14);
    }
    // warhead
    b.mat(mat::FLAT).tinted(false).hex(0xd6492f).spec(0.1);
    barrel(&mut b, 0.0, 0.09, -0.62, -0.7, 0.07, 0.055, 12);
    b.hex(0xe9e4d8);
    barrel(&mut b, 0.0, 0.09, -0.7, -0.78, 0.055, 0.0, 12);
    // shoulder pad in the rarity colour, grips and the sights
    accent(&mut b);
    bxc(&mut b, 0.0, 0.18, 0.47, 0.06, 0.032, 0.1);
    grip_box(&mut b, Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.02, 0.06, 0.025), -0.25);
    grip_box(&mut b, Vec3::new(0.0, 0.0, -0.3), Vec3::new(0.018, 0.045, 0.022), 0.0);
    b.hex(DARK).tinted(false).mat(mat::FLAT);
    grip_ribs(&mut b, Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.02, 0.06, 0.025), -0.25, 5);
    metal(&mut b, DARK);
    bxc(&mut b, 0.0, 0.2, -0.1, 0.012, 0.03, 0.04);
    bxc(&mut b, 0.0, 0.2, -0.1, 0.0035, 0.0035, 0.045);
    bxc(&mut b, 0.0, 0.176, -0.52, 0.004, 0.02, 0.005);
    trigger(&mut b, 0.0, 0.0, 0.05);
    // accent band
    b.mat(mat::EMISSIVE).tinted(true).color(Vec3::new(0.9, 0.9, 0.9));
    barrel(&mut b, 0.0, 0.09, 0.2, 0.15, 0.0865, 0.0865, 14);
    barrel(&mut b, 0.0, 0.09, -0.46, -0.48, 0.0865, 0.0865, 14);
    chunky(b.finish(), 1.4)
}

/// Harvesting tool: grip at the origin, head pointing up (+Y), pick toward -Z.
pub fn pickaxe() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::WOOD).hex(0x9c6a3a).ao(0.7, 1.0).spec(0.1);
    b.cylinder(Vec3::new(0.0, -0.22, 0.0), 0.022, 0.02, 0.95, 8, true, true);
    // grip wrap
    b.mat(mat::CLOTH).hex(0xd6492f);
    b.cylinder(Vec3::new(0.0, -0.08, 0.0), 0.0245, 0.0245, 0.16, 8, false, false);
    // head block
    b.mat(mat::METAL).hex(0x6f7783).spec(0.8);
    b.box_center(Vec3::new(0.0, 0.74, 0.0), Vec3::new(0.032, 0.04, 0.045));
    // curved pick toward -Z and adze toward +Z (segments bending downward)
    let seg = |b: &mut MeshBuilder, z0: f32, z1: f32, y0: f32, y1: f32, w0: f32, w1: f32| {
        let c0 = Vec3::new(0.0, y0, z0);
        let c1 = Vec3::new(0.0, y1, z1);
        let d = c1 - c0;
        let len = d.length();
        b.push_xf(Mat4::from_translation(c0) * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Z, d / len)));
        b.box_faces(Vec3::new(-w0 * 0.5, -w0 * 0.5, 0.0), Vec3::new(w0 * 0.5, w0 * 0.5, len), 0b111111);
        let _ = w1;
        b.pop_xf();
    };
    b.hex(0xb5bcc7);
    seg(&mut b, -0.04, -0.2, 0.745, 0.735, 0.05, 0.035);
    seg(&mut b, -0.2, -0.33, 0.735, 0.68, 0.04, 0.02);
    seg(&mut b, -0.33, -0.4, 0.68, 0.6, 0.026, 0.01);
    b.hex(0x8f98a6);
    seg(&mut b, 0.04, 0.17, 0.745, 0.745, 0.05, 0.04);
    b.box_center(Vec3::new(0.0, 0.742, 0.21), Vec3::new(0.012, 0.05, 0.045));
    // socket plates either side of the head, a steel cap on the handle's end and a wrist strap
    b.hex(0x8f98a6).spec(0.8);
    b.box_center(Vec3::new(0.0, 0.7, 0.0), Vec3::new(0.026, 0.02, 0.05));
    b.box_center(Vec3::new(0.0, 0.78, 0.0), Vec3::new(0.026, 0.012, 0.05));
    b.hex(0xb5bcc7);
    b.sphere(Vec3::new(0.0, -0.23, 0.0), 0.03, 1);
    b.mat(mat::CLOTH).hex(0x4a3a2c);
    b.box_center(Vec3::new(0.0, -0.2, 0.034), Vec3::new(0.004, 0.06, 0.012));
    b.box_center(Vec3::new(0.0, -0.26, 0.012), Vec3::new(0.004, 0.012, 0.03));
    // accent collar
    b.mat(mat::EMISSIVE).tinted(false).color(Vec3::new(1.0, 0.55, 0.15));
    b.cylinder(Vec3::new(0.0, 0.66, 0.0), 0.027, 0.027, 0.03, 8, false, false);
    b.finish()
}

// =======================================================================================
// Items
// =======================================================================================

pub fn ammo_box() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::FLAT).color(grey(1.0)).ao(0.7, 1.0);
    b.box_center(Vec3::new(0.0, 0.07, 0.0), Vec3::new(0.12, 0.07, 0.075));
    b.color(grey(0.8));
    b.box_center(Vec3::new(0.0, 0.147, 0.0), Vec3::new(0.125, 0.012, 0.08));
    // handle
    b.tinted(false).color(Vec3::new(0.25, 0.27, 0.3));
    b.box_center(Vec3::new(0.0, 0.17, 0.0), Vec3::new(0.05, 0.012, 0.012));
    // label with bullets
    b.color(Vec3::new(0.95, 0.95, 0.9));
    b.box_center(Vec3::new(0.0, 0.07, -0.0765), Vec3::new(0.075, 0.035, 0.002));
    b.mat(mat::METAL).color(Vec3::new(0.85, 0.65, 0.3));
    for k in 0..3 {
        barrel(&mut b, -0.04 + k as f32 * 0.04, 0.075, -0.0775, -0.082, 0.011, 0.011, 8);
    }
    // darker corner guards and ribs in the box colour, steel latches on the front and a handle post at each end
    b.mat(mat::FLAT).tinted(true).color(grey(0.55)).ao(0.8, 1.0);
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        b.box_center(Vec3::new(sx * 0.1175, 0.07, sz * 0.0725), Vec3::new(0.0065, 0.0715, 0.0065));
    }
    b.color(grey(0.78));
    for z in [-0.0775f32, 0.0775] {
        for y in [0.03f32, 0.112] {
            b.box_center(Vec3::new(0.0, y, z), Vec3::new(0.1, 0.007, 0.0025));
        }
    }
    b.mat(mat::METAL).tinted(false).hex(0xb9bec8).spec(0.9);
    for s in [-1.0f32, 1.0] {
        b.box_center(Vec3::new(s * 0.085, 0.13, -0.0815), Vec3::new(0.014, 0.022, 0.003));
        b.box_center(Vec3::new(s * 0.085, 0.108, -0.0805), Vec3::new(0.008, 0.005, 0.005));
        b.box_center(Vec3::new(s * 0.05, 0.164, 0.0), Vec3::new(0.005, 0.014, 0.012));
    }
    b.finish()
}

pub fn item_bandage() -> MeshData {
    let mut b = MeshBuilder::new();
    b.tinted(false).mat(mat::CLOTH).color(Vec3::new(0.96, 0.95, 0.9)).ao(0.7, 1.0);
    b.push_xf(Mat4::from_rotation_z(FRAC_PI_2));
    b.cylinder(Vec3::new(-0.065, 0.065, 0.0), 0.075, 0.075, 0.13, 14, true, true);
    b.color(Vec3::new(0.88, 0.2, 0.2));
    b.cylinder(Vec3::new(-0.01, 0.065, 0.0), 0.0765, 0.0765, 0.02, 14, false, false);
    b.color(Vec3::new(0.8, 0.8, 0.78));
    b.cylinder(Vec3::new(-0.066, 0.065, 0.0), 0.03, 0.03, 0.132, 10, true, true);
    b.pop_xf();
    // loose tail
    b.color(Vec3::new(0.97, 0.96, 0.92));
    b.box_center(Vec3::new(0.0, 0.003, -0.095), Vec3::new(0.065, 0.003, 0.05));
    b.finish()
}

pub fn item_medkit() -> MeshData {
    let mut b = MeshBuilder::new();
    b.tinted(false).mat(mat::FLAT).color(Vec3::new(0.95, 0.95, 0.94)).ao(0.7, 1.0);
    b.box_center(Vec3::new(0.0, 0.09, 0.0), Vec3::new(0.135, 0.09, 0.055));
    b.color(Vec3::new(0.8, 0.82, 0.84));
    b.box_center(Vec3::new(0.0, 0.185, 0.0), Vec3::new(0.14, 0.008, 0.06));
    // handle
    b.color(Vec3::new(0.35, 0.37, 0.4));
    b.box_center(Vec3::new(0.0, 0.205, 0.0), Vec3::new(0.05, 0.012, 0.012));
    b.box_center(Vec3::new(-0.05, 0.195, 0.0), Vec3::new(0.01, 0.012, 0.012));
    b.box_center(Vec3::new(0.05, 0.195, 0.0), Vec3::new(0.01, 0.012, 0.012));
    // red cross on both faces, and a smaller one on the lid
    b.mat(mat::EMISSIVE).color(Vec3::new(0.95, 0.12, 0.12));
    for z in [-0.0565f32, 0.0565] {
        b.box_center(Vec3::new(0.0, 0.09, z), Vec3::new(0.065, 0.017, 0.002));
        b.box_center(Vec3::new(0.0, 0.09, z), Vec3::new(0.017, 0.065, 0.002));
    }
    b.box_center(Vec3::new(0.0, 0.1935, 0.0), Vec3::new(0.04, 0.0015, 0.011));
    b.box_center(Vec3::new(0.0, 0.1935, 0.0), Vec3::new(0.011, 0.0015, 0.04));
    // red edge band under the lid, metal clasps, and bumpers on the corners
    b.mat(mat::FLAT).color(Vec3::new(0.86, 0.16, 0.16));
    b.box_center(Vec3::new(0.0, 0.176, 0.0), Vec3::new(0.1375, 0.007, 0.0575));
    b.mat(mat::METAL).color(Vec3::new(0.7, 0.72, 0.76)).spec(0.9);
    for s in [-1.0f32, 1.0] {
        b.box_center(Vec3::new(s * 0.09, 0.15, -0.0575), Vec3::new(0.016, 0.022, 0.003));
    }
    b.mat(mat::FLAT).color(Vec3::new(0.78, 0.8, 0.82));
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        b.box_center(Vec3::new(sx * 0.1325, 0.09, sz * 0.0525), Vec3::new(0.0055, 0.091, 0.0055));
    }
    b.finish()
}

fn potion(radius: f32, neck: f32, liquid: Vec3, glass: Vec3, cork: Vec3) -> MeshData {
    let mut b = MeshBuilder::new();
    b.tinted(false).ao(0.75, 1.0);
    // glowing liquid body
    b.mat(mat::EMISSIVE).color(liquid);
    b.blob(Vec3::new(0.0, radius * 1.05, 0.0), Vec3::new(radius, radius * 0.98, radius), 2, 0.0, 0, true);
    // glass neck and shoulders
    b.mat(mat::GLASS).color(glass);
    b.cylinder(Vec3::new(0.0, radius * 1.8, 0.0), radius * 0.38, radius * 0.3, neck, 10, false, false);
    b.cylinder(Vec3::new(0.0, radius * 1.8 + neck - 0.005, 0.0), radius * 0.36, radius * 0.36, 0.012, 10, true, true);
    // cork
    b.mat(mat::WOOD).color(cork);
    b.cylinder(Vec3::new(0.0, radius * 1.8 + neck, 0.0), radius * 0.26, radius * 0.3, radius * 0.45, 8, false, true);
    // a ribbon round the neck and a bright glint on the glass
    b.mat(mat::FLAT).color(Vec3::new(0.95, 0.85, 0.35));
    b.cylinder(Vec3::new(0.0, radius * 1.8 + neck * 0.35, 0.0), radius * 0.4, radius * 0.4, radius * 0.16, 10, false, false);
    b.mat(mat::EMISSIVE).color(Vec3::new(1.0, 1.0, 1.0));
    b.push_xf(Mat4::from_translation(Vec3::new(-radius * 0.5, radius * 1.45, -radius * 0.72)));
    b.blob(Vec3::ZERO, Vec3::new(radius * 0.16, radius * 0.3, radius * 0.07), 1, 0.0, 0, true);
    b.pop_xf();
    b.finish()
}

pub fn item_shield_mini() -> MeshData {
    potion(0.065, 0.07, Vec3::new(0.2, 0.6, 1.0), Vec3::new(0.7, 0.88, 1.0), Vec3::new(0.7, 0.5, 0.3))
}

pub fn item_shield_big() -> MeshData {
    potion(0.095, 0.1, Vec3::new(0.15, 0.45, 1.0), Vec3::new(0.7, 0.88, 1.0), Vec3::new(0.7, 0.5, 0.3))
}

pub fn item_chug() -> MeshData {
    let mut b = MeshBuilder::new();
    b.tinted(false).ao(0.75, 1.0);
    b.mat(mat::EMISSIVE).color(Vec3::new(0.62, 0.32, 1.0));
    b.blob(Vec3::new(0.0, 0.13, 0.0), Vec3::new(0.105, 0.125, 0.105), 2, 0.0, 0, true);
    b.mat(mat::GLASS).color(Vec3::new(0.85, 0.75, 1.0));
    b.cylinder(Vec3::new(0.0, 0.235, 0.0), 0.05, 0.04, 0.07, 10, false, false);
    b.mat(mat::FLAT).color(Vec3::new(0.95, 0.85, 0.3));
    b.cylinder(Vec3::new(0.0, 0.3, 0.0), 0.045, 0.05, 0.04, 10, true, true);
    // handle
    b.mat(mat::METAL).color(Vec3::new(0.85, 0.85, 0.9));
    for s in [-1.0f32, 1.0] {
        b.box_center(Vec3::new(s * 0.125, 0.19, 0.0), Vec3::new(0.012, 0.045, 0.012));
        b.box_center(Vec3::new(s * 0.108, 0.245, 0.0), Vec3::new(0.016, 0.012, 0.012));
        b.box_center(Vec3::new(s * 0.108, 0.135, 0.0), Vec3::new(0.016, 0.012, 0.012));
    }
    // label
    b.mat(mat::FLAT).color(Vec3::new(0.95, 0.9, 1.0));
    b.box_center(Vec3::new(0.0, 0.14, -0.1), Vec3::new(0.05, 0.05, 0.004));
    b.finish()
}

// =======================================================================================
// Chest, bus, rocket
// =======================================================================================

/// Treasure chest base; the lid is a separate mesh hinged at the back top edge.
pub fn chest_base() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::WOOD).hex(0xb0702f).ao(0.55, 1.0).spec(0.1);
    b.box_min_max(Vec3::new(-0.46, 0.0, -0.3), Vec3::new(0.46, 0.46, 0.3));
    // golden corner posts and bands
    b.mat(mat::METAL).hex(0xf2c040).spec(0.9);
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            b.box_center(Vec3::new(sx * 0.45, 0.23, sz * 0.29), Vec3::new(0.03, 0.235, 0.03));
        }
    }
    b.box_min_max(Vec3::new(-0.465, 0.0, -0.305), Vec3::new(0.465, 0.04, 0.305));
    b.box_min_max(Vec3::new(-0.465, 0.41, -0.305), Vec3::new(0.465, 0.46, 0.305));
    for sx in [-0.22f32, 0.22] {
        b.box_min_max(Vec3::new(sx - 0.03, 0.0, -0.31), Vec3::new(sx + 0.03, 0.46, 0.31));
    }
    // rivets along the bands, corner caps, a lock plate on the front and handles on the ends
    b.mat(mat::METAL).hex(0xffd75a).spec(0.9);
    for sx in [-0.22f32, 0.22] {
        for k in 0..4 {
            b.sphere(Vec3::new(sx, 0.09 + k as f32 * 0.1, -0.312), 0.017, 0);
        }
    }
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            b.sphere(Vec3::new(sx * 0.45, 0.46, sz * 0.29), 0.04, 1);
            b.sphere(Vec3::new(sx * 0.45, 0.02, sz * 0.29), 0.035, 0);
        }
        // end handle: a plate and a bar
        b.box_center(Vec3::new(sx * 0.472, 0.26, 0.0), Vec3::new(0.008, 0.05, 0.08));
        b.box_center(Vec3::new(sx * 0.492, 0.3, 0.0), Vec3::new(0.012, 0.012, 0.075));
        b.box_center(Vec3::new(sx * 0.492, 0.22, 0.0), Vec3::new(0.012, 0.012, 0.075));
    }
    b.hex(0xd9ac3a);
    b.box_center(Vec3::new(0.0, 0.33, -0.316), Vec3::new(0.07, 0.07, 0.008));
    b.mat(mat::FLAT).tinted(false).color(Vec3::new(0.2, 0.15, 0.1));
    b.box_center(Vec3::new(0.0, 0.34, -0.3245), Vec3::new(0.012, 0.024, 0.003));
    // glowing treasure inside (visible when the lid opens)
    b.mat(mat::EMISSIVE).tinted(false).color(Vec3::new(1.0, 0.8, 0.3));
    b.box_min_max(Vec3::new(-0.4, 0.43, -0.25), Vec3::new(0.4, 0.45, 0.25));
    b.finish()
}

/// Lid with its hinge on the x axis at the origin (local -Z is the front of the chest).
pub fn chest_lid() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::WOOD).hex(0xb0702f).ao(0.55, 1.0).spec(0.1);
    // half cylinder along X; the hinge is at z = +0.3 (back), front edge at z = -0.3
    let seg = 10usize;
    let r = 0.3;
    let hh = 0.46;
    let mut rings: Vec<(Vec3, Vec3)> = vec![];
    for i in 0..=seg {
        let a = i as f32 / seg as f32 * PI;
        // goes from front (z=-r) over the top to the back (z=+r)
        let z = -a.cos() * r;
        let y = a.sin() * 0.17;
        let n = Vec3::new(0.0, a.sin() * 0.17 / 0.17, -a.cos()).normalize();
        rings.push((Vec3::new(0.0, y, z + 0.3), n));
    }
    for i in 0..seg {
        let (p0, n0) = rings[i];
        let (p1, n1) = rings[i + 1];
        let v = [
            b.vert(Vec3::new(-hh, p0.y, p0.z), n0, 1.0),
            b.vert(Vec3::new(hh, p0.y, p0.z), n0, 1.0),
            b.vert(Vec3::new(hh, p1.y, p1.z), n1, 1.0),
            b.vert(Vec3::new(-hh, p1.y, p1.z), n1, 1.0),
        ];
        b.quad(v[0], v[1], v[2], v[3]);
    }
    // end caps
    for s in [-1.0f32, 1.0] {
        let n = Vec3::new(s, 0.0, 0.0);
        let c = b.vert(Vec3::new(s * hh, 0.0, 0.3), n, 1.0);
        let ids: Vec<u32> = rings.iter().map(|(p, _)| b.vert(Vec3::new(s * hh, p.y, p.z), n, 1.0)).collect();
        for w in ids.windows(2) {
            b.tri(c, w[0], w[1]);
        }
    }
    // golden bands and lock
    b.mat(mat::METAL).hex(0xf2c040).spec(0.9);
    for sx in [-0.22f32, 0.22] {
        let mut prev: Option<(Vec3, Vec3)> = None;
        for &(p, n) in &rings {
            if let Some((pp, pn)) = prev {
                let pa = Vec3::new(sx - 0.03, pp.y + 0.004, pp.z);
                let pb = Vec3::new(sx + 0.03, pp.y + 0.004, pp.z);
                let pc = Vec3::new(sx + 0.03, p.y + 0.004, p.z);
                let pd = Vec3::new(sx - 0.03, p.y + 0.004, p.z);
                let va = [b.vert(pa, pn, 1.0), b.vert(pb, pn, 1.0), b.vert(pc, n, 1.0), b.vert(pd, n, 1.0)];
                b.quad(va[0], va[1], va[2], va[3]);
            }
            prev = Some((p, n));
        }
    }
    b.box_center(Vec3::new(0.0, 0.015, -0.3), Vec3::new(0.05, 0.05, 0.02));
    // a golden edge along the front, rivets on the bands and two barrel hinges at the back
    b.box_center(Vec3::new(0.0, 0.0, -0.302), Vec3::new(0.466, 0.022, 0.012));
    b.hex(0xffd75a);
    for sx in [-0.22f32, 0.22] {
        for k in 0..5 {
            let a = 0.2 + k as f32 * 0.65;
            let (z, y) = (-a.cos() * 0.3 + 0.3, a.sin() * 0.17);
            b.sphere(Vec3::new(sx - 0.025, y + 0.012, z), 0.014, 0);
            b.sphere(Vec3::new(sx + 0.025, y + 0.012, z), 0.014, 0);
        }
    }
    for sx in [-0.3f32, 0.3] {
        b.push_xf(Mat4::from_translation(Vec3::new(sx, 0.0, 0.3)) * Mat4::from_rotation_z(FRAC_PI_2));
        b.cylinder(Vec3::new(0.0, -0.06, 0.0), 0.03, 0.03, 0.12, 8, true, true);
        b.pop_xf();
    }
    b.mat(mat::FLAT).tinted(false).color(Vec3::new(0.2, 0.15, 0.1));
    b.box_center(Vec3::new(0.0, 0.01, -0.322), Vec3::new(0.012, 0.022, 0.004));
    b.finish()
}

/// Extrudes a left-right symmetric cross-section along Z and rounds both ends over. `half` is the right half (x >= 0) of
/// the outline in the XY plane, from the bottom centre round to the top centre, with heights that never go down; each
/// point carries the band of the segment that starts at it, and `style` sets the colour and material of a band. The ends
/// at `z0` and `z1` are rounded with the radii in `round` (0 leaves a square edge) and closed by flat caps painted in the
/// same bands, so a band of glass running along the sides wraps round into a windscreen and a rear window.
fn loft(b: &mut MeshBuilder, half: &[(Vec2, u8)], z0: f32, z1: f32, round: [f32; 2], steps: usize, style: &dyn Fn(&mut MeshBuilder, u8)) {
    let n = half.len();
    let seg_n: Vec<Vec2> = (0..n - 1)
        .map(|i| {
            let d = half[i + 1].0 - half[i].0;
            Vec2::new(d.y, -d.x).normalize_or_zero()
        })
        .collect();
    let mirror = |v: Vec2| Vec2::new(-v.x, v.y);
    // the normals of the segments either side of a point (the two centre points sit between a segment and its mirror image)
    let around = |i: usize| (if i == 0 { mirror(seg_n[0]) } else { seg_n[i - 1] }, if i == n - 1 { mirror(seg_n[n - 2]) } else { seg_n[i] });
    let inset_n: Vec<Vec2> = (0..n)
        .map(|i| {
            let (a, c) = around(i);
            (a + c).normalize_or_zero()
        })
        .collect();
    // smooth across gentle bends, sharp at corners
    let shade = |seg: usize, i: usize| {
        let (a, c) = around(i);
        if a.dot(c) > 0.75 {
            inset_n[i]
        } else {
            seg_n[seg]
        }
    };
    // stations along the length: (z, inset, weight of the outline normal, z part of the normal)
    let mut st: Vec<(f32, f32, f32, f32)> = Vec::new();
    let arc = |k: usize| k as f32 / steps as f32 * FRAC_PI_2;
    if round[0] > 0.0 {
        for k in (1..=steps).rev() {
            let t = arc(k);
            st.push((z0 + round[0] * (1.0 - t.sin()), round[0] * (1.0 - t.cos()), t.cos(), -t.sin()));
        }
    }
    st.push((z0 + round[0], 0.0, 1.0, 0.0));
    st.push((z1 - round[1], 0.0, 1.0, 0.0));
    if round[1] > 0.0 {
        for k in 1..=steps {
            let t = arc(k);
            st.push((z1 - round[1] * (1.0 - t.sin()), round[1] * (1.0 - t.cos()), t.cos(), t.sin()));
        }
    }
    let (ylo, yhi) = half.iter().fold((f32::MAX, f32::MIN), |(lo, hi), (p, _)| (lo.min(p.y), hi.max(p.y)));
    let ao = |y: f32| 0.72 + 0.28 * ((y - ylo) / (yhi - ylo).max(1e-3)).clamp(0.0, 1.0);
    let at = |i: usize, s: f32, inset: f32| {
        let p = half[i].0 - inset_n[i] * inset;
        Vec2::new(p.x * s, p.y)
    };
    for seg in 0..n - 1 {
        style(b, half[seg].1);
        for s in [1.0f32, -1.0] {
            let mut prev: Option<[u32; 2]> = None;
            for &(z, inset, w, nz) in &st {
                let mut ids = [0u32; 2];
                for (k, i) in [seg, seg + 1].into_iter().enumerate() {
                    let p = at(i, s, inset);
                    let sn = shade(seg, i);
                    ids[k] = b.vert(Vec3::new(p.x, p.y, z), Vec3::new(sn.x * s * w, sn.y * w, nz).normalize_or_zero(), ao(p.y));
                }
                if let Some(pv) = prev {
                    b.quad(pv[0], pv[1], ids[1], ids[0]);
                }
                prev = Some(ids);
            }
        }
    }
    // end caps, cut into horizontal strips so each keeps the colour of its band
    for (station, nz) in [(0usize, -1.0f32), (st.len() - 1, 1.0)] {
        let (z, inset, _, _) = st[station];
        for i in 0..n - 1 {
            style(b, half[i].1);
            let (r0, r1) = (at(i, 1.0, inset), at(i + 1, 1.0, inset));
            let q = [Vec3::new(r0.x, r0.y, z), Vec3::new(r1.x, r1.y, z), Vec3::new(-r1.x, r1.y, z), Vec3::new(-r0.x, r0.y, z)];
            let ids = q.map(|p| b.vert(p, Vec3::Z * nz, ao(p.y)));
            b.quad(ids[0], ids[1], ids[2], ids[3]);
        }
    }
}

/// A round tube between two points (rails, rungs, cables).
fn tube(b: &mut MeshBuilder, p0: Vec3, p1: Vec3, r: f32, seg: u32) {
    let d = p1 - p0;
    let len = d.length();
    if len < 1e-4 {
        return;
    }
    let q = Quat::from_rotation_arc(Vec3::Y, d / len);
    b.with_xf(Mat4::from_rotation_translation(q, p0), |b| b.cylinder(Vec3::ZERO, r, r, len, seg, false, false));
}

/// Height of the Battle Bus's flat roof above the bus's origin. The riders stand on it (see `game::scene`).
pub const BUS_ROOF_Y: f32 = 1.21;

/// The bus body's cross-section, right half, for [`loft`]: blue below a yellow stripe and the window band, white roof.
const BUS_BODY: [(Vec2, u8); 13] = [
    (Vec2::new(0.0, -1.1), 0),
    (Vec2::new(1.36, -1.1), 0),
    (Vec2::new(1.47, -1.04), 0),
    (Vec2::new(1.5, -0.92), 0),
    (Vec2::new(1.5, -0.52), 1),
    (Vec2::new(1.5, -0.4), 0),
    (Vec2::new(1.5, -0.05), 2),
    (Vec2::new(1.455, 0.72), 3),
    (Vec2::new(1.44, 0.95), 3),
    (Vec2::new(1.4, 1.08), 3),
    (Vec2::new(1.32, 1.17), 3),
    (Vec2::new(1.2, BUS_ROOF_Y), 3),
    (Vec2::new(0.0, BUS_ROOF_Y), 3),
];
/// Where the body starts and ends along Z, and how far its front and back are rounded over.
const BUS_BODY_Z: [f32; 2] = [-4.3, 4.3];
const BUS_BODY_ROUND: [f32; 2] = [0.3, 0.45];
/// The bonnet's cross-section, its length and the rounding of its nose.
const BUS_BONNET: [(Vec2, u8); 8] = [
    (Vec2::new(0.0, -1.1), 0),
    (Vec2::new(1.3, -1.1), 0),
    (Vec2::new(1.38, -1.02), 0),
    (Vec2::new(1.4, -0.5), 0),
    (Vec2::new(1.36, -0.3), 0),
    (Vec2::new(1.26, -0.19), 0),
    (Vec2::new(1.1, -0.14), 0),
    (Vec2::new(0.0, -0.12), 0),
];
const BUS_BONNET_Z: [f32; 2] = [-5.55, -4.0];
const BUS_BONNET_ROUND: [f32; 2] = [0.16, 0.0];

/// The bus's painted shell: the body and the bonnet, each a closed, rounded loft. Everything else on the bus sits on it.
fn bus_shell(b: &mut MeshBuilder, paint: &dyn Fn(&mut MeshBuilder, u8)) {
    loft(b, &BUS_BODY, BUS_BODY_Z[0], BUS_BODY_Z[1], BUS_BODY_ROUND, 3, paint);
    loft(b, &BUS_BONNET, BUS_BONNET_Z[0], BUS_BONNET_Z[1], BUS_BONNET_ROUND, 3, paint);
}

/// How far out the side of a cross-section (`half`, as for [`loft`]) is at height `y`.
fn outline_x(half: &[(Vec2, u8)], y: f32) -> f32 {
    let mut x = 0.0f32;
    for w in half.windows(2) {
        let (a, c) = (w[0].0, w[1].0);
        if y < a.y.min(c.y) || y > a.y.max(c.y) {
            continue;
        }
        x = x.max(if (c.y - a.y).abs() < 1e-6 { a.x.max(c.x) } else { a.x + (c.x - a.x) * (y - a.y) / (c.y - a.y) });
    }
    x
}

/// The balloon's envelope: the mouth's height above the bus's origin, the height from the mouth to the crown, the widest
/// radius, and its profile as (height fraction from the mouth up to the crown, radius fraction) at the seams between gores.
const BALLOON_MOUTH_Y: f32 = 5.4;
const BALLOON_HEIGHT: f32 = 13.4;
const BALLOON_RADIUS: f32 = 6.2;
const BALLOON_SHAPE: [(f32, f32); 13] = [(0.0, 0.14), (0.06, 0.22), (0.15, 0.38), (0.27, 0.58), (0.4, 0.79), (0.52, 0.94), (0.63, 1.0), (0.73, 0.98), (0.82, 0.9), (0.89, 0.76), (0.95, 0.55), (0.985, 0.3), (1.0, 0.0)];
/// How far the middle of a gore bulges out past its seams.
const BALLOON_BULGE: f32 = 1.035;

/// The widest the balloon is at height `y` above the bus's origin (to the middle of a gore; the skirt below the mouth
/// counts), or `None` above or below it.
pub fn bus_envelope_radius(y: f32) -> Option<f32> {
    let h = (y - BALLOON_MOUTH_Y) / BALLOON_HEIGHT;
    if (-0.5 / BALLOON_HEIGHT..0.0).contains(&h) {
        return Some(0.98);
    }
    BALLOON_SHAPE.windows(2).find(|w| (w[0].0..=w[1].0).contains(&h)).map(|w| {
        let k = (h - w[0].0) / (w[1].0 - w[0].0);
        lerp(w[0].1, w[1].1, k) * BALLOON_RADIUS * BALLOON_BULGE
    })
}

/// Radius of the cables that hold the bus under its balloon.
pub const BUS_CABLE_R: f32 = 0.04;

/// The four cables between the balloon and the bus, as (top, bottom) points on their centre lines in the bus's frame. They
/// hang from the envelope's seams, well out from the bus, and come down outside the roof's edge to the sides of the bus
/// just under it, so they never cross the roof deck that the riders stand on (see `game::scene::bus_rider_spot`).
pub fn bus_cables() -> [(Vec3, Vec3); 4] {
    let (h, r) = BALLOON_SHAPE[3];
    let t = r * BALLOON_RADIUS * std::f32::consts::FRAC_1_SQRT_2;
    let y = BALLOON_MOUTH_Y + h * BALLOON_HEIGHT;
    [(1.0f32, 1.0f32), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)].map(|(sx, sz)| (Vec3::new(sx * t, y, sz * t), Vec3::new(sx * 1.47, 0.85, if sz > 0.0 { 3.72 } else { -3.85 })))
}

/// The Battle Bus hanging under its hot-air balloon. Origin at the centre of the bus body; the front faces -Z.
///
/// A blue bus with rounded ends and a white roof: a row of windows framed by pillars and a chrome sill wraps round into
/// the windscreen and the rear window; a short bonnet carries the grille, round headlights and the front bumper; a door
/// on the right, tail lights, a rear bumper and a ladder up the back to a roof rack with luggage at its front. Four
/// cables run from the envelope down to the sides of the bus; the burner hangs under the envelope's mouth.
///
/// Everything bolted onto the body sits on its surface, even where the ends round over (see the tests).
pub fn bus() -> MeshData {
    let mut b = MeshBuilder::new();
    const BLUE: u32 = 0x2f7fe6;
    const WHITE: u32 = 0xf4f7fb;
    let paint = |b: &mut MeshBuilder, band: u8| {
        match band {
            0 => b.mat(mat::PLASTER).hex(BLUE).spec(0.4),
            1 => b.mat(mat::PLASTER).hex(0xffc531).spec(0.4),
            2 => b.mat(mat::GLASS).hex(0x22344a).spec(0.9),
            _ => b.mat(mat::PLASTER).hex(WHITE).spec(0.35),
        };
    };
    let chrome = |b: &mut MeshBuilder| {
        b.mat(mat::METAL).hex(0xaab2bc).spec(0.9);
    };
    let dark = |b: &mut MeshBuilder| {
        b.mat(mat::FLAT).hex(0x16191f).spec(0.1);
    };
    let lamp = |b: &mut MeshBuilder, rgb: u32| {
        b.mat(mat::EMISSIVE).hex(rgb);
    };
    let [zf, zr] = BUS_BODY_Z;
    let p = Vec2::new;
    bus_shell(&mut b, &paint);

    // ---- the window row: pillars between the panes and a chrome sill below them; wheel wells and mirrors ------------------
    let (sill, header) = (p(1.5, -0.05), p(1.455, 0.72));
    let slant = (sill.x - header.x).atan2(header.y - sill.y);
    let mid = (sill + header) * 0.5;
    for s in [1.0f32, -1.0] {
        // the left side is the right side mirrored
        b.push_xf(Mat4::from_scale(Vec3::new(s, 1.0, 1.0)));
        paint(&mut b, 0);
        b.push_xf(Mat4::from_translation(Vec3::new(mid.x, mid.y, 0.0)) * Mat4::from_rotation_z(slant));
        for k in 0..=6 {
            let z = lerp(-3.8, 3.82, k as f32 / 6.0);
            b.box_faces(Vec3::new(-0.01, -0.4, z - 0.085), Vec3::new(0.035, 0.4, z + 0.085), 0b111101);
        }
        if s > 0.0 {
            // the split between the two leaves of the door
            b.box_faces(Vec3::new(-0.01, -0.4, -3.21), Vec3::new(0.03, 0.4, -3.15), 0b111101);
        }
        b.pop_xf();
        chrome(&mut b);
        b.box_faces(Vec3::new(1.49, -0.09, -3.86), Vec3::new(1.535, -0.02, 3.88), 0b111101);
        // dark wheel wells behind the tyres, laid onto the side of the bonnet (front) and the body (back) where they curve
        // in towards the bottom
        dark(&mut b);
        for (outline, zc, r) in [(&BUS_BONNET[..], -4.65f32, 0.66f32), (&BUS_BODY[..], 2.85, 0.7)] {
            let on_side = |y: f32, z: f32| Vec3::new(outline_x(outline, y) + 0.004, y, z);
            let hub = b.vert(on_side(-1.1, zc), Vec3::X, 0.4);
            let mut inner: Vec<u32> = vec![hub; 11];
            for rr in [0.3, 0.52, r] {
                let ring: Vec<u32> = (0..=10)
                    .map(|k| {
                        let a = k as f32 / 10.0 * PI;
                        b.vert(on_side(-1.1 + a.sin() * rr, zc + a.cos() * rr), Vec3::X, 0.55)
                    })
                    .collect();
                for k in 0..10 {
                    if inner[k] == inner[k + 1] {
                        b.tri(inner[k], ring[k], ring[k + 1]);
                    } else {
                        b.quad(inner[k], ring[k], ring[k + 1], inner[k + 1]);
                    }
                }
                inner = ring;
            }
        }
        // a mirror on an arm at each front corner
        b.mat(mat::METAL).hex(0x30353d).spec(0.5);
        tube(&mut b, Vec3::new(1.44, 0.45, -4.05), Vec3::new(1.74, 0.42, -4.28), 0.03, 4);
        b.box_center(Vec3::new(1.77, 0.28, -4.28), Vec3::new(0.05, 0.17, 0.1));
        b.pop_xf();
    }
    // the door on the right: glass in its lower half, a frame round it and a step under it
    b.mat(mat::GLASS).hex(0x2a4058).spec(0.9);
    b.box_faces(Vec3::new(1.495, -0.96, -3.72), Vec3::new(1.52, -0.06, -2.64), 0b111101);
    paint(&mut b, 0);
    b.box_faces(Vec3::new(1.495, -1.0, -3.21), Vec3::new(1.528, -0.06, -3.15), 0b111101);
    b.box_faces(Vec3::new(1.495, -1.04, -3.8), Vec3::new(1.528, -0.94, -2.56), 0b111101);
    b.mat(mat::METAL).hex(0x4a5059).spec(0.5);
    b.box_min_max(Vec3::new(1.3, -1.2, -3.75), Vec3::new(1.62, -1.09, -2.6));

    // ---- wheels: tyres with chrome hubs ----------------------------------------------------------------------------------
    for s in [1.0f32, -1.0] {
        for z in [-4.65f32, 2.85] {
            b.push_xf(Mat4::from_translation(Vec3::new(s * 1.42, -1.1, z)) * Mat4::from_rotation_z(-s * FRAC_PI_2));
            b.mat(mat::FLAT).hex(0x1c1d21).spec(0.15).ao(0.8, 1.0);
            b.cylinder(Vec3::new(0.0, -0.2, 0.0), 0.6, 0.6, 0.4, 16, true, true);
            chrome(&mut b);
            b.cylinder(Vec3::new(0.0, 0.2, 0.0), 0.36, 0.33, 0.03, 12, false, true);
            b.mat(mat::METAL).hex(0x3b414a).spec(0.6);
            b.cylinder(Vec3::new(0.0, 0.22, 0.0), 0.13, 0.1, 0.07, 8, false, true);
            b.ao(1.0, 1.0);
            b.pop_xf();
        }
    }

    // ---- front: grille, headlights, indicators, bumper and plate on the flat of the bonnet's nose; windscreen pillar,
    // wipers and the destination sign ------------------------------------------------------------------------------------
    let zn = BUS_BONNET_Z[0];
    dark(&mut b);
    b.box_center(Vec3::new(0.0, -0.6, zn - 0.008), Vec3::new(0.5, 0.25, 0.012));
    chrome(&mut b);
    for y in [-0.79f32, -0.67, -0.55, -0.43] {
        b.box_center(Vec3::new(0.0, y, zn - 0.018), Vec3::new(0.5, 0.011, 0.008));
    }
    b.box_center(Vec3::new(0.0, -0.33, zn - 0.016), Vec3::new(0.56, 0.03, 0.016));
    b.box_center(Vec3::new(0.0, -0.865, zn - 0.016), Vec3::new(0.56, 0.02, 0.016));
    for s in [1.0f32, -1.0] {
        b.push_xf(Mat4::from_translation(Vec3::new(s * 0.88, -0.55, zn)) * rot_x(-FRAC_PI_2));
        chrome(&mut b);
        b.cylinder(Vec3::ZERO, 0.17, 0.16, 0.05, 12, false, true);
        lamp(&mut b, 0xfff3c4);
        b.cylinder(Vec3::ZERO, 0.125, 0.11, 0.075, 12, false, true);
        b.pop_xf();
        lamp(&mut b, 0xffa529);
        b.box_center(Vec3::new(s * 0.88, -0.8, zn - 0.01), Vec3::new(0.1, 0.035, 0.014));
    }
    // the bumper reaches back into the nose, where it rounds over at the bottom, and wraps round the corners
    chrome(&mut b);
    b.box_min_max(Vec3::new(-1.3, -1.11, -5.72), Vec3::new(1.3, -0.89, -5.42));
    for s in [1.0f32, -1.0] {
        b.box_min_max(Vec3::new(s * 1.3 - 0.1, -1.11, -5.62), Vec3::new(s * 1.3 + 0.1, -0.89, -5.3));
    }
    b.mat(mat::FLAT).hex(0xf2f0e6);
    b.box_center(Vec3::new(0.0, -1.0, -5.724), Vec3::new(0.3, 0.075, 0.004));
    paint(&mut b, 0);
    b.box_center(Vec3::new(0.0, 0.2975, zf - 0.012), Vec3::new(0.05, 0.4275, 0.014));
    dark(&mut b);
    for s in [1.0f32, -1.0] {
        // the wipers rest at the foot of the windscreen, their inner ends on the bonnet
        b.push_xf(Mat4::from_translation(Vec3::new(s * 0.5, 0.012, zf - 0.01)) * Mat4::from_rotation_z(s * 0.35));
        b.box_center(Vec3::ZERO, Vec3::new(0.36, 0.014, 0.012));
        b.pop_xf();
    }
    lamp(&mut b, 0xffcf5a);
    b.box_center(Vec3::new(0.0, 0.81, zf - 0.008), Vec3::new(0.62, 0.065, 0.01));

    // ---- back: tail lights and an engine grille on the flat of the rear, the bumper, a plate and a ladder up to the roof --
    for s in [1.0f32, -1.0] {
        lamp(&mut b, 0xff2a1e);
        b.box_center(Vec3::new(s * 0.85, -0.42, zr + 0.012), Vec3::new(0.1, 0.18, 0.014));
        lamp(&mut b, 0xffa529);
        b.box_center(Vec3::new(s * 0.85, -0.12, zr + 0.012), Vec3::new(0.1, 0.05, 0.014));
    }
    dark(&mut b);
    b.box_center(Vec3::new(0.0, -0.42, zr + 0.006), Vec3::new(0.25, 0.18, 0.01));
    chrome(&mut b);
    for y in [-0.53f32, -0.42, -0.31] {
        b.box_center(Vec3::new(0.0, y, zr + 0.018), Vec3::new(0.25, 0.02, 0.012));
    }
    // the bumper reaches into the body where the rear rounds over at the bottom
    b.box_min_max(Vec3::new(-1.4, -1.13, 4.0), Vec3::new(1.4, -0.89, 4.37));
    b.mat(mat::FLAT).hex(0xf2f0e6);
    b.box_center(Vec3::new(0.0, -1.0, 4.376), Vec3::new(0.3, 0.085, 0.006));
    // the ladder stands on the flat of the rear, follows the rounding over the top and lies along the roof
    b.mat(mat::METAL).hex(0x9aa3ad).spec(0.7);
    let (rr, ry_top) = (BUS_BODY_ROUND[1], BUS_ROOF_Y - BUS_BODY_ROUND[1]);
    let (lz, bend) = (zr + 0.025, rr + 0.025);
    for x in [-0.6f32, -0.34] {
        tube(&mut b, Vec3::new(x, -0.6, lz), Vec3::new(x, ry_top, lz), 0.028, 6);
        let arc = |k: usize| {
            let a = k as f32 / 4.0 * FRAC_PI_2;
            Vec3::new(x, ry_top + a.sin() * bend, zr - rr + a.cos() * bend)
        };
        for k in 0..4 {
            tube(&mut b, arc(k), arc(k + 1), 0.028, 6);
        }
        tube(&mut b, arc(4), arc(4) - Vec3::Z * 0.4, 0.028, 6);
    }
    for k in 0..5 {
        let y = -0.5 + k as f32 * 0.28;
        tube(&mut b, Vec3::new(-0.6, y, lz), Vec3::new(-0.34, y, lz), 0.022, 4);
    }

    // ---- roof rack: rails on posts, runners along the sides and luggage strapped down at the front; the deck where the
    // riders stand stays clear --------------------------------------------------------------------------------------------
    let ry = BUS_ROOF_Y;
    let (rx, rz0, rz1, rh) = (1.1f32, -3.75f32, 3.6f32, 0.32f32);
    b.mat(mat::METAL).hex(0x2e333b).spec(0.6);
    for s in [1.0f32, -1.0] {
        tube(&mut b, Vec3::new(s * rx, ry + rh, rz0), Vec3::new(s * rx, ry + rh, rz1), 0.035, 6);
        for k in 0..5 {
            let z = lerp(rz0, rz1, k as f32 / 4.0);
            tube(&mut b, Vec3::new(s * rx, ry, z), Vec3::new(s * rx, ry + rh, z), 0.03, 6);
        }
        tube(&mut b, Vec3::new(s * 1.0, ry + 0.022, rz0), Vec3::new(s * 1.0, ry + 0.022, rz1), 0.025, 6);
    }
    for z in [rz0, rz1] {
        tube(&mut b, Vec3::new(-rx, ry + rh, z), Vec3::new(rx, ry + rh, z), 0.035, 6);
    }
    b.mat(mat::CLOTH).ao(0.75, 1.0);
    b.hex(0x9c6233);
    b.box_center(Vec3::new(-0.52, ry + 0.197, -3.38), Vec3::new(0.34, 0.2, 0.22));
    b.hex(0x27a7a0);
    b.box_center(Vec3::new(0.36, ry + 0.147, -3.4), Vec3::new(0.3, 0.15, 0.2));
    b.hex(0xd64933);
    b.box_center(Vec3::new(-0.46, ry + 0.474, -3.4), Vec3::new(0.2, 0.08, 0.15));
    b.mat(mat::FLAT).hex(0x2b2118).ao(1.0, 1.0);
    b.box_center(Vec3::new(-0.62, ry + 0.2, -3.38), Vec3::new(0.04, 0.21, 0.235));
    b.box_center(Vec3::new(0.36, ry + 0.15, -3.4), Vec3::new(0.04, 0.16, 0.215));

    // ---- balloon: sixteen pillowy gores, a white crown, the skirt and load ring at the mouth, the burner and the cables ----
    let (mouth_y, height, rad) = (BALLOON_MOUTH_Y, BALLOON_HEIGHT, BALLOON_RADIUS);
    let prof: Vec<Vec2> = BALLOON_SHAPE.iter().map(|&(h, r)| p(r * rad, mouth_y + h * height)).collect();
    let prof_n: Vec<Vec2> = (0..prof.len())
        .map(|k| {
            let d = prof[(k + 1).min(prof.len() - 1)] - prof[k.saturating_sub(1)];
            Vec2::new(d.y, -d.x).normalize_or_zero()
        })
        .collect();
    let gores = 16usize;
    b.mat(mat::CLOTH).spec(0.2);
    for g in 0..gores {
        let (a0, a1) = (g as f32 / gores as f32 * TAU_F, (g + 1) as f32 / gores as f32 * TAU_F);
        b.hex(if g % 2 == 0 { 0xf2493b } else { 0xfcd849 });
        let mut prev: Option<[u32; 3]> = None;
        for (k, (q, nq)) in prof.iter().zip(&prof_n).enumerate() {
            // darker towards the mouth, where the envelope is in its own shade
            let ao = 0.7 + 0.3 * (k as f32 / 6.0).min(1.0);
            let mut ids = [0u32; 3];
            // each gore bulges between its seams: the middle stands out and the normals at the seams lean away from it
            for (c, (a, bulge, tilt)) in [(a0, 1.0, -0.28f32), ((a0 + a1) * 0.5, BALLOON_BULGE, 0.0), (a1, 1.0, 0.28)].into_iter().enumerate() {
                let (sa, ca) = a.sin_cos();
                let radial = Vec3::new(ca, 0.0, sa);
                let tangent = Vec3::new(-sa, 0.0, ca);
                let pos = Vec3::new(ca * q.x * bulge, q.y, sa * q.x * bulge);
                let nrm = (radial * nq.x + Vec3::Y * nq.y + tangent * tilt * nq.x.abs()).normalize_or_zero();
                ids[c] = b.vert(pos, if nrm == Vec3::ZERO { Vec3::Y } else { nrm }, ao);
            }
            if let Some(pv) = prev {
                b.quad(pv[0], pv[1], ids[1], ids[0]);
                b.quad(pv[1], pv[2], ids[2], ids[1]);
            }
            prev = Some(ids);
        }
    }
    b.mat(mat::CLOTH).hex(0xf6f3ea);
    b.cylinder(Vec3::new(0.0, mouth_y + height * 0.985 - 0.05, 0.0), 1.95, 1.2, 0.32, 16, false, true);
    b.hex(0x8e231c).ao(0.6, 1.0);
    b.cylinder(Vec3::new(0.0, mouth_y - 0.5, 0.0), 0.98, 0.87, 0.52, 16, false, false);
    b.ao(1.0, 1.0);
    chrome(&mut b);
    b.cylinder(Vec3::new(0.0, mouth_y - 0.06, 0.0), 0.92, 0.92, 0.08, 16, false, false);
    // the burner hangs in a frame under the mouth
    let (fy, fc) = (4.45f32, 0.55f32);
    b.mat(mat::METAL).hex(0x50565f).spec(0.6);
    let corners = [Vec3::new(fc, fy, fc), Vec3::new(-fc, fy, fc), Vec3::new(-fc, fy, -fc), Vec3::new(fc, fy, -fc)];
    for k in 0..4 {
        tube(&mut b, corners[k], corners[(k + 1) % 4], 0.035, 6);
        tube(&mut b, corners[k], corners[k] * Vec3::new(1.15, 0.0, 1.15) + Vec3::Y * (mouth_y - 0.05), 0.025, 4);
    }
    for x in [-0.2f32, 0.2] {
        b.cylinder(Vec3::new(x, fy - 0.12, 0.0), 0.15, 0.17, 0.4, 8, true, true);
    }
    lamp(&mut b, 0xff8a2a);
    for x in [-0.2f32, 0.2] {
        b.cylinder(Vec3::new(x, fy + 0.28, 0.0), 0.12, 0.0, 0.55, 8, false, false);
    }
    // the cables from the envelope's seams down to brackets on the sides of the bus
    for (top, bottom) in bus_cables() {
        b.mat(mat::FLAT).hex(0x3b3530);
        tube(&mut b, top, bottom, BUS_CABLE_R, 6);
        chrome(&mut b);
        b.box_center(Vec3::new(bottom.x.signum() * 1.47, bottom.y, bottom.z), Vec3::new(0.042, 0.06, 0.06));
    }
    b.finish()
}

pub fn rocket_missile() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(0xdfe3e8).ao(0.8, 1.0).spec(0.6);
    barrel(&mut b, 0.0, 0.0, 0.3, -0.18, 0.06, 0.06, 10);
    b.mat(mat::FLAT).hex(0xd6492f);
    barrel(&mut b, 0.0, 0.0, -0.18, -0.38, 0.06, 0.0, 10);
    b.hex(0xd6492f);
    for k in 0..4 {
        let a = k as f32 * FRAC_PI_2;
        b.push_xf(Mat4::from_rotation_z(a));
        b.box_center(Vec3::new(0.0, 0.09, 0.26), Vec3::new(0.007, 0.055, 0.06));
        b.pop_xf();
    }
    b.mat(mat::EMISSIVE).tinted(false).color(Vec3::new(1.0, 0.7, 0.25));
    barrel(&mut b, 0.0, 0.0, 0.34, 0.3, 0.05, 0.05, 8);
    b.finish()
}

// =======================================================================================
// Build pieces. Local frame: the cell's min corner is the origin, the piece spans
// x in [0, TILE]; instances place and rotate them (see `game::rig::piece_transform`).
// =======================================================================================

use crate::game::pieces::{LEVEL_H, THICK, TILE};

pub fn piece_wall() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::BUILD).color(grey(1.0)).ao(0.6, 1.0);
    let (t, h, th) = (TILE, LEVEL_H, THICK);
    // inner panel
    b.box_min_max(Vec3::new(0.12, 0.1, -th * 0.27), Vec3::new(t - 0.12, h - 0.1, th * 0.27));
    // frame
    b.color(grey(0.78));
    let fw = 0.22;
    b.box_min_max(Vec3::new(0.0, 0.0, -th / 2.0), Vec3::new(fw, h, th / 2.0));
    b.box_min_max(Vec3::new(t - fw, 0.0, -th / 2.0), Vec3::new(t, h, th / 2.0));
    b.box_min_max(Vec3::new(fw, 0.0, -th / 2.0), Vec3::new(t - fw, 0.2, th / 2.0));
    b.box_min_max(Vec3::new(fw, h - 0.2, -th / 2.0), Vec3::new(t - fw, h, th / 2.0));
    // cross braces
    b.color(grey(0.86));
    for s in [1.0f32, -1.0] {
        let (a, c) = (Vec3::new(fw, 0.2, s * th * 0.3), Vec3::new(t - fw, h - 0.2, s * th * 0.3));
        let d = c - a;
        let len = d.length();
        let dir = d / len;
        let right = dir.cross(Vec3::Z).normalize();
        let n = dir.cross(right).normalize();
        let _ = n;
        b.push_xf(Mat4::from_translation(a) * Mat4::from_quat(Quat::from_rotation_arc(Vec3::X, dir)));
        b.box_min_max(Vec3::new(0.0, -0.09, -0.02 * s.abs()), Vec3::new(len, 0.09, 0.03 * s));
        b.pop_xf();
    }
    b.finish()
}

pub fn piece_floor() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::BUILD).color(grey(1.0)).ao(0.55, 1.0);
    let (t, th) = (TILE, THICK);
    b.box_min_max(Vec3::new(0.0, -th, 0.0), Vec3::new(t, 0.0, t));
    // border trim raised a little
    b.color(grey(0.8));
    let bw = 0.18;
    b.box_min_max(Vec3::new(0.0, -th, 0.0), Vec3::new(t, 0.04, bw));
    b.box_min_max(Vec3::new(0.0, -th, t - bw), Vec3::new(t, 0.04, t));
    b.box_min_max(Vec3::new(0.0, -th, bw), Vec3::new(bw, 0.04, t - bw));
    b.box_min_max(Vec3::new(t - bw, -th, bw), Vec3::new(t, 0.04, t - bw));
    b.finish()
}

pub fn piece_roof() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::BUILD).color(grey(1.0)).ao(0.55, 1.0);
    let (t, h, th) = (TILE, LEVEL_H, THICK);
    b.box_min_max(Vec3::new(0.0, h - th, 0.0), Vec3::new(t, h, t));
    b.color(grey(0.8));
    let bw = 0.2;
    b.box_min_max(Vec3::new(0.0, h - th, 0.0), Vec3::new(t, h + 0.06, bw));
    b.box_min_max(Vec3::new(0.0, h - th, t - bw), Vec3::new(t, h + 0.06, t));
    b.box_min_max(Vec3::new(0.0, h - th, bw), Vec3::new(bw, h + 0.06, t - bw));
    b.box_min_max(Vec3::new(t - bw, h - th, bw), Vec3::new(t, h + 0.06, t - bw));
    // ridge boards
    b.color(grey(0.9));
    b.box_min_max(Vec3::new(bw, h, t * 0.5 - 0.08), Vec3::new(t - bw, h + 0.09, t * 0.5 + 0.08));
    b.box_min_max(Vec3::new(t * 0.5 - 0.08, h, bw), Vec3::new(t * 0.5 + 0.08, h + 0.09, t - bw));
    // underside beams
    b.color(grey(0.7));
    b.box_min_max(Vec3::new(bw, h - th - 0.12, t * 0.5 - 0.1), Vec3::new(t - bw, h - th, t * 0.5 + 0.1));
    b.finish()
}

/// Ramp rising toward +X (instances rotate it for the other directions).
pub fn piece_ramp() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::BUILD).color(grey(1.0)).ao(0.55, 1.0);
    let (t, h) = (TILE, LEVEL_H);
    // the solid wedge
    b.wedge(Vec3::new(0.0, 0.0, 0.0), Vec3::new(t, h, t), 0);
    // side rails along the slope
    b.color(grey(0.8));
    let slope = h / t;
    let rail = |b: &mut MeshBuilder, z0: f32, z1: f32| {
        let a = Vec3::new(0.0, 0.0, z0);
        let d = Vec3::new(t, h, 0.0);
        let len = d.length();
        b.push_xf(Mat4::from_translation(a) * Mat4::from_quat(Quat::from_rotation_arc(Vec3::X, d / len)));
        b.box_min_max(Vec3::new(0.0, 0.0, 0.0), Vec3::new(len, 0.12, z1 - z0));
        b.pop_xf();
    };
    rail(&mut b, -0.02, 0.18);
    rail(&mut b, t - 0.18, t + 0.02);
    // cross boards on the walking surface
    b.color(grey(0.9));
    let boards = 8;
    for k in 0..boards {
        let x = (k as f32 + 0.5) / boards as f32 * t;
        let y = x * slope;
        b.box_center(Vec3::new(x, y + 0.02, t * 0.5), Vec3::new(0.03, 0.025, t * 0.5 - 0.15));
    }
    b.finish()
}

// =======================================================================================
// Tests
// =======================================================================================

/// A thin straight rod between two points (parachute cords).
fn rod(b: &mut MeshBuilder, p0: Vec3, p1: Vec3, r: f32) {
    let d = p1 - p0;
    let len = d.length();
    if len < 1e-4 {
        return;
    }
    let q = Quat::from_rotation_arc(Vec3::Y, d / len);
    b.with_xf(Mat4::from_rotation_translation(q, p0), |b| b.cylinder(Vec3::ZERO, r, r, len, 4, false, false));
}

/// The supply drop crate: blue painted planks, white panels with a cross on every side, steel corners and handles, a lifting
/// frame on top and four beacon lights. Origin at the middle of the bottom; 1.2 m wide and 1 m tall.
pub fn supply_crate() -> MeshData {
    let mut b = MeshBuilder::new();
    let (blue, navy, white, steel) = (0x2a6fe0, 0x14398a, 0xf2f5fa, 0xcfd8e6);
    b.mat(mat::WOOD).hex(blue).ao(0.55, 1.0).spec(0.15);
    b.box_min_max(Vec3::new(-0.58, 0.04, -0.58), Vec3::new(0.58, 0.96, 0.58));
    // darker frame: corner posts and rims
    b.hex(navy).spec(0.2);
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            b.box_center(Vec3::new(sx * 0.57, 0.5, sz * 0.57), Vec3::new(0.045, 0.5, 0.045));
        }
    }
    b.box_min_max(Vec3::new(-0.61, 0.0, -0.61), Vec3::new(0.61, 0.07, 0.61));
    b.box_min_max(Vec3::new(-0.61, 0.93, -0.61), Vec3::new(0.61, 1.0, 0.61));
    // a white panel with a blue cross on each of the four sides
    for side in 0..4 {
        b.with_xf(Mat4::from_rotation_y(side as f32 * FRAC_PI_2), |b| {
            b.mat(mat::FLAT).tinted(false).hex(white).ao(0.8, 1.0);
            b.box_center(Vec3::new(0.0, 0.5, -0.585), Vec3::new(0.34, 0.3, 0.008));
            b.hex(0x1d55c4);
            b.box_center(Vec3::new(0.0, 0.5, -0.5935), Vec3::new(0.2, 0.05, 0.004));
            b.box_center(Vec3::new(0.0, 0.5, -0.5935), Vec3::new(0.05, 0.2, 0.004));
        });
    }
    // steel: corner brackets with bolts, two carrying handles
    b.mat(mat::METAL).tinted(true).hex(steel).spec(0.9);
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            b.box_center(Vec3::new(sx * 0.595, 0.93, sz * 0.595), Vec3::new(0.05, 0.05, 0.05));
            b.box_center(Vec3::new(sx * 0.595, 0.07, sz * 0.595), Vec3::new(0.05, 0.05, 0.05));
        }
        b.box_center(Vec3::new(sx * 0.64, 0.62, 0.17), Vec3::new(0.03, 0.02, 0.02));
        b.box_center(Vec3::new(sx * 0.64, 0.62, -0.17), Vec3::new(0.03, 0.02, 0.02));
        b.box_center(Vec3::new(sx * 0.665, 0.62, 0.0), Vec3::new(0.014, 0.014, 0.19));
    }
    // the lifting frame the parachute cords hang from
    b.box_min_max(Vec3::new(-0.3, 1.0, -0.3), Vec3::new(0.3, 1.04, 0.3));
    for (sx, sz) in [(1.0f32, 0.0f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let (hx, hz) = if sx != 0.0 { (0.012, 0.11) } else { (0.11, 0.012) };
        b.box_center(Vec3::new(sx * 0.11, 1.14, sz * 0.11), Vec3::new(hx, 0.012, hz));
    }
    for sx in [-1.0f32, 1.0] {
        b.box_center(Vec3::new(sx * 0.11, 1.09, 0.0), Vec3::new(0.012, 0.05, 0.012));
    }
    // beacon lights on the top corners
    b.mat(mat::EMISSIVE).tinted(false).color(Vec3::new(0.35, 0.75, 1.0));
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            b.sphere(Vec3::new(sx * 0.5, 1.03, sz * 0.5), 0.045, 1);
        }
    }
    b.finish()
}

/// The parachute of a supply crate: a blue and white canopy with cords down to the crate's four top corners.
/// Origin where the cords meet the middle of the crate's top; the canopy is about 4.5 m above.
pub fn supply_chute() -> MeshData {
    let mut b = MeshBuilder::new();
    let segs = 12usize;
    let rings = 5usize;
    let (rad, rise) = (2.6f32, 1.3f32);
    let ctr = Vec3::new(0.0, 4.6, 0.0);
    let pt = |a: f32, t: f32| {
        let rr = rad * (1.0 - t * 0.92);
        let y = rise * (1.0 - (1.0 - t).powi(2));
        ctr + Vec3::new(a.cos() * rr, y - rise * 0.2 * (1.0 - t), a.sin() * rr)
    };
    b.mat(mat::CLOTH).tinted(false).ao(0.8, 1.0);
    for g in 0..segs {
        let a0 = g as f32 / segs as f32 * TAU_F;
        let a1 = (g + 1) as f32 / segs as f32 * TAU_F;
        b.hex(if g % 2 == 0 { 0x2a6fe0 } else { 0xf3f6fb });
        for r in 0..rings {
            let (t0, t1) = (r as f32 / rings as f32, (r + 1) as f32 / rings as f32);
            let p = [pt(a0, t0), pt(a1, t0), pt(a1, t1), pt(a0, t1)];
            let mid = (p[0] + p[1] + p[2] + p[3]) * 0.25;
            let n = (mid - ctr + Vec3::Y * 0.6).normalize();
            b.quad_out(p, n);
            // the underside, seen from below as the crate comes down
            b.quad_out([p[3], p[2], p[1], p[0]], -n);
        }
    }
    // a crown patch and a hem ring
    b.hex(0x14398a);
    let top = pt(0.0, 1.0);
    for k in 0..8 {
        let (a0, a1) = (k as f32 / 8.0 * TAU_F, (k + 1) as f32 / 8.0 * TAU_F);
        let up = Vec3::Y * 0.014;
        b.tri_out(top + up, pt(a0, 0.93) + up, pt(a1, 0.93) + up, Vec3::Y);
    }
    // cords: every other seam down to a top corner of the crate
    b.mat(mat::FLAT).tinted(false).hex(0xe8e2d0);
    let corners = [Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 0.0, -0.5), Vec3::new(0.5, 0.0, 0.5), Vec3::new(-0.5, 0.0, 0.5)];
    for g in 0..segs {
        let a = g as f32 / segs as f32 * TAU_F;
        rod(&mut b, pt(a, 0.0) - Vec3::Y * 0.02, corners[g * 4 / segs], 0.014);
    }
    b.finish()
}

/// A frag grenade: an olive egg with grooves, a steel fuse head, the spoon down its side and the ring of the pin.
/// Origin at the middle of the bottom; about 12 cm tall.
pub fn grenade_frag() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(0x5d7a3c).spec(0.35).ao(0.6, 1.0);
    b.blob(Vec3::new(0.0, 0.055, 0.0), Vec3::new(0.045, 0.055, 0.045), 2, 0.0, 1, true);
    b.hex(0x3e5328);
    for k in 0..3 {
        b.cylinder(Vec3::new(0.0, 0.028 + k as f32 * 0.024, 0.0), 0.0465, 0.0465, 0.006, 14, false, false);
    }
    b.hex(0xb9bec8).spec(0.9);
    b.cylinder(Vec3::new(0.0, 0.104, 0.0), 0.016, 0.014, 0.022, 10, false, true);
    b.box_center(Vec3::new(0.024, 0.088, 0.0), Vec3::new(0.005, 0.034, 0.011));
    b.hex(0xe0c24a);
    for k in 0..8 {
        let (a0, a1) = (k as f32 / 8.0 * TAU_F, (k + 1) as f32 / 8.0 * TAU_F);
        let c = Vec3::new(-0.026, 0.12, 0.0);
        rod(&mut b, c + Vec3::new(a0.cos(), 0.0, a0.sin()) * 0.014, c + Vec3::new(a1.cos(), 0.0, a1.sin()) * 0.014, 0.0025);
    }
    b.finish()
}

/// An impulse grenade: a blue ball with a glowing band and white caps. Origin at the bottom; about 11 cm tall.
pub fn grenade_impulse() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(0x2d62c8).spec(0.6).ao(0.6, 1.0);
    b.sphere(Vec3::new(0.0, 0.05, 0.0), 0.05, 2);
    b.hex(0xe9eef7);
    b.cylinder(Vec3::new(0.0, 0.088, 0.0), 0.024, 0.02, 0.02, 12, false, true);
    b.cylinder(Vec3::new(0.0, -0.004, 0.0), 0.02, 0.024, 0.016, 12, true, false);
    b.mat(mat::EMISSIVE).tinted(false).color(Vec3::new(0.45, 0.95, 1.0));
    b.cylinder(Vec3::new(0.0, 0.043, 0.0), 0.0515, 0.0515, 0.013, 16, false, false);
    for k in 0..4 {
        let a = k as f32 * FRAC_PI_2;
        b.box_center(Vec3::new(a.cos() * 0.036, 0.075, a.sin() * 0.036), Vec3::splat(0.006));
    }
    b.finish()
}

/// A Boogie Bomb: a little disco ball, mirror tiles and a few lit ones, with a silver cap. Origin at the bottom; 13 cm tall.
pub fn boogie_bomb() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(0xd8ccf5).spec(1.0).ao(0.7, 1.0);
    b.sphere(Vec3::new(0.0, 0.055, 0.0), 0.055, 1);
    b.mat(mat::EMISSIVE).tinted(false);
    let lit = [0xff5fc8, 0x5fe0ff, 0xffe05f, 0xa66bff];
    for k in 0..12 {
        let a = k as f32 / 12.0 * TAU_F;
        let y = 0.055 + ((k * 7 % 5) as f32 / 4.0 - 0.5) * 0.06;
        let r = (0.055f32.powi(2) - (y - 0.055).powi(2)).max(0.0).sqrt() + 0.001;
        b.color(hex(lit[k % lit.len()]));
        b.box_center(Vec3::new(a.cos() * r, y, a.sin() * r), Vec3::splat(0.008));
    }
    b.mat(mat::METAL).tinted(true).hex(0xc4c9d4).spec(0.9);
    b.cylinder(Vec3::new(0.0, 0.104, 0.0), 0.018, 0.015, 0.02, 10, false, true);
    b.cylinder(Vec3::new(0.0, 0.124, 0.0), 0.004, 0.004, 0.012, 6, false, true);
    b.finish()
}

/// The frame of a Launch Pad: a low steel square with hazard stripes round its edge and a spring in each corner.
/// Origin at the middle of the bottom; 1.7 m across.
pub fn launch_pad_base() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(0x3b4350).spec(0.5).ao(0.6, 1.0);
    b.box_min_max(Vec3::new(-0.85, 0.0, -0.85), Vec3::new(0.85, 0.08, 0.85));
    b.mat(mat::FLAT).tinted(false);
    for side in 0..4 {
        b.with_xf(Mat4::from_rotation_y(side as f32 * FRAC_PI_2), |b| {
            for k in 0..8 {
                b.hex(if k % 2 == 0 { 0xf2c12e } else { 0x24272d });
                let x0 = -0.85 + k as f32 * 0.2125;
                b.box_min_max(Vec3::new(x0, 0.08, -0.85), Vec3::new(x0 + 0.2125, 0.115, -0.77));
            }
        });
    }
    b.mat(mat::METAL).tinted(true).hex(0xc9d1dc).spec(0.9);
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            for k in 0..4 {
                b.cylinder(Vec3::new(sx * 0.62, 0.09 + k as f32 * 0.028, sz * 0.62), 0.065, 0.065, 0.012, 10, false, false);
            }
        }
    }
    b.finish()
}

/// The bounce plate of a Launch Pad (origin at the middle of its underside): blue, with yellow chevrons and a glowing rim.
pub fn launch_pad_plate() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(0x2f7df0).spec(0.45).ao(0.75, 1.0);
    b.box_min_max(Vec3::new(-0.72, 0.0, -0.72), Vec3::new(0.72, 0.06, 0.72));
    b.mat(mat::FLAT).tinted(false).hex(0xffd23f);
    for k in 0..2 {
        let z = -0.25 + k as f32 * 0.4;
        for sx in [-1.0f32, 1.0] {
            b.with_xf(Mat4::from_translation(Vec3::new(sx * 0.17, 0.062, z)) * Mat4::from_rotation_y(sx * 0.75), |b| {
                b.box_center(Vec3::ZERO, Vec3::new(0.06, 0.004, 0.2));
            });
        }
    }
    b.mat(mat::EMISSIVE).tinted(false).color(Vec3::new(0.4, 0.8, 1.0));
    for side in 0..4 {
        b.with_xf(Mat4::from_rotation_y(side as f32 * FRAC_PI_2), |b| {
            b.box_min_max(Vec3::new(-0.72, 0.06, -0.72), Vec3::new(0.72, 0.075, -0.68));
        });
    }
    b.finish()
}

/// A Slurp Juice: a round flask of blue juice with a purple swirl, a cork and a label. Origin at the bottom; 18 cm tall.
pub fn item_slurp() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::GLASS).hex(0x36b4ff).spec(0.9).ao(0.75, 1.0);
    b.blob(Vec3::new(0.0, 0.07, 0.0), Vec3::new(0.065, 0.07, 0.065), 2, 0.0, 3, true);
    b.mat(mat::EMISSIVE).tinted(false).color(Vec3::new(0.62, 0.36, 1.0));
    for k in 0..3 {
        let y = 0.04 + k as f32 * 0.03;
        let r = (0.065f32.powi(2) * (1.0 - ((y - 0.07) / 0.07).powi(2))).max(0.0).sqrt() + 0.002;
        b.cylinder(Vec3::new(0.0, y, 0.0), r, r, 0.008, 14, false, false);
    }
    b.mat(mat::GLASS).tinted(true).hex(0x7fd4ff);
    b.cylinder(Vec3::new(0.0, 0.13, 0.0), 0.024, 0.02, 0.035, 12, false, false);
    b.mat(mat::WOOD).hex(0xb8864a);
    b.cylinder(Vec3::new(0.0, 0.16, 0.0), 0.022, 0.024, 0.024, 10, false, true);
    b.mat(mat::FLAT).tinted(false).hex(0xf4f1e6);
    b.box_center(Vec3::new(0.0, 0.07, -0.066), Vec3::new(0.03, 0.022, 0.002));
    b.finish()
}

/// An ammo box: an olive steel case with dark rims, carrying handles and yellow stencilled marks; glowing brass inside.
/// Origin at the middle of the bottom; 0.8 m long. The lid is [`ammo_box_lid`].
pub fn ammo_box_base() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(0x5a6a3b).spec(0.3).ao(0.55, 1.0);
    b.box_min_max(Vec3::new(-0.4, 0.0, -0.25), Vec3::new(0.4, 0.32, 0.25));
    b.hex(0x39432a);
    b.box_min_max(Vec3::new(-0.41, 0.0, -0.26), Vec3::new(0.41, 0.035, 0.26));
    b.box_min_max(Vec3::new(-0.41, 0.285, -0.26), Vec3::new(0.41, 0.32, 0.26));
    for sx in [-1.0f32, 1.0] {
        b.box_center(Vec3::new(sx * 0.415, 0.2, 0.0), Vec3::new(0.012, 0.025, 0.09));
    }
    b.mat(mat::FLAT).tinted(false).hex(0xe9c84a);
    b.box_center(Vec3::new(-0.2, 0.16, -0.252), Vec3::new(0.11, 0.035, 0.002));
    b.box_center(Vec3::new(0.15, 0.16, -0.252), Vec3::new(0.06, 0.035, 0.002));
    b.mat(mat::EMISSIVE).tinted(false).color(Vec3::new(1.0, 0.75, 0.3));
    b.box_min_max(Vec3::new(-0.36, 0.28, -0.21), Vec3::new(0.36, 0.3, 0.21));
    b.finish()
}

/// The lid of an ammo box, hinged on the x axis at the origin (local -Z is the front).
pub fn ammo_box_lid() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(0x5f6f3e).spec(0.3).ao(0.6, 1.0);
    b.box_min_max(Vec3::new(-0.41, 0.0, -0.52), Vec3::new(0.41, 0.05, 0.0));
    b.hex(0x39432a);
    b.box_center(Vec3::new(0.0, 0.06, -0.26), Vec3::new(0.1, 0.012, 0.02));
    b.box_center(Vec3::new(0.0, 0.03, -0.525), Vec3::new(0.05, 0.03, 0.008));
    b.finish()
}

/// A Supply Llama: a purple piñata llama with a striped saddle blanket and a paper fringe. Origin between its feet,
/// facing -Z; about 1.6 m tall.
pub fn llama() -> MeshData {
    let mut b = MeshBuilder::new();
    let (purple, light) = (0x8a58dc, 0xb48bf0);
    b.mat(mat::CLOTH).hex(purple).ao(0.6, 1.0);
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            b.cylinder(Vec3::new(sx * 0.2, 0.0, sz * 0.3), 0.075, 0.085, 0.62, 8, true, false);
        }
    }
    b.blob(Vec3::new(0.0, 0.82, 0.0), Vec3::new(0.3, 0.26, 0.46), 2, 0.02, 7, true);
    rod(&mut b, Vec3::new(0.0, 0.9, -0.32), Vec3::new(0.0, 1.42, -0.48), 0.12);
    b.hex(light);
    b.blob(Vec3::new(0.0, 1.5, -0.56), Vec3::new(0.13, 0.13, 0.19), 2, 0.0, 9, true);
    b.blob(Vec3::new(0.0, 1.45, -0.72), Vec3::new(0.08, 0.07, 0.08), 1, 0.0, 11, true);
    b.hex(purple);
    for sx in [-1.0f32, 1.0] {
        b.cone(Vec3::new(sx * 0.07, 1.6, -0.52), 0.035, 0.14, 6);
    }
    b.blob(Vec3::new(0.0, 0.9, 0.48), Vec3::new(0.06, 0.08, 0.06), 1, 0.0, 13, true);
    b.mat(mat::FLAT).tinted(false).hex(0x15121c);
    for sx in [-1.0f32, 1.0] {
        b.sphere(Vec3::new(sx * 0.085, 1.54, -0.68), 0.022, 1);
    }
    // the saddle blanket, in stripes
    let stripes = [0xffd23f, 0xff5fa8, 0x4fd3ff, 0xffd23f, 0xff5fa8];
    for (k, c) in stripes.iter().enumerate() {
        b.hex(*c);
        let z = -0.24 + k as f32 * 0.12;
        b.box_min_max(Vec3::new(-0.31, 0.78, z), Vec3::new(0.31, 1.09, z + 0.12));
    }
    // paper fringe round the belly
    let fringe = [0xff5fa8, 0xffd23f, 0x4fd3ff, 0x7cf06a];
    for k in 0..16 {
        b.hex(fringe[k % fringe.len()]);
        let a = k as f32 / 16.0 * TAU_F;
        let (x, z) = (a.cos() * 0.29, a.sin() * 0.44);
        b.box_center(Vec3::new(x, 0.6, z), Vec3::new(0.03, 0.06, 0.03));
    }
    b.finish()
}

/// A stack of building material lying on the ground: planks, bricks or metal plates. Origin at the bottom; ~0.5 m long.
pub fn mat_stack(m: u8) -> MeshData {
    let mut b = MeshBuilder::new();
    match m {
        0 => {
            b.mat(mat::WOOD).hex(0xb98450).ao(0.6, 1.0);
            for k in 0..3 {
                let rot = if k % 2 == 0 { 0.0 } else { FRAC_PI_2 * 0.35 };
                b.with_xf(Mat4::from_translation(Vec3::new(0.0, 0.035 + k as f32 * 0.07, 0.0)) * Mat4::from_rotation_y(rot), |b| {
                    b.box_center(Vec3::new(0.0, 0.0, -0.08), Vec3::new(0.25, 0.03, 0.065));
                    b.box_center(Vec3::new(0.0, 0.0, 0.08), Vec3::new(0.25, 0.03, 0.065));
                });
            }
        }
        1 => {
            b.mat(mat::STONE).hex(0xb87a62).ao(0.55, 1.0);
            for k in 0..2 {
                for j in 0..2 {
                    let off = if k % 2 == 0 { 0.0 } else { 0.06 };
                    b.box_center(Vec3::new(-0.1 + j as f32 * 0.21 - off, 0.05 + k as f32 * 0.1, 0.0), Vec3::new(0.1, 0.045, 0.07));
                }
            }
        }
        _ => {
            b.mat(mat::METAL).hex(0x9fb4cf).spec(0.85).ao(0.6, 1.0);
            for k in 0..3 {
                b.box_center(Vec3::new(0.0, 0.02 + k as f32 * 0.045, 0.0), Vec3::new(0.22 - k as f32 * 0.02, 0.018, 0.16 - k as f32 * 0.015));
            }
            b.hex(0x5f6f86);
            for (x, z) in [(-0.17, -0.11), (0.17, -0.11), (-0.17, 0.11), (0.17, 0.11)] {
                b.sphere(Vec3::new(x, 0.022, z), 0.012, 0);
            }
        }
    }
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<(&'static str, MeshData)> {
        vec![
            ("torso", char_torso()),
            ("pelvis", char_pelvis()),
            ("trim", char_trim()),
            ("head", char_head()),
            ("hair1", hair(1)),
            ("hair2", hair(2)),
            ("hair3", hair(3)),
            ("cap", headgear(1)),
            ("beanie", headgear(2)),
            ("helmet", headgear(3)),
            ("hat", headgear(4)),
            ("arm_up", char_arm_up()),
            ("arm_low", char_arm_low()),
            ("hand", char_hand()),
            ("leg_up", char_leg_up()),
            ("leg_low", char_leg_low()),
            ("boot", char_boot()),
            ("backpack", char_backpack()),
            ("glider", glider()),
            ("pistol", weapon_pistol()),
            ("smg", weapon_smg()),
            ("ar", weapon_ar()),
            ("shotgun", weapon_shotgun()),
            ("sniper", weapon_sniper()),
            ("rocket", weapon_rocket()),
            ("pickaxe", pickaxe()),
            ("ammo", ammo_box()),
            ("bandage", item_bandage()),
            ("medkit", item_medkit()),
            ("mini", item_shield_mini()),
            ("big", item_shield_big()),
            ("chug", item_chug()),
            ("chest_base", chest_base()),
            ("chest_lid", chest_lid()),
            ("bus", bus()),
            ("missile", rocket_missile()),
            ("wall", piece_wall()),
            ("floor", piece_floor()),
            ("roof", piece_roof()),
            ("ramp", piece_ramp()),
        ]
    }

    #[test]
    fn every_model_builds_with_valid_geometry() {
        for (name, m) in all() {
            assert!(!m.is_empty(), "{name} is empty");
            assert!(m.verts.len() < 6000, "{name} has {} verts", m.verts.len());
            let bb = m.bounds();
            assert!(bb.min.is_finite() && bb.max.is_finite(), "{name} bounds");
            assert!(m.idx.iter().all(|&i| (i as usize) < m.verts.len()), "{name} indices");
            assert!(m.verts.iter().all(|v| v.nrm.iter().all(|c| c.is_finite()) && v.pos.iter().all(|c| c.is_finite())), "{name} data");
            // no degenerate normals
            let bad = m.verts.iter().filter(|v| (Vec3::from(v.nrm).length() - 1.0).abs() > 0.05).count();
            assert!(bad == 0, "{name}: {bad} vertices with non-unit normals");
        }
    }

    #[test]
    fn body_parts_add_up_to_a_character_of_the_right_height() {
        use dims::*;
        let head_top = HIP_Y + NECK_Y + HEAD_C + char_head().bounds().max.y - HEAD_C;
        assert!((head_top - 1.78).abs() < 0.06, "head top {head_top}");
        let leg_reach = HIP_Y - HIP_DROP - THIGH - SHIN;
        assert!((0.05..0.11).contains(&leg_reach), "ankle height {leg_reach}");
        let boot = char_boot().bounds();
        assert!(boot.min.y > -0.12 && boot.min.y < -0.07, "boot sole {}", boot.min.y);
        // arm segments hang straight down from their joints
        let arm = char_arm_up().bounds();
        assert!(arm.min.y < -UPPER_ARM * 0.95 && arm.max.y < 0.1);
    }

    #[test]
    fn the_battle_bus_keeps_its_size_and_has_a_flat_roof_to_ride_on() {
        let m = bus();
        eprintln!("bus: {} verts, {} tris", m.verts.len(), m.tri_count());
        assert!(m.verts.len() < 4800, "bus has {} verts", m.verts.len());
        let bb = m.bounds();
        let body = m.verts.iter().filter(|v| v.pos[1] < 2.0).fold(Aabb::EMPTY, |mut b, v| {
            b.extend(Vec3::from(v.pos));
            b
        });
        // about 3 m wide (3.6 with the mirrors) and 10 m long from bumper to bumper, hanging under a balloon about 19 m up
        assert!((1.7..1.9).contains(&body.max.x) && (body.min.x + body.max.x).abs() < 1e-3, "width {:?}", (body.min.x, body.max.x));
        assert!((-5.9..-5.5).contains(&body.min.z) && (4.3..4.6).contains(&body.max.z), "length {:?}", (body.min.z, body.max.z));
        assert!((-1.8..-1.6).contains(&bb.min.y) && (18.5..19.2).contains(&bb.max.y), "height {:?}", (bb.min.y, bb.max.y));
        assert!((6.0..6.8).contains(&bb.max.x), "balloon radius {}", bb.max.x);
        // the deck where the riders stand is clear: nothing rises out of the roof between the rack's runners, from behind the
        // luggage at the front to the back
        let deck: Vec<&Vertex> = m.verts.iter().filter(|v| v.pos[0].abs() < 0.95 && (-3.1..3.4).contains(&v.pos[2]) && v.pos[1] > BUS_ROOF_Y + 0.004 && v.pos[1] < 4.0).collect();
        assert!(deck.is_empty(), "something stands on the deck: {:?}", deck.first().map(|v| v.pos));
        assert!(m.verts.iter().any(|v| v.pos[0] == 0.0 && v.pos[1] == BUS_ROOF_Y), "the roof is at the deck height");
        // the cables stay out beyond the roof's edge all the way up past the riders' heads
        for (top, bottom) in bus_cables() {
            // (the top end is on the envelope's seam)
            for k in 0..40 {
                let q = bottom.lerp(top, k as f32 / 40.0);
                if (BUS_ROOF_Y..BUS_ROOF_Y + 3.0).contains(&q.y) {
                    assert!(q.x.abs() - BUS_CABLE_R > 1.45, "a cable comes in over the deck at {q:?}");
                }
                if let Some(r) = bus_envelope_radius(q.y) {
                    assert!(Vec2::new(q.x, q.z).length() > r - 0.05, "a cable runs inside the envelope at {q:?}");
                }
            }
        }
    }

    /// The bus's shell on its own, as triangles, one closed solid per list (the body, then the bonnet).
    fn bus_shell_solids() -> Vec<Vec<[Vec3; 3]>> {
        [(&BUS_BODY[..], BUS_BODY_Z, BUS_BODY_ROUND), (&BUS_BONNET[..], BUS_BONNET_Z, BUS_BONNET_ROUND)]
            .into_iter()
            .map(|(half, z, round)| {
                let mut b = MeshBuilder::new();
                loft(&mut b, half, z[0], z[1], round, 3, &|_, _| {});
                let m = b.finish();
                m.idx.chunks(3).map(|t| [0, 1, 2].map(|k| Vec3::from(m.verts[t[k] as usize].pos))).collect()
            })
            .collect()
    }

    /// Whether `p` lies inside one of the closed `solids` (an odd number of crossings along a ray).
    fn inside(solids: &[Vec<[Vec3; 3]>], p: Vec3) -> bool {
        let d = Vec3::new(0.137, 0.981, 0.093).normalize();
        solids.iter().any(|s| s.iter().filter(|t| ray_tri(p, d, t[0], t[1], t[2]).is_some()).count() % 2 == 1)
    }

    /// How far a ray from `p` along `d` goes before it meets one of the `tris`, measured square to the surface it meets (so a
    /// thin tube lying on a surface reads as touching it, not as far off along a glancing ray). Glancing hits do not count,
    /// nor do triangles with a corner at `skip` (the face the ray starts from).
    fn gap_to(tris: &[[Vec3; 3]], p: Vec3, d: Vec3, skip: Option<Vec3>) -> Option<f32> {
        let mut best: Option<(f32, f32)> = None;
        for t in tris {
            if skip.is_some_and(|q| t.contains(&q)) {
                continue;
            }
            if let Some(h) = ray_tri(p, d, t[0], t[1], t[2]) {
                if best.is_none_or(|b| h < b.0) {
                    best = Some((h, (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero().dot(d).abs()));
                }
            }
        }
        best.filter(|b| b.1 > 0.5).map(|b| b.0 * b.1)
    }

    #[test]
    fn nothing_bolted_onto_the_bus_floats_off_its_body() {
        // every face of a part on the bus that looks at the body from close by must touch it, go into it or rest on another
        // part: a ray out of the face meets something at once, or the body not at all within a hand's breadth. Decals (the
        // wheel wells) must lie on the body.
        let solids = bus_shell_solids();
        let shell: Vec<[Vec3; 3]> = solids.concat();
        let mut b = MeshBuilder::new();
        bus_shell(&mut b, &|_, _| {});
        let first = b.finish().verts.len();
        let m = bus();
        let all: Vec<[Vec3; 3]> = m.idx.chunks(3).map(|t| [0, 1, 2].map(|k| Vec3::from(m.verts[t[k] as usize].pos))).collect();
        let col = |rgb: u32| {
            let c = to_srgb8(hex(rgb));
            [c[0], c[1], c[2]]
        };
        // the tyres, the mirrors on their arms and the cables stand off the body on purpose
        let free = [col(0x1c1d21), col(0x30353d), col(0x3b3530), col(0x3b414a)];
        let well = col(0x16191f);
        let mut floating = vec![];
        for v in &m.verts[first..] {
            let (p, n, c) = (Vec3::from(v.pos), Vec3::from(v.nrm), [v.col[0], v.col[1], v.col[2]]);
            if p.y > 2.0 || free.contains(&c) || inside(&solids, p) {
                continue;
            }
            // a face that looks at the body from close by must touch it, or rest on another part that does
            if let Some(g) = gap_to(&shell, p, n, None).filter(|g| (0.012..0.2).contains(g)) {
                if !gap_to(&all, p - n * 0.002, n, Some(p)).is_some_and(|g| g < 0.014) {
                    floating.push((g, p, c));
                }
            }
            if c == well && p.x.abs() > 1.25 && (-1.09..-0.3).contains(&p.y) {
                let g = gap_to(&shell, p, -n, None);
                assert!(g.is_some_and(|g| g < 0.012), "a wheel well stands off the side at {p:?}: {g:?}");
            }
        }
        floating.sort_by(|a, b| b.0.total_cmp(&a.0));
        assert!(floating.is_empty(), "{} vertices float off the body, the worst: {:?}", floating.len(), &floating[..floating.len().min(12)]);
    }

    #[test]
    fn glider_canopy_floats_above_the_hand_bar() {
        let g = glider().bounds();
        assert!(g.min.y > -0.35 && g.max.y > 1.8 && g.max.y < 2.4, "glider y {:?}", (g.min.y, g.max.y));
        assert!(g.max.x > 1.8 && g.max.x < 2.3, "canopy radius {}", g.max.x);
    }

    #[test]
    fn weapons_point_down_negative_z_and_sit_around_the_grip() {
        for (name, m) in [("pistol", weapon_pistol()), ("smg", weapon_smg()), ("ar", weapon_ar()), ("shotgun", weapon_shotgun()), ("sniper", weapon_sniper()), ("rocket", weapon_rocket())] {
            let bb = m.bounds();
            assert!(bb.min.z < -0.2, "{name} muzzle z {}", bb.min.z);
            assert!(bb.max.z < 0.7 && bb.max.z > 0.02, "{name} butt z {}", bb.max.z);
            assert!(bb.max.y < 0.35 && bb.min.y > -0.3, "{name} height {:?}", (bb.min.y, bb.max.y));
            assert!(bb.max.x < 0.2 && bb.min.x > -0.2, "{name} width");
        }
    }

    #[test]
    fn build_pieces_fit_their_grid_cell() {
        let w = piece_wall().bounds();
        assert!(w.min.x >= -0.01 && w.max.x <= TILE + 0.01 && (w.max.y - LEVEL_H).abs() < 0.02 && w.min.y >= -0.01);
        assert!(w.max.z <= THICK * 0.51 && w.min.z >= -THICK * 0.51);
        let f = piece_floor().bounds();
        assert!(f.max.y <= 0.05 && (f.min.y + THICK).abs() < 0.01 && f.max.x <= TILE + 0.01 && f.max.z <= TILE + 0.01);
        let r = piece_ramp().bounds();
        assert!((r.max.y - LEVEL_H).abs() < 0.2 && r.max.x <= TILE + 0.01 && r.max.z <= TILE + 0.05, "ramp bounds {r:?}");
        let rf = piece_roof().bounds();
        assert!(rf.min.y >= LEVEL_H - THICK - 0.15 && rf.max.y <= LEVEL_H + 0.1);
    }

    #[test]
    fn untinted_details_keep_their_own_colour() {
        let head = char_head();
        let untinted = head.verts.iter().filter(|v| v.col[3] == 0).count();
        let tinted = head.verts.iter().filter(|v| v.col[3] == 255).count();
        assert!(untinted >= 16 && tinted > 100, "eyes untinted {untinted}, skin tinted {tinted}");
    }

    #[test]
    fn weapon_muzzles_match_what_the_rig_expects() {
        use crate::game::items::WeaponKind;
        use crate::game::rig::weapon_model;
        for kind in WeaponKind::ALL {
            let (id, muzzle) = weapon_model(kind);
            let bb = crate::meshlib::build_mesh(id).bounds();
            assert!((bb.min.z - muzzle.z).abs() < 0.05, "{kind:?}: model tip at z {} but the muzzle is at {}", bb.min.z, muzzle.z);
            assert!(muzzle.y > bb.min.y && muzzle.y < bb.max.y, "{kind:?}: muzzle height {} outside the model", muzzle.y);
        }
    }

    #[test]
    fn weapons_take_the_rarity_tint_on_their_furniture_only() {
        for (name, m) in [("pistol", weapon_pistol()), ("smg", weapon_smg()), ("ar", weapon_ar()), ("shotgun", weapon_shotgun()), ("sniper", weapon_sniper()), ("rocket", weapon_rocket())] {
            let tinted = m.verts.iter().filter(|v| v.col[3] == 255).count() as f32 / m.verts.len() as f32;
            assert!((0.04..0.7).contains(&tinted), "{name}: {:.0}% of the vertices follow the rarity tint", tinted * 100.0);
        }
    }

    #[test]
    fn backpack_sits_behind_the_torso_and_the_whole_character_stays_light() {
        let bp = char_backpack().bounds();
        assert!(bp.max.z > 0.2 && bp.max.z < 0.4 && bp.max.x < 0.25 && bp.min.y > 0.0 && bp.max.y < 0.6, "backpack {bp:?}");
        // a character is drawn as ~17 rigid parts: keep the total small enough for dozens of them on screen
        let parts = [char_torso(), char_trim(), char_pelvis(), char_head(), hair(2), headgear(3), char_backpack(), char_boot(), char_boot()];
        let one_each: usize = parts.iter().map(|m| m.tri_count()).sum();
        let limbs: usize = [char_arm_up(), char_arm_low(), char_hand(), char_leg_up(), char_leg_low()].iter().map(|m| m.tri_count()).sum::<usize>() * 2;
        assert!(one_each + limbs < 12_000, "{} triangles per character", one_each + limbs);
    }

    #[test]
    fn hair_and_hats_wrap_the_head() {
        let head = char_head().bounds();
        for (name, m) in [("hair1", hair(1)), ("hair2", hair(2)), ("hair3", hair(3)), ("cap", headgear(1)), ("beanie", headgear(2)), ("helmet", headgear(3)), ("hat", headgear(4))] {
            let bb = m.bounds();
            // reaches over the crown but stays on the head: nothing floats above or far from it
            assert!(bb.max.y > head.max.y - 0.04 && bb.max.y < head.max.y + 0.2, "{name}: top {} vs head top {}", bb.max.y, head.max.y);
            assert!(bb.min.z > -0.31 && bb.max.z < 0.45 && bb.max.x < 0.3 && bb.min.x > -0.3, "{name}: {bb:?}");
        }
    }
}
