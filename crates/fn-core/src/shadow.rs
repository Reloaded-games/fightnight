//! Cascaded shadow map matrices. Each cascade is a texel-snapped orthographic
//! projection fitted to a bounding sphere of its camera-frustum slice, so shadows
//! stay stable while the camera moves and rotates.

use crate::camera::Camera;
use crate::math::{Mat4, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct Cascade {
    pub vp: Mat4,
    pub center: Vec3,
    pub radius: f32,
}

pub const SPLITS: [f32; 3] = [16.0, 56.0, 170.0];
const MARGINS: [f32; 3] = [70.0, 110.0, 190.0];

#[allow(deprecated)]
pub fn compute_cascades(cam: &Camera, sun_dir: Vec3, shadow_size: u32) -> [Cascade; 3] {
    let mut out = [Cascade { vp: Mat4::IDENTITY, center: Vec3::ZERO, radius: 1.0 }; 3];
    for i in 0..3 {
        let n = if i == 0 { cam.near } else { SPLITS[i - 1] * 0.92 };
        let f = SPLITS[i];
        let corners = cam.slice_corners(n, f);
        let mut center = Vec3::ZERO;
        for c in &corners {
            center += *c;
        }
        center /= 8.0;
        let mut radius = 0.0f32;
        for c in &corners {
            radius = radius.max((*c - center).length());
        }
        radius = (radius * 16.0).ceil() / 16.0;
        let margin = MARGINS[i];
        let up = if sun_dir.y.abs() > 0.97 { Vec3::Z } else { Vec3::Y };
        let eye = center + sun_dir * (radius + margin);
        let view = Mat4::look_at_rh(eye, center, up);
        let texel = 2.0 * radius / shadow_size as f32;
        let c_ls = view.transform_point3(center);
        let sx = (c_ls.x / texel).round() * texel;
        let sy = (c_ls.y / texel).round() * texel;
        let proj = Mat4::orthographic_rh(sx - radius, sx + radius, sy - radius, sy + radius, 0.0, margin + 2.0 * radius + 10.0);
        out[i] = Cascade { vp: proj * view, center, radius };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_contains_slice_corners() {
        let cam = Camera::from_yaw_pitch(Vec3::new(10.0, 5.0, 20.0), 0.7, -0.2, 1.0, 16.0 / 9.0, 0.1);
        let sun = Vec3::new(0.5, 0.7, 0.4).normalize();
        let cs = compute_cascades(&cam, sun, 2048);
        for (i, c) in cs.iter().enumerate() {
            let n = if i == 0 { cam.near } else { SPLITS[i - 1] * 0.92 };
            for p in cam.slice_corners(n, SPLITS[i]) {
                let q = c.vp * p.extend(1.0);
                let ndc = q.truncate() / q.w;
                assert!(ndc.x.abs() <= 1.01 && ndc.y.abs() <= 1.01 && ndc.z >= -0.01 && ndc.z <= 1.01, "cascade {i} corner outside: {ndc:?}");
            }
        }
    }

    #[test]
    fn cascades_are_texel_stable_under_small_motion() {
        let sun = Vec3::new(0.5, 0.7, 0.4).normalize();
        let a = Camera::from_yaw_pitch(Vec3::new(0.0, 5.0, 0.0), 0.0, -0.2, 1.0, 1.7, 0.1);
        let b = Camera::from_yaw_pitch(Vec3::new(0.001, 5.0, 0.0), 0.0, -0.2, 1.0, 1.7, 0.1);
        let ca = compute_cascades(&a, sun, 2048);
        let cb = compute_cascades(&b, sun, 2048);
        // Tiny motion must not change the projection (snapped to texels).
        assert!((ca[0].vp.to_cols_array()[12] - cb[0].vp.to_cols_array()[12]).abs() < 1e-3);
    }
}
