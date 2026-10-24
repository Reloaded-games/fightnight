//! Town planning: turns each point of interest into streets, lots and buildings.

use super::buildings::*;
use super::collision::*;
use super::gen::*;
use super::heightmap::Heightmap;
use super::props::{PropInst, PropKind};
use super::splat::*;
use crate::math::*;
use crate::mesh::{mat, MeshData};
use crate::rng::Rng;
use std::f32::consts::FRAC_PI_2;

pub struct BuildingRec {
    pub id: u32,
    pub poi: usize,
    pub kind: &'static str,
    pub placed: Placed,
}

#[derive(Default)]
pub struct TownOutput {
    pub buildings: Vec<BuildingRec>,
    /// World-space decoration meshes (lamps, fences, docks...).
    pub decor: Vec<(MeshData, Vec3)>,
    pub decor_cols: Vec<Collider>,
    pub props: Vec<PropInst>,
}

struct Planner<'a> {
    hm: &'a Heightmap,
    layout: &'a Layout,
    splat: &'a mut Splat,
    rng: Rng,
    out: TownOutput,
    /// Expanded footprints (min, max) of everything placed so far.
    fps: Vec<(Vec2, Vec2)>,
}

const STREET_HALF: f32 = 3.4;

/// Where a building may stand besides level, dry ground away from lakes.
#[derive(Clone, Copy)]
struct Site {
    /// How far the ground may fall away under the footprint.
    max_drop: f32,
    /// It stands on an island in a lake.
    on_island: bool,
    /// It stands at the lowest ground under it, sunk into the slope (landmarks without a floor to walk on).
    sink: bool,
}

impl Default for Site {
    fn default() -> Self {
        Site { max_drop: 1.5, on_island: false, sink: false }
    }
}

/// Quarter turns that make a building's door (its local +Z) face `dir`.
fn facing_rot(dir: Vec2) -> u8 {
    if dir.x.abs() > dir.y.abs() {
        if dir.x > 0.0 {
            1
        } else {
            3
        }
    } else if dir.y > 0.0 {
        0
    } else {
        2
    }
}

/// A unit vector at angle `a` (radians) in the x/z plane.
fn dir_of(a: f32) -> Vec2 {
    Vec2::new(a.cos(), a.sin())
}

impl<'a> Planner<'a> {
    fn height_range(&self, min: Vec2, max: Vec2) -> (f32, f32) {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for (x, z) in [(min.x, min.y), (max.x, min.y), (max.x, max.y), (min.x, max.y), ((min.x + max.x) / 2.0, (min.y + max.y) / 2.0)] {
            let h = self.hm.height_at(x, z);
            lo = lo.min(h);
            hi = hi.max(h);
        }
        (lo, hi)
    }

    fn lake_conflict(&self, p: Vec2, extent: f32) -> bool {
        self.layout.lakes.iter().any(|l| l.center.distance(p) < l.radius * 1.45 + extent + 2.0)
    }

    /// Try to put a building at `pos` (x, z). Returns its index on success.
    fn place(&mut self, g: &Geom, pos: Vec2, rot: u8, poi: usize, kind: &'static str, margin: f32, lawn: bool) -> Option<usize> {
        self.place_with(g, pos, rot, poi, kind, margin, lawn, Site::default())
    }

    /// [`Planner::place`] for an unusual site (see [`Site`]).
    #[allow(clippy::too_many_arguments)]
    fn place_with(&mut self, g: &Geom, pos: Vec2, rot: u8, poi: usize, kind: &'static str, margin: f32, lawn: bool, site: Site) -> Option<usize> {
        let half = if rot % 2 == 0 { g.half } else { Vec2::new(g.half.y, g.half.x) };
        let (min, max) = (pos - half, pos + half);
        let (lo, hi) = self.height_range(min - Vec2::splat(1.0), max + Vec2::splat(1.0));
        if lo < 2.0 || hi - lo > site.max_drop || (!site.on_island && self.lake_conflict(pos, half.max_element())) {
            return None;
        }
        let (emin, emax) = (min - Vec2::splat(margin), max + Vec2::splat(margin));
        if self.fps.iter().any(|&(a, b)| emin.x < b.x && emax.x > a.x && emin.y < b.y && emax.y > a.y) {
            return None;
        }
        let id = self.out.buildings.len() as u32;
        let at = Vec3::new(pos.x, if site.sink { lo } else { hi }, pos.y);
        let mut placed = place(g, at, rot, Tag::Building(id));
        let hm = self.hm;
        add_entrance_steps(&mut placed, g, at, rot, Tag::Building(id), &|x, z| hm.height_at(x, z));
        self.fps.push((emin, emax));
        if lawn {
            self.splat.paint_rect(min - Vec2::splat(3.0), max + Vec2::splat(3.0), CH_LAWN, 6.0);
        }
        self.out.buildings.push(BuildingRec { id, poi, kind, placed });
        Some(id as usize)
    }

    /// A decoration (mesh only + optional colliders) at a world position; rotation is a quarter-turn count.
    fn decor(&mut self, g: &Geom, pos: Vec3, rot: u8, reserve: Option<f32>) {
        if let Some(r) = reserve {
            let (mn, mx) = (Vec2::new(pos.x - r, pos.z - r), Vec2::new(pos.x + r, pos.z + r));
            self.fps.push((mn, mx));
        }
        let placed = place(g, pos, rot, Tag::Static);
        self.out.decor.push((placed.mesh, pos));
        self.out.decor_cols.extend(placed.cols);
    }

    fn lamp(&mut self, pos: Vec2) {
        let h = self.hm.height_at(pos.x, pos.y);
        let g = gen_lamp_post(0xfff0b8);
        let rot = 0;
        self.decor(&g, Vec3::new(pos.x, h, pos.y), rot, None);
    }

