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
    // Original brick toy variants; the scene selects these only in LEGO mode.
    LegoTorso,
    LegoTrim,
    LegoPelvis,
    LegoHead,
    LegoArmUp,
    LegoArmLow,
    LegoHand,
    LegoLegUp,
    LegoLegLow,
    LegoBoot,
    LegoBackpack,
    LegoHair,
    LegoHat,
    LegoPistol,
    LegoSmg,
    LegoAr,
    LegoShotgun,
    LegoSniper,
    LegoRocket,
    LegoPickaxe,
    LegoAmmo,
    LegoBandage,
    LegoMedkit,
    LegoShieldMini,
    LegoShieldBig,
    LegoChug,
    LegoChestBase,
    LegoChestLid,
    LegoWall,
    LegoFloor,
    LegoRoof,
    LegoRamp,
    LegoPine,
    LegoOak,
    LegoBirch,
    LegoPalm,
    LegoBush,
    LegoRock,
    LegoGrass,
    LegoFlowers,
    LegoStump,
    LegoCrate,
    LegoBarrel,
    LegoHay,
    VehicleBody,
    VehicleWheel,
    // --- vegetation variants (the slim spruce, the broad oak, the tall birch, the flowering bush) ---
    Pine2,
    Pine3,
    Oak2,
    Oak3,
    Birch2,
    Birch3,
    Bush2,
    Bush3,
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

/// One flat triangle with its own ambient-occlusion values; the winding follows `outward`.
fn flat_tri(b: &mut MeshBuilder, p: [Vec3; 3], ao: [f32; 3], outward: Vec3) {
    let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
    let n = if n.dot(outward) < 0.0 { -n } else { n };
    let i = [b.vert(p[0], n, ao[0]), b.vert(p[1], n, ao[1]), b.vert(p[2], n, ao[2])];
    b.tri(i[0], i[1], i[2]);
}

/// One faceted fir tier: an apex over a ring that alternates between long spikes (the branch tips) and
/// short valleys, so the outline reads as layered branches instead of a smooth cone.
#[allow(clippy::too_many_arguments)]
fn fir_tier(b: &mut MeshBuilder, base: Vec3, r: f32, h: f32, spikes: u32, twist: f32, shade: f32, ao_lo: f32) {
    let n = (spikes * 2) as usize;
    let ring: Vec<Vec3> = (0..n)
        .map(|i| {
            let a = twist + i as f32 * PI / spikes as f32;
            let tip = i % 2 == 0;
            let rr = if tip { r } else { r * 0.7 };
            base + Vec3::new(a.cos() * rr, if tip { 0.0 } else { h * 0.13 }, a.sin() * rr)
        })
        .collect();
    let apex = base + Vec3::Y * h;
    let under = base + Vec3::Y * (h * 0.1);
    for i in 0..n {
        let (p0, p1) = (ring[i], ring[(i + 1) % n]);
        let mid = (p0 + p1) * 0.5 - base;
        b.color(tree_gray(shade * if i % 2 == 0 { 1.0 } else { 0.9 }));
        flat_tri(b, [p0, p1, apex], [ao_lo, ao_lo, 1.0], Vec3::new(mid.x, h * 0.4, mid.z));
        b.color(tree_gray(shade * 0.55));
        flat_tri(b, [p0, p1, under], [ao_lo * 0.7; 3], Vec3::NEG_Y);
    }
}

