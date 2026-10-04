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

pub fn char_torso() -> MeshData {
    use dims::*;
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH);
    // waist and belly
    b.color(grey(0.94)).ao(0.7, 1.0);
    b.blob(Vec3::new(0.0, 0.11, 0.0), Vec3::new(0.175, 0.14, 0.125), 2, 0.0, 0, true);
    // chest and shoulders: one broad ellipsoid
    b.color(grey(1.0)).ao(0.8, 1.0);
    b.blob(Vec3::new(0.0, 0.345, 0.0), Vec3::new(0.235, 0.21, 0.145), 2, 0.0, 0, true);
    // shoulder caps
    b.color(grey(0.97));
    for s in [-1.0f32, 1.0] {
        b.sphere(Vec3::new(s * SHOULDER_X * 0.97, SHOULDER_Y, 0.0), 0.082, 2);
    }
    // collar band + neck
    b.color(grey(0.72)).ao(0.8, 1.0);
    b.cylinder(Vec3::new(0.0, SHOULDER_Y + 0.02, 0.0), 0.092, 0.075, 0.05, 12, false, true);
    b.mat(mat::SKIN).color(grey(1.0));
    b.cylinder(Vec3::new(0.0, SHOULDER_Y + 0.05, 0.0), 0.06, 0.055, NECK_Y - SHOULDER_Y - 0.03, 10, false, false);
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
    b.sphere(c + Vec3::new(0.0, -0.012, -0.152), 0.03, 2);
    for s in [-1.0f32, 1.0] {
        b.sphere(c + Vec3::new(s * 0.152, -0.005, 0.01), 0.036, 1);
    }
    // eyes (kept untinted so they stay white and dark whatever the skin tone)
    b.tinted(false).mat(mat::FLAT);
    for s in [-1.0f32, 1.0] {
        b.color(grey(0.98));
        b.box_center(c + Vec3::new(s * 0.062, 0.025, -0.1435), Vec3::new(0.034, 0.027, 0.012));
        b.color(Vec3::new(0.1, 0.16, 0.3));
        b.box_center(c + Vec3::new(s * 0.062 - s * 0.004, 0.02, -0.152), Vec3::new(0.019, 0.022, 0.007));
        b.color(Vec3::new(0.05, 0.05, 0.08));
        b.box_center(c + Vec3::new(s * 0.062 - s * 0.004, 0.02, -0.1575), Vec3::new(0.0095, 0.012, 0.004));
        b.color(Vec3::new(0.22, 0.15, 0.1));
        b.box_center(c + Vec3::new(s * 0.062, 0.068, -0.145), Vec3::new(0.038, 0.008, 0.008));
    }
    // smile
    b.color(Vec3::new(0.6, 0.22, 0.22));
    b.box_center(c + Vec3::new(0.0, -0.082, -0.14), Vec3::new(0.034, 0.007, 0.008));
    b.box_center(c + Vec3::new(-0.04, -0.074, -0.138), Vec3::new(0.008, 0.007, 0.008));
    b.box_center(c + Vec3::new(0.04, -0.074, -0.138), Vec3::new(0.008, 0.007, 0.008));
    b.finish()
}

pub fn hair(style: u8) -> MeshData {
    use dims::*;
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.7, 1.0);
    let c = Vec3::new(0.0, HEAD_C, 0.0);
    match style {
        1 => {
            // short crop: covers the crown and the back of the head
            b.blob(c + Vec3::new(0.0, 0.045, 0.012), Vec3::new(0.146, 0.125, 0.152), 2, 0.05, 3, true);
            b.blob(c + Vec3::new(0.0, 0.1, -0.06), Vec3::new(0.12, 0.05, 0.07), 1, 0.05, 5, true);
        }
        2 => {
            // long hair falling over the shoulders
            b.blob(c + Vec3::new(0.0, 0.045, 0.012), Vec3::new(0.148, 0.128, 0.155), 2, 0.05, 3, true);
            b.blob(c + Vec3::new(0.0, -0.1, 0.07), Vec3::new(0.135, 0.2, 0.085), 2, 0.04, 9, true);
            b.blob(c + Vec3::new(0.0, 0.1, -0.06), Vec3::new(0.12, 0.05, 0.07), 1, 0.05, 5, true);
        }
        3 => {
            // ponytail
            b.blob(c + Vec3::new(0.0, 0.045, 0.012), Vec3::new(0.146, 0.125, 0.152), 2, 0.05, 3, true);
            b.blob(c + Vec3::new(0.0, 0.1, -0.06), Vec3::new(0.12, 0.05, 0.07), 1, 0.05, 5, true);
            b.sphere(c + Vec3::new(0.0, 0.06, 0.16), 0.045, 1);
            b.push_xf(Mat4::from_translation(c + Vec3::new(0.0, 0.05, 0.17)) * rot_x(PI * 0.62));
            b.cylinder(Vec3::ZERO, 0.04, 0.012, 0.3, 8, false, false);
            b.pop_xf();
        }
        _ => {}
    }
    b.finish()
}

