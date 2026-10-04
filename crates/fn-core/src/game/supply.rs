//! Supply drops: a blue crate on a parachute that comes down inside the safe zone while the match is on.
//!
//! A crate is a [`Chest`] with `supply` set that starts high above its resting place and falls; everything else about it
//! (opening it, the loot popping out, bots going for it, being told to the other players) is a chest's.

use super::*;
use crate::math::*;

/// Seconds between the storm appearing and the first crate, and the range of the wait between two crates.
const FIRST_DELAY: f32 = 50.0;
const GAP: (f32, f32) = (85.0, 115.0);
/// Crates per match, and the storm phase from which the circle is too small to be worth another.
pub const MAX_DROPS: u32 = 5;
const LAST_PHASE: usize = 4;
/// How far above the ground a crate is let go, how fast it comes down (m/s), and the last stretch where the canopy slows it.
pub const DROP_HEIGHT: f32 = 240.0;
pub const FALL_SPEED: f32 = 12.0;
const SLOW_ZONE: f32 = 25.0;

/// Wait before the first crate; a storm that runs faster (tests, practice) brings it sooner.
pub fn first_delay(storm_speed: f32) -> f32 {
    FIRST_DELAY / storm_speed.max(0.01)
}

/// Let a crate fall for `dt` seconds (and count the time since it touched down). The host and every guest's copy of the match
/// run this the same way, so a crate announced once lands in the same place at the same time everywhere.
pub fn fall_step(c: &mut Chest, dt: f32) {
    if !c.supply {
        return;
    }
    if c.falling() {
        let above = c.pos.y - c.land_y;
        let v = FALL_SPEED * (0.3 + 0.7 * (above / SLOW_ZONE).clamp(0.0, 1.0));
        c.pos.y = (c.pos.y - v * dt).max(c.land_y);
        c.since_land = 0.0;
    } else {
        c.since_land += dt;
    }
}

/// Count the wait down and let the next crate go when it is over.
pub fn update(g: &mut Game, dt: f32) {
    if !g.storm.active || g.phase != Phase::Playing || g.supply_drops >= MAX_DROPS || g.storm.phase > LAST_PHASE {
        return;
    }
    g.supply_timer -= dt;
    if g.supply_timer > 0.0 {
        return;
    }
    match drop_spot(g) {
        Some(at) => {
            launch(g, at);
            g.supply_timer = g.rng.range(GAP.0, GAP.1) / g.cfg.storm_speed.max(0.01);
        }
        // nowhere suitable right now (the circle is mostly sea): look again shortly
        None => g.supply_timer = 4.0,
    }
}

/// Dry, level, open ground inside the circle the storm is heading for, so the crate is worth the walk.
pub fn drop_spot(g: &mut Game) -> Option<Vec2> {
    let (c, r) = if g.storm.state == StormState::Done { (g.storm.center, g.storm.radius) } else { (g.storm.to_center, g.storm.to_radius) };
    let r = (r * 0.85).max(10.0);
    for _ in 0..80 {
        let p = c + g.rng.in_disc(r);
        let h = g.world.hm.height_at(p.x, p.y);
        if h < 3.0 || g.world.hm.slope_at(p.x, p.y) > 0.1 || g.world.layout.lakes.iter().any(|l| l.center.distance(p) < l.radius * 1.4) {
            continue;
        }
        // nothing (a tree, a wall, a rock) within a few metres: the crate must sit in the open
        let mut blocked = false;
        g.world.statics.query(&Aabb::new(Vec3::new(p.x - 3.0, -10.0, p.y - 3.0), Vec3::new(p.x + 3.0, 200.0, p.y + 3.0)), |_, _| blocked = true);
        if !blocked {
            return Some(p);
        }
    }
    None
}

/// Let a crate go high above `at`; returns its id.
pub fn launch(g: &mut Game, at: Vec2) -> u32 {
    let h = g.world.hm.height_at(at.x, at.y);
    let id = g.new_id();
    let yaw = g.rng.range(0.0, std::f32::consts::TAU);
    g.chests.push(Chest { id, pos: Vec3::new(at.x, h + DROP_HEIGHT, at.y), yaw, open_t: 0.0, opened: false, supply: true, land_y: h, since_land: 0.0 });
    g.supply_drops += 1;
    g.toast("Supply drop incoming. Follow the blue beam", 6.0, 0);
    id
}

#[cfg(test)]
mod tests {
    use super::super::testutil::game_cfg;
    use super::*;

    fn quiet_game() -> Game {
        let mut g = game_cfg(GameConfig { bots: 2, skip_bus: true, seed: 7, god_mode: true, ..Default::default() });
        // nobody shoots the player while the test waits
        for a in g.actors.iter_mut().skip(1) {
            a.brain = None;
        }
        g
    }