    fn street(&mut self, a: Vec2, b: Vec2, half: f32) {
        self.splat.paint_segment(a, b, half, CH_ASPHALT);
    }

    fn prop(&mut self, kind: PropKind, pos: Vec2, yaw: f32, scale: f32) {
        let h = self.hm.height_at(pos.x, pos.y);
        self.out.props.push(PropInst { kind, pos: Vec3::new(pos.x, h, pos.y), yaw, scale, tint: [1.0, 1.0, 1.0] });
    }

    // ---- lots along a street --------------------------------------------------------
    /// Fill both sides of a street running through `c` along `along` with buildings.
    #[allow(clippy::too_many_arguments)]
    fn street_lots(&mut self, poi_idx: usize, c: Vec2, along_x: bool, from: f32, to: f32, skip: &[(f32, f32)], shop_zone: f32, two_story: f32, kind_hint: Option<&'static str>) {
        for side in [-1.0f32, 1.0] {
            let mut u = from;
            let mut guard = 0;
            while u < to && guard < 40 {
                guard += 1;
                // choose building
                let roll = self.rng.f32();
                let near = (u).abs() < shop_zone;
                let shop = kind_hint != Some("house") && near && roll < 0.38;
                let two = !shop && roll > 1.0 - two_story;
                let (w, d) = if shop {
                    (self.rng.range(9.0, 12.5), self.rng.range(7.5, 9.0))
                } else if two {
                    (self.rng.range(8.5, 11.5), self.rng.range(9.8, 11.5))
                } else {
                    (self.rng.range(7.5, 11.0), self.rng.range(6.5, 9.2))
                };
                let (hw, hd) = (w / 2.0, d / 2.0);
                // skip cross street zones
                if let Some(&(a, b)) = skip.iter().find(|&&(a, b)| u + w + 1.0 > a && u - 1.0 < b) {
                    u = b + 1.5;
                    let _ = a;
                    continue;
                }
                let setback = self.rng.range(2.4, 3.4);
                let off = STREET_HALF + setback + hd;
                let (pos, rot) = if along_x {
                    (Vec2::new(c.x + u + hw, c.y + side * off), if side > 0.0 { 2 } else { 0 })
                } else {
                    (Vec2::new(c.x + side * off, c.y + u + hw), if side > 0.0 { 3 } else { 1 })
                };
                let (geom, name): (Geom, &'static str) = if shop {
                    (gen_shop(&mut self.rng, w, d), "shop")
                } else {
                    let st = random_style(&mut self.rng, if two { 2 } else { 1 });
                    (gen_house(&mut self.rng, w, d, &st), "house")
                };
                let placed = self.place(&geom, pos, rot, poi_idx, name, 2.5, !shop).is_some();
                u += w + if placed { self.rng.range(2.5, 5.0) } else { 3.0 };
            }
        }
    }
}

pub fn build_towns(layout: &Layout, hm: &Heightmap, splat: &mut Splat, seed: u32) -> TownOutput {
    let mut pl = Planner { hm, layout, splat, rng: Rng::new(seed as u64 ^ 0x70E75), out: TownOutput::default(), fps: vec![] };
    for (pi, poi) in layout.pois.iter().enumerate() {
        match poi.kind {
            PoiKind::Town => plan_town(&mut pl, pi, poi),
            PoiKind::Coastal => plan_coastal(&mut pl, pi, poi),
            PoiKind::Farm => plan_farm(&mut pl, pi, poi),
            PoiKind::Lodge => plan_lodge(&mut pl, pi, poi),
            PoiKind::Lakeside => plan_lakeside(&mut pl, pi, poi),
            PoiKind::Hamlet => plan_hamlet(&mut pl, pi, poi),
            PoiKind::Station => plan_station(&mut pl, pi, poi),
            PoiKind::City => plan_city(&mut pl, pi, poi),
            PoiKind::Suburb => plan_suburb(&mut pl, pi, poi),
            PoiKind::Industrial => plan_industrial(&mut pl, pi, poi),
            PoiKind::Mansions => plan_mansions(&mut pl, pi, poi),
            PoiKind::Junkyard => plan_junkyard(&mut pl, pi, poi),
            PoiKind::Crater => plan_crater(&mut pl, pi, poi),
            PoiKind::Swamp => plan_swamp(&mut pl, pi, poi),
            PoiKind::Retail => plan_retail(&mut pl, pi, poi),
        }
    }
    // an island in a lake that no lakeside place is built round (Lazy Lake's village stands up the slope beside it)
    for lake in layout.lakes.iter().filter(|l| l.island > 0.0) {
        let owned = layout.pois.iter().any(|p| p.kind == PoiKind::Lakeside && p.center.distance(lake.center) < lake.radius * 0.5);
        let nearest = layout.pois.iter().enumerate().min_by(|a, b| a.1.center.distance(lake.center).total_cmp(&b.1.center.distance(lake.center)));
        if let (false, Some((pi, _))) = (owned, nearest) {
            island_house(&mut pl, lake, pi);
        }
    }
    pl.out
}

