// Instanced lit meshes: buildings, trees, characters, weapons, pickups, building pieces.
// Per-vertex material id selects procedural surface detail.

struct VIn {
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

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) wpos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) col: vec4<f32>,
    @location(3) attr: vec4<f32>,
    @location(4) ipar: vec4<f32>,
    @location(5) lpos: vec3<f32>,
    @location(6) tint: vec4<f32>,
};

fn wind_offset(origin: vec2<f32>, sway: f32) -> vec3<f32> {
    let t = G.cam_pos.w;
    let ph = origin.x * 0.31 + origin.y * 0.23;
    let gust = sin(t * 1.3 + ph) * 0.6 + sin(t * 2.7 + ph * 1.7) * 0.4;
    let g2 = cos(t * 1.1 + ph * 1.3) * 0.6;
    return vec3<f32>(gust, 0.0, g2) * (sway * G.misc.x * 0.22);
}

@vertex
fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    let p4 = vec4<f32>(v.pos, 1.0);
    var wp = vec3<f32>(dot(v.m0, p4), dot(v.m1, p4), dot(v.m2, p4));
    let c0 = vec3<f32>(v.m0.x, v.m1.x, v.m2.x);
    let c1 = vec3<f32>(v.m0.y, v.m1.y, v.m2.y);
    let c2 = vec3<f32>(v.m0.z, v.m1.z, v.m2.z);
    let det = dot(c0, cross(c1, c2));
    let cof = mat3x3<f32>(cross(c1, c2), cross(c2, c0), cross(c0, c1));
    var n = normalize(cof * v.nrm);
    if (det < 0.0) { n = -n; }
    let sway = v.attr.z;
    if (sway > 0.0) {
        let origin = vec2<f32>(v.m0.w, v.m2.w);
        wp = wp + wind_offset(origin, sway);
    }
    o.clip = G.view_proj * vec4<f32>(wp, 1.0);
    o.wpos = wp;
    o.nrm = n;
    o.col = v.col;
    o.attr = v.attr;
    o.ipar = v.ipar;
    o.lpos = v.pos;
    o.tint = v.icol;
    return o;
}

// ---- procedural surface patterns ------------------------------------------------
fn planks(coord: vec2<f32>, width: f32) -> f32 {
    // returns 0..1 where 0 is a groove between boards
    let b = coord.x / width;
    let f = fract(b);
    let id = floor(b);
    let groove = smoothstep(0.0, 0.06, f) * smoothstep(1.0, 0.94, f);
    let grain = vnoise(vec2<f32>(coord.y * 0.7 + id * 3.1, id * 7.3)) * 0.25;
    return groove * (0.78 + grain + hash21(vec2<f32>(id, 1.0)) * 0.18);
}

fn bricks(coord: vec2<f32>) -> f32 {
    let row = floor(coord.y / 0.2);
    let off = select(0.0, 0.5, (i32(row) & 1) == 1);
    let bx = fract(coord.x / 0.45 + off);
    let by = fract(coord.y / 0.2);
    let mortar = smoothstep(0.0, 0.07, bx) * smoothstep(1.0, 0.93, bx) * smoothstep(0.0, 0.14, by) * smoothstep(1.0, 0.86, by);
    let id = vec2<f32>(floor(coord.x / 0.45 + off), row);
    return mix(0.62, 0.88 + hash21(id) * 0.18, mortar);
}

fn shingles(coord: vec2<f32>) -> f32 {
    let row = floor(coord.y / 0.28);
    let off = select(0.0, 0.5, (i32(row) & 1) == 1);
    let sx = fract(coord.x / 0.4 + off);
    let sy = fract(coord.y / 0.28);
    let edge = smoothstep(0.0, 0.12, sy) * smoothstep(0.0, 0.05, sx) * smoothstep(1.0, 0.95, sx);
    let id = vec2<f32>(floor(coord.x / 0.4 + off), row);
    return mix(0.58, 0.9 + hash21(id) * 0.16, edge);
}

fn dominant_uv(p: vec3<f32>, n: vec3<f32>) -> vec2<f32> {
    let a = abs(n);
    if (a.y > a.x && a.y > a.z) { return p.xz; }
    if (a.x > a.z) { return vec2<f32>(p.z, p.y); }
    return vec2<f32>(p.x, p.y);
}

// Build-piece materials: 0 wood, 1 stone, 2 metal.
fn build_pattern(kind: i32, lp: vec3<f32>, wp: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    // returns albedo multiplier (rgb) in .xyz
    let uv = dominant_uv(wp, n);
    var m = vec3<f32>(1.0);
    if (kind == 0) {
        let p = planks(vec2<f32>(uv.x + uv.y * 0.0, uv.y), 0.34);
        m = vec3<f32>(p);
    } else if (kind == 1) {
        m = vec3<f32>(bricks(vec2<f32>(uv.x, uv.y)));
    } else {
        let f = fract(uv * vec2<f32>(0.5, 0.5));
        let seam = smoothstep(0.0, 0.03, f.x) * smoothstep(1.0, 0.97, f.x) * smoothstep(0.0, 0.03, f.y) * smoothstep(1.0, 0.97, f.y);
        m = vec3<f32>(mix(0.62, 1.0, seam));
    }
    return m;
}