/// Fir trees. Foliage vertex colours are neutral greys; the instance tint supplies the hue.
/// `slim` is the tall narrow spruce; the other is the broad pine.
fn pine(detail: bool, slim: bool) -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::BARK).tinted(false).hex(0x6b4a2b).ao(0.55, 1.0);
    let trunk_h = if detail { 2.4 } else { 3.0 };
    b.cylinder(Vec3::ZERO, 0.30, 0.18, trunk_h, if detail { 7 } else { 5 }, false, false);
    b.mat(mat::FOLIAGE).tinted(true).sway(0.0);
    if detail {
        // (tiers, first tier base, spacing, widest radius, narrowest radius, tier height)
        let (tiers, y0, step, r0, r1, h0) = if slim { (8usize, 1.2f32, 1.05f32, 1.85f32, 0.5f32, 1.9f32) } else { (6, 1.25, 1.35, 2.5, 0.75, 2.5) };
        for i in 0..tiers {
            let t = i as f32 / (tiers - 1) as f32;
            let r = r0 + (r1 - r0) * t;
            let h = h0 - 0.1 * i as f32;
            b.sway(0.12 + t * 0.5);
            let spikes = if slim { 7 - (i as u32) / 3 } else { 9 - (i as u32) / 2 };
            fir_tier(&mut b, Vec3::new(0.0, y0 + i as f32 * step, 0.0), r, h, spikes, i as f32 * 0.55, 0.72 + 0.28 * t, 0.5 + 0.25 * t);
        }
    } else {
        let tiers = 3;
        for i in 0..tiers {
            let t = i as f32 / (tiers - 1) as f32;
            b.color(tree_gray(0.74 + 0.26 * t)).ao(0.55 + 0.2 * t, 1.0);
            let base_y = if slim { 1.5 + i as f32 * 2.7 } else { 1.7 + i as f32 * 2.4 };
            let r = (if slim { 1.9 } else { 2.5 }) * (1.0 - t * 0.62);
            let h = if slim { 3.8 - i as f32 * 0.2 } else { 3.8 - i as f32 * 0.3 };
            b.push_xf(Mat4::from_rotation_y(i as f32 * 0.55));
            b.sway(0.15 + t * 0.5);
            b.cone(Vec3::new(0.0, base_y, 0.0), r, h, 6);
            b.pop_xf();
        }
    }
    b.finish()
}

