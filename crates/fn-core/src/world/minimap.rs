//! A stylised top-down map of the island for the HUD (north is -Z, up on the image).

use super::props::PropKind;
use super::{World, WORLD_HALF, WORLD_SIZE};
use crate::math::*;

/// Render the island as `size` x `size` RGBA pixels. Pixel (i, j) covers world x = i, z = j.
pub fn render(w: &World, size: usize) -> Vec<u8> {
    let mut px = vec![255u8; size * size * 4];
    let light = Vec3::new(-0.55, 0.75, -0.35).normalize();
    // coverage of trees (for soft forest shading)
    let dn = size / 4;
    let mut density = vec![0.0f32; dn * dn];
    for chunk in &w.chunk_props {
        for p in chunk {
            if p.kind.is_tree() {
                let i = (((p.pos.x + WORLD_HALF) / WORLD_SIZE * dn as f32) as usize).min(dn - 1);
                let j = (((p.pos.z + WORLD_HALF) / WORLD_SIZE * dn as f32) as usize).min(dn - 1);
                density[j * dn + i] += 1.0;
            }
        }
    }
    // one pass of blur so forests read as soft patches
    let mut blurred = vec![0.0f32; dn * dn];
    for j in 0..dn {
        for i in 0..dn {
            let mut s = 0.0;
            let mut n = 0.0;
            for dj in -1i32..=1 {
                for di in -1i32..=1 {
                    let (x, y) = (i as i32 + di, j as i32 + dj);
                    if x >= 0 && y >= 0 && (x as usize) < dn && (y as usize) < dn {
                        s += density[y as usize * dn + x as usize];
                        n += 1.0;
                    }
                }
            }
            blurred[j * dn + i] = s / n;
        }
    }
    let mut land_mask = vec![false; size * size];
    let cell = WORLD_SIZE / dn as f32;
    let forest_k = 1.0 / (cell * cell / 130.0); // trees per cell that read as a thick wood
    for j in 0..size {
        for i in 0..size {
            let x = -WORLD_HALF + (i as f32 + 0.5) / size as f32 * WORLD_SIZE;
            let z = -WORLD_HALF + (j as f32 + 0.5) / size as f32 * WORLD_SIZE;
            let h = w.hm.height_at(x, z);
            let p2 = Vec2::new(x, z);
            let lake = w.layout.lakes.iter().any(|l| l.center.distance(p2) < l.radius * 1.2 && h < l.level - 0.1);
            let mut c;
            if h < 0.0 || lake {
                let depth = if lake { 2.5 } else { -h };
                c = Vec3::new(0.30, 0.68, 0.92).lerp(Vec3::new(0.12, 0.40, 0.78), (depth / 14.0).clamp(0.0, 1.0));
                // foam along the shore
                let shore = (1.0 - (depth / 1.4).clamp(0.0, 1.0)) * 0.55;
                c = c.lerp(Vec3::new(0.85, 0.95, 1.0), shore);
            } else {
                let n = w.hm.normal_at(x, z);
                let shade = (n.dot(light) * 0.55 + 0.52).clamp(0.55, 1.18);
                let base = w.painter.color(x, z, h, w.hm.slope_at(x, z));
                // lift the saturation a touch so the map pops like the in-game look
                let g = (base.x + base.y + base.z) / 3.0;
                let base = Vec3::splat(g) + (base - Vec3::splat(g)) * 1.25;
                c = base * shade * 1.05;
                // forest shading
                // bilinear lookup keeps the woods soft instead of blocky
                let fx = ((x + WORLD_HALF) / WORLD_SIZE * dn as f32 - 0.5).clamp(0.0, dn as f32 - 1.001);
                let fz = ((z + WORLD_HALF) / WORLD_SIZE * dn as f32 - 0.5).clamp(0.0, dn as f32 - 1.001);
                let (i0, j0) = (fx as usize, fz as usize);
                let (tx, tz) = (fx - i0 as f32, fz - j0 as f32);
                let d = |i: usize, j: usize| blurred[j * dn + i];
                let dens = lerp(lerp(d(i0, j0), d(i0 + 1, j0), tx), lerp(d(i0, j0 + 1), d(i0 + 1, j0 + 1), tx), tz);
                let f = smoothstep(0.05, 0.85, dens * forest_k);
                c = c.lerp(Vec3::new(0.10, 0.32, 0.16) * shade, f * 0.55);
                let s = w.splat.sample(p2);
                if s[3] > 0.5 {
                    c = c.lerp(Vec3::new(0.93, 0.83, 0.55), 0.8);
                }
                if s[0] > 0.3 {
                    c = c.lerp(Vec3::new(0.86, 0.76, 0.55), 0.85);
                }
                if s[1] > 0.3 {
                    c = c.lerp(Vec3::new(0.62, 0.64, 0.68), 0.9);
                }
            }
            let o = (j * size + i) * 4;
            px[o] = (c.x.clamp(0.0, 1.0) * 255.0) as u8;
            px[o + 1] = (c.y.clamp(0.0, 1.0) * 255.0) as u8;
            px[o + 2] = (c.z.clamp(0.0, 1.0) * 255.0) as u8;
            land_mask[j * size + i] = !(h < 0.0 || lake);
        }
    }
    soften_land(&mut px, &land_mask, size, 3);
    let to_px = |p: Vec2| (((p.x + WORLD_HALF) / WORLD_SIZE * size as f32) as i32, ((p.y + WORLD_HALF) / WORLD_SIZE * size as f32) as i32);
    let mut put = |x: i32, y: i32, c: [u8; 3], a: f32| {
        if x >= 0 && y >= 0 && (x as usize) < size && (y as usize) < size {
            let o = (y as usize * size + x as usize) * 4;
            for k in 0..3 {
                px[o + k] = (px[o + k] as f32 * (1.0 - a) + c[k] as f32 * a) as u8;
            }
        }
    };
    // rocks
    for chunk in &w.chunk_props {
        for p in chunk {
            if matches!(p.kind, PropKind::Boulder | PropKind::Rock0 | PropKind::Rock1 | PropKind::Rock2) {
                let (x, y) = to_px(Vec2::new(p.pos.x, p.pos.z));
                put(x, y, [128, 130, 136], 0.9);
            }
        }
    }
    // buildings: warm roofs with a drop shadow
    for b in &w.buildings {
        let (x0, y0) = to_px(Vec2::new(b.aabb.min.x, b.aabb.min.z));
        let (x1, y1) = to_px(Vec2::new(b.aabb.max.x, b.aabb.max.z));
        for y in y0..=y1 {
            for x in x0..=x1 {
                put(x + 1, y + 1, [30, 40, 30], 0.35);
            }
        }
        let roof = match b.kind {
            "barn" => [196, 84, 70],
            "windmill" => [214, 200, 170],
            _ => [226, 214, 190],
        };
        for y in y0..=y1 {
            for x in x0..=x1 {
                let edge = x == x0 || x == x1 || y == y0 || y == y1;
                put(x, y, if edge { [150, 120, 100] } else { roof }, 0.96);
            }
        }
    }
    px
}