fn plan_town(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let r = poi.radius;
    let ext = r * 0.93;
    // streets
    pl.street(c - Vec2::new(ext, 0.0), c + Vec2::new(ext, 0.0), STREET_HALF);
    pl.street(c - Vec2::new(0.0, ext), c + Vec2::new(0.0, ext), STREET_HALF);
    pl.splat.paint_disc(c, 9.5, CH_ASPHALT, EDGE_RAMP);
    // plaza with fountain
    let h = pl.hm.height_at(c.x, c.y);
    let fountain = gen_fountain();
    pl.decor(&fountain, Vec3::new(c.x, h, c.y), 0, Some(5.0));
    // benches facing the fountain from the four diagonals, and a planter beside each one
    let blooms = [0xff6f91u32, 0xffd23f, 0xfdfbf0, 0xff8a3d];
    for k in 0..4 {
        let a = std::f32::consts::FRAC_PI_4 + k as f32 * FRAC_PI_2;
        let p = c + Vec2::new(a.cos(), a.sin()) * 6.6;
        pl.decor(&gen_bench(a.cos().atan2(a.sin())), Vec3::new(p.x, pl.hm.height_at(p.x, p.y), p.y), 0, None);
        let q = c + Vec2::new((a + 0.42).cos(), (a + 0.42).sin()) * 8.2;
        pl.decor(&gen_planter(blooms[k]), Vec3::new(q.x, pl.hm.height_at(q.x, q.y), q.y), 0, None);
    }
    // lots on the main street (x axis) and cross street (z axis)
    let skip = [(-14.0f32, 14.0f32)];
    pl.street_lots(pi, c, true, -ext + 8.0, ext - 18.0, &skip, 36.0, 0.35, None);
    pl.street_lots(pi, c, false, -ext * 0.85, ext * 0.85 - 12.0, &skip, 30.0, 0.35, None);
    // lamp posts along both streets
    let mut u = -ext + 6.0;
    while u < ext - 4.0 {
        if u.abs() > 9.0 {
            for s in [-1.0f32, 1.0] {
                pl.lamp(Vec2::new(c.x + u, c.y + s * (STREET_HALF + 0.9)));
            }
        }
        u += 26.0;
    }
    let mut u = -ext * 0.85 + 6.0;
    while u < ext * 0.85 {
        if u.abs() > 9.0 {
            for s in [-1.0f32, 1.0] {
                pl.lamp(Vec2::new(c.x + s * (STREET_HALF + 0.9), c.y + u));
            }
        }
        u += 26.0;
    }
    // water tower as a landmark, if there is room
    let wt = gen_water_tower();
    for k in 0..12 {
        let a = k as f32 / 12.0 * std::f32::consts::TAU + 0.4;
        let p = c + Vec2::new(a.cos(), a.sin()) * (r * 0.78);
        if pl.place(&wt, p, 0, pi, "tower", 3.0, false).is_some() {
            break;
        }
    }
}

fn plan_coastal(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let ext = poi.radius * 0.93;
    let along_x = poi.axis_x;
    let (a, b) = if along_x { (c - Vec2::new(ext, 0.0), c + Vec2::new(ext, 0.0)) } else { (c - Vec2::new(0.0, ext), c + Vec2::new(0.0, ext)) };
    pl.street(a, b, STREET_HALF);
    pl.street_lots(pi, c, along_x, -ext + 6.0, ext - 14.0, &[], 20.0, 0.1, None);
    // lamps
    let mut u = -ext + 8.0;
    while u < ext {
        let p = if along_x { Vec2::new(c.x + u, c.y + STREET_HALF + 0.9) } else { Vec2::new(c.x + STREET_HALF + 0.9, c.y + u) };
        pl.lamp(p);
        u += 28.0;
    }
    // lighthouse near the shore: step outward from the village centre until we reach the beach
    let out_dir = c.normalize_or_zero();
    let lh = gen_lighthouse();
    let mut best = None;
    for step in 0..60 {
        let p = c + out_dir * (poi.radius * 0.5 + step as f32 * 3.0);
        let h = pl.hm.height_at(p.x, p.y);
        if h < 2.2 {
            break;
        }
        if h > 2.6 {
            best = Some(p);
        }
    }
    if let Some(p) = best {
        // back off a little from the water
        let p = p - out_dir * 6.0;
        pl.place(&lh, p, 0, pi, "lighthouse", 4.0, false);
    }
    // dock into the sea
    let dock = gen_dock(14.0);
    for off in [-14.0f32, 16.0, -26.0] {
        let tang = Vec2::new(-out_dir.y, out_dir.x);
        let mut p = c + out_dir * (poi.radius * 0.5) + tang * off;
        let mut found = false;
        for _ in 0..70 {
            p += out_dir * 3.0;
            if pl.hm.height_at(p.x, p.y) < 0.6 {
                found = true;
                break;
            }
        }
        if found {
            let back = p - out_dir * 6.0;
            // dock points along +Z in local space; rotate to face `out_dir`
            let rot = if out_dir.x.abs() > out_dir.y.abs() { if out_dir.x > 0.0 { 1 } else { 3 } } else if out_dir.y > 0.0 { 0 } else { 2 };
            pl.decor(&dock, Vec3::new(back.x, 0.55, back.y), rot, Some(8.0));
            break;
        }
    }
}

fn plan_farm(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let barn = gen_barn(&mut pl.rng);
    pl.place(&barn, c + Vec2::new(-14.0, -4.0), 0, pi, "barn", 3.0, true);
    let st = random_style(&mut pl.rng, 1);
    let house = gen_house(&mut pl.rng, 10.0, 8.5, &Style { porch: true, chimney: true, ..st });
    pl.place(&house, c + Vec2::new(14.0, -2.0), 1, pi, "house", 3.0, true);
    pl.place(&gen_silo(), c + Vec2::new(-26.0, -14.0), 0, pi, "silo", 3.0, false);
    // windmill landmark
    let wm = gen_windmill();
    for k in 0..10 {
        let a = k as f32 / 10.0 * std::f32::consts::TAU;
        if let Some(id) = pl.place(&wm, c + Vec2::new(a.cos(), a.sin()) * 36.0, 0, pi, "windmill", 5.0, false) {
            let _ = id;
            break;
        }
    }
    // fields
    for (dx, dz, w, d) in [(-30.0, 22.0, 36.0, 24.0), (14.0, 26.0, 30.0, 20.0)] {
        let min = c + Vec2::new(dx - w / 2.0, dz - d / 2.0);
        let max = c + Vec2::new(dx + w / 2.0, dz + d / 2.0);
        if pl.height_range(min, max).0 > 2.4 {
            pl.splat.paint_rect(min, max, CH_FIELD, EDGE_RAMP);
            pl.fps.push((min, max));
        }
    }
    // hay bales
    for _ in 0..9 {
        let p = c + Vec2::new(pl.rng.range(-38.0, 38.0), pl.rng.range(-30.0, 30.0));
        let ok = !pl.fps.iter().any(|&(a, b)| p.x > a.x - 1.5 && p.x < b.x + 1.5 && p.y > a.y - 1.5 && p.y < b.y + 1.5);
        if ok && pl.hm.height_at(p.x, p.y) > 2.4 {
            let yaw = pl.rng.range(0.0, std::f32::consts::TAU);
            pl.prop(PropKind::HayBale, p, yaw, 1.0);
            let h = pl.hm.height_at(p.x, p.y);
            pl.out.decor_cols.push(Collider::cyl(p.x, p.y, 0.8, h, h + 1.2, Tag::Static));
        }
    }
}

