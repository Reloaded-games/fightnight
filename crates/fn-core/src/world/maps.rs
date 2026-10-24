//! Fixed islands recreated from the real Fortnite maps: the coastline, the high ground, the water, the named places and
//! the roads between them, written down as tables. The island generator follows them instead of its own noise
//! (`MapKind::Island`, the procedural island, stays as it was).
//!
//! Positions are in the units of the in-game map grid: x runs 0..10 from west to east (columns A..J), y runs 0..10 from
//! north to south (rows 1..10), so the middle of cell D6 is (3.5, 5.5). The grid covers the whole square world, sea
//! included; north is -Z in the world, up on the map.

use super::gen::PoiKind;
use super::WORLD_SIZE;
use crate::math::*;

/// Which island a match is played on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MapKind {
    /// The procedural island (a new one for every seed).
    #[default]
    Island,
    /// Fortnite Battle Royale, Chapter 1 (season 4).
    Chapter1,
    /// Fortnite Battle Royale, Chapter 2 (season 1).
    Chapter2,
}

impl MapKind {
    pub const ALL: [MapKind; 3] = [MapKind::Island, MapKind::Chapter1, MapKind::Chapter2];

    pub fn index(self) -> u8 {
        match self {
            MapKind::Island => 0,
            MapKind::Chapter1 => 1,
            MapKind::Chapter2 => 2,
        }
    }

    pub fn from_index(i: u8) -> Option<MapKind> {
        MapKind::ALL.get(i as usize).copied()
    }

    /// The name used in match options and saved settings.
    pub fn key(self) -> &'static str {
        match self {
            MapKind::Island => "island",
            MapKind::Chapter1 => "chapter1",
            MapKind::Chapter2 => "chapter2",
        }
    }

    pub fn from_key(s: &str) -> Option<MapKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "island" | "original" | "procedural" | "0" => Some(MapKind::Island),
            "chapter1" | "ch1" | "c1" | "1" => Some(MapKind::Chapter1),
            "chapter2" | "ch2" | "c2" | "2" => Some(MapKind::Chapter2),
            _ => None,
        }
    }

    /// What the menu calls it.
    pub fn label(self) -> &'static str {
        match self {
            MapKind::Island => "Original island",
            MapKind::Chapter1 => "Chapter 1",
            MapKind::Chapter2 => "Chapter 2",
        }
    }

    /// The table the island is built from (`None` for the procedural island).
    pub fn spec(self) -> Option<&'static MapSpec> {
        match self {
            MapKind::Island => None,
            MapKind::Chapter1 => Some(&super::maps_data::CHAPTER1),
            MapKind::Chapter2 => Some(&super::maps_data::CHAPTER2),
        }
    }
}

/// One cell of the map grid, in metres. The grid covers 90% of the world, so even a coast at the very edge of the map
/// keeps a strip of sea between it and the edge of the world.
pub const GRID_CELL: f32 = WORLD_SIZE / 10.0 * 0.9;

/// A map-grid position (see the module docs) in world metres (x, z).
pub fn grid_to_world(x: f32, y: f32) -> Vec2 {
    Vec2::new((x - 5.0) * GRID_CELL, (y - 5.0) * GRID_CELL)
}

/// A world position (x, z) on the map grid.
pub fn world_to_grid(p: Vec2) -> (f32, f32) {
    (p.x / GRID_CELL + 5.0, p.y / GRID_CELL + 5.0)
}

