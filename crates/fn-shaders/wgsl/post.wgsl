// Post processing: bloom (threshold + downsample + upsample) and the final composite
// (tone mapping, colour grading, vignette, FXAA, damage / storm tints).

struct PostU {
    a: vec4<f32>,   // texel.xy (of the source), threshold, knee
    b: vec4<f32>,   // exposure, saturation, bloom strength, damage flash
    c: vec4<f32>,   // vignette, storm tint, time, fxaa (0/1)
    d: vec4<f32>,   // screen w, h, -, -
};
@group(0) @binding(0) var<uniform> P: PostU;
@group(0) @binding(1) var src_tex: texture_2d<f32>;
@group(0) @binding(2) var bloom_tex: texture_2d<f32>;
@group(0) @binding(3) var samp: sampler;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VOut {
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    var o: VOut;
    o.uv = vec2<f32>(x, 1.0 - y);
    o.clip = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

fn luma(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)); }

fn karis(c: vec3<f32>) -> f32 { return 1.0 / (1.0 + luma(c)); }

fn sample_src(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(src_tex, samp, uv, 0.0).rgb;
}

// 13-tap downsample (Call of Duty: Advanced Warfare style)
fn down13(uv: vec2<f32>, t: vec2<f32>) -> array<vec3<f32>, 5> {
    let a = sample_src(uv + t * vec2<f32>(-2.0, -2.0));
    let b = sample_src(uv + t * vec2<f32>(0.0, -2.0));
    let c = sample_src(uv + t * vec2<f32>(2.0, -2.0));
    let d = sample_src(uv + t * vec2<f32>(-2.0, 0.0));
    let e = sample_src(uv);
    let f = sample_src(uv + t * vec2<f32>(2.0, 0.0));
    let g = sample_src(uv + t * vec2<f32>(-2.0, 2.0));
    let h = sample_src(uv + t * vec2<f32>(0.0, 2.0));
    let i = sample_src(uv + t * vec2<f32>(2.0, 2.0));
    let j = sample_src(uv + t * vec2<f32>(-1.0, -1.0));
    let k = sample_src(uv + t * vec2<f32>(1.0, -1.0));
    let l = sample_src(uv + t * vec2<f32>(-1.0, 1.0));
    let m = sample_src(uv + t * vec2<f32>(1.0, 1.0));
    var g0 = (a + b + d + e) * 0.25;
    var g1 = (b + c + e + f) * 0.25;
    var g2 = (d + e + g + h) * 0.25;
    var g3 = (e + f + h + i) * 0.25;
    var g4 = (j + k + l + m) * 0.25;
    return array<vec3<f32>, 5>(g0, g1, g2, g3, g4);
}

@fragment
fn fs_bloom_first(v: VOut) -> @location(0) vec4<f32> {
    let g = down13(v.uv, P.a.xy);
    // Karis-weighted groups to suppress fireflies, then soft threshold
    let w0 = karis(g[0]); let w1 = karis(g[1]); let w2 = karis(g[2]); let w3 = karis(g[3]); let w4 = karis(g[4]);
    var c = (g[0] * w0 + g[1] * w1 + g[2] * w2 + g[3] * w3) * 0.125 + g[4] * w4 * 0.5;
    c = c / ((w0 + w1 + w2 + w3) * 0.125 + w4 * 0.5);
    let br = max(c.r, max(c.g, c.b));
    let thr = P.a.z;
    let knee = P.a.w;
    var soft = clamp(br - thr + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee + 0.00001);
    let contrib = max(soft, br - thr) / max(br, 0.00001);
    return vec4<f32>(c * contrib, 1.0);
}

@fragment
fn fs_bloom_down(v: VOut) -> @location(0) vec4<f32> {
    let g = down13(v.uv, P.a.xy);
    let c = (g[0] + g[1] + g[2] + g[3]) * 0.125 + g[4] * 0.5;
    return vec4<f32>(c, 1.0);
}

// tent upsample; result is additively blended into the larger mip
@fragment
fn fs_bloom_up(v: VOut) -> @location(0) vec4<f32> {
    let t = P.a.xy;
    var c = sample_src(v.uv + vec2<f32>(-t.x, -t.y)) * 1.0;
    c = c + sample_src(v.uv + vec2<f32>(0.0, -t.y)) * 2.0;
    c = c + sample_src(v.uv + vec2<f32>(t.x, -t.y)) * 1.0;
    c = c + sample_src(v.uv + vec2<f32>(-t.x, 0.0)) * 2.0;
    c = c + sample_src(v.uv) * 4.0;
    c = c + sample_src(v.uv + vec2<f32>(t.x, 0.0)) * 2.0;
    c = c + sample_src(v.uv + vec2<f32>(-t.x, t.y)) * 1.0;
    c = c + sample_src(v.uv + vec2<f32>(0.0, t.y)) * 2.0;
    c = c + sample_src(v.uv + vec2<f32>(t.x, t.y)) * 1.0;
    return vec4<f32>(c / 16.0 * P.a.z, 1.0);
}