/// Masked box blur over land pixels only (keeps the shoreline crisp, removes the patchwork look of
/// the per-cell ground colours).
fn soften_land(px: &mut [u8], mask: &[bool], size: usize, radius: usize) {
    let r = radius as i32;
    let n = size as i32;
    let mut tmp = vec![[0.0f32; 3]; size * size];
    // horizontal
    for j in 0..size {
        for i in 0..size {
            let mut acc = [0.0f32; 3];
            let mut cnt = 0.0;
            for d in -r..=r {
                let x = i as i32 + d;
                if x < 0 || x >= n {
                    continue;
                }
                let k = j * size + x as usize;
                if mask[k] {
                    for c in 0..3 {
                        acc[c] += px[k * 4 + c] as f32;
                    }
                    cnt += 1.0;
                }
            }
            if cnt > 0.0 {
                tmp[j * size + i] = [acc[0] / cnt, acc[1] / cnt, acc[2] / cnt];
            }
        }
    }
    // vertical
    for j in 0..size {
        for i in 0..size {
            let k = j * size + i;
            if !mask[k] {
                continue;
            }
            let mut acc = [0.0f32; 3];
            let mut cnt = 0.0;
            for d in -r..=r {
                let y = j as i32 + d;
                if y < 0 || y >= n {
                    continue;
                }
                let kk = y as usize * size + i;
                if mask[kk] {
                    for c in 0..3 {
                        acc[c] += tmp[kk][c];
                    }
                    cnt += 1.0;
                }
            }
            if cnt > 0.0 {
                for c in 0..3 {
                    px[k * 4 + c] = (acc[c] / cnt) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimap_has_water_land_and_buildings() {
        let w = World::generate(1234);
        let size = 256;
        let px = render(&w, size);
        assert_eq!(px.len(), size * size * 4);
        // the corner is open ocean (blue dominant)
        let c = &px[0..4];
        assert!(c[2] > c[0] + 40, "corner should be water: {c:?}");
        // the island centre is land (green or sand dominant, not water)
        let o = ((size / 2) * size + size / 2) * 4;
        let m = &px[o..o + 4];
        assert!(!(m[2] > m[1] + 50), "centre should be land: {m:?}");
        // opaque everywhere
        assert!(px.chunks(4).all(|p| p[3] == 255));
    }
}
