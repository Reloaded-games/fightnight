//! Island generation: base terrain, then a layout pass (lakes, towns, roads) that
//! reshapes the heightmap so towns sit on plateaus and roads on smooth ground.

use super::heightmap::Heightmap;
use super::maps::{grid_to_world, polygon_signed_distance, polyline_distance, MapSpec, ReliefKind, GRID_CELL};
use super::nav::NavGrid;
use super::{CELL, GRID_N, WORLD_HALF, WORLD_SIZE};
use crate::math::*;
use crate::noise::*;
use crate::rng::*;
use std::sync::Arc;

// Expand the coastline and landmark distribution with the playable world;
// individual buildings, trees, roads and hills retain human-scale dimensions.
const ISLAND_SCALE: f32 = WORLD_SIZE / 1280.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoiKind {
    Town,
    Coastal,
    Farm,
    Lodge,
    Lakeside,
    Hamlet,
    Station,
    /// A dense grid of tall blocks (Tilted Towers).
    City,
    /// Houses round a green in the middle (Pleasant Park).
    Suburb,
    /// Warehouses, containers and chimneys (Flush Factory, Steamy Stacks, Dirty Docks).
    Industrial,
    /// A few big houses on large plots, often over the sea (Snobby Shores).
    Mansions,
    /// Stacks of wrecked cars behind a fence (Junk Junction).
    Junkyard,
    /// Buildings on the floor of a crater (Dusty Divot).
    Crater,
    /// A village on the dry spots of a swamp (Moisty Mire, Slurpy Swamp).
    Swamp,
    /// A row of shops facing a car park, houses behind (Retail Row).
    Retail,
}

impl PoiKind {
    /// A built-up place whose roads come in from all four sides.
    pub fn crossroads(self) -> bool {
        matches!(self, PoiKind::Town | PoiKind::City | PoiKind::Suburb | PoiKind::Retail | PoiKind::Industrial)
    }
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
            k if k.crossroads() => vec![self.center + ax, self.center - ax, self.center + az, self.center - az],
            PoiKind::Coastal | PoiKind::Station | PoiKind::Mansions => {
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
    /// Radius of an island in the middle as a share of the lake's (0 = none).
    pub island: f32,
}

/// A river at sea level (its bed is carved below the sea, so the sea's water fills it).
#[derive(Clone, Debug)]
pub struct River {
    pub pts: Vec<Vec2>,
    pub width: f32,
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
    pub rivers: Vec<River>,
    /// Waterlogged ground: centre and radius.
    pub swamps: Vec<(Vec2, f32)>,
}

impl Layout {
    /// Distance from `p` to the nearest river bank (negative inside a river).
    pub fn river_distance(&self, p: Vec2) -> f32 {
        self.rivers.iter().map(|r| polyline_distance(&r.pts, p).0 - r.width * 0.5).fold(f32::MAX, f32::min)
    }
}

/// Signed distance to a recreated map's coastline on a grid over the whole world, so it can be looked up quickly.
struct CoastField {
    n: usize,
    d: Vec<f32>,
}

impl CoastField {
    const N: usize = 321;

    fn new(spec: &MapSpec) -> Self {
        let poly = spec.coast_world();
        let n = Self::N;
        let step = WORLD_SIZE / (n - 1) as f32;
        let mut d = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                let p = Vec2::new(-WORLD_HALF + i as f32 * step, -WORLD_HALF + j as f32 * step);
                d[j * n + i] = polygon_signed_distance(&poly, p);
            }
        }
        Self { n, d }
    }

    fn sample(&self, x: f32, z: f32) -> f32 {
        let n = self.n;
        let step = WORLD_SIZE / (n - 1) as f32;
        let fx = ((x + WORLD_HALF) / step).clamp(0.0, (n - 1) as f32 - 1e-3);
        let fz = ((z + WORLD_HALF) / step).clamp(0.0, (n - 1) as f32 - 1e-3);
        let (i, j) = (fx as usize, fz as usize);
        let (tx, tz) = (fx - i as f32, fz - j as f32);
        let at = |i: usize, j: usize| self.d[j * n + i];
        let a = lerp(at(i, j), at(i + 1, j), tx);
        let b = lerp(at(i, j + 1), at(i + 1, j + 1), tx);
        // beyond the edge of the grid, keep getting further out to sea
        let out = (x.abs() - WORLD_HALF).max(0.0) + (z.abs() - WORLD_HALF).max(0.0);
        lerp(a, b, tz) - out
    }
}

