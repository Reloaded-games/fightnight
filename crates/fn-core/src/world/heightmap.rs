//! Regular-grid heightfield. Height queries evaluate the *same triangulation*
//! the terrain mesh is built from, so characters stand exactly on what is drawn.

use crate::math::*;

#[derive(Clone)]
pub struct Heightmap {
    /// Vertices per side.
    pub n: usize,
    /// Metres between vertices.
    pub cell: f32,
    /// Half the world size in metres (world spans -half..half).
    pub half: f32,
    pub h: Vec<f32>,
}

impl Heightmap {
    pub fn new(n: usize, cell: f32) -> Self {
        let half = (n - 1) as f32 * cell * 0.5;
        Self { n, cell, half, h: vec![0.0; n * n] }
    }

    #[inline]
    pub fn idx(&self, i: usize, j: usize) -> usize {
        j * self.n + i
    }
    #[inline]
    pub fn get(&self, i: usize, j: usize) -> f32 {
        self.h[j * self.n + i]
    }
    #[inline]
    pub fn set(&mut self, i: usize, j: usize, v: f32) {
        self.h[j * self.n + i] = v;
    }
    /// Clamped integer access.
    #[inline]
    pub fn get_c(&self, i: i32, j: i32) -> f32 {
        let m = self.n as i32 - 1;
        self.h[(j.clamp(0, m) as usize) * self.n + i.clamp(0, m) as usize]
    }
    #[inline]
    pub fn world_x(&self, i: usize) -> f32 {
        -self.half + i as f32 * self.cell
    }
    #[inline]
    pub fn contains(&self, x: f32, z: f32) -> bool {
        x > -self.half && x < self.half && z > -self.half && z < self.half
    }