fn plan_lodge(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let big = gen_cabin(&mut pl.rng, 13.0, 9.5);
    pl.place(&big, c, 0, pi, "lodge", 3.0, false);
    for k in 0..7 {
        let a = k as f32 / 7.0 * std::f32::consts::TAU + pl.rng.range(-0.3, 0.3);
        let r = pl.rng.range(26.0, 40.0);
        let p = c + Vec2::new(a.cos(), a.sin()) * r;
        let (cw, cd) = (pl.rng.range(6.5, 8.5), pl.rng.range(6.0, 7.5));
        let cab = gen_cabin(&mut pl.rng, cw, cd);
        // face the centre
        let to_c = (c - p).normalize_or_zero();
        let rot = if to_c.x.abs() > to_c.y.abs() { if to_c.x > 0.0 { 1 } else { 3 } } else if to_c.y > 0.0 { 0 } else { 2 };
        pl.place(&cab, p, rot, pi, "cabin", 3.0, false);
    }
}

/// The house on the island in the middle of a lake (Loot Lake, Lazy Lake).
fn island_house(pl: &mut Planner, lake: &Lake, pi: usize) {
    let st = random_style(&mut pl.rng, 2);
    let g = gen_house(&mut pl.rng, 10.5, 10.0, &Style { porch: true, chimney: true, ..st });
    pl.place_with(&g, lake.center, 0, pi, "house", 1.0, false, Site { on_island: true, ..Site::default() });
}

fn plan_lakeside(pl: &mut Planner, pi: usize, poi: &Poi) {
    // houses on a ring around the nearest lake, front doors facing the water
    let Some(lake) = pl.layout.lakes.iter().min_by(|a, b| a.center.distance(poi.center).partial_cmp(&b.center.distance(poi.center)).unwrap()) else { return };
    let lake = lake.clone();
    let ring = lake.radius * 1.45 + 17.0;
    if lake.island > 0.0 {
        // the house on the island in the middle (Loot Lake)
        island_house(pl, &lake, pi);
    }
    // a lake with an island has a few houses right on its shore, not a whole village round it
    let shore = lake.island > 0.0;
    let n = if shore { 3 } else { 9 };
    let (hub, ring) = if shore { (lake.center, lake.radius * 1.05 + 10.0) } else { (lake.center, ring) };
    for k in 0..n {
        let a = k as f32 / n as f32 * std::f32::consts::TAU + 0.2;
        let p = hub + Vec2::new(a.cos(), a.sin()) * ring;
        let to_l = (hub - p).normalize_or_zero();
        let rot = if to_l.x.abs() > to_l.y.abs() { if to_l.x > 0.0 { 1 } else { 3 } } else if to_l.y > 0.0 { 0 } else { 2 };
        let st = random_style(&mut pl.rng, 1);
        let (w, d) = (pl.rng.range(7.5, 10.0), pl.rng.range(6.5, 8.5));
        let g = gen_house(&mut pl.rng, w, d, &Style { porch: true, ..st });
        pl.place_with(&g, p, rot, pi, "house", 3.0, true, Site { on_island: shore, ..Site::default() });
    }
    // dock reaching into the lake
    let dock = gen_dock(11.0);
    for k in 0..16 {
        let a = k as f32 / 16.0 * std::f32::consts::TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        let shore = lake.center + dir * (lake.radius * 1.05);
        if pl.hm.height_at(shore.x, shore.y) < lake.level - 0.1 {
            continue;
        }
        let rot = if dir.x.abs() > dir.y.abs() { if dir.x > 0.0 { 3 } else { 1 } } else if dir.y > 0.0 { 2 } else { 0 };
        let p = lake.center + dir * (lake.radius * 1.08);
        pl.decor(&dock, Vec3::new(p.x, lake.level + 0.45, p.y), rot, Some(6.0));
        break;
    }
}

fn plan_hamlet(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let h = pl.hm.height_at(c.x, c.y);
    pl.decor(&gen_well(), Vec3::new(c.x, h, c.y), 0, Some(3.5));
    let spots = [Vec2::new(-16.0, -10.0), Vec2::new(16.0, -8.0), Vec2::new(-14.0, 14.0), Vec2::new(17.0, 14.0), Vec2::new(0.0, -24.0)];
    for (k, off) in spots.iter().enumerate() {
        if k >= 4 && pl.rng.chance(0.5) {
            continue;
        }
        let p = c + *off;
        let to_c = (c - p).normalize_or_zero();
        let rot = if to_c.x.abs() > to_c.y.abs() { if to_c.x > 0.0 { 1 } else { 3 } } else if to_c.y > 0.0 { 0 } else { 2 };
        let st = random_style(&mut pl.rng, 1);
        let (w, d) = (pl.rng.range(7.0, 9.5), pl.rng.range(6.0, 8.0));
        let g = gen_house(&mut pl.rng, w, d, &st);
        pl.place(&g, p, rot, pi, "house", 3.0, true);
    }
    pl.splat.paint_disc(c, 5.5, CH_DIRT, EDGE_RAMP);
}

