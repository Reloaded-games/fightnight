//! Character movement: ground, swimming, skydiving and gliding. Shared by the
//! player and every bot.

use super::actor::*;
use super::env::*;
use super::events::*;
use super::intent::Intent;
use super::items::Item;
use crate::math::*;
use crate::world::collision::Tag;
use crate::world::splat::CH_ASPHALT;

pub const RUN_SPEED: f32 = 6.4;
pub const SPRINT_MUL: f32 = 1.36;
pub const CROUCH_MUL: f32 = 0.5;
pub const ADS_MUL: f32 = 0.64;
pub const ACCEL: f32 = 46.0;
pub const DECEL: f32 = 38.0;
pub const AIR_ACCEL: f32 = 9.0;
pub const GRAVITY: f32 = 24.5;
pub const JUMP_VEL: f32 = 8.0;
pub const MAX_SLOPE: f32 = 1.3;
pub const SNAP_DOWN: f32 = 0.75;
pub const SWIM_SPEED: f32 = 3.4;
pub const FALL_DAMAGE_MIN: f32 = 7.5;

#[derive(Default, Clone, Copy)]
pub struct MoveResult {
    pub fall_damage: f32,
    pub landed_from_sky: bool,
}

pub fn surface_under(env: &Env, pos: Vec3, g: Ground) -> Surface {
    if let Some(tag) = g.tag {
        return match tag {
            Tag::Building(_) | Tag::Piece(_) | Tag::Prop(_) => Surface::Wood,
            _ => Surface::Stone,
        };
    }
    if env.water_depth(pos.x, pos.z) > 0.15 {
        return Surface::Water;
    }
    if g.y < 2.4 {
        Surface::Sand
    } else if env.world.splat.sample(Vec2::new(pos.x, pos.z))[CH_ASPHALT] > 0.5 {
        Surface::Stone
    } else {
        Surface::Grass
    }
}

fn steer(v: Vec2, target: Vec2, accel: f32, dt: f32) -> Vec2 {
    let dv = target - v;
    let max = accel * dt;
    let l = dv.length();
    if l <= max {
        target
    } else {
        v + dv / l * max
    }
}

/// Speed multiplier from the item in hand and current action.
fn speed_mods(a: &Actor) -> f32 {
    let mut m = 1.0;
    if let Some((kind, _, _)) = a.inv.selected_weapon() {
        m *= kind.def().move_mul;
    }
    if matches!(a.action, Action::Heal { .. }) {
        m *= 0.7;
    }
    m
}

