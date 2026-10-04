//! Small math helpers on top of `glam`, plus a few geometric query primitives
//! (AABB, ray tests) shared by world generation, collision and AI.

pub use glam::{IVec2, IVec3, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
use std::f32::consts::{PI, TAU};

pub const DEG: f32 = PI / 180.0;

#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
#[inline]
pub fn saturate(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}
#[inline]
pub fn inv_lerp(a: f32, b: f32, v: f32) -> f32 {
    if (b - a).abs() < 1e-8 {
        0.0
    } else {
        (v - a) / (b - a)
    }
}
#[inline]
pub fn remap(v: f32, a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    lerp(b0, b1, inv_lerp(a0, a1, v))
}
/// Hermite smoothstep that also works with reversed edges (e0 > e1).
#[inline]
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = saturate(inv_lerp(e0, e1, x));
    t * t * (3.0 - 2.0 * t)
}
#[inline]
pub fn smootherstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = saturate(inv_lerp(e0, e1, x));
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}
/// Wrap an angle to (-PI, PI].
#[inline]
pub fn wrap_pi(a: f32) -> f32 {
    let mut a = a % TAU;
    if a > PI {
        a -= TAU;
    } else if a <= -PI {
        a += TAU;
    }
    a
}
#[inline]
pub fn angle_diff(from: f32, to: f32) -> f32 {
    wrap_pi(to - from)
}
/// Move `cur` toward `target` by at most `max_delta`.
#[inline]
pub fn approach(cur: f32, target: f32, max_delta: f32) -> f32 {
    if (target - cur).abs() <= max_delta {
        target
    } else {
        cur + (target - cur).signum() * max_delta
    }
}
/// Frame-rate independent exponential smoothing factor.
#[inline]
pub fn damp(rate: f32, dt: f32) -> f32 {
    1.0 - (-rate * dt).exp()
}
#[inline]
pub fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    a + angle_diff(a, b) * t
}
#[inline]
pub fn vlerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a + (b - a) * t
}

/// Forward direction on the ground plane for a yaw angle. Yaw 0 faces -Z.
#[inline]
pub fn yaw_forward(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}
#[inline]
pub fn yaw_right(yaw: f32) -> Vec3 {
    Vec3::new(yaw.cos(), 0.0, -yaw.sin())
}
/// Full look direction from yaw (around Y) and pitch (positive = looking up).
#[inline]
pub fn look_dir(yaw: f32, pitch: f32) -> Vec3 {
    let cp = pitch.cos();
    Vec3::new(-yaw.sin() * cp, pitch.sin(), -yaw.cos() * cp)
}
/// Yaw angle that makes `yaw_forward` point along the (x, z) direction.
#[inline]
pub fn yaw_of(dir_xz: Vec2) -> f32 {
    (-dir_xz.x).atan2(-dir_xz.y)
}

#[inline]
pub fn xz(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}
#[inline]
pub fn v3(xz: Vec2, y: f32) -> Vec3 {
    Vec3::new(xz.x, y, xz.y)
}

/// Axis aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }
    pub fn from_center_half(c: Vec3, h: Vec3) -> Self {
        Self { min: c - h, max: c + h }
    }
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    pub fn half(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }
    pub fn extend(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }
    pub fn union(&self, o: &Aabb) -> Aabb {
        Aabb { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    pub fn expanded(&self, m: f32) -> Aabb {
        Aabb { min: self.min - Vec3::splat(m), max: self.max + Vec3::splat(m) }
    }
    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }
    pub fn contains_xz(&self, x: f32, z: f32) -> bool {
        x >= self.min.x && x <= self.max.x && z >= self.min.z && z <= self.max.z
    }
    pub fn intersects(&self, o: &Aabb) -> bool {
        self.min.cmple(o.max).all() && self.max.cmpge(o.min).all()
    }
    /// Ray vs box slab test. Returns (t_near, t_far) if the ray overlaps the box
    /// within [0, max_t].
    pub fn ray(&self, o: Vec3, inv_d: Vec3, max_t: f32) -> Option<(f32, f32)> {
        let t1 = (self.min - o) * inv_d;
        let t2 = (self.max - o) * inv_d;
        let tmin = t1.min(t2);
        let tmax = t1.max(t2);
        let near = tmin.x.max(tmin.y).max(tmin.z).max(0.0);
        let far = tmax.x.min(tmax.y).min(tmax.z).min(max_t);
        if near <= far {
            Some((near, far))
        } else {
            None
        }
    }
    /// Outward normal of the face hit at parameter `t` along the ray.
    pub fn hit_normal(&self, p: Vec3) -> Vec3 {
        let c = self.center();
        let h = self.half().max(Vec3::splat(1e-5));
        let d = (p - c) / h;
        let a = d.abs();
        if a.x >= a.y && a.x >= a.z {
            Vec3::new(d.x.signum(), 0.0, 0.0)
        } else if a.y >= a.z {
            Vec3::new(0.0, d.y.signum(), 0.0)
        } else {
            Vec3::new(0.0, 0.0, d.z.signum())
        }
    }
}

/// Ray vs sphere; returns nearest non-negative t.
pub fn ray_sphere(o: Vec3, d: Vec3, c: Vec3, r: f32) -> Option<f32> {
    let oc = o - c;
    let b = oc.dot(d);
    let cc = oc.dot(oc) - r * r;
    let disc = b * b - cc;
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let t0 = -b - s;
    if t0 >= 0.0 {
        return Some(t0);
    }
    let t1 = -b + s;
    if t1 >= 0.0 {
        Some(0.0)
    } else {
        None
    }
}

