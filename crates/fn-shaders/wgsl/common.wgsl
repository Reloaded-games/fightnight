// ---------------------------------------------------------------------------
// FightNight shared shader prelude: globals, noise, sky, fog, shadows, lighting.
// This text is prepended to every scene shader. Colours are linear HDR.
// ---------------------------------------------------------------------------

struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    cascade_vp: array<mat4x4<f32>, 3>,
    cam_pos: vec4<f32>,        // xyz, w = time (seconds)
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    cam_fwd: vec4<f32>,
    sun_dir: vec4<f32>,        // unit vector pointing TOWARD the sun
    sun_color: vec4<f32>,
    sky_color: vec4<f32>,      // hemisphere ambient (up)
    ground_color: vec4<f32>,   // hemisphere ambient (down)
    fog_params: vec4<f32>,     // x density, y height falloff, z sun scatter, w -
    storm: vec4<f32>,          // x centre.x, y centre.z, z radius, w strength
    cascade_splits: vec4<f32>, // far distance of cascade 0,1,2 ; w = distance where shadows are fully faded
    shadow_info: vec4<f32>,    // x texel size (m) of cascade 0 hint, y enabled, z normal offset scale, w -
    screen: vec4<f32>,         // width, height, 1/width, 1/height
    post: vec4<f32>,           // exposure, saturation, bloom strength, damage flash
    world: vec4<f32>,          // half extent, heightmap n, cell size, sea level
    misc: vec4<f32>,           // x wind strength, y storm time, z player in storm (0/1), w -
};

@group(0) @binding(0) var<uniform> G: Globals;
@group(0) @binding(1) var shadow_map: texture_depth_2d_array;
@group(0) @binding(2) var shadow_samp: sampler_comparison;
@group(0) @binding(3) var height_tex: texture_2d<f32>;
@group(0) @binding(4) var splat_tex: texture_2d<f32>;
@group(0) @binding(5) var lin_samp: sampler;

const PI: f32 = 3.14159265;
const HORIZON: vec3<f32> = vec3<f32>(0.60, 0.80, 1.00);

fn saturate(x: f32) -> f32 { return clamp(x, 0.0, 1.0); }
fn saturate3(x: vec3<f32>) -> vec3<f32> { return clamp(x, vec3<f32>(0.0), vec3<f32>(1.0)); }

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), c > vec3<f32>(0.04045));
}

// ---- hashing / noise --------------------------------------------------------
fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 456.21));
    q = q + dot(q, q + 45.32);
    return fract(q.x * q.y);
}
fn hash22(p: vec2<f32>) -> vec2<f32> {
    let a = hash21(p);
    return vec2<f32>(a, hash21(p + a * 19.19));
}
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
fn fbm2(p_in: vec2<f32>) -> f32 {
    var p = p_in;
    var s = 0.0;
    var a = 0.5;
    for (var i = 0; i < 4; i = i + 1) {
        s = s + vnoise(p) * a;
        p = p * 2.03 + vec2<f32>(17.1, 9.2);
        a = a * 0.5;
    }
    return s;
}

// Interleaved gradient noise for dithering.
fn ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

// ---- terrain sampling (matches the CPU triangulation) --------------------------
fn height_at_texel(i: i32, j: i32) -> f32 {
    let n = i32(G.world.y);
    return textureLoad(height_tex, vec2<i32>(clamp(i, 0, n - 1), clamp(j, 0, n - 1)), 0).r;
}
fn terrain_height(p: vec2<f32>) -> f32 {
    let half = G.world.x;
    let n = G.world.y;
    let cell = G.world.z;
    let g = clamp((p + vec2<f32>(half)) / cell, vec2<f32>(0.0), vec2<f32>(n - 1.001));
    let fi = floor(g);
    let f = g - fi;
    let i = i32(fi.x);
    let j = i32(fi.y);
    let h00 = height_at_texel(i, j);
    let h10 = height_at_texel(i + 1, j);
    let h01 = height_at_texel(i, j + 1);
    let h11 = height_at_texel(i + 1, j + 1);
    if (f.y >= f.x) {
        return h00 + (h11 - h01) * f.x + (h01 - h00) * f.y;
    }
    return h00 + (h10 - h00) * f.x + (h11 - h10) * f.y;
}