pub fn step_ground(a: &mut Actor, it: &Intent, env: &Env, dt: f32, ev: &mut Vec<Event>) -> MoveResult {
    let mut res = MoveResult::default();
    // ---- timers & crouch -----------------------------------------------------
    a.jump_buffer = (a.jump_buffer - dt).max(0.0);
    if it.jump {
        a.jump_buffer = 0.14;
    }
    a.coyote = if a.on_ground { 0.1 } else { (a.coyote - dt).max(0.0) };
    let want_crouch = it.crouch;
    if a.crouching && !want_crouch {
        let clear = env.ceiling(a.pos.x, a.pos.z, a.pos.y + CROUCH_HEIGHT).is_none_or(|c| c > a.pos.y + HEIGHT + 0.05);
        if clear {
            a.crouching = false;
        }
    } else {
        a.crouching = want_crouch;
    }

    // ---- wish velocity ----------------------------------------------------------
    let wish = {
        let w = it.wish;
        if w.length() > 1.0 {
            w.normalize()
        } else {
            w
        }
    };
    let moving_input = wish.length() > 0.1;
    let firing = it.fire && matches!(a.inv.selected_item(), Some(Item::Weapon { .. }));
    a.sprinting = it.sprint && moving_input && !a.ads && !a.crouching && !firing && a.on_ground;
    let depth = env.water_depth(a.pos.x, a.pos.z);
    let mut speed = RUN_SPEED * speed_mods(a);
    if a.crouching {
        speed *= CROUCH_MUL;
    } else if a.sprinting {
        speed *= SPRINT_MUL;
    }
    if a.ads {
        speed *= ADS_MUL;
    }
    if depth > 0.3 {
        speed *= 1.0 - 0.3 * ((depth - 0.3) / 0.8).clamp(0.0, 1.0);
    }
    let target = wish * speed;
    let accel = if a.on_ground {
        if moving_input {
            ACCEL
        } else {
            DECEL
        }
    } else {
        AIR_ACCEL
    };
    let mut vh = steer(Vec2::new(a.vel.x, a.vel.z), target, accel, dt);

    // steep terrain: slide along contours instead of climbing
    if a.on_ground {
        let grad = env.world.hm.gradient_at(a.pos.x, a.pos.z);
        let slope = grad.length();
        if slope > MAX_SLOPE {
            let up = grad / slope;
            let into = vh.dot(up);
            if into > 0.0 {
                vh -= up * into;
            }
        }
    }

    // ---- jump & gravity -------------------------------------------------------------
    if a.jump_buffer > 0.0 && (a.on_ground || a.coyote > 0.0) {
        a.vel.y = JUMP_VEL;
        a.on_ground = false;
        a.coyote = 0.0;
        a.jump_buffer = 0.0;
        ev.push(Event::Jump { actor: a.id, pos: a.pos });
    }
    if !a.on_ground {
        a.vel.y = (a.vel.y - GRAVITY * dt).max(-70.0);
        a.peak_y = a.peak_y.max(a.pos.y);
    }

    // ---- horizontal integration + collision -----------------------------------------
    let old = a.pos;
    let mut p = old;
    p.x += vh.x * dt;
    p.z += vh.y * dt;
    let h = a.height();
    // what the colliders pushed us back by; summing the push vectors themselves (rather than comparing the
    // position before and after) keeps f32 rounding of `pos + v*dt` — which grows with |pos| — from looking like a wall
    let mut pushed = Vec2::ZERO;
    for _ in 0..3 {
        let push = env.push_out(p, RADIUS, h, STEP_HEIGHT);
        if push.length_squared() < 1e-9 {
            break;
        }
        p.x += push.x;
        p.z += push.y;
        pushed += push;
    }
    // slide: drop the part of the velocity that points into whatever pushed us
    if pushed.length_squared() > 1e-10 {
        let n = pushed.normalize();
        let vn = vh.dot(n);
        if vn < 0.0 {
            vh -= n * vn;
        }
    }
    a.vel.x = vh.x;
    a.vel.z = vh.y;

    // ---- vertical ------------------------------------------------------------------------
    let ground = env.ground(p.x, p.z, old.y, STEP_HEIGHT);
    if a.on_ground {
        if ground.y > old.y + STEP_HEIGHT + 1e-3 {
            // terrain too high to step onto (a cliff): stay put
            p.x = old.x;
            p.z = old.z;
            a.vel.x = 0.0;
            a.vel.z = 0.0;
            p.y = env.ground(p.x, p.z, old.y, STEP_HEIGHT).y.max(old.y.min(p.y));
            p.y = old.y;
        } else if ground.y >= old.y - SNAP_DOWN {
            p.y = ground.y;
        } else {
            // walked off a ledge
            a.on_ground = false;
            p.y = old.y;
            a.vel.y = 0.0;
        }
    } else {
        p.y = old.y + a.vel.y * dt;
        if a.vel.y > 0.0 {
            if let Some(c) = env.ceiling(p.x, p.z, old.y + h) {
                if p.y + h > c {
                    p.y = c - h;
                    a.vel.y = 0.0;
                }
            }
        }
        let g = env.ground(p.x, p.z, old.y.max(p.y), STEP_HEIGHT);
        if a.vel.y <= 0.0 && p.y <= g.y {
            // landing
            let speed_down = -a.vel.y;
            let fall = a.peak_y - g.y;
            p.y = g.y;
            a.vel.y = 0.0;
            a.on_ground = true;
            let surf = surface_under(env, p, g);
            ev.push(Event::Land { actor: a.id, pos: p, speed: speed_down, surface: surf });
            a.anim.land = (speed_down / 14.0).clamp(0.0, 1.0);
            if fall > FALL_DAMAGE_MIN && surf != Surface::Water && env.water_depth(p.x, p.z) < 1.0 {
                res.fall_damage = (6.0 + (fall - FALL_DAMAGE_MIN) * 4.5).min(70.0);
            }
        }
    }
    let travelled = Vec2::new(p.x - old.x, p.z - old.z).length();
    a.pos = p;
    if a.on_ground {
        a.peak_y = a.pos.y;
        // footsteps
        if !a.crouching {
            a.steps += travelled;
            let stride = if a.sprinting { 2.9 } else { 2.15 };
            if a.steps > stride {
                a.steps -= stride;
                let g = env.ground(a.pos.x, a.pos.z, a.pos.y + 0.2, 0.3);
                ev.push(Event::Footstep { actor: a.id, pos: a.pos, surface: surface_under(env, a.pos, g) });
            }
        }
    }

    // ---- into the water? -------------------------------------------------------------------
    let d = env.water_depth(a.pos.x, a.pos.z);
    if d > 1.15 && a.on_ground {
        if let Some(l) = env.water_level(a.pos.x, a.pos.z) {
            a.mode = MoveMode::Swim;
            a.vel.y = 0.0;
            a.pos.y = l - 0.95;
            a.crouching = false;
        }
    }
    res
}