/// Ray vs vertical capsule (cylinder with spherical caps) whose axis spans
/// from `base` to `top` (both on the same vertical line).
pub fn ray_capsule_y(o: Vec3, d: Vec3, base: Vec3, top: Vec3, r: f32) -> Option<f32> {
    // Work in XZ for the infinite cylinder, then clamp to caps with spheres.
    let mut best: Option<f32> = None;
    let mut consider = |t: f32| {
        if t >= 0.0 && best.map_or(true, |b| t < b) {
            best = Some(t);
        }
    };
    let dx = d.x;
    let dz = d.z;
    let ox = o.x - base.x;
    let oz = o.z - base.z;
    let a = dx * dx + dz * dz;
    if a > 1e-8 {
        let b = ox * dx + oz * dz;
        let c = ox * ox + oz * oz - r * r;
        let disc = b * b - a * c;
        if disc >= 0.0 {
            let s = disc.sqrt();
            for t in [(-b - s) / a, (-b + s) / a] {
                if t >= 0.0 {
                    let y = o.y + d.y * t;
                    if y >= base.y && y <= top.y {
                        consider(t);
                    }
                }
            }
        }
    } else {
        // Ray parallel to the axis: only the caps can be hit.
        let c = ox * ox + oz * oz - r * r;
        if c <= 0.0 && o.y >= base.y && o.y <= top.y {
            consider(0.0);
        }
    }
    if let Some(t) = ray_sphere(o, d, base, r) {
        consider(t);
    }
    if let Some(t) = ray_sphere(o, d, top, r) {
        consider(t);
    }
    best
}

/// Moller-Trumbore ray/triangle intersection. Returns t (>= 0) of the hit.
pub fn ray_tri(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let tv = o - a;
    let u = tv.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = tv.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    if t >= 0.0 {
        Some(t)
    } else {
        None
    }
}

/// Closest distance from a point to a 2D segment, also returning the
/// parameter along the segment in [0,1].
pub fn point_segment_dist(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 < 1e-8 { 0.0 } else { saturate((p - a).dot(ab) / l2) };
    ((p - (a + ab * t)).length(), t)
}

/// Convert HSV (all 0..1) to RGB (0..1).
pub fn hsv(h: f32, s: f32, v: f32) -> Vec3 {
    let h = (h - h.floor()) * 6.0;
    let i = h.floor();
    let f = h - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i as i32 {
        0 => Vec3::new(v, t, p),
        1 => Vec3::new(q, v, p),
        2 => Vec3::new(p, v, t),
        3 => Vec3::new(p, q, v),
        4 => Vec3::new(t, p, v),
        _ => Vec3::new(v, p, q),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_roundtrip() {
        for i in 0..32 {
            let yaw = -3.1 + i as f32 * 0.2;
            let f = yaw_forward(yaw);
            let back = yaw_of(Vec2::new(f.x, f.z));
            assert!(angle_diff(yaw, back).abs() < 1e-4, "{yaw} {back}");
            let r = yaw_right(yaw);
            assert!(f.dot(r).abs() < 1e-5);
        }
        // Yaw 0 faces -Z; positive yaw turns toward -X.
        assert!((yaw_forward(0.0) - Vec3::NEG_Z).length() < 1e-6);
    }

    #[test]
    fn smoothstep_reversed_edges() {
        assert!((smoothstep(10.0, 0.0, 0.0) - 1.0).abs() < 1e-6);
        assert!(smoothstep(10.0, 0.0, 10.0).abs() < 1e-6);
        assert!((smoothstep(0.0, 10.0, 5.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn ray_box_hits() {
        let b = Aabb::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        let o = Vec3::new(0.0, 0.0, -5.0);
        let d = Vec3::Z;
        let (n, f) = b.ray(o, d.recip(), 100.0).unwrap();
        assert!((n - 4.0).abs() < 1e-4 && (f - 6.0).abs() < 1e-4);
        assert!(b.ray(Vec3::new(5.0, 0.0, -5.0), d.recip(), 100.0).is_none());
        assert_eq!(b.hit_normal(Vec3::new(0.0, 0.0, -1.0)), Vec3::NEG_Z);
    }

    #[test]
    fn ray_capsule_hits_body_and_caps() {
        let base = Vec3::new(0.0, 0.0, 0.0);
        let top = Vec3::new(0.0, 1.0, 0.0);
        let t = ray_capsule_y(Vec3::new(0.0, 0.5, -5.0), Vec3::Z, base, top, 0.4).unwrap();
        assert!((t - 4.6).abs() < 1e-3);
        let t = ray_capsule_y(Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y, base, top, 0.4).unwrap();
        assert!((t - 3.6).abs() < 1e-3);
        assert!(ray_capsule_y(Vec3::new(2.0, 0.5, -5.0), Vec3::Z, base, top, 0.4).is_none());
    }

    #[test]
    fn ray_tri_basic() {
        let t = ray_tri(
            Vec3::new(0.2, 0.2, 5.0),
            Vec3::NEG_Z,
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
        )
        .unwrap();
        assert!((t - 5.0).abs() < 1e-5);
    }
}
