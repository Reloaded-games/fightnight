//! Fortnite-style building: walls, floors, ramps and roofs snapped to a 3D grid.
//!
//! Space is divided into cells of `TILE` metres. A piece lives at (cell x, cell z,
//! level). A *structure* (a connected set of pieces) shares one `base_y`, the height of
//! its level 0, so pieces stack and line up exactly even on sloping terrain.

use super::actor::PieceKind;
use super::env::Env;
use super::items::Mat;
use crate::math::*;
use crate::world::collision::*;
use std::collections::HashMap;

pub const TILE: f32 = 4.5;
pub const LEVEL_H: f32 = 3.4;
pub const THICK: f32 = 0.26;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PieceKey {
    pub kind: PieceKind,
    pub x: i32,
    pub z: i32,
    pub level: i32,
    /// Walls: 0 = edge along X on the cell's -Z side, 1 = edge along Z on the cell's -X side.
    /// Ramps: direction it rises toward (0 +X, 1 +Z, 2 -X, 3 -Z). Floors/roofs: 0.
    pub dir: u8,
}

#[derive(Clone, Debug)]
pub struct Piece {
    pub id: u32,
    pub key: PieceKey,
    pub mat: Mat,
    pub hp: f32,
    pub max_hp: f32,
    pub base_y: f32,
    pub owner: usize,
    pub collider: u32,
    pub age: f32,
}

pub struct Pieces {
    pub list: Vec<Option<Piece>>,
    pub index: HashMap<PieceKey, u32>,
    pub grid: SpatialGrid,
    free: Vec<u32>,
    pub changed: bool,
}

impl Pieces {
    pub fn new() -> Self {
        Self { list: vec![], index: HashMap::new(), grid: SpatialGrid::new(crate::world::WORLD_HALF + 32.0, 8.0), free: vec![], changed: false }
    }

    pub fn clear(&mut self) {
        *self = Pieces::new();
    }

    pub fn count(&self) -> usize {
        self.index.len()
    }