/// The grid cell a world position lies in, the way the map names it ("D6").
pub fn cell_name(p: Vec2) -> String {
    let (x, y) = world_to_grid(p);
    let col = (x.floor().clamp(0.0, 9.0) as u8 + b'A') as char;
    format!("{}{}", col, y.floor().clamp(0.0, 9.0) as u32 + 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReliefKind {
    /// A rounded rise of the ground.
    Hill,
    /// A steep, rocky peak.
    Mountain,
    /// A flat-topped rise with steep sides.
    Plateau,
    /// A bowl dug into the ground with a raised rim (the meteor crater of Dusty Divot).
    Crater,
}

/// A feature of the relief: `h` metres high (or deep, for a crater), `r` grid cells across its foot.
#[derive(Clone, Copy, Debug)]
pub struct Relief {
    pub kind: ReliefKind,
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub h: f32,
}

/// A lake, `r` cells in radius; `island` is the radius of an island in its middle as a share of the lake's (0 = none).
#[derive(Clone, Copy, Debug)]
pub struct LakeSpec {
    pub name: &'static str,
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub island: f32,
}

/// A river at sea level along `pts` (from its source to its mouth), `width` metres wide.
#[derive(Clone, Copy, Debug)]
pub struct RiverSpec {
    pub name: &'static str,
    pub pts: &'static [(f32, f32)],
    pub width: f32,
}

/// A round region (centre and radius in grid cells).
#[derive(Clone, Copy, Debug)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub r: f32,
}

impl Area {
    /// 1 inside the core of the region, fading to 0 at its edge.
    pub fn weight(&self, p: Vec2) -> f32 {
        let c = grid_to_world(self.x, self.y);
        let r = self.r * GRID_CELL;
        smoothstep(r, r * 0.6, p.distance(c))
    }
}

/// A named place: its kind decides how it is built, `r` is its radius in metres.
#[derive(Clone, Copy, Debug)]
pub struct PoiSpec {
    pub name: &'static str,
    pub kind: PoiKind,
    pub x: f32,
    pub y: f32,
    pub r: f32,
    /// Direction of the main street: true = along x (east-west).
    pub axis_x: bool,
}

/// A road between two named places.
#[derive(Clone, Copy, Debug)]
pub struct RoadSpec {
    pub from: &'static str,
    pub to: &'static str,
    pub paved: bool,
}

/// Everything an island is built from.
#[derive(Debug)]
pub struct MapSpec {
    pub name: &'static str,
    /// The coastline, clockwise from the north, in grid units.
    pub coast: &'static [(f32, f32)],
    /// Typical height of the open land, metres above the sea.
    pub plains: f32,
    /// Ground higher than this is white with snow.
    pub snow_line: f32,
    pub relief: &'static [Relief],
    pub lakes: &'static [LakeSpec],
    pub rivers: &'static [RiverSpec],
    /// Low, waterlogged ground with pools of open water.
    pub swamps: &'static [Area],
    /// Thick woods.
    pub woods: &'static [Area],
    /// Open fields and farmland: hardly a tree.
    pub open: &'static [Area],
    pub pois: &'static [PoiSpec],
    pub roads: &'static [RoadSpec],
}

impl MapSpec {
    /// The coastline in world metres.
    pub fn coast_world(&self) -> Vec<Vec2> {
        self.coast.iter().map(|&(x, y)| grid_to_world(x, y)).collect()
    }

    pub fn poi(&self, name: &str) -> Option<&PoiSpec> {
        self.pois.iter().find(|p| p.name == name)
    }
}

/// Signed distance from `p` to a closed polygon: positive inside, negative outside.
pub fn polygon_signed_distance(poly: &[Vec2], p: Vec2) -> f32 {
    let mut d = f32::MAX;
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        d = d.min(point_segment_dist(p, a, b).0);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    if inside {
        d
    } else {
        -d
    }
}

/// Distance from `p` to a polyline, and how far along it (0 at the first point, 1 at the last) the nearest point is.
pub fn polyline_distance(pts: &[Vec2], p: Vec2) -> (f32, f32) {
    let total: f32 = pts.windows(2).map(|w| w[0].distance(w[1])).sum::<f32>().max(1e-3);
    let mut best = (f32::MAX, 0.0);
    let mut run = 0.0;
    for w in pts.windows(2) {
        let (d, t) = point_segment_dist(p, w[0], w[1]);
        let len = w[0].distance(w[1]);
        if d < best.0 {
            best = (d, (run + t * len) / total);
        }
        run += len;
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::World;

    const RECREATED: [MapKind; 2] = [MapKind::Chapter1, MapKind::Chapter2];

    #[test]
    fn grid_positions_round_trip_and_name_their_cells() {
        let p = grid_to_world(3.5, 5.5);
        let (x, y) = world_to_grid(p);
        assert!((x - 3.5).abs() < 1e-4 && (y - 5.5).abs() < 1e-4);
        assert_eq!(cell_name(p), "D6");
        assert_eq!(cell_name(grid_to_world(0.1, 0.1)), "A1");
        assert_eq!(cell_name(grid_to_world(9.9, 9.9)), "J10");
        // north is -Z, west is -X
        assert!(grid_to_world(5.0, 1.0).y < 0.0 && grid_to_world(1.0, 5.0).x < 0.0);
        for m in MapKind::ALL {
            assert_eq!(MapKind::from_key(m.key()), Some(m));
            assert_eq!(MapKind::from_index(m.index()), Some(m));
        }
        assert_eq!(MapKind::from_index(9), None);
        assert_eq!(MapKind::default(), MapKind::Island, "tests and old saves keep the procedural island");
    }

    #[test]
    fn the_tables_hang_together() {
        for map in RECREATED {
            let spec = map.spec().unwrap();
            assert!(spec.coast.len() >= 24, "{map:?}: a rough coastline");
            assert!(spec.coast.iter().all(|&(x, y)| (0.2..=9.8).contains(&x) && (0.2..=9.8).contains(&y)), "{map:?}: the coast leaves sea round the island");
            let coast = spec.coast_world();
            for p in spec.pois {
                assert!(!p.name.contains(',') && !p.name.contains(';'), "{} (the page splits the list on these)", p.name);
                assert!(spec.pois.iter().filter(|q| q.name == p.name).count() == 1, "{} twice", p.name);
                let inland = polygon_signed_distance(&coast, grid_to_world(p.x, p.y));
                assert!(inland > 25.0, "{map:?}: {} is {inland:.0} m inside the coast", p.name);
            }
            for r in spec.roads {
                assert!(spec.poi(r.from).is_some() && spec.poi(r.to).is_some(), "{map:?}: road {} - {}", r.from, r.to);
            }
            for l in spec.lakes {
                assert!(polygon_signed_distance(&coast, grid_to_world(l.x, l.y)) > l.r * GRID_CELL, "{map:?}: lake {} reaches the sea", l.name);
            }
        }
    }

    #[test]
    fn the_recreated_maps_are_built_where_the_tables_put_them() {
        for map in RECREATED {
            let w = World::generate_map(1234, map);
            let spec = map.spec().unwrap();
            assert_eq!(w.map, map);
            assert_eq!(w.layout.pois.len(), spec.pois.len());
            for ps in spec.pois {
                let poi = w.layout.pois.iter().position(|p| p.name == ps.name).unwrap_or_else(|| panic!("{map:?}: {} missing", ps.name));
                let c = w.layout.pois[poi].center;
                assert!(w.hm.height_at(c.x, c.y) > 0.8, "{map:?}: {} is under water", ps.name);
                assert_eq!(cell_name(c), cell_name(grid_to_world(ps.x, ps.y)));
                let built = w.buildings.iter().filter(|b| b.poi == poi).count();
                let want = match ps.kind {
                    super::super::gen::PoiKind::City | super::super::gen::PoiKind::Town | super::super::gen::PoiKind::Suburb | super::super::gen::PoiKind::Retail => 6,
                    super::super::gen::PoiKind::Station | super::super::gen::PoiKind::Junkyard => 1,
                    _ => 2,
                };
                assert!(built >= want, "{map:?}: {} ({:?}) has {built} buildings", ps.name, ps.kind);
            }
            // the sea all round
            for (x, z) in [(-940.0, -940.0), (940.0, -940.0), (-940.0, 940.0), (940.0, 940.0)] {
                assert!(w.hm.height_at(x, z) < -3.0, "{map:?}: sea at the corner ({x}, {z})");
            }
            // lakes hold water, rivers run at sea level along their course
            for l in &w.layout.lakes {
                let probe = l.center + Vec2::new(l.radius * if l.island > 0.0 { 0.75 } else { 0.0 }, 0.0);
                assert!(w.hm.height_at(probe.x, probe.y) < l.level - 1.0, "{map:?}: a dry lake at {:?}", l.center);
            }
            for r in &w.layout.rivers {
                let total: f32 = r.pts.windows(2).map(|s| s[0].distance(s[1])).sum();
                let (mut wet, mut n) = (0, 0);
                let mut run = 0.0;
                for s in r.pts.windows(2) {
                    let len = s[0].distance(s[1]);
                    let mut t = 0.0;
                    while t < len {
                        let p = s[0].lerp(s[1], t / len);
                        if run + t > total * 0.05 && run + t < total * 0.95 {
                            n += 1;
                            wet += (w.hm.height_at(p.x, p.y) < 0.0) as usize;
                        }
                        t += 12.0;
                    }
                    run += len;
                }
                assert!(wet * 10 >= n * 9, "{map:?}: river water at {wet} of {n} points");
            }
            assert!(w.layout.roads.len() * 10 >= spec.roads.len() * 9, "{map:?}: {} of {} roads", w.layout.roads.len(), spec.roads.len());
            assert!(w.chest_spots.len() > 60 && w.loot_spots.len() > 120, "{map:?}: {} chests, {} loot spots", w.chest_spots.len(), w.loot_spots.len());
            // a bot can walk from the first place to nearly every other one
            let from = w.layout.pois[0].center;
            let reach = w.layout.pois.iter().filter(|p| w.nav.find_path(from, p.center, 200_000).is_some()).count();
            assert!(reach * 10 >= w.layout.pois.len() * 9, "{map:?}: only {reach} of {} places reachable on foot", w.layout.pois.len());
        }
    }

    #[test]
    fn rivers_are_crossed_at_fords_and_lakes_hold_their_water() {
        for map in RECREATED {
            let w = World::generate_map(1234, map);
            // a walk from one bank of a river to the other never goes through deep water
            for r in &w.layout.rivers {
                let total: f32 = r.pts.windows(2).map(|s| s[0].distance(s[1])).sum();
                let (mut run, mut probe) = (0.0, None);
                for s in r.pts.windows(2) {
                    let len = s[0].distance(s[1]);
                    if run + len >= total * 0.5 {
                        let t = (total * 0.5 - run) / len;
                        let dir = (s[1] - s[0]).normalize();
                        probe = Some((s[0].lerp(s[1], t), Vec2::new(-dir.y, dir.x)));
                        break;
                    }
                    run += len;
                }
                let (mid, across) = probe.unwrap();
                let (a, b) = (mid - across * (r.width * 0.5 + 34.0), mid + across * (r.width * 0.5 + 34.0));
                if w.hm.height_at(a.x, a.y) < 1.0 || w.hm.height_at(b.x, b.y) < 1.0 || w.nav.is_blocked_at(a) || w.nav.is_blocked_at(b) {
                    continue;
                }
                let path = w.nav.find_path(a, b, 400_000).unwrap_or_else(|| panic!("{map:?}: no way across the river at {mid:?}"));
                for q in path.windows(2) {
                    for k in 0..8 {
                        let p = q[0].lerp(q[1], k as f32 / 8.0);
                        assert!(w.hm.height_at(p.x, p.y) > -0.6, "{map:?}: the way across the river at {mid:?} swims at {p:?}");
                    }
                }
            }
            // round a lake, inside the circle its water fills, the ground never drops below the water (a channel there
            // would leave the water hanging over it)
            for l in &w.layout.lakes {
                for k in 0..96 {
                    let a = k as f32 / 96.0 * std::f32::consts::TAU;
                    for t in [1.08, 1.15, 1.22, 1.3] {
                        let p = l.center + Vec2::new(a.cos(), a.sin()) * l.radius * t;
                        assert!(w.hm.height_at(p.x, p.y) > l.level - 0.5, "{map:?}: the ground at {p:?} is below the lake at {:?}", l.center);
                    }
                }
            }
            // no loot lies on the bed of a lake
            for s in &w.loot_spots {
                let p = Vec2::new(s.pos.x, s.pos.z);
                assert!(!w.layout.lakes.iter().any(|l| l.center.distance(p) < l.radius && s.pos.y < l.level), "{map:?}: loot under the lake at {p:?}");
            }
        }
    }

    #[test]
    fn places_keep_their_streets_and_doors_open() {
        use super::super::collision::Tag;
        use super::super::gen::PoiKind;
        for map in RECREATED {
            for seed in [1234, 7, 99] {
                let w = World::generate_map(seed, map);
                for (pi, poi) in w.layout.pois.iter().enumerate() {
                    // the streets through a factory yard and a shopping row stay clear of buildings
                    let along = if poi.axis_x { Vec2::X } else { Vec2::Y };
                    let across = Vec2::new(-along.y, along.x);
                    let streets: Vec<Vec2> = match poi.kind {
                        PoiKind::Industrial => vec![along],
                        PoiKind::Retail => vec![along, across],
                        _ => vec![],
                    };
                    for dir in streets {
                        let side = Vec2::new(dir.y.abs(), dir.x.abs());
                        let half = dir.abs() * poi.radius * 0.8 + side * 2.4;
                        let (lo, hi) = (poi.center - half, poi.center + half);
                        for b in w.buildings.iter().filter(|b| b.kind != "house") {
                            let hit = b.aabb.min.x < hi.x && b.aabb.max.x > lo.x && b.aabb.min.z < hi.y && b.aabb.max.z > lo.y;
                            assert!(!hit, "{map:?} seed {seed}: a {} stands on a street of {}", b.kind, poi.name);
                        }
                    }
                    // every factory yard keeps its site office
                    if poi.kind == PoiKind::Industrial {
                        assert!(w.buildings.iter().any(|b| b.poi == pi && b.kind == "shop"), "{map:?} seed {seed}: {} has no office", poi.name);
                    }
                    // nothing is put down in front of a door
                    for b in w.buildings.iter().filter(|b| b.poi == pi && b.door_out.distance(b.door_in) > 0.5) {
                        let d = b.door_out;
                        let probe = Aabb::new(Vec3::new(d.x - 0.6, d.y + 0.4, d.z - 0.6), Vec3::new(d.x + 0.6, d.y + 1.6, d.z + 0.6));
                        let mut blocked = false;
                        w.statics.query(&probe, |_, c| {
                            // (something bulky: a lamp post beside a door is no wall in front of it)
                            let bb = c.shape.aabb();
                            let size = bb.max - bb.min;
                            blocked |= c.tag == Tag::Static && size.x.min(size.z) > 1.0 && size.y > 2.0 && bb.intersects(&probe)
                        });
                        assert!(!blocked, "{map:?} seed {seed}: something stands in front of the door of {} {} in {}", b.kind, b.id, poi.name);
                    }
                }
                // a village in a swamp stands low, among its pools
                for poi in w.layout.pois.iter().filter(|p| p.kind == PoiKind::Swamp) {
                    assert!((2.0..4.0).contains(&poi.ground), "{map:?}: {} stands at {} m", poi.name, poi.ground);
                }
            }
        }
    }

    #[test]
    fn bots_do_not_end_up_stranded_on_the_island_in_loot_lake() {
        use crate::game::{Game, GameConfig, PlayerInput};
        let w0 = World::generate_map(1234, MapKind::Chapter1);
        let lake = w0.layout.lakes.iter().find(|l| l.island > 0.0).unwrap().clone();
        drop(w0);
        let mut stranded = 0;
        for seed in 1..=4u64 {
            let w = World::generate_map(1234, MapKind::Chapter1);
            let mut g = Game::new(w, GameConfig { bots: 39, seed, god_mode: true, map: MapKind::Chapter1, ..Default::default() });
            let inp = PlayerInput::default();
            let mut t = 0.0;
            while t < 150.0 {
                g.update(1.0 / 30.0, &inp);
                t += 1.0 / 30.0;
            }
            stranded += g.actors.iter().skip(1).filter(|a| a.alive && Vec2::new(a.pos.x, a.pos.z).distance(lake.center) < lake.radius * 0.35).count();
        }
        assert_eq!(stranded, 0, "bots still on the island in Loot Lake after 150 s");
    }

    #[test]
    fn bots_landing_on_the_shore_of_loot_lake_jump_off_the_bus_in_time() {
        use crate::game::{actor::MoveMode, Game, GameConfig, PlayerInput};
        let (mut aboard, mut landers) = (0, 0);
        for seed in 1..=24u64 {
            let w = World::generate_map(seed as u32, MapKind::Chapter1);
            let lake = w.layout.lakes.iter().find(|l| l.island > 0.0).unwrap().clone();
            let mut g = Game::new(w, GameConfig { bots: 39, seed, god_mode: true, map: MapKind::Chapter1, ..Default::default() });
            let inp = PlayerInput::default();
            let dt = 1.0 / 30.0;
            let shore = |a: &crate::game::actor::Actor| a.brain.as_ref().and_then(|b| b.landing()).is_some_and(|p| p.distance(lake.center) < lake.radius * 1.5);
            while g.bus.active {
                if g.bus.t + dt >= g.bus.total {
                    aboard += g.actors.iter().skip(1).filter(|a| a.mode == MoveMode::Bus && shore(a)).count();
                    landers += g.actors.iter().skip(1).filter(|a| shore(a)).count();
                }
                g.update(dt, &inp);
            }
        }
        assert!(landers > 20, "only {landers} bots chose Loot Lake");
        assert_eq!(aboard, 0, "{aboard} of {landers} bots heading for Loot Lake were still on the bus at the end of its route");
    }

    #[test]
    fn a_recreated_map_is_the_same_on_every_machine() {
        for map in RECREATED {
            let (a, b) = (World::generate_map(77, map), World::generate_map(77, map));
            assert_eq!(a.buildings.len(), b.buildings.len());
            assert!(a.buildings.iter().zip(&b.buildings).all(|(x, y)| x.door_out == y.door_out && x.kind == y.kind));
            assert!(a.chest_spots.iter().zip(&b.chest_spots).all(|(x, y)| x.pos == y.pos && x.kind == y.kind));
            assert_eq!(a.harvest.len(), b.harvest.len());
            assert!(a.hm.h == b.hm.h);
        }
    }
}
