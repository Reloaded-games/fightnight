//! The island: terrain, towns, trees, colliders, navigation data.

pub mod buildings;
pub mod collision;
pub mod gen;
pub mod heightmap;
pub mod maps;
mod maps_data;
pub mod minimap;
pub mod nav;
pub mod props;
pub mod splat;
pub mod terrain_mesh;
pub mod towns;

use crate::math::*;
use crate::mesh::MeshData;
use crate::rng::Rng;
use collision::*;
use gen::{BaseTerrain, Layout, RoadKind};
use heightmap::Heightmap;
use nav::NavGrid;
use props::{Harvestable, PropInst};
use splat::Splat;
use std::collections::HashMap;
use terrain_mesh::{TerrainMesh, TerrainPainter};

/// The larger island keeps the same terrain and navigation grid budgets. Three
/// metre terrain cells let vehicles cross 2.25 times the previous map area
/// without allocating 2.25 times as many terrain vertices on every client.
pub const WORLD_SIZE: f32 = 1920.0;
pub const WORLD_HALF: f32 = WORLD_SIZE * 0.5;
pub const CHUNK_CELLS: usize = 32;
pub const CHUNKS: usize = 20;
pub const GRID_N: usize = CHUNKS * CHUNK_CELLS + 1;
pub const CELL: f32 = WORLD_SIZE / (GRID_N - 1) as f32;
pub const CHUNK_SIZE: f32 = CHUNK_CELLS as f32 * CELL;
pub const SEA_LEVEL: f32 = 0.0;
pub const NAV_CELL: f32 = CELL * 2.0;
/// Chests lying in the open (on top of the ones inside buildings), the least distance between any two chests and how hard to look for room.
pub const OUTDOOR_CHESTS: usize = 56;
pub const OUTDOOR_CHEST_SPACING: f32 = 70.0;
const OUTDOOR_CHEST_TRIES: usize = 8000;
/// Supply Llamas hidden out in the wild, away from the towns.
pub const LLAMAS: usize = 4;

/// Whether anything solid (a wall, a table, a bed) takes up the space a box `margin` metres around `p` and `height` tall would need.
fn crowded(statics: &SpatialGrid, p: Vec3, margin: f32, height: f32) -> bool {
    let q = Aabb::new(Vec3::new(p.x - margin, p.y + 0.05, p.z - margin), Vec3::new(p.x + margin, p.y + height, p.z + margin));
    let mut hit = false;
    // (the grid hands over everything in the cells the box touches)
    statics.query(&q, |_, c| hit |= c.shape.aabb().intersects(&q));
    hit
}

#[derive(Clone, Debug)]
pub struct BuildingInfo {
    pub id: u32,
    pub poi: usize,
    pub kind: &'static str,
    pub aabb: Aabb,
    pub door_out: Vec3,
    pub door_in: Vec3,
    pub center: Vec3,
    /// Quarter turns the building was rotated by.
    pub rot: u8,
    /// The flight of stairs up to the second floor, if the building has one.
    pub stairs: Option<buildings::Stairs>,
}

#[derive(Clone, Debug)]
pub struct LootSpot {
    pub pos: Vec3,
    pub building: Option<u32>,
}

/// What sort of container stands somewhere. Supply drops are never part of the island: they come down during a match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChestKind {
    /// The golden chest: a weapon, ammo for it and healing.
    Treasure,
    /// A small green box of ammunition.
    AmmoBox,
    /// A Supply Llama: materials, ammo, healing and utility items.
    Llama,
    /// A supply drop (see `game::supply`).
    Supply,
}

#[derive(Clone, Debug)]
pub struct ChestSpot {
    pub pos: Vec3,
    pub yaw: f32,
    pub building: Option<u32>,
    pub kind: ChestKind,
}

/// Merged static geometry (buildings, decor) belonging to one terrain chunk.
pub struct ChunkMesh {
    pub chunk: usize,
    pub mesh: MeshData,
    pub aabb: Aabb,
}

#[derive(Clone, Copy, Debug)]
pub struct Windmill {
    pub hub: Vec3,
    pub rot: u8,
}

