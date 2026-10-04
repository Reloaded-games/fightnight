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
            if i == PLAYER {
                self.toast(format!("Not enough {}", mat.name().to_lowercase()), 1.5, 3);
            }
            self.actors[i].fire_cd = 0.3;
            return false;
        }
        let plan = self.plan_for(i);
        if !plan.valid {
            self.actors[i].fire_cd = 0.12;
            return false;
        }
        self.pieces.insert(plan.key, mat, plan.base_y, i);
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
        let p = &self.actors[PLAYER];
        self.placement_preview = if p.alive && p.build_mode && p.mode == MoveMode::Ground { Some(self.plan_for(PLAYER)) } else { None };
        self.interact_target = if p.alive && matches!(p.mode, MoveMode::Ground | MoveMode::Swim) { self.find_target(PLAYER) } else { None };
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
        assert!(g.placement_preview.map_or(false, |p| p.valid));
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