// ---- composite -----------------------------------------------------------------
fn aces(x: vec3<f32>) -> vec3<f32> {
    return clamp((x * (2.51 * x + vec3<f32>(0.03))) / (x * (2.43 * x + vec3<f32>(0.59)) + vec3<f32>(0.14)), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn grade(hdr: vec3<f32>) -> vec3<f32> {
    var c = aces(hdr * P.b.x);
    let l = luma(c);
    c = mix(vec3<f32>(l), c, P.b.y);
    c = pow(c, vec3<f32>(1.0 / 2.2));
    c = (c - vec3<f32>(0.5)) * 1.07 + vec3<f32>(0.5);
    return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn fetch_graded(uv: vec2<f32>) -> vec3<f32> {
    let h = textureSampleLevel(src_tex, samp, uv, 0.0).rgb;
    let b = textureSampleLevel(bloom_tex, samp, uv, 0.0).rgb;
    return grade(h + b * P.b.z);
}

@fragment
fn fs_composite(v: VOut) -> @location(0) vec4<f32> {
    var col: vec3<f32>;
    if (P.c.w > 0.5) {
        // FXAA-lite on the graded image
        let t = vec2<f32>(P.d.z, P.d.w);
        let m = fetch_graded(v.uv);
        let nw = fetch_graded(v.uv + vec2<f32>(-t.x, -t.y));
        let ne = fetch_graded(v.uv + vec2<f32>(t.x, -t.y));
        let sw = fetch_graded(v.uv + vec2<f32>(-t.x, t.y));
        let se = fetch_graded(v.uv + vec2<f32>(t.x, t.y));
        let lm = luma(m);
        let lnw = luma(nw); let lne = luma(ne); let lsw = luma(sw); let lse = luma(se);
        let lmin = min(lm, min(min(lnw, lne), min(lsw, lse)));
        let lmax = max(lm, max(max(lnw, lne), max(lsw, lse)));
        var dir = vec2<f32>(-((lnw + lne) - (lsw + lse)), ((lnw + lsw) - (lne + lse)));
        let reduce = max((lnw + lne + lsw + lse) * 0.25 * 0.125, 1.0 / 128.0);
        let rcp = 1.0 / (min(abs(dir.x), abs(dir.y)) + reduce);
        dir = clamp(dir * rcp, vec2<f32>(-8.0), vec2<f32>(8.0)) * t;
        let a = 0.5 * (fetch_graded(v.uv + dir * (1.0 / 3.0 - 0.5)) + fetch_graded(v.uv + dir * (2.0 / 3.0 - 0.5)));
        let b = a * 0.5 + 0.25 * (fetch_graded(v.uv + dir * -0.5) + fetch_graded(v.uv + dir * 0.5));
        let lb = luma(b);
        if (lb < lmin || lb > lmax) { col = a; } else { col = b; }
    } else {
        col = fetch_graded(v.uv);
    }
    // vignette
    let q = v.uv - vec2<f32>(0.5);
    let r = length(q * vec2<f32>(1.0, 0.85)) * 1.45;
    col = col * (1.0 - P.c.x * smoothstep(0.55, 1.25, r));
    // damage flash: red edges
    col = col + vec3<f32>(0.75, 0.02, 0.0) * P.b.w * smoothstep(0.25, 1.1, r);
    col = mix(col, col * vec3<f32>(1.0, 0.55, 0.5) + vec3<f32>(0.2, 0.0, 0.0), P.b.w * 0.25);
    // storm: purple desaturation + edge glow while the player is inside it
    let storm = P.c.y;
    if (storm > 0.0) {
        let l = luma(col);
        let purple = vec3<f32>(l) * vec3<f32>(0.75, 0.55, 1.05);
        col = mix(col, purple, storm * 0.55);
        col = col + vec3<f32>(0.38, 0.05, 0.75) * storm * smoothstep(0.3, 1.2, r) * (0.7 + 0.3 * sin(P.c.z * 4.0));
    }
    // light dithering to hide banding in the sky gradient
    let n = fract(sin(dot(v.clip.xy, vec2<f32>(12.9898, 78.233))) * 43758.5453) - 0.5;
    col = col + vec3<f32>(n / 255.0);
    return vec4<f32>(col, 1.0);
}
