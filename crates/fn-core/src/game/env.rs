//! Physics queries against the whole environment: terrain, static colliders and
//! player-built pieces. Everything that moves, shoots, or looks goes through here.

use crate::math::*;
use crate::world::collision::*;
use crate::world::World;

pub const STEP_HEIGHT: f32 = 0.55;

#[derive(Clone, Copy, Debug)]
pub struct Ground {
    pub y: f32,
    /// None = terrain.
    pub tag: Option<Tag>,
}

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub t: f32,
    pub normal: Vec3,
    /// None = terrain.
    pub tag: Option<Tag>,
}

pub struct Env<'a> {
    pub world: &'a World,
    pub pieces: &'a SpatialGrid,
}

impl<'a> Env<'a> {
    pub fn new(world: &'a World, pieces: &'a SpatialGrid) -> Self {
        Self { world, pieces }
    }

    #[inline]
    pub fn terrain(&self, x: f32, z: f32) -> f32 {
        self.world.hm.height_at(x, z)
    }

    fn column(x: f32, z: f32) -> Aabb {
        Aabb::new(Vec3::new(x - 0.04, -200.0, z - 0.04), Vec3::new(x + 0.04, 2000.0, z + 0.04))
    }

    /// Highest surface under (x, z) that is not above `feet_y + step`. Terrain always counts
    /// (the caller deals with slopes that are too steep).
    pub fn ground(&self, x: f32, z: f32, feet_y: f32, step: f32) -> Ground {
        let mut best = Ground { y: self.terrain(x, z), tag: None };
        let col = Self::column(x, z);
        let mut consider = |_: u32, c: &Collider| {
            if let Some(top) = c.shape.top_at(x, z) {
                if top <= feet_y + step && top > best.y {
                    best = Ground { y: top, tag: Some(c.tag) };
                }
            }
        };
        self.world.statics.query(&col, &mut consider);
        self.pieces.query(&col, &mut consider);
        best
    }

    /// Accumulated horizontal push needed to separate a cylinder (feet position, radius, height)
    /// from solid geometry. Terrain steepness is handled by the movement code.
    pub fn push_out(&self, feet: Vec3, radius: f32, height: f32, step: f32) -> Vec2 {
        let c = Vec2::new(feet.x, feet.z);
        let q = Aabb::new(Vec3::new(c.x - radius - 0.05, feet.y - 0.2, c.y - radius - 0.05), Vec3::new(c.x + radius + 0.05, feet.y + height, c.y + radius + 0.05));
        let mut total = Vec2::ZERO;
        let mut f = |_: u32, col: &Collider| {
            if let Some(p) = col.shape.push_out(c + total, radius, feet.y, feet.y + height, step) {
                total += p;
            }
        };
        self.world.statics.query(&q, &mut f);
        self.pieces.query(&q, &mut f);
        total
    }

    /// The lowest underside above `head_y` at (x, z), if any (blocks jumping / rising).
    pub fn ceiling(&self, x: f32, z: f32, head_y: f32) -> Option<f32> {
        let col = Aabb::new(Vec3::new(x - 0.2, head_y - 0.1, z - 0.2), Vec3::new(x + 0.2, head_y + 60.0, z + 0.2));
        let mut best: Option<f32> = None;
        let mut f = |_: u32, c: &Collider| {
            let a = c.shape.aabb();
            if x >= a.min.x - 0.2 && x <= a.max.x + 0.2 && z >= a.min.z - 0.2 && z <= a.max.z + 0.2 {
                let b = c.shape.bottom();
                if b >= head_y - 0.05 && best.map_or(true, |v| b < v) {
                    best = Some(b);
                }
            }
        };
        self.world.statics.query(&col, &mut f);
        self.pieces.query(&col, &mut f);
        best
    }

    /// Water surface height at a position, if it lies under water.
    pub fn water_level(&self, x: f32, z: f32) -> Option<f32> {
        let t = self.terrain(x, z);
        for l in &self.world.layout.lakes {
            if (x - l.center.x).powi(2) + (z - l.center.y).powi(2) < (l.radius * 1.3).powi(2) && t < l.level - 0.05 {
                return Some(l.level);
            }
        }
        if t < 0.0 {
            Some(0.0)
        } else {
            None
        }
    }