    /// Exact height on the triangulated surface. The cell diagonal runs from
    /// (i, j) to (i+1, j+1). Positions outside the map clamp to the border.
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        let gx = ((x + self.half) / self.cell).clamp(0.0, (self.n - 1) as f32 - 1e-4);
        let gz = ((z + self.half) / self.cell).clamp(0.0, (self.n - 1) as f32 - 1e-4);
        let (i, j) = (gx as usize, gz as usize);
        let (fx, fz) = (gx - i as f32, gz - j as f32);
        let h00 = self.get(i, j);
        let h10 = self.get(i + 1, j);
        let h01 = self.get(i, j + 1);
        let h11 = self.get(i + 1, j + 1);
        if fz >= fx {
            h00 + (h11 - h01) * fx + (h01 - h00) * fz
        } else {
            h00 + (h10 - h00) * fx + (h11 - h10) * fz
        }
    }

    /// Surface normal of the triangle under (x, z).
    pub fn normal_at(&self, x: f32, z: f32) -> Vec3 {
        let gx = ((x + self.half) / self.cell).clamp(0.0, (self.n - 1) as f32 - 1e-4);
        let gz = ((z + self.half) / self.cell).clamp(0.0, (self.n - 1) as f32 - 1e-4);
        let (i, j) = (gx as usize, gz as usize);
        let (fx, fz) = (gx - i as f32, gz - j as f32);
        let h00 = self.get(i, j);
        let h10 = self.get(i + 1, j);
        let h01 = self.get(i, j + 1);
        let h11 = self.get(i + 1, j + 1);
        let c = self.cell;
        // slopes along +x and +z within the triangle
        let (sx, sz) = if fz >= fx { ((h11 - h01) / c, (h01 - h00) / c) } else { ((h10 - h00) / c, (h11 - h10) / c) };
        Vec3::new(-sx, 1.0, -sz).normalize()
    }

    /// Smooth vertex normal from central differences (used for shading).
    pub fn smooth_normal(&self, i: usize, j: usize) -> Vec3 {
        let (i, j) = (i as i32, j as i32);
        let dx = (self.get_c(i + 1, j) - self.get_c(i - 1, j)) / (2.0 * self.cell);
        let dz = (self.get_c(i, j + 1) - self.get_c(i, j - 1)) / (2.0 * self.cell);
        Vec3::new(-dx, 1.0, -dz).normalize()
    }

    /// Gradient magnitude (rise over run) at a point.
    pub fn slope_at(&self, x: f32, z: f32) -> f32 {
        let n = self.normal_at(x, z);
        (1.0 - n.y * n.y).max(0.0).sqrt() / n.y.max(1e-3)
    }

    /// Gradient vector (dh/dx, dh/dz).
    pub fn gradient_at(&self, x: f32, z: f32) -> Vec2 {
        let n = self.normal_at(x, z);
        Vec2::new(-n.x / n.y, -n.z / n.y)
    }

    /// First intersection of a ray with the terrain surface (marching with
    /// refinement). Returns the ray parameter.
    pub fn raycast(&self, o: Vec3, d: Vec3, max_t: f32) -> Option<f32> {
        let step = self.cell * 0.5;
        let mut t = 0.0f32;
        let mut prev_t = 0.0f32;
        let mut prev_above = o.y >= self.height_at(o.x, o.z);
        if !prev_above {
            // Starting below ground: treat as no hit (e.g. inside a hill by accident).
            return None;
        }
        while t < max_t {
            t = (t + step).min(max_t);
            let p = o + d * t;
            // Early out once outside the map and moving away / above everything.
            if !self.contains(p.x, p.z) && p.y > 120.0 {
                return None;
            }
            let above = p.y >= self.height_at(p.x, p.z);
            if prev_above && !above {
                // refine by bisection
                let (mut a, mut b) = (prev_t, t);
                for _ in 0..8 {
                    let m = (a + b) * 0.5;
                    let q = o + d * m;
                    if q.y >= self.height_at(q.x, q.z) {
                        a = m;
                    } else {
                        b = m;
                    }
                }
                return Some((a + b) * 0.5);
            }
            prev_above = above;
            prev_t = t;
            if t >= max_t {
                break;
            }
        }
        None
    }

    /// Box blur used to smooth road corridors and plazas.
    pub fn blurred(&self, radius: usize, passes: usize) -> Vec<f32> {
        let n = self.n;
        let mut a = self.h.clone();
        let mut b = vec![0.0; n * n];
        let r = radius as i32;
        for _ in 0..passes {
            // horizontal
            for j in 0..n {
                let row = j * n;
                let mut sum = 0.0;
                for k in -r..=r {
                    sum += a[row + (k.clamp(0, n as i32 - 1)) as usize];
                }
                for i in 0..n as i32 {
                    b[row + i as usize] = sum / (2 * r + 1) as f32;
                    sum += a[row + (i + r + 1).clamp(0, n as i32 - 1) as usize];
                    sum -= a[row + (i - r).clamp(0, n as i32 - 1) as usize];
                }
            }
            // vertical
            for i in 0..n {
                let mut sum = 0.0;
                for k in -r..=r {
                    sum += b[(k.clamp(0, n as i32 - 1)) as usize * n + i];
                }
                for j in 0..n as i32 {
                    a[j as usize * n + i] = sum / (2 * r + 1) as f32;
                    sum += b[(j + r + 1).clamp(0, n as i32 - 1) as usize * n + i];
                    sum -= b[(j - r).clamp(0, n as i32 - 1) as usize * n + i];
                }
            }
        }
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp_map() -> Heightmap {
        let mut hm = Heightmap::new(17, 2.0);
        for j in 0..17 {
            for i in 0..17 {
                // height = 0.5 * x + 0.25 * z (planar)
                let x = hm.world_x(i);
                let z = hm.world_x(j);
                hm.set(i, j, 0.5 * x + 0.25 * z);
            }
        }
        hm
    }

    #[test]
    fn planar_surface_is_exact_everywhere() {
        let hm = ramp_map();
        for k in 0..200 {
            let x = -15.0 + (k as f32 * 0.1537) % 30.0;
            let z = -15.0 + (k as f32 * 0.2311) % 30.0;
            let expect = 0.5 * x + 0.25 * z;
            assert!((hm.height_at(x, z) - expect).abs() < 1e-3, "{x},{z}");
        }
    }

    #[test]
    fn normal_and_slope_of_plane() {
        let hm = ramp_map();
        let n = hm.normal_at(1.0, 2.0);
        let expect = Vec3::new(-0.5, 1.0, -0.25).normalize();
        assert!((n - expect).length() < 1e-4);
        let s = hm.slope_at(1.0, 2.0);
        assert!((s - (0.25f32 + 0.0625).sqrt()).abs() < 1e-3);
        let g = hm.gradient_at(1.0, 2.0);
        assert!((g - Vec2::new(0.5, 0.25)).length() < 1e-3);
    }

    #[test]
    fn raycast_hits_plane_where_expected() {
        let hm = ramp_map();
        // shoot straight down at (4, 4): ground height 3.0
        let t = hm.raycast(Vec3::new(4.0, 20.0, 4.0), Vec3::NEG_Y, 100.0).unwrap();
        assert!((20.0 - t - 3.0).abs() < 0.01, "t={t}");
        // shoot upward: no hit
        assert!(hm.raycast(Vec3::new(4.0, 20.0, 4.0), Vec3::Y, 100.0).is_none());
    }

    #[test]
    fn blur_preserves_constant_field() {
        let mut hm = Heightmap::new(33, 2.0);
        hm.h.iter_mut().for_each(|v| *v = 5.0);
        let b = hm.blurred(3, 2);
        assert!(b.iter().all(|v| (*v - 5.0).abs() < 1e-3));
    }
}
