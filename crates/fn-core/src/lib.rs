//! FightNight core: platform independent game simulation, world generation and
//! procedural mesh/sound generation. Compiles natively (for tests) and to wasm.
pub mod camera;
pub mod math;
pub mod mesh;
pub mod meshlib;
pub mod noise;
pub mod png;
pub mod rng;
pub mod world;

pub fn hello() -> &'static str {
    "fightnight"
}
