//! The library of shared meshes (vegetation, rocks, props, units for characters,
//! weapons, items, build pieces). Every instanced draw in the game references one
//! of these by `MeshId`; the renderer uploads them all once.

use crate::math::*;
use crate::mesh::*;
use crate::models;
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum MeshId {
    // --- primitives --------------------------------------------------------
    UnitBox,
    UnitSphere1,
    UnitSphere2,
    UnitCylinder,
    UnitCone,
    // --- vegetation (lod0 / lod1) ---------------------------------------------
    Pine0,
    Pine1,
    Oak0,
    Oak1,
    Birch0,
    Birch1,
    Palm0,
    Palm1,
    Bush0,
    Bush1,
    Rock0,
    Rock1,
    Rock2,
    Boulder,
    FlowerClump,
    GrassTuft,
    Stump,
    // --- small props -------------------------------------------------------------
    HayBale,
    Barrel,
    Crate,
    WindmillBlades,
    // --- characters (rigid parts posed by `game::rig`) ------------------------------
    CharTorso,
    CharTrim,
    CharPelvis,
    CharHead,
    Hair1,
    Hair2,
    Hair3,
    Cap,
    Beanie,
    Helmet,
    Hat,
    CharArmUp,
    CharArmLow,
    CharHand,
    CharLegUp,
    CharLegLow,
    CharBoot,
    CharBackpack,
    Glider,
    // --- weapons, items ---------------------------------------------------------------
    WpnPistol,
    WpnSmg,
    WpnAr,
    WpnShotgun,
    WpnSniper,
    WpnRocket,
    Pickaxe,
    AmmoBox,
    ItemBandage,
    ItemMedkit,
    ItemShieldMini,
    ItemShieldBig,
    ItemChug,
    // --- world objects ---------------------------------------------------------------------
    ChestBase,
    ChestLid,
    Bus,
    Missile,
    PieceWall,
    PieceFloor,
    PieceRoof,
    PieceRamp,
    // --- fillers for future content are appended below by other modules -----------
    Count,
}

impl MeshId {
    pub const fn idx(self) -> u16 {
        self as u16
    }
}

fn tree_gray(b: f32) -> Vec3 {
    Vec3::splat(b)
}

/// Pine tree. Foliage vertex colours are neutral greys; the instance tint supplies the hue.
fn pine(detail: bool) -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::BARK).hex(0x6b4a2b).ao(0.55, 1.0);
    b.cylinder(Vec3::ZERO, 0.30, 0.18, if detail { 2.6 } else { 3.0 }, if detail { 7 } else { 5 }, false, false);
    b.mat(mat::FOLIAGE).sway(0.0);
    let tiers = if detail { 5 } else { 3 };
    for i in 0..tiers {
        let t = i as f32 / (tiers - 1) as f32;
        let shade = 0.74 + 0.26 * t;
        b.color(tree_gray(shade)).ao(0.55 + 0.2 * t, 1.0);
        let base_y = if detail { 1.5 + i as f32 * 1.5 } else { 1.7 + i as f32 * 2.4 };
        let r = (if detail { 2.3 } else { 2.5 }) * (1.0 - t * 0.62);
        let h = if detail { 2.6 - i as f32 * 0.15 } else { 3.8 - i as f32 * 0.3 };
        let rot = Mat4::from_rotation_y(i as f32 * 0.55);
        b.push_xf(rot);
        // gentle sway increases with height
        b.sway(0.15 + t * 0.5);
        b.cone(Vec3::new(0.0, base_y, 0.0), r, h, if detail { 9 } else { 6 });
        b.pop_xf();
    }
    b.finish()
}