    fn supply_crates(g: &Game) -> Vec<&Chest> {
        g.chests.iter().filter(|c| c.supply).collect()
    }

    #[test]
    fn a_supply_drop_comes_down_inside_the_safe_zone_and_gives_the_best_loot() {
        let mut g = quiet_game();
        assert!(supply_crates(&g).is_empty(), "nothing falls before the match is under way");
        let mut waited = 0.0;
        while supply_crates(&g).is_empty() && waited < 120.0 {
            g.update(1.0 / 30.0, &PlayerInput::default());
            waited += 1.0 / 30.0;
        }
        assert_eq!(supply_crates(&g).len(), 1, "the first crate comes after {waited:.0} s");
        assert!((FIRST_DELAY - 2.0..FIRST_DELAY + 2.0).contains(&waited), "first crate after {waited:.0} s");
        assert!(g.toast.iter().any(|t| t.0.contains("Supply drop")), "players are told: {:?}", g.toast);
        let id = supply_crates(&g)[0].id;
        let c = g.chests[g.chest_index(id).unwrap()].clone();
        assert!(c.falling() && c.pos.y > c.land_y + DROP_HEIGHT - 5.0, "it starts high above the ground");
        // in the circle the storm is heading for, on dry open ground
        assert!(Vec2::new(c.pos.x, c.pos.z).distance(g.storm.to_center) <= g.storm.to_radius, "outside the safe zone");
        assert!(c.land_y > 3.0 && (c.land_y - g.world.hm.height_at(c.pos.x, c.pos.z)).abs() < 0.01);
        // nobody can open it in mid-air, even standing right under it
        g.actors[PLAYER].pos = Vec3::new(c.pos.x, c.land_y, c.pos.z + 1.0);
        g.actors[PLAYER].mode = MoveMode::Ground;
        g.actors[PLAYER].on_ground = true;
        assert_eq!(g.find_target(PLAYER), None);
        assert!(!g.interact(PLAYER));
        // it lands after about twenty seconds, and not before ten
        let mut t = 0.0;
        while g.chests[g.chest_index(id).unwrap()].falling() && t < 60.0 {
            g.actors[PLAYER].pos = Vec3::new(c.pos.x, c.land_y, c.pos.z + 1.0);
            g.update(1.0 / 30.0, &PlayerInput::default());
            t += 1.0 / 30.0;
        }
        assert!((12.0..40.0).contains(&t), "the crate took {t:.1} s to land");
        let landed = g.chests[g.chest_index(id).unwrap()].clone();
        assert!((landed.pos.y - landed.land_y).abs() < 1e-4 && !landed.opened);
        assert_eq!((landed.pos.x, landed.pos.z), (c.pos.x, c.pos.z), "it comes straight down");
        // now it is a chest: E opens it and the loot is better than any other chest's
        g.actors[PLAYER].pos = Vec3::new(landed.pos.x, landed.land_y, landed.pos.z + 1.5);
        let before = g.pickups.len();
        assert_eq!(g.find_target(PLAYER), Some(loot::Target::Chest(id)));
        assert!(g.interact(PLAYER));
        assert!(g.chests[g.chest_index(id).unwrap()].opened);
        let dropped: Vec<&Pickup> = g.pickups.iter().skip(before).collect();
        let weapons: Vec<Rarity> = dropped.iter().filter_map(|p| if let PickupKind::Weapon { rarity, .. } = p.kind { Some(rarity) } else { None }).collect();
        assert_eq!(weapons.len(), 2, "{} items", dropped.len());
        assert!(weapons.iter().all(|r| *r >= Rarity::Epic), "{weapons:?}");
        assert!(dropped.len() >= 6, "{} items came out", dropped.len());
        assert!(g.events.iter().any(|e| matches!(e, Event::ChestOpen { .. })));
    }

