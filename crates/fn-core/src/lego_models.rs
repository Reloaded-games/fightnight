//! Original brick toy meshes. These keep the cartoon rig's pivots and gameplay collision,
//! so switching the visual theme never changes aiming, movement or build placement.
use crate::game::GameMode;
use crate::game::pieces::{LEVEL_H, THICK, TILE};
use crate::math::*;
use crate::mesh::*;
use crate::meshlib::MeshId;
use crate::models::dims::*;
use std::f32::consts::FRAC_PI_2;

/// Alternate geometry only for the LEGO theme; other modes keep the Blender cartoon assets.
pub fn mapped_mesh(id: MeshId, mode: GameMode) -> MeshId {
    if mode != GameMode::Lego { return id; }
    use MeshId::*;
    match id {
        CharTorso => LegoTorso, CharTrim => LegoTrim, CharPelvis => LegoPelvis,
        CharHead => LegoHead, CharArmUp => LegoArmUp, CharArmLow => LegoArmLow,
        CharHand => LegoHand, CharLegUp => LegoLegUp, CharLegLow => LegoLegLow,
        CharBoot => LegoBoot, CharBackpack => LegoBackpack,
        Hair1 | Hair2 | Hair3 => LegoHair, Cap | Beanie | Helmet | Hat => LegoHat,
        WpnPistol => LegoPistol, WpnSmg => LegoSmg, WpnAr => LegoAr,
        WpnShotgun => LegoShotgun, WpnSniper => LegoSniper, WpnRocket => LegoRocket,
        Pickaxe => LegoPickaxe, AmmoBox => LegoAmmo, ItemBandage => LegoBandage,
        ItemMedkit => LegoMedkit, ItemShieldMini => LegoShieldMini,
        ItemShieldBig => LegoShieldBig, ItemChug => LegoChug,
        ChestBase => LegoChestBase, ChestLid => LegoChestLid,
        PieceWall => LegoWall, PieceFloor => LegoFloor, PieceRoof => LegoRoof, PieceRamp => LegoRamp,
        Pine0 | Pine1 => LegoPine, Oak0 | Oak1 => LegoOak, Birch0 | Birch1 => LegoBirch,
        Palm0 | Palm1 => LegoPalm, Bush0 | Bush1 => LegoBush,
        Rock0 | Rock1 | Rock2 | Boulder => LegoRock, GrassTuft => LegoGrass,
        FlowerClump => LegoFlowers, Stump => LegoStump, Crate => LegoCrate,
        Barrel => LegoBarrel, HayBale => LegoHay,
        _ => id,
    }
}

/// Used by renderers to retain the original mesh's shadow policy.
pub fn original_mesh(id: MeshId) -> MeshId {
    use MeshId::*;
    match id {
        LegoTorso => CharTorso, LegoTrim => CharTrim, LegoPelvis => CharPelvis,
        LegoHead => CharHead, LegoArmUp => CharArmUp, LegoArmLow => CharArmLow,
        LegoHand => CharHand, LegoLegUp => CharLegUp, LegoLegLow => CharLegLow,
        LegoBoot => CharBoot, LegoBackpack => CharBackpack, LegoHair => Hair1, LegoHat => Cap,
        LegoPistol => WpnPistol, LegoSmg => WpnSmg, LegoAr => WpnAr,
        LegoShotgun => WpnShotgun, LegoSniper => WpnSniper, LegoRocket => WpnRocket,
        LegoPickaxe => Pickaxe, LegoAmmo => AmmoBox, LegoBandage => ItemBandage,
        LegoMedkit => ItemMedkit, LegoShieldMini => ItemShieldMini,
        LegoShieldBig => ItemShieldBig, LegoChug => ItemChug,
        LegoChestBase => ChestBase, LegoChestLid => ChestLid,
        LegoWall => PieceWall, LegoFloor => PieceFloor, LegoRoof => PieceRoof, LegoRamp => PieceRamp,
        LegoPine => Pine0, LegoOak => Oak0, LegoBirch => Birch0, LegoPalm => Palm0,
        LegoBush => Bush0, LegoRock => Rock0, LegoGrass => GrassTuft,
        LegoFlowers => FlowerClump, LegoStump => Stump, LegoCrate => Crate,
        LegoBarrel => Barrel, LegoHay => HayBale,
        _ => id,
    }
}