fn plan_station(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let along_x = poi.axis_x;
    let (a, b) = if along_x { (c - Vec2::new(30.0, 0.0), c + Vec2::new(30.0, 0.0)) } else { (c - Vec2::new(0.0, 30.0), c + Vec2::new(0.0, 30.0)) };
    pl.street(a, b, STREET_HALF);
    let canopy = gen_gas_canopy();
    let off = 12.0;
    let (cp, rot) = if along_x { (Vec2::new(c.x, c.y + off), 2) } else { (Vec2::new(c.x + off, c.y), 3) };
    pl.place(&canopy, cp, rot, pi, "station", 2.0, false);
    let shop = gen_shop(&mut pl.rng, 11.0, 8.0);
    let (sp, srot) = if along_x { (Vec2::new(c.x + 17.0, c.y - 11.0), 0) } else { (Vec2::new(c.x - 11.0, c.y + 17.0), 1) };
    pl.place(&shop, sp, srot, pi, "shop", 2.0, false);
    for k in 0..4 {
        let p = c + Vec2::new(pl.rng.range(-20.0, 20.0), pl.rng.range(-20.0, 20.0));
        let kind = if k % 2 == 0 { PropKind::Barrel } else { PropKind::Crate };
        if !pl.fps.iter().any(|&(a, b)| p.x > a.x - 2.0 && p.x < b.x + 2.0 && p.y > a.y - 2.0 && p.y < b.y + 2.0) && pl.splat.sample(p)[CH_ASPHALT] < 0.2 {
            let yaw = pl.rng.range(0.0, std::f32::consts::TAU);
            pl.prop(kind, p, yaw, 1.0);
            let h = pl.hm.height_at(p.x, p.y);
            pl.out.decor_cols.push(Collider::cyl(p.x, p.y, 0.55, h, h + 1.0, Tag::Static));
        }
    }
    let _ = FRAC_PI_2;
}

/// A grid of streets with blocks of tall buildings between them and a clock tower in the middle (Tilted Towers).
fn plan_city(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let ext = poi.radius * 0.93;
    let n = if poi.radius >= 70.0 { 3 } else { 2 };
    let block = ext * 2.0 / n as f32;
    let lines: Vec<f32> = (0..=n).map(|k| -ext + k as f32 * block).collect();
    for &u in &lines {
        pl.street(c + Vec2::new(u, -ext), c + Vec2::new(u, ext), STREET_HALF);
        pl.street(c + Vec2::new(-ext, u), c + Vec2::new(ext, u), STREET_HALF);
    }
    pl.splat.paint_rect(c - Vec2::splat(ext), c + Vec2::splat(ext), CH_ASPHALT, EDGE_RAMP);
    let middle = n / 2;
    for bx in 0..n {
        for bz in 0..n {
            let (x0, x1) = (lines[bx] + STREET_HALF + 1.2, lines[bx + 1] - STREET_HALF - 1.2);
            let (z0, z1) = (lines[bz] + STREET_HALF + 1.2, lines[bz + 1] - STREET_HALF - 1.2);
            let (iw, id) = (x1 - x0, z1 - z0);
            if iw < 9.0 || id < 9.0 {
                continue;
            }
            let bc = c + Vec2::new((x0 + x1) * 0.5, (z0 + z1) * 0.5);
            if bx == middle && bz == middle && n % 2 == 1 {
                // the clock tower: tall and narrow, in a little square
                let st = Style { wall: 0xC9B79C, wall_mat: mat::BRICK, ..random_style(&mut pl.rng, 6) };
                let g = gen_tower(&mut pl.rng, 9.0, 9.0, 6, &st);
                pl.place(&g, bc, 0, pi, "clock tower", 0.6, false);
                continue;
            }
            // two blocks of flats side by side along the longer side of the block, fronts to the streets either side
            let along_x = (bx + bz) % 2 == 0;
            let span = if along_x { iw } else { id };
            let depth = if along_x { id } else { iw };
            let pair = span >= 19.5;
            let count = if pair { 2 } else { 1 };
            for k in 0..count {
                let w = (span / count as f32 - 1.4).clamp(8.5, 13.0) * pl.rng.range(0.88, 1.0);
                let d = (depth - 1.0).clamp(8.5, 13.0) * pl.rng.range(0.88, 1.0);
                let off = if pair { (k as f32 - 0.5) * span * 0.5 } else { 0.0 };
                let p = if along_x { bc + Vec2::new(off, 0.0) } else { bc + Vec2::new(0.0, off) };
                // doors face the street on the short side nearest the edge of the city
                let out = if along_x { Vec2::new(0.0, if bz < middle || (n % 2 == 0 && bz == 0) { -1.0 } else { 1.0 }) } else { Vec2::new(if bx < middle || (n % 2 == 0 && bx == 0) { -1.0 } else { 1.0 }, 0.0) };
                let rot = facing_rot(out);
                // (w runs along the pair, d across it; a quarter turn swaps the generator's width and depth)
                let (ex, ez) = if along_x { (w, d) } else { (d, w) };
                let (gw, gd) = if rot % 2 == 0 { (ex, ez) } else { (ez, ex) };
                let floors = 3 + (pl.rng.f32() * 3.0) as u32;
                let st = random_style(&mut pl.rng, floors);
                let g = gen_tower(&mut pl.rng, gw, gd, floors, &st);
                pl.place(&g, p, rot, pi, "tower block", 0.6, false);
            }
        }
    }
    let mut u = -ext + 8.0;
    while u < ext - 4.0 {
        for &l in &lines[1..n] {
            pl.lamp(c + Vec2::new(u, l + STREET_HALF + 0.9));
            pl.lamp(c + Vec2::new(l + STREET_HALF + 0.9, u));
        }
        u += 24.0;
    }
}

