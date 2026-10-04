//! Camera matrices (reverse-Z, infinite far plane) and frustum culling.

use crate::math::*;

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub pos: Vec3,
    pub fwd: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub fov_y: f32,
    pub aspect: f32,
    pub near: f32,
    pub view: Mat4,
    pub proj: Mat4,
    pub view_proj: Mat4,
}

impl Camera {
    /// Build from a position and a look direction. `roll` rotates around the
    /// view axis (used for glider banking).
    #[allow(deprecated)]
    pub fn look(pos: Vec3, fwd: Vec3, roll: f32, fov_y: f32, aspect: f32, near: f32) -> Self {
        let fwd = fwd.normalize();
        let world_up = if fwd.y.abs() > 0.999 { Vec3::Z } else { Vec3::Y };
        let mut right = fwd.cross(world_up).normalize();
        let mut up = right.cross(fwd).normalize();
        if roll != 0.0 {
            let (s, c) = roll.sin_cos();
            let r2 = right * c + up * s;
            let u2 = up * c - right * s;
            right = r2;
            up = u2;
        }
        let view = Mat4::look_to_rh(pos, fwd, up);
        let proj = Mat4::perspective_infinite_reverse_rh(fov_y, aspect, near);
        Self { pos, fwd, right, up, fov_y, aspect, near, view, proj, view_proj: proj * view }
    }

    pub fn from_yaw_pitch(pos: Vec3, yaw: f32, pitch: f32, fov_y: f32, aspect: f32, near: f32) -> Self {
        Self::look(pos, look_dir(yaw, pitch), 0.0, fov_y, aspect, near)
    }

    pub fn tan_half_y(&self) -> f32 {
        (self.fov_y * 0.5).tan()
    }

    /// World-space ray through a point in normalised device coordinates
    /// (x, y in [-1, 1], y up).
    pub fn ray(&self, ndc: Vec2) -> Vec3 {
        let ty = self.tan_half_y();
        (self.fwd + self.right * (ndc.x * ty * self.aspect) + self.up * (ndc.y * ty)).normalize()
    }

    /// Project a world point to NDC; returns None when behind the camera.
    pub fn project(&self, p: Vec3) -> Option<Vec3> {
        let c = self.view_proj * p.extend(1.0);
        if c.w <= 0.0001 {
            return None;
        }
        Some(Vec3::new(c.x / c.w, c.y / c.w, c.z / c.w))
    }

    pub fn frustum(&self) -> Frustum {
        Frustum::from_view_proj(&self.view_proj)
    }

    /// The 8 corners of the frustum slice between view-space depths `n` and `f`.
    pub fn slice_corners(&self, n: f32, f: f32) -> [Vec3; 8] {
        let ty = self.tan_half_y();
        let tx = ty * self.aspect;
        let mut out = [Vec3::ZERO; 8];
        let mut k = 0;
        for d in [n, f] {
            for sy in [-1.0f32, 1.0] {
                for sx in [-1.0f32, 1.0] {
                    out[k] = self.pos + self.fwd * d + self.right * (sx * tx * d) + self.up * (sy * ty * d);
                    k += 1;
                }
            }
        }
        out
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    /// Planes as (normal.xyz, d) with the inside where dot(n, p) + d >= 0.
    /// Left, right, bottom, top, near (the far plane is at infinity).
    pub planes: [Vec4; 5],
}

impl Frustum {
    pub fn from_view_proj(m: &Mat4) -> Self {
        let r0 = m.row(0);
        let r1 = m.row(1);
        let r2 = m.row(2);
        let r3 = m.row(3);
        let norm = |p: Vec4| {
            let l = p.truncate().length().max(1e-8);
            p / l
        };
        Self { planes: [norm(r3 + r0), norm(r3 - r0), norm(r3 + r1), norm(r3 - r1), norm(r2)] }
    }

    pub fn intersects_aabb(&self, b: &Aabb) -> bool {
        for p in &self.planes {
            // positive vertex
            let v = Vec3::new(if p.x >= 0.0 { b.max.x } else { b.min.x }, if p.y >= 0.0 { b.max.y } else { b.min.y }, if p.z >= 0.0 { b.max.z } else { b.min.z });
            if p.x * v.x + p.y * v.y + p.z * v.z + p.w < 0.0 {
                return false;
            }
        }
        true
    }

    pub fn intersects_sphere(&self, c: Vec3, r: f32) -> bool {
        self.planes.iter().all(|p| p.x * c.x + p.y * c.y + p.z * c.z + p.w >= -r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera {
        Camera::from_yaw_pitch(Vec3::new(0.0, 2.0, 0.0), 0.0, 0.0, 60f32.to_radians(), 16.0 / 9.0, 0.1)
    }

    #[test]
    fn project_center_ray_and_back() {
        let c = cam();
        // Looking along -Z: a point straight ahead projects to the screen centre.
        let n = c.project(Vec3::new(0.0, 2.0, -10.0)).unwrap();
        assert!(n.x.abs() < 1e-4 && n.y.abs() < 1e-4);
        assert!(n.z > 0.0 && n.z < 1.0);
        assert!(c.project(Vec3::new(0.0, 2.0, 10.0)).is_none());
        // Ray through the centre equals forward.
        assert!((c.ray(Vec2::ZERO) - c.fwd).length() < 1e-5);
        // Round trip: ray through the projection of a point passes through that point.
        let p = Vec3::new(3.0, 5.0, -12.0);
        let ndc = c.project(p).unwrap();
        let r = c.ray(Vec2::new(ndc.x, ndc.y));
        let t = (p - c.pos).dot(r);
        assert!((c.pos + r * t - p).length() < 1e-3);
    }

    #[test]
    fn frustum_culls_sensibly() {
        let c = cam();
        let f = c.frustum();
        let inside = Aabb::from_center_half(Vec3::new(0.0, 2.0, -20.0), Vec3::splat(1.0));
        let behind = Aabb::from_center_half(Vec3::new(0.0, 2.0, 20.0), Vec3::splat(1.0));
        let side = Aabb::from_center_half(Vec3::new(500.0, 2.0, -20.0), Vec3::splat(1.0));
        let far = Aabb::from_center_half(Vec3::new(0.0, 2.0, -9000.0), Vec3::splat(1.0));
        assert!(f.intersects_aabb(&inside));
        assert!(!f.intersects_aabb(&behind));
        assert!(!f.intersects_aabb(&side));
        assert!(f.intersects_aabb(&far), "infinite far plane");
        assert!(f.intersects_sphere(Vec3::new(0.0, 2.0, -5.0), 1.0));
        assert!(!f.intersects_sphere(Vec3::new(0.0, 2.0, 50.0), 1.0));
    }

    #[test]
    fn slice_corners_bounds() {
        let c = cam();
        let s = c.slice_corners(1.0, 10.0);
        for k in 0..4 {
            assert!(((s[k] - c.pos).dot(c.fwd) - 1.0).abs() < 1e-4);
            assert!(((s[k + 4] - c.pos).dot(c.fwd) - 10.0).abs() < 1e-4);
        }
    }

    #[test]
    fn roll_rotates_up_vector() {
        let c = Camera::look(Vec3::ZERO, Vec3::NEG_Z, std::f32::consts::FRAC_PI_2, 1.0, 1.0, 0.1);
        assert!(c.up.x.abs() > 0.99, "{:?}", c.up);
        assert!(c.fwd.dot(c.up).abs() < 1e-4 && c.fwd.dot(c.right).abs() < 1e-4);
    }
}
