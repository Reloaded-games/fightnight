//! Dump a top-down map of the generated world: `cargo run -p fn-core --release --example dump_map -- <seed> <out.png>`
use fn_core::math::*;
use fn_core::world::props::*;
use fn_core::world::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1234);
    let out = args.get(2).cloned().unwrap_or_else(|| "map.png".into());
    let t0 = std::time::Instant::now();
    let w = World::generate(seed);
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
            let (col, r) = match p.kind {
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
    std::fs::write(&out, fn_core::png::encode_rgba(size as u32, size as u32, &px)).unwrap();
    println!("wrote {out}");
}
