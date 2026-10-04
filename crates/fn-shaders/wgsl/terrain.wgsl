// Terrain: painted vertex colours + splat map (roads, lawns, fields) + procedural detail.

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) col: vec4<f32>,
    @location(3) attr: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) wpos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) col: vec3<f32>,
};

@vertex
fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    o.clip = G.view_proj * vec4<f32>(v.pos, 1.0);
    o.wpos = v.pos;
    o.nrm = v.nrm;
    o.col = v.col.rgb;
    return o;
}

fn crisp(v: f32) -> f32 {
    let w = max(fwidth(v) * 0.8, 0.03);
    return smoothstep(0.5 - w, 0.5 + w, v);
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    var albedo = srgb_to_linear(i.col);
    let N = normalize(i.nrm);
    let uv = (i.wpos.xz + vec2<f32>(G.world.x)) / (G.world.x * 2.0);
    let sp_raw = textureSampleLevel(splat_tex, lin_samp, uv, 0.0);
    // roads / fields are stored as soft ramps; threshold them for crisp anti-aliased edges
    var sp = sp_raw;
    sp.r = crisp(sp_raw.r);
    sp.g = crisp(sp_raw.g);
    sp.a = crisp(sp_raw.a);
    sp.b = smoothstep(0.15, 0.85, sp_raw.b);

    let dist = distance(i.wpos, G.cam_pos.xyz);
    let near = 1.0 - smoothstep(40.0, 240.0, dist);
    let n_fine = fbm2(i.wpos.xz * 1.1);
    let n_mid = vnoise(i.wpos.xz * 0.23 + vec2<f32>(5.0, 9.0));
    // painterly brightness variation + fine "grass blade" streaks near the camera
    let streak = vnoise(vec2<f32>(i.wpos.x * 5.0 + i.wpos.z * 1.3, i.wpos.z * 5.0 - i.wpos.x * 0.7));
    var vary = 1.0 + (n_mid - 0.5) * 0.16 + (n_fine - 0.5) * 0.22 * near + (streak - 0.5) * 0.10 * near;
    albedo = albedo * vary;

    // splat layers
    let dirt = vec3<f32>(0.46, 0.34, 0.20) * (0.88 + n_fine * 0.3);
    let asphalt = vec3<f32>(0.080, 0.085, 0.095) * (0.85 + n_fine * 0.4);
    let lawn = srgb_to_linear(vec3<f32>(0.43, 0.66, 0.31)) * (0.94 + n_mid * 0.14);
    let soil = srgb_to_linear(vec3<f32>(0.78, 0.66, 0.34));
    // farmland rows
    let rows = 0.5 + 0.5 * sin(i.wpos.x * 3.2 + sin(i.wpos.z * 0.4) * 0.5);
    let field = mix(soil, soil * vec3<f32>(0.78, 0.62, 0.45), rows * 0.65);
    albedo = mix(albedo, lawn, sp.b * 0.85);
    albedo = mix(albedo, field, sp.a);
    albedo = mix(albedo, dirt, sp.r);
    albedo = mix(albedo, asphalt, sp.g);

    // wet sand darkening near the waterline
    let wet = 1.0 - smoothstep(0.0, 0.9, i.wpos.y - G.world.w);
    albedo = albedo * mix(1.0, 0.72, wet);

    // A toy baseplate uses the same collision terrain, with anti-aliased stud
    // rings and plate seams that fade before their detail can shimmer.
    if (G.misc.w > 0.5) {
        let tile = fract(i.wpos.xz / 4.0);
        let aa = max(fwidth(tile), vec2<f32>(0.002));
        let seam = 1.0 - smoothstep(0.0, aa.x * 2.0 + 0.016, min(tile.x, 1.0 - tile.x)) * smoothstep(0.0, aa.y * 2.0 + 0.016, min(tile.y, 1.0 - tile.y));
        let stud = length(fract(i.wpos.xz / 0.8) - vec2<f32>(0.5));
        let edge = max(fwidth(stud), 0.015);
        let ring = smoothstep(0.24 - edge, 0.24 + edge, stud) * (1.0 - smoothstep(0.30 - edge, 0.30 + edge, stud));
        let detail = 1.0 - smoothstep(18.0, 65.0, dist);
        albedo *= 1.0 - (seam * 0.16 + ring * 0.18) * detail;
        albedo += vec3<f32>(0.025) * (1.0 - smoothstep(0.0, 0.24, stud)) * detail;
    }

    var col = light_surface(albedo, N, i.wpos, 1.0, 0.0, 1.0, 0.0);
    col = apply_storm(col, i.wpos);
    col = apply_fog(col, i.wpos);
    return vec4<f32>(col, 1.0);
}
