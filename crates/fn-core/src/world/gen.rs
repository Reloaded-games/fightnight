//! Island generation: base terrain, then a layout pass (lakes, towns, roads) that
//! reshapes the heightmap so towns sit on plateaus and roads on smooth ground.

use super::heightmap::Heightmap;
use super::nav::NavGrid;
use super::{CELL, GRID_N, WORLD_HALF};
use crate::math::*;
use crate::noise::*;
use crate::rng::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoiKind {
    Town,
    Coastal,
    Farm,
    Lodge,
    Lakeside,
    Hamlet,
    Station,
}

#[derive(Clone, Debug)]
pub struct Poi {
    pub name: String,
    pub kind: PoiKind,
    pub center: Vec2,
    pub radius: f32,
    /// Plateau height the town is levelled to.
    pub ground: f32,
    /// Direction of the main street (true = along X). Cross streets run the other way.
    pub axis_x: bool,
}

impl Poi {
    /// Points where roads from other places join this POI.
    pub fn entries(&self) -> Vec<Vec2> {
        let r = self.radius * 0.93;
        let (ax, az) = (Vec2::new(r, 0.0), Vec2::new(0.0, r));
        match self.kind {
            PoiKind::Town => vec![self.center + ax, self.center - ax, self.center + az, self.center - az],
            PoiKind::Coastal | PoiKind::Station => {
                let d = if self.axis_x { ax } else { az };
                vec![self.center + d, self.center - d]
            }
            // roads reach the lakeside village on the ring road, on the side facing the island centre
            PoiKind::Lakeside => vec![self.center + (-self.center).normalize_or_zero() * (self.radius * 0.85)],
            _ => vec![self.center],
        }
    }
}

#[derive(Clone, Debug)]
pub struct Lake {
    pub center: Vec2,
    pub radius: f32,
    /// Water surface height.
    pub level: f32,
    pub depth: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoadKind {
    Dirt,
    Asphalt,
}

#[derive(Clone, Debug)]
pub struct Road {
    pub pts: Vec<Vec2>,
    pub width: f32,
    pub kind: RoadKind,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub mountain: Vec2,
    pub pois: Vec<Poi>,
    pub lakes: Vec<Lake>,
    pub roads: Vec<Road>,
}

/// Pure noise-based terrain before any gameplay-driven reshaping.
#[derive(Clone)]
pub struct BaseTerrain {
    pub seed: u32,
    pub mountain: Vec2,
}

impl BaseTerrain {
    pub fn new(seed: u32) -> Self {
        // Pick the mountain direction from the seed, well inland.
        let mut r = Rng::new(seed as u64 ^ 0xA5A5);
        let a = r.range(0.0, std::f32::consts::TAU);
        let dist = r.range(150.0, 230.0);
        Self { seed, mountain: Vec2::new(a.cos() * dist, a.sin() * dist) }
    }

    /// Metres inside the coastline (negative in the sea).
    pub fn coast_dist(&self, x: f32, z: f32) -> f32 {
        let r = (x * x + z * z).sqrt();
        let a = z.atan2(x);
        let (ca, sa) = (a.cos(), a.sin());
        let s = self.seed;
        let n1 = fbm(ca * 1.3 + 11.0, sa * 1.3 + 5.0, 3, s ^ 0x11);
        let n2 = fbm(ca * 3.4 - 7.0, sa * 3.4 + 13.0, 2, s ^ 0x22);
        let coast_r = 452.0 + 66.0 * n1 + 24.0 * n2;
        let bays = fbm(x / 150.0 + 3.7, z / 150.0 - 8.1, 3, s ^ 0x33) * 30.0;
        coast_r + bays - r
    }

    pub fn height(&self, x: f32, z: f32) -> f32 {
        let d = self.coast_dist(x, z);
        let s = self.seed;
        if d < 0.0 {
            // continental shelf: shallow near shore, deeper out to sea
            return -14.0 * (1.0 - (d / 42.0).exp());
        }
        let beach = 2.8 * smoothstep(0.0, 26.0, d);
        let inland = smoothstep(10.0, 110.0, d);
        let big = fbm01(x / 240.0 + 20.0, z / 240.0 - 5.0, 4, s ^ 0x44);
        let hills = big.powf(1.5) * 30.0;
        let detail = fbm(x / 55.0, z / 55.0, 3, s ^ 0x55) * 2.4;
        let ridge = ridged(x / 150.0, z / 150.0, 3, s ^ 0x66) * 9.0 * smoothstep(0.4, 0.75, big);
        let m = (self.mountain - Vec2::new(x, z)).length() / 150.0;
        let mountain = (-(m * m)).exp() * (31.0 * (0.6 + 0.4 * ridged(x / 80.0 + 9.0, z / 80.0, 4, s ^ 0x77)));
        let micro = fbm(x / 17.0, z / 17.0, 2, s ^ 0x88) * 0.35;
        beach + inland * (hills + detail + ridge) + mountain * smoothstep(40.0, 120.0, d) + micro * smoothstep(0.0, 20.0, d)
    }