/// Pure noise-based terrain before any gameplay-driven reshaping (or, for a recreated map, its table plus a little noise).
#[derive(Clone)]
pub struct BaseTerrain {
    pub seed: u32,
    pub mountain: Vec2,
    /// The recreated map the island follows; `None` for the procedural island.
    pub spec: Option<&'static MapSpec>,
    coast: Option<Arc<CoastField>>,
}

impl BaseTerrain {
    pub fn new(seed: u32) -> Self {
        // Pick the mountain direction from the seed, well inland.
        let mut r = Rng::new(seed as u64 ^ 0xA5A5);
        let a = r.range(0.0, std::f32::consts::TAU);
        let dist = r.range(150.0, 230.0) * ISLAND_SCALE;
        Self { seed, mountain: Vec2::new(a.cos() * dist, a.sin() * dist), spec: None, coast: None }
    }

    /// The island of a recreated map: its coastline, relief and water come from the table.
    pub fn from_spec(seed: u32, spec: &'static MapSpec) -> Self {
        let peak = spec
            .relief
            .iter()
            .filter(|r| r.h > 0.0)
            .max_by(|a, b| a.h.total_cmp(&b.h))
            .map(|r| grid_to_world(r.x, r.y))
            .unwrap_or(Vec2::ZERO);
        Self { seed, mountain: peak, spec: Some(spec), coast: Some(Arc::new(CoastField::new(spec))) }
    }

    /// Metres inside the coastline (negative in the sea).
    pub fn coast_dist(&self, x: f32, z: f32) -> f32 {
        if let Some(c) = &self.coast {
            // the table's outline, roughened so the shore is not ruler-straight
            let wiggle = fbm(x / 70.0 + 3.7, z / 70.0 - 8.1, 3, self.seed ^ 0x33) * 9.0;
            return c.sample(x, z) + wiggle;
        }
        let r = (x * x + z * z).sqrt();
        let a = z.atan2(x);
        let (ca, sa) = (a.cos(), a.sin());
        let s = self.seed;
        let n1 = fbm(ca * 1.3 + 11.0, sa * 1.3 + 5.0, 3, s ^ 0x11);
        let n2 = fbm(ca * 3.4 - 7.0, sa * 3.4 + 13.0, 2, s ^ 0x22);
        let coast_r = (452.0 + 66.0 * n1 + 24.0 * n2) * ISLAND_SCALE;
        let bays = fbm(x / (150.0 * ISLAND_SCALE) + 3.7, z / (150.0 * ISLAND_SCALE) - 8.1, 3, s ^ 0x33) * (30.0 * ISLAND_SCALE);
        coast_r + bays - r
    }

