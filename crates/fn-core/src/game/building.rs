//! Build-mode actions for any actor: toggling build mode, selecting pieces and
//! materials, placing, and the placement preview for the HUD.

use super::actor::*;
use super::env::Env;
use super::events::*;
use super::intent::Intent;
use super::items::*;
use super::pieces::*;
use super::*;

pub const PLACE_COOLDOWN: f32 = 0.11;

impl Game {
    pub fn handle_building(&mut self, i: usize, it: &Intent) {
        {
            let a = &mut self.actors[i];
            if it.toggle_build {
                a.build_mode = !a.build_mode;
                a.ads = false;
                if a.build_mode {
                    a.action = Action::None;
                }
            }
            if let Some(p) = it.piece {
                a.build_piece = p;
                if !a.build_mode {
                    a.build_mode = true;
                    a.action = Action::None;
                }
            }
            if it.cycle_mat {
                let m = Mat::ALL[(a.build_mat.index() + 1) % 3];
                a.build_mat = m;
            }
        }
        let a = &self.actors[i];
        if a.build_mode && (it.place || it.fire) && a.fire_cd <= 0.0 && a.mode == MoveMode::Ground {
            self.place_piece(i);
        }
    }

    /// Where the actor's next piece would go.
    pub fn plan_for(&self, i: usize) -> Placement {
        let a = &self.actors[i];
        let env = Env::new(&self.world, &self.pieces.grid);
        let (level, _) = standing_level(&self.pieces, &env, a.pos);
        plan_piece(&self.pieces, &env, a.build_piece, a.pos, a.yaw, level, None)
    }

    pub fn place_piece(&mut self, i: usize) -> bool {
        let (mat, kind) = (self.actors[i].build_mat, self.actors[i].build_piece);
        let cost = kind.cost();
        if self.actors[i].inv.mats[mat.index()] < cost {
            if self.actors[i].human {
                self.toast_to(i, format!("Not enough {}", mat.name().to_lowercase()), 1.5, 3);
            }
            self.actors[i].fire_cd = 0.3;
            return false;
        }
        let plan = self.plan_for(i);
        if !plan.valid {
            self.actors[i].fire_cd = 0.12;
            return false;
        }
        let footing = footing_for(&plan.key, plan.base_y, &Env::new(&self.world, &self.pieces.grid));
        self.pieces.insert_footed(plan.key, mat, plan.base_y, i, footing);
        let a = &mut self.actors[i];
        a.inv.mats[mat.index()] -= cost;
        a.fire_cd = PLACE_COOLDOWN;
        a.anim.build = 1.0;
        let pos = shape_of(&plan.key, plan.base_y).aabb().center();
        self.events.push(Event::Built { actor: i, pos, piece: kind, mat });
        self.events.push(Event::Noise { pos, radius: 22.0, source: i });
        true
    }