    /// Depth of water above the ground at (x, z) (0 on dry land).
    pub fn water_depth(&self, x: f32, z: f32) -> f32 {
        match self.water_level(x, z) {
            Some(l) => (l - self.terrain(x, z)).max(0.0),
            None => 0.0,
        }
    }

    /// Ray against terrain + colliders. `bullets` makes low decorative colliders transparent.
    pub fn raycast(&self, o: Vec3, d: Vec3, max_t: f32, bullets: bool) -> Option<RayHit> {
        let mut best: Option<RayHit> = None;
        if let Some(t) = self.world.hm.raycast(o, d, max_t) {
            let p = o + d * t;
            best = Some(RayHit { t, normal: self.world.hm.normal_at(p.x, p.z), tag: None });
        }
        let limit = best.map_or(max_t, |b| b.t.min(max_t));
        if let Some(h) = self.collider_ray(o, d, limit, bullets) {
            if best.map_or(true, |b| h.t < b.t) {
                best = Some(h);
            }
        }
        best
    }

    /// Ray against static colliders and player-built pieces only (no terrain, nothing is transparent).
    /// Bots feel for walls with it.
    pub fn probe(&self, o: Vec3, d: Vec3, max_t: f32) -> Option<RayHit> {
        self.collider_ray(o, d, max_t, false)
    }

    fn collider_ray(&self, o: Vec3, d: Vec3, max_t: f32, bullets: bool) -> Option<RayHit> {
        let mut best: Option<RayHit> = None;
        for grid in [&self.world.statics, self.pieces] {
            let limit = best.map_or(max_t, |b| b.t.min(max_t));
            let mut local_best: Option<(f32, Vec3, Tag)> = None;
            grid.walk_ray(o, d, limit, |ids, t_exit| {
                for &id in ids {
                    let Some(c) = grid.get(id) else { continue };
                    if bullets && !c.blocks_bullets {
                        continue;
                    }
                    let cur = local_best.map_or(limit, |l| l.0);
                    if let Some((t, n)) = c.shape.raycast(o, d, cur) {
                        if t < cur {
                            local_best = Some((t, n, c.tag));
                        }
                    }
                }
                // keep walking while no hit has been found, or the best hit lies beyond this cell
                local_best.map_or(true, |(t, _, _)| t > t_exit)
            });
            if let Some((t, n, tag)) = local_best {
                if best.map_or(true, |b| t < b.t) {
                    best = Some(RayHit { t, normal: n, tag: Some(tag) });
                }
            }
        }
        best
    }