pub fn step_swim(a: &mut Actor, it: &Intent, env: &Env, dt: f32, _ev: &mut Vec<Event>) {
    let wish = if it.wish.length() > 1.0 { it.wish.normalize() } else { it.wish };
    let v = steer(Vec2::new(a.vel.x, a.vel.z), wish * SWIM_SPEED, 12.0, dt);
    let mut p = a.pos;
    p.x += v.x * dt;
    p.z += v.y * dt;
    for _ in 0..2 {
        let push = env.push_out(p, RADIUS, 1.0, 0.3);
        if push.length_squared() < 1e-9 {
            break;
        }
        p.x += push.x;
        p.z += push.y;
    }
    a.vel.x = v.x;
    a.vel.z = v.y;
    let depth = env.water_depth(p.x, p.z);
    let g = env.ground(p.x, p.z, p.y, 0.5);
    if depth < 0.8 {
        // wading again
        a.mode = MoveMode::Ground;
        a.on_ground = true;
        a.pos = Vec3::new(p.x, g.y, p.z);
        a.peak_y = a.pos.y;
        return;
    }
    let lvl = env.water_level(p.x, p.z).unwrap_or(0.0);
    let bob = (a.anim.time * 2.0).sin() * 0.04;
    a.pos = Vec3::new(p.x, lvl - 0.95 + bob, p.z);
    a.peak_y = a.pos.y;
    a.sprinting = false;
    a.crouching = false;
    a.ads = false;
}

// ---------------------------------------------------------------------------------------
// Skydiving & gliding
// ---------------------------------------------------------------------------------------

pub const GLIDER_AUTO_DEPLOY: f32 = 95.0;

/// Free fall: look down to dive fast, look ahead to cover distance.
pub fn step_freefall(a: &mut Actor, it: &Intent, env: &Env, dt: f32, ev: &mut Vec<Event>) -> MoveResult {
    let dive = ((-a.pitch - 0.1) / 1.1).clamp(0.0, 1.0);
    let fall_speed = lerp(26.0, 60.0, dive);
    let h_speed = lerp(30.0, 9.0, dive);
    let fwd = yaw_forward(a.yaw);
    let right = yaw_right(a.yaw);
    let steer_in = it.wish; // world-space wish from WASD relative to camera was already applied by the caller
    let target = Vec2::new(fwd.x, fwd.z) * h_speed + steer_in * 7.0;
    let v = steer(Vec2::new(a.vel.x, a.vel.z), target, 18.0, dt);
    a.vel.x = v.x;
    a.vel.z = v.y;
    a.vel.y = approach(a.vel.y, -fall_speed, 34.0 * dt);
    a.pos += a.vel * dt;
    let _ = right;
    let ground = env.ground(a.pos.x, a.pos.z, a.pos.y, 0.0);
    let height_above = a.pos.y - ground.y;
    let mut res = MoveResult::default();
    if (it.deploy && height_above > 30.0) || height_above < GLIDER_AUTO_DEPLOY {
        a.mode = MoveMode::Glide;
        a.glide_deployed = true;
        ev.push(Event::GliderDeploy { actor: a.id, pos: a.pos });
        return res;
    }
    // a failed glider (should not happen) – land hard
    if height_above <= 0.0 {
        a.pos.y = ground.y;
        land_from_sky(a, env, ev);
        res.landed_from_sky = true;
    }
    res
}

pub fn step_glide(a: &mut Actor, it: &Intent, env: &Env, dt: f32, ev: &mut Vec<Event>) -> MoveResult {
    // nose down = faster, nose up = slower
    let nose = (-a.pitch).clamp(-0.35, 0.9);
    let dive = (nose / 0.9).clamp(0.0, 1.0);
    let flare = (-nose / 0.35).clamp(0.0, 1.0);
    let h_speed = 13.5 + 12.0 * dive - 4.0 * flare;
    let sink = 6.0 + 9.0 * dive - 2.0 * flare;
    let fwd = yaw_forward(a.yaw);
    let target = Vec2::new(fwd.x, fwd.z) * h_speed + it.wish * 3.5;
    let v = steer(Vec2::new(a.vel.x, a.vel.z), target, 10.0, dt);
    a.vel.x = v.x;
    a.vel.z = v.y;
    a.vel.y = approach(a.vel.y, -sink, 14.0 * dt);
    a.pos += a.vel * dt;
    let mut res = MoveResult::default();
    let ground = env.ground(a.pos.x, a.pos.z, a.pos.y + 2.0, 0.6);
    if a.pos.y <= ground.y + 0.02 || (it.deploy && a.pos.y - ground.y < 3.0) {
        a.pos.y = ground.y.max(a.pos.y.min(ground.y + 0.02));
        land_from_sky(a, env, ev);
        res.landed_from_sky = true;
    }
    // trees / buildings: bounce off obstacles horizontally
    let push = env.push_out(a.pos, RADIUS, HEIGHT, 0.2);
    a.pos.x += push.x;
    a.pos.z += push.y;
    res
}

fn land_from_sky(a: &mut Actor, env: &Env, ev: &mut Vec<Event>) {
    let g = env.ground(a.pos.x, a.pos.z, a.pos.y + 0.5, 0.6);
    a.pos.y = g.y;
    let speed = -a.vel.y;
    a.vel.y = 0.0;
    a.vel.x *= 0.35;
    a.vel.z *= 0.35;
    a.glide_deployed = false;
    a.peak_y = a.pos.y;
    if env.water_depth(a.pos.x, a.pos.z) > 1.15 {
        a.mode = MoveMode::Swim;
    } else {
        a.mode = MoveMode::Ground;
        a.on_ground = true;
    }
    a.anim.land = 0.7;
    ev.push(Event::Land { actor: a.id, pos: a.pos, speed, surface: surface_under(env, a.pos, g) });
}