fn oak(detail: bool) -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::BARK).hex(0x6a4a2e).ao(0.55, 1.0);
    b.cylinder(Vec3::ZERO, 0.36, 0.22, 3.2, if detail { 8 } else { 5 }, false, false);
    if detail {
        // two branches
        for (dx, dz) in [(0.9f32, 0.3f32), (-0.8, -0.5)] {
            let dir = Vec3::new(dx, 1.0, dz).normalize();
            let rot = Quat::from_rotation_arc(Vec3::Y, dir);
            b.push_xf(Mat4::from_translation(Vec3::new(0.0, 2.4, 0.0)) * Mat4::from_quat(rot));
            b.cylinder(Vec3::ZERO, 0.16, 0.08, 1.8, 5, false, false);
            b.pop_xf();
        }
    }
    b.mat(mat::FOLIAGE);
    let blobs: &[(Vec3, Vec3)] = &[
        (Vec3::new(0.0, 4.9, 0.0), Vec3::new(2.7, 2.2, 2.7)),
        (Vec3::new(1.9, 4.1, 0.7), Vec3::new(1.9, 1.7, 1.9)),
        (Vec3::new(-1.7, 4.3, -0.8), Vec3::new(2.0, 1.7, 2.0)),
        (Vec3::new(0.3, 6.2, -0.3), Vec3::new(1.8, 1.5, 1.8)),
        (Vec3::new(0.5, 3.9, -1.8), Vec3::new(1.7, 1.4, 1.7)),
        (Vec3::new(-0.6, 4.0, 1.9), Vec3::new(1.6, 1.4, 1.6)),
    ];
    let n = if detail { blobs.len() } else { 3 };
    for (i, (c, r)) in blobs.iter().take(n).enumerate() {
        let tone = 0.80 + 0.2 * (c.y - 3.5) / 3.0;
        b.color(tree_gray(tone.clamp(0.7, 1.0))).ao(0.52, 1.0).sway(0.4 + 0.3 * (c.y / 6.0));
        b.blob(*c, *r, if detail { 1 } else { 0 }, if detail { 0.2 } else { 0.1 }, 7 + i as u32 * 13, true);
    }
    b.finish()
}

fn birch(detail: bool) -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::BARK).hex(0xe7e3d6).ao(0.6, 1.0);
    b.cylinder(Vec3::ZERO, 0.22, 0.12, 5.2, if detail { 7 } else { 5 }, false, false);
    if detail {
        // dark birch marks
        b.hex(0x3b3a36);
        for k in 0..6 {
            let y = 0.6 + k as f32 * 0.75;
            let a = k as f32 * 2.1;
            b.push_xf(Mat4::from_translation(Vec3::new(a.cos() * 0.19, y, a.sin() * 0.19)) * Mat4::from_rotation_y(-a));
            b.box_center(Vec3::ZERO, Vec3::new(0.03, 0.05, 0.1));
            b.pop_xf();
        }
    }
    b.mat(mat::FOLIAGE);
    let blobs: &[(Vec3, Vec3)] = &[
        (Vec3::new(0.0, 5.4, 0.0), Vec3::new(1.5, 1.7, 1.5)),
        (Vec3::new(0.9, 4.5, 0.3), Vec3::new(1.1, 1.2, 1.1)),
        (Vec3::new(-0.8, 4.8, -0.4), Vec3::new(1.2, 1.3, 1.2)),
        (Vec3::new(0.1, 6.5, -0.1), Vec3::new(1.0, 1.1, 1.0)),
    ];
    for (i, (c, r)) in blobs.iter().take(if detail { 4 } else { 2 }).enumerate() {
        b.color(tree_gray(0.86 + 0.14 * (c.y - 4.0) / 3.0)).ao(0.58, 1.0).sway(0.55);
        b.blob(*c, *r, if detail { 1 } else { 0 }, 0.15, 31 + i as u32 * 7, true);
    }
    b.finish()
}