/// Houses round a green with a ring road, and streets out to the four sides (Pleasant Park).
fn plan_suburb(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let r = poi.radius;
    let ring = (r * 0.42).max(16.0);
    pl.splat.paint_disc(c, ring - STREET_HALF - 0.8, CH_LAWN, 4.0);
    let segs = 36;
    for k in 0..segs {
        let (a0, a1) = (k as f32 / segs as f32 * std::f32::consts::TAU, (k + 1) as f32 / segs as f32 * std::f32::consts::TAU);
        pl.street(c + dir_of(a0) * ring, c + dir_of(a1) * ring, STREET_HALF);
    }
    for d in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
        pl.street(c + d * ring, c + d * r * 0.93, STREET_HALF);
    }
    // the gazebo-like fountain in the middle of the green, benches round it
    let h = pl.hm.height_at(c.x, c.y);
    pl.decor(&gen_fountain(), Vec3::new(c.x, h, c.y), 0, Some(5.0));
    for k in 0..4 {
        let a = std::f32::consts::FRAC_PI_4 + k as f32 * FRAC_PI_2;
        let p = c + dir_of(a) * 7.0;
        pl.decor(&gen_bench(a.cos().atan2(a.sin())), Vec3::new(p.x, pl.hm.height_at(p.x, p.y), p.y), 0, None);
    }
    // houses round the outside of the ring, doors to the green; the four streets out stay clear
    let houses_r = ring + STREET_HALF + 8.5;
    let count = ((std::f32::consts::TAU * houses_r) / 15.5) as usize;
    for k in 0..count {
        let a = k as f32 / count as f32 * std::f32::consts::TAU + 0.2;
        let d = dir_of(a);
        if d.x.abs() < 0.2 || d.y.abs() < 0.2 {
            continue;
        }
        let two = pl.rng.chance(0.35);
        let st = random_style(&mut pl.rng, if two { 2 } else { 1 });
        let (w, dd) = if two { (pl.rng.range(8.5, 10.5), pl.rng.range(9.8, 11.0)) } else { (pl.rng.range(7.5, 10.0), pl.rng.range(6.5, 8.5)) };
        let g = gen_house(&mut pl.rng, w, dd, &st);
        pl.place(&g, c + d * houses_r, facing_rot(-d), pi, "house", 2.0, true);
    }
    // more houses along the streets out, in a big enough place
    if r > 62.0 {
        let near = ring + houses_r - ring + 9.0;
        let far = r * 0.9;
        pl.street_lots(pi, c, true, near, far - 9.0, &[], 0.0, 0.3, Some("house"));
        pl.street_lots(pi, c, true, -far, -near - 11.0, &[], 0.0, 0.3, Some("house"));
        pl.street_lots(pi, c, false, near, far - 9.0, &[], 0.0, 0.3, Some("house"));
        pl.street_lots(pi, c, false, -far, -near - 11.0, &[], 0.0, 0.3, Some("house"));
    }
}

/// Warehouses round an asphalt yard, with shipping containers and tall chimneys (Flush Factory, Steamy Stacks, Dirty Docks).
fn plan_industrial(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let r = poi.radius;
    let along = if poi.axis_x { Vec2::X } else { Vec2::Y };
    pl.street(c - along * r * 0.93, c + along * r * 0.93, STREET_HALF);
    pl.splat.paint_disc(c, r * 0.6, CH_ASPHALT, EDGE_RAMP);
    // nothing is built on the street through the yard
    let band = along * r * 0.93 + Vec2::new(along.y, along.x) * (STREET_HALF + 1.0);
    pl.fps.push((c - band, c + band));
    // the halls on a ring round the yard, big doors to the middle, clear of the street
    let a0 = if poi.axis_x { 0.0 } else { std::f32::consts::FRAC_PI_2 };
    let angles: &[f32] = if r > 60.0 { &[45.0, 135.0, 225.0, 315.0] } else { &[60.0, 120.0, 270.0] };
    for &deg in angles {
        let d = dir_of(a0 + deg.to_radians());
        let (w, dd) = (pl.rng.range(17.0, 23.0), pl.rng.range(12.5, 16.0));
        let rot = facing_rot(-d);
        let g = gen_warehouse(&mut pl.rng, w, dd);
        for ring in [0.5, 0.42, 0.6] {
            if pl.place(&g, c + d * r * ring, rot, pi, "warehouse", 2.0, false).is_some() {
                break;
            }
        }
    }
    // the site office
    let office = gen_shop(&mut pl.rng, 10.0, 8.0);
    let mut built = false;
    for k in 0..8 {
        let p = c + dir_of(k as f32 / 8.0 * std::f32::consts::TAU + 1.1) * r * 0.25;
        if pl.place(&office, p, facing_rot(c - p), pi, "shop", 1.5, false).is_some() {
            built = true;
            break;
        }
    }
    if !built {
        // a crowded yard: the free lot nearest its middle
        let n = (r * 0.6 / 3.0) as i32;
        let mut lots: Vec<Vec2> = (-n..=n).flat_map(|i| (-n..=n).map(move |j| Vec2::new(i as f32, j as f32) * 3.0)).filter(|o| o.length() > 1.0 && o.length() < r * 0.6).collect();
        lots.sort_by(|a, b| a.length().total_cmp(&b.length()));
        for o in lots {
            if pl.place(&office, c + o, facing_rot(-o), pi, "shop", 1.5, false).is_some() {
                break;
            }
        }
    }
    // chimneys
    for k in 0..2 {
        let stack = gen_smokestack(pl.rng.range(24.0, 34.0));
        for t in 0..10 {
            let p = c + dir_of(k as f32 * 3.0 + t as f32 * 0.7) * r * pl.rng.range(0.55, 0.8);
            if pl.place_with(&stack, p, 0, pi, "chimney", 2.0, false, Site { max_drop: 4.0, sink: true, ..Site::default() }).is_some() {
                break;
            }
        }
    }
    place_containers(pl, c, r * 0.55, 8);
}