pub fn headgear(kind: u8) -> MeshData {
    use dims::*;
    let mut b = MeshBuilder::new();
    let c = Vec3::new(0.0, HEAD_C, 0.0);
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.7, 1.0);
    match kind {
        1 => {
            // baseball cap
            b.blob(c + Vec3::new(0.0, 0.055, 0.0), Vec3::new(0.152, 0.115, 0.158), 2, 0.0, 0, true);
            b.color(grey(0.8));
            b.push_xf(Mat4::from_translation(c + Vec3::new(0.0, 0.065, -0.17)) * rot_x(0.12));
            b.box_center(Vec3::ZERO, Vec3::new(0.1, 0.008, 0.075));
            b.pop_xf();
            b.color(grey(0.6)).spec(0.1);
            b.sphere(c + Vec3::new(0.0, 0.17, 0.0), 0.02, 1);
        }
        2 => {
            // beanie with a bobble
            b.blob(c + Vec3::new(0.0, 0.06, 0.0), Vec3::new(0.152, 0.13, 0.156), 2, 0.0, 0, true);
            b.color(grey(0.72));
            b.cylinder(c + Vec3::new(0.0, 0.0, 0.0), 0.156, 0.154, 0.06, 12, false, false);
            b.color(grey(1.0));
            b.sphere(c + Vec3::new(0.0, 0.2, 0.0), 0.042, 1);
        }
        3 => {
            // helmet
            b.mat(mat::METAL).spec(0.7);
            b.blob(c + Vec3::new(0.0, 0.035, 0.005), Vec3::new(0.168, 0.15, 0.17), 2, 0.0, 0, true);
            b.color(grey(0.75));
            b.box_center(c + Vec3::new(0.0, 0.14, 0.0), Vec3::new(0.02, 0.03, 0.17));
            b.color(grey(0.6));
            b.push_xf(Mat4::from_translation(c + Vec3::new(0.0, 0.1, -0.17)) * rot_x(0.2));
            b.box_center(Vec3::ZERO, Vec3::new(0.11, 0.008, 0.05));
            b.pop_xf();
        }
        4 => {
            // wide hat
            b.cylinder(c + Vec3::new(0.0, 0.075, 0.0), 0.27, 0.27, 0.014, 18, true, true);
            b.cylinder(c + Vec3::new(0.0, 0.075, 0.0), 0.15, 0.125, 0.14, 14, false, true);
            b.color(grey(0.6));
            b.cylinder(c + Vec3::new(0.0, 0.082, 0.0), 0.153, 0.151, 0.03, 14, false, false);
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
    b.sphere(Vec3::new(0.0, 0.0, 0.0), 0.07, 2);
    b.sphere(Vec3::new(0.0, -UPPER_ARM, 0.0), 0.058, 2);
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
    b.finish()
}

pub fn char_hand() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::SKIN).color(grey(1.0)).ao(0.8, 1.0);
    b.blob(Vec3::new(0.0, -0.05, 0.0), Vec3::new(0.052, 0.062, 0.045), 2, 0.0, 0, true);
    b.sphere(Vec3::new(0.0, -0.05, -0.048), 0.027, 1);
    b.finish()
}

pub fn char_leg_up() -> MeshData {
    use dims::*;
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.7, 1.0);
    b.cylinder(Vec3::new(0.0, -THIGH, 0.0), 0.078, 0.098, THIGH, 12, false, false);
    b.sphere(Vec3::new(0.0, 0.0, 0.0), 0.098, 2);
    b.sphere(Vec3::new(0.0, -THIGH, 0.0), 0.08, 2);
    b.finish()
}