    /// True if nothing blocks the straight line between two points.
    pub fn line_clear(&self, a: Vec3, b: Vec3) -> bool {
        let d = b - a;
        let l = d.length();
        if l < 0.01 {
            return true;
        }
        self.raycast(a, d / l, l - 0.05, true).is_none()
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use crate::world::World;
    use std::sync::OnceLock;
    pub fn world() -> &'static World {
        static W: OnceLock<World> = OnceLock::new();
        W.get_or_init(|| World::generate(1234))
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::world;
    use super::*;

    fn env() -> (Env<'static>, &'static SpatialGrid) {
        let w = world();
        // an empty pieces grid
        let pieces: &'static SpatialGrid = Box::leak(Box::new(SpatialGrid::new(crate::world::WORLD_HALF + 16.0, 8.0)));
        (Env::new(w, pieces), pieces)
    }

    #[test]
    fn ground_matches_terrain_in_open_and_floor_in_houses() {
        let (e, _) = env();
        let w = world();
        let p = w.layout.pois[0].center + Vec2::new(0.0, 40.0);
        let g = e.ground(p.x, p.y, 100.0, 0.5);
        assert!((g.y - w.hm.height_at(p.x, p.y)).abs() < 0.05 || g.tag.is_some());
        // inside a house the ground is the floor, above the terrain
        let b = w.buildings.iter().find(|b| b.kind == "house").unwrap();
        let inside = Vec2::new(b.door_in.x, b.door_in.z);
        let g = e.ground(inside.x, inside.y, b.door_in.y + 0.1, STEP_HEIGHT);
        assert!(g.y > w.hm.height_at(inside.x, inside.y) + 0.15, "floor must be above terrain: {} vs {}", g.y, w.hm.height_at(inside.x, inside.y));
        assert!((g.y - b.door_in.y).abs() < 0.2, "floor height {} vs door_in {}", g.y, b.door_in.y);
    }

    #[test]
    fn walls_block_but_doors_do_not() {
        let (e, _) = env();
        let w = world();
        let b = w.buildings.iter().find(|b| b.kind == "house").unwrap();
        let dout = b.door_out;
        let din = b.door_in;
        // walking from outside to inside through the door: no horizontal push
        let dir = (din - dout).normalize();
        let steps = 40;
        let mut feet = Vec3::new(dout.x, w.hm.height_at(dout.x, dout.z), dout.z);
        let mut max_push = 0.0f32;
        for k in 0..steps {
            let target = dout + dir * (k as f32 / steps as f32) * (din - dout).length();
            feet.x = target.x;
            feet.z = target.z;
            let g = e.ground(feet.x, feet.z, feet.y, STEP_HEIGHT);
            feet.y = g.y;
            let push = e.push_out(feet, 0.4, 1.8, STEP_HEIGHT);
            max_push = max_push.max(push.length());
        }
        assert!(max_push < 0.05, "door should be passable, max push {max_push}");
        // rays aimed at the wall at knee height (below any window sill) must hit, except through the doorway
        let right = Vec3::new(-dir.z, 0.0, dir.x);
        let mut hits = 0;
        for k in [-2.0f32, -1.5, 1.5, 2.0] {
            let origin = Vec3::new(dout.x, din.y + 0.4, dout.z) + right * k;
            if e.raycast(origin, dir, 12.0, true).is_some() {
                hits += 1;
            }
        }
        assert!(hits >= 3, "rays at the wall beside the door must hit ({hits}/4)");
    }

    #[test]
    fn raycast_hits_terrain_and_trees() {
        let (e, _) = env();
        let w = world();
        let o = Vec3::new(0.0, 80.0, 0.0);
        let h = e.raycast(o, Vec3::NEG_Y, 200.0, true).unwrap();
        assert!(((o.y - h.t) - w.hm.height_at(0.0, 0.0)).abs() < 0.2);
        assert!(h.tag.is_none());
        // a ray aimed at a tree trunk hits a Tree collider
        let t = w.harvest.iter().find(|h| h.kind == crate::world::props::HarvestKind::Tree).unwrap();
        let o = Vec3::new(t.pos.x - 10.0, t.pos.y + 1.0, t.pos.z);
        let d = Vec3::X;
        let hit = e.raycast(o, d, 30.0, true).expect("ray should hit the trunk (or something before it)");
        assert!(hit.t < 11.0);
    }

    #[test]
    fn water_depth_in_sea_and_lake() {
        let (e, _) = env();
        let w = world();
        assert!(e.water_depth(620.0, 0.0) > 5.0);
        assert_eq!(e.water_depth(w.layout.pois[0].center.x, w.layout.pois[0].center.y), 0.0);
        let l = &w.layout.lakes[0];
        assert!(e.water_depth(l.center.x, l.center.y) > 2.0, "lake centre must be deep");
        assert!(e.water_level(l.center.x, l.center.y).unwrap() > 1.0);
    }

    #[test]
    fn ceiling_found_under_roofs() {
        let (e, _) = env();
        let w = world();
        let b = w.buildings.iter().find(|b| b.kind == "house").unwrap();
        let c = e.ceiling(b.door_in.x, b.door_in.z, b.door_in.y + 1.8);
        assert!(c.is_some() && c.unwrap() > b.door_in.y + 2.0, "ceiling above the head inside a house: {c:?}");
        assert!(e.ceiling(b.door_out.x, b.door_out.z, b.door_out.y + 1.8).is_none());
    }
}