    /// Refresh HUD-facing previews (called once per frame).
    pub fn update_previews(&mut self) {
        let me = self.local;
        let p = &self.actors[me];
        self.placement_preview = if p.alive && p.build_mode && p.mode == MoveMode::Ground { Some(self.plan_for(me)) } else { None };
        self.interact_target = if p.alive && matches!(p.mode, MoveMode::Ground | MoveMode::Swim) { self.find_target(me) } else { None };
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{free_spot, game};
    use super::*;

    #[test]
    fn building_costs_materials_and_places_a_wall_that_blocks() {
        let mut g = game(1, true);
        let spot = free_spot(&g);
        g.actors[PLAYER].pos = spot;
        g.actors[PLAYER].mode = MoveMode::Ground;
        g.actors[PLAYER].on_ground = true;
        g.actors[PLAYER].yaw = 0.0;
        let wood0 = g.actors[PLAYER].inv.mats[0];
        g.update(1.0 / 60.0, &PlayerInput { piece: Some(PieceKind::Wall), ..Default::default() });
        assert!(g.actors[PLAYER].build_mode);
        assert!(g.placement_preview.is_some_and(|p| p.valid));
        g.update(1.0 / 60.0, &PlayerInput { place: true, ..Default::default() });
        assert_eq!(g.pieces.count(), 1);
        assert_eq!(g.actors[PLAYER].inv.mats[0], wood0 - 10);
        assert!(g.events.iter().any(|e| matches!(e, Event::Built { piece: PieceKind::Wall, .. })));
        // walking into it is blocked: face -Z and run
        g.actors[PLAYER].yaw = 0.0;
        for _ in 0..180 {
            g.update(1.0 / 60.0, &PlayerInput { move_axis: Vec2::new(0.0, 1.0), ..Default::default() });
        }
        let wall = g.pieces.iter().next().unwrap();
        let wall_z = pieces::shape_of(&wall.key, wall.base_y).aabb().center().z;
        assert!(g.actors[PLAYER].pos.z > wall_z - 0.0, "the player cannot walk through the wall (z {} vs wall {wall_z})", g.actors[PLAYER].pos.z);
    }

    #[test]
    fn walls_and_floors_on_a_slope_reach_down_to_the_ground_so_nothing_passes_underneath() {
        // a structure sits on the highest ground under its footprint: on a hillside the downhill end of a wall used to
        // stand a metre or two clear of the terrain, and actors and bullets went straight under it
        let mut g = game(1, true);
        fn lowest_under(g: &Game, key: &PieceKey) -> (f32, Vec2) {
            let (cx, cz) = (key.x as f32 * TILE, key.z as f32 * TILE);
            let mut best = (f32::MAX, Vec2::ZERO);
            for a in 0..=8 {
                for b in 0..=if key.kind == PieceKind::Floor { 8 } else { 0 } {
                    let u = a as f32 / 8.0 * TILE;
                    let (x, z) = match (key.kind, key.dir) {
                        (PieceKind::Wall, 0) => (cx + u, cz),
                        (PieceKind::Wall, _) => (cx, cz + u),
                        _ => (cx + u, cz + b as f32 / 8.0 * TILE),
                    };
                    let h = g.world.hm.height_at(x, z);
                    if h < best.0 {
                        best = (h, Vec2::new(x, z));
                    }
                }
            }
            best
        }
        // hillside cells where a valid wall's base is far above its lowest ground
        let mut cases = vec![];
        'scan: for gz in -85..85 {
            for gx in -85..85 {
                let centre = Vec3::new((gx as f32 + 0.5) * TILE, 0.0, (gz as f32 + 0.5) * TILE);
                for yaw in [0.0, yaw_of(Vec2::new(-1.0, 0.0))] {
                    let pl = plan_piece(&g.pieces, &g.env(), PieceKind::Wall, centre, yaw, 0, None);
                    if !pl.valid {
                        continue;
                    }
                    let (low, at) = lowest_under(&g, &pl.key);
                    if pl.base_y - low > 1.4 && low > 1.0 && cases.len() < 4 && cases.iter().all(|c: &(Vec3, f32, PieceKey, f32, Vec2)| c.0.distance(centre) > 60.0) {
                        cases.push((centre, yaw, pl.key, low, at));
                        if cases.len() == 4 {
                            break 'scan;
                        }
                    }
                }
            }
        }
        assert!(cases.len() >= 3, "the island must have hillsides to test on ({} found)", cases.len());
        for (centre, yaw, key, low, at) in cases {
            let h = g.world.hm.height_at(centre.x, centre.z);
            let a = &mut g.actors[PLAYER];
            a.pos = Vec3::new(centre.x, h, centre.z);
            a.yaw = yaw;
            a.build_piece = PieceKind::Wall;
            a.inv.mats = [999; 3];
            a.fire_cd = 0.0;
            assert!(g.place_piece(PLAYER), "placing at {centre:?}");
            let piece = g.pieces.at(&key).expect("the wall was placed").clone();
            let bb = g.pieces.grid.get(piece.collider).unwrap().shape.aabb();
            assert!(bb.min.y <= low + 0.05, "the wall's body starts {:.2} m above its lowest ground ({key:?})", bb.min.y - low);
            assert!(piece.footing > 1.0, "a footing under the wall: {}", piece.footing);
            // a ray across the wall at knee height above its lowest ground must hit something solid
            // (a little inside the wall's end, where the lowest ground is)
            let (o, d) = if key.dir == 0 {
                (Vec3::new(at.x.clamp(bb.min.x + 0.3, bb.max.x - 0.3), low + 0.4, bb.min.z - 2.0), Vec3::Z)
            } else {
                (Vec3::new(bb.min.x - 2.0, low + 0.4, at.y.clamp(bb.min.z + 0.3, bb.max.z - 0.3)), Vec3::X)
            };
            let hit = g.env().probe(o, d, 4.5);
            assert!(hit.is_some_and(|h| matches!(h.tag, Some(crate::world::collision::Tag::Piece(_)))), "a shot at knee height goes under the wall ({key:?}, ground {low:.2}): {hit:?} from {o:?}, wall {bb:?}");
        }
        // floors reach down too
        let env = g.env();
        let mut floors = 0;
        for gz in -60..60 {
            for gx in -60..60 {
                let key = PieceKey { kind: PieceKind::Floor, x: gx, z: gz, level: 0, dir: 0 };
                let base = structure_base(&g.pieces, &env, gx, gz);
                let foot = footing_for(&key, base, &env);
                let (low, _) = lowest_under(&g, &key);
                let body = shape_of_footed(&key, base, foot).aabb();
                assert!(body.min.y <= low - THICK + 0.05 || body.min.y <= low + 0.05, "floor at ({gx},{gz}): body from {:.2}, lowest ground {low:.2}", body.min.y);
                floors += (foot > 0.5) as i32;
            }
        }
        assert!(floors > 100, "{floors} floors on slopes were checked");
    }