pub fn char_leg_low() -> MeshData {
    use dims::*;
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.7, 1.0);
    b.cylinder(Vec3::new(0.0, -SHIN, 0.0), 0.062, 0.078, SHIN, 12, false, false);
    b.finish()
}

pub fn char_boot() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.6, 1.0);
    // shaft
    b.cylinder(Vec3::new(0.0, -0.065, 0.0), 0.083, 0.078, 0.145, 12, false, true);
    // foot
    b.blob(Vec3::new(0.0, -0.045, -0.05), Vec3::new(0.075, 0.055, 0.15), 2, 0.0, 0, true);
    // sole
    b.color(grey(0.3)).ao(1.0, 1.0);
    b.box_min_max(Vec3::new(-0.074, -0.098, -0.2), Vec3::new(0.074, -0.068, 0.09));
    b.finish()
}

pub fn char_backpack() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::CLOTH).color(grey(1.0)).ao(0.6, 1.0);
    b.box_center(Vec3::new(0.0, 0.3, 0.165), Vec3::new(0.145, 0.19, 0.075));
    b.color(grey(0.85));
    b.box_center(Vec3::new(0.0, 0.5, 0.17), Vec3::new(0.15, 0.035, 0.08));
    for s in [-1.0f32, 1.0] {
        b.color(grey(0.9));
        b.box_center(Vec3::new(s * 0.1, 0.14, 0.165), Vec3::new(0.05, 0.07, 0.08));
    }
    // rolled mat
    b.color(grey(0.5)).tinted(false).color(Vec3::new(0.35, 0.37, 0.4));
    b.push_xf(Mat4::from_translation(Vec3::new(0.0, 0.07, 0.17)) * Mat4::from_rotation_z(FRAC_PI_2));
    b.cylinder(Vec3::new(0.0, -0.13, 0.0), 0.045, 0.045, 0.26, 8, true, true);
    b.pop_xf();
    // straps
    b.tinted(true).color(grey(0.35));
    for s in [-1.0f32, 1.0] {
        b.box_center(Vec3::new(s * 0.1, 0.38, 0.0), Vec3::new(0.018, 0.17, 0.1));
    }
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
    // rim ring
    b.color(grey(0.5));
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
// =======================================================================================

const GUNMETAL: u32 = 0x4a4f58;
const DARK: u32 = 0x2b2e34;
const LIGHT_METAL: u32 = 0x9aa2ae;
const WOOD: u32 = 0x8a5630;
const POLYMER: u32 = 0x5c616b;

fn grip_box(b: &mut MeshBuilder, c: Vec3, half: Vec3, tilt: f32) {
    b.push_xf(Mat4::from_translation(c) * rot_x(tilt));
    b.box_center(Vec3::ZERO, half);
    b.pop_xf();
}

pub fn weapon_pistol() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(GUNMETAL).ao(0.7, 1.0).spec(0.7);
    // slide and barrel
    b.box_center(Vec3::new(0.0, 0.045, -0.085), Vec3::new(0.017, 0.022, 0.125));
    b.hex(LIGHT_METAL);
    b.box_center(Vec3::new(0.0, 0.069, -0.08), Vec3::new(0.012, 0.004, 0.11));
    b.hex(DARK);
    barrel(&mut b, 0.0, 0.043, -0.2, -0.235, 0.009, 0.009, 8);
    // frame and grip
    b.mat(mat::FLAT).hex(POLYMER).spec(0.1);
    b.box_center(Vec3::new(0.0, 0.01, -0.07), Vec3::new(0.016, 0.014, 0.1));
    grip_box(&mut b, Vec3::new(0.0, -0.045, 0.012), Vec3::new(0.018, 0.058, 0.026), 0.2);
    // trigger guard
    b.hex(DARK);
    b.box_center(Vec3::new(0.0, -0.012, -0.045), Vec3::new(0.006, 0.012, 0.022));
    // front and rear sights
    b.box_center(Vec3::new(0.0, 0.08, -0.19), Vec3::new(0.004, 0.007, 0.004));
    b.box_center(Vec3::new(0.0, 0.078, 0.03), Vec3::new(0.01, 0.006, 0.006));
    chunky(b.finish(), 1.4)
}

