// The storm wall: a tall purple cylinder around the safe zone.

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) wpos: vec3<f32>,
    @location(1) ang: f32,
    @location(2) h: f32,
};

const SEGS: f32 = 160.0;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VOut {
    var cx = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    var cy = array<f32, 6>(0.0, 0.0, 1.0, 0.0, 1.0, 1.0);
    let seg = f32(vi / 6u);
    let k = vi % 6u;
    let a = (seg + cx[k]) / SEGS * 6.2831853;
    let h = cy[k] * 1800.0 - 30.0;
    let p = vec3<f32>(G.storm.x + cos(a) * G.storm.z, h, G.storm.y + sin(a) * G.storm.z);
    var o: VOut;
    o.clip = G.view_proj * vec4<f32>(p, 1.0);
    o.wpos = p;
    o.ang = a;
    o.h = h;
    return o;
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    if (G.storm.z <= 0.0) { discard; }
    let t = G.misc.y;
    let arc = i.ang * G.storm.z * 0.015;
    let n1 = fbm2(vec2<f32>(arc * 1.3 + t * 0.07, i.h * 0.012 - t * 0.35));
    let n2 = vnoise(vec2<f32>(arc * 4.0 - t * 0.2, i.h * 0.05 - t * 0.9));
    let bands = 0.55 + 0.45 * n1 + 0.25 * n2;
    let low = 1.0 - smoothstep(0.0, 380.0, max(i.h, 0.0));
    // the column dissolves into the sky (no visible rim) and is only visible when you are near it
    let sky_fade = 1.0 - smoothstep(250.0, 1500.0, max(i.h, 0.0));
    let dcam = distance(i.wpos.xz, G.cam_pos.xz);
    let near = 1.0 - smoothstep(300.0, 850.0, dcam);
    let alpha = (0.30 + 0.38 * bands) * (0.35 + 0.65 * low) * sky_fade * near;
    // deep violet energy curtain; the bright streaks stay well below white so the tone mapper keeps the hue
    let base = vec3<f32>(0.30, 0.05, 0.78);
    let hot = vec3<f32>(0.62, 0.20, 1.05);
    var rgb = mix(base, hot, n2 * 0.8) * (0.45 + 0.75 * bands);
    // brighter where the wall meets the ground
    rgb = rgb + hot * 0.5 * (1.0 - smoothstep(0.0, 18.0, abs(i.h - terrain_height(i.wpos.xz))));
    rgb = apply_fog(rgb, i.wpos);
    return vec4<f32>(rgb * alpha, alpha);
}
