//! Coherent noise used by terrain and prop generation. Everything is pure and
//! deterministic given a `seed`.

use crate::math::smootherstep;
use crate::rng::{hash2, hash2f};

#[inline]
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Value noise in [0, 1].
pub fn value(x: f32, y: f32, seed: u32) -> f32 {
    let xi = x.floor();
    let yi = y.floor();
    let fx = fade(x - xi);
    let fy = fade(y - yi);
    let (xi, yi) = (xi as i32, yi as i32);
    let a = hash2f(xi, yi, seed);
    let b = hash2f(xi + 1, yi, seed);
    let c = hash2f(xi, yi + 1, seed);
    let d = hash2f(xi + 1, yi + 1, seed);
    let top = a + (b - a) * fx;
    let bot = c + (d - c) * fx;
    top + (bot - top) * fy
}

#[inline]
fn grad(h: u32, x: f32, y: f32) -> f32 {
    // 8 gradient directions
    match h & 7 {
        0 => x + y,
        1 => x - y,
        2 => -x + y,
        3 => -x - y,
        4 => x * 1.414,
        5 => -x * 1.414,
        6 => y * 1.414,
        _ => -y * 1.414,
    }
}

/// Perlin-style gradient noise in roughly [-1, 1].
pub fn perlin(x: f32, y: f32, seed: u32) -> f32 {
    let xi = x.floor();
    let yi = y.floor();
    let fx = x - xi;
    let fy = y - yi;
    let (xi, yi) = (xi as i32, yi as i32);
    let u = fade(fx);
    let v = fade(fy);
    let n00 = grad(hash2(xi, yi, seed), fx, fy);
    let n10 = grad(hash2(xi + 1, yi, seed), fx - 1.0, fy);
    let n01 = grad(hash2(xi, yi + 1, seed), fx, fy - 1.0);
    let n11 = grad(hash2(xi + 1, yi + 1, seed), fx - 1.0, fy - 1.0);
    let a = n00 + (n10 - n00) * u;
    let b = n01 + (n11 - n01) * u;
    (a + (b - a) * v) * 0.8
}

/// Fractal Brownian motion of `perlin`, normalised to roughly [-1, 1].
pub fn fbm(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut norm = 0.0;
    for i in 0..octaves {
        // Rotate each octave a little to hide axis-aligned artefacts.
        let (rx, ry) = (x * freq * 0.8 + y * freq * 0.6, -x * freq * 0.6 + y * freq * 0.8);
        sum += perlin(rx + i as f32 * 17.3, ry - i as f32 * 9.1, seed.wrapping_add(i * 131)) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// fbm remapped to [0, 1].
#[inline]
pub fn fbm01(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    fbm(x, y, octaves, seed) * 0.5 + 0.5
}

/// Ridged multi-fractal in [0, 1]: sharp crests, good for rocky mountains.
pub fn ridged(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut freq = 1.0;
    let mut norm = 0.0;
    for i in 0..octaves {
        let n = 1.0 - perlin(x * freq + i as f32 * 5.7, y * freq - i as f32 * 3.1, seed.wrapping_add(i * 977)).abs();
        sum += n * n * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.1;
    }
    sum / norm
}

/// Cellular (worley) distance to the nearest feature point, in [0, ~1].
pub fn worley(x: f32, y: f32, seed: u32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let mut best = f32::MAX;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let cx = xi + dx;
            let cy = yi + dy;
            let px = cx as f32 + hash2f(cx, cy, seed);
            let py = cy as f32 + hash2f(cx, cy, seed ^ 0x5bd1_e995);
            let d = (px - x) * (px - x) + (py - y) * (py - y);
            if d < best {
                best = d;
            }
        }
    }
    best.sqrt()
}

/// Soft threshold helper: 0 below `lo`, 1 above `hi`, smooth in between.
#[inline]
pub fn band(v: f32, lo: f32, hi: f32) -> f32 {
    smootherstep(lo, hi, v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_in_range_and_continuous() {
        let mut prev = value(0.0, 3.3, 1);
        for i in 1..2000 {
            let v = value(i as f32 * 0.01, 3.3, 1);
            assert!((0.0..=1.0).contains(&v));
            assert!((v - prev).abs() < 0.08, "discontinuity at {i}");
            prev = v;
        }
    }

    #[test]
    fn perlin_range() {
        let mut lo = 1.0f32;
        let mut hi = -1.0f32;
        for i in 0..4000 {
            let v = perlin(i as f32 * 0.137, i as f32 * 0.071, 5);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        assert!(lo > -1.2 && hi < 1.2, "{lo} {hi}");
        assert!(hi - lo > 0.8, "noise should span a decent range: {lo} {hi}");
    }

    #[test]
    fn fbm_deterministic() {
        assert_eq!(fbm(1.5, 2.5, 5, 9), fbm(1.5, 2.5, 5, 9));
        assert_ne!(fbm(1.5, 2.5, 5, 9), fbm(1.5, 2.5, 5, 10));
    }

    #[test]
    fn ridged_and_worley_in_range() {
        for i in 0..500 {
            let r = ridged(i as f32 * 0.31, i as f32 * 0.17, 4, 2);
            assert!((0.0..=1.0).contains(&r));
            let w = worley(i as f32 * 0.31, i as f32 * 0.17, 2);
            assert!((0.0..=1.5).contains(&w));
        }
    }
}
