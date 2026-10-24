//! Dump a top-down map of the generated world: `cargo run -p fn-core --release --example dump_map -- <seed> <out.png> [map]`
//! (`map`: island, chapter1 or chapter2; the recreated maps get the map grid and the names of their places drawn on).
use fn_core::math::*;
use fn_core::world::props::*;
use fn_core::world::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1234);
    let out = args.get(2).cloned().unwrap_or_else(|| "map.png".into());
    let map = args.get(3).and_then(|s| maps::MapKind::from_key(s)).unwrap_or_default();
    let t0 = std::time::Instant::now();
    let w = World::generate_map(seed, map);
    println!("world generated in {:?}", t0.elapsed());
    let mut counts = std::collections::BTreeMap::new();
    for c in &w.chunk_props {
        for p in c {
            *counts.entry(format!("{:?}", p.kind)).or_insert(0) += 1;
        }
    }
    println!("props: {counts:?}");
    println!("buildings: {}, chests: {}, loot spots: {}, harvestables: {}", w.buildings.len(), w.chest_spots.len(), w.loot_spots.len(), w.harvest.len());
    let mut kinds = std::collections::BTreeMap::new();
    for b in &w.buildings {
        *kinds.entry(b.kind).or_insert(0) += 1;
    }
    println!("building kinds: {kinds:?}");
    for p in &w.layout.pois {
        let n = w.buildings.iter().filter(|b| w.layout.pois[b.poi].name == p.name).count();
        println!("  {:<14} {:?} -> {n} buildings", p.name, p.kind);
    }
    let size = 1280usize;
    let mut px = vec![255u8; size * size * 4];
    let light = Vec3::new(-0.5, 0.8, -0.4).normalize();
    for j in 0..size {
        for i in 0..size {
            let x = -WORLD_HALF + (i as f32 + 0.5) / size as f32 * WORLD_SIZE;
            let z = -WORLD_HALF + (j as f32 + 0.5) / size as f32 * WORLD_SIZE;
            let h = w.hm.height_at(x, z);
            let n = w.hm.normal_at(x, z);
            let shade = (n.dot(light) * 0.6 + 0.55).clamp(0.3, 1.2);
            let lake = w.layout.lakes.iter().any(|l| l.center.distance(Vec2::new(x, z)) < l.radius * 1.2 && h < l.level - 0.1);
            let mut c = if h < 0.0 || lake {
                let depth = if lake { 3.0 } else { -h };
                Vec3::new(0.2, 0.55, 0.85).lerp(Vec3::new(0.05, 0.25, 0.6), (depth / 12.0).clamp(0.0, 1.0))
            } else {
                let c = w.painter.color(x, z, h, w.hm.slope_at(x, z));
                c * shade
            };
            let s = w.splat.sample(Vec2::new(x, z));
            if s[0] > 0.5 {
                c = Vec3::new(0.7, 0.58, 0.38);
            }
            if s[1] > 0.5 {
                c = Vec3::splat(0.3);
            }
            if s[3] > 0.5 {
                c = Vec3::new(0.8, 0.68, 0.35);
            }
            if s[2] > 0.6 {
                c = c.lerp(Vec3::new(0.5, 0.9, 0.3), 0.5);
            }
            let o = (j * size + i) * 4;
            px[o] = (c.x.clamp(0.0, 1.0) * 255.0) as u8;
            px[o + 1] = (c.y.clamp(0.0, 1.0) * 255.0) as u8;
            px[o + 2] = (c.z.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    let to_px = |p: Vec2| (((p.x + WORLD_HALF) / WORLD_SIZE * size as f32) as i32, ((p.y + WORLD_HALF) / WORLD_SIZE * size as f32) as i32);
    let mut put = |x: i32, y: i32, c: [u8; 3]| {
        if x >= 0 && y >= 0 && (x as usize) < size && (y as usize) < size {
            let o = (y as usize * size + x as usize) * 4;
            px[o] = c[0];
            px[o + 1] = c[1];
            px[o + 2] = c[2];
        }
    };
    for chunk in &w.chunk_props {
        for p in chunk {
            let (x, y) = to_px(Vec2::new(p.pos.x, p.pos.z));
            let (col, r) = match p.kind.base() {
                PropKind::Pine => ([20, 80, 40], 2),
                PropKind::Oak => ([40, 130, 30], 2),
                PropKind::Birch => ([110, 170, 60], 2),
                PropKind::Palm => ([30, 150, 90], 2),
                PropKind::Bush => ([60, 140, 50], 1),
                PropKind::Rock0 | PropKind::Rock1 | PropKind::Rock2 | PropKind::Boulder => ([120, 120, 125], 1),
                _ => ([220, 200, 60], 1),
            };
            let col = if p.kind.is_tree() && w.painter.autumn(p.pos.x, p.pos.z) > 0.55 { [210, 110, 30] } else { col };
            for dy in -r..=r {
                for dx in -r..=r {
                    put(x + dx, y + dy, col);
                }
            }
        }
    }
    for b in &w.buildings {
        let (x0, y0) = to_px(Vec2::new(b.aabb.min.x, b.aabb.min.z));
        let (x1, y1) = to_px(Vec2::new(b.aabb.max.x, b.aabb.max.z));
        for y in y0..=y1 {
            for x in x0..=x1 {
                put(x, y, [200, 60, 50]);
            }
        }
        let (dx, dy) = to_px(Vec2::new(b.door_out.x, b.door_out.z));
        put(dx, dy, [255, 255, 255]);
    }
    for c in &w.chest_spots {
        let (x, y) = to_px(Vec2::new(c.pos.x, c.pos.z));
        for d in -1..=1 {
            put(x + d, y, [255, 215, 0]);
            put(x, y + d, [255, 215, 0]);
        }
    }
    for r in &w.layout.rivers {
        for seg in r.pts.windows(2) {
            for k in 0..=40 {
                let (x, y) = to_px(seg[0].lerp(seg[1], k as f32 / 40.0));
                put(x, y, [40, 90, 200]);
            }
        }
    }
    if map != maps::MapKind::Island {
        // the map grid, labelled like the game's map: columns A..J, rows 1..10
        for k in 0..=10 {
            let a = maps::grid_to_world(k as f32, 0.0);
            let b = maps::grid_to_world(k as f32, 10.0);
            let (x, y0) = to_px(a);
            let (_, y1) = to_px(b);
            for y in y0..=y1 {
                put(x, y, [255, 255, 255]);
            }
            let a = maps::grid_to_world(0.0, k as f32);
            let b = maps::grid_to_world(10.0, k as f32);
            let (x0, y) = to_px(a);
            let (x1, _) = to_px(b);
            for x in x0..=x1 {
                put(x, y, [255, 255, 255]);
            }
        }
        for k in 0..10 {
            let (x, y) = to_px(maps::grid_to_world(k as f32 + 0.45, 0.0));
            text(&mut px, size, x, y + 4, &((b'A' + k as u8) as char).to_string(), [255, 255, 255], 2);
            let (x, y) = to_px(maps::grid_to_world(0.0, k as f32 + 0.45));
            text(&mut px, size, x + 4, y, &(k + 1).to_string(), [255, 255, 255], 2);
        }
        for p in &w.layout.pois {
            let (x, y) = to_px(p.center);
            let r = (p.radius / WORLD_SIZE * size as f32) as i32;
            for k in 0..64 {
                let a = k as f32 / 64.0 * std::f32::consts::TAU;
                let (cx, cy) = (x + (a.cos() * r as f32) as i32, y + (a.sin() * r as f32) as i32);
                let o = (cy.clamp(0, size as i32 - 1) as usize * size + cx.clamp(0, size as i32 - 1) as usize) * 4;
                px[o..o + 3].copy_from_slice(&[255, 255, 0]);
            }
            let label = p.name.to_uppercase();
            let wpx = label.len() as i32 * 6 * 2;
            text(&mut px, size, x - wpx / 2 + 1, y - 7 + 1, &label, [0, 0, 0], 2);
            text(&mut px, size, x - wpx / 2, y - 7, &label, [255, 255, 255], 2);
        }
    }
    std::fs::write(&out, fn_core::png::encode_rgba(size as u32, size as u32, &px)).unwrap();
    println!("wrote {out}");
}

/// A 5x7 pixel font for the labels: capitals, digits and a little punctuation.
fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        '\'' => [0x04, 0x04, 0x08, 0x00, 0x00, 0x00, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        _ => [0; 7],
    }
}

/// Write `s` with its top left corner at (x, y), each font pixel `scale` image pixels.
fn text(px: &mut [u8], size: usize, x: i32, y: i32, s: &str, col: [u8; 3], scale: i32) {
    for (k, c) in s.chars().enumerate() {
        let g = glyph(c);
        for (row, bits) in g.iter().enumerate() {
            for colx in 0..5 {
                if bits & (0x10 >> colx) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let (px_x, px_y) = (x + (k as i32 * 6 + colx) * scale + dx, y + row as i32 * scale + dy);
                        if px_x >= 0 && px_y >= 0 && (px_x as usize) < size && (px_y as usize) < size {
                            let o = (px_y as usize * size + px_x as usize) * 4;
                            px[o..o + 3].copy_from_slice(&col);
                        }
                    }
                }
            }
        }
    }
}
