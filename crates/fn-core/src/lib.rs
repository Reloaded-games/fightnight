//! FightNight core: platform independent game simulation, world generation and
//! procedural mesh/sound generation. Compiles natively (for tests) and to wasm.
// Graphics/sim code passes many scalars around; bundling them into structs would only add noise.
#![allow(clippy::too_many_arguments, clippy::type_complexity, clippy::manual_is_multiple_of, clippy::needless_range_loop)]

pub mod audio_synth;
pub mod camera;
pub mod game;
pub mod input;
pub mod math;
pub mod mesh;
pub mod meshlib;
pub mod models;
pub mod net;
pub mod noise;
pub mod png;
pub mod rng;
pub mod shadow;
pub mod world;