// ---------------------------------------------------------------------------------------
// Body orientation and animation state
// ---------------------------------------------------------------------------------------

pub fn update_body(a: &mut Actor, it: &Intent, dt: f32) {
    let sp = a.speed_xz();
    let aiming = it.fire || a.ads || a.shot_flash > 0.0 || a.build_mode;
    let mut target = a.body_yaw;
    if aiming || matches!(a.mode, MoveMode::Freefall | MoveMode::Glide | MoveMode::Bus) {
        target = a.yaw;
    } else if sp > 0.8 {
        target = yaw_of(Vec2::new(a.vel.x, a.vel.z));
    } else if angle_diff(a.body_yaw, a.yaw).abs() > 1.4 {
        target = a.yaw;
    }
    a.body_yaw = lerp_angle(a.body_yaw, target, damp(16.0, dt));
}

pub fn update_anim(a: &mut Actor, dt: f32) {
    let an = &mut a.anim;
    an.time += dt;
    let sp = Vec2::new(a.vel.x, a.vel.z).length();
    let ground = matches!(a.mode, MoveMode::Ground) && a.on_ground;
    let run_target = if ground { (sp / RUN_SPEED).clamp(0.0, 1.6) } else { 0.0 };
    an.run = lerp(an.run, run_target, damp(14.0, dt));
    an.sprint = lerp(an.sprint, if a.sprinting { 1.0 } else { 0.0 }, damp(10.0, dt));
    an.crouch = lerp(an.crouch, if a.crouching { 1.0 } else { 0.0 }, damp(14.0, dt));
    an.air = lerp(an.air, if ground { 0.0 } else { 1.0 }, damp(12.0, dt));
    an.aim = lerp(an.aim, if a.ads || a.shot_flash > 0.0 { 1.0 } else { 0.0 }, damp(16.0, dt));
    an.land = (an.land - dt * 3.5).max(0.0);
    an.recoil = (an.recoil - dt * 9.0).max(0.0);
    an.swing = (an.swing - dt * 3.2).max(0.0);
    an.reload = match a.action {
        Action::Reload { t, dur } => (t / dur).clamp(0.0, 1.0),
        _ => 0.0,
    };
    an.heal = match a.action {
        Action::Heal { t, dur, .. } => (t / dur).clamp(0.0, 1.0),
        _ => 0.0,
    };
    an.build = lerp(an.build, if a.build_mode { 1.0 } else { 0.0 }, damp(12.0, dt));
    an.emote = lerp(an.emote, if a.emoting { 1.0 } else { 0.0 }, damp(9.0, dt));
    if an.emote > 0.01 {
        an.emote_clock += dt;
    }
    if ground && sp > 0.2 {
        an.phase += sp * dt * (if a.crouching { 2.6 } else { 1.55 });
    } else if !ground {
        an.phase += dt * 2.0;
    }
    // lean into movement, in the body's local frame
    let (s, c) = a.body_yaw.sin_cos();
    let fwd = Vec2::new(-s, -c);
    let right = Vec2::new(c, -s);
    let v = Vec2::new(a.vel.x, a.vel.z);
    an.lean_fwd = lerp(an.lean_fwd, v.dot(fwd) / RUN_SPEED, damp(10.0, dt));
    an.lean_side = lerp(an.lean_side, v.dot(right) / RUN_SPEED, damp(10.0, dt));
    a.shot_flash = (a.shot_flash - dt).max(0.0);
    a.hit_flash = (a.hit_flash - dt * 4.0).max(0.0);
    let eye_target = a.pos.y;
    a.eye_smooth = if a.eye_smooth == 0.0 || (a.eye_smooth - eye_target).abs() > 4.0 { eye_target } else { lerp(a.eye_smooth, eye_target, damp(14.0, dt)) };
}

#[cfg(test)]
mod tests {
    use super::super::env::testutil::world;
    use super::*;
    use crate::game::items::*;
    use crate::rng::Rng;
    use crate::world::collision::SpatialGrid;

    fn setup() -> (Actor, SpatialGrid) {
        let mut rng = Rng::new(1);
        let mut a = Actor::new(0, "t", true, Outfit::random(&mut rng));
        a.mode = MoveMode::Ground;
        a.on_ground = true;
        a.inv.add_weapon(WeaponKind::AssaultRifle, Rarity::Common, 30);
        (a, SpatialGrid::new(crate::world::WORLD_HALF + 16.0, 8.0))
    }

    fn run(a: &mut Actor, it: &Intent, env: &Env, secs: f32) -> Vec<Event> {
        let mut ev = vec![];
        let dt = 1.0 / 60.0;
        for _ in 0..(secs / dt) as usize {
            step_ground(a, it, env, dt, &mut ev);
            update_anim(a, dt);
        }
        ev
    }