fn palm(detail: bool) -> MeshData {
    let mut b = MeshBuilder::new();
    // curved, ringed trunk made of tilted tapered segments
    let segs = if detail { 7 } else { 4 };
    let seg_h = 7.4 / segs as f32;
    let mut pos = Vec3::ZERO;
    let mut dir = Vec3::Y;
    let bend = Vec3::new(0.16, 0.0, 0.05);
    b.mat(mat::BARK).ao(0.6, 1.0);
    for i in 0..segs {
        let t = i as f32 / segs as f32;
        let r0 = 0.30 - 0.1 * t;
        let r1 = 0.30 - 0.1 * (t + 1.0 / segs as f32);
        if i % 2 == 0 {
            b.hex(0x8a6a44);
        } else {
            b.hex(0x7a5a38);
        }
        let rot = Quat::from_rotation_arc(Vec3::Y, dir);
        b.push_xf(Mat4::from_translation(pos) * Mat4::from_quat(rot));
        b.cylinder(Vec3::ZERO, r0, r1, seg_h * 1.05, if detail { 7 } else { 5 }, false, false);
        b.pop_xf();
        pos += dir * seg_h;
        dir = (dir + bend).normalize();
    }
    // fronds
    b.mat(mat::FOLIAGE);
    let fronds = if detail { 9 } else { 6 };
    for f in 0..fronds {
        let a = f as f32 / fronds as f32 * TAU + 0.3;
        let (sa, ca) = a.sin_cos();
        let len = 3.6 + (f % 3) as f32 * 0.35;
        let steps = if detail { 5 } else { 3 };
        let mut prev_l = pos;
        let mut prev_r = pos;
        for s in 1..=steps {
            let t = s as f32 / steps as f32;
            let out = t * len;
            let droop = -0.55 * t * t * len + 1.1 * t * (1.0 - t) * len * 0.5;
            let w = 0.55 * (1.0 - t * 0.85) + 0.05;
            let centre = pos + Vec3::new(ca * out, droop, sa * out);
            let side = Vec3::new(-sa, 0.0, ca) * w;
            let l = centre - side + Vec3::Y * (0.12 * t);
            let r = centre + side + Vec3::Y * (0.12 * t);
            let shade = 0.8 + 0.2 * t;
            b.color(tree_gray(shade)).sway(0.5 + 0.5 * t).ao(0.8, 1.0);
            // two sided quad
            b.quad_flat([prev_l, prev_r, r, l], [1.0; 4]);
            b.quad_flat([prev_l, l, r, prev_r], [1.0; 4]);
            prev_l = l;
            prev_r = r;
        }
    }
    // coconuts
    if detail {
        b.mat(mat::FLAT).hex(0x5a3d22).sway(0.0).ao(0.7, 1.0);
        for k in 0..3 {
            let a = k as f32 * 2.1;
            b.sphere(pos + Vec3::new(a.cos() * 0.25, -0.3, a.sin() * 0.25), 0.17, 1);
        }
    }
    b.finish()
}

fn bush(detail: bool) -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::FOLIAGE);
    let blobs: &[(Vec3, Vec3)] = &[
        (Vec3::new(0.0, 0.55, 0.0), Vec3::new(0.95, 0.65, 0.95)),
        (Vec3::new(0.7, 0.45, 0.3), Vec3::new(0.65, 0.5, 0.65)),
        (Vec3::new(-0.6, 0.45, -0.3), Vec3::new(0.7, 0.5, 0.7)),
    ];
    for (i, (c, r)) in blobs.iter().take(if detail { 3 } else { 2 }).enumerate() {
        b.color(tree_gray(0.82 + 0.18 * c.y)).ao(0.5, 1.0).sway(0.35);
        b.blob(*c, *r, if detail { 1 } else { 0 }, 0.18, 3 + i as u32 * 11, true);
    }
    b.finish()
}

fn rock(variant: u32, boulder: bool) -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::STONE).hex(0x8d8e94).ao(0.55, 1.0);
    let (radii, amp) = if boulder { (Vec3::new(2.0, 1.5, 1.8), 0.28) } else { (Vec3::new(1.0, 0.7, 0.9), 0.3) };
    b.blob(Vec3::new(0.0, radii.y * 0.5, 0.0), radii, 1, amp, 100 + variant * 17, false);
    if boulder || variant == 2 {
        b.hex(0x7f8087);
        b.blob(Vec3::new(radii.x * 0.7, radii.y * 0.25, radii.z * 0.3), radii * 0.5, 1, amp, 7 + variant, false);
    }
    b.finish()
}

fn flower_clump() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::FOLIAGE).color(Vec3::splat(1.0)).ao(0.7, 1.0).sway(0.6);
    for (x, z, r) in [(0.0, 0.0, 0.17), (0.22, 0.12, 0.13), (-0.18, 0.15, 0.14), (0.05, -0.22, 0.13)] {
        b.sphere(Vec3::new(x, 0.32 + r * 0.3, z), r, 0);
    }
    b.mat(mat::GRASS).color(Vec3::new(0.78, 0.9, 0.7));
    for a in [0.0f32, 2.1, 4.2] {
        let (s, c) = a.sin_cos();
        b.tri_flat(Vec3::new(c * 0.02, 0.0, s * 0.02), Vec3::new(-s * 0.03, 0.0, c * 0.03), Vec3::new(c * 0.25, 0.3, s * 0.25));
    }
    b.finish()
}