    pub fn height(&self, x: f32, z: f32) -> f32 {
        let d = self.coast_dist(x, z);
        let s = self.seed;
        if d < 0.0 {
            // continental shelf: shallow near shore, deeper out to sea
            return -14.0 * (1.0 - (d / 42.0).exp());
        }
        if let Some(spec) = self.spec {
            // gently rolling land at the map's usual height, with the table's hills, mountains and craters on it
            let beach = 2.8 * smoothstep(0.0, 26.0, d);
            let inland = smoothstep(10.0, 90.0, d);
            let big = fbm01(x / 240.0 + 20.0, z / 240.0 - 5.0, 4, s ^ 0x44);
            let rolling = big.powf(1.5) * 15.0 + fbm(x / 55.0, z / 55.0, 3, s ^ 0x55) * 2.4;
            let micro = fbm(x / 17.0, z / 17.0, 2, s ^ 0x88) * 0.35;
            return beach + inland * ((spec.plains - 2.8).max(0.0) + rolling) + self.relief(spec, x, z) * smoothstep(0.0, 45.0, d) + micro * smoothstep(0.0, 20.0, d);
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

    /// The height the table's relief features add at a point.
    fn relief(&self, spec: &MapSpec, x: f32, z: f32) -> f32 {
        let p = Vec2::new(x, z);
        let s = self.seed;
        let mut h = 0.0;
        for r in spec.relief {
            let foot = r.r * GRID_CELL;
            let c = grid_to_world(r.x, r.y);
            if p.distance(c) > foot * 1.6 {
                continue;
            }
            // hills are not round: the distance to the top is bent by noise, a crater keeps its circle
            let bend = if r.kind == ReliefKind::Crater { 0.0 } else { foot * 0.38 };
            let warp = Vec2::new(fbm(x / 85.0 + r.x * 3.1, z / 85.0, 2, s ^ 0x31), fbm(x / 85.0, z / 85.0 + r.y * 3.1, 2, s ^ 0x32)) * bend;
            let d = (p + warp).distance(c);
            if d > foot * 1.25 {
                continue;
            }
            let t = d / foot;
            h += match r.kind {
                ReliefKind::Hill => r.h * (-4.0 * t * t).exp() * (0.85 + 0.3 * fbm01(x / 60.0, z / 60.0, 2, s ^ 0x99)),
                ReliefKind::Mountain => r.h * (-3.2 * t * t).exp() * (0.62 + 0.38 * ridged(x / 70.0 + 9.0, z / 70.0, 4, s ^ 0x77)),
                ReliefKind::Plateau => r.h * smoothstep(1.0, 0.72, t),
                // a flat floor, steep walls and a raised rim
                ReliefKind::Crater => -r.h * smoothstep(0.82, 0.5, t) + r.h * 0.32 * (-((t - 0.88) / 0.13).powi(2)).exp(),
            };
        }
        h
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
    for _ in 0..1200 {
        let p = rng.in_disc(340.0 * ISLAND_SCALE);
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
        lakes.push(Lake { center: p, radius: rng.range(34.0, 50.0), level: h - 0.2, depth: rng.range(3.0, 4.5), island: 0.0 });
        if lakes.len() >= 3 {
            break;
        }
    }
    lakes
}

const POI_NAMES: [&str; 18] = [
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
    "Quartz Quay",
    "Highland Haven",
    "Orchard Outpost",
    "Willow Workshop",
    "Amber Acres",
    "Beacon Bay",
];

fn poi_radius(kind: PoiKind) -> f32 {
    match kind {
        PoiKind::Town => 95.0,
        PoiKind::Coastal => 78.0,
        PoiKind::Farm => 72.0,
        PoiKind::Lodge => 66.0,
        PoiKind::Lakeside => 92.0,
        PoiKind::Hamlet => 48.0,
        PoiKind::Station => 36.0,
        PoiKind::City => 70.0,
        PoiKind::Suburb => 75.0,
        PoiKind::Industrial => 70.0,
        PoiKind::Mansions => 70.0,
        PoiKind::Junkyard => 50.0,
        PoiKind::Crater => 55.0,
        PoiKind::Swamp => 65.0,
        PoiKind::Retail => 65.0,
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
    for _ in 0..5000 {
        let p = rng.in_disc(WORLD_HALF * 0.78);
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
        if lakes.iter().any(|l| l.center.distance(p) < l.radius * 2.2 + 100.0) {
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
        .filter(|c| c.0.length() < 170.0 * ISLAND_SCALE)
        .min_by(|a, b| (a.3 + a.0.length() * 0.0008).partial_cmp(&(b.3 + b.0.length() * 0.0008)).unwrap())
        .or_else(|| pool.iter().min_by(|a, b| a.0.length().partial_cmp(&b.0.length()).unwrap()));
    if let Some(t) = town {
        add(&mut pois, PoiKind::Town, t.0, 0);
    }

    // 2. Coastal village: near the shore, far from the town.
    let town_c = pois.first().map(|p| p.center).unwrap_or(Vec2::ZERO);
    let coastal = pool
        .iter()
        .filter(|c| c.2 < 110.0 * ISLAND_SCALE && taken(&pois, c.0, PoiKind::Coastal))
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
        if taken(&pois, lake.center, PoiKind::Lakeside) {
            add(&mut pois, PoiKind::Lakeside, lake.center, 3);
            if let Some(last) = pois.last_mut() {
                last.ground = lake.level + 0.45;
                last.radius = 92.0;
            }
        }
    }

    // 5..: farthest-point sampling for the rest.
    let rest = [
        PoiKind::Farm, PoiKind::Hamlet, PoiKind::Hamlet, PoiKind::Station, PoiKind::Coastal,
        PoiKind::Lodge, PoiKind::Hamlet, PoiKind::Coastal, PoiKind::Station, PoiKind::Town,
        PoiKind::Farm, PoiKind::Station, PoiKind::Farm, PoiKind::Coastal,
    ];
    for (k, kind) in rest.iter().enumerate() {
        let mut best: Option<(f32, Vec2)> = None;
        for c in &pool {
            if !taken(&pois, c.0, *kind) {
                continue;
            }
            if *kind == PoiKind::Farm && c.3 > 0.12 {
                continue;
            }
            if *kind == PoiKind::Coastal && c.2 > 120.0 * ISLAND_SCALE {
                continue;
            }
            let md = pois.iter().map(|o| o.center.distance(c.0)).fold(f32::MAX, f32::min);
            if best.is_none_or(|b| md > b.0) {
                best = Some((md, c.0));
            }
        }
        if let Some((_, p)) = best {
            add(&mut pois, *kind, p, k + 4);
        }
    }
    pois
}

/// The coarse grid roads are routed over: the sea, the coast and the lakes are blocked.
fn road_grid(base: &BaseTerrain, lakes: &[Lake]) -> NavGrid {
    let cell = 8.0 * ISLAND_SCALE;
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
    grid
}

/// A road from place `a` to place `b` (entry to entry), routed around steep ground and, where it can, rivers.
fn route_road(grid: &NavGrid, base: &BaseTerrain, rivers: &[River], a: &Poi, b: &Poi, asphalt: bool) -> Option<Road> {
    let nearest = |p: &Poi, to: Vec2| p.entries().into_iter().min_by(|x, y| x.distance(to).partial_cmp(&y.distance(to)).unwrap()).unwrap();
    let pa = nearest(a, b.center);
    let pb = nearest(b, a.center);
    let cost = |i: i32, j: i32| -> Option<f32> {
        let c = grid.center(i, j);
        let s = base.slope(c.x, c.y, 6.0);
        // crossing a river is fine (there is a ford), following one is not
        let wet = rivers.iter().any(|r| polyline_distance(&r.pts, c).0 < r.width * 0.5 + 8.0);
        Some(s * 9.0 + if wet { 6.0 } else { 0.0 })
    };
    let path = grid.find_path_with(pa, pb, 60_000, cost)?;
    let mut pts: Vec<Vec2> = path;
    if pts.len() < 2 {
        return None;
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
    let kind = if asphalt { RoadKind::Asphalt } else { RoadKind::Dirt };
    Some(Road { pts, width: if asphalt { 7.5 } else { 5.4 }, kind })
}

fn plan_roads(base: &BaseTerrain, lakes: &[Lake], pois: &[Poi]) -> Vec<Road> {
    if pois.len() < 2 {
        return vec![];
    }
    let grid = road_grid(base, lakes);
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
    // Alternate routes for drivers rather than a single tree of dead ends.
    let mut extras = 0;
    for a in 0..n {
        if extras >= 6 {
            break;
        }
        let mut cand: Vec<(f32, usize)> = (0..n)
            .filter(|&b| b != a && !edges.iter().any(|&(x, y)| (x == a && y == b) || (x == b && y == a)))
            .map(|b| (pois[a].center.distance(pois[b].center), b))
            .collect();
        cand.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        if let Some(&(d, b)) = cand.first() {
            if d < 330.0 * ISLAND_SCALE {
                edges.push((a, b));
                extras += 1;
            }
        }
    }
    let mut roads = vec![];
    for (a, b) in edges {
        let asphalt = matches!(pois[a].kind, PoiKind::Town | PoiKind::Station) || matches!(pois[b].kind, PoiKind::Town | PoiKind::Station);
        if let Some(r) = route_road(&grid, base, &[], &pois[a], &pois[b], asphalt) {
            roads.push(r);
        }
    }
    roads
}

pub fn plan_layout(base: &BaseTerrain) -> Layout {
    if let Some(spec) = base.spec {
        return layout_from_spec(base, spec);
    }
    let mut rng = Rng::new(base.seed as u64 ^ 0xC0FFEE);
    let lakes = plan_lakes(base, &mut rng);
    let pois = plan_pois(base, &lakes, &mut rng);
    let roads = plan_roads(base, &lakes, &pois);
    Layout { mountain: base.mountain, pois, lakes, roads, rivers: vec![], swamps: vec![] }
}

/// The layout of a recreated map: its lakes, rivers, swamps, places and roads, straight from the table.
fn layout_from_spec(base: &BaseTerrain, spec: &'static MapSpec) -> Layout {
    let lakes: Vec<Lake> = spec
        .lakes
        .iter()
        .map(|l| {
            let center = grid_to_world(l.x, l.y);
            let radius = l.r * GRID_CELL;
            // the water lies a little below the lowest ground round its shore
            let lowest = (0..24)
                .map(|k| {
                    let a = k as f32 / 24.0 * std::f32::consts::TAU;
                    let p = center + Vec2::new(a.cos(), a.sin()) * radius * 1.4;
                    base.height(p.x, p.y)
                })
                .fold(f32::MAX, f32::min);
            Lake { center, radius, level: (lowest - 0.4).max(1.2), depth: (radius * 0.06).clamp(3.0, 5.0), island: l.island }
        })
        .collect();
    let rivers: Vec<River> = spec.rivers.iter().map(|r| River { pts: r.pts.iter().map(|&(x, y)| grid_to_world(x, y)).collect(), width: r.width }).collect();
    let swamps: Vec<(Vec2, f32)> = spec.swamps.iter().map(|a| (grid_to_world(a.x, a.y), a.r * GRID_CELL)).collect();
    let pois: Vec<Poi> = spec
        .pois
        .iter()
        .map(|ps| {
            let center = grid_to_world(ps.x, ps.y);
            let radius = ps.r;
            let mut ground = ground_height(base, center, radius);
            if ps.kind == PoiKind::Lakeside {
                // a place on a lake sits at its shore
                if let Some(l) = lakes.iter().filter(|l| l.center.distance(center) < l.radius + radius).min_by(|a, b| a.center.distance(center).total_cmp(&b.center.distance(center))) {
                    ground = l.level + 0.45;
                }
            }
            Poi { name: ps.name.to_string(), kind: ps.kind, center, radius, ground, axis_x: ps.axis_x }
        })
        .collect();
    let grid = road_grid(base, &lakes);
    let mut roads = vec![];
    for r in spec.roads {
        let (Some(a), Some(b)) = (pois.iter().position(|p| p.name == r.from), pois.iter().position(|p| p.name == r.to)) else { continue };
        if let Some(road) = route_road(&grid, base, &rivers, &pois[a], &pois[b], r.paved) {
            roads.push(road);
        }
    }
    Layout { mountain: base.mountain, pois, lakes, roads, rivers, swamps }
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
    // where roads cross rivers the bed stays shallow enough to drive through
    let mut ford = vec![false; if layout.rivers.is_empty() { 0 } else { GRID_N * GRID_N }];
    if !layout.rivers.is_empty() {
        for road in &layout.roads {
            for seg in road.pts.windows(2) {
                let reach = road.width * 0.5 + 3.5;
                let (a, b) = (seg[0], seg[1]);
                let (lo, hi) = (a.min(b) - Vec2::splat(reach), a.max(b) + Vec2::splat(reach));
                let i0 = ((lo.x + WORLD_HALF) / CELL).floor().max(0.0) as usize;
                let j0 = ((lo.y + WORLD_HALF) / CELL).floor().max(0.0) as usize;
                let i1 = (((hi.x + WORLD_HALF) / CELL).ceil() as usize).min(GRID_N - 1);
                let j1 = (((hi.y + WORLD_HALF) / CELL).ceil() as usize).min(GRID_N - 1);
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        let p = Vec2::new(hm.world_x(i), hm.world_x(j));
                        if point_segment_dist(p, a, b).0 < reach {
                            ford[j * GRID_N + i] = true;
                        }
                    }
                }
            }
        }
    }
    for j in 0..GRID_N {
        let z = hm.world_x(j);
        for i in 0..GRID_N {
            let x = hm.world_x(i);
            let p = Vec2::new(x, z);
            let mut h = base.height(x, z);
            // swamps: low, waterlogged ground with pools of open water
            for &(c, r) in &layout.swamps {
                let dd = p.distance(c);
                if dd < r {
                    let bog = -0.1 + fbm(x / 16.0 + 4.0, z / 16.0 - 2.0, 3, base.seed ^ 0x5A) * 2.2;
                    h = lerp(h, bog, smoothstep(r, r * 0.55, dd));
                }
            }
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
                // (a big lake does not flatten half the island round it)
                let r_blend = if lake.radius > 60.0 { lake.radius + 40.0 } else { lake.radius * 2.1 };
                if dd < r_blend {
                    let shore = lake.level + 0.45;
                    let wflat = smootherstep(r_blend, lake.radius * 1.05, dd);
                    h = lerp(h, shore, wflat);
                    let t = dd / lake.radius;
                    h -= lake.depth * smoothstep(1.0, 0.45, t) + 0.0;
                    if lake.island > 0.0 {
                        // an island in the middle, standing a little above the water
                        let ri = lake.radius * lake.island;
                        h = lerp(h, lake.level + 1.3, smoothstep(ri * 1.25, ri * 0.75, dd));
                    }
                }
            }
            // rivers run at sea level: their bed is below it, shallow (a ford) where a road crosses
            for river in &layout.rivers {
                let (dd, _) = polyline_distance(&river.pts, p);
                let half = river.width * 0.5;
                let bank = 22.0;
                if dd < half + bank {
                    let ford = ford[j * GRID_N + i];
                    let bed = if ford { -0.2 } else { -1.9 + 1.5 * (dd / half).clamp(0.0, 1.0).powi(2) };
                    let edge = if ford { -0.2 } else { 0.35 };
                    let target = if dd < half { bed } else { lerp(edge, h, smoothstep(half, half + bank, dd)) };
                    h = h.min(target);
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
            let p = Vec2::new(a.cos(), a.sin()) * (WORLD_HALF - 60.0);
            assert!(b.height(p.x, p.y) < -3.0, "sea near world edge angle {k}");
        }
    }

    #[test]
    fn layout_places_pois_apart_on_land() {
        for seed in [1u32, 7, 42, 2024, 99999] {
            let b = BaseTerrain::new(seed);
            let l = plan_layout(&b);
            assert!(l.pois.len() >= 13, "seed {seed}: only {} POIs", l.pois.len());
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
    fn larger_island_layout_is_deterministic_and_distributed() {
        let b = BaseTerrain::new(1234);
        let first = plan_layout(&b);
        let second = plan_layout(&b);
        assert_eq!(first.pois.len(), second.pois.len());
        assert_eq!(first.roads.len(), second.roads.len());
        for (a, b) in first.pois.iter().zip(&second.pois) {
            assert_eq!((a.name.as_str(), a.kind, a.center, a.radius, a.ground, a.axis_x), (b.name.as_str(), b.kind, b.center, b.radius, b.ground, b.axis_x));
        }
        for (a, b) in first.roads.iter().zip(&second.roads) {
            assert_eq!((&a.pts, a.width, a.kind), (&b.pts, b.width, b.kind));
        }
        // The expanded area must contain useful land and destinations in every
        // quadrant, rather than adding an empty sea around the old map.
        for (sx, sz) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
            assert!(first.pois.iter().any(|p| p.center.x * sx > 100.0 && p.center.y * sz > 100.0), "quadrant ({sx}, {sz}) has no destination");
        }
        assert!(first.pois.iter().filter(|p| p.center.length() > 450.0).count() >= 6);
        assert!(first.roads.len() >= first.pois.len(), "expanded map should have alternate road routes");
        assert_eq!(first.lakes.len(), 3);
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
