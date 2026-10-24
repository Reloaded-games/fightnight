//! Height map of an island as a picture (grey by height, contour lines every 5 m, water blue):
//! `cargo run -p fn-core --release --example dump_height -- <seed> <out.png> [map]`
use fn_core::world::*;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1234);
    let out = args.get(2).cloned().unwrap_or_else(|| "height.png".into());
    let map = args.get(3).and_then(|s| maps::MapKind::from_key(s)).unwrap_or_default();
    let w = World::generate_map(seed, map);
    let size = 960usize;
    let mut px = vec![255u8; size * size * 4];
    let mut top = 0.0f32;
    for j in 0..size {
        for i in 0..size {
            let x = -WORLD_HALF + (i as f32 + 0.5) / size as f32 * WORLD_SIZE;
            let z = -WORLD_HALF + (j as f32 + 0.5) / size as f32 * WORLD_SIZE;
            let h = w.hm.height_at(x, z);
            top = top.max(h);
            let c = if h < 0.0 {
                [40, 90, 200]
            } else {
                let g = (40.0 + h * 3.6).clamp(0.0, 255.0) as u8;
                let contour = (h / 5.0).fract() < 0.06;
                if contour { [g / 2, g / 2, g / 2] } else { [g, g, g] }
            };
            let o = (j * size + i) * 4;
            px[o..o + 3].copy_from_slice(&c);
        }
    }
    std::fs::write(&out, fn_core::png::encode_rgba(size as u32, size as u32, &px)).unwrap();
    println!("highest point {top:.1} m; wrote {out}");
}