    #[test]
    fn out_of_materials_blocks_building() {
        let mut g = game(1, true);
        let spot = free_spot(&g);
        g.actors[PLAYER].pos = spot;
        g.actors[PLAYER].mode = MoveMode::Ground;
        g.actors[PLAYER].on_ground = true;
        g.actors[PLAYER].inv.mats = [5, 0, 0];
        g.update(1.0 / 60.0, &PlayerInput { piece: Some(PieceKind::Floor), place: true, ..Default::default() });
        g.update(1.0 / 60.0, &PlayerInput { place: true, ..Default::default() });
        assert_eq!(g.pieces.count(), 0);
        assert!(g.toast.iter().any(|t| t.0.contains("Not enough")));
    }

    #[test]
    fn ramp_rushing_climbs_into_the_air() {
        let mut g = game(1, true);
        let spot = free_spot(&g);
        g.actors[PLAYER].pos = spot;
        g.actors[PLAYER].mode = MoveMode::Ground;
        g.actors[PLAYER].on_ground = true;
        g.actors[PLAYER].yaw = 0.0;
        g.actors[PLAYER].inv.mats = [500, 0, 0];
        let y0 = spot.y;
        // enter build mode with ramps selected, then alternate: place a ramp, run forward and jump
        g.update(1.0 / 60.0, &PlayerInput { piece: Some(PieceKind::Ramp), ..Default::default() });
        let mut climbed = 0.0f32;
        for _ in 0..900 {
            // hold forward and the build button: every free tile ahead gets a ramp
            g.update(1.0 / 60.0, &PlayerInput { move_axis: Vec2::new(0.0, 1.0), place: true, ..Default::default() });
            climbed = climbed.max(g.actors[PLAYER].pos.y - y0);
            if climbed > 3.0 * LEVEL_H {
                break;
            }
        }
        assert!(climbed > 2.0 * LEVEL_H, "ramp rush should climb at least two levels: {climbed}");
        assert!(g.pieces.count() >= 3);
    }
}