@fragment
fn fs_main(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    if (i.ipar.y < 0.999) {
        if (i.ipar.y < ign(i.clip.xy)) { discard; }
    }
    var N = normalize(i.nrm);
    if (!front) { N = -N; }
    let mat_id = i32(i.attr.y * 255.0 + 0.5);
    // vertex alpha is the tint weight: 1 follows the instance colour, 0 keeps the baked colour
    var albedo = srgb_to_linear(i.col.rgb) * mix(vec3<f32>(1.0), i.tint.rgb, i.col.a);
    var ao = i.attr.x;
    var spec = i.attr.w;
    var shin = 28.0;
    var rim = 0.10;
    var emissive = 0.0;
    let wp = i.wpos;
    let uv = dominant_uv(wp, N);

    switch (mat_id) {
        case 1, 2: { // foliage / grass blades
            let v = vnoise(wp.xz * 2.3 + vec2<f32>(wp.y * 1.7, 0.0));
            albedo = albedo * (0.82 + 0.36 * v);
            rim = 0.35;
            // fake translucency when backlit
            let V = normalize(G.cam_pos.xyz - wp);
            let tr = pow(saturate(dot(-V, G.sun_dir.xyz)), 3.0) * 0.45;
            albedo = albedo * (1.0 + tr);
        }
        case 3: { // wood planks
            albedo = albedo * planks(vec2<f32>(uv.x, uv.y), 0.3);
            spec = 0.04;
        }
        case 4: { // brick
            albedo = albedo * bricks(uv);
        }
        case 5: { // roof shingles
            albedo = albedo * shingles(vec2<f32>(wp.x + wp.z, wp.y * 1.4 + wp.x * 0.2));
            spec = 0.06;
        }
        case 6: { // metal
            let streak = vnoise(vec2<f32>(uv.x * 30.0, uv.y * 1.5));
            albedo = albedo * (0.86 + streak * 0.2);
            spec = 0.9;
            shin = 70.0;
            rim = 0.25;
        }
        case 7: { // glass
            spec = 1.0;
            shin = 120.0;
            rim = 0.8;
        }
        case 9: { // emissive
            emissive = 1.0;
        }
        case 10: { // skin
            rim = 0.22;
            spec = 0.08;
        }
        case 11: { // cloth
            let w = vnoise(uv * 38.0);
            albedo = albedo * (0.94 + w * 0.12);
            rim = 0.14;
        }
        case 12: { // stone
            let s = fbm2(uv * 3.0);
            albedo = albedo * (0.78 + s * 0.4);
        }
        case 13: { // plaster walls
            let s = vnoise(uv * 6.0) * 0.5 + vnoise(uv * 21.0) * 0.5;
            albedo = albedo * (0.93 + s * 0.12);
        }
        case 14: { // asphalt / concrete
            let s = vnoise(uv * 14.0);
            albedo = albedo * (0.86 + s * 0.28);
        }
        case 15: { // bark
            let s = vnoise(vec2<f32>(atan2(wp.z, wp.x) * 5.0, wp.y * 0.8));
            albedo = albedo * (0.72 + s * 0.5);
        }
        case 16: { // build piece (instance param z selects wood/stone/metal)
            let kind = i32(i.ipar.z + 0.5);
            albedo = albedo * build_pattern(kind, i.lpos, wp, N);
            // framed look: darker toward the piece's edges
            if (kind == 2) { spec = 0.7; shin = 60.0; } else { spec = 0.03; }
            rim = 0.12;
        }
        default: {}
    }

    var col: vec3<f32>;
    if (emissive > 0.5) {
        col = albedo * 2.2;
    } else {
        col = light_surface(albedo, N, wp, ao, spec, shin, rim);
    }
    // hit / pickup flash
    col = col + vec3<f32>(1.0, 0.9, 0.8) * i.ipar.x;
    col = apply_storm(col, wp);
    col = apply_fog(col, wp);
    return vec4<f32>(col, 1.0);
}

// Translucent hologram used for the building placement preview (colour + alpha from the instance).
@fragment
fn fs_ghost(i: VOut) -> @location(0) vec4<f32> {
    let N = normalize(i.nrm);
    let V = normalize(G.cam_pos.xyz - i.wpos);
    let fres = pow(1.0 - saturate(abs(dot(N, V))), 2.0);
    let pulse = 0.85 + 0.15 * sin(G.cam_pos.w * 5.0);
    let a = i.tint.a * (0.30 + 0.45 * fres) * pulse;
    let rgb = i.tint.rgb * (1.3 + fres * 1.6);
    return vec4<f32>(rgb * a, a);
}