    /// Rise over run measured at a scale of `r` metres.
    pub fn slope(&self, x: f32, z: f32, r: f32) -> f32 {
        let dx = (self.height(x + r, z) - self.height(x - r, z)) / (2.0 * r);
        let dz = (self.height(x, z + r) - self.height(x, z - r)) / (2.0 * r);
        (dx * dx + dz * dz).sqrt()
    }
}

fn plan_lakes(base: &BaseTerrain, rng: &mut Rng) -> Vec<Lake> {
    let mut cands: Vec<(Vec2, f32)> = vec![];
    for _ in 0..600 {
        let p = rng.in_disc(340.0);
        if base.coast_dist(p.x, p.y) < 130.0 {
            continue;
        }
        let h = base.height(p.x, p.y);
        if !(2.5..=16.0).contains(&h) {
            continue;
        }
        if (p - base.mountain).length() < 175.0 || p.length() < 150.0 {
            continue;
        }
        if base.slope(p.x, p.y, 10.0) > 0.22 {
            continue;
        }
        cands.push((p, h));
    }
    cands.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let mut lakes: Vec<Lake> = vec![];
    for (p, h) in cands {
        if lakes.iter().any(|l| l.center.distance(p) < 260.0) {
            continue;
        }
        lakes.push(Lake { center: p, radius: rng.range(34.0, 50.0), level: h - 0.2, depth: rng.range(3.0, 4.5) });
        if lakes.len() >= 2 {
            break;
        }
    }
    lakes
}

const POI_NAMES: [&str; 12] = [
    "Maple Meadows",
    "Sunset Cove",
    "Pine Ridge",
    "Lazy Lagoon",
    "Windmill Farm",
    "Autumn Hollow",
    "Cedar Crossing",
    "Dusty Depot",
    "Harbor Heights",
    "Fox Hill",
    "Birch Bend",
    "Gull Point",
];

fn poi_radius(kind: PoiKind) -> f32 {
    match kind {
        PoiKind::Town => 95.0,
        PoiKind::Coastal => 78.0,
        PoiKind::Farm => 72.0,
        PoiKind::Lodge => 66.0,
        PoiKind::Lakeside => 60.0,
        PoiKind::Hamlet => 48.0,
        PoiKind::Station => 36.0,
    }
}

fn ground_height(base: &BaseTerrain, c: Vec2, r: f32) -> f32 {
    let mut sum = 0.0;
    let mut n = 0.0;
    for k in 0..16 {
        let a = k as f32 / 16.0 * std::f32::consts::TAU;
        let rr = if k % 2 == 0 { r * 0.5 } else { r * 0.25 };
        sum += base.height(c.x + a.cos() * rr, c.y + a.sin() * rr);
        n += 1.0;
    }
    sum += base.height(c.x, c.y) * 2.0;
    n += 2.0;
    (sum / n).max(2.6)
}

fn plan_pois(base: &BaseTerrain, lakes: &[Lake], rng: &mut Rng) -> Vec<Poi> {
    // Candidate pool of flat-ish land.
    let mut pool: Vec<(Vec2, f32, f32, f32)> = vec![]; // pos, height, coast dist, slope
    for _ in 0..2500 {
        let p = rng.in_disc(400.0);
        let d = base.coast_dist(p.x, p.y);
        if d < 55.0 {
            continue;
        }
        let h = base.height(p.x, p.y);
        if !(2.6..=40.0).contains(&h) {
            continue;
        }
        let sl = base.slope(p.x, p.y, 14.0);
        if sl > 0.26 {
            continue;
        }
        if lakes.iter().any(|l| l.center.distance(p) < l.radius * 2.2 + 40.0) {
            continue;
        }
        pool.push((p, h, d, sl));
    }
    let mut pois: Vec<Poi> = vec![];
    let taken = |pois: &Vec<Poi>, p: Vec2, kind: PoiKind| -> bool {
        let r = poi_radius(kind);
        pois.iter().all(|o| o.center.distance(p) > (o.radius + r) * 1.05 + 25.0)
    };
    let add = |pois: &mut Vec<Poi>, kind: PoiKind, center: Vec2, idx: usize| {
        let radius = poi_radius(kind);
        let ground = ground_height(base, center, radius);
        // Streets of coastal villages run along the shore, i.e. perpendicular to the direction out to sea.
        let out = center.normalize_or_zero();
        let axis_x = if kind == PoiKind::Coastal { out.x.abs() < out.y.abs() } else { center.x.abs() >= center.y.abs() };
        pois.push(Poi { name: POI_NAMES[idx].to_string(), kind, center, radius, ground, axis_x });
    };

    // 1. Central town: flattest candidate near the island centre.
    let town = pool
        .iter()
        .filter(|c| c.0.length() < 170.0)
        .min_by(|a, b| (a.3 + a.0.length() * 0.0008).partial_cmp(&(b.3 + b.0.length() * 0.0008)).unwrap())
        .or_else(|| pool.iter().min_by(|a, b| a.0.length().partial_cmp(&b.0.length()).unwrap()));
    if let Some(t) = town {
        add(&mut pois, PoiKind::Town, t.0, 0);
    }

    // 2. Coastal village: near the shore, far from the town.
    let town_c = pois.first().map(|p| p.center).unwrap_or(Vec2::ZERO);
    let coastal = pool
        .iter()
        .filter(|c| c.2 < 110.0 && taken(&pois, c.0, PoiKind::Coastal))
        .max_by(|a, b| a.0.distance(town_c).partial_cmp(&b.0.distance(town_c)).unwrap());
    if let Some(c) = coastal {
        add(&mut pois, PoiKind::Coastal, c.0, 1);
    }

    // 3. Mountain lodge on the flank of the mountain.
    let mut lodge: Option<Vec2> = None;
    let mut best = f32::MAX;
    for _ in 0..800 {
        let a = rng.range(0.0, std::f32::consts::TAU);
        let rr = rng.range(95.0, 150.0);
        let p = base.mountain + Vec2::new(a.cos(), a.sin()) * rr;
        if base.coast_dist(p.x, p.y) < 90.0 || !taken(&pois, p, PoiKind::Lodge) {
            continue;
        }
        let h = base.height(p.x, p.y);
        if !(16.0..=34.0).contains(&h) {
            continue;
        }
        let sl = base.slope(p.x, p.y, 12.0);
        if sl < best && sl < 0.4 {
            best = sl;
            lodge = Some(p);
        }
    }
    if let Some(p) = lodge {
        add(&mut pois, PoiKind::Lodge, p, 2);
    }

    // 4. Lakeside village: the POI is centred on the first lake, houses form a ring around it.
    if let Some(lake) = lakes.first() {
        if taken(&pois, lake.center, PoiKind::Lakeside) || true {
            add(&mut pois, PoiKind::Lakeside, lake.center, 3);
            if let Some(last) = pois.last_mut() {
                last.ground = lake.level + 0.45;
                last.radius = 92.0;
            }
        }
    }

    // 5..: farthest-point sampling for the rest.
    let rest = [PoiKind::Farm, PoiKind::Hamlet, PoiKind::Hamlet, PoiKind::Station, PoiKind::Coastal];
    for (k, kind) in rest.iter().enumerate() {
        let mut best: Option<(f32, Vec2)> = None;
        for c in &pool {
            if !taken(&pois, c.0, *kind) {
                continue;
            }
            if *kind == PoiKind::Farm && c.3 > 0.12 {
                continue;
            }
            if *kind == PoiKind::Coastal && c.2 > 120.0 {
                continue;
            }
            let md = pois.iter().map(|o| o.center.distance(c.0)).fold(f32::MAX, f32::min);
            if best.is_none_or(|b| md > b.0) {
                best = Some((md, c.0));
            }
        }
        if let Some((_, p)) = best {
            let names = [4, 5, 6, 7, 8];
            add(&mut pois, *kind, p, names[k]);
        }
    }
    pois
}

fn plan_roads(base: &BaseTerrain, lakes: &[Lake], pois: &[Poi]) -> Vec<Road> {
    if pois.len() < 2 {
        return vec![];
    }
    // Coarse cost grid
    let cell = 8.0;
    let mut grid = NavGrid::new(WORLD_HALF, cell);
    for j in 0..grid.h as i32 {
        for i in 0..grid.w as i32 {
            let c = grid.center(i, j);
            let h = base.height(c.x, c.y);
            let water = lakes.iter().any(|l| l.center.distance(c) < l.radius * 1.35 + 6.0);
            if h < 0.9 || base.coast_dist(c.x, c.y) < 8.0 || water {
                grid.set_blocked(i, j, true);
            }
        }
    }
    let mut edges: Vec<(usize, usize)> = vec![];
    // Prim MST
    let n = pois.len();
    let mut in_tree = vec![false; n];
    in_tree[0] = true;
    for _ in 1..n {
        let mut best: Option<(f32, usize, usize)> = None;
        for a in 0..n {
            if !in_tree[a] {
                continue;
            }
            for b in 0..n {
                if in_tree[b] {
                    continue;
                }
                let d = pois[a].center.distance(pois[b].center);
                if best.is_none_or(|x| d < x.0) {
                    best = Some((d, a, b));
                }
            }
        }
        if let Some((_, a, b)) = best {
            in_tree[b] = true;
            edges.push((a, b));
        }
    }
    // A few loops: connect each POI to its nearest non-adjacent neighbour (limit 3).
    let mut extras = 0;
    for a in 0..n {
        if extras >= 3 {
            break;
        }
        let mut cand: Vec<(f32, usize)> = (0..n)
            .filter(|&b| b != a && !edges.iter().any(|&(x, y)| (x == a && y == b) || (x == b && y == a)))
            .map(|b| (pois[a].center.distance(pois[b].center), b))
            .collect();
        cand.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        if let Some(&(d, b)) = cand.first() {
            if d < 330.0 {
                edges.push((a, b));
                extras += 1;
            }
        }
    }
    let mut roads = vec![];
    for (a, b) in edges {
        let nearest = |p: &Poi, to: Vec2| p.entries().into_iter().min_by(|x, y| x.distance(to).partial_cmp(&y.distance(to)).unwrap()).unwrap();
        let pa = nearest(&pois[a], pois[b].center);
        let pb = nearest(&pois[b], pois[a].center);
        let cost = |i: i32, j: i32| -> Option<f32> {
            let c = grid.center(i, j);
            let s = base.slope(c.x, c.y, 6.0);
            Some(s * 9.0 + 0.0)
        };
        let Some(path) = grid.find_path_with(pa, pb, 60_000, cost) else { continue };
        let mut pts: Vec<Vec2> = path;
        if pts.len() < 2 {
            continue;
        }
        pts[0] = pa;
        *pts.last_mut().unwrap() = pb;
        // Chaikin smoothing keeps the endpoints.
        for _ in 0..3 {
            let mut out = vec![pts[0]];
            for w in pts.windows(2) {
                out.push(w[0] * 0.75 + w[1] * 0.25);
                out.push(w[0] * 0.25 + w[1] * 0.75);
            }
            out.push(*pts.last().unwrap());
            pts = out;
        }
        let kind = if matches!(pois[a].kind, PoiKind::Town | PoiKind::Station) || matches!(pois[b].kind, PoiKind::Town | PoiKind::Station) {
            RoadKind::Asphalt
        } else {
            RoadKind::Dirt
        };
        roads.push(Road { pts, width: if kind == RoadKind::Asphalt { 6.0 } else { 4.6 }, kind });
    }
    roads
}

pub fn plan_layout(base: &BaseTerrain) -> Layout {
    let mut rng = Rng::new(base.seed as u64 ^ 0xC0FFEE);
    let lakes = plan_lakes(base, &mut rng);
    let pois = plan_pois(base, &lakes, &mut rng);
    let roads = plan_roads(base, &lakes, &pois);
    Layout { mountain: base.mountain, pois, lakes, roads }
}

/// Distance from a point to the nearest road centre-line, with that road's width.
pub fn road_distance(layout: &Layout, p: Vec2) -> (f32, f32, RoadKind) {
    let mut best = (f32::MAX, 0.0, RoadKind::Dirt);
    for r in &layout.roads {
        for w in r.pts.windows(2) {
            let (d, _) = point_segment_dist(p, w[0], w[1]);
            if d < best.0 {
                best = (d, r.width, r.kind);
            }
        }
    }
    best
}

/// Build the final heightmap: base terrain + town plateaus + lake basins + smoothed roads.
pub fn build_heightmap(base: &BaseTerrain, layout: &Layout) -> Heightmap {
    let mut hm = Heightmap::new(GRID_N, CELL);
    for j in 0..GRID_N {
        let z = hm.world_x(j);
        for i in 0..GRID_N {
            let x = hm.world_x(i);
            let p = Vec2::new(x, z);
            let mut h = base.height(x, z);
            for poi in &layout.pois {
                let dd = p.distance(poi.center);
                let r_in = poi.radius * 0.72;
                let r_out = poi.radius * 1.5;
                if dd < r_out {
                    let w = smootherstep(r_out, r_in, dd);
                    h = lerp(h, poi.ground, w);
                }
            }
            for lake in &layout.lakes {
                let dd = p.distance(lake.center);
                let r_blend = lake.radius * 2.1;
                if dd < r_blend {
                    let shore = lake.level + 0.45;
                    let wflat = smootherstep(r_blend, lake.radius * 1.05, dd);
                    h = lerp(h, shore, wflat);
                    let t = dd / lake.radius;
                    h -= lake.depth * smoothstep(1.0, 0.45, t) + 0.0;
                }
            }
            hm.set(i, j, h);
        }
    }
    // Smooth road corridors.
    if !layout.roads.is_empty() {
        let blurred = hm.blurred(4, 2);
        let mut weight = vec![0.0f32; GRID_N * GRID_N];
        for road in &layout.roads {
            let reach = road.width * 0.5 + 7.0;
            for seg in road.pts.windows(2) {
                let (a, b) = (seg[0], seg[1]);
                let lo = a.min(b) - Vec2::splat(reach);
                let hi = a.max(b) + Vec2::splat(reach);
                let (i0, j0) = (((lo.x + WORLD_HALF) / CELL).floor().max(0.0) as usize, ((lo.y + WORLD_HALF) / CELL).floor().max(0.0) as usize);
                let (i1, j1) = (
                    (((hi.x + WORLD_HALF) / CELL).ceil() as usize).min(GRID_N - 1),
                    (((hi.y + WORLD_HALF) / CELL).ceil() as usize).min(GRID_N - 1),
                );
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        let p = Vec2::new(hm.world_x(i), hm.world_x(j));
                        let (d, _) = point_segment_dist(p, a, b);
                        let w = smoothstep(reach, road.width * 0.5, d);
                        let k = j * GRID_N + i;
                        if w > weight[k] {
                            weight[k] = w;
                        }
                    }
                }
            }
        }
        for k in 0..GRID_N * GRID_N {
            if weight[k] > 0.0 {
                let h = hm.h[k];
                hm.h[k] = lerp(h, blurred[k], weight[k] * 0.9);
            }
        }
    }
    hm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn island_has_sea_around_and_land_in_middle() {
        let b = BaseTerrain::new(1234);
        assert!(b.height(0.0, 0.0) > 1.0);
        for k in 0..16 {
            let a = k as f32 / 16.0 * std::f32::consts::TAU;
            let p = Vec2::new(a.cos(), a.sin()) * 600.0;
            assert!(b.height(p.x, p.y) < -3.0, "sea at r=600 angle {k}");
        }
    }

    #[test]
    fn layout_places_pois_apart_on_land() {
        for seed in [1u32, 7, 42, 2024, 99999] {
            let b = BaseTerrain::new(seed);
            let l = plan_layout(&b);
            assert!(l.pois.len() >= 5, "seed {seed}: only {} POIs", l.pois.len());
            for (i, p) in l.pois.iter().enumerate() {
                assert!(b.height(p.center.x, p.center.y) > 1.5, "seed {seed}: {} in sea", p.name);
                for q in &l.pois[i + 1..] {
                    assert!(p.center.distance(q.center) > 120.0, "seed {seed}: {} and {} too close", p.name, q.name);
                }
            }
            assert!(!l.roads.is_empty());
            for r in &l.roads {
                assert!(r.pts.len() >= 4);
            }
        }
    }

    #[test]
    fn heightmap_flat_under_towns_and_water_in_lakes() {
        let b = BaseTerrain::new(1234);
        let l = plan_layout(&b);
        let hm = build_heightmap(&b, &l);
        for poi in l.pois.iter().filter(|p| p.kind != PoiKind::Lakeside) {
            // Core of the town plateau should be nearly flat and at ground level
            let h0 = hm.height_at(poi.center.x, poi.center.y);
            for k in 0..8 {
                let a = k as f32 / 8.0 * std::f32::consts::TAU;
                let p = poi.center + Vec2::new(a.cos(), a.sin()) * poi.radius * 0.5;
                let h = hm.height_at(p.x, p.y);
                assert!((h - h0).abs() < 1.2, "{}: plateau not flat ({h} vs {h0})", poi.name);
            }
        }
        for lake in &l.lakes {
            let hc = hm.height_at(lake.center.x, lake.center.y);
            assert!(hc < lake.level - 1.5, "lake centre should be a basin: {hc} vs level {}", lake.level);
            let hr = hm.height_at(lake.center.x + lake.radius * 1.3, lake.center.y);
            assert!(hr > lake.level - 0.2, "lake rim must be above water: {hr} vs {}", lake.level);
        }
    }
}