/// `n` shipping containers lying about within `r` of `c`, some stacked two high.
fn place_containers(pl: &mut Planner, c: Vec2, r: f32, n: usize) {
    let paints = [0xb5463a, 0x2f6fb0, 0x3e7e4e, 0xd9a23b, 0x5a5f6a, 0xc96f2a];
    for _ in 0..n {
        let p = c + pl.rng.in_disc(r);
        let rot = if pl.rng.chance(0.5) { 0 } else { 1 };
        let half = if rot == 0 { Vec2::new(3.3, 1.5) } else { Vec2::new(1.5, 3.3) };
        let (lo, hi) = pl.height_range(p - half, p + half);
        let overlaps = |a: Vec2, b: Vec2| p.x + half.x > a.x && p.x - half.x < b.x && p.y + half.y > a.y && p.y - half.y < b.y;
        // (and never in front of a door: the way from outside to inside stays open)
        let at_door = pl.out.buildings.iter().any(|b| {
            let (i, o) = (Vec2::new(b.placed.door_in.x, b.placed.door_in.z), Vec2::new(b.placed.door_out.x, b.placed.door_out.z));
            overlaps(i.min(o) - Vec2::splat(3.5), i.max(o) + Vec2::splat(3.5))
        });
        if lo < 2.0 || hi - lo > 0.8 || at_door || pl.fps.iter().any(|&(a, b)| overlaps(a, b)) {
            continue;
        }
        let g = gen_container(*pl.rng.pick(&paints));
        pl.decor(&g, Vec3::new(p.x, hi, p.y), rot, None);
        pl.fps.push((p - half, p + half));
        if pl.rng.chance(0.3) {
            let top = gen_container(*pl.rng.pick(&paints));
            pl.decor(&top, Vec3::new(p.x, hi + 2.6, p.y), rot, None);
        }
    }
}

/// A few big two-storey houses on wide lots along one road (Snobby Shores).
fn plan_mansions(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let ext = poi.radius * 0.93;
    let along = if poi.axis_x { Vec2::X } else { Vec2::Y };
    let across = Vec2::new(-along.y, along.x);
    pl.street(c - along * ext, c + along * ext, STREET_HALF);
    let mut u = -ext + 12.0;
    let mut k = 0;
    while u < ext - 10.0 {
        let side = if k % 2 == 0 { 1.0 } else { -1.0 };
        k += 1;
        let (w, d) = (pl.rng.range(12.0, 14.5), pl.rng.range(10.4, 12.4));
        let st = random_style(&mut pl.rng, 2);
        let g = gen_house(&mut pl.rng, w, d, &Style { porch: true, chimney: true, ..st });
        let p = c + along * u + across * side * (STREET_HALF + pl.rng.range(6.0, 9.0) + d * 0.5);
        if pl.place(&g, p, facing_rot(-across * side), pi, "house", 4.0, true).is_some() {
            // a hedge-like fence along the front of the lot
            let fence = gen_fence(w + 4.0, 0xf2efe6);
            let f = c + along * (u - (w + 4.0) * 0.5) + across * side * (STREET_HALF + 2.2);
            // (the fence runs along its +X; a quarter turn the other way round makes that +Z)
            let rot = if poi.axis_x { 0 } else { 3 };
            pl.decor(&fence, Vec3::new(f.x, pl.hm.height_at(f.x, f.y), f.y), rot, None);
        }
        u += pl.rng.range(22.0, 30.0) * 0.5 + 6.0;
    }
}

/// Rows of crushed cars in a dirt yard, with loot on top of the piles and an office (Junk Junction).
fn plan_junkyard(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let r = poi.radius;
    pl.splat.paint_disc(c, r * 0.8, CH_DIRT, EDGE_RAMP);
    let office = gen_shop(&mut pl.rng, 10.5, 8.0);
    pl.place(&office, c + Vec2::new(0.0, -r * 0.55), 0, pi, "shop", 2.0, false);
    let step = Vec2::new(10.0, 8.5);
    let n = (r * 0.7 / step.x) as i32;
    for i in -n..=n {
        for j in -n..=n {
            if i == 0 && j.abs() <= 1 {
                // an aisle down the middle
                continue;
            }
            let p = c + Vec2::new(i as f32 * step.x, j as f32 * step.y) + pl.rng.in_disc(1.2);
            if p.distance(c) > r * 0.75 {
                continue;
            }
            let layers = 2 + (pl.rng.f32() * 4.0) as u32;
            let mut g = gen_wreck_stack(&mut pl.rng, layers);
            // something worth climbing for on top of every pile, now and then a chest
            g.loot.push(Vec3::new(0.0, g.height, 0.0));
            if pl.rng.chance(0.18) {
                g.chests.push((Vec3::new(0.0, g.height, 0.0), 0.0));
                g.loot.clear();
            }
            let rot = (pl.rng.f32() * 4.0) as u8;
            pl.place(&g, p, rot, pi, "wrecks", 1.4, false);
        }
    }
    place_containers(pl, c, r * 0.7, 4);
}

/// The research site on the floor of a meteor crater: two labs and containers (Dusty Divot).
fn plan_crater(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let r = poi.radius;
    pl.splat.paint_disc(c, r * 0.55, CH_DIRT, EDGE_RAMP);
    for (k, off) in [Vec2::new(-12.0, -4.0), Vec2::new(12.0, 5.0)].into_iter().enumerate() {
        let g = gen_warehouse(&mut pl.rng, 16.5, 12.5);
        let rot = if k == 0 { 1 } else { 3 };
        pl.place(&g, c + off, rot, pi, "warehouse", 1.5, false);
    }
    place_containers(pl, c, r * 0.5, 6);
}