/// A tuft of grass blades. Greys; the instance tint gives the green.
fn grass_tuft() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::GRASS);
    let blades = 6;
    for i in 0..blades {
        let a = i as f32 / blades as f32 * TAU + (i as f32 * 0.7);
        let (s, c) = a.sin_cos();
        let off = Vec3::new(c * 0.12, 0.0, s * 0.12);
        let h = 0.55 + 0.35 * ((i * 7 % 5) as f32 / 4.0);
        let lean = Vec3::new(c, 0.0, s) * (0.18 + 0.1 * (i % 3) as f32);
        let side = Vec3::new(-s, 0.0, c) * 0.05;
        // base two verts, mid two, tip
        let base_l = off - side;
        let base_r = off + side;
        let mid_l = off + lean * 0.4 + Vec3::Y * (h * 0.55) - side * 0.6;
        let mid_r = off + lean * 0.4 + Vec3::Y * (h * 0.55) + side * 0.6;
        let tip = off + lean + Vec3::Y * h;
        let n = Vec3::new(-c, 0.3, -s).normalize();
        let mk = |b: &mut MeshBuilder, p: Vec3, shade: f32, sway: f32| {
            let v = b.vert(p, n, 1.0);
            b.mesh.verts[v as usize].col = to_srgb8(Vec3::splat(shade));
            b.set_sway(v, sway);
            v
        };
        let v0 = mk(&mut b, base_l, 0.55, 0.0);
        let v1 = mk(&mut b, base_r, 0.55, 0.0);
        let v2 = mk(&mut b, mid_l, 0.85, 0.5);
        let v3 = mk(&mut b, mid_r, 0.85, 0.5);
        let v4 = mk(&mut b, tip, 1.0, 1.0);
        b.quad(v0, v1, v3, v2);
        b.tri(v2, v3, v4);
        // back faces (the pipeline for grass is double sided, but keep winding consistent)
    }
    b.finish()
}

fn stump() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::BARK).hex(0x6a4a2e).ao(0.55, 1.0);
    b.cylinder(Vec3::ZERO, 0.42, 0.34, 0.7, 7, false, true);
    b.finish()
}

fn hay_bale() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::FLAT).hex(0xe2b94a).ao(0.55, 1.0);
    b.push_xf(Mat4::from_rotation_z(PI / 2.0) * Mat4::from_translation(Vec3::new(0.0, -0.6, 0.0)));
    b.cylinder(Vec3::ZERO, 0.62, 0.62, 1.2, 12, true, true);
    b.pop_xf();
    b.hex(0xc89a30);
    for k in [-0.3f32, 0.3] {
        b.push_xf(Mat4::from_translation(Vec3::new(k, 0.62, 0.0)));
        b.box_center(Vec3::ZERO, Vec3::new(0.025, 0.005, 0.64));
        b.pop_xf();
    }
    b.finish()
}

fn barrel() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(0x3f6fb8).spec(0.5).ao(0.6, 1.0);
    b.cylinder(Vec3::ZERO, 0.42, 0.42, 1.0, 12, true, true);
    b.hex(0x2a4d86);
    for y in [0.15f32, 0.5, 0.85] {
        b.cylinder(Vec3::new(0.0, y - 0.025, 0.0), 0.435, 0.435, 0.05, 12, false, false);
    }
    b.finish()
}

fn crate_mesh() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::WOOD).hex(0xb88a52).ao(0.6, 1.0);
    b.box_min_max(Vec3::new(-0.5, 0.0, -0.5), Vec3::new(0.5, 1.0, 0.5));
    b.hex(0x8d6636);
    // corner posts + cross braces
    for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        b.box_center(Vec3::new(sx * 0.47, 0.5, sz * 0.47), Vec3::new(0.06, 0.51, 0.06));
    }
    b.box_center(Vec3::new(0.0, 0.5, 0.505), Vec3::new(0.5, 0.05, 0.02));
    b.box_center(Vec3::new(0.0, 0.5, -0.505), Vec3::new(0.5, 0.05, 0.02));
    b.finish()
}

