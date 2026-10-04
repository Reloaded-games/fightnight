//! Match flow: the battle bus, the shrinking storm, and the win condition.

use super::actor::*;
use super::events::*;
use super::*;
use crate::math::*;
use crate::rng::Rng;
use crate::world::{WORLD_HALF, WORLD_SIZE};

const MAP_SCALE: f32 = WORLD_SIZE / 1280.0;

/// Initial storm radius: comfortably covers the whole island.
pub const R0: f32 = WORLD_HALF + 90.0;
pub const BUS_ALTITUDE: f32 = 285.0;
pub const BUS_SPEED: f32 = 38.0 * MAP_SCALE;

/// (wait seconds, shrink seconds, end radius, damage per second)
pub const PHASES: [(f32, f32, f32, f32); 7] = [
    (90.0, 75.0, 340.0 * MAP_SCALE, 1.0),
    (55.0, 60.0, 210.0 * MAP_SCALE, 2.0),
    (40.0, 45.0, 125.0 * MAP_SCALE, 3.0),
    (30.0, 40.0, 70.0 * MAP_SCALE, 5.0),
    (25.0, 35.0, 32.0, 7.0),
    (20.0, 30.0, 10.0, 10.0),
    (15.0, 25.0, 0.0, 10.0),
];

pub fn initial_storm() -> Storm {
    Storm {
        active: false,
        phase: 0,
        state: StormState::Waiting,
        timer: PHASES[0].0,
        duration: PHASES[0].0,
        center: Vec2::ZERO,
        radius: R0,
        from_center: Vec2::ZERO,
        from_radius: R0,
        to_center: Vec2::ZERO,
        to_radius: PHASES[0].2,
        dmg: 0.0,
        tick: 0.0,
    }
}

pub fn initial_bus(rng: &mut Rng) -> Bus {
    let ang = rng.range(0.0, std::f32::consts::TAU);
    let dir = Vec2::new(ang.cos(), ang.sin());
    let through = rng.in_disc(110.0 * MAP_SCALE);
    let half = 790.0 * MAP_SCALE;
    let start = through - dir * half;
    let end = through + dir * half;
    let total = half * 2.0 / BUS_SPEED;
    Bus { active: true, pos: Vec3::new(start.x, BUS_ALTITUDE, start.y), dir, speed: BUS_SPEED, start, end, t: 0.0, total }
}

/// Start the waiting period of the current storm phase and pick the next safe circle.
pub fn begin_storm_wait(g: &mut Game) {
    let p = g.storm.phase.min(PHASES.len() - 1);
    let (wait, shrink, end_r, dmg) = PHASES[p];
    let speed = g.cfg.storm_speed.max(0.01);
    g.storm.state = StormState::Waiting;
    g.storm.timer = wait / speed;
    g.storm.duration = wait / speed;
    g.storm.from_center = g.storm.center;
    g.storm.from_radius = g.storm.radius;
    g.storm.to_radius = end_r;
    g.storm.dmg = dmg;
    let _ = shrink;
    let max_off = (g.storm.radius - end_r).max(0.0) * 0.8;
    let mut best = g.storm.center;
    for _ in 0..30 {
        let c = g.storm.center + g.rng.in_disc(max_off);
        // prefer circles centred over land, ideally not right at the shore
        if g.world.hm.height_at(c.x, c.y) > 3.0 {
            best = c;
            break;
        }
    }
    g.storm.to_center = best;
    g.events.push(Event::StormPhase { phase: p, shrinking: false });
}

