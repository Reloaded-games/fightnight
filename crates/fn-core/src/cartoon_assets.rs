//! Blender-authored meshes baked to the renderer's vertex format. Embedded in the
//! native executable and WASM so gameplay does not depend on asset network calls.
use crate::mesh::{MeshData, Vertex};
use crate::meshlib::MeshId;

macro_rules! asset {
    ($name:literal) => {
        include_bytes!(concat!("../../../assets/cartoon/meshes/", $name, ".fnmesh")) as &[u8]
    };
}

pub fn mesh(id: MeshId) -> Option<MeshData> {
    use MeshId::*;
    let bytes = match id {
        CharTorso => asset!("CharTorso"),
        CharTrim => asset!("CharTrim"),
        CharPelvis => asset!("CharPelvis"),
        CharHead => asset!("CharHead"),
        Hair1 => asset!("Hair1"),
        Hair2 => asset!("Hair2"),
        Hair3 => asset!("Hair3"),
        CharArmUp => asset!("CharArmUp"),
        CharArmLow => asset!("CharArmLow"),
        CharHand => asset!("CharHand"),
        CharLegUp => asset!("CharLegUp"),
        CharLegLow => asset!("CharLegLow"),
        CharBoot => asset!("CharBoot"),
        CharBackpack => asset!("CharBackpack"),
        WpnPistol => asset!("WpnPistol"),
        WpnSmg => asset!("WpnSmg"),
        WpnAr => asset!("WpnAr"),
        WpnShotgun => asset!("WpnShotgun"),
        WpnSniper => asset!("WpnSniper"),
        WpnRocket => asset!("WpnRocket"),
        Pickaxe => asset!("Pickaxe"),
        AmmoBox => asset!("AmmoBox"),
        ItemShieldMini => asset!("ItemShieldMini"),
        ItemShieldBig => asset!("ItemShieldBig"),
        ItemChug => asset!("ItemChug"),
        ItemMedkit => asset!("ItemMedkit"),
        ItemBandage => asset!("ItemBandage"),
        ChestBase => asset!("ChestBase"),
        ChestLid => asset!("ChestLid"),
        Pine0 => asset!("Pine0"),
        Oak0 => asset!("Oak0"),
        Birch0 => asset!("Birch0"),
        Bush0 => asset!("Bush0"),
        GrassTuft => asset!("GrassTuft"),
        _ => return None,
    };
    Some(decode(bytes))
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn float_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_bits(u32_at(bytes, offset))
}

fn decode(bytes: &[u8]) -> MeshData {
    assert_eq!(&bytes[..4], b"FNM1", "invalid baked mesh version");
    let nv = u32_at(bytes, 4) as usize;
    let ni = u32_at(bytes, 8) as usize;
    assert_eq!(bytes.len(), 12 + nv * 32 + ni * 4);
    let verts = bytes[12..12 + nv * 32]
        .chunks_exact(32)
        .map(|v| Vertex {
            pos: [float_at(v, 0), float_at(v, 4), float_at(v, 8)],
            nrm: [float_at(v, 12), float_at(v, 16), float_at(v, 20)],
            col: v[24..28].try_into().unwrap(),
            attr: v[28..32].try_into().unwrap(),
        })
        .collect();
    let idx = bytes[12 + nv * 32..]
        .chunks_exact(4)
        .map(|v| u32_at(v, 0))
        .collect();
    MeshData { verts, idx }
}

const MOTION: &[u8] = include_bytes!("../../../assets/cartoon/locomotion.fnmotion");

/// Periodic keyframes shared with the Blender walk/sprint clips. The gameplay
/// rig blends these with crouching, jumping and weapon IK; units are metres/radians.
pub fn locomotion(phase: f32) -> [f32; 6] {
    let count = u32_at(MOTION, 4) as usize;
    let frame = phase.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * count as f32;
    let i = frame.floor() as usize % count;
    let j = (i + 1) % count;
    let t = frame.fract();
    std::array::from_fn(|k| {
        let a = float_at(MOTION, 8 + i * 24 + k * 4);
        let b = float_at(MOTION, 8 + j * 24 + k * 4);
        a + (b - a) * t
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;
    use crate::meshlib::ALL_IDS;

    #[test]
    fn baked_assets_are_complete_and_valid_for_gpu_upload() {
        let mut count = 0;
        for &id in ALL_IDS {
            let Some(mesh) = mesh(id) else { continue };
            count += 1;
            assert!(!mesh.is_empty() && mesh.verts.len() < 5000, "{id:?}");
            assert_eq!(mesh.idx.len() % 3, 0);
            assert!(mesh.idx.iter().all(|&i| (i as usize) < mesh.verts.len()));
            for v in &mesh.verts {
                assert!(Vec3::from(v.pos).is_finite(), "{id:?}");
                let n = Vec3::from(v.nrm).length();
                assert!((n - 1.0).abs() < 0.001, "{id:?}: normal {n}");
                assert!(v.attr[1] <= crate::mesh::mat::BUILD);
            }
            for tri in mesh.idx.chunks_exact(3) {
                let [a, b, c] =
                    std::array::from_fn::<_, 3, _>(|i| Vec3::from(mesh.verts[tri[i] as usize].pos));
                assert!(
                    (b - a).cross(c - a).length_squared() > 1e-14,
                    "{id:?}: degenerate triangle"
                );
            }
        }
        assert_eq!(count, 34, "an exported mesh was omitted from the runtime");
    }

    #[test]
    fn motion_is_finite_periodic_and_has_opposed_strides() {
        assert_eq!(&MOTION[..4], b"FNA1");
        assert_eq!(MOTION.len(), 8 + u32_at(MOTION, 4) as usize * 24);
        for i in 0..128 {
            let phase = i as f32 * 0.05;
            let a = locomotion(phase);
            let b = locomotion(phase + std::f32::consts::TAU);
            assert!(a.iter().all(|v| v.is_finite()));
            for k in 0..6 {
                assert!((a[k] - b[k]).abs() < 0.00001);
            }
        }
        assert!(locomotion(std::f32::consts::FRAC_PI_2)[1] > 0.4);
        assert!(locomotion(-std::f32::consts::FRAC_PI_2)[1] < -0.3);
    }
}