    fn open_field() -> Vec3 {
        // town plateau, away from buildings: take a point on the main street
        let w = world();
        let p = &w.layout.pois[0];
        Vec3::new(p.center.x + p.radius * 0.7, p.ground, p.center.y)
    }

    #[test]
    fn accelerates_to_run_speed_on_flat_ground_and_stops() {
        let w = world();
        let (mut a, pieces) = setup();
        let env = Env::new(w, &pieces);
        let p = open_field();
        a.pos = Vec3::new(p.x, w.hm.height_at(p.x, p.z), p.z);
        let it = Intent { wish: Vec2::new(0.0, 1.0), ..Default::default() };
        // walk along +Z for 1 s
        run(&mut a, &it, &env, 1.0);
        assert!((a.speed_xz() - RUN_SPEED * 0.97).abs() < 0.8, "speed {}", a.speed_xz());
        assert!(a.on_ground);
        // release: decelerates to a stop quickly
        run(&mut a, &Intent::default(), &env, 0.6);
        assert!(a.speed_xz() < 0.05, "should stop: {}", a.speed_xz());
    }

    /// A start point on dry, gentle, obstacle-free ground with `len` metres of clear run along `dir`,
    /// whose coordinate along the run is at least `min_abs` from the origin (either side).
    fn clear_run(env: &Env, dir: Vec2, min_abs: f32, len: f32) -> Option<Vec3> {
        let along = |p: Vec2| p.dot(dir);
        let mut o = -420.0;
        while o <= 420.0 {
            let mut c = min_abs + 4.0;
            while c <= 560.0 {
                for sign in [1.0f32, -1.0] {
                    let start = dir * (sign * c) + Vec2::new(-dir.y, dir.x) * o;
                    let ok = (0..=len as i32).all(|k| {
                        let p = start + dir * k as f32;
                        let y = env.terrain(p.x, p.y);
                        env.water_depth(p.x, p.y) == 0.0
                            && along(p).abs() >= min_abs
                            && env.world.hm.gradient_at(p.x, p.y).length() < 0.2
                            && env.push_out(Vec3::new(p.x, y, p.y), RADIUS + 1.5, HEIGHT, STEP_HEIGHT).length() == 0.0
                    });
                    if ok {
                        return Some(Vec3::new(start.x, env.terrain(start.x, start.y), start.y));
                    }
                }
                c += 12.0;
            }
            o += 20.0;
        }
        None
    }