pub struct World {
    pub seed: u32,
    /// Which island this is (the procedural one, or a recreated Fortnite map).
    pub map: maps::MapKind,
    pub base: BaseTerrain,
    pub layout: Layout,
    pub hm: Heightmap,
    pub painter: TerrainPainter,
    pub terrain: TerrainMesh,
    pub splat: Splat,
    pub statics: SpatialGrid,
    pub buildings: Vec<BuildingInfo>,
    pub chunk_props: Vec<Vec<PropInst>>,
    pub harvest: Vec<Harvestable>,
    pub chunk_meshes: Vec<ChunkMesh>,
    pub windmills: Vec<Windmill>,
    pub loot_spots: Vec<LootSpot>,
    pub chest_spots: Vec<ChestSpot>,
    pub nav: NavGrid,
    /// Ground rectangles (min, max in x/z) where grass and flowers must not grow: building footprints, docks...
    pub no_cover: Vec<(Vec2, Vec2)>,
}

impl World {
    /// The procedural island for `seed`.
    pub fn generate(seed: u32) -> World {
        World::generate_map(seed, maps::MapKind::Island)
    }

    /// The island of a match: the procedural one for `seed`, or a recreated map (`seed` then only varies the small
    /// details: trees, rocks, loot).
    pub fn generate_map(seed: u32, map: maps::MapKind) -> World {
        let spec = map.spec();
        let base = match spec {
            Some(spec) => BaseTerrain::from_spec(seed, spec),
            None => BaseTerrain::new(seed),
        };
        let layout = gen::plan_layout(&base);
        let hm = gen::build_heightmap(&base, &layout);
        let painter = TerrainPainter { seed, spec };
        let mut splat = Splat::new();
        for r in &layout.roads {
            let ch = if r.kind == RoadKind::Asphalt { splat::CH_ASPHALT } else { splat::CH_DIRT };
            splat.paint_polyline(&r.pts, r.width * 0.5, ch);
        }
        let towns = towns::build_towns(&layout, &hm, &mut splat, seed);

        let mut statics = SpatialGrid::new(WORLD_HALF + 16.0, 8.0);
        let mut buildings = vec![];
        let mut chunk_meshes: HashMap<usize, MeshData> = HashMap::new();
        let mut windmills = vec![];
        let mut loot_spots = vec![];
        let mut chest_spots = vec![];
        for b in &towns.buildings {
            let p = &b.placed;
            for c in &p.cols {
                statics.insert(*c);
            }
            let center = p.aabb.center();
            let ci = props::chunk_index_of(center.x, center.z);
            chunk_meshes.entry(ci).or_default().append(&p.mesh, Mat4::IDENTITY);
            buildings.push(BuildingInfo { id: b.id, poi: b.poi, kind: b.kind, aabb: p.aabb, door_out: p.door_out, door_in: p.door_in, center, rot: p.rot, stairs: p.stairs });
            for (pos, yaw) in &p.chests {
                chest_spots.push(ChestSpot { pos: *pos, yaw: *yaw, building: Some(b.id), kind: ChestKind::Treasure });
            }
            // one of the loot spots of a building with a few becomes an ammo box, where there is room for one
            let mut brng = Rng::new(seed as u64 ^ 0xA770 ^ ((b.id as u64) << 20));
            let boxed = if p.loot.len() >= 2 { Some(brng.below(p.loot.len())) } else { None };
            for (k, l) in p.loot.iter().enumerate() {
                if Some(k) == boxed && !crowded(&statics, *l, 0.4, 0.8) {
                    let yaw = brng.below(4) as f32 * std::f32::consts::FRAC_PI_2;
                    chest_spots.push(ChestSpot { pos: *l, yaw, building: Some(b.id), kind: ChestKind::AmmoBox });
                } else {
                    loot_spots.push(LootSpot { pos: *l, building: Some(b.id) });
                }
            }
            if let Some(h) = p.hub {
                windmills.push(Windmill { hub: h, rot: p.rot });
            }
        }
        for (mesh, pos) in &towns.decor {
            let ci = props::chunk_index_of(pos.x, pos.z);
            chunk_meshes.entry(ci).or_default().append(mesh, Mat4::IDENTITY);
        }
        for c in &towns.decor_cols {
            statics.insert(*c);
        }

        // grass would poke through floors and planks, so keep it out of footprints
        let mut no_cover: Vec<(Vec2, Vec2)> = buildings.iter().map(|b: &BuildingInfo| (Vec2::new(b.aabb.min.x, b.aabb.min.z), Vec2::new(b.aabb.max.x, b.aabb.max.z))).collect();
        // ... and out of the steps in front of raised doors
        no_cover.extend(towns.buildings.iter().filter_map(|b| b.placed.steps).map(|bb| (Vec2::new(bb.min.x, bb.min.z), Vec2::new(bb.max.x, bb.max.z))));
        for (mesh, _) in &towns.decor {
            let bb = mesh.bounds();
            let (dx, dz) = (bb.max.x - bb.min.x, bb.max.z - bb.min.z);
            if dx.is_finite() && dz.is_finite() && dx * dz > 3.0 {
                no_cover.push((Vec2::new(bb.min.x, bb.min.z), Vec2::new(bb.max.x, bb.max.z)));
            }
        }

        let mut nature = props::scatter_nature(&base, &layout, &hm, &painter, &splat, &mut statics);
        for p in &towns.props {
            let ci = props::chunk_index_of(p.pos.x, p.pos.z);
            nature.chunk_props[ci].push(*p);
        }

        let terrain = terrain_mesh::build_terrain_mesh(&hm, &painter);

        let mut cm: Vec<ChunkMesh> = chunk_meshes
            .into_iter()
            .map(|(chunk, mesh)| {
                let aabb = mesh.bounds();
                ChunkMesh { chunk, mesh, aabb }
            })
            .collect();
        cm.sort_by_key(|c| c.chunk);

        // Outdoor loot near towns so players always find something on landing.
        let mut rng = Rng::new(seed as u64 ^ 0x100D);
        for poi in &layout.pois {
            let n = match poi.kind {
                gen::PoiKind::Town => 14,
                gen::PoiKind::Station | gen::PoiKind::Hamlet => 5,
                _ => 8,
            };
            for _ in 0..n {
                let p = poi.center + rng.in_disc(poi.radius * 0.85);
                let h = hm.height_at(p.x, p.y);
                // (not on the bed of a lake either)
                let wet = layout.lakes.iter().any(|l| l.center.distance(p) < l.radius * 1.3 && h < l.level + 0.2);
                if h < 2.0 || wet || splat.sample(p)[splat::CH_ASPHALT] > 0.3 {
                    continue;
                }
                let mut blocked = false;
                statics.query(&Aabb::new(Vec3::new(p.x - 1.5, -10.0, p.y - 1.5), Vec3::new(p.x + 1.5, 100.0, p.y + 1.5)), |_, _| blocked = true);
                if !blocked {
                    // now and then an ammo box instead of something lying on the grass
                    if rng.chance(0.12) {
                        let yaw = rng.range(0.0, std::f32::consts::TAU);
                        chest_spots.push(ChestSpot { pos: Vec3::new(p.x, h, p.y), yaw, building: None, kind: ChestKind::AmmoBox });
                    } else {
                        loot_spots.push(LootSpot { pos: Vec3::new(p.x, h, p.y), building: None });
                    }
                }
            }
        }

        // Chests out in the open, spread over the whole island: not every one has to be raided from inside a town.
        let mut crng = Rng::new(seed as u64 ^ 0xC4E57);
        let mut outdoor = 0;
        for _ in 0..OUTDOOR_CHEST_TRIES {
            if outdoor >= OUTDOOR_CHESTS {
                break;
            }
            let p = crng.in_disc(WORLD_HALF * 0.86);
            let h = hm.height_at(p.x, p.y);
            let yaw = crng.range(0.0, std::f32::consts::TAU);
            // dry land, level enough for the chest to sit flat, clear of the sea, the lakes and the roads
            if h < 2.5 || hm.slope_at(p.x, p.y) > 0.16 || layout.lakes.iter().any(|l| l.center.distance(p) < l.radius * 1.4) {
                continue;
            }
            let ground = splat.sample(p);
            if ground[splat::CH_ASPHALT] > 0.05 || ground[splat::CH_DIRT] > 0.35 {
                continue;
            }
            if chest_spots.iter().any(|c| c.kind == ChestKind::Treasure && Vec2::new(c.pos.x, c.pos.z).distance(p) < OUTDOOR_CHEST_SPACING) {
                continue;
            }
            let mut blocked = false;
            statics.query(&Aabb::new(Vec3::new(p.x - 2.0, -10.0, p.y - 2.0), Vec3::new(p.x + 2.0, 100.0, p.y + 2.0)), |_, _| blocked = true);
            if blocked {
                continue;
            }
            chest_spots.push(ChestSpot { pos: Vec3::new(p.x, h, p.y), yaw, building: None, kind: ChestKind::Treasure });
            outdoor += 1;
        }

        // Supply Llamas: a few, hidden out in the wild well away from the towns and from each other.
        let mut lrng = Rng::new(seed as u64 ^ 0x11A3A);
        let mut llamas: Vec<Vec2> = vec![];
        for _ in 0..OUTDOOR_CHEST_TRIES {
            if llamas.len() >= LLAMAS {
                break;
            }
            let p = lrng.in_disc(WORLD_HALF * 0.82);
            let h = hm.height_at(p.x, p.y);
            let yaw = lrng.range(0.0, std::f32::consts::TAU);
            if h < 3.0 || hm.slope_at(p.x, p.y) > 0.2 || layout.lakes.iter().any(|l| l.center.distance(p) < l.radius * 1.4) {
                continue;
            }
            if layout.pois.iter().any(|poi| poi.center.distance(p) < poi.radius + 40.0) || splat.sample(p)[splat::CH_ASPHALT] > 0.05 {
                continue;
            }
            // (a recreated map is busier: the llamas are not quite as far apart there)
            let apart = if spec.is_some() { WORLD_HALF * 0.32 } else { WORLD_HALF * 0.45 };
            if llamas.iter().any(|q| q.distance(p) < apart) || chest_spots.iter().any(|c| Vec2::new(c.pos.x, c.pos.z).distance(p) < 25.0) {
                continue;
            }
            let mut blocked = false;
            statics.query(&Aabb::new(Vec3::new(p.x - 2.0, -10.0, p.y - 2.0), Vec3::new(p.x + 2.0, 100.0, p.y + 2.0)), |_, _| blocked = true);
            if blocked {
                continue;
            }
            chest_spots.push(ChestSpot { pos: Vec3::new(p.x, h, p.y), yaw, building: None, kind: ChestKind::Llama });
            llamas.push(p);
        }

        let mut world = World { seed, map, base, layout, hm, painter, terrain, splat, statics, buildings, chunk_props: nature.chunk_props, harvest: nature.harvest, chunk_meshes: cm, windmills, loot_spots, chest_spots, nav: NavGrid::new(WORLD_HALF, NAV_CELL), no_cover };
        world.build_nav();
        world
    }

    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        self.hm.height_at(x, z)
    }

    pub fn on_land(&self, x: f32, z: f32) -> bool {
        self.hm.height_at(x, z) > 0.3
    }

    /// Mark blocked navigation cells: water, cliffs, trunks, rocks, buildings (with door corridors).
    fn build_nav(&mut self) {
        let mut nav = NavGrid::new(WORLD_HALF, NAV_CELL);
        let (w, h) = (nav.w as i32, nav.h as i32);
        for j in 0..h {
            for i in 0..w {
                let c = nav.center(i, j);
                let ht = self.hm.height_at(c.x, c.y);
                // (a river is crossed at its fords, where the roads go over it)
                let blocked = ht < -0.3 || self.hm.slope_at(c.x, c.y) > 0.85 || self.layout.lakes.iter().any(|l| l.center.distance(c) < l.radius * 1.02 && ht < l.level - 0.35);
                if blocked {
                    nav.set_blocked(i, j, true);
                }
            }
        }
        // static colliders
        for item in self.statics.items.iter().flatten() {
            match (item.tag, item.shape) {
                (Tag::Tree(_), Shape::Cyl { cx, cz, .. }) | (Tag::Rock(_), Shape::Cyl { cx, cz, .. }) => {
                    let (i, j) = nav.cell_of(Vec2::new(cx, cz));
                    nav.set_blocked(i, j, true);
                }
                _ => {}
            }
        }
        for b in &self.buildings {
            let min = Vec2::new(b.aabb.min.x - 1.2, b.aabb.min.z - 1.2);
            let max = Vec2::new(b.aabb.max.x + 1.2, b.aabb.max.z + 1.2);
            let (i0, j0) = nav.cell_of(min);
            let (i1, j1) = nav.cell_of(max);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    nav.set_blocked(i, j, true);
                }
            }
        }
        // door corridors: from the door outward by 10 m
        for b in &self.buildings {
            let out = Vec2::new(b.door_out.x, b.door_out.z);
            let inn = Vec2::new(b.door_in.x, b.door_in.z);
            let dir = (out - inn).normalize_or_zero();
            for s in 0..12 {
                let p = inn + dir * (s as f32 * 1.5);
                let (i, j) = nav.cell_of(p);
                nav.set_blocked(i, j, false);
            }
        }
        self.nav = nav;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_generates_with_content() {
        let w = World::generate(1234);
        assert_eq!(w.terrain.chunks.len(), CHUNKS * CHUNKS);
        assert_eq!(w.hm.n, 641, "larger map must retain the terrain memory budget");
        assert_eq!(w.hm.half, WORLD_HALF);
        assert_eq!(w.nav.w * w.nav.h, 320 * 320, "navigation cost must stay bounded");
        assert_eq!(CHUNKS as f32 * CHUNK_SIZE, WORLD_SIZE);
        assert!(w.layout.pois.len() >= 13, "{} destinations", w.layout.pois.len());
        assert!(w.layout.pois.iter().any(|p| p.center.length() > 500.0), "new destinations must extend beyond the old island");
        assert!(w.hm.height_at(0.0, 0.0).is_finite());
        assert!(w.buildings.len() >= 55, "{} buildings", w.buildings.len());
        let trees = w.harvest.iter().filter(|h| h.kind == props::HarvestKind::Tree).count();
        let rocks = w.harvest.iter().filter(|h| matches!(h.kind, props::HarvestKind::Rock | props::HarvestKind::Ore)).count();
        let ore = w.harvest.iter().filter(|h| h.kind == props::HarvestKind::Ore).count();
        let props_total: usize = w.chunk_props.iter().map(|c| c.len()).sum();
        assert!(trees > 2400, "{trees} trees");
        assert!(rocks > 80, "{rocks} rocks");
        assert!(ore * 8 > rocks && ore * 2 < rocks, "{ore} of {rocks} rocks are ore: metal must be findable but rarer than stone");
        assert!(props_total > 4500, "{props_total} props");
        assert!(w.chest_spots.len() >= 8, "{} chests", w.chest_spots.len());
        assert!(w.loot_spots.len() >= 60, "{} loot spots", w.loot_spots.len());
        assert!(!w.chunk_meshes.is_empty());
        let town = &w.layout.pois[0];
        assert!(w.nav.nearest_free(town.center, 4).is_some());
    }

    #[test]
    fn chests_are_scattered_across_the_whole_island() {
        let w = crate::game::env::testutil::world();
        let in_the_open = |c: &&ChestSpot| c.building.is_none() && c.kind == ChestKind::Treasure;
        let outdoor: Vec<Vec2> = w.chest_spots.iter().filter(in_the_open).map(|c| Vec2::new(c.pos.x, c.pos.z)).collect();
        assert!(outdoor.len() >= OUTDOOR_CHESTS * 4 / 5, "{} chests in the open", outdoor.len());
        for c in w.chest_spots.iter().filter(in_the_open) {
            assert!(c.pos.y > 2.5 && w.on_land(c.pos.x, c.pos.z), "chest in the water at {:?}", c.pos);
        }
        // apart from each other
        for (i, a) in outdoor.iter().enumerate() {
            for b in &outdoor[i + 1..] {
                assert!(a.distance(*b) >= OUTDOOR_CHEST_SPACING - 0.01, "two chests {:.0} m apart", a.distance(*b));
            }
        }
        // spread over the island: every quarter of it has several, and some lie far out
        for (sx, sz) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            let n = outdoor.iter().filter(|p| p.x * sx > 0.0 && p.y * sz > 0.0).count();
            assert!(n >= 6, "{n} chests in the quarter ({sx}, {sz})");
        }
        assert!(outdoor.iter().any(|p| p.length() > WORLD_HALF * 0.5), "no chest far from the middle");
        // and a good share is out in the wild rather than beside a town
        let wild = outdoor.iter().filter(|p| w.layout.pois.iter().all(|poi| poi.center.distance(**p) > poi.radius)).count();
        assert!(wild * 2 >= outdoor.len(), "only {wild} of {} chests are away from the towns", outdoor.len());
        // every client builds the same island, so the same chests
        let again = World::generate(1234);
        assert_eq!(again.chest_spots.len(), w.chest_spots.len());
        assert!(again.chest_spots.iter().zip(&w.chest_spots).all(|(a, b)| a.pos == b.pos && a.yaw == b.yaw && a.kind == b.kind));
    }

    #[test]
    fn ammo_boxes_stand_in_houses_and_by_the_towns_and_llamas_hide_in_the_wild() {
        let w = crate::game::env::testutil::world();
        let boxes: Vec<&ChestSpot> = w.chest_spots.iter().filter(|c| c.kind == ChestKind::AmmoBox).collect();
        let indoors = boxes.iter().filter(|c| c.building.is_some()).count();
        assert!(indoors >= 30, "{indoors} ammo boxes inside buildings");
        assert!(boxes.len() > indoors, "and some outside, by the towns");
        for b in &boxes {
            assert!(!crowded(&w.statics, b.pos, 0.3, 0.6) || b.building.is_none(), "an ammo box inside the furniture at {:?}", b.pos);
        }
        let llamas: Vec<&ChestSpot> = w.chest_spots.iter().filter(|c| c.kind == ChestKind::Llama).collect();
        assert_eq!(llamas.len(), LLAMAS, "every llama found a hiding place");
        for l in &llamas {
            let p = Vec2::new(l.pos.x, l.pos.z);
            assert!(w.on_land(p.x, p.y) && l.pos.y > 3.0);
            assert!(w.layout.pois.iter().all(|poi| poi.center.distance(p) >= poi.radius + 40.0), "a llama in town at {p:?}");
            for o in &llamas {
                let q = Vec2::new(o.pos.x, o.pos.z);
                assert!(q == p || q.distance(p) >= WORLD_HALF * 0.45, "two llamas {:.0} m apart", q.distance(p));
            }
        }
        // every floor loot spot that became an ammo box is no longer a loot spot as well
        for b in boxes.iter().filter(|c| c.building.is_some()) {
            assert!(w.loot_spots.iter().all(|l| l.pos.distance(b.pos) > 0.01));
        }
    }

    #[test]
    fn loot_and_chests_do_not_spawn_inside_furniture() {
        let w = World::generate(1234);
        // an item (about 0.3 m tall) or a chest (0.9 x 0.6 x 0.8 m) is "embedded" when a solid reaches above its top over its footprint
        let embedded = |p: Vec3, margin: f32, height: f32| -> Option<String> {
            let q = Aabb::new(Vec3::new(p.x - margin, p.y + 0.05, p.z - margin), Vec3::new(p.x + margin, p.y + height, p.z + margin));
            let mut hit = None;
            w.statics.query(&q, |_, c| {
                let bb = c.shape.aabb();
                if matches!(c.shape, Shape::Box { .. }) && bb.min.y < p.y + 0.25 && bb.max.y > p.y + height && bb.min.x < p.x + margin && bb.max.x > p.x - margin && bb.min.z < p.z + margin && bb.max.z > p.z - margin {
                    hit = Some(format!("{:?} x {:.2}..{:.2} y {:.2}..{:.2} z {:.2}..{:.2}", c.tag, bb.min.x, bb.max.x, bb.min.y, bb.max.y, bb.min.z, bb.max.z));
                }
            });
            hit
        };
        let mut bad = vec![];
        for l in w.loot_spots.iter().filter(|l| l.building.is_some()) {
            if let Some(d) = embedded(l.pos, 0.12, 0.35) {
                bad.push(format!("loot at {:?} inside {d}", l.pos));
            }
        }
        for c in &w.chest_spots {
            if let Some(d) = embedded(c.pos, 0.4, 0.8) {
                bad.push(format!("chest at {:?} inside {d}", c.pos));
            }
        }
        assert!(bad.is_empty(), "{} spots are inside furniture:\n{}", bad.len(), bad.join("\n"));
    }

    #[test]
    fn trunk_colliders_stand_on_the_ground() {
        let w = World::generate(1234);
        for hv in w.harvest.iter().take(300) {
            let c = w.statics.get(hv.collider).expect("collider exists");
            if let Shape::Cyl { cx, cz, y0, y1, .. } = c.shape {
                let g = w.hm.height_at(cx, cz);
                assert!(y0 < g && y1 > g + 0.5, "collider vertical range wrong");
            }
        }
    }

    #[test]
    fn no_trees_inside_buildings() {
        let w = World::generate(1234);
        for b in &w.buildings {
            let bb = b.aabb.expanded(0.3);
            for hv in w.harvest.iter().filter(|h| h.kind == props::HarvestKind::Tree) {
                assert!(!(hv.pos.x > bb.min.x && hv.pos.x < bb.max.x && hv.pos.z > bb.min.z && hv.pos.z < bb.max.z), "tree inside {}", b.kind);
            }
        }
    }
}