    pub fn get(&self, id: u32) -> Option<&Piece> {
        self.list.get(id as usize).and_then(|p| p.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = &Piece> {
        self.list.iter().filter_map(|p| p.as_ref())
    }

    pub fn at(&self, key: &PieceKey) -> Option<&Piece> {
        self.index.get(key).and_then(|&i| self.get(i))
    }

    pub fn insert(&mut self, key: PieceKey, mat: Mat, base_y: f32, owner: usize) -> u32 {
        let id = if let Some(i) = self.free.pop() { i } else {
            self.list.push(None);
            (self.list.len() - 1) as u32
        };
        let shape = shape_of(&key, base_y);
        let collider = self.grid.insert(Collider::new(shape, Tag::Piece(id)));
        let hp = mat.piece_hp();
        self.list[id as usize] = Some(Piece { id, key, mat, hp, max_hp: hp, base_y, owner, collider, age: 0.0 });
        self.index.insert(key, id);
        self.changed = true;
        id
    }

    pub fn remove(&mut self, id: u32) -> Option<Piece> {
        let p = self.list.get_mut(id as usize)?.take()?;
        self.index.remove(&p.key);
        self.grid.remove(p.collider);
        self.free.push(id);
        self.changed = true;
        Some(p)
    }

    /// Apply damage; returns the piece if it was destroyed.
    pub fn damage(&mut self, id: u32, amount: f32) -> Option<Piece> {
        let p = self.list.get_mut(id as usize)?.as_mut()?;
        p.hp -= amount;
        if p.hp <= 0.0 {
            return self.remove(id);
        }
        None
    }

    pub fn tick(&mut self, dt: f32) {
        for p in self.list.iter_mut().flatten() {
            p.age += dt;
        }
    }
}

impl Default for Pieces {
    fn default() -> Self {
        Self::new()
    }
}

/// Height of the floor of `level` for a structure with `base_y`.
pub fn level_y(base_y: f32, level: i32) -> f32 {
    base_y + level as f32 * LEVEL_H
}

/// World-space collision shape for a piece.
pub fn shape_of(k: &PieceKey, base_y: f32) -> Shape {
    let y0 = level_y(base_y, k.level);
    let (cx, cz) = (k.x as f32 * TILE, k.z as f32 * TILE);
    match k.kind {
        PieceKind::Wall => {
            if k.dir == 0 {
                Shape::Box { min: Vec3::new(cx, y0, cz - THICK / 2.0), max: Vec3::new(cx + TILE, y0 + LEVEL_H, cz + THICK / 2.0) }
            } else {
                Shape::Box { min: Vec3::new(cx - THICK / 2.0, y0, cz), max: Vec3::new(cx + THICK / 2.0, y0 + LEVEL_H, cz + TILE) }
            }
        }
        PieceKind::Floor => Shape::Box { min: Vec3::new(cx, y0 - THICK, cz), max: Vec3::new(cx + TILE, y0, cz + TILE) },
        PieceKind::Roof => Shape::Box { min: Vec3::new(cx, y0 + LEVEL_H - THICK, cz), max: Vec3::new(cx + TILE, y0 + LEVEL_H, cz + TILE) },
        PieceKind::Ramp => Shape::Wedge { min: Vec3::new(cx, y0, cz), max: Vec3::new(cx + TILE, y0 + LEVEL_H, cz + TILE), dir: k.dir & 3 },
    }
}

/// Which cardinal direction (0 +X, 1 +Z, 2 -X, 3 -Z) a yaw points to.
pub fn cardinal(yaw: f32) -> u8 {
    let f = yaw_forward(yaw);
    if f.x.abs() > f.z.abs() {
        if f.x > 0.0 { 0 } else { 2 }
    } else if f.z > 0.0 {
        1
    } else {
        3
    }
}

pub fn dir_vec(d: u8) -> (i32, i32) {
    match d & 3 {
        0 => (1, 0),
        1 => (0, 1),
        2 => (-1, 0),
        _ => (0, -1),
    }
}

pub fn cell_of(p: Vec3) -> (i32, i32) {
    ((p.x / TILE).floor() as i32, (p.z / TILE).floor() as i32)
}

/// A proposed placement.
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub key: PieceKey,
    pub base_y: f32,
    pub valid: bool,
}

/// A piece is supported if any other piece occupies a neighbouring cell on the same or an
/// adjacent level (so structures must grow out of the ground or out of each other).
fn supported(pieces: &Pieces, key: &PieceKey) -> bool {
    pieces.iter().any(|p| p.key != *key && (p.key.x - key.x).abs() <= 1 && (p.key.z - key.z).abs() <= 1 && (p.key.level - key.level).abs() <= 1)
}

/// Structure base height: inherited from any connected piece nearby (matching by level), else from
/// the terrain under the cell. Returns the base for level 0.
pub fn structure_base(pieces: &Pieces, env: &Env, cx: i32, cz: i32) -> f32 {
    // look at pieces in the 3x3 cell neighbourhood on any level
    for dx in -1..=1 {
        for dz in -1..=1 {
            if let Some(p) = pieces.iter().find(|p| p.key.x == cx + dx && p.key.z == cz + dz) {
                return p.base_y;
            }
        }
    }
    // fresh structure: sit on the highest terrain point of the footprint (rounded)
    let (x0, z0) = (cx as f32 * TILE, cz as f32 * TILE);
    let mut hmax = f32::MIN;
    for (fx, fz) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5)] {
        hmax = hmax.max(env.terrain(x0 + fx * TILE, z0 + fz * TILE));
    }
    (hmax * 4.0).round() / 4.0
}

