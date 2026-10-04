//! Deterministic random numbers: a PCG32 generator plus stateless integer hashes
//! used for scattering props reproducibly.

use crate::math::{Vec2, Vec3};

#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
    inc: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut r = Rng { state: 0, inc: (seed << 1) | 1 };
        r.u32();
        r.state = r.state.wrapping_add(seed ^ 0x9E37_79B9_7F4A_7C15);
        r.u32();
        r
    }

    /// Derive an independent stream (e.g. one per subsystem) from this one.
    pub fn fork(&mut self, salt: u64) -> Rng {
        let s = ((self.u32() as u64) << 32) | self.u32() as u64;
        Rng::new(s ^ salt.wrapping_mul(0x2545_F491_4F6C_DD1D))
    }

    #[inline]
    pub fn u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in [0, 1).
    #[inline]
    pub fn f32(&mut self) -> f32 {
        (self.u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    #[inline]
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f32()
    }

    /// Uniform integer in [a, b).
    #[inline]
    pub fn range_i(&mut self, a: i32, b: i32) -> i32 {
        if b <= a {
            return a;
        }
        a + (self.u32() % (b - a) as u32) as i32
    }

    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.u32() as usize) % n
        }
    }

    #[inline]
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    #[inline]
    pub fn sign(&mut self) -> f32 {
        if self.u32() & 1 == 0 {
            1.0
        } else {
            -1.0
        }
    }

    pub fn pick<'a, T>(&mut self, s: &'a [T]) -> &'a T {
        &s[self.below(s.len())]
    }

    /// Pick an index according to non-negative weights.
    pub fn weighted(&mut self, weights: &[f32]) -> usize {
        let total: f32 = weights.iter().sum();
        if total <= 0.0 {
            return self.below(weights.len());
        }
        let mut r = self.f32() * total;
        for (i, w) in weights.iter().enumerate() {
            if r < *w {
                return i;
            }
            r -= *w;
        }
        weights.len() - 1
    }

    pub fn shuffle<T>(&mut self, s: &mut [T]) {
        for i in (1..s.len()).rev() {
            let j = self.below(i + 1);
            s.swap(i, j);
        }
    }

    /// Uniform point in a disc (area-uniform).
    pub fn in_disc(&mut self, r: f32) -> Vec2 {
        let a = self.f32() * std::f32::consts::TAU;
        let d = self.f32().sqrt() * r;
        Vec2::new(a.cos() * d, a.sin() * d)
    }

    pub fn unit_vec3(&mut self) -> Vec3 {
        loop {
            let v = Vec3::new(self.range(-1.0, 1.0), self.range(-1.0, 1.0), self.range(-1.0, 1.0));
            let l = v.length_squared();
            if l > 1e-4 && l <= 1.0 {
                return v / l.sqrt();
            }
        }
    }

    /// Approximately normal distributed (sum of uniforms), mean 0, sd ~1.
    pub fn gauss(&mut self) -> f32 {
        let mut s = 0.0;
        for _ in 0..6 {
            s += self.f32();
        }
        (s - 3.0) * 1.414
    }
}

#[inline]
pub fn hash_u32(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

#[inline]
pub fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    let h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1) ^ seed.wrapping_mul(0x9e37_79b9);
    hash_u32(h)
}

#[inline]
pub fn hash3(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    hash_u32(hash2(x, y, seed) ^ (z as u32).wrapping_mul(0x85eb_ca6b))
}

/// Hash to a float in [0, 1).
#[inline]
pub fn hash2f(x: i32, y: i32, seed: u32) -> f32 {
    (hash2(x, y, seed) >> 8) as f32 * (1.0 / 16_777_216.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_in_range() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            let x = a.f32();
            assert_eq!(x, b.f32());
            assert!((0.0..1.0).contains(&x));
        }
        let mut c = Rng::new(43);
        assert_ne!(Rng::new(42).u32(), c.u32());
    }

    #[test]
    fn range_i_bounds() {
        let mut r = Rng::new(7);
        for _ in 0..1000 {
            let v = r.range_i(-3, 5);
            assert!((-3..5).contains(&v));
        }
    }

    #[test]
    fn weighted_respects_zero() {
        let mut r = Rng::new(9);
        for _ in 0..500 {
            assert_ne!(r.weighted(&[1.0, 0.0, 2.0]), 1);
        }
    }

    #[test]
    fn disc_samples_inside() {
        let mut r = Rng::new(3);
        for _ in 0..500 {
            assert!(r.in_disc(5.0).length() <= 5.0001);
        }
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(hash2(3, 4, 5), hash2(3, 4, 5));
        assert_ne!(hash2(3, 4, 5), hash2(4, 3, 5));
        let f = hash2f(10, 20, 1);
        assert!((0.0..1.0).contains(&f));
    }
}