fn windmill_blades() -> MeshData {
    // Blades are built around the origin in the XY plane, rotating about Z.
    let mut b = MeshBuilder::new();
    b.mat(mat::WOOD).hex(0xf0e6cf);
    for k in 0..4 {
        let a = k as f32 * PI / 2.0 + PI / 4.0;
        b.push_xf(Mat4::from_rotation_z(a));
        b.hex(0x8b6a43);
        b.box_min_max(Vec3::new(-0.12, 0.8, -0.1), Vec3::new(0.12, 8.6, 0.1));
        b.hex(0xf4efe3);
        b.box_min_max(Vec3::new(0.12, 2.0, -0.05), Vec3::new(1.7, 8.4, 0.05));
        b.hex(0xc9bda3);
        for r in 0..5 {
            let y = 2.4 + r as f32 * 1.2;
            b.box_min_max(Vec3::new(0.12, y, -0.07), Vec3::new(1.7, y + 0.07, 0.07));
        }
        b.pop_xf();
    }
    b.hex(0x6f5233);
    b.box_center(Vec3::ZERO, Vec3::new(0.5, 0.5, 0.45));
    b.finish()
}

fn unit_box() -> MeshData {
    let mut b = MeshBuilder::new();
    b.hex(0xffffff).box_center(Vec3::ZERO, Vec3::splat(0.5));
    b.finish()
}
fn unit_sphere(subdiv: u32) -> MeshData {
    let mut b = MeshBuilder::new();
    b.hex(0xffffff).sphere(Vec3::ZERO, 0.5, subdiv);
    b.finish()
}
fn unit_cylinder() -> MeshData {
    let mut b = MeshBuilder::new();
    b.hex(0xffffff).cylinder(Vec3::new(0.0, -0.5, 0.0), 0.5, 0.5, 1.0, 14, true, true);
    b.finish()
}
fn unit_cone() -> MeshData {
    let mut b = MeshBuilder::new();
    b.hex(0xffffff).cylinder(Vec3::new(0.0, -0.5, 0.0), 0.5, 0.0, 1.0, 14, true, false);
    b.finish()
}

/// Build the mesh for one id. Panics on `Count`.
pub fn build_mesh(id: MeshId) -> MeshData {
    use MeshId::*;
    match id {
        UnitBox => unit_box(),
        UnitSphere1 => unit_sphere(1),
        UnitSphere2 => unit_sphere(2),
        UnitCylinder => unit_cylinder(),
        UnitCone => unit_cone(),
        Pine0 => pine(true),
        Pine1 => pine(false),
        Oak0 => oak(true),
        Oak1 => oak(false),
        Birch0 => birch(true),
        Birch1 => birch(false),
        Palm0 => palm(true),
        Palm1 => palm(false),
        Bush0 => bush(true),
        Bush1 => bush(false),
        Rock0 => rock(0, false),
        Rock1 => rock(1, false),
        Rock2 => rock(2, false),
        Boulder => rock(3, true),
        FlowerClump => flower_clump(),
        GrassTuft => grass_tuft(),
        Stump => stump(),
        HayBale => hay_bale(),
        Barrel => barrel(),
        Crate => crate_mesh(),
        WindmillBlades => windmill_blades(),
        CharTorso => models::char_torso(),
        CharTrim => models::char_trim(),
        CharPelvis => models::char_pelvis(),
        CharHead => models::char_head(),
        Hair1 => models::hair(1),
        Hair2 => models::hair(2),
        Hair3 => models::hair(3),
        Cap => models::headgear(1),
        Beanie => models::headgear(2),
        Helmet => models::headgear(3),
        Hat => models::headgear(4),
        CharArmUp => models::char_arm_up(),
        CharArmLow => models::char_arm_low(),
        CharHand => models::char_hand(),
        CharLegUp => models::char_leg_up(),
        CharLegLow => models::char_leg_low(),
        CharBoot => models::char_boot(),
        CharBackpack => models::char_backpack(),
        Glider => models::glider(),
        WpnPistol => models::weapon_pistol(),
        WpnSmg => models::weapon_smg(),
        WpnAr => models::weapon_ar(),
        WpnShotgun => models::weapon_shotgun(),
        WpnSniper => models::weapon_sniper(),
        WpnRocket => models::weapon_rocket(),
        Pickaxe => models::pickaxe(),
        AmmoBox => models::ammo_box(),
        ItemBandage => models::item_bandage(),
        ItemMedkit => models::item_medkit(),
        ItemShieldMini => models::item_shield_mini(),
        ItemShieldBig => models::item_shield_big(),
        ItemChug => models::item_chug(),
        ChestBase => models::chest_base(),
        ChestLid => models::chest_lid(),
        Bus => models::bus(),
        Missile => models::rocket_missile(),
        PieceWall => models::piece_wall(),
        PieceFloor => models::piece_floor(),
        PieceRoof => models::piece_roof(),
        PieceRamp => models::piece_ramp(),
        Count => panic!("MeshId::Count is not a mesh"),
    }
}