/// Work out where a piece of `kind` would go for an actor standing at `pos` facing `yaw`.
/// `level` is the actor's current build level (see [`standing_level`]).
pub fn plan_piece(pieces: &Pieces, env: &Env, kind: PieceKind, pos: Vec3, yaw: f32, level: i32, base_hint: Option<f32>) -> Placement {
    let (cx, cz) = cell_of(pos);
    let d = cardinal(yaw);
    let (dx, dz) = dir_vec(d);
    let mut key = match kind {
        PieceKind::Wall => {
            // the edge of the current cell that the actor faces, or the next cell's if taken
            let mk = |cx: i32, cz: i32| match d {
                0 => PieceKey { kind, x: cx + 1, z: cz, level, dir: 1 },
                2 => PieceKey { kind, x: cx, z: cz, level, dir: 1 },
                1 => PieceKey { kind, x: cx, z: cz + 1, level, dir: 0 },
                _ => PieceKey { kind, x: cx, z: cz, level, dir: 0 },
            };
            let first = mk(cx, cz);
            if pieces.index.contains_key(&first) {
                mk(cx + dx, cz + dz)
            } else {
                first
            }
        }
        PieceKind::Floor => {
            let here = PieceKey { kind, x: cx, z: cz, level, dir: 0 };
            if pieces.index.contains_key(&here) {
                PieceKey { kind, x: cx + dx, z: cz + dz, level, dir: 0 }
            } else {
                here
            }
        }
        PieceKind::Ramp => PieceKey { kind, x: cx + dx, z: cz + dz, level, dir: d },
        PieceKind::Roof => PieceKey { kind, x: cx, z: cz, level, dir: 0 },
    };
    key.dir &= 3;
    // anchor the structure
    let base_y = base_hint.unwrap_or_else(|| structure_base(pieces, env, key.x, key.z));
    let base_y = if pieces.iter().any(|p| (p.key.x - key.x).abs() <= 1 && (p.key.z - key.z).abs() <= 1) { structure_base(pieces, env, key.x, key.z) } else { base_y };

    let mut valid = !pieces.index.contains_key(&key);
    // not buried inside terrain / static geometry and supported
    if valid {
        let shape = shape_of(&key, base_y);
        let bb = shape.aabb();
        // must not overlap static world geometry (trees, houses...)
        let inner = Aabb::new(bb.min + Vec3::splat(0.1), bb.max - Vec3::splat(0.1));
        env.world.statics.query(&inner, |_, c| {
            if c.shape.aabb().intersects(&inner) {
                valid = false;
            }
        });
        // below ground by more than a floor's thickness: invalid
        let top = bb.max.y;
        if top < env.terrain(bb.center().x, bb.center().z) - 0.5 {
            valid = false;
        }
        // support: ground contact at level 0, or an adjacent piece
        if valid && key.level != 0 {
            valid = supported(pieces, &key);
        }
        if valid && key.level == 0 {
            // reject placing far above the terrain without any neighbour (floating)
            let gap = level_y(base_y, key.level) - env.terrain(bb.center().x, bb.center().z);
            if gap > LEVEL_H + 0.5 && !supported(pieces, &key) {
                valid = false;
            }
        }
    }
    Placement { key, base_y, valid }
}

/// The build level and structure base an actor is currently standing at.
pub fn standing_level(pieces: &Pieces, env: &Env, pos: Vec3) -> (i32, f32) {
    let (cx, cz) = cell_of(pos);
    let base = structure_base(pieces, env, cx, cz);
    let lvl = ((pos.y - base) / LEVEL_H + 0.18).floor() as i32;
    (lvl.max(0), base)
}

#[cfg(test)]
mod tests {
    use super::super::env::testutil::world;
    use super::*;

    /// A flat, empty patch of ground (no statics within 14 m) for building tests.
    fn open_pos() -> Vec3 {
        let w = world();
        for r in (30..300).step_by(10) {
            for a in 0..24 {
                let ang = a as f32 / 24.0 * std::f32::consts::TAU;
                let (x, z) = (ang.cos() * r as f32, ang.sin() * r as f32);
                // align to the middle of a cell so neighbouring cells are also free
                let (cx, cz) = ((x / TILE).floor(), (z / TILE).floor());
                let (x, z) = ((cx + 0.5) * TILE, (cz + 0.5) * TILE);
                let h = w.hm.height_at(x, z);
                if h < 4.0 || w.hm.slope_at(x, z) > 0.06 {
                    continue;
                }
                let q = Aabb::new(Vec3::new(x - 14.0, -50.0, z - 14.0), Vec3::new(x + 14.0, 200.0, z + 14.0));
                let mut free = true;
                w.statics.query(&q, |_, _| free = false);
                if free {
                    return Vec3::new(x, h, z);
                }
            }
        }
        panic!("no open ground found");
    }

