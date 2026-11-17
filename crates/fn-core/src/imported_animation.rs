//! Bounded, pre-baked native animation clips. No decoder or native DLL runs in the browser.
use crate::mesh::MeshData;

/// Validate the entire clip before any renderer state is changed.
pub fn decode(bytes: &[u8]) -> Result<(Vec<MeshData>, f32), &'static str> {
    if bytes.len() < 12 || bytes.len() > 64*1024*1024 || &bytes[..4] != b"FNA1" { return Err("Invalid animation header"); }
    let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    let fps = f32::from_le_bytes(bytes[8..12].try_into().unwrap());
    if !(2..=120).contains(&count) || !fps.is_finite() || fps <= 0.0 || fps > 240.0 { return Err("Invalid animation timing"); }
    let mut frames = Vec::with_capacity(count);
    let mut at = 12;
    let mut vertices = 0;
    let mut topology = None;
    for _ in 0..count {
        if at + 4 > bytes.len() { return Err("Truncated animation"); }
        let length = u32::from_le_bytes(bytes[at..at+4].try_into().unwrap()) as usize;
        at += 4;
        if length > bytes.len()-at { return Err("Truncated animation frame"); }
        let mesh = crate::imported_mesh::decode(&bytes[at..at+length])?;
        vertices += mesh.verts.len();
        if vertices > 1_000_000 { return Err("Animation exceeds vertex budget"); }
        let shape = (mesh.verts.len(), mesh.idx.len());
        if topology.is_some_and(|t| t != shape) { return Err("Animation topology changes"); }
        topology = Some(shape);
        frames.push(mesh);
        at += length;
    }
    if at != bytes.len() { return Err("Trailing animation data"); }
    Ok((frames, fps))
}

/// Loop by simulation time, so pausing the game also pauses imported clips.
pub fn frame_at(elapsed: f32, fps: f32, count: usize) -> usize {
    if count == 0 { return 0; }
    ((elapsed.max(0.0)*fps) as usize)%count
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mesh = include_bytes!("../../../assets/cartoon/meshes/WpnAr.fnmesh");
        let mut bytes = b"FNA1".to_vec();
        bytes.extend(2u32.to_le_bytes()); bytes.extend(30f32.to_le_bytes());
        for _ in 0..2 { bytes.extend((mesh.len() as u32).to_le_bytes()); bytes.extend(mesh); }
        bytes
    }
    #[test]
    fn decodes_complete_frames_and_loops() {
        let (frames, fps) = decode(&fixture()).unwrap();
        assert_eq!(frames.len(), 2); assert_eq!(fps, 30.0);
        assert_eq!(frame_at(0.04, fps, 2), 1); assert_eq!(frame_at(0.08, fps, 2), 0);
        assert_eq!(frame_at(-1.0, fps, 2), 0); assert_eq!(frame_at(1.0, fps, 0), 0);
    }
    #[test]
    fn rejects_partial_and_unbounded_clips() {
        let mut bytes = fixture();
        assert!(decode(&bytes[..bytes.len()-1]).is_err());
        bytes[8..12].copy_from_slice(&f32::NAN.to_le_bytes()); assert!(decode(&bytes).is_err());
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes()); assert!(decode(&bytes).is_err());
        let mut bytes = fixture(); bytes.push(0); assert!(decode(&bytes).is_err());
    }
}