/// Round-crowned tree. `broad` is the wide, low oak with a short thick trunk.
fn oak(detail: bool, broad: bool) -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::BARK).tinted(false).hex(0x6a4a2e).ao(0.55, 1.0);
    let trunk_h = if broad { 2.6 } else { 3.2 };
    let (r0, r1) = if broad { (0.46, 0.28) } else { (0.36, 0.22) };
    b.cylinder(Vec3::ZERO, r0, r1, trunk_h, if detail { 8 } else { 5 }, false, false);
    if detail {
        // root flare: short cones leaning out from the base
        for k in 0..4 {
            let a = k as f32 * 1.7 + 0.4;
            b.push_xf(Mat4::from_rotation_y(a) * Mat4::from_translation(Vec3::new(r0 * 0.7, 0.0, 0.0)) * Mat4::from_rotation_z(-0.95));
            b.cylinder(Vec3::ZERO, 0.17, 0.05, 0.85, 5, false, false);
            b.pop_xf();
        }
        // forks into the crown
        let forks: &[(f32, f32)] = if broad { &[(1.2, 0.5), (-1.1, -0.6), (0.2, 1.2)] } else { &[(0.9, 0.3), (-0.8, -0.5)] };
        for &(dx, dz) in forks {
            let dir = Vec3::new(dx, 1.0, dz).normalize();
            b.push_xf(Mat4::from_translation(Vec3::new(0.0, trunk_h - 0.8, 0.0)) * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, dir)));
            b.cylinder(Vec3::ZERO, 0.16, 0.08, 1.8, 5, false, false);
            b.pop_xf();
        }
    }
    b.mat(mat::FOLIAGE).tinted(true);
    // (centre, radii, tone): the lower ring is darker so the crown has an underside
    let tall: &[(Vec3, Vec3, f32)] = &[
        (Vec3::new(0.0, 4.9, 0.0), Vec3::new(2.7, 2.2, 2.7), 0.92),
        (Vec3::new(1.9, 4.1, 0.7), Vec3::new(1.9, 1.7, 1.9), 0.85),
        (Vec3::new(-1.7, 4.3, -0.8), Vec3::new(2.0, 1.7, 2.0), 0.86),
        (Vec3::new(0.3, 6.2, -0.3), Vec3::new(1.8, 1.5, 1.8), 1.0),
        (Vec3::new(0.5, 3.9, -1.8), Vec3::new(1.7, 1.4, 1.7), 0.8),
        (Vec3::new(-0.6, 4.0, 1.9), Vec3::new(1.6, 1.4, 1.6), 0.82),
        (Vec3::new(-1.4, 5.5, 1.3), Vec3::new(1.4, 1.2, 1.4), 0.95),
        (Vec3::new(1.3, 5.6, -1.4), Vec3::new(1.4, 1.2, 1.4), 0.97),
        (Vec3::new(0.2, 3.5, 0.2), Vec3::new(2.3, 1.0, 2.3), 0.68),
    ];
    let wide: &[(Vec3, Vec3, f32)] = &[
        (Vec3::new(0.0, 4.2, 0.0), Vec3::new(3.1, 2.0, 3.1), 0.92),
        (Vec3::new(2.7, 3.7, 0.9), Vec3::new(2.2, 1.6, 2.2), 0.84),
        (Vec3::new(-2.5, 3.8, -1.0), Vec3::new(2.3, 1.6, 2.1), 0.85),
        (Vec3::new(0.6, 3.6, -2.7), Vec3::new(2.2, 1.5, 2.0), 0.8),
        (Vec3::new(-0.7, 3.7, 2.6), Vec3::new(2.1, 1.5, 2.0), 0.82),
        (Vec3::new(0.3, 5.5, 0.3), Vec3::new(2.1, 1.4, 2.1), 1.0),
        (Vec3::new(1.8, 5.0, -1.6), Vec3::new(1.6, 1.2, 1.6), 0.96),
        (Vec3::new(-1.9, 5.0, 1.5), Vec3::new(1.6, 1.2, 1.6), 0.95),
        (Vec3::new(0.0, 3.1, 0.0), Vec3::new(2.8, 0.9, 2.8), 0.66),
    ];
    let blobs = if broad { wide } else { tall };
    let n = if detail { blobs.len() } else { 3 };
    for (i, (c, r, tone)) in blobs.iter().take(n).enumerate() {
        b.color(tree_gray(*tone)).ao(0.52, 1.0).sway(0.4 + 0.3 * (c.y / 6.0));
        b.blob(*c, *r, if detail { 1 } else { 0 }, if detail { 0.2 } else { 0.1 }, 7 + i as u32 * 13, true);
    }
    b.finish()
}