// ---- sky ---------------------------------------------------------------------
fn sky_gradient(d: vec3<f32>) -> vec3<f32> {
    let h = saturate(d.y);
    let zenith = vec3<f32>(0.030, 0.200, 0.780);
    let mid = vec3<f32>(0.120, 0.430, 0.960);
    var c = mix(HORIZON, mid, smoothstep(0.0, 0.25, h));
    c = mix(c, zenith, smoothstep(0.2, 0.9, h));
    return c;
}

fn cloud_density(p: vec2<f32>) -> f32 {
    let n = fbm2(p);
    return smoothstep(0.50, 0.78, n);
}

// Returns rgb in .xyz, coverage alpha in .w
fn clouds(d: vec3<f32>, t: f32) -> vec4<f32> {
    if (d.y < 0.015) { return vec4<f32>(0.0); }
    let k = 1.0 / (d.y + 0.08);
    let p = d.xz * k * 1.15 + vec2<f32>(t * 0.012, t * 0.004) + vec2<f32>(3.7, 1.3);
    let dens = cloud_density(p);
    if (dens <= 0.001) { return vec4<f32>(0.0); }
    let sun2 = normalize(vec2<f32>(G.sun_dir.x, G.sun_dir.z) + vec2<f32>(0.0001));
    let dens2 = cloud_density(p + sun2 * 0.18);
    let lit = saturate(0.55 + (dens - dens2) * 2.6);
    let shadow_col = vec3<f32>(0.62, 0.72, 0.92);
    let lit_col = vec3<f32>(1.15, 1.10, 1.04);
    let col = mix(shadow_col, lit_col, lit);
    let fade = smoothstep(0.015, 0.20, d.y);
    return vec4<f32>(col, dens * fade);
}

fn sun_disc(d: vec3<f32>) -> vec3<f32> {
    let s = saturate(dot(d, G.sun_dir.xyz));
    let glow = pow(s, 5.0) * 0.12 + pow(s, 48.0) * 0.45;
    let disc = smoothstep(0.9993, 0.9997, s) * 9.0;
    return vec3<f32>(1.0, 0.86, 0.60) * (glow + disc);
}

fn sky_color(d: vec3<f32>, t: f32) -> vec3<f32> {
    var c = sky_gradient(d) + sun_disc(d);
    let cl = clouds(d, t);
    c = mix(c, cl.xyz, cl.w);
    return c;
}

// Cheap version used for reflections (no cloud noise).
fn sky_color_cheap(d: vec3<f32>) -> vec3<f32> {
    return sky_gradient(d) + sun_disc(d) * 0.6;
}

// ---- fog & storm -------------------------------------------------------------
fn fog_color_for(dir: vec3<f32>) -> vec3<f32> {
    let s = pow(saturate(dot(dir, G.sun_dir.xyz)), 6.0);
    return HORIZON + vec3<f32>(1.0, 0.78, 0.45) * s * G.fog_params.z;
}

fn apply_fog(color: vec3<f32>, wpos: vec3<f32>) -> vec3<f32> {
    let v = wpos - G.cam_pos.xyz;
    let dist = length(v);
    let dir = v / max(dist, 0.001);
    let hf = exp(-max(wpos.y, 0.0) * G.fog_params.y);
    let amt = 1.0 - exp(-dist * G.fog_params.x * (0.35 + 0.65 * hf));
    return mix(color, fog_color_for(dir), saturate(amt));
}

fn apply_storm(color: vec3<f32>, wpos: vec3<f32>) -> vec3<f32> {
    if (G.storm.z <= 0.0) { return color; }
    let r = distance(wpos.xz, G.storm.xy);
    let outside = smoothstep(G.storm.z - 1.5, G.storm.z + 5.0, r);
    if (outside <= 0.0) { return color; }
    let t = G.misc.y;
    let n = vnoise(wpos.xz * 0.06 + vec2<f32>(t * 0.15, -t * 0.1));
    let lum = dot(color, vec3<f32>(0.299, 0.587, 0.114));
    let tinted = vec3<f32>(lum) * vec3<f32>(0.55, 0.30, 0.95) * 0.75 + vec3<f32>(0.10, 0.015, 0.24) * (0.7 + 0.6 * n);
    return mix(color, tinted, outside * 0.88 * G.storm.w);
}

