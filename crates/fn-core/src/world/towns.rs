//! Town planning: turns each point of interest into streets, lots and buildings.

use super::buildings::*;
use super::collision::*;
use super::gen::*;
use super::heightmap::Heightmap;
use super::props::{PropInst, PropKind};
use super::splat::*;
use crate::math::*;
use crate::mesh::MeshData;
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
        let half = if rot % 2 == 0 { g.half } else { Vec2::new(g.half.y, g.half.x) };
        let (min, max) = (pos - half, pos + half);
        let (lo, hi) = self.height_range(min - Vec2::splat(1.0), max + Vec2::splat(1.0));
        if lo < 2.0 || hi - lo > 1.5 || self.lake_conflict(pos, half.max_element()) {
            return None;
        }
        let (emin, emax) = (min - Vec2::splat(margin), max + Vec2::splat(margin));
        if self.fps.iter().any(|&(a, b)| emin.x < b.x && emax.x > a.x && emin.y < b.y && emax.y > a.y) {
            return None;
        }
        let id = self.out.buildings.len() as u32;
        let placed = place(g, Vec3::new(pos.x, hi, pos.y), rot, Tag::Building(id));
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

fn plan_lakeside(pl: &mut Planner, pi: usize, poi: &Poi) {
    // houses on a ring around the nearest lake, front doors facing the water
    let Some(lake) = pl.layout.lakes.iter().min_by(|a, b| a.center.distance(poi.center).partial_cmp(&b.center.distance(poi.center)).unwrap()) else { return };
    let lake = lake.clone();
    let ring = lake.radius * 1.45 + 17.0;
    let n = 9;
    for k in 0..n {
        let a = k as f32 / n as f32 * std::f32::consts::TAU + 0.2;
        let p = lake.center + Vec2::new(a.cos(), a.sin()) * ring;
        let to_l = (lake.center - p).normalize_or_zero();
        let rot = if to_l.x.abs() > to_l.y.abs() { if to_l.x > 0.0 { 1 } else { 3 } } else if to_l.y > 0.0 { 0 } else { 2 };
        let st = random_style(&mut pl.rng, 1);
        let (w, d) = (pl.rng.range(7.5, 10.0), pl.rng.range(6.5, 8.5));
        let g = gen_house(&mut pl.rng, w, d, &Style { porch: true, ..st });
        pl.place(&g, p, rot, pi, "house", 3.0, true);
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