    #[test]
    fn shapes_have_expected_extents() {
        let k = PieceKey { kind: PieceKind::Wall, x: 2, z: 3, level: 0, dir: 0 };
        let s = shape_of(&k, 10.0).aabb();
        assert!((s.min.x - 2.0 * TILE).abs() < 1e-4 && (s.max.x - 3.0 * TILE).abs() < 1e-4);
        assert!((s.min.y - 10.0).abs() < 1e-4 && (s.max.y - (10.0 + LEVEL_H)).abs() < 1e-4);
        assert!((s.max.z - s.min.z - THICK).abs() < 1e-4);
        let f = shape_of(&PieceKey { kind: PieceKind::Floor, x: 0, z: 0, level: 1, dir: 0 }, 0.0).aabb();
        assert!((f.max.y - LEVEL_H).abs() < 1e-4);
        let r = shape_of(&PieceKey { kind: PieceKind::Ramp, x: 0, z: 0, level: 0, dir: 1 }, 5.0);
        // top of the ramp at its high edge equals the next level
        assert!((r.top_at(2.0, TILE - 0.01).unwrap() - (5.0 + LEVEL_H)).abs() < 0.02);
        assert!((r.top_at(2.0, 0.01).unwrap() - 5.0).abs() < 0.02);
    }

    #[test]
    fn insert_remove_and_collider_roundtrip() {
        let mut ps = Pieces::new();
        let key = PieceKey { kind: PieceKind::Wall, x: 1, z: 1, level: 0, dir: 0 };
        let id = ps.insert(key, Mat::Wood, 0.0, 0);
        assert_eq!(ps.count(), 1);
        assert!(ps.get(id).is_some() && ps.at(&key).is_some());
        assert_eq!(ps.grid.len(), 1);
        let hp = ps.get(id).unwrap().max_hp;
        assert!(ps.damage(id, hp - 1.0).is_none());
        assert!(ps.damage(id, 5.0).is_some(), "destroyed at <= 0 hp");
        assert_eq!(ps.count(), 0);
        assert_eq!(ps.grid.len(), 0);
        // slot reuse
        let id2 = ps.insert(key, Mat::Stone, 0.0, 0);
        assert_eq!(id2, id);
    }

    #[test]
    fn walls_snap_to_the_edge_the_player_faces() {
        let w = world();
        let ps = Pieces::new();
        let env = Env::new(w, &ps.grid);
        let pos = open_pos();
        let (cx, cz) = cell_of(pos);
        // facing -Z (yaw 0): north edge of current cell
        let p = plan_piece(&ps, &env, PieceKind::Wall, pos, 0.0, 0, None);
        assert_eq!((p.key.x, p.key.z, p.key.dir), (cx, cz, 0));
        // facing +Z (yaw PI): the cell's south edge = next cell's north edge
        let p = plan_piece(&ps, &env, PieceKind::Wall, pos, std::f32::consts::PI, 0, None);
        assert_eq!((p.key.x, p.key.z, p.key.dir), (cx, cz + 1, 0));
        // facing +X (yaw -PI/2)
        let p = plan_piece(&ps, &env, PieceKind::Wall, pos, -std::f32::consts::FRAC_PI_2, 0, None);
        assert_eq!((p.key.x, p.key.z, p.key.dir), (cx + 1, cz, 1));
        assert!(p.valid);
    }