// ---- shadows -----------------------------------------------------------------
fn shadow_tap(uv: vec2<f32>, layer: i32, depth: f32) -> f32 {
    return textureSampleCompareLevel(shadow_map, shadow_samp, uv, layer, depth);
}

fn shadow_factor(wpos: vec3<f32>, N: vec3<f32>, view_depth: f32) -> f32 {
    if (G.shadow_info.y < 0.5) { return 1.0; }
    var layer = 2;
    if (view_depth < G.cascade_splits.x) { layer = 0; }
    else if (view_depth < G.cascade_splits.y) { layer = 1; }
    if (view_depth > G.cascade_splits.w) { return 1.0; }
    // Larger cascades have larger texels, so push further along the normal.
    let off = G.shadow_info.z * (1.0 + f32(layer) * 1.7);
    let wp = wpos + N * off;
    let lp = G.cascade_vp[layer] * vec4<f32>(wp, 1.0);
    let ndc = lp.xyz / lp.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || ndc.z > 1.0) { return 1.0; }
    let texel = 1.0 / 2048.0;
    let r = texel * (1.0 + f32(layer) * 0.0);
    var s = 0.0;
    // 3x3 PCF (each tap is itself a 2x2 hardware comparison)
    s = s + shadow_tap(uv + vec2<f32>(-r, -r), layer, ndc.z);
    s = s + shadow_tap(uv + vec2<f32>(0.0, -r), layer, ndc.z);
    s = s + shadow_tap(uv + vec2<f32>(r, -r), layer, ndc.z);
    s = s + shadow_tap(uv + vec2<f32>(-r, 0.0), layer, ndc.z);
    s = s + shadow_tap(uv, layer, ndc.z);
    s = s + shadow_tap(uv + vec2<f32>(r, 0.0), layer, ndc.z);
    s = s + shadow_tap(uv + vec2<f32>(-r, r), layer, ndc.z);
    s = s + shadow_tap(uv + vec2<f32>(0.0, r), layer, ndc.z);
    s = s + shadow_tap(uv + vec2<f32>(r, r), layer, ndc.z);
    s = s / 9.0;
    // fade out at the far end so the cut-off is not visible
    let fade = 1.0 - smoothstep(G.cascade_splits.w * 0.82, G.cascade_splits.w, view_depth);
    return mix(1.0, s, fade);
}

// ---- lighting ----------------------------------------------------------------
// Soft wrapped sun light + hemisphere ambient, tuned for a bright cartoon-shooter look.
fn light_surface(albedo: vec3<f32>, N: vec3<f32>, wpos: vec3<f32>, ao: f32, spec: f32, shininess: f32, rim: f32) -> vec3<f32> {
    let V = normalize(G.cam_pos.xyz - wpos);
    let L = G.sun_dir.xyz;
    let ndl = dot(N, L);
    let view_depth = dot(wpos - G.cam_pos.xyz, G.cam_fwd.xyz);
    let sh = shadow_factor(wpos, N, view_depth);
    let wrap = saturate((ndl + 0.30) / 1.30);
    let direct = G.sun_color.rgb * (wrap * sh);
    let hemi = mix(G.ground_color.rgb, G.sky_color.rgb, N.y * 0.5 + 0.5);
    // a touch of warm bounce filling the shadow side
    let bounce = G.sun_color.rgb * 0.06 * saturate(-ndl * 0.5 + 0.5) * (1.0 - sh * 0.5);
    var col = albedo * (direct + (hemi + bounce) * ao);
    if (spec > 0.0) {
        let H = normalize(L + V);
        let ns = pow(saturate(dot(N, H)), shininess);
        col = col + G.sun_color.rgb * (ns * spec * sh * step(0.0, ndl));
    }
    let fres = pow(1.0 - saturate(dot(N, V)), 3.0);
    col = col + G.sky_color.rgb * (fres * rim * ao);
    return col;
}