pub fn weapon_smg() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(GUNMETAL).ao(0.7, 1.0).spec(0.6);
    b.box_center(Vec3::new(0.0, 0.025, -0.14), Vec3::new(0.025, 0.04, 0.2));
    b.hex(LIGHT_METAL);
    b.box_center(Vec3::new(0.0, 0.069, -0.13), Vec3::new(0.014, 0.006, 0.18));
    b.hex(DARK);
    barrel(&mut b, 0.0, 0.03, -0.32, -0.46, 0.011, 0.011, 8);
    barrel(&mut b, 0.0, 0.03, -0.46, -0.5, 0.017, 0.017, 8);
    // magazine
    b.mat(mat::FLAT).hex(POLYMER).spec(0.05);
    grip_box(&mut b, Vec3::new(0.0, -0.09, -0.1), Vec3::new(0.017, 0.085, 0.024), 0.06);
    // grip
    grip_box(&mut b, Vec3::new(0.0, -0.06, 0.04), Vec3::new(0.02, 0.06, 0.025), 0.25);
    // stub stock
    b.hex(POLYMER);
    b.box_center(Vec3::new(0.0, 0.02, 0.17), Vec3::new(0.017, 0.03, 0.07));
    // red dot sight
    b.hex(DARK);
    b.box_center(Vec3::new(0.0, 0.092, -0.1), Vec3::new(0.016, 0.018, 0.03));
    b.mat(mat::EMISSIVE).tinted(false).color(Vec3::new(1.0, 0.15, 0.1));
    b.box_center(Vec3::new(0.0, 0.098, -0.13), Vec3::new(0.003, 0.003, 0.003));
    chunky(b.finish(), 1.4)
}

pub fn weapon_ar() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(GUNMETAL).ao(0.7, 1.0).spec(0.6);
    // receiver
    b.box_center(Vec3::new(0.0, 0.03, -0.1), Vec3::new(0.027, 0.045, 0.2));
    // upper rail
    b.hex(LIGHT_METAL);
    b.box_center(Vec3::new(0.0, 0.082, -0.18), Vec3::new(0.015, 0.007, 0.27));
    // handguard + barrel + muzzle brake
    b.mat(mat::FLAT).hex(POLYMER).spec(0.1);
    b.box_center(Vec3::new(0.0, 0.025, -0.45), Vec3::new(0.028, 0.034, 0.13));
    b.mat(mat::METAL).hex(DARK).spec(0.8);
    barrel(&mut b, 0.0, 0.028, -0.55, -0.72, 0.012, 0.012, 8);
    barrel(&mut b, 0.0, 0.028, -0.72, -0.77, 0.019, 0.017, 8);
    // stock
    b.mat(mat::FLAT).hex(POLYMER).spec(0.05);
    b.box_center(Vec3::new(0.0, 0.025, 0.27), Vec3::new(0.022, 0.05, 0.09));
    b.hex(DARK);
    b.box_center(Vec3::new(0.0, 0.025, 0.365), Vec3::new(0.024, 0.056, 0.012));
    // pistol grip + magazine
    grip_box(&mut b, Vec3::new(0.0, -0.05, 0.02), Vec3::new(0.02, 0.06, 0.025), 0.28);
    b.hex(GUNMETAL);
    grip_box(&mut b, Vec3::new(0.0, -0.1, -0.14), Vec3::new(0.02, 0.085, 0.032), -0.14);
    // sight: carry handle with a front post
    b.mat(mat::METAL).hex(DARK).spec(0.7);
    b.box_center(Vec3::new(0.0, 0.11, -0.04), Vec3::new(0.012, 0.022, 0.04));
    b.box_center(Vec3::new(0.0, 0.1, -0.6), Vec3::new(0.005, 0.02, 0.006));
    // rarity stripe along the receiver (accent colour from the instance tint)
    b.mat(mat::EMISSIVE).tinted(true).color(Vec3::new(0.9, 0.9, 0.9));
    b.box_center(Vec3::new(0.0, 0.0, -0.1), Vec3::new(0.0285, 0.007, 0.15));
    chunky(b.finish(), 1.4)
}