    #[test]
    fn run_speed_is_the_same_far_from_the_world_origin() {
        // f32 rounding of `pos + v*dt` grows with the coordinate; beyond 256 m it exceeded the tolerance used to
        // detect wall hits and zeroed the run velocity every other frame (a crawl across most of the island)
        let w = world();
        let (mut a, pieces) = setup();
        let env = Env::new(w, &pieces);
        for dir in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
            let start = clear_run(&env, dir, 262.0, 14.0).unwrap_or_else(|| panic!("no clear run along {dir:?}"));
            for hz in [30.0f32, 60.0, 144.0, 240.0] {
                let dt = 1.0 / hz;
                a.pos = start;
                a.vel = Vec3::ZERO;
                a.on_ground = true;
                let it = Intent { wish: dir, ..Default::default() };
                let mut ev = vec![];
                let mut travelled = 0.0;
                for _ in 0..(1.2 * hz) as usize {
                    let before = a.pos;
                    step_ground(&mut a, &it, &env, dt, &mut ev);
                    travelled += Vec2::new(a.pos.x - before.x, a.pos.z - before.z).length();
                }
                // 1.2 s at 6.4 m/s with a 0.14 s ramp-up is about 7.0 m
                assert!(travelled > 6.4, "{hz} Hz along {dir:?} from {start:?}: only {travelled:.2} m in 1.2 s (speed {:.2})", a.speed_xz());
                assert!(a.speed_xz() > RUN_SPEED * 0.95, "{hz} Hz along {dir:?} from {start:?}: speed {:.2}", a.speed_xz());
            }
        }
    }

    #[test]
    fn sprint_is_faster_and_ads_slower() {
        let w = world();
        let (mut a, pieces) = setup();
        let env = Env::new(w, &pieces);
        let p = open_field();
        a.pos = Vec3::new(p.x, w.hm.height_at(p.x, p.z), p.z);
        run(&mut a, &Intent { wish: Vec2::new(0.0, -1.0), sprint: true, ..Default::default() }, &env, 1.2);
        let sprint = a.speed_xz();
        run(&mut a, &Intent::default(), &env, 0.8);
        a.ads = true;
        run(&mut a, &Intent { wish: Vec2::new(0.0, 1.0), ..Default::default() }, &env, 1.0);
        let ads = a.speed_xz();
        assert!(sprint > RUN_SPEED * 1.2, "sprint {sprint}");
        assert!(ads < RUN_SPEED * 0.7, "ads {ads}");
    }

    #[test]
    fn jump_leaves_ground_and_lands_back_with_event() {
        let w = world();
        let (mut a, pieces) = setup();
        let env = Env::new(w, &pieces);
        let p = open_field();
        let ground_y = w.hm.height_at(p.x, p.z);
        a.pos = Vec3::new(p.x, ground_y, p.z);
        let mut ev = vec![];
        let dt = 1.0 / 60.0;
        step_ground(&mut a, &Intent { jump: true, ..Default::default() }, &env, dt, &mut ev);
        assert!(!a.on_ground && a.vel.y > 5.0);
        let mut apex: f32 = 0.0;
        for _ in 0..120 {
            step_ground(&mut a, &Intent::default(), &env, dt, &mut ev);
            apex = apex.max(a.pos.y - ground_y);
        }
        assert!((0.95..1.6).contains(&apex), "jump apex {apex}");
        assert!(a.on_ground);
        assert!(ev.iter().any(|e| matches!(e, Event::Jump { .. })));
        assert!(ev.iter().any(|e| matches!(e, Event::Land { .. })));
        assert!((a.pos.y - ground_y).abs() < 0.05);
    }

    #[test]
    fn walks_through_door_and_up_the_step_but_not_through_walls() {
        let w = world();
        let (mut a, pieces) = setup();
        let env = Env::new(w, &pieces);
        let b = w.buildings.iter().find(|b| b.kind == "house").unwrap();
        // start outside the door and walk toward the door_in point
        let start = b.door_out;
        a.pos = Vec3::new(start.x, w.hm.height_at(start.x, start.z), start.z);
        let dir = Vec2::new(b.door_in.x - start.x, b.door_in.z - start.z).normalize();
        let it = Intent { wish: dir, ..Default::default() };
        run(&mut a, &it, &env, 1.8);
        let inside = Vec2::new(a.pos.x - b.center.x, a.pos.z - b.center.z);
        assert!(inside.x.abs() < b.aabb.half().x && inside.y.abs() < b.aabb.half().z, "actor should be inside the house, at {:?}", a.pos);
        assert!(a.pos.y > w.hm.height_at(a.pos.x, a.pos.z) + 0.1, "standing on the floor above terrain");
        // now walk sideways into a wall until blocked: the actor must stay inside
        let side = Vec2::new(-dir.y, dir.x);
        run(&mut a, &Intent { wish: side, ..Default::default() }, &env, 3.0);
        run(&mut a, &Intent { wish: -side, ..Default::default() }, &env, 6.0);
        let inside = Vec2::new(a.pos.x - b.center.x, a.pos.z - b.center.z);
        assert!(inside.x.abs() < b.aabb.half().x + 0.3 && inside.y.abs() < b.aabb.half().z + 0.3, "walls must contain the actor: {:?} aabb {:?}", a.pos, b.aabb);
    }

    #[test]
    fn every_door_in_the_world_can_be_walked_through_from_the_ground() {
        // some lots sit on a slope, and the door side can stand well above the terrain: without steps those houses are
        // closed to anyone who cannot jump a metre (every bot, and anyone not hunting for the one jumpable sill)
        let w = world();
        let (mut a, pieces) = setup();
        let env = Env::new(w, &pieces);
        let mut shut = vec![];
        let mut tested = 0;
        for (bi, b) in w.buildings.iter().enumerate() {
            if !matches!(b.kind, "house" | "cabin" | "shop" | "barn" | "lodge") {
                continue;
            }
            tested += 1;
            let out = Vec2::new(b.door_out.x - b.door_in.x, b.door_out.z - b.door_in.z).normalize();
            // start in front of the door, on the terrain, and walk at the point just inside the door
            let start = Vec2::new(b.door_out.x, b.door_out.z) + out * 3.0;
            a.pos = Vec3::new(start.x, env.terrain(start.x, start.y), start.y);
            a.vel = Vec3::ZERO;
            a.on_ground = true;
            a.mode = MoveMode::Ground;
            let target = Vec2::new(b.door_in.x, b.door_in.z);
            let mut ev = vec![];
            let mut reached = false;
            for _ in 0..(6 * 60) {
                let to = target - Vec2::new(a.pos.x, a.pos.z);
                if to.length() < 0.5 {
                    reached = true;
                    break;
                }
                step_ground(&mut a, &Intent { wish: to.normalize(), ..Default::default() }, &env, 1.0 / 60.0, &mut ev);
            }
            if !reached || a.pos.y < b.door_in.y - 0.4 {
                shut.push((bi, b.kind, (b.door_in.y - env.terrain(b.door_out.x, b.door_out.z) * 1.0).max(0.0)));
            }
        }
        assert!(tested > 40, "{tested} buildings tested");
        assert!(shut.is_empty(), "{} of {tested} doors cannot be walked through: {shut:?}", shut.len());
    }

    fn dir_vec(d: u8) -> Vec2 {
        [Vec2::X, Vec2::Y, Vec2::NEG_X, Vec2::NEG_Y][d as usize & 3]
    }

    /// Start, end and half length of the line up the middle of a flight, and the floor it starts from.
    fn flight_line(st: &crate::world::buildings::Stairs) -> (Vec2, Vec2, f32) {
        let r = dir_vec(st.dir);
        let c = Vec2::new((st.min.x + st.max.x) / 2.0, (st.min.z + st.max.z) / 2.0);
        let half = if r.x != 0.0 { (st.max.x - st.min.x) / 2.0 } else { (st.max.z - st.min.z) / 2.0 };
        (c - r * half, c + r * half, st.min.y)
    }

    #[test]
    fn climbs_and_descends_the_stairs_of_every_two_story_house() {
        let w = world();
        let (mut a, pieces) = setup();
        let env = Env::new(w, &pieces);
        let houses: Vec<_> = w.buildings.iter().filter(|b| b.stairs.is_some()).collect();
        assert!(houses.len() >= 3, "seed 1234 must contain two-story houses to test the stairs ({})", houses.len());
        for b in houses {
            let st = b.stairs.unwrap();
            let (foot, head, floor) = flight_line(&st);
            let up = dir_vec(st.dir);
            // the way on to the upper floor is off the open side of the flight
            let side = dir_vec(crate::world::buildings::open_side(st.dir));
            let storey = st.max.y - floor;
            a.pos = Vec3::new(foot.x - up.x * 0.7, floor, foot.y - up.y * 0.7);
            a.vel = Vec3::ZERO;
            a.on_ground = true;
            a.mode = MoveMode::Ground;
            // up to the head of the flight, without jumping
            run(&mut a, &Intent { wish: up, ..Default::default() }, &env, 3.0);
            assert!(a.pos.y > floor + storey - 0.3, "house {} should have climbed the stairs: y {} (floor {floor}, upper {})", b.id, a.pos.y, floor + storey);
            assert!(a.on_ground);
            // ... and off the side onto the upper floor, which is well away from the flight
            run(&mut a, &Intent { wish: side, ..Default::default() }, &env, 1.5);
            assert!((a.pos.y - (floor + storey)).abs() < 0.05, "house {}: standing on the upper floor, y {}", b.id, a.pos.y);
            let off = (Vec2::new(a.pos.x, a.pos.z) - head).dot(side);
            assert!(off > 1.0, "house {}: the opening at the head of the stairs must let you onto the floor (moved {off})", b.id);
            // and back down
            run(&mut a, &Intent { wish: -side, ..Default::default() }, &env, 2.5);
            run(&mut a, &Intent { wish: -up, ..Default::default() }, &env, 3.0);
            assert!(a.pos.y < floor + 0.1 && a.on_ground, "house {}: walked back down the stairs to the floor, y {} (floor {floor})", b.id, a.pos.y);
        }
    }

    #[test]
    fn stairs_are_made_of_real_steps() {
        let w = world();
        let mut seen = 0;
        for b in w.buildings.iter().filter(|b| b.stairs.is_some()) {
            let st = b.stairs.unwrap();
            let r = dir_vec(st.dir);
            let probe = crate::math::Aabb::new(st.min - Vec3::splat(0.01), st.max + Vec3::splat(0.01));
            let mut treads = vec![];
            w.statics.query(&probe, |_, c| {
                let bb = c.shape.aabb();
                if bb.min.x >= st.min.x - 0.01 && bb.max.x <= st.max.x + 0.01 && bb.min.z >= st.min.z - 0.01 && bb.max.z <= st.max.z + 0.01 && bb.min.y <= st.min.y + 0.01 && bb.max.y <= st.max.y + 0.01 {
                    assert!(matches!(c.shape, crate::world::collision::Shape::Box { .. }), "the stairs are made of boxes, not a ramp");
                    treads.push((Vec2::new(bb.max.x + bb.min.x, bb.max.z + bb.min.z).dot(r), bb.max.y));
                }
            });
            // every step is a block up to its tread, and each tread is a small rise above the one below it
            treads.sort_by(|a, b| a.0.total_cmp(&b.0));
            treads.dedup(); // a step that spans two grid cells is visited from both
            assert_eq!(treads.len(), 16, "house {}: {treads:?}", b.id);
            let mut last = st.min.y;
            for (_, top) in &treads {
                let rise = top - last;
                assert!(rise > 0.1 && rise < 0.25, "house {}: rise {rise}", b.id);
                last = *top;
            }
            assert!((last - st.max.y).abs() < 0.01, "the top step is level with the upper floor");
            seen += 1;
        }
        assert!(seen >= 3);
    }

    #[test]
    fn cannot_climb_a_cliff() {
        let w = world();
        let (mut a, pieces) = setup();
        let env = Env::new(w, &pieces);
        // find the steepest nearby terrain and try to run straight up it
        let mut best = (0.0f32, Vec2::ZERO);
        for j in 0..60 {
            for i in 0..60 {
                let p = Vec2::new(-300.0 + i as f32 * 10.0, -300.0 + j as f32 * 10.0);
                if w.hm.height_at(p.x, p.y) < 3.0 {
                    continue;
                }
                let s = w.hm.slope_at(p.x, p.y);
                if s > best.0 {
                    best = (s, p);
                }
            }
        }
        if best.0 < MAX_SLOPE {
            return; // this seed has no cliffs; nothing to test
        }
        let p = best.1;
        a.pos = Vec3::new(p.x, w.hm.height_at(p.x, p.y), p.y);
        let grad = w.hm.gradient_at(p.x, p.y).normalize();
        let start_y = a.pos.y;
        run(&mut a, &Intent { wish: grad, ..Default::default() }, &env, 1.5);
        assert!(a.pos.y - start_y < 3.0, "should not scale a cliff: climbed {}", a.pos.y - start_y);
    }

    #[test]
    fn swimming_in_deep_water_and_wading_back() {
        let w = world();
        let (mut a, pieces) = setup();
        let env = Env::new(w, &pieces);
        let l = &w.layout.lakes[0];
        // stand at the lake centre: should become a swimmer
        a.pos = Vec3::new(l.center.x, w.hm.height_at(l.center.x, l.center.y), l.center.y);
        a.on_ground = true;
        let mut ev = vec![];
        step_ground(&mut a, &Intent::default(), &env, 1.0 / 60.0, &mut ev);
        assert_eq!(a.mode, MoveMode::Swim);
        assert!((a.pos.y - (l.level - 0.95)).abs() < 0.1);
        // swim out of the lake to the shore
        let dir = Vec2::new(1.0, 0.0);
        for _ in 0..(60 * 14) {
            step_swim(&mut a, &Intent { wish: dir, ..Default::default() }, &env, 1.0 / 60.0, &mut ev);
            if a.mode == MoveMode::Ground {
                break;
            }
        }
        assert_eq!(a.mode, MoveMode::Ground, "should wade out at the shore (pos {:?})", a.pos);
    }

    #[test]
    fn skydive_then_glide_then_land_on_ground() {
        let w = world();
        let (mut a, pieces) = setup();
        let env = Env::new(w, &pieces);
        let p = open_field();
        a.mode = MoveMode::Freefall;
        a.on_ground = false;
        a.pos = Vec3::new(p.x, 260.0, p.z);
        a.pitch = -1.0; // dive
        let mut ev = vec![];
        let dt = 1.0 / 60.0;
        let mut glided_at = None;
        let mut t = 0.0;
        for _ in 0..(60 * 60) {
            t += dt;
            match a.mode {
                MoveMode::Freefall => {
                    step_freefall(&mut a, &Intent::default(), &env, dt, &mut ev);
                    if a.mode == MoveMode::Glide {
                        glided_at = Some(a.pos.y);
                    }
                }
                MoveMode::Glide => {
                    step_glide(&mut a, &Intent::default(), &env, dt, &mut ev);
                }
                _ => break,
            }
        }
        let g = glided_at.expect("glider must auto-deploy");
        let ground = w.hm.height_at(a.pos.x, a.pos.z);
        assert!(g - ground < GLIDER_AUTO_DEPLOY + 25.0 && g - ground > 20.0, "deployed {} m above ground", g - ground);
        assert_eq!(a.mode, MoveMode::Ground, "should have landed (t={t})");
        assert!(a.on_ground && (a.pos.y - ground).abs() < 0.3);
        assert!(ev.iter().any(|e| matches!(e, Event::GliderDeploy { .. })));
        assert!(ev.iter().any(|e| matches!(e, Event::Land { .. })));
    }

    #[test]
    fn fall_damage_applies_for_big_drops() {
        let w = world();
        let (mut a, pieces) = setup();
        let env = Env::new(w, &pieces);
        let p = open_field();
        let g = w.hm.height_at(p.x, p.z);
        a.pos = Vec3::new(p.x, g + 14.0, p.z);
        a.on_ground = false;
        a.peak_y = a.pos.y;
        let mut ev = vec![];
        let mut dmg = 0.0;
        for _ in 0..120 {
            let r = step_ground(&mut a, &Intent::default(), &env, 1.0 / 60.0, &mut ev);
            dmg += r.fall_damage;
        }
        assert!(dmg > 10.0 && dmg < 60.0, "fall damage {dmg}");
        // small hops do no damage
        a.pos.y = g + 1.5;
        a.on_ground = false;
        a.peak_y = a.pos.y;
        let mut dmg = 0.0;
        for _ in 0..120 {
            dmg += step_ground(&mut a, &Intent::default(), &env, 1.0 / 60.0, &mut ev).fall_damage;
        }
        assert_eq!(dmg, 0.0);
    }

    #[test]
    fn footsteps_are_emitted_at_stride_intervals() {
        let w = world();
        let (mut a, pieces) = setup();
        let env = Env::new(w, &pieces);
        let p = open_field();
        a.pos = Vec3::new(p.x, w.hm.height_at(p.x, p.z), p.z);
        let ev = run(&mut a, &Intent { wish: Vec2::new(0.0, 1.0), ..Default::default() }, &env, 3.0);
        let steps = ev.iter().filter(|e| matches!(e, Event::Footstep { .. })).count();
        // ~6.4 m/s * 3 s / 2.15 m stride = about 8
        assert!((5..=10).contains(&steps), "{steps} footsteps");
    }
}