    #[test]
    fn crates_keep_coming_but_only_a_few_and_never_when_the_circle_is_tiny() {
        let mut g = quiet_game();
        g.cfg.storm_speed = 20.0;
        g.supply_timer = first_delay(20.0);
        g.storm.timer = 1e9; // the circle stays where it is
        for _ in 0..(60 * 30) {
            g.update(1.0 / 30.0, &PlayerInput::default());
        }
        let n = supply_crates(&g).len() as u32;
        assert_eq!(n, g.supply_drops);
        assert_eq!(n, MAX_DROPS, "{n} crates in the first minutes");
        let ids: std::collections::HashSet<u32> = supply_crates(&g).iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), n as usize, "every crate has its own id");
        // in the last circles nothing more is sent
        let mut g = quiet_game();
        g.storm.phase = LAST_PHASE + 1;
        g.supply_timer = 0.0;
        for _ in 0..30 {
            g.update(1.0 / 30.0, &PlayerInput::default());
        }
        assert!(supply_crates(&g).is_empty());
    }

    #[test]
    fn a_crate_never_lands_in_the_water_or_among_trees_and_walls() {
        let mut g = quiet_game();
        let mut found = 0;
        for _ in 0..300 {
            // anywhere the circle might be
            g.storm.to_center = g.rng.in_disc(crate::world::WORLD_HALF * 0.8);
            g.storm.to_radius = g.rng.range(60.0, 500.0);
            let Some(p) = drop_spot(&mut g) else { continue };
            found += 1;
            assert!(g.world.hm.height_at(p.x, p.y) >= 3.0, "{p:?} is by the sea");
            assert!(g.world.layout.lakes.iter().all(|l| l.center.distance(p) >= l.radius * 1.4), "{p:?} is by a lake");
            assert!(p.distance(g.storm.to_center) <= g.storm.to_radius);
            let mut hits = 0;
            g.world.statics.query(&Aabb::new(Vec3::new(p.x - 2.5, 0.0, p.y - 2.5), Vec3::new(p.x + 2.5, 100.0, p.y + 2.5)), |_, _| hits += 1);
            assert_eq!(hits, 0, "{p:?} is among trees or walls");
        }
        assert!(found > 150, "a spot was found {found} times in 300");
    }

    #[test]
    fn a_bot_opens_a_supply_crate_once_it_has_landed_and_not_before() {
        let mut g = game_cfg(GameConfig { bots: 1, skip_bus: true, seed: 7, god_mode: true, ..Default::default() });
        g.pickups.clear();
        g.chests.clear();
        // the bot is armed; the crate comes down 50 m from it
        let bot = 1;
        let at = Vec2::new(g.actors[bot].pos.x, g.actors[bot].pos.z);
        g.actors[bot].inv.add_weapon(WeaponKind::AssaultRifle, Rarity::Rare, 30);
        g.actors[bot].inv.ammo[AmmoKind::Medium.index()] = 90;
        g.storm.to_center = at;
        g.storm.to_radius = 90.0;
        let spot = drop_spot(&mut g).expect("open ground near the bot");
        let id = launch(&mut g, spot);
        // while it is in the air it cannot be opened, by a bot or anyone else
        for _ in 0..(10 * 30) {
            g.update(1.0 / 30.0, &PlayerInput::default());
        }
        assert!(g.chests[g.chest_index(id).unwrap()].falling(), "still in the air after ten seconds");
        assert!(!g.chests[g.chest_index(id).unwrap()].opened);
        // once it is down the bot goes and opens it
        let i = g.chest_index(id).unwrap();
        g.chests[i].pos.y = g.chests[i].land_y;
        let mut opened_at = None;
        for k in 0..(120 * 30) {
            g.update(1.0 / 30.0, &PlayerInput::default());
            if g.chests[g.chest_index(id).unwrap()].opened {
                opened_at = Some(k as f32 / 30.0);
                break;
            }
        }
        assert!(opened_at.is_some(), "the bot never opened the crate (it ended {:.0} m away)", g.actors[bot].pos.distance(Vec3::new(spot.x, g.actors[bot].pos.y, spot.y)));
    }

    #[test]
    fn a_falling_crate_is_slowest_near_the_ground_and_never_goes_through_it() {
        let mut c = Chest { id: 1, pos: Vec3::new(0.0, 100.0 + DROP_HEIGHT, 0.0), yaw: 0.0, open_t: 0.0, opened: false, supply: true, land_y: 100.0, since_land: 0.0 };
        let (mut high, mut low) = (0.0, 0.0);
        let mut prev = c.pos.y;
        let mut t = 0.0;
        while c.falling() {
            fall_step(&mut c, 1.0 / 60.0);
            t += 1.0 / 60.0;
            let v = (prev - c.pos.y) * 60.0;
            if c.pos.y - c.land_y > SLOW_ZONE + 1.0 { high = v } else if c.pos.y - c.land_y < 3.0 { low = v }
            prev = c.pos.y;
            assert!(c.pos.y >= c.land_y);
        }
        assert!(low < high * 0.5, "{low} m/s at the end, {high} m/s up high");
        assert!((15.0..30.0).contains(&t), "{t:.1} s in the air");
        fall_step(&mut c, 2.0);
        assert!((c.since_land - 2.0).abs() < 1e-3 && c.pos.y == c.land_y);
        // an ordinary chest does not move
        let mut normal = Chest::on_ground(2, Vec3::new(0.0, 5.0, 0.0), 0.0);
        fall_step(&mut normal, 1.0);
        assert_eq!((normal.pos.y, normal.since_land), (5.0, 0.0));
    }
}
