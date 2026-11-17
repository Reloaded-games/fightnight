//! Validated runtime meshes from the local extraction library.
use crate::mesh::{MeshData, Vertex};

/// Decode the same packed FNM1 format as authored meshes, rejecting malformed input.
pub fn decode(bytes: &[u8]) -> Result<MeshData, &'static str> {
    if bytes.len() < 12 || &bytes[..4] != b"FNM1" { return Err("Invalid mesh header"); }
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let vertices = word(4) as usize;
    let indices = word(8) as usize;
    if vertices == 0 || vertices > 200_000 || indices == 0 || indices > 900_000 || indices % 3 != 0 {
        return Err("Mesh exceeds runtime limits or has no triangles");
    }
    if bytes.len() != 12 + vertices * 32 + indices * 4 { return Err("Truncated mesh"); }
    let mut mesh = MeshData::default();
    for offset in (12..12 + vertices * 32).step_by(32) {
        let float = |i| f32::from_bits(word(offset + i));
        let pos = [float(0), float(4), float(8)];
        let nrm = [float(12), float(16), float(20)];
        if pos.iter().any(|v| !v.is_finite() || v.abs() > 100.0) || nrm.iter().any(|v| !v.is_finite()) {
            return Err("Invalid vertex");
        }
        mesh.verts.push(Vertex { pos, nrm, col: bytes[offset + 24..offset + 28].try_into().unwrap(), attr: bytes[offset + 28..offset + 32].try_into().unwrap() });
    }
    for offset in (12 + vertices * 32..bytes.len()).step_by(4) {
        let index = word(offset);
        if index as usize >= vertices { return Err("Index outside mesh"); }
        mesh.idx.push(index);
    }
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &[u8] = include_bytes!("../../../assets/cartoon/meshes/WpnAr.fnmesh");

    #[test]
    fn imports_a_real_authored_mesh() {
        let mesh = decode(SAMPLE).unwrap();
        assert!(!mesh.idx.is_empty());
        assert!(mesh.bounds().min.is_finite());
    }

    #[test]
    fn refuses_truncation_nonfinite_vertices_and_invalid_indices() {
        for length in [0, 4, 11, SAMPLE.len() - 1] { assert!(decode(&SAMPLE[..length]).is_err()); }
        let mut bytes = SAMPLE.to_vec();
        bytes[12..16].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode(&bytes).is_err());
        let mut bytes = SAMPLE.to_vec();
        let last = bytes.len() - 4;
        bytes[last..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn refuses_oversized_counts_before_allocation() {
        let mut bytes = SAMPLE.to_vec();
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&bytes).is_err());
    }
}
