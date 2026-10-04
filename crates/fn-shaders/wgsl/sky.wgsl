// Full-screen sky: gradient, sun, drifting cartoon clouds. Drawn last at the far plane.

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VOut {
    var o: VOut;
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    o.ndc = vec2<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0);
    o.clip = vec4<f32>(o.ndc, 0.0, 1.0); // reverse-Z: 0 is the far plane
    return o;
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    let d = normalize(G.cam_fwd.xyz + G.cam_right.xyz * (i.ndc.x * G.cam_right.w) + G.cam_up.xyz * (i.ndc.y * G.cam_up.w));
    var c: vec3<f32>;
    if (d.y > 0.0) {
        c = sky_color(d, G.cam_pos.w);
    } else {
        // below the horizon (far sea haze)
        c = mix(HORIZON, vec3<f32>(0.20, 0.50, 0.75), smoothstep(0.0, 0.5, -d.y));
    }
    return vec4<f32>(c, 1.0);
}