/// Slim white-trunked tree with a clumpy crown. `tall` is the lanky one.
fn birch(detail: bool, tall: bool) -> MeshData {
    let mut b = MeshBuilder::new();
    let trunk_h = if tall { 6.2 } else { 5.2 };
    b.mat(mat::BARK).tinted(false).hex(0xe7e3d6).ao(0.6, 1.0);
    b.cylinder(Vec3::ZERO, 0.22, 0.12, trunk_h, if detail { 7 } else { 5 }, false, false);
    if detail {
        // dark birch marks and a couple of stub branches
        b.hex(0x3b3a36);
        let marks = if tall { 8 } else { 6 };
        for k in 0..marks {
            let y = 0.6 + k as f32 * (trunk_h - 1.2) / marks as f32;
            let a = k as f32 * 2.1;
            b.push_xf(Mat4::from_translation(Vec3::new(a.cos() * 0.19, y, a.sin() * 0.19)) * Mat4::from_rotation_y(-a));
            b.box_center(Vec3::ZERO, Vec3::new(0.03, 0.05, 0.1));
            b.pop_xf();
        }
        b.hex(0xcfcabb);
        for (k, y) in [(0.0f32, trunk_h * 0.55), (2.6, trunk_h * 0.7)] {
            b.push_xf(Mat4::from_translation(Vec3::new(0.0, y, 0.0)) * Mat4::from_rotation_y(k) * Mat4::from_rotation_z(-0.9));
            b.cylinder(Vec3::ZERO, 0.07, 0.035, 0.9, 5, false, false);
            b.pop_xf();
        }
    }
    b.mat(mat::FOLIAGE).tinted(true);
    let dy = if tall { 1.0 } else { 0.0 };
    let blobs: &[(Vec3, Vec3)] = &[
        (Vec3::new(0.0, 5.4 + dy, 0.0), Vec3::new(1.5, 1.7, 1.5)),
        (Vec3::new(0.9, 4.5 + dy, 0.3), Vec3::new(1.1, 1.2, 1.1)),
        (Vec3::new(-0.8, 4.8 + dy, -0.4), Vec3::new(1.2, 1.3, 1.2)),
        (Vec3::new(0.1, 6.5 + dy, -0.1), Vec3::new(1.0, 1.1, 1.0)),
        (Vec3::new(-0.5, 5.9 + dy, 0.8), Vec3::new(0.9, 1.0, 0.9)),
        (Vec3::new(0.6, 5.7 + dy, -0.8), Vec3::new(0.9, 1.0, 0.9)),
    ];
    for (i, (c, r)) in blobs.iter().take(if detail { 6 } else { 2 }).enumerate() {
        b.color(tree_gray(0.86 + 0.14 * ((c.y - dy - 4.0) / 3.0).clamp(0.0, 1.0))).ao(0.58, 1.0).sway(0.55);
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
    b.mat(mat::BARK).tinted(false).ao(0.6, 1.0);
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
    // fronds: arched strips whose edges alternate between wide and narrow, like leaflets
    b.mat(mat::FOLIAGE).tinted(true);
    let fronds = if detail { 10 } else { 6 };
    for f in 0..fronds {
        let a = f as f32 / fronds as f32 * TAU + 0.3;
        let (sa, ca) = a.sin_cos();
        let len = 3.7 + (f % 3) as f32 * 0.35;
        let steps = if detail { 10 } else { 3 };
        let mut prev_l = pos;
        let mut prev_r = pos;
        for s in 1..=steps {
            let t = s as f32 / steps as f32;
            let out = t * len;
            let droop = -0.55 * t * t * len + 1.1 * t * (1.0 - t) * len * 0.5;
            let leaflet = if detail && s % 2 == 0 { 0.62 } else { 1.0 };
            let w = (0.6 * (1.0 - t * 0.8) + 0.05) * leaflet;
            let centre = pos + Vec3::new(ca * out, droop, sa * out);
            let side = Vec3::new(-sa, 0.0, ca) * w;
            let l = centre - side + Vec3::Y * (0.12 * t);
            let r = centre + side + Vec3::Y * (0.12 * t);
            let shade = 0.78 + 0.22 * t - if f % 2 == 0 { 0.0 } else { 0.06 };
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
        b.mat(mat::FLAT).tinted(false).hex(0x5a3d22).sway(0.0).ao(0.7, 1.0);
        for k in 0..4 {
            let a = k as f32 * 1.6;
            b.sphere(pos + Vec3::new(a.cos() * 0.25, -0.3, a.sin() * 0.25), 0.17, 1);
        }
    }
    b.finish()
}

/// Bush. The flowering variant is dotted with blossoms that keep their own colours.
fn bush(detail: bool, flowers: bool) -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::FOLIAGE);
    let blobs: &[(Vec3, Vec3, f32)] = &[
        (Vec3::new(0.0, 0.55, 0.0), Vec3::new(0.95, 0.65, 0.95), 1.0),
        (Vec3::new(0.7, 0.45, 0.3), Vec3::new(0.65, 0.5, 0.65), 0.92),
        (Vec3::new(-0.6, 0.45, -0.3), Vec3::new(0.7, 0.5, 0.7), 0.9),
        (Vec3::new(0.0, 0.35, 0.7), Vec3::new(0.6, 0.42, 0.6), 0.85),
        (Vec3::new(-0.2, 0.95, -0.1), Vec3::new(0.6, 0.42, 0.6), 1.05),
    ];
    let n = if detail { blobs.len() } else { 2 };
    for (i, (c, r, tone)) in blobs.iter().take(n).enumerate() {
        b.color(tree_gray((0.82 + 0.18 * c.y) * tone)).ao(0.5, 1.0).sway(0.35);
        b.blob(*c, *r, if detail { 1 } else { 0 }, 0.18, 3 + i as u32 * 11, true);
    }
    if flowers {
        b.tinted(false).mat(mat::FLAT).sway(0.35).ao(1.0, 1.0);
        let pal = [0xff7fa8u32, 0xfdfbf0, 0xffd23f, 0xff7fa8, 0xfdfbf0];
        let count = if detail { 16 } else { 7 };
        for k in 0..count {
            // golden-angle spiral over the upper half of the bush
            let a = k as f32 * 2.399;
            let v = 0.25 + 0.7 * ((k * 7 % count) as f32 / count as f32);
            let (sx, sz) = (a.cos() * (1.0 - v * v).sqrt(), a.sin() * (1.0 - v * v).sqrt());
            let p = Vec3::new(sx * 0.95, 0.55 + v * 0.62, sz * 0.95);
            b.hex(pal[k as usize % pal.len()]);
            b.sphere(p, 0.085, 0);
        }
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
    // a lighter, flatter slab leaning on the side and a pebble or two at the foot
    b.hex(0x9a9ba2);
    let slab = Vec3::new(radii.x * 0.55, radii.y * 0.4, radii.z * 0.5);
    b.blob(Vec3::new(-radii.x * 0.75, slab.y * 0.7, radii.z * 0.35), slab, 1, amp, 40 + variant, false);
    b.hex(0x77787f);
    b.blob(Vec3::new(radii.x * 0.15, radii.y * 0.12, -radii.z * 1.05), radii * 0.22, 0, amp, 51 + variant, false);
    if boulder {
        b.blob(Vec3::new(-radii.x * 0.2, radii.y * 0.1, radii.z * 1.1), radii * 0.2, 0, amp, 63, false);
    }
    // moss on top (keeps its green whatever the rock is tinted)
    b.tinted(false).mat(mat::FLAT).hex(0x6fa84f).ao(0.8, 1.0);
    let top = radii.y * 0.5 + radii.y * 0.8;
    b.blob(Vec3::new(radii.x * 0.05, top, 0.0), Vec3::new(radii.x * 0.62, radii.y * 0.3, radii.z * 0.55), 1, 0.1, 71 + variant, true);
    b.finish()
}

/// Meadow flowers: four blooms on short stems, each a ring of petals around a yellow eye. The
/// petals take the instance colour so every clump is a different colour. Ground cover is drawn
/// double sided, so every part is a single quad.
fn flower_clump() -> MeshData {
    let mut b = MeshBuilder::new();
    for (k, (x, z, r, h)) in [(0.0f32, 0.0f32, 0.17f32, 0.34f32), (0.22, 0.12, 0.13, 0.28), (-0.18, 0.15, 0.14, 0.3), (0.05, -0.22, 0.13, 0.26)].iter().enumerate() {
        let c = Vec3::new(*x, *h, *z);
        // stem: a thin flat blade facing a different way for every bloom
        b.mat(mat::GRASS).tinted(false).hex(0x58a63c).ao(0.8, 1.0).sway(0.3);
        let side = Vec3::new(k as f32 * 0.7 + 0.3, 0.0, 1.0).normalize() * 0.012;
        b.quad_flat([Vec3::new(*x, 0.0, *z) - side, Vec3::new(*x, 0.0, *z) + side, c + side, c - side], [0.7, 0.7, 1.0, 1.0]);
        // petals: flat kites around the eye, tilted up a little
        b.mat(mat::FOLIAGE).tinted(true).sway(0.6).ao(0.75, 1.0);
        let petals = 5;
        for p in 0..petals {
            let a = p as f32 / petals as f32 * TAU + k as f32;
            let (sa, ca) = a.sin_cos();
            let dir = Vec3::new(ca, 0.18, sa);
            let side = Vec3::new(-sa, 0.0, ca) * (r * 0.45);
            b.color(Vec3::splat(if p % 2 == 0 { 1.0 } else { 0.86 }));
            b.quad_flat([c, c + dir * (r * 0.5) + side, c + dir * r + Vec3::Y * 0.02, c + dir * (r * 0.5) - side], [0.8, 1.0, 1.0, 1.0]);
        }
        b.mat(mat::FLAT).tinted(false).hex(0xffc83a).sway(0.6).ao(1.0, 1.0);
        b.sphere(c + Vec3::Y * 0.02, r * 0.28, 0);
    }
    // leaves stay green whatever the flower colour is
    b.mat(mat::GRASS).tinted(false).hex(0x58a63c).sway(0.0).ao(1.0, 1.0);
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
    let blades = 7;
    for i in 0..blades {
        let a = i as f32 / blades as f32 * TAU + (i as f32 * 0.7);
        let (s, c) = a.sin_cos();
        let off = Vec3::new(c * 0.12, 0.0, s * 0.12);
        let h = (0.42 + 0.3 * ((i * 7 % 5) as f32 / 4.0)) * 0.9;
        let lean = Vec3::new(c, 0.0, s) * (0.16 + 0.09 * (i % 3) as f32);
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
    b.mat(mat::BARK).tinted(false).hex(0x6a4a2e).ao(0.55, 1.0);
    b.cylinder(Vec3::ZERO, 0.42, 0.34, 0.7, 9, false, false);
    // roots flaring out at the foot
    for k in 0..4 {
        let a = k as f32 * 1.7 + 0.3;
        b.push_xf(Mat4::from_rotation_y(a) * Mat4::from_translation(Vec3::new(0.3, 0.0, 0.0)) * Mat4::from_rotation_z(-1.0));
        b.cylinder(Vec3::ZERO, 0.12, 0.04, 0.5, 5, false, false);
        b.pop_xf();
    }
    // the cut face with growth rings
    b.mat(mat::FLAT).hex(0xd9b27a).ao(1.0, 1.0);
    b.cylinder(Vec3::new(0.0, 0.7, 0.0), 0.34, 0.34, 0.012, 9, false, true);
    b.hex(0xb98d56);
    b.cylinder(Vec3::new(0.0, 0.7, 0.0), 0.22, 0.22, 0.016, 9, false, true);
    b.hex(0xd9b27a);
    b.cylinder(Vec3::new(0.0, 0.7, 0.0), 0.12, 0.12, 0.02, 9, false, true);
    b.finish()
}

fn hay_bale() -> MeshData {
    let mut b = MeshBuilder::new();
    // a round bale lying on its side: built upright along Y, then tipped over and set on the ground
    b.push_xf(Mat4::from_translation(Vec3::new(0.0, 0.6, 0.0)) * Mat4::from_rotation_z(PI / 2.0));
    b.mat(mat::FLAT).hex(0xe2b94a).ao(0.55, 1.0);
    b.cylinder(Vec3::new(0.0, -0.6, 0.0), 0.62, 0.62, 1.2, 14, true, true);
    // twine around the roll
    b.hex(0xb98a2a);
    for y in [-0.3f32, 0.3] {
        b.cylinder(Vec3::new(0.0, y - 0.015, 0.0), 0.633, 0.633, 0.03, 14, false, false);
    }
    // a spiral of lighter straw on both ends
    b.hex(0xf0d27a).ao(1.0, 1.0);
    for r in [0.5f32, 0.3, 0.12] {
        b.cylinder(Vec3::new(0.0, 0.6, 0.0), r, r, 0.006, 12, false, true);
        b.cylinder(Vec3::new(0.0, -0.606, 0.0), r, r, 0.006, 12, true, false);
        b.hex(0xd9b34e);
    }
    b.pop_xf();
    b.finish()
}

fn barrel() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).hex(0x3f6fb8).spec(0.5).ao(0.6, 1.0);
    b.cylinder(Vec3::ZERO, 0.42, 0.42, 1.0, 14, true, false);
    // rolled rim and a recessed lid with a bung and a vent
    b.hex(0x34609f);
    b.cylinder(Vec3::new(0.0, 0.96, 0.0), 0.435, 0.435, 0.07, 14, false, true);
    b.hex(0x2a4d86);
    b.cylinder(Vec3::new(0.0, 1.0, 0.0), 0.36, 0.36, 0.004, 14, false, true);
    b.hex(0xc7ccd4);
    b.cylinder(Vec3::new(0.15, 1.004, 0.1), 0.06, 0.06, 0.04, 8, false, true);
    b.cylinder(Vec3::new(-0.15, 1.004, -0.1), 0.045, 0.045, 0.03, 8, false, true);
    // hoops
    b.hex(0x2a4d86);
    for y in [0.15f32, 0.5, 0.85] {
        b.cylinder(Vec3::new(0.0, y - 0.025, 0.0), 0.435, 0.435, 0.05, 14, false, false);
    }
    // a hazard label on the side (untinted)
    b.mat(mat::FLAT).tinted(false).hex(0xf2c94c);
    b.box_center(Vec3::new(0.0, 0.5, -0.4235), Vec3::new(0.12, 0.09, 0.008));
    b.hex(0x2b2e34);
    for k in 0..3 {
        b.box_center(Vec3::new(-0.08 + k as f32 * 0.08, 0.5, -0.4325), Vec3::new(0.016, 0.075, 0.002));
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
    for y in [0.12f32, 0.88] {
        b.box_center(Vec3::new(0.0, y, 0.505), Vec3::new(0.5, 0.05, 0.02));
        b.box_center(Vec3::new(0.0, y, -0.505), Vec3::new(0.5, 0.05, 0.02));
        b.box_center(Vec3::new(0.505, y, 0.0), Vec3::new(0.02, 0.05, 0.5));
        b.box_center(Vec3::new(-0.505, y, 0.0), Vec3::new(0.02, 0.05, 0.5));
    }
    // diagonal braces on the four sides
    for k in 0..4 {
        b.push_xf(Mat4::from_rotation_y(k as f32 * FRAC_PI_2_F) * Mat4::from_translation(Vec3::new(0.0, 0.5, 0.505)) * Mat4::from_rotation_z(0.8));
        b.box_center(Vec3::ZERO, Vec3::new(0.52, 0.045, 0.014));
        b.pop_xf();
    }
    // a lid with a lip
    b.hex(0xa87a46);
    b.box_min_max(Vec3::new(-0.53, 1.0, -0.53), Vec3::new(0.53, 1.04, 0.53));
    b.finish()
}

const FRAC_PI_2_F: f32 = std::f32::consts::FRAC_PI_2;

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
    if let Some(mesh) = crate::lego_models::mesh(id) { return mesh; }
    if let Some(mesh) = crate::cartoon_assets::mesh(id) { return mesh; }
    use MeshId::*;
    match id {
        UnitBox => unit_box(),
        UnitSphere1 => unit_sphere(1),
        UnitSphere2 => unit_sphere(2),
        UnitCylinder => unit_cylinder(),
        UnitCone => unit_cone(),
        Pine0 => pine(true, false),
        Pine1 => pine(false, false),
        Oak0 => oak(true, false),
        Oak1 => oak(false, false),
        Birch0 => birch(true, false),
        Birch1 => birch(false, false),
        Palm0 => palm(true),
        Palm1 => palm(false),
        Bush0 => bush(true, false),
        Bush1 => bush(false, false),
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
        VehicleBody => crate::game::vehicles::body_mesh(),
        VehicleWheel => crate::game::vehicles::wheel_mesh(),
        Pine2 => pine(true, true),
        Pine3 => pine(false, true),
        Oak2 => oak(true, true),
        Oak3 => oak(false, true),
        Birch2 => birch(true, true),
        Birch3 => birch(false, true),
        Bush2 => bush(true, true),
        Bush3 => bush(false, true),
        _ => panic!("MeshId {id:?} is not a mesh"),
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
    MeshId::LegoTorso,
    MeshId::LegoTrim,
    MeshId::LegoPelvis,
    MeshId::LegoHead,
    MeshId::LegoArmUp,
    MeshId::LegoArmLow,
    MeshId::LegoHand,
    MeshId::LegoLegUp,
    MeshId::LegoLegLow,
    MeshId::LegoBoot,
    MeshId::LegoBackpack,
    MeshId::LegoHair,
    MeshId::LegoHat,
    MeshId::LegoPistol,
    MeshId::LegoSmg,
    MeshId::LegoAr,
    MeshId::LegoShotgun,
    MeshId::LegoSniper,
    MeshId::LegoRocket,
    MeshId::LegoPickaxe,
    MeshId::LegoAmmo,
    MeshId::LegoBandage,
    MeshId::LegoMedkit,
    MeshId::LegoShieldMini,
    MeshId::LegoShieldBig,
    MeshId::LegoChug,
    MeshId::LegoChestBase,
    MeshId::LegoChestLid,
    MeshId::LegoWall,
    MeshId::LegoFloor,
    MeshId::LegoRoof,
    MeshId::LegoRamp,
    MeshId::LegoPine,
    MeshId::LegoOak,
    MeshId::LegoBirch,
    MeshId::LegoPalm,
    MeshId::LegoBush,
    MeshId::LegoRock,
    MeshId::LegoGrass,
    MeshId::LegoFlowers,
    MeshId::LegoStump,
    MeshId::LegoCrate,
    MeshId::LegoBarrel,
    MeshId::LegoHay,
    MeshId::VehicleBody,
    MeshId::VehicleWheel,
    MeshId::Pine2,
    MeshId::Pine3,
    MeshId::Oak2,
    MeshId::Oak3,
    MeshId::Birch2,
    MeshId::Birch3,
    MeshId::Bush2,
    MeshId::Bush3,
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

    #[test]
    fn tree_variants_have_their_own_shape_and_cheap_distant_versions() {
        let b = |id: MeshId| build_mesh(id).bounds();
        // the slim spruce is as tall as the pine but narrower; the broad oak is lower and wider than the oak
        let (pine, slim) = (b(MeshId::Pine0), b(MeshId::Pine2));
        assert!((8.5..11.5).contains(&slim.max.y) && slim.max.x < pine.max.x * 0.9, "spruce {:?} vs pine {:?}", slim.max, pine.max);
        let (oak, broad) = (b(MeshId::Oak0), b(MeshId::Oak2));
        assert!(broad.max.x > oak.max.x * 1.05 && broad.max.y < oak.max.y, "broad oak {:?} vs oak {:?}", broad.max, oak.max);
        assert!((7.0..9.5).contains(&b(MeshId::Birch2).max.y), "tall birch {}", b(MeshId::Birch2).max.y);
        for (hi, lo) in [(MeshId::Pine2, MeshId::Pine3), (MeshId::Oak2, MeshId::Oak3), (MeshId::Birch2, MeshId::Birch3), (MeshId::Bush2, MeshId::Bush3)] {
            assert!(build_mesh(lo).tri_count() * 2 <= build_mesh(hi).tri_count(), "{lo:?} should be much cheaper than {hi:?}");
        }
    }

    #[test]
    fn flowering_bush_keeps_its_blossoms_untinted_and_plain_bush_has_none() {
        let untinted = |id: MeshId| build_mesh(id).verts.iter().filter(|v| v.col[3] == 0).count();
        assert!(untinted(MeshId::Bush2) > 100 && untinted(MeshId::Bush0) == 0);
        // rocks keep a green moss cap whatever tint the rock gets, flowers a yellow eye
        assert!(untinted(MeshId::Rock0) > 30 && untinted(MeshId::Boulder) > 30 && untinted(MeshId::FlowerClump) > 30);
    }
}