pub fn update_storm(g: &mut Game, dt: f32) {
    if !g.storm.active {
        return;
    }
    let speed = g.cfg.storm_speed.max(0.01);
    match g.storm.state {
        StormState::Waiting => {
            g.storm.timer -= dt;
            if g.storm.timer <= 0.0 {
                let p = g.storm.phase.min(PHASES.len() - 1);
                g.storm.state = StormState::Shrinking;
                g.storm.duration = PHASES[p].1 / speed;
                g.storm.timer = g.storm.duration;
                g.events.push(Event::StormPhase { phase: p, shrinking: true });
                g.toast("The storm is closing in", 4.0, 2);
            }
        }
        StormState::Shrinking => {
            g.storm.timer -= dt;
            let t = (1.0 - g.storm.timer / g.storm.duration).clamp(0.0, 1.0);
            g.storm.center = g.storm.from_center.lerp(g.storm.to_center, t);
            g.storm.radius = lerp(g.storm.from_radius, g.storm.to_radius, t);
            if g.storm.timer <= 0.0 {
                g.storm.center = g.storm.to_center;
                g.storm.radius = g.storm.to_radius;
                g.storm.phase += 1;
                if g.storm.phase >= PHASES.len() {
                    g.storm.state = StormState::Done;
                } else {
                    begin_storm_wait(g);
                    g.toast("Storm forming - get to the safe zone", 4.0, 1);
                }
            }
        }
        StormState::Done => {}
    }
    // damage anyone caught outside, once per second
    g.storm.tick += dt;
    if g.storm.tick >= 1.0 {
        g.storm.tick -= 1.0;
        let dmg = g.storm.dmg;
        let (c, r) = (g.storm.center, g.storm.radius);
        let n = g.actors.len();
        for i in 0..n {
            let a = &g.actors[i];
            if !a.alive || a.mode == MoveMode::Bus {
                continue;
            }
            if Vec2::new(a.pos.x, a.pos.z).distance(c) > r {
                if g.cfg.god_mode && i == g.local {
                    continue;
                }
                let pos = a.pos + Vec3::Y;
                g.damage_actor(i, dmg, None, "The Storm", false, pos, true);
            }
        }
    }
}

pub fn in_storm(g: &Game, pos: Vec3) -> bool {
    g.storm.active && Vec2::new(pos.x, pos.z).distance(g.storm.center) > g.storm.radius
}

pub fn update_bus(g: &mut Game, dt: f32) {
    if !g.bus.active {
        return;
    }
    g.bus.t += dt;
    let t = (g.bus.t / g.bus.total).clamp(0.0, 1.0);
    let p = g.bus.start.lerp(g.bus.end, t);
    g.bus.pos = Vec3::new(p.x, BUS_ALTITUDE, p.y);
    if g.bus.t >= g.bus.total {
        g.bus.active = false;
        g.phase = Phase::Playing;
        let n = g.actors.len();
        for i in 0..n {
            if g.actors[i].mode == MoveMode::Bus {
                leave_bus(g, i);
            }
        }
        g.storm.active = true;
        g.storm.phase = 0;
        begin_storm_wait(g);
    } else if g.bus.t > 4.0 && g.phase == Phase::Bus {
        // the match counts as started once the bus is under way
        g.phase = Phase::Bus;
    }
}

/// Jump out of the bus into free fall.
pub fn leave_bus(g: &mut Game, i: usize) {
    let dir = g.bus.dir;
    let pos = g.bus.pos - Vec3::Y * 1.5;
    let a = &mut g.actors[i];
    if a.mode != MoveMode::Bus {
        return;
    }
    a.mode = MoveMode::Freefall;
    a.on_ground = false;
    a.pos = pos;
    a.vel = Vec3::new(dir.x * 6.0, -6.0, dir.y * 6.0);
    a.pitch = a.pitch.min(-0.5);
    a.peak_y = pos.y;
    g.events.push(Event::BusJump { actor: i, pos });
}

