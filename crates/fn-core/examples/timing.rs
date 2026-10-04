//! Time world generation pieces and audio synthesis: `cargo run -p fn-core --release --example timing [minimap.png]`
use fn_core::audio_synth::*;
fn main() {
    let w = fn_core::world::World::generate(1234);
    let t = std::time::Instant::now();
    let px = fn_core::world::minimap::render(&w, 1024);
    println!("minimap 1024: {:?}", t.elapsed());
    if let Some(path) = std::env::args().nth(1) {
        std::fs::write(path, fn_core::png::encode_rgba(1024, 1024, &px)).unwrap();
    }
    let t0 = std::time::Instant::now();
    let mut total = 0usize;
    let mut worst = (0.0f64, "");
    for sfx in Sfx::ALL {
        for v in 0..sfx.variants() {
            let t = std::time::Instant::now();
            let b = synth(*sfx, 44100, v);
            let e = t.elapsed().as_secs_f64();
            if e > worst.0 { worst = (e, sfx.name()); }
            total += b.len();
        }
    }
    println!("audio: {:?} for {} samples ({:.1} s of sound), slowest {} {:.0} ms", t0.elapsed(), total, total as f32 / 44100.0, worst.1, worst.0 * 1000.0);
}