pub fn weapon_shotgun() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(GUNMETAL).ao(0.7, 1.0).spec(0.7);
    // receiver
    b.box_center(Vec3::new(0.0, 0.025, -0.06), Vec3::new(0.026, 0.04, 0.12));
    // barrel and magazine tube
    b.hex(DARK);
    barrel(&mut b, 0.0, 0.04, -0.15, -0.78, 0.016, 0.016, 8);
    barrel(&mut b, 0.0, 0.0, -0.17, -0.7, 0.014, 0.014, 8);
    b.hex(LIGHT_METAL);
    barrel(&mut b, 0.0, 0.04, -0.78, -0.8, 0.02, 0.02, 8);
    // wooden pump
    b.mat(mat::WOOD).hex(WOOD).spec(0.1);
    b.box_center(Vec3::new(0.0, 0.0, -0.33), Vec3::new(0.026, 0.026, 0.09));
    // wooden stock
    b.box_center(Vec3::new(0.0, 0.015, 0.24), Vec3::new(0.022, 0.05, 0.13));
    b.mat(mat::FLAT).hex(DARK);
    b.box_center(Vec3::new(0.0, 0.015, 0.375), Vec3::new(0.024, 0.056, 0.01));
    // grip
    b.mat(mat::WOOD).hex(WOOD);
    grip_box(&mut b, Vec3::new(0.0, -0.05, 0.04), Vec3::new(0.019, 0.055, 0.024), 0.3);
    // shell holder on the side
    b.mat(mat::FLAT).hex(0xd33a2a);
    for k in 0..3 {
        b.box_center(Vec3::new(0.0285, 0.035, -0.02 - k as f32 * 0.03), Vec3::new(0.006, 0.014, 0.011));
    }
    chunky(b.finish(), 1.4)
}

pub fn weapon_sniper() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(GUNMETAL).ao(0.7, 1.0).spec(0.7);
    // receiver
    b.box_center(Vec3::new(0.0, 0.03, -0.1), Vec3::new(0.024, 0.04, 0.17));
    // long barrel + muzzle brake
    b.hex(DARK);
    barrel(&mut b, 0.0, 0.035, -0.26, -1.0, 0.012, 0.01, 8);
    b.hex(LIGHT_METAL);
    barrel(&mut b, 0.0, 0.035, -1.0, -1.06, 0.018, 0.018, 8);
    // stock (green-ish polymer)
    b.mat(mat::FLAT).hex(0x4d5a45).spec(0.05);
    b.box_center(Vec3::new(0.0, 0.0, 0.2), Vec3::new(0.022, 0.05, 0.14));
    b.box_center(Vec3::new(0.0, 0.03, -0.2), Vec3::new(0.02, 0.025, 0.12));
    grip_box(&mut b, Vec3::new(0.0, -0.05, 0.035), Vec3::new(0.019, 0.055, 0.024), 0.3);
    // scope
    b.mat(mat::METAL).hex(DARK).spec(0.8);
    barrel(&mut b, 0.0, 0.1, 0.0, -0.36, 0.03, 0.03, 12);
    barrel(&mut b, 0.0, 0.1, 0.03, 0.0, 0.036, 0.03, 12);
    barrel(&mut b, 0.0, 0.1, -0.36, -0.4, 0.03, 0.04, 12);
    b.box_center(Vec3::new(0.0, 0.068, -0.05), Vec3::new(0.008, 0.016, 0.01));
    b.box_center(Vec3::new(0.0, 0.068, -0.27), Vec3::new(0.008, 0.016, 0.01));
    // lenses
    b.mat(mat::GLASS).tinted(false).color(Vec3::new(0.25, 0.55, 0.85));
    barrel(&mut b, 0.0, 0.1, 0.034, 0.03, 0.026, 0.026, 12);
    barrel(&mut b, 0.0, 0.1, -0.402, -0.406, 0.034, 0.034, 12);
    // bolt handle
    b.mat(mat::METAL).tinted(true).hex(LIGHT_METAL);
    b.box_center(Vec3::new(0.04, 0.035, -0.02), Vec3::new(0.02, 0.006, 0.006));
    b.sphere(Vec3::new(0.065, 0.035, -0.02), 0.011, 1);
    // magazine
    b.mat(mat::FLAT).hex(POLYMER);
    b.box_center(Vec3::new(0.0, -0.025, -0.09), Vec3::new(0.014, 0.025, 0.03));
    chunky(b.finish(), 1.4)
}

