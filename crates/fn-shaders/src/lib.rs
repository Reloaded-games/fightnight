//! WGSL shader sources. Scene shaders are `COMMON` + a stage specific body; the
//! post-processing shader is self contained. A unit test validates every module
//! with naga so shader mistakes are caught without a GPU.

pub const COMMON: &str = include_str!("../wgsl/common.wgsl");
pub const TERRAIN: &str = include_str!("../wgsl/terrain.wgsl");
pub const MESH: &str = include_str!("../wgsl/mesh.wgsl");
pub const SKY: &str = include_str!("../wgsl/sky.wgsl");
pub const WATER: &str = include_str!("../wgsl/water.wgsl");
pub const PARTICLES: &str = include_str!("../wgsl/particles.wgsl");
pub const STORM: &str = include_str!("../wgsl/storm.wgsl");
pub const SHADOW: &str = include_str!("../wgsl/shadow.wgsl");
pub const POST: &str = include_str!("../wgsl/post.wgsl");

/// Scene shaders share the prelude (globals, noise, sky, lighting).
pub fn scene(body: &str) -> String {
    format!("{COMMON}\n{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(name: &str, src: &str) {
        let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{name}: parse error:\n{}", e.emit_to_string(src)));
        let mut v = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
        if let Err(e) = v.validate(&module) {
            panic!("{name}: validation error:\n{}", e.emit_to_string(src));
        }
    }

    #[test]
    fn all_shaders_validate() {
        validate("terrain", &scene(TERRAIN));
        validate("mesh", &scene(MESH));
        validate("sky", &scene(SKY));
        validate("water", &scene(WATER));
        validate("particles", &scene(PARTICLES));
        validate("storm", &scene(STORM));
        validate("shadow", SHADOW);
        validate("post", POST);
    }
}