/// Declare a winner when one actor is left.
pub fn check_victory(g: &mut Game) {
    if g.phase == Phase::Over {
        return;
    }
    let alive: Vec<usize> = g.actors.iter().filter(|a| a.alive).map(|a| a.id).collect();
    // Actors that fall in the same step have no order between them (who is processed first is an accident of the
    // actor list), so they share the best place the group could have taken instead of ranking by index.
    let fallen: Vec<usize> = g.actors.iter().filter(|a| !a.alive && a.death_step == g.step_no).map(|a| a.id).collect();
    if fallen.len() > 1 {
        for &i in &fallen {
            g.actors[i].placement = alive.len() as u32 + 1;
        }
    }
    if g.phase == Phase::Playing && alive.len() <= 1 {
        g.phase = Phase::Over;
        if let Some(&w) = alive.first() {
            g.winner = Some(w);
            // the match is decided: whatever the winner was doing (reloading, healing, building) stops for the dance
            let a = &mut g.actors[w];
            a.placement = 1;
            a.action = Action::None;
            a.build_mode = false;
            a.ads = false;
            a.emoting = true;
            a.anim.emote_clock = 0.0;
            g.events.push(Event::Victory { winner: w });
        } else {
            // the last ones standing fell together: a draw, sharing first place
            g.tie = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{game, game_cfg};
    use super::*;

    #[test]
    fn phases_are_monotonic_and_end_small() {
        let mut prev = R0;
        let mut prev_dmg = 0.0;
        for (wait, shrink, r, dmg) in PHASES {
            assert!(wait > 0.0 && shrink > 0.0);
            assert!(r < prev, "radii must shrink: {r} !< {prev}");
            assert!(dmg >= prev_dmg);
            prev = r;
            prev_dmg = dmg;
        }
        assert_eq!(PHASES.last().unwrap().2, 0.0);
        // The larger island allows extra travel time in the first two phases.
        let total: f32 = PHASES.iter().map(|p| p.0 + p.1).sum();
        assert!((420.0..620.0).contains(&total), "total storm time {total}");
    }

    #[test]
    fn bus_crosses_the_island_and_everyone_leaves() {
        let mut g = game(5, false);
        assert_eq!(g.phase, Phase::Bus);
        assert!(g.actors.iter().all(|a| a.mode == MoveMode::Bus));
        let inp = PlayerInput::default();
        let mut t = 0.0;
        while g.phase == Phase::Bus && t < 100.0 {
            g.update(0.05, &inp);
            t += 0.05;
        }
        assert_eq!(g.phase, Phase::Playing, "bus must finish by itself");
        assert!(t > 30.0 && t < 60.0, "bus trip took {t}s");
        assert!(g.actors.iter().all(|a| a.mode != MoveMode::Bus));
        assert!(g.storm.active);
    }

    #[test]
    fn bus_and_initial_storm_cover_the_expanded_island() {
        for seed in [1, 7, 42, 1234, 2024] {
            let bus = initial_bus(&mut Rng::new(seed));
            assert!(bus.start.length() > WORLD_HALF && bus.end.length() > WORLD_HALF);
            let (distance, _) = point_segment_dist(Vec2::ZERO, bus.start, bus.end);
            assert!(distance < 170.0, "bus should pass near the centre of the island");
            assert!((40.0..45.0).contains(&bus.total), "larger map should retain a short bus journey");
        }
        const { assert!(R0 > WORLD_HALF) };
        assert!(PHASES[0].2 > 450.0, "first circle should suit the expanded land area");
    }

    #[test]
    fn storm_shrinks_into_a_valid_circle_and_damages_outsiders() {
        let mut g = game_cfg(GameConfig { bots: 3, skip_bus: true, seed: 7, storm_speed: 20.0, ..Default::default() });
        let inp = PlayerInput::default();
        assert!(g.storm.active && g.storm.state == StormState::Waiting);
        let first_target = g.storm.to_center;
        assert!(g.storm.to_radius < R0);
        // the target circle lies inside the current one
        assert!(first_target.distance(g.storm.center) + g.storm.to_radius <= g.storm.radius + 1.0);
        // move the player far outside the final circle and run the storm
        let edge = WORLD_HALF - 40.0;
        let out = Vec3::new(edge, g.world.hm.height_at(edge, 0.0), 0.0);
        g.actors[PLAYER].pos = out;
        g.actors[PLAYER].mode = MoveMode::Ground;
        let hp0 = g.actors[PLAYER].hp;
        for _ in 0..600 {
            g.update(0.1, &inp);
            if g.storm.state == StormState::Shrinking {
                break;
            }
        }
        assert_eq!(g.storm.state, StormState::Shrinking);
        // radius is decreasing over time
        let r0 = g.storm.radius;
        for _ in 0..20 {
            g.update(0.1, &inp);
        }
        assert!(g.storm.radius < r0, "storm must shrink: {} !< {r0}", g.storm.radius);
        // run until the player has been outside long enough to take damage
        for _ in 0..200 {
            g.update(0.1, &inp);
        }
        let p = g.player();
        assert!(!p.alive || p.hp < hp0, "storm should hurt anyone outside: hp {}", p.hp);
    }

    #[test]
    fn last_one_standing_wins() {
        let mut g = game(3, true);
        for i in 1..g.actors.len() {
            g.actors[i].alive = false;
            g.actors[i].mode = MoveMode::Dead;
        }
        g.update(0.1, &PlayerInput::default());
        assert_eq!(g.phase, Phase::Over);
        assert_eq!(g.winner, Some(PLAYER));
        assert!(g.events.iter().any(|e| matches!(e, Event::Victory { winner: 0 })));
    }

    /// Put the first `n` actors (the human and the bots after them) outside a tiny storm circle on low health, so that
    /// the next storm tick eliminates all of them in the same step.
    fn doom(g: &mut Game, n: usize) {
        g.storm.active = true;
        g.storm.center = Vec2::new(0.0, 0.0);
        g.storm.radius = 1.0;
        g.storm.dmg = 10.0;
        g.storm.tick = 0.99;
        for i in 0..n {
            let (x, z) = (120.0 + i as f32 * 6.0, 120.0);
            let h = g.world.hm.height_at(x, z);
            let a = &mut g.actors[i];
            a.pos = Vec3::new(x, h, z);
            a.mode = MoveMode::Ground;
            a.on_ground = true;
            a.hp = 5.0;
            a.shield = 0.0;
            a.brain = None;
        }
    }

    #[test]
    fn the_last_two_falling_in_the_same_step_draw_instead_of_one_winning_by_list_order() {
        let mut g = game(1, true);
        doom(&mut g, 2);
        g.update(1.0 / 60.0, &PlayerInput::default());
        assert!(g.actors.iter().all(|a| !a.alive), "the storm tick took both");
        assert_eq!(g.phase, Phase::Over);
        assert!(g.tie, "a draw is declared");
        assert_eq!(g.winner, None);
        assert!(!g.events.iter().any(|e| matches!(e, Event::Victory { .. })));
        assert_eq!(g.actors[PLAYER].placement, 1, "the human is not ranked below the bot just for coming first in the list");
        assert_eq!(g.actors[1].placement, 1);
    }

    #[test]
    fn several_falling_in_one_step_share_a_placement_and_the_match_goes_on() {
        let mut g = game(4, true);
        // survivors: actors 3 and 4 stay inside the circle
        doom(&mut g, 3);
        for i in 3..5 {
            let a = &mut g.actors[i];
            a.pos = Vec3::new(0.0, g.world.hm.height_at(0.0, 0.0), 0.0);
            a.brain = None;
            a.hp = 100.0;
        }
        g.update(1.0 / 60.0, &PlayerInput::default());
        assert_eq!((0..3).filter(|&i| !g.actors[i].alive).count(), 3);
        assert_eq!(g.phase, Phase::Playing);
        assert!(!g.tie && g.winner.is_none());
        for i in 0..3 {
            assert_eq!(g.actors[i].placement, 3, "actor {i}: three fell together with two left, tied for 3rd");
        }
        // the next one to fall is 2nd, as usual
        g.eliminate(3, None, "Test", false);
        assert_eq!(g.actors[3].placement, 2);
    }

    #[test]
    fn survived_time_stops_when_the_player_dies() {
        let mut g = game(2, true);
        let idle = PlayerInput::default();
        for _ in 0..60 {
            g.update(0.1, &idle);
        }
        let t_death = g.match_time;
        assert!(t_death > 5.0);
        let chest = g.actors[PLAYER].chest();
        g.damage_actor(PLAYER, 500.0, Some(1), "Test", false, chest, true);
        assert!(!g.actors[PLAYER].alive);
        // the match goes on (spectating): the clock keeps running but the player's time does not
        for _ in 0..60 {
            g.update(0.1, &idle);
        }
        assert!(g.match_time > t_death + 5.0);
        let j = hud::hud_json(&g, &g.camera(1.6), false);
        let key = "\"time\":";
        let at = j.find(key).expect("stats.time") + key.len();
        let shown: f32 = j[at..].split([',', '}']).next().unwrap().parse().unwrap();
        assert!((shown - t_death).abs() < 0.2, "the end screen shows {shown} s, the player fell at {t_death} s");
    }

    #[test]
    fn the_winner_dances_and_the_camera_swings_round_to_their_face() {
        let mut g = game(3, true);
        for i in 1..g.actors.len() {
            g.actors[i].alive = false;
            g.actors[i].mode = MoveMode::Dead;
        }
        // open ground, so nothing pulls the camera in
        let w = &g.world;
        let p = w.layout.pois[0].center + Vec2::new(w.layout.pois[0].radius * 0.6, 40.0);
        let h = w.hm.height_at(p.x, p.y);
        g.actors[PLAYER].pos = Vec3::new(p.x, h, p.y);
        g.actors[PLAYER].mode = MoveMode::Ground;
        g.actors[PLAYER].on_ground = true;
        let idle = PlayerInput::default();
        g.update(0.1, &idle);
        assert_eq!(g.phase, Phase::Over);
        assert!(g.actors[PLAYER].emoting, "the winner celebrates");
        for _ in 0..30 {
            g.update(0.1, &idle);
        }
        let a = &g.actors[PLAYER];
        assert!(a.anim.emote > 0.95, "the dance has fully blended in: {}", a.anim.emote);
        // the camera is in front of the dancer, close, and looking back at them
        let cam = g.camera(16.0 / 9.0);
        let to_cam = cam.pos - (a.pos + Vec3::Y * 1.05);
        let front = yaw_forward(a.body_yaw + 0.3 * a.anim.emote_clock);
        assert!((1.0..5.0).contains(&to_cam.length()), "orbit distance {}", to_cam.length());
        assert!(to_cam.normalize().dot(front) > 0.8, "camera should sit on the dancer's face side");
        assert!(cam.fwd.dot(-to_cam.normalize()) > 0.9, "camera should look at the dancer");
    }

    #[test]
    fn a_winner_caught_mid_reload_still_dances() {
        let mut g = game(2, true);
        for i in 1..g.actors.len() {
            g.actors[i].alive = false;
            g.actors[i].mode = MoveMode::Dead;
        }
        g.actors[PLAYER].mode = MoveMode::Ground;
        g.actors[PLAYER].on_ground = true;
        g.actors[PLAYER].action = Action::Reload { t: 0.5, dur: 2.0 };
        g.actors[PLAYER].build_mode = true;
        let idle = PlayerInput::default();
        g.update(0.1, &idle);
        for _ in 0..5 {
            g.update(0.1, &idle);
        }
        assert!(g.actors[PLAYER].emoting, "the victory dance must survive an in-progress action");
        assert!(!g.actors[PLAYER].build_mode);
    }

    #[test]
    fn a_bot_that_wins_stops_and_celebrates_too() {
        let mut g = game(3, true);
        g.actors[PLAYER].alive = false;
        g.actors[PLAYER].mode = MoveMode::Dead;
        for i in 2..g.actors.len() {
            g.actors[i].alive = false;
            g.actors[i].mode = MoveMode::Dead;
        }
        // bot 1 is the survivor and is mid-stride when the match ends
        g.actors[1].on_ground = true;
        g.actors[1].mode = MoveMode::Ground;
        let idle = PlayerInput::default();
        g.update(0.1, &idle);
        assert_eq!(g.winner, Some(1));
        let at = g.actors[1].pos;
        for _ in 0..40 {
            g.update(0.1, &idle);
        }
        let a = &g.actors[1];
        assert!(a.emoting || a.anim.emote > 0.9, "the winning bot dances");
        assert!(a.pos.distance(at) < 0.5, "and stays put instead of wandering off: moved {}", a.pos.distance(at));
        assert_eq!(g.camera_actor(), 1, "the spectator camera follows the winner");
    }
}
