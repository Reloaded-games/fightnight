//! Natural props: trees, bushes and rocks scattered by biome masks, plus
//! lazily generated grass and flowers around the player.

use super::collision::*;
use super::gen::*;
use super::heightmap::Heightmap;
use super::splat::*;
use super::terrain_mesh::TerrainPainter;
use super::{CHUNKS, CHUNK_SIZE, WORLD_HALF};
use crate::math::*;
use crate::mesh::Instance;
use crate::meshlib::MeshId;
use crate::rng::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PropKind {
    Pine,
    Oak,
    Birch,
    Palm,
    Bush,
    Rock0,
    Rock1,
    Rock2,
    Boulder,
    Stump,
    HayBale,
    Barrel,
    Crate,
}

pub const PROP_KINDS: [PropKind; 13] = [
    PropKind::Pine,
    PropKind::Oak,
    PropKind::Birch,
    PropKind::Palm,
    PropKind::Bush,
    PropKind::Rock0,
    PropKind::Rock1,
    PropKind::Rock2,
    PropKind::Boulder,
    PropKind::Stump,
    PropKind::HayBale,
    PropKind::Barrel,
    PropKind::Crate,
];

impl PropKind {
    /// Mesh for a level of detail (0 = full, 1 = cheap distant version).
    pub fn mesh(self, lod: usize) -> MeshId {
        use MeshId::*;
        let l = lod.min(1);
        match self {
            PropKind::Pine => [Pine0, Pine1][l],
            PropKind::Oak => [Oak0, Oak1][l],
            PropKind::Birch => [Birch0, Birch1][l],
            PropKind::Palm => [Palm0, Palm1][l],
            PropKind::Bush => [Bush0, Bush1][l],
            PropKind::Rock0 => Rock0,
            PropKind::Rock1 => Rock1,
            PropKind::Rock2 => Rock2,
            PropKind::Boulder => Boulder,
            PropKind::Stump => Stump,
            PropKind::HayBale => HayBale,
            PropKind::Barrel => Barrel,
            PropKind::Crate => Crate,
        }
    }
    pub fn is_tree(self) -> bool {
        matches!(self, PropKind::Pine | PropKind::Oak | PropKind::Birch | PropKind::Palm)
    }
    pub fn casts_shadow(self) -> bool {
        !matches!(self, PropKind::Bush)
    }
    /// Distance beyond which this prop is not drawn at all.
    pub fn draw_distance(self) -> f32 {
        match self {
            PropKind::Pine | PropKind::Oak | PropKind::Birch | PropKind::Palm => 520.0,
            PropKind::Boulder => 450.0,
            PropKind::Bush => 160.0,
            _ => 230.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PropInst {
    pub kind: PropKind,
    pub pos: Vec3,
    pub yaw: f32,
    pub scale: f32,
    /// Linear colour multiplier (foliage hue etc.).
    pub tint: [f32; 3],
}

impl PropInst {
    pub fn instance(&self) -> Instance {
        Instance::at(self.pos, self.yaw, self.scale, [self.tint[0], self.tint[1], self.tint[2], 1.0])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarvestKind {
    Tree,
    Rock,
}

/// Something the pickaxe can chop down / break.
#[derive(Clone, Debug)]
pub struct Harvestable {
    pub kind: HarvestKind,
    pub pos: Vec3,
    pub height: f32,
    pub hp: f32,
    pub max_hp: f32,
    pub alive: bool,
    pub collider: u32,
    /// Chunk index and slot of the visual instance.
    pub chunk: u32,
    pub slot: u32,
    pub prop_kind: PropKind,
}

pub fn chunk_index_of(x: f32, z: f32) -> usize {
    let cx = (((x + WORLD_HALF) / CHUNK_SIZE).floor() as i32).clamp(0, CHUNKS as i32 - 1) as usize;
    let cz = (((z + WORLD_HALF) / CHUNK_SIZE).floor() as i32).clamp(0, CHUNKS as i32 - 1) as usize;
    cz * CHUNKS + cx
}

fn srgb_lin(hex: u32) -> [f32; 3] {
    let c = crate::mesh::hex(hex);
    [c.x.powf(2.2), c.y.powf(2.2), c.z.powf(2.2)]
}

fn tint_of(hex: u32, mul: f32, jitter: f32, rng: &mut Rng) -> [f32; 3] {
    let c = srgb_lin(hex);
    let k = mul * (1.0 + rng.range(-jitter, jitter));
    [c[0] * k, c[1] * k, c[2] * k]
}

pub struct ScatterOut {
    pub chunk_props: Vec<Vec<PropInst>>,
    pub harvest: Vec<Harvestable>,
}

impl ScatterOut {
    fn push(&mut self, p: PropInst) -> (u32, u32) {
        let ci = chunk_index_of(p.pos.x, p.pos.z);
        self.chunk_props[ci].push(p);
        (ci as u32, (self.chunk_props[ci].len() - 1) as u32)
    }
}

fn on_road_or_lawn(splat: &Splat, p: Vec2, extra: f32) -> bool {
    for (dx, dz) in [(0.0, 0.0), (extra, 0.0), (-extra, 0.0), (0.0, extra), (0.0, -extra)] {
        let s = splat.sample(p + Vec2::new(dx, dz));
        if s[CH_DIRT] > 0.35 || s[CH_ASPHALT] > 0.35 || s[CH_FIELD] > 0.35 {
            return true;
        }
    }
    false
}

/// Scatter trees, bushes, rocks. Colliders are inserted into `statics`.
pub fn scatter_nature(base: &BaseTerrain, layout: &Layout, hm: &Heightmap, painter: &TerrainPainter, splat: &Splat, statics: &mut SpatialGrid) -> ScatterOut {
    let mut out = ScatterOut { chunk_props: vec![Vec::new(); CHUNKS * CHUNKS], harvest: vec![] };
    let seed = base.seed;
    let mut rng = Rng::new(seed as u64 ^ 0x7EE5);
    let far_from_lake = |p: Vec2, extra: f32| layout.lakes.iter().all(|l| l.center.distance(p) > l.radius * 1.5 + extra);

    // town "core" test: sparse trees inside POIs
    let poi_factor = |p: Vec2| -> f32 {
        let mut f: f32 = 1.0;
        for poi in &layout.pois {
            let d = p.distance(poi.center);
            let r = poi.radius * 1.05;
            if d < r {
                let k = match poi.kind {
                    PoiKind::Lodge => 0.7,
                    PoiKind::Farm => 0.12,
                    _ => 0.1,
                };
                f = f.min(lerp(k, 1.0, smoothstep(r * 0.55, r, d)));
            }
        }
        f
    };

    let blocked_by_static = |statics: &SpatialGrid, p: Vec2, r: f32| -> bool {
        let mut hit = false;
        statics.query(&Aabb::new(Vec3::new(p.x - r, -50.0, p.y - r), Vec3::new(p.x + r, 200.0, p.y + r)), |_, c| {
            if matches!(c.tag, Tag::Building(_) | Tag::Prop(_)) || matches!(c.tag, Tag::Tree(_) | Tag::Rock(_)) {
                hit = true;
            }
        });
        hit
    };

    // ---- trees -----------------------------------------------------------------
    let step = 5.4;
    let n = (WORLD_HALF * 2.0 / step) as i32;
    for gz in 0..n {
        for gx in 0..n {
            let jx = hash2f(gx, gz, seed) - 0.5;
            let jz = hash2f(gx, gz, seed ^ 0x55) - 0.5;
            let p = Vec2::new(-WORLD_HALF + (gx as f32 + 0.5 + jx * 0.9) * step, -WORLD_HALF + (gz as f32 + 0.5 + jz * 0.9) * step);
            let h = hm.height_at(p.x, p.y);
            if h < 0.9 {
                continue;
            }
            let d_coast = base.coast_dist(p.x, p.y);
            if d_coast < 6.0 {
                continue;
            }
            let slope = hm.slope_at(p.x, p.y);
            if slope > 0.62 {
                continue;
            }
            let forest = painter.forest(p.x, p.y);
            let aut = painter.autumn(p.x, p.y);
            let beach = h < 4.2 && d_coast < 70.0;
            let mut prob = 0.04 + 0.75 * forest.powf(1.05);
            if beach {
                prob = 0.16 * smoothstep(8.0, 22.0, d_coast);
            }
            prob *= poi_factor(p);
            if rng.f32() > prob * 1.35 {
                continue;
            }
            if !far_from_lake(p, 5.0) || on_road_or_lawn(splat, p, 3.0) {
                continue;
            }
            if blocked_by_static(statics, p, 2.6) {
                continue;
            }
            // species
            let r = rng.f32();
            let high = h > 26.0;
            let kind;
            let tint;
            if beach {
                kind = PropKind::Palm;
                tint = tint_of(0x4da63a, 1.15, 0.12, &mut rng);
            } else if aut > 0.55 && !high {
                kind = if r < 0.6 { PropKind::Oak } else { PropKind::Birch };
                let pal = [0xe8892a, 0xd2562b, 0xf1c232, 0xc77a2c];
                tint = tint_of(*rng.pick(&pal), 1.2, 0.1, &mut rng);
            } else if high || (forest > 0.55 && r < 0.5) {
                kind = PropKind::Pine;
                tint = tint_of(*rng.pick(&[0x379048, 0x2f8043, 0x3d9c52, 0x35884b]), 1.2, 0.12, &mut rng);
            } else if r < 0.62 {
                kind = PropKind::Oak;
                tint = tint_of(*rng.pick(&[0x5fb23a, 0x4fa233, 0x74bf3f, 0x62ad45]), 1.2, 0.1, &mut rng);
            } else if r < 0.84 {
                kind = PropKind::Birch;
                tint = tint_of(*rng.pick(&[0x7bc24a, 0x8cca4f, 0x6fb844]), 1.2, 0.1, &mut rng);
            } else {
                kind = PropKind::Pine;
                tint = tint_of(*rng.pick(&[0x379048, 0x3d9c52]), 1.2, 0.12, &mut rng);
            }
            let scale = rng.range(0.78, 1.32) * if kind == PropKind::Pine { 1.1 } else { 1.0 };
            let pos = Vec3::new(p.x, h - 0.1, p.y);
            let inst = PropInst { kind, pos, yaw: rng.range(0.0, std::f32::consts::TAU), scale, tint };
            let (ci, slot) = out.push(inst);
            let (radius, height) = match kind {
                PropKind::Pine => (0.30 * scale, 10.0 * scale),
                PropKind::Oak => (0.36 * scale, 7.5 * scale),
                PropKind::Birch => (0.22 * scale, 7.5 * scale),
                _ => (0.26 * scale, 7.4 * scale),
            };
            let idx = out.harvest.len() as u32;
            let collider = statics.insert(Collider { shape: Shape::Cyl { cx: p.x, cz: p.y, r: radius.max(0.28), y0: h - 1.0, y1: h + height }, tag: Tag::Tree(idx), blocks_bullets: true });
            out.harvest.push(Harvestable { kind: HarvestKind::Tree, pos, height, hp: 100.0, max_hp: 100.0, alive: true, collider, chunk: ci, slot, prop_kind: kind });
        }
    }

    // ---- bushes ------------------------------------------------------------------
    let step = 4.6;
    let n = (WORLD_HALF * 2.0 / step) as i32;
    for gz in 0..n {
        for gx in 0..n {
            let jx = hash2f(gx, gz, seed ^ 0xB5) - 0.5;
            let jz = hash2f(gx, gz, seed ^ 0xB6) - 0.5;
            let p = Vec2::new(-WORLD_HALF + (gx as f32 + 0.5 + jx * 0.9) * step, -WORLD_HALF + (gz as f32 + 0.5 + jz * 0.9) * step);
            let h = hm.height_at(p.x, p.y);
            if h < 2.8 || base.coast_dist(p.x, p.y) < 14.0 {
                continue;
            }
            if hm.slope_at(p.x, p.y) > 0.6 || !far_from_lake(p, 3.0) {
                continue;
            }
            let forest = painter.forest(p.x, p.y);
            let prob = (0.03 + 0.34 * forest) * poi_factor(p).max(0.35);
            if rng.f32() > prob {
                continue;
            }
            if on_road_or_lawn(splat, p, 1.5) || blocked_by_static(statics, p, 1.4) {
                continue;
            }
            let aut = painter.autumn(p.x, p.y);
            let tint = if aut > 0.55 { tint_of(*rng.pick(&[0xd9892c, 0xb8582b, 0x8aa83a]), 1.15, 0.1, &mut rng) } else { tint_of(*rng.pick(&[0x5aa83a, 0x4a9a35, 0x6cbb42, 0x3f8f3a]), 1.2, 0.12, &mut rng) };
            out.push(PropInst { kind: PropKind::Bush, pos: Vec3::new(p.x, h - 0.1, p.y), yaw: rng.range(0.0, std::f32::consts::TAU), scale: rng.range(0.8, 1.5), tint });
        }
    }

    // ---- rocks -------------------------------------------------------------------
    let step = 13.0;
    let n = (WORLD_HALF * 2.0 / step) as i32;
    for gz in 0..n {
        for gx in 0..n {
            let jx = hash2f(gx, gz, seed ^ 0xC1) - 0.5;
            let jz = hash2f(gx, gz, seed ^ 0xC2) - 0.5;
            let p = Vec2::new(-WORLD_HALF + (gx as f32 + 0.5 + jx * 0.9) * step, -WORLD_HALF + (gz as f32 + 0.5 + jz * 0.9) * step);
            let h = hm.height_at(p.x, p.y);
            if h < 1.2 || base.coast_dist(p.x, p.y) < 4.0 || !far_from_lake(p, 0.0) {
                continue;
            }
            let slope = hm.slope_at(p.x, p.y);
            let prob = (0.08 + slope * 0.75 + (h / 70.0) * 0.12) * poi_factor(p);
            if rng.f32() > prob {
                continue;
            }
            if on_road_or_lawn(splat, p, 2.0) || blocked_by_static(statics, p, 2.2) {
                continue;
            }
            let big = (slope > 0.4 || h > 30.0) && rng.chance(0.4);
            let kind = if big { PropKind::Boulder } else { *rng.pick(&[PropKind::Rock0, PropKind::Rock1, PropKind::Rock2]) };
            let scale = if big { rng.range(0.9, 1.6) } else { rng.range(0.7, 1.7) };
            let g = rng.range(0.85, 1.15);
            let tint = [g, g * rng.range(0.95, 1.05), g * rng.range(0.95, 1.08)];
            let pos = Vec3::new(p.x, h - 0.15 * scale, p.y);
            let (ci, slot) = out.push(PropInst { kind, pos, yaw: rng.range(0.0, std::f32::consts::TAU), scale, tint });
            let radius = if big { 2.0 * scale * 0.85 } else { 0.9 * scale * 0.85 };
            let height = if big { 2.4 * scale } else { 0.9 * scale };
            let idx = out.harvest.len() as u32;
            let collider = statics.insert(Collider { shape: Shape::Cyl { cx: p.x, cz: p.y, r: radius, y0: h - 1.0, y1: h + height }, tag: Tag::Rock(idx), blocks_bullets: true });
            out.harvest.push(Harvestable { kind: HarvestKind::Rock, pos, height, hp: if big { 220.0 } else { 120.0 }, max_hp: if big { 220.0 } else { 120.0 }, alive: true, collider, chunk: ci, slot, prop_kind: kind });
        }
    }
    out
}

// ---- grass and flowers -------------------------------------------------------------

/// Deterministic grass tufts and flower clumps for one terrain chunk. Called lazily
/// as the camera approaches; returns (grass tufts, flower clumps).
pub fn scatter_ground_cover(world: &super::World, cx: usize, cz: usize) -> (Vec<Instance>, Vec<Instance>) {
    let hm = &world.hm;
    let seed = world.seed;
    let x0 = -WORLD_HALF + cx as f32 * CHUNK_SIZE;
    let z0 = -WORLD_HALF + cz as f32 * CHUNK_SIZE;
    let mut grass = vec![];
    let mut flowers = vec![];
    let step = 1.25;
    let n = (CHUNK_SIZE / step) as i32;
    // footprints (building floors, docks) overlapping this chunk, grown a little so the yard is clear too
    let margin = 0.6;
    let cover_free: Vec<&(Vec2, Vec2)> = world
        .no_cover
        .iter()
        .filter(|(mn, mx)| mn.x - margin < x0 + CHUNK_SIZE && mx.x + margin > x0 && mn.y - margin < z0 + CHUNK_SIZE && mx.y + margin > z0)
        .collect();
    for gz in 0..n {
        for gx in 0..n {
            let (ix, iz) = (cx as i32 * n + gx, cz as i32 * n + gz);
            let jx = hash2f(ix, iz, seed ^ 0x61);
            let jz = hash2f(ix, iz, seed ^ 0x62);
            let p = Vec2::new(x0 + (gx as f32 + jx) * step, z0 + (gz as f32 + jz) * step);
            let h = hm.height_at(p.x, p.y);
            if h < 2.6 {
                continue;
            }
            if cover_free.iter().any(|(mn, mx)| p.x > mn.x - margin && p.x < mx.x + margin && p.y > mn.y - margin && p.y < mx.y + margin) {
                continue;
            }
            let s = world.splat.sample(p);
            if s[CH_DIRT] > 0.2 || s[CH_ASPHALT] > 0.2 || s[CH_FIELD] > 0.2 {
                continue;
            }
            let slope = hm.slope_at(p.x, p.y);
            if slope > 0.65 || h > 42.0 {
                continue;
            }
            let lawn = s[CH_LAWN];
            // thinner grass on lawns and in the forest shade
            let forest = world.painter.forest(p.x, p.y);
            let density = (0.85 - forest * 0.25) * (1.0 - lawn * 0.35);
            let r = hash2f(ix, iz, seed ^ 0x63);
            if r > density {
                continue;
            }
            let tone = hash2f(ix, iz, seed ^ 0x64);
            let aut = world.painter.autumn(p.x, p.y);
            let base_hex: u32 = if aut > 0.5 { 0xd0a43c } else if tone < 0.3 { 0x5fb23a } else if tone < 0.7 { 0x76c445 } else { 0x4ea231 };
            let tint = srgb_lin(base_hex);
            let sc = 0.9 + hash2f(ix, iz, seed ^ 0x65) * 0.8;
            let pos = Vec3::new(p.x, h - 0.03, p.y);
            let yaw = hash2f(ix, iz, seed ^ 0x66) * std::f32::consts::TAU;
            grass.push(Instance::at(pos, yaw, sc, [tint[0] * 1.5, tint[1] * 1.5, tint[2] * 1.5, 1.0]));
            // flowers in meadows
            let meadow = smoothstep(0.55, 0.7, crate::noise::fbm01(p.x / 55.0 + 4.0, p.y / 55.0 - 9.0, 3, seed ^ 0xF1));
            if meadow > 0.0 && r < density * 0.11 * meadow && slope < 0.4 {
                let pal = [0xff6fa5u32, 0xffd23f, 0xffffff, 0xb36bff, 0xff7a3d, 0x59c2ff];
                let c = srgb_lin(pal[(hash2(ix, iz, seed ^ 0x67) % pal.len() as u32) as usize]);
                flowers.push(Instance::at(pos, yaw, 0.9 + tone * 0.6, [c[0] * 1.8, c[1] * 1.8, c[2] * 1.8, 1.0]));
            }
        }
    }
    (grass, flowers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grass_and_flowers_stay_out_of_building_footprints() {
        let w = crate::world::World::generate(1234);
        assert!(!w.no_cover.is_empty());
        // check every chunk that holds a building
        let mut chunks: Vec<usize> = w.buildings.iter().map(|b| chunk_index_of(b.aabb.center().x, b.aabb.center().z)).collect();
        chunks.sort_unstable();
        chunks.dedup();
        let mut tested = 0;
        for ci in chunks {
            let (grass, flowers) = scatter_ground_cover(&w, ci % CHUNKS, ci / CHUNKS);
            for inst in grass.iter().chain(&flowers) {
                let (x, z) = (inst.m0[3], inst.m2[3]);
                for b in &w.buildings {
                    let inside = x > b.aabb.min.x && x < b.aabb.max.x && z > b.aabb.min.z && z < b.aabb.max.z;
                    assert!(!inside, "ground cover at ({x:.1}, {z:.1}) is inside {} {}", b.kind, b.id);
                }
                tested += 1;
            }
        }
        assert!(tested > 100, "expected plenty of grass around towns, got {tested}");
    }

    #[test]
    fn chunk_index_bounds() {
        assert_eq!(chunk_index_of(-WORLD_HALF + 1.0, -WORLD_HALF + 1.0), 0);
        assert_eq!(chunk_index_of(WORLD_HALF - 1.0, WORLD_HALF - 1.0), CHUNKS * CHUNKS - 1);
        assert_eq!(chunk_index_of(10_000.0, -10_000.0), CHUNKS - 1);
    }

    #[test]
    fn prop_meshes_resolve() {
        for k in PROP_KINDS {
            let _ = k.mesh(0);
            let _ = k.mesh(1);
        }
        assert!(PropKind::Pine.is_tree() && !PropKind::Bush.is_tree());
    }
}