pub fn weapon_rocket() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(0x586b4d).ao(0.7, 1.0).spec(0.4);
    // main tube resting over the shoulder; the grip hangs below the middle
    barrel(&mut b, 0.0, 0.09, 0.45, -0.55, 0.085, 0.085, 14);
    // flared rear and front rings
    b.hex(DARK);
    barrel(&mut b, 0.0, 0.09, 0.62, 0.45, 0.115, 0.085, 14);
    barrel(&mut b, 0.0, 0.09, -0.55, -0.62, 0.095, 0.095, 14);
    // warhead
    b.mat(mat::FLAT).hex(0xd6492f).spec(0.1);
    barrel(&mut b, 0.0, 0.09, -0.62, -0.7, 0.07, 0.055, 12);
    b.hex(0xe9e4d8);
    barrel(&mut b, 0.0, 0.09, -0.7, -0.78, 0.055, 0.0, 12);
    // grips and sight
    b.mat(mat::FLAT).hex(POLYMER);
    grip_box(&mut b, Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.02, 0.06, 0.025), 0.25);
    grip_box(&mut b, Vec3::new(0.0, 0.0, -0.3), Vec3::new(0.018, 0.045, 0.022), 0.0);
    b.hex(DARK);
    b.box_center(Vec3::new(0.0, 0.2, -0.1), Vec3::new(0.012, 0.03, 0.04));
    // accent band
    b.mat(mat::EMISSIVE).tinted(true).color(Vec3::new(0.9, 0.9, 0.9));
    barrel(&mut b, 0.0, 0.09, 0.2, 0.15, 0.0865, 0.0865, 14);
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
    // red cross on both faces
    b.mat(mat::EMISSIVE).color(Vec3::new(0.95, 0.12, 0.12));
    for z in [-0.0565f32, 0.0565] {
        b.box_center(Vec3::new(0.0, 0.09, z), Vec3::new(0.065, 0.017, 0.002));
        b.box_center(Vec3::new(0.0, 0.09, z), Vec3::new(0.017, 0.065, 0.002));
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
    b.mat(mat::FLAT).tinted(false).color(Vec3::new(0.2, 0.15, 0.1));
    b.box_center(Vec3::new(0.0, 0.01, -0.322), Vec3::new(0.012, 0.022, 0.004));
    b.finish()
}