pub const ALL_IDS: &[MeshId] = &[
    MeshId::UnitBox,
    MeshId::UnitSphere1,
    MeshId::UnitSphere2,
    MeshId::UnitCylinder,
    MeshId::UnitCone,
    MeshId::Pine0,
    MeshId::Pine1,
    MeshId::Oak0,
    MeshId::Oak1,
    MeshId::Birch0,
    MeshId::Birch1,
    MeshId::Palm0,
    MeshId::Palm1,
    MeshId::Bush0,
    MeshId::Bush1,
    MeshId::Rock0,
    MeshId::Rock1,
    MeshId::Rock2,
    MeshId::Boulder,
    MeshId::FlowerClump,
    MeshId::GrassTuft,
    MeshId::Stump,
    MeshId::HayBale,
    MeshId::Barrel,
    MeshId::Crate,
    MeshId::WindmillBlades,
    MeshId::CharTorso,
    MeshId::CharTrim,
    MeshId::CharPelvis,
    MeshId::CharHead,
    MeshId::Hair1,
    MeshId::Hair2,
    MeshId::Hair3,
    MeshId::Cap,
    MeshId::Beanie,
    MeshId::Helmet,
    MeshId::Hat,
    MeshId::CharArmUp,
    MeshId::CharArmLow,
    MeshId::CharHand,
    MeshId::CharLegUp,
    MeshId::CharLegLow,
    MeshId::CharBoot,
    MeshId::CharBackpack,
    MeshId::Glider,
    MeshId::WpnPistol,
    MeshId::WpnSmg,
    MeshId::WpnAr,
    MeshId::WpnShotgun,
    MeshId::WpnSniper,
    MeshId::WpnRocket,
    MeshId::Pickaxe,
    MeshId::AmmoBox,
    MeshId::ItemBandage,
    MeshId::ItemMedkit,
    MeshId::ItemShieldMini,
    MeshId::ItemShieldBig,
    MeshId::ItemChug,
    MeshId::ChestBase,
    MeshId::ChestLid,
    MeshId::Bus,
    MeshId::Missile,
    MeshId::PieceWall,
    MeshId::PieceFloor,
    MeshId::PieceRoof,
    MeshId::PieceRamp,
];

/// All meshes in `MeshId` order.
pub fn build_all() -> Vec<MeshData> {
    assert_eq!(ALL_IDS.len(), MeshId::Count as usize, "ALL_IDS must list every MeshId in order");
    ALL_IDS.iter().enumerate().map(|(i, id)| {
        assert_eq!(*id as usize, i, "ALL_IDS out of order at {i}");
        build_mesh(*id)
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_meshes_build_and_have_sane_sizes() {
        let all = build_all();
        assert_eq!(all.len(), MeshId::Count as usize);
        for (i, m) in all.iter().enumerate() {
            assert!(!m.is_empty(), "mesh {i} ({:?}) empty", ALL_IDS[i]);
            assert!(m.verts.len() < 5000, "mesh {:?} has {} verts", ALL_IDS[i], m.verts.len());
            let bb = m.bounds();
            assert!(bb.min.is_finite() && bb.max.is_finite());
            for v in &m.verts {
                assert!(v.nrm.iter().all(|c| c.is_finite()));
            }
            assert!(m.idx.iter().all(|&ix| (ix as usize) < m.verts.len()));
        }
    }

    #[test]
    fn trees_have_plausible_heights() {
        let h = |id: MeshId| build_mesh(id).bounds().max.y;
        assert!((8.0..12.0).contains(&h(MeshId::Pine0)), "pine {}", h(MeshId::Pine0));
        assert!((6.0..9.0).contains(&h(MeshId::Oak0)), "oak {}", h(MeshId::Oak0));
        assert!((6.5..9.0).contains(&h(MeshId::Birch0)), "birch {}", h(MeshId::Birch0));
        assert!((6.0..9.0).contains(&h(MeshId::Palm0)), "palm {}", h(MeshId::Palm0));
        // LOD1 should be much cheaper than LOD0
        assert!(build_mesh(MeshId::Pine1).tri_count() * 2 < build_mesh(MeshId::Pine0).tri_count());
        assert!(build_mesh(MeshId::Oak1).tri_count() * 3 < build_mesh(MeshId::Oak0).tri_count());
    }
}