/// Cabins on the dry ground in the middle of a swamp, boardwalks out over the water (Moisty Mire).
fn plan_swamp(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let r = poi.radius;
    let h = pl.hm.height_at(c.x, c.y);
    pl.decor(&gen_well(), Vec3::new(c.x, h, c.y), 0, Some(3.5));
    pl.splat.paint_disc(c, 6.0, CH_DIRT, EDGE_RAMP);
    let mut placed = 0;
    for _ in 0..40 {
        if placed >= 8 {
            break;
        }
        let p = c + pl.rng.in_disc(r * 0.62);
        let (w, d) = (pl.rng.range(6.5, 9.0), pl.rng.range(6.0, 7.8));
        let g = gen_cabin(&mut pl.rng, w, d);
        if pl.place(&g, p, facing_rot(c - p), pi, "cabin", 3.0, false).is_some() {
            placed += 1;
        }
    }
    // boardwalks from the edge of the dry ground out over the water
    let walk = gen_dock(12.0);
    for k in 0..6 {
        let d = dir_of(k as f32 / 6.0 * std::f32::consts::TAU + 0.4);
        let mut p = c + d * (r * 0.4);
        for _ in 0..40 {
            p += d * 2.0;
            if pl.hm.height_at(p.x, p.y) < 0.4 {
                let back = p - d * 5.0;
                let rot = if d.x.abs() > d.y.abs() { if d.x > 0.0 { 1 } else { 3 } } else if d.y > 0.0 { 0 } else { 2 };
                pl.decor(&walk, Vec3::new(back.x, 0.55, back.y), rot, Some(4.0));
                break;
            }
        }
    }
}

/// A row of shops behind a car park on one side of the main street, houses along the rest (Retail Row).
fn plan_retail(pl: &mut Planner, pi: usize, poi: &Poi) {
    let c = poi.center;
    let r = poi.radius;
    let ext = r * 0.93;
    let along = if poi.axis_x { Vec2::X } else { Vec2::Y };
    let across = Vec2::new(-along.y, along.x);
    pl.street(c - along * ext, c + along * ext, STREET_HALF);
    pl.street(c - across * ext, c + across * ext, STREET_HALF);
    // the car park, then the shops shoulder to shoulder behind it, fronts to the street
    let park = 13.0;
    let span = (ext * 0.62).min(48.0);
    let lot_min = c - along * span + across * (STREET_HALF + 0.5);
    let lot_max = c + along * span + across * (STREET_HALF + park);
    let (pmin, pmax) = (lot_min.min(lot_max), lot_min.max(lot_max));
    pl.splat.paint_rect(pmin, pmax, CH_ASPHALT, EDGE_RAMP);
    pl.fps.push((pmin, pmax));
    let mut u = -span;
    while u < span - 8.0 {
        let (w, d) = (pl.rng.range(9.5, 12.5), pl.rng.range(7.6, 9.0));
        if u + w > span + 2.0 {
            break;
        }
        if u + w > -(STREET_HALF + 1.0) && u < STREET_HALF + 1.0 {
            // a gap in the row where the cross street goes through
            u = STREET_HALF + 1.0;
            continue;
        }
        let g = gen_shop(&mut pl.rng, w, d);
        let p = c + along * (u + w * 0.5) + across * (STREET_HALF + park + 1.2 + d * 0.5);
        pl.place(&g, p, facing_rot(-across), pi, "shop", 0.3, false);
        u += w + 0.8;
    }
    for k in 0..4 {
        let p = c + along * (-span + 6.0 + k as f32 * (span * 2.0 - 12.0) / 3.0) + across * (STREET_HALF + park * 0.5);
        pl.lamp(p);
    }
    // houses along the far side of the main street and both sides of the cross street
    pl.street_lots(pi, c, poi.axis_x, -ext + 6.0, ext - 14.0, &[(-10.0, 10.0)], 0.0, 0.3, Some("house"));
    pl.street_lots(pi, c, !poi.axis_x, -ext + 6.0, ext - 14.0, &[(-10.0, 10.0)], 0.0, 0.3, Some("house"));
    // the water tower over the town
    let wt = gen_water_tower();
    for k in 0..12 {
        let p = c + dir_of(k as f32 / 12.0 * std::f32::consts::TAU + 2.2) * (r * 0.75);
        if pl.place(&wt, p, 0, pi, "tower", 3.0, false).is_some() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn towns_are_populated_and_non_overlapping() {
        for seed in [1234u32, 7, 99] {
            let base = BaseTerrain::new(seed);
            let layout = plan_layout(&base);
            let hm = build_heightmap(&base, &layout);
            let mut splat = Splat::new();
            let out = build_towns(&layout, &hm, &mut splat, seed);
            let n = out.buildings.len();
            assert!(n >= 25, "seed {seed}: only {n} buildings");
            // every building has a door and some loot (landmarks excepted)
            let with_loot = out.buildings.iter().filter(|b| !b.placed.loot.is_empty()).count();
            assert!(with_loot >= n / 2, "seed {seed}: too few buildings with loot ({with_loot}/{n})");
            // footprints do not overlap
            for (i, a) in out.buildings.iter().enumerate() {
                for b in &out.buildings[i + 1..] {
                    let (pa, pb) = (&a.placed.aabb, &b.placed.aabb);
                    let overlap = pa.min.x < pb.max.x - 0.5 && pa.max.x > pb.min.x + 0.5 && pa.min.z < pb.max.z - 0.5 && pa.max.z > pb.min.z + 0.5;
                    assert!(!overlap, "seed {seed}: {} and {} overlap", a.kind, b.kind);
                }
            }
            // buildings sit above sea level
            for b in &out.buildings {
                assert!(hm.height_at(b.placed.door_out.x, b.placed.door_out.z) > 1.0, "building door in the water");
            }
            let chests: usize = out.buildings.iter().map(|b| b.placed.chests.len()).sum();
            assert!(chests >= 6, "seed {seed}: only {chests} chests");
        }
    }
}
