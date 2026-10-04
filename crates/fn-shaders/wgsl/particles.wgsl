// Billboard particles, stretched sparks/tracers and vertical loot beams.
// Output is premultiplied so the same shader serves alpha and additive blending.

struct PIn {
    @location(0) a: vec4<f32>,     // pos.xyz, size
    @location(1) b: vec4<f32>,     // axis.xyz, length
    @location(2) color: vec4<f32>,
    @location(3) c: vec4<f32>,     // shape, rotation, glow/softness, seed
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,    // -1..1 across, 0..1 along for axis shapes (stored in y as -1..1 too)
    @location(1) color: vec4<f32>,
    @location(2) c: vec4<f32>,
    @location(3) wpos: vec3<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, p: PIn) -> VOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0));
    let local = corners[vi];
    let shape = i32(p.c.x + 0.5);
    var world: vec3<f32>;
    if (shape == 2 || shape == 4 || shape == 5) {
        let axis = normalize(p.b.xyz);
        let view_dir = normalize(G.cam_pos.xyz - p.a.xyz);
        var side = cross(axis, view_dir);
        let sl = length(side);
        if (sl < 0.001) { side = G.cam_right.xyz; } else { side = side / sl; }
        var t = local.y * 0.5;            // centred for sparks
        if (shape != 2) { t = local.y * 0.5 + 0.5; } // from base to tip for beams / tracers
        world = p.a.xyz + axis * (p.b.w * t) + side * (local.x * p.a.w);
    } else {
        let cs = cos(p.c.y);
        let sn = sin(p.c.y);
        let rl = vec2<f32>(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
        world = p.a.xyz + (G.cam_right.xyz * rl.x + G.cam_up.xyz * rl.y) * p.a.w;
    }
    var o: VOut;
    o.clip = G.view_proj * vec4<f32>(world, 1.0);
    o.uv = local;
    o.color = p.color;
    o.c = p.c;
    o.wpos = world;
    return o;
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    let shape = i32(i.c.x + 0.5);
    let r = length(i.uv);
    var a = 0.0;
    switch (shape) {
        case 0: { // soft glow
            a = exp(-r * r * 3.2) * smoothstep(1.0, 0.8, r);
        }
        case 1: { // smoke puff
            let n = fbm2(i.uv * 1.8 + vec2<f32>(i.c.w * 7.0, i.c.w * 3.0));
            a = smoothstep(1.0, 0.25, r + (n - 0.5) * 0.9) * 0.9;
        }
        case 2: { // spark (stretched, centred)
            let along = abs(i.uv.y);
            a = exp(-i.uv.x * i.uv.x * 7.0) * (1.0 - along * along * along);
        }
        case 3: { // expanding ring
            a = smoothstep(0.16, 0.0, abs(r - 0.82)) * smoothstep(1.0, 0.7, r);
        }
        case 4: { // loot beam (base at uv.y = -1, tip at +1)
            let v = i.uv.y * 0.5 + 0.5;
            let core = pow(max(1.0 - abs(i.uv.x), 0.0), 1.6);
            let flick = 0.88 + 0.12 * sin(G.cam_pos.w * 3.0 + i.c.w * 6.28 + v * 8.0);
            a = core * pow(1.0 - v, 0.9) * flick;
        }
        case 5: { // tracer: bright head at the tip
            let v = i.uv.y * 0.5 + 0.5;
            a = exp(-i.uv.x * i.uv.x * 5.0) * smoothstep(0.0, 1.0, v) * (0.35 + 0.65 * v);
        }
        case 6: { // 4-point star / muzzle flash
            let x = abs(i.uv.x);
            let y = abs(i.uv.y);
            let star = max(exp(-(x * 9.0 + y * 1.2)), exp(-(y * 9.0 + x * 1.2)));
            a = max(star, exp(-r * r * 5.0) * 0.8) * smoothstep(1.0, 0.85, r);
        }
        default: {
            a = smoothstep(1.0, 0.0, r);
        }
    }
    a = a * i.color.a;
    if (a < 0.003) { discard; }
    var rgb = i.color.rgb;
    rgb = apply_fog(rgb, i.wpos);
    return vec4<f32>(rgb * a, a);
}