fn plastic() -> MeshBuilder {
    let mut b = MeshBuilder::new();
    b.mat(mat::FLAT).color(Vec3::ONE).spec(0.32).ao(0.78, 1.0);
    b
}
fn stud(b: &mut MeshBuilder, at: Vec3, radius: f32, height: f32) {
    b.cylinder(at, radius, radius, height, 8, false, true);
}
fn brick(b: &mut MeshBuilder, at: Vec3, half: Vec3, nx: usize, nz: usize) {
    b.box_center(at, half);
    for x in 0..nx {
        for z in 0..nz {
            let px = at.x + (2.0 * (x as f32 + 0.5) / nx as f32 - 1.0) * half.x;
            let pz = at.z + (2.0 * (z as f32 + 0.5) / nz as f32 - 1.0) * half.z;
            let r = (half.x / nx as f32).min(half.z / nz as f32) * 0.52;
            stud(b, Vec3::new(px, at.y + half.y, pz), r, r * 0.44);
        }
    }
}
fn bar(b: &mut MeshBuilder, at: Vec3, length: f32, radius: f32) {
    b.push_xf(Mat4::from_translation(at) * Mat4::from_rotation_x(-FRAC_PI_2));
    b.cylinder(Vec3::ZERO, radius, radius, length, 8, true, true);
    b.pop_xf();
}
fn limb(length: f32, radius: f32) -> MeshData {
    let mut b = plastic();
    b.box_min_max(Vec3::new(-radius, -length, -radius), Vec3::new(radius, 0.0, radius));
    b.color(Vec3::splat(0.65));
    bar(&mut b, Vec3::new(0.0, 0.0, radius + 0.008), radius * 2.0 + 0.016, radius * 0.74);
    b.finish()
}
fn weapon(length: f32, style: u8) -> MeshData {
    let mut b = plastic();
    b.color(Vec3::splat(0.9));
    // The original right-hand grip and muzzle stay in exactly the same positions.
    brick(&mut b, Vec3::new(0.0, 0.025, -length * 0.28), Vec3::new(0.055, 0.055, length * 0.24), 1, 3);
    b.tinted(false).hex(0xff972b);
    b.box_center(Vec3::new(0.0, -0.075, 0.0), Vec3::new(0.035, 0.08, 0.035));
    bar(&mut b, Vec3::new(0.0, 0.035, -length * 0.48), length * 0.52, if style == 5 { 0.095 } else { 0.035 });
    b.hex(0x1c344c);
    b.box_center(Vec3::new(0.0, 0.025, 0.13), Vec3::new(0.045, 0.045, if style == 0 { 0.04 } else { 0.12 }));
    if style == 4 {
        b.hex(0x56e0eb);
        bar(&mut b, Vec3::new(0.0, 0.14, -0.20), 0.18, 0.04);
    }
    if style == 2 || style == 1 {
        b.hex(0x304d60);
        brick(&mut b, Vec3::new(0.0, -0.11, -0.16), Vec3::new(0.045, 0.085, 0.055), 1, 1);
    }
    b.finish()
}
fn bottle(size: f32) -> MeshData {
    let mut b = plastic();
    b.tinted(false).hex(0x48d5f0).spec(0.5);
    brick(&mut b, Vec3::new(0.0, size * 0.55, 0.0), Vec3::new(size * 0.33, size * 0.45, size * 0.33), 2, 2);
    b.hex(0xdee8f1);
    stud(&mut b, Vec3::new(0.0, size, 0.0), size * 0.23, size * 0.12);
    b.hex(0x1684bf);
    b.box_center(Vec3::new(0.0, size * 0.55, -size * 0.34), Vec3::new(size * 0.18, size * 0.20, 0.008));
    b.finish()
}
fn tree(style: u8) -> MeshData {
    let mut b = plastic();
    b.tinted(false).hex(if style == 2 { 0xe7e6d7 } else { 0x785037 });
    brick(&mut b, Vec3::new(0.0, 1.7, 0.0), Vec3::new(0.30, 1.7, 0.30), 1, 1);
    b.tinted(true).color(Vec3::ONE).spec(0.25);
    if style == 0 {
        for k in 0..5 {
            let r = 2.45 - k as f32 * 0.38;
            brick(&mut b, Vec3::new(0.0, 3.2 + k as f32 * 1.35, 0.0), Vec3::new(r, 0.57, r), 3, 3);
        }
    } else if style == 3 {
        for k in 0..4 {
            b.push_xf(Mat4::from_rotation_y(k as f32 * FRAC_PI_2));
            brick(&mut b, Vec3::new(1.6, 6.3, 0.0), Vec3::new(2.1, 0.22, 0.65), 5, 2);
            b.pop_xf();
        }
    } else {
        for (at, half) in [
            (Vec3::new(0.0, 5.4, 0.0), Vec3::new(2.7, 1.4, 2.5)),
            (Vec3::new(-1.7, 4.6, 0.7), Vec3::new(1.6, 0.8, 1.6)),
            (Vec3::new(1.8, 4.8, -0.5), Vec3::new(1.5, 0.9, 1.5)),
        ] { brick(&mut b, at, half, 3, 3); }
    }
    b.finish()
}
fn piece(kind: u8) -> MeshData {
    let mut b = plastic();
    let seam = 0.025;
    if kind == 0 {
        let nx = 8;
        let ny = 6;
        let w = TILE / nx as f32;
        let h = LEVEL_H / ny as f32;
        for y in 0..ny {
            for x in 0..nx {
                b.color(Vec3::splat(if (x + y) % 2 == 0 { 1.0 } else { 0.9 }));
                b.box_min_max(Vec3::new(x as f32 * w + seam, y as f32 * h + seam, -THICK * 0.5),
                    Vec3::new((x + 1) as f32 * w - seam, (y + 1) as f32 * h - seam, THICK * 0.5));
            }
        }
        b.color(Vec3::ONE);
        for x in 0..nx {
            stud(&mut b, Vec3::new((x as f32 + 0.5) * w, LEVEL_H, 0.0), THICK * 0.34, 0.07);
        }
    } else if kind == 3 {
        // Keep a smooth walking surface matching the solid ramp collider.
        b.wedge(Vec3::ZERO, Vec3::new(TILE, LEVEL_H, TILE), 0);
        let slope = LEVEL_H / TILE;
        for x in 0..8 {
            for z in 0..8 {
                let px = (x as f32 + 0.5) * TILE / 8.0;
                let pz = (z as f32 + 0.5) * TILE / 8.0;
                stud(&mut b, Vec3::new(px, px * slope, pz), 0.14, 0.055);
            }
        }
    } else {
        let y = if kind == 2 { LEVEL_H } else { 0.0 };
        brick(&mut b, Vec3::new(TILE * 0.5, y - THICK * 0.5, TILE * 0.5),
            Vec3::new(TILE * 0.5, THICK * 0.5, TILE * 0.5), 8, 8);
    }
    b.finish()
}