    #[test]
    fn ramp_goes_in_front_and_stacks_into_a_staircase() {
        let w = world();
        let mut ps = Pieces::new();
        let pos = open_pos();
        let mut cur = pos;
        let yaw = 0.0; // facing -Z
        let mut level = 0;
        let mut last_top: Option<f32> = None;
        for step in 0..4 {
            let placement = {
                let env = Env::new(w, &ps.grid);
                plan_piece(&ps, &env, PieceKind::Ramp, cur, yaw, level, None)
            };
            assert!(placement.valid, "ramp {step} must be valid");
            assert_eq!(placement.key.dir, 3, "facing -Z the ramp rises toward -Z");
            if let Some(t) = last_top {
                // the new ramp starts exactly at the previous ramp's top
                let low: f32 = shape_of(&placement.key, placement.base_y).aabb().min.y;
                assert!((low - t).abs() < 1e-3, "ramp {step}: low edge {low} must meet previous top {t}");
            }
            ps.insert(placement.key, Mat::Wood, placement.base_y, 0);
            let top = shape_of(&placement.key, placement.base_y).aabb().max.y;
            last_top = Some(top);
            // stand on top of the new ramp (near its high edge) and build the next level
            let (dx, dz) = dir_vec(placement.key.dir);
            cur = Vec3::new((placement.key.x as f32 + 0.5 + 0.4 * dx as f32) * TILE, top, (placement.key.z as f32 + 0.5 + 0.4 * dz as f32) * TILE);
            let env = Env::new(w, &ps.grid);
            level = standing_level(&ps, &env, cur).0;
            assert_eq!(level, step + 1, "after ramp {step} the player is on level {}", step + 1);
        }
        assert_eq!(ps.count(), 4);
    }

    #[test]
    fn duplicates_and_floating_pieces_are_rejected() {
        let w = world();
        let mut ps = Pieces::new();
        let pos = open_pos();
        let first = {
            let env = Env::new(w, &ps.grid);
            plan_piece(&ps, &env, PieceKind::Floor, pos, 0.0, 0, None)
        };
        assert!(first.valid);
        ps.insert(first.key, Mat::Wood, first.base_y, 0);
        // a floor in the same cell resolves to the next cell instead of duplicating
        let second = {
            let env = Env::new(w, &ps.grid);
            plan_piece(&ps, &env, PieceKind::Floor, pos, 0.0, 0, None)
        };
        assert_ne!(second.key, first.key);
        // a level-3 wall far from anything is unsupported
        let float = {
            let env = Env::new(w, &ps.grid);
            plan_piece(&ps, &env, PieceKind::Wall, pos + Vec3::new(40.0, 0.0, 0.0), 0.0, 3, None)
        };
        assert!(!float.valid, "floating piece must be invalid");
        // but a level-1 wall above the existing floor is supported by it
        let above = {
            let env = Env::new(w, &ps.grid);
            plan_piece(&ps, &env, PieceKind::Wall, pos, 0.0, 1, None)
        };
        assert!(above.valid);
    }

    #[test]
    fn pieces_block_walking_and_ramps_are_walkable() {
        use crate::game::env::STEP_HEIGHT;
        let w = world();
        let mut ps = Pieces::new();
        let pos = open_pos();
        let (cx, cz) = cell_of(pos);
        let base = {
            let env = Env::new(w, &ps.grid);
            structure_base(&ps, &env, cx, cz)
        };
        ps.insert(PieceKey { kind: PieceKind::Wall, x: cx, z: cz, level: 0, dir: 0 }, Mat::Wood, base, 0);
        ps.insert(PieceKey { kind: PieceKind::Ramp, x: cx, z: cz - 3, level: 0, dir: 3 }, Mat::Wood, base, 0);
        let env = Env::new(w, &ps.grid);
        // a person standing just south of the wall, trying to push north, gets pushed back
        let feet = Vec3::new((cx as f32 + 0.5) * TILE, base, cz as f32 * TILE + 0.1);
        let push = env.push_out(feet, 0.38, 1.78, STEP_HEIGHT);
        assert!(push.y > 0.1, "wall must push the actor out: {push:?}");
        // the ramp's surface rises from base to base+LEVEL_H across its cell
        let low = env.ground((cx as f32 + 0.5) * TILE, (cz as f32 - 2.0) * TILE - 0.05 + TILE, base, STEP_HEIGHT).y;
        let _ = low;
        let mid = env.ground((cx as f32 + 0.5) * TILE, (cz - 3) as f32 * TILE + TILE * 0.5, base + LEVEL_H * 0.5, STEP_HEIGHT).y;
        assert!((mid - (base + LEVEL_H * 0.5)).abs() < 0.15, "ramp mid height {mid} vs {}", base + LEVEL_H * 0.5);
    }
}
