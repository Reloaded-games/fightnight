//! Ground "splat" map: an RGBA8 texture covering the whole island that the
//! terrain shader uses to draw crisp roads, lawns and farmland on top of the
//! painted vertex colours. R = dirt road, G = asphalt, B = lawn, A = farmland.

use super::{WORLD_HALF, WORLD_SIZE};
use crate::math::*;

pub const SPLAT_N: usize = 1024;
pub const CH_DIRT: usize = 0;
pub const CH_ASPHALT: usize = 1;
pub const CH_LAWN: usize = 2;
pub const CH_FIELD: usize = 3;
/// Width in metres of the stored edge ramp for crisp features (roads, fields).
pub const EDGE_RAMP: f32 = 2.4;

#[derive(Clone)]
pub struct Splat {
    pub data: Vec<u8>,
}

impl Default for Splat {
    fn default() -> Self {
        Self::new()
    }
}

impl Splat {
    pub fn new() -> Self {
        Self { data: vec![0; SPLAT_N * SPLAT_N * 4] }
    }

    #[inline]
    fn to_texel(p: Vec2) -> Vec2 {
        (p + Vec2::splat(WORLD_HALF)) / WORLD_SIZE * SPLAT_N as f32
    }
    #[inline]
    fn texel_center(i: usize, j: usize) -> Vec2 {
        Vec2::new((i as f32 + 0.5) / SPLAT_N as f32 * WORLD_SIZE - WORLD_HALF, (j as f32 + 0.5) / SPLAT_N as f32 * WORLD_SIZE - WORLD_HALF)
    }

    fn put(&mut self, i: usize, j: usize, ch: usize, cov: f32) {
        let k = (j * SPLAT_N + i) * 4 + ch;
        let v = (cov.clamp(0.0, 1.0) * 255.0) as u8;
        if v > self.data[k] {
            self.data[k] = v;
        }
    }

    fn bounds(&self, lo: Vec2, hi: Vec2) -> (usize, usize, usize, usize) {
        let a = Self::to_texel(lo);
        let b = Self::to_texel(hi);
        let c = |v: f32| (v.floor().max(0.0) as usize).min(SPLAT_N - 1);
        (c(a.x), c(b.x + 1.0), c(a.y), c(b.y + 1.0))
    }

    /// Paint a stroke along a segment. The stored value is a linear ramp across
    /// `ramp` metres centred on the stroke edge (0.5 at the edge); the terrain
    /// shader thresholds it to get crisp, anti-aliased edges. A wide `ramp`
    /// (lawns) simply stays soft.
    pub fn paint_segment(&mut self, a: Vec2, b: Vec2, half_width: f32, ch: usize) {
        self.paint_segment_ramp(a, b, half_width, ch, EDGE_RAMP);
    }

    pub fn paint_segment_ramp(&mut self, a: Vec2, b: Vec2, half_width: f32, ch: usize, ramp: f32) {
        let reach = half_width + ramp;
        let (i0, i1, j0, j1) = self.bounds(a.min(b) - Vec2::splat(reach), a.max(b) + Vec2::splat(reach));
        for j in j0..=j1 {
            for i in i0..=i1 {
                let p = Self::texel_center(i, j);
                let (d, _) = point_segment_dist(p, a, b);
                let cov = 0.5 + (half_width - d) / ramp;
                if cov > 0.0 {
                    self.put(i, j, ch, cov);
                }
            }
        }
    }

    pub fn paint_polyline(&mut self, pts: &[Vec2], half_width: f32, ch: usize) {
        for w in pts.windows(2) {
            self.paint_segment(w[0], w[1], half_width, ch);
        }
    }

    pub fn paint_disc(&mut self, c: Vec2, r: f32, ch: usize, ramp: f32) {
        let reach = r + ramp;
        let (i0, i1, j0, j1) = self.bounds(c - Vec2::splat(reach), c + Vec2::splat(reach));
        for j in j0..=j1 {
            for i in i0..=i1 {
                let d = Self::texel_center(i, j).distance(c);
                let cov = 0.5 + (r - d) / ramp;
                if cov > 0.0 {
                    self.put(i, j, ch, cov);
                }
            }
        }
    }

    /// Axis aligned rectangle (ramp across its edges).
    pub fn paint_rect(&mut self, min: Vec2, max: Vec2, ch: usize, ramp: f32) {
        let (i0, i1, j0, j1) = self.bounds(min - Vec2::splat(ramp), max + Vec2::splat(ramp));
        for j in j0..=j1 {
            for i in i0..=i1 {
                let p = Self::texel_center(i, j);
                let dx = (min.x - p.x).max(p.x - max.x);
                let dz = (min.y - p.y).max(p.y - max.y);
                let d = dx.max(dz);
                let cov = 0.5 - d / ramp;
                if cov > 0.0 {
                    self.put(i, j, ch, cov);
                }
            }
        }
    }

    pub fn sample(&self, p: Vec2) -> [f32; 4] {
        let t = Self::to_texel(p);
        let (i, j) = ((t.x as usize).min(SPLAT_N - 1), (t.y as usize).min(SPLAT_N - 1));
        let k = (j * SPLAT_N + i) * 4;
        [self.data[k] as f32 / 255.0, self.data[k + 1] as f32 / 255.0, self.data[k + 2] as f32 / 255.0, self.data[k + 3] as f32 / 255.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_paints_centre_not_far() {
        let mut s = Splat::new();
        s.paint_segment(Vec2::new(-50.0, 0.0), Vec2::new(50.0, 0.0), 3.0, CH_ASPHALT);
        assert!(s.sample(Vec2::new(0.0, 0.0))[CH_ASPHALT] > 0.95);
        assert!(s.sample(Vec2::new(0.0, 1.5))[CH_ASPHALT] > 0.9);
        assert!(s.sample(Vec2::new(0.0, 8.0))[CH_ASPHALT] < 0.01);
        assert!(s.sample(Vec2::new(0.0, 0.0))[CH_DIRT] < 0.01);
    }

    #[test]
    fn disc_and_rect() {
        let mut s = Splat::new();
        s.paint_disc(Vec2::new(100.0, 100.0), 10.0, CH_LAWN, 2.0);
        assert!(s.sample(Vec2::new(100.0, 100.0))[CH_LAWN] > 0.95);
        assert!(s.sample(Vec2::new(120.0, 100.0))[CH_LAWN] < 0.01);
        s.paint_rect(Vec2::new(-20.0, -20.0), Vec2::new(20.0, 20.0), CH_FIELD, 1.0);
        assert!(s.sample(Vec2::new(0.0, 0.0))[CH_FIELD] > 0.95);
        assert!(s.sample(Vec2::new(30.0, 0.0))[CH_FIELD] < 0.01);
    }
}
