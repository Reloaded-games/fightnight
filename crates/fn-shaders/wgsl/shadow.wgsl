// Depth-only shadow caster pass. Uses its own tiny bind group (cascade matrix + wind).

struct ShadowU {
    vp: mat4x4<f32>,
    params: vec4<f32>, // x time, y wind strength
};
@group(0) @binding(0) var<uniform> S: ShadowU;

struct TerrainIn {
    @location(0) pos: vec3<f32>,
};

@vertex
fn vs_terrain(v: TerrainIn) -> @builtin(position) vec4<f32> {
    return S.vp * vec4<f32>(v.pos, 1.0);
}

struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) col: vec4<f32>,
    @location(3) attr: vec4<f32>,
    @location(4) m0: vec4<f32>,
    @location(5) m1: vec4<f32>,
    @location(6) m2: vec4<f32>,
    @location(7) icol: vec4<f32>,
    @location(8) ipar: vec4<f32>,
};

@vertex
fn vs_mesh(v: MeshIn) -> @builtin(position) vec4<f32> {
    let p4 = vec4<f32>(v.pos, 1.0);
    var wp = vec3<f32>(dot(v.m0, p4), dot(v.m1, p4), dot(v.m2, p4));
    let sway = v.attr.z;
    if (sway > 0.0) {
        let origin = vec2<f32>(v.m0.w, v.m2.w);
        let t = S.params.x;
        let ph = origin.x * 0.31 + origin.y * 0.23;
        let gust = sin(t * 1.3 + ph) * 0.6 + sin(t * 2.7 + ph * 1.7) * 0.4;
        let g2 = cos(t * 1.1 + ph * 1.3) * 0.6;
        wp = wp + vec3<f32>(gust, 0.0, g2) * (sway * S.params.y * 0.22);
    }
    return S.vp * vec4<f32>(wp, 1.0);
}
