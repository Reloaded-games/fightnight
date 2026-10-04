// Water: depth-based colour from the heightmap, fresnel sky reflection, foam and glints.
// One instance per body: (centre.x, level, centre.z, half extent).

struct VIn {
    @location(0) body: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) wpos: vec3<f32>,
    @location(1) level: f32,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, v: VIn) -> VOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0));
    let c = corners[vi];
    var o: VOut;
    let p = vec3<f32>(v.body.x + c.x * v.body.w, v.body.y, v.body.z + c.y * v.body.w);
    o.wpos = p;
    o.level = v.body.y;
    o.clip = G.view_proj * vec4<f32>(p, 1.0);
    return o;
}

fn wave_height(p: vec2<f32>, t: f32) -> f32 {
    let a = vnoise(p * 0.35 + vec2<f32>(t * 0.35, t * 0.2));
    let b = vnoise(p * 0.9 + vec2<f32>(-t * 0.5, t * 0.3));
    let c = vnoise(p * 2.3 + vec2<f32>(t * 0.9, -t * 0.7));
    return a * 0.6 + b * 0.3 + c * 0.1;
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    let t = G.cam_pos.w;
    let ground = select(-40.0, terrain_height(i.wpos.xz), abs(i.wpos.x) < G.world.x - 2.0 && abs(i.wpos.z) < G.world.x - 2.0);
    let depth = i.level - ground;
    if (depth < 0.0) { discard; }

    let V = normalize(G.cam_pos.xyz - i.wpos);
    // perturbed normal from the height-field of waves
    let e = 0.15;
    let h0 = wave_height(i.wpos.xz, t);
    let hx = wave_height(i.wpos.xz + vec2<f32>(e, 0.0), t);
    let hz = wave_height(i.wpos.xz + vec2<f32>(0.0, e), t);
    let amp = 0.55 * (0.4 + 0.6 * smoothstep(0.0, 2.5, depth));
    var N = normalize(vec3<f32>(-(hx - h0) / e * amp, 1.0, -(hz - h0) / e * amp));
    // soften waves in the distance to avoid shimmer
    let dist = distance(i.wpos, G.cam_pos.xyz);
    N = normalize(mix(N, vec3<f32>(0.0, 1.0, 0.0), smoothstep(80.0, 500.0, dist)));

    let shallow = vec3<f32>(0.10, 0.62, 0.62);
    let mid = vec3<f32>(0.03, 0.34, 0.62);
    let deep = vec3<f32>(0.010, 0.10, 0.34);
    var body = mix(shallow, mid, smoothstep(0.2, 3.0, depth));
    body = mix(body, deep, smoothstep(3.0, 14.0, depth));

    // light scattering through the water column
    let sun_up = saturate(G.sun_dir.y);
    body = body * (0.55 + 0.9 * sun_up);

    let fres = 0.03 + 0.97 * pow(1.0 - saturate(dot(N, V)), 5.0);
    let R = reflect(-V, N);
    let refl = sky_color_cheap(vec3<f32>(R.x, abs(R.y), R.z));
    var col = mix(body, refl, saturate(fres * 0.85 + 0.04));

    // sun glints
    let H = normalize(G.sun_dir.xyz + V);
    let glint = pow(saturate(dot(N, H)), 380.0) * 5.0 + pow(saturate(dot(N, H)), 60.0) * 0.25;
    col = col + vec3<f32>(1.0, 0.92, 0.75) * glint * select(0.0, 1.0, G.sun_dir.y > 0.0);

    // foam along shorelines + travelling crest lines
    let n1 = vnoise(i.wpos.xz * 1.7 + vec2<f32>(t * 0.4, 0.0));
    let shore = 1.0 - smoothstep(0.0, 1.1 + n1 * 0.9, depth);
    let ripple = 0.5 + 0.5 * sin(depth * 5.0 - t * 1.6 + n1 * 4.0);
    let foam = saturate(shore * (0.55 + ripple * 0.55));
    col = mix(col, vec3<f32>(1.0), foam * 0.9);

    var alpha = saturate(depth / 0.55);
    alpha = max(alpha * 0.93, foam);
    col = apply_storm(col, i.wpos);
    col = apply_fog(col, i.wpos);
    return vec4<f32>(col, alpha);
}
