//! The island: terrain, towns, trees, colliders, navigation data.

pub mod buildings;
pub mod collision;
pub mod gen;
pub mod heightmap;
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

#[derive(Clone, Debug)]
pub struct ChestSpot {
    pub pos: Vec3,
    pub yaw: f32,
    pub building: Option<u32>,
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
    pub fn generate(seed: u32) -> World {
        let base = BaseTerrain::new(seed);
        let layout = gen::plan_layout(&base);
        let hm = gen::build_heightmap(&base, &layout);
        let painter = TerrainPainter { seed };
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
            for l in &p.loot {
                loot_spots.push(LootSpot { pos: *l, building: Some(b.id) });
            }
            for (pos, yaw) in &p.chests {
                chest_spots.push(ChestSpot { pos: *pos, yaw: *yaw, building: Some(b.id) });
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
                if h < 2.0 || splat.sample(p)[splat::CH_ASPHALT] > 0.3 {
                    continue;
                }
                let mut blocked = false;
                statics.query(&Aabb::new(Vec3::new(p.x - 1.5, -10.0, p.y - 1.5), Vec3::new(p.x + 1.5, 100.0, p.y + 1.5)), |_, _| blocked = true);
                if !blocked {
                    loot_spots.push(LootSpot { pos: Vec3::new(p.x, h, p.y), building: None });
                }
            }
        }

        let mut world = World { seed, base, layout, hm, painter, terrain, splat, statics, buildings, chunk_props: nature.chunk_props, harvest: nature.harvest, chunk_meshes: cm, windmills, loot_spots, chest_spots, nav: NavGrid::new(WORLD_HALF, NAV_CELL), no_cover };
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