/// The battle bus hanging under its balloon. Origin at the centre of the bus body; the front
/// faces -Z.
pub fn bus() -> MeshData {
    let mut b = MeshBuilder::new();
    let (w, l) = (3.0f32, 8.6f32);
    // body
    b.mat(mat::PLASTER).hex(0x2f78e6).ao(0.7, 1.0).spec(0.35);
    b.box_min_max(Vec3::new(-w / 2.0, -1.1, -l / 2.0), Vec3::new(w / 2.0, 0.6, l / 2.0));
    // hood
    b.box_min_max(Vec3::new(-w / 2.0 + 0.1, -1.1, -l / 2.0 - 1.4), Vec3::new(w / 2.0 - 0.1, -0.1, -l / 2.0 + 0.05));
    // upper white band + roof
    b.hex(0xf4f7fb);
    b.box_min_max(Vec3::new(-w / 2.0 - 0.03, 0.3, -l / 2.0), Vec3::new(w / 2.0 + 0.03, 0.6, l / 2.0));
    b.box_min_max(Vec3::new(-w / 2.0 + 0.05, 0.6, -l / 2.0 + 0.1), Vec3::new(w / 2.0 - 0.05, 1.15, l / 2.0 - 0.1));
    b.hex(0xe9a321);
    b.box_min_max(Vec3::new(-w / 2.0 - 0.035, -0.55, -l / 2.0 + 0.05), Vec3::new(w / 2.0 + 0.035, -0.4, l / 2.0 - 0.05));
    // windows
    b.mat(mat::GLASS).tinted(false).color(Vec3::new(0.12, 0.2, 0.3));
    for k in 0..6 {
        let z = -l / 2.0 + 0.9 + k as f32 * 1.3;
        for s in [-1.0f32, 1.0] {
            b.box_center(Vec3::new(s * (w / 2.0 + 0.012), 0.0, z), Vec3::new(0.01, 0.28, 0.5));
        }
    }
    b.box_center(Vec3::new(0.0, 0.05, -l / 2.0 - 0.005), Vec3::new(1.2, 0.35, 0.02));
    // wheels
    b.mat(mat::FLAT).tinted(false).color(Vec3::new(0.08, 0.08, 0.1));
    for s in [-1.0f32, 1.0] {
        for z in [-2.9f32, 2.7] {
            b.push_xf(Mat4::from_translation(Vec3::new(s * (w / 2.0 - 0.05), -1.1, z)) * Mat4::from_rotation_z(FRAC_PI_2));
            b.cylinder(Vec3::new(0.0, -0.2, 0.0), 0.62, 0.62, 0.4, 14, true, true);
            b.pop_xf();
        }
    }
    b.color(Vec3::new(0.5, 0.52, 0.56)).mat(mat::METAL);
    for s in [-1.0f32, 1.0] {
        for z in [-2.9f32, 2.7] {
            b.push_xf(Mat4::from_translation(Vec3::new(s * (w / 2.0 + 0.16), -1.1, z)) * Mat4::from_rotation_z(FRAC_PI_2));
            b.cylinder(Vec3::ZERO, 0.3, 0.3, 0.02, 10, true, true);
            b.pop_xf();
        }
    }
    // headlights
    b.mat(mat::EMISSIVE).color(Vec3::new(1.0, 0.95, 0.7));
    for s in [-1.0f32, 1.0] {
        b.box_center(Vec3::new(s * 1.0, -0.6, -l / 2.0 - 1.41), Vec3::new(0.22, 0.14, 0.02));
    }
    // bumper
    b.mat(mat::METAL).color(Vec3::new(0.8, 0.82, 0.86));
    b.box_center(Vec3::new(0.0, -0.95, -l / 2.0 - 1.45), Vec3::new(1.4, 0.12, 0.08));
    // balloon: ring of gores
    let bc = Vec3::new(0.0, 12.0, 0.0);
    let rad = 6.2f32;
    let segs = 14usize;
    let rows = 8usize;
    b.mat(mat::CLOTH).tinted(false).ao(0.75, 1.0);
    for g in 0..segs {
        let a0 = g as f32 / segs as f32 * TAU_F;
        let a1 = (g + 1) as f32 / segs as f32 * TAU_F;
        let col = if g % 2 == 0 { Vec3::new(0.95, 0.3, 0.25) } else { Vec3::new(0.98, 0.85, 0.3) };
        b.color(col);
        for r in 0..rows {
            let t0 = r as f32 / rows as f32 * PI * 0.88 + 0.0;
            let t1 = (r + 1) as f32 / rows as f32 * PI * 0.88;
            let pt = |a: f32, t: f32| {
                let s = t.sin();
                // slightly pear shaped: wider on top
                let rr = rad * s * (1.0 + 0.05 * t.cos());
                bc + Vec3::new(a.cos() * rr, -t.cos() * rad * 1.15 + 0.0, a.sin() * rr)
            };
            let p = [pt(a0, t0), pt(a1, t0), pt(a1, t1), pt(a0, t1)];
            let mid = (p[0] + p[1] + p[2] + p[3]) * 0.25;
            b.quad_out(p, mid - bc);
        }
    }
    // ropes to the roof
    b.mat(mat::FLAT).color(Vec3::new(0.35, 0.28, 0.2));
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let top = bc + Vec3::new(sx * 3.2, -rad * 1.15 * 0.55, sz * 3.2);
        let bot = Vec3::new(sx * 1.3, 1.15, sz * 3.4);
        let d = top - bot;
        let len = d.length();
        b.push_xf(Mat4::from_translation(bot) * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, d / len)));
        b.cylinder(Vec3::ZERO, 0.05, 0.05, len, 5, false, false);
        b.pop_xf();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<(&'static str, MeshData)> {
        vec![
            ("torso", char_torso()),
            ("pelvis", char_pelvis()),
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
}