/// All theme geometry is original, generated locally with no external service or API key.
pub fn mesh(id: MeshId) -> Option<MeshData> {
    use MeshId::*;
    let mut b = plastic();
    match id {
        LegoTorso => {
            b.box_min_max(Vec3::new(-0.23, 0.0, -0.135), Vec3::new(0.23, 0.50, 0.135));
            b.tinted(false).hex(0xffc94a);
            stud(&mut b, Vec3::new(0.0, 0.50, 0.0), 0.067, 0.08);
            b.hex(0x254d58);
            b.box_center(Vec3::new(0.0, 0.27, -0.142), Vec3::new(0.065, 0.085, 0.008));
        }
        LegoTrim => {
            for s in [-1.0, 1.0] {
                brick(&mut b, Vec3::new(s * SHOULDER_X, SHOULDER_Y, 0.0), Vec3::new(0.065, 0.04, 0.14), 1, 2);
            }
            b.box_center(Vec3::new(0.0, 0.10, -0.147), Vec3::new(0.23, 0.027, 0.018));
        }
        LegoPelvis => {
            brick(&mut b, Vec3::new(0.0, -0.04, 0.0), Vec3::new(0.20, 0.075, 0.12), 2, 1);
            b.tinted(false).hex(0x23333e);
            b.box_center(Vec3::new(0.0, 0.02, -0.13), Vec3::new(0.20, 0.023, 0.02));
        }
        LegoHead => {
            b.tinted(false).hex(0xffc94a);
            b.cylinder(Vec3::new(0.0, -0.005, 0.0), 0.147, 0.147, 0.27, 16, true, true);
            stud(&mut b, Vec3::new(0.0, 0.265, 0.0), 0.065, 0.045);
            b.hex(0x263447).spec(0.1);
            for s in [-1.0, 1.0] {
                b.box_center(Vec3::new(s * 0.052, 0.16, -0.142), Vec3::new(0.018, 0.027, 0.007));
                b.box_center(Vec3::new(s * 0.052, 0.206, -0.134), Vec3::new(0.025, 0.007, 0.007));
            }
            b.box_center(Vec3::new(0.0, 0.069, -0.148), Vec3::new(0.037, 0.009, 0.005));
            for s in [-1.0, 1.0] {
                b.box_center(Vec3::new(s * 0.043, 0.078, -0.143), Vec3::new(0.010, 0.014, 0.006));
            }
        }
        LegoArmUp => return Some(limb(UPPER_ARM, 0.066)),
        LegoArmLow => return Some(limb(FOREARM, 0.057)),
        LegoLegUp => return Some(limb(THIGH, 0.085)),
        LegoLegLow => return Some(limb(SHIN, 0.074)),
        LegoHand => {
            b.tinted(false).hex(0xffc94a);
            b.box_center(Vec3::new(0.0, -0.05, 0.018), Vec3::new(0.05, 0.055, 0.018));
            for s in [-1.0, 1.0] {
                b.box_center(Vec3::new(s * 0.042, -0.05, -0.02), Vec3::new(0.016, 0.055, 0.036));
            }
        }
        LegoBoot => {
            brick(&mut b, Vec3::new(0.0, -0.026, -0.055), Vec3::new(0.082, 0.062, 0.14), 1, 2);
            b.color(Vec3::splat(0.35));
            b.box_center(Vec3::new(0.0, -0.085, -0.055), Vec3::new(0.084, 0.010, 0.142));
        }
        LegoBackpack => {
            brick(&mut b, Vec3::new(0.0, 0.30, 0.205), Vec3::new(0.15, 0.185, 0.075), 2, 1);
            b.color(Vec3::splat(0.45));
            b.box_center(Vec3::new(0.0, 0.36, 0.287), Vec3::new(0.10, 0.074, 0.012));
        }
        LegoHair | LegoHat => {
            brick(&mut b, Vec3::new(0.0, 0.27, 0.008), Vec3::new(0.16, 0.045, 0.16), 2, 2);
            b.box_center(Vec3::new(0.0, 0.17, 0.13), Vec3::new(0.15, 0.09, 0.045));
            if id == LegoHat {
                b.box_center(Vec3::new(0.0, 0.23, -0.20), Vec3::new(0.15, 0.02, 0.06));
            }
        }
        LegoPistol => return Some(weapon(0.24, 0)),
        LegoSmg => return Some(weapon(0.51, 1)),
        LegoAr => return Some(weapon(0.78, 2)),
        LegoShotgun => return Some(weapon(0.81, 3)),
        LegoSniper => return Some(weapon(1.07, 4)),
        LegoRocket => return Some(weapon(0.79, 5)),
        LegoPickaxe => {
            b.tinted(false).hex(0x35bdd0);
            brick(&mut b, Vec3::new(0.0, 0.06, 0.0), Vec3::new(0.034, 0.38, 0.034), 1, 1);
            b.hex(0xffb541);
            brick(&mut b, Vec3::new(0.0, 0.43, 0.0), Vec3::new(0.27, 0.04, 0.065), 4, 1);
        }
        LegoAmmo | LegoBandage | LegoMedkit => {
            let half = if id == LegoMedkit { Vec3::new(0.25, 0.13, 0.16) } else { Vec3::new(0.15, 0.10, 0.10) };
            b.tinted(false).hex(if id == LegoAmmo { 0xffb941 } else { 0xf0f0e6 });
            brick(&mut b, Vec3::Y * half.y, half, 3, 2);
            if id != LegoAmmo {
                b.hex(0xe64555);
                b.box_center(Vec3::new(0.0, half.y, -half.z - 0.008), Vec3::new(half.x * 0.5, 0.025, 0.009));
                b.box_center(Vec3::new(0.0, half.y, -half.z - 0.009), Vec3::new(0.025, half.y * 0.65, 0.010));
            }
        }
        LegoShieldMini => return Some(bottle(0.30)),
        LegoShieldBig => return Some(bottle(0.45)),
        LegoChug => return Some(bottle(0.55)),
        LegoChestBase => {
            b.tinted(false).hex(0xffa52c);
            brick(&mut b, Vec3::new(0.0, 0.25, 0.0), Vec3::new(0.42, 0.25, 0.27), 4, 2);
            b.hex(0x29415b);
            for s in [-1.0, 1.0] {
                b.box_center(Vec3::new(s * 0.29, 0.25, -0.282), Vec3::new(0.032, 0.24, 0.014));
            }
            b.hex(0xffef77);
            b.box_center(Vec3::new(0.0, 0.35, -0.29), Vec3::new(0.06, 0.075, 0.02));
        }
        LegoChestLid => {
            b.tinted(false).hex(0xffbc39);
            // Chest lid origin follows the renderer's back-edge hinge (local Z is 0..0.6).
            brick(&mut b, Vec3::new(0.0, 0.045, 0.30), Vec3::new(0.43, 0.08, 0.30), 4, 2);
        }
        LegoWall => return Some(piece(0)), LegoFloor => return Some(piece(1)),
        LegoRoof => return Some(piece(2)), LegoRamp => return Some(piece(3)),
        LegoPine => return Some(tree(0)), LegoOak => return Some(tree(1)),
        LegoBirch => return Some(tree(2)), LegoPalm => return Some(tree(3)),
        LegoBush => {
            brick(&mut b, Vec3::new(0.0, 0.7, 0.0), Vec3::new(1.15, 0.7, 1.1), 3, 3);
            brick(&mut b, Vec3::new(0.3, 1.6, 0.1), Vec3::new(0.70, 0.25, 0.65), 2, 2);
        }
        LegoRock => {
            b.tinted(false).hex(0x9196a7);
            brick(&mut b, Vec3::new(0.0, 0.45, 0.0), Vec3::new(0.85, 0.45, 0.75), 3, 3);
            b.hex(0xb5bbc8);
            brick(&mut b, Vec3::new(0.10, 1.0, -0.08), Vec3::new(0.56, 0.15, 0.51), 2, 2);
        }
        LegoGrass | LegoFlowers => {
            for k in 0..3 {
                b.push_xf(Mat4::from_rotation_y(k as f32 * 1.05));
                b.box_center(Vec3::new(0.11, 0.20, 0.0), Vec3::new(0.035, 0.20, 0.12));
                if id == LegoFlowers {
                    b.tinted(false).hex(if k % 2 == 0 { 0xffd346 } else { 0xf06f9c });
                    brick(&mut b, Vec3::new(0.11, 0.43, 0.0), Vec3::new(0.10, 0.045, 0.10), 1, 1);
                    b.tinted(true).color(Vec3::ONE);
                }
                b.pop_xf();
            }
        }
        LegoStump => {
            b.tinted(false).hex(0x885b39);
            brick(&mut b, Vec3::new(0.0, 0.40, 0.0), Vec3::new(0.48, 0.4, 0.48), 2, 2);
        }
        LegoCrate | LegoBarrel | LegoHay => {
            b.tinted(false).hex(match id { LegoBarrel => 0x408ab4, LegoHay => 0xe4b557, _ => 0xbc7a3d });
            brick(&mut b, Vec3::new(0.0, 0.55, 0.0), Vec3::new(0.5, 0.55, 0.5), 3, 3);
            b.hex(0x344455);
            b.box_center(Vec3::new(0.0, 0.6, -0.512), Vec3::new(0.50, 0.032, 0.012));
        }
        _ => return None,
    }
    Some(b.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn themes_preserve_classic_meshes_and_replace_brick_characters() {
        assert_eq!(mapped_mesh(MeshId::CharTorso, GameMode::BattleRoyale), MeshId::CharTorso);
        assert_eq!(mapped_mesh(MeshId::CharTorso, GameMode::ZeroBuild), MeshId::CharTorso);
        assert_eq!(mapped_mesh(MeshId::CharTorso, GameMode::Lego), MeshId::LegoTorso);
        assert_eq!(original_mesh(MeshId::LegoWall), MeshId::PieceWall);
    }
    #[test]
    fn brick_rig_and_weapons_keep_gameplay_pivots() {
        let sole = mesh(MeshId::LegoBoot).unwrap().bounds().min.y;
        assert!(sole >= -0.10 && sole <= -0.075);
        assert!(mesh(MeshId::LegoArmUp).unwrap().bounds().min.y <= -UPPER_ARM);
        assert!((mesh(MeshId::LegoHead).unwrap().bounds().max.y - 0.31).abs() < 0.001);
        for (id, length) in [(MeshId::LegoPistol, 0.24), (MeshId::LegoAr, 0.78), (MeshId::LegoSniper, 1.07)] {
            assert!((mesh(id).unwrap().bounds().min.z + length).abs() < 0.001);
        }
    }
}

