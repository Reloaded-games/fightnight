//! Utility items: grenades and the Boogie Bomb are thrown in an arc and bounce until they go off, the Launch Pad is set
//! down and throws whoever steps on it into the sky with their glider out.

use super::actor::*;
use super::combat::Blast;
use super::env::Env;
use super::events::*;
use super::items::*;
use super::*;
use crate::math::*;

/// How fast a throw leaves the hand (m/s), how hard it falls, and how big the thing is for collisions.
pub const THROW_SPEED: f32 = 22.0;
pub const THROW_GRAVITY: f32 = 20.0;
/// Upward speed added to every throw, so one aimed at the horizon still flies in an arc (about 17 m on level ground).
const THROW_LOFT: f32 = 5.5;
const THROW_RADIUS: f32 = 0.09;
/// Share of the speed into a surface that comes back out of it, and of the speed along it that is kept.
const RESTITUTION: f32 = 0.35;
const FRICTION: f32 = 0.7;
/// Seconds between throws.
pub const THROW_COOLDOWN: f32 = 0.6;
/// A frag grenade goes off this long after the throw; an impulse grenade or a Boogie Bomb on its first bounce once armed (or
/// at the latest after its fuse).
pub const FRAG_FUSE: f32 = 2.5;
pub const IMPULSE_ARM: f32 = 0.2;
pub const IMPULSE_FUSE: f32 = 3.0;
/// How far a Boogie Bomb reaches and how long those it catches have to dance (no moving, shooting, building or healing).
pub const BOOGIE_RADIUS: f32 = 6.0;
pub const BOOGIE_TIME: f32 = 5.0;
/// Upward speed a Launch Pad gives, how close to its middle a player has to step, and the pad's size.
pub const LAUNCH_SPEED: f32 = 34.0;
pub const PAD_RADIUS: f32 = 1.0;
/// How far an impulse reaches, and how hard it throws someone at its centre (sideways, upwards).
pub const IMPULSE_RADIUS: f32 = 6.5;
const IMPULSE_PUSH: f32 = 22.0;
const IMPULSE_LIFT: f32 = 16.0;

/// A frag grenade: the damage of a rocket, a smaller blast, and it wrecks buildings.
pub const FRAG_BLAST: Blast = Blast { damage: 105.0, radius: 5.0, structure: 375.0, push: 6.0, lift: 3.5, source: "Frag Grenade" };

/// Something thrown and still bouncing about.
#[derive(Clone, Debug)]
pub struct Thrown {
    pub id: u32,
    pub kind: UtilityKind,
    pub pos: Vec3,
    pub vel: Vec3,
    pub owner: usize,
    pub age: f32,
    /// It has come to rest on the ground.
    pub resting: bool,
    /// Whether it has hit anything yet (an impulse grenade goes off on its first bounce).
    pub bounced: bool,
    pub spin: f32,
}

/// Something set down that stays: a Launch Pad.
#[derive(Clone, Debug)]
pub struct Trap {
    pub id: u32,
    pub kind: UtilityKind,
    pub pos: Vec3,
    pub yaw: f32,
    pub owner: usize,
    /// 1 when it has just thrown someone up, fading to 0 (the plate springs up).
    pub bounce: f32,
}

/// Where a throw starts and how fast it goes, for someone at `pos` (feet) throwing along `dir`. A little loft keeps a throw
/// at the horizon arcing, and part of the thrower's own speed carries over.
pub fn throw_start(pos: Vec3, yaw: f32, dir: Vec3, carried: Vec3) -> (Vec3, Vec3) {
    let hand = pos + Vec3::Y * 1.5 + yaw_right(yaw) * 0.25;
    let vel = dir * THROW_SPEED + Vec3::Y * THROW_LOFT + Vec3::new(carried.x, 0.0, carried.z) * 0.5;
    (hand + dir * 0.35, vel)
}

/// Move a thrown object for `dt` seconds: gravity, then a bounce off whatever it meets (the ground, a wall, a build).
/// Returns whether it hit something on the way.
pub fn step_thrown(env: &Env, pos: &mut Vec3, vel: &mut Vec3, resting: &mut bool, dt: f32) -> bool {
    if *resting {
        return false;
    }
    vel.y -= THROW_GRAVITY * dt;
    let step = *vel * dt;
    let len = step.length();
    if len < 1e-6 {
        return false;
    }
    let dir = step / len;
    let mut hit = false;
    match env.raycast(*pos, dir, len + THROW_RADIUS, true) {
        Some(h) => {
            hit = true;
            *pos += dir * (h.t - THROW_RADIUS).max(0.0);
            let n = if h.normal.length_squared() > 0.5 { h.normal.normalize() } else { Vec3::Y };
            let vn = n * vel.dot(n);
            let vt = *vel - vn;
            *vel = vt * FRICTION - vn * RESTITUTION;
            // settled on something you could stand on
            if n.y > 0.6 && vel.length() < 1.6 {
                *vel = Vec3::ZERO;
                *resting = true;
            }
        }
        None => *pos += step,
    }
    // never sink into the ground (a fast fall can step through the terrain between two samples)
    let ground = env.terrain(pos.x, pos.z) + THROW_RADIUS;
    if pos.y < ground {
        pos.y = ground;
        if vel.y < 0.0 {
            vel.y = -vel.y * RESTITUTION;
            vel.x *= FRICTION;
            vel.z *= FRICTION;
            hit = true;
            if vel.length() < 1.6 {
                *vel = Vec3::ZERO;
                *resting = true;
            }
        }
    }
    hit
}

/// The arc a throw would follow from `pos` with `vel`, until it first hits something (for the aiming preview).
pub fn predict_arc(env: &Env, mut pos: Vec3, mut vel: Vec3, max_t: f32) -> Vec<Vec3> {
    let mut pts = vec![pos];
    let mut resting = false;
    let dt = 1.0 / 30.0;
    let mut t = 0.0;
    while t < max_t {
        let hit = step_thrown(env, &mut pos, &mut vel, &mut resting, dt);
        pts.push(pos);
        if hit {
            break;
        }
        t += dt;
    }
    pts
}

/// The aim (pitch) that lands a throw by `a`, facing `yaw`, closest to `target`, found by trying a fan of arcs; `None` when none
/// comes down within a few metres of it (out of reach, or a wall in the way). For the bots.
pub fn lob_pitch(env: &Env, a: &Actor, yaw: f32, target: Vec3) -> Option<f32> {
    let mut best: Option<(f32, f32)> = None;
    for k in 0..=26 {
        let pitch = -0.4 + k as f32 * 0.05;
        let (o, v) = throw_start(a.pos, yaw, look_dir(yaw, pitch), Vec3::ZERO);
        let Some(end) = predict_arc(env, o, v, 3.0).last().copied() else { continue };
        let miss = Vec2::new(end.x - target.x, end.z - target.z).length() + (end.y - target.y).abs() * 0.5;
        if best.is_none_or(|b| miss < b.1) {
            best = Some((pitch, miss));
        }
    }
    best.filter(|b| b.1 < 3.0).map(|b| b.0)
}

/// Whoever steps on a Launch Pad flies up with the glider out. Shared by the host and a guest's prediction of its own player,
/// so both see the launch at the same moment. Returns the pad that fired.
pub fn launch_pad_touch(a: &mut Actor, traps: &[Trap]) -> Option<u32> {
    if !a.alive || a.mode != MoveMode::Ground || a.vel.y > 1.0 {
        return None;
    }
    let pad = traps.iter().find(|t| t.kind == UtilityKind::LaunchPad && Vec2::new(a.pos.x - t.pos.x, a.pos.z - t.pos.z).length() < PAD_RADIUS && (a.pos.y - t.pos.y).abs() < 0.5)?;
    a.mode = MoveMode::Glide;
    a.glide_deployed = true;
    a.on_ground = false;
    a.crouching = false;
    a.sprinting = false;
    a.ads = false;
    a.emoting = false;
    a.vel.y = LAUNCH_SPEED;
    a.pos.y += 0.05;
    a.peak_y = a.pos.y;
    Some(pad.id)
}

/// While a Boogie Bomb has someone dancing they cannot move, shoot, build or use anything: they can only look around. Shared by
/// the host and a guest's prediction of its own player. Returns the intent to use instead of theirs, if they are dancing.
pub fn dancing_intent(a: &mut Actor, it: &Intent, dt: f32) -> Option<Intent> {
    if a.boogie <= 0.0 {
        return None;
    }
    a.boogie = (a.boogie - dt).max(0.0);
    a.emoting = a.boogie > 0.0 && a.alive && a.mode == MoveMode::Ground;
    Some(Intent { yaw: it.yaw, pitch: it.pitch, ..Default::default() })
}

impl Game {
    /// Throw the utility item in hand, or set it down. Returns whether anything happened.
    pub fn use_utility(&mut self, i: usize, kind: UtilityKind) -> bool {
        let slot = self.actors[i].inv.selected;
        let done = if kind.def().thrown { self.throw_utility(i, kind) } else { self.place_pad(i) };
        if done {
            self.actors[i].inv.use_one(slot);
            self.actors[i].fire_cd = THROW_COOLDOWN;
        }
        done
    }

    fn throw_utility(&mut self, i: usize, kind: UtilityKind) -> bool {
        let (_, dir) = self.shot_ray(i);
        let a = &self.actors[i];
        let (mut origin, vel) = throw_start(a.pos, a.yaw, dir, a.vel);
        // pressed against a wall the hand is already through it: let go from the chest instead
        let chest = a.pos + Vec3::Y * 1.3;
        if !self.env().line_clear(chest, origin) {
            origin = chest;
        }
        let id = self.new_id();
        let spin = self.rng.range(0.0, std::f32::consts::TAU);
        self.thrown.push(Thrown { id, kind, pos: origin, vel, owner: i, age: 0.0, resting: false, bounced: false, spin });
        let a = &mut self.actors[i];
        a.anim.swing = 1.0;
        a.emoting = false;
        self.events.push(Event::Swing { actor: i });
        true
    }

    /// Set a Launch Pad down on the ground (or a floor) just ahead. Returns whether there was room for it.
    fn place_pad(&mut self, i: usize) -> bool {
        let a = &self.actors[i];
        let (yaw, human) = (a.yaw, a.human);
        let f = yaw_forward(a.yaw);
        let at = Vec3::new(a.pos.x + f.x * 1.6, a.pos.y, a.pos.z + f.z * 1.6);
        let env = self.env();
        let g = env.ground(at.x, at.z, a.pos.y + 1.0, 1.2);
        let level = [(-0.7, -0.7), (0.7, -0.7), (-0.7, 0.7), (0.7, 0.7)].iter().all(|(dx, dz)| (env.ground(at.x + dx, at.z + dz, g.y + 0.6, 0.6).y - g.y).abs() < 0.45);
        let dry = env.water_level(at.x, at.z).is_none_or(|l| l < g.y + 0.1);
        let clear = env.push_out(Vec3::new(at.x, g.y, at.z), 0.8, 1.0, 0.3).length_squared() < 1e-4;
        let room = (g.y - a.pos.y).abs() < 1.2 && level && dry && clear && !self.traps.iter().any(|t| t.pos.distance(Vec3::new(at.x, g.y, at.z)) < 2.4);
        if !room {
            if human {
                self.toast_to(i, "No room for a Launch Pad here", 2.0, 2);
            }
            return false;
        }
        let id = self.new_id();
        let pos = Vec3::new(at.x, g.y, at.z);
        self.traps.push(Trap { id, kind: UtilityKind::LaunchPad, pos, yaw, owner: i, bounce: 0.0 });
        self.events.push(Event::TrapPlaced { actor: i, pos });
        true
    }

    /// An impulse: no damage, but everyone close (the thrower too) is thrown away from it, and lands without fall damage.
    pub fn impulse(&mut self, pos: Vec3, owner: usize) {
        self.events.push(Event::Impulse { pos });
        self.events.push(Event::Noise { pos, radius: 60.0, source: owner });
        let env = Env::new(&self.world, &self.pieces.grid);
        let mut pushes = vec![];
        for (j, a) in self.actors.iter().enumerate() {
            if !a.alive || !matches!(a.mode, MoveMode::Ground | MoveMode::Glide | MoveMode::Freefall) {
                continue;
            }
            let chest = a.chest();
            let d = chest.distance(pos);
            if d >= IMPULSE_RADIUS || !env.line_clear(pos, chest) {
                continue;
            }
            let k = (1.0 - d / IMPULSE_RADIUS).clamp(0.0, 1.0);
            let away = Vec2::new(chest.x - pos.x, chest.z - pos.z).normalize_or_zero();
            pushes.push((j, Vec3::new(away.x, 0.0, away.y) * IMPULSE_PUSH * (0.35 + 0.65 * k) + Vec3::Y * IMPULSE_LIFT * (0.45 + 0.55 * k)));
        }
        for (j, v) in pushes {
            let a = &mut self.actors[j];
            a.vel += v;
            a.emoting = false;
            if a.mode == MoveMode::Ground {
                a.on_ground = false;
                a.launched = true;
            }
        }
    }

    /// A Boogie Bomb goes off: everyone close (the thrower too, if careless) drops what they are doing and dances.
    pub fn boogie(&mut self, pos: Vec3, owner: usize) {
        self.events.push(Event::Boogie { pos });
        self.events.push(Event::Noise { pos, radius: 40.0, source: owner });
        let env = Env::new(&self.world, &self.pieces.grid);
        let caught: Vec<usize> = self
            .actors
            .iter()
            .enumerate()
            .filter(|(j, a)| a.alive && a.mode == MoveMode::Ground && a.chest().distance(pos) < BOOGIE_RADIUS && env.line_clear(pos, a.chest()) && !self.vehicles.iter().any(|v| v.driver == Some(*j)))
            .map(|(j, _)| j)
            .collect();
        for j in caught {
            let a = &mut self.actors[j];
            a.boogie = BOOGIE_TIME;
            a.emoting = true;
            a.anim.emote_clock = 0.0;
            a.action = Action::None;
            a.build_mode = false;
            a.ads = false;
        }
    }
}

/// Grenades and Boogie Bombs fly and go off; Launch Pads settle after throwing someone up.
pub fn update(g: &mut Game, dt: f32) {
    let mut i = 0;
    while i < g.thrown.len() {
        let env = Env::new(&g.world, &g.pieces.grid);
        let t = &mut g.thrown[i];
        t.age += dt;
        t.spin += dt * if t.resting { 0.0 } else { 9.0 };
        let hit = step_thrown(&env, &mut t.pos, &mut t.vel, &mut t.resting, dt);
        t.bounced |= hit;
        let go = match t.kind {
            UtilityKind::FragGrenade => t.age >= FRAG_FUSE,
            UtilityKind::ImpulseGrenade | UtilityKind::BoogieBomb => (hit && t.age >= IMPULSE_ARM) || t.age >= IMPULSE_FUSE,
            UtilityKind::LaunchPad => true,
        };
        let lost = t.pos.y < -60.0;
        if !go && !lost {
            i += 1;
            continue;
        }
        let t = g.thrown.remove(i);
        if lost {
            continue;
        }
        match t.kind {
            UtilityKind::FragGrenade => g.blast(t.pos + Vec3::Y * 0.1, t.owner, &FRAG_BLAST),
            UtilityKind::ImpulseGrenade => g.impulse(t.pos + Vec3::Y * 0.1, t.owner),
            UtilityKind::BoogieBomb => g.boogie(t.pos + Vec3::Y * 0.1, t.owner),
            UtilityKind::LaunchPad => {}
        }
    }
    for t in &mut g.traps {
        t.bounce = (t.bounce - dt * 3.0).max(0.0);
    }
    // a pad set down on a build goes when the floor under it is shot away
    let env = Env::new(&g.world, &g.pieces.grid);
    g.traps.retain(|t| env.ground(t.pos.x, t.pos.z, t.pos.y + 0.05, 0.0).y > t.pos.y - 0.05);
}

/// The part of [`update`] a guest's copy of the match runs between snapshots: thrown things keep flying (the host decides when
/// they go off) and pads settle after a launch.
pub fn coast(g: &mut Game, dt: f32) {
    let env = Env::new(&g.world, &g.pieces.grid);
    for t in &mut g.thrown {
        t.age += dt;
        t.spin += dt * if t.resting { 0.0 } else { 9.0 };
        step_thrown(&env, &mut t.pos, &mut t.vel, &mut t.resting, dt);
    }
    for t in &mut g.traps {
        t.bounce = (t.bounce - dt * 3.0).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{free_spot, game};
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn idle() -> PlayerInput {
        PlayerInput::default()
    }

    fn click() -> PlayerInput {
        PlayerInput { fire: true, fire_pressed: true, ..Default::default() }
    }

    /// Stand actor `i` still on the ground at (x, z), facing -Z.
    fn stand(g: &mut Game, i: usize, x: f32, z: f32) {
        let y = g.world.hm.height_at(x, z);
        let a = &mut g.actors[i];
        a.pos = Vec3::new(x, y, z);
        a.vel = Vec3::ZERO;
        a.mode = MoveMode::Ground;
        a.on_ground = true;
        a.peak_y = y;
        a.eye_smooth = y;
        a.yaw = 0.0;
        a.pitch = 0.0;
        a.body_yaw = 0.0;
        a.brain = None;
        a.hp = 100.0;
        a.shield = 0.0;
    }

    /// The player alone on open ground (the bots frozen well away), holding `n` of `kind`, ready to use it.
    fn holding(kind: UtilityKind, n: u32) -> (Game, Vec3) {
        let mut g = game(3, true);
        let p = free_spot(&g);
        stand(&mut g, PLAYER, p.x, p.z);
        for k in 1..g.actors.len() {
            stand(&mut g, k, p.x + 40.0 + k as f32 * 3.0, p.z + 40.0);
        }
        g.actors[PLAYER].inv.add_utility(kind, n);
        let slot = g.actors[PLAYER].inv.slots.iter().position(|s| matches!(s, Some(Item::Utility { kind: k, .. }) if *k == kind)).unwrap();
        g.select_slot(PLAYER, slot);
        for _ in 0..30 {
            g.update(DT, &idle());
        }
        (g, p)
    }

    fn count_of(g: &Game, kind: UtilityKind) -> u32 {
        g.actors[PLAYER].inv.slots.iter().flatten().map(|s| match s {
            Item::Utility { kind: k, count } if *k == kind => *count,
            _ => 0,
        }).sum()
    }

    /// Something thrown, about to touch the ground at (x, z) and `age` seconds old.
    fn about_to_land(g: &mut Game, kind: UtilityKind, x: f32, z: f32, age: f32, owner: usize) {
        let y = g.world.hm.height_at(x, z);
        let id = g.new_id();
        g.thrown.push(Thrown { id, kind, pos: Vec3::new(x, y + 0.12, z), vel: Vec3::new(0.0, -6.0, 0.0), owner, age, resting: false, bounced: false, spin: 0.0 });
    }

    #[test]
    fn a_click_throws_a_grenade_in_an_arc_and_it_goes_off_after_its_fuse() {
        let (mut g, p) = holding(UtilityKind::FragGrenade, 3);
        g.update(DT, &click());
        assert_eq!(g.thrown.len(), 1, "the click throws one");
        assert_eq!(count_of(&g, UtilityKind::FragGrenade), 2, "and it leaves the inventory");
        assert!(g.events.iter().any(|e| matches!(e, Event::Swing { actor: PLAYER })), "the throwing arm swings");
        // a second click straight away does nothing: there is a moment between throws
        g.update(DT, &click());
        assert_eq!(g.thrown.len(), 1);
        let (mut top, mut t, mut boom) = (f32::MIN, DT * 2.0, None);
        // (where it left the hand, one tick back along its path)
        let start_y = g.thrown[0].pos.y - g.thrown[0].vel.y * DT * 2.0;
        while t < FRAG_FUSE + 0.5 {
            g.update(DT, &idle());
            t += DT;
            if let Some(th) = g.thrown.first() {
                top = top.max(th.pos.y);
            }
            if let Some(Event::Explosion { pos, .. }) = g.events.iter().find(|e| matches!(e, Event::Explosion { .. })) {
                boom = Some((t, *pos));
                break;
            }
        }
        let (when, at) = boom.expect("the grenade went off");
        assert!((when - FRAG_FUSE).abs() < 0.1, "after its fuse: {when:.2} s");
        assert!(top > start_y + 0.4, "it flew in an arc: rose {:.2} m", top - start_y);
        assert!(p.z - at.z > 8.0, "it went off well ahead of the thrower: {:.1} m", p.z - at.z);
        assert!((at.y - g.world.hm.height_at(at.x, at.z)).abs() < 0.6, "on the ground");
        assert!(g.thrown.is_empty());
        assert_eq!(g.actors[PLAYER].hp, 100.0, "far enough away");
    }

    #[test]
    fn a_frag_grenade_hurts_everyone_close_including_its_thrower_and_wrecks_builds() {
        let (mut g, p) = holding(UtilityKind::FragGrenade, 1);
        let at = Vec2::new(p.x, p.z - 14.0);
        stand(&mut g, 1, at.x + 1.5, at.y);
        let cell = crate::game::pieces::cell_of(Vec3::new(at.x - 2.0, 0.0, at.y));
        let key = PieceKey { kind: PieceKind::Wall, x: cell.0, z: cell.1, level: 0, dir: 1 };
        let base = crate::game::pieces::structure_base(&g.pieces, &g.env(), cell.0, cell.1);
        let wall = g.pieces.insert(key, Mat::Wood, base, PLAYER);
        about_to_land(&mut g, UtilityKind::FragGrenade, at.x, at.y, FRAG_FUSE - 0.02, PLAYER);
        for _ in 0..4 {
            g.update(DT, &idle());
        }
        assert!(g.thrown.is_empty(), "it went off");
        assert!(g.actors[1].hp < 50.0, "the bot beside it is badly hurt: {}", g.actors[1].hp);
        assert!(g.pieces.get(wall).is_none(), "the wall beside it is wrecked");
        assert!(g.actors[PLAYER].hp == 100.0, "out of reach");
        // one at the thrower's own feet hurts them as well
        about_to_land(&mut g, UtilityKind::FragGrenade, p.x + 0.6, p.z, FRAG_FUSE - 0.02, PLAYER);
        for _ in 0..4 {
            g.update(DT, &idle());
        }
        assert!(g.actors[PLAYER].hp < 60.0, "explosives hurt their owner: {}", g.actors[PLAYER].hp);
    }

    #[test]
    fn an_impulse_grenade_throws_people_away_without_hurting_them() {
        let (mut g, p) = holding(UtilityKind::ImpulseGrenade, 1);
        let at = Vec2::new(p.x, p.z - 10.0);
        stand(&mut g, 1, at.x + 2.0, at.y);
        stand(&mut g, 2, at.x - 2.5, at.y + 0.5);
        g.actors[1].shield = 50.0;
        about_to_land(&mut g, UtilityKind::ImpulseGrenade, at.x, at.y, 0.5, PLAYER);
        g.update(DT, &idle());
        assert!(g.events.iter().any(|e| matches!(e, Event::Impulse { .. })), "it went off on landing");
        assert!(g.thrown.is_empty());
        for (k, side) in [(1, 1.0), (2, -1.0)] {
            let a = &g.actors[k];
            assert!(a.vel.x * side > 8.0 && a.vel.y > 6.0, "actor {k} thrown away and up: {:?}", a.vel);
            assert!(a.launched, "actor {k} flies safe from fall damage");
        }
        let from = [g.actors[1].pos, g.actors[2].pos];
        for _ in 0..(60 * 3) {
            g.update(DT, &idle());
        }
        for (k, was) in [(1, from[0]), (2, from[1])] {
            let a = &g.actors[k];
            assert!(a.on_ground && !a.launched, "actor {k} landed");
            assert!(Vec2::new(a.pos.x - was.x, a.pos.z - was.z).length() > 4.0, "actor {k} went some way");
            assert_eq!(a.hp, 100.0, "actor {k} is unhurt");
        }
        assert_eq!(g.actors[1].shield, 50.0);
    }

    #[test]
    fn a_launched_body_takes_no_fall_damage_from_any_height() {
        let (mut g, p) = holding(UtilityKind::ImpulseGrenade, 1);
        for launched in [false, true] {
            stand(&mut g, 1, p.x + 6.0, p.z);
            let a = &mut g.actors[1];
            a.pos.y += 25.0;
            a.peak_y = a.pos.y;
            a.on_ground = false;
            a.launched = launched;
            for _ in 0..(60 * 3) {
                g.update(DT, &idle());
            }
            let a = &g.actors[1];
            assert!(a.on_ground);
            if launched {
                assert_eq!(a.hp, 100.0, "thrown by an impulse or a pad: no fall damage");
            } else {
                assert!(a.hp < 90.0, "a plain 25 m drop hurts: {}", a.hp);
            }
        }
    }

    #[test]
    fn a_boogie_bomb_makes_everyone_close_dance_for_five_seconds() {
        let (mut g, p) = holding(UtilityKind::BoogieBomb, 1);
        // the player also carries a rifle to try to shoot with
        g.actors[PLAYER].inv.add_weapon(WeaponKind::AssaultRifle, Rarity::Rare, 30);
        g.actors[PLAYER].inv.ammo[AmmoKind::Medium.index()] = 90;
        let rifle = g.actors[PLAYER].inv.slots.iter().position(|s| matches!(s, Some(Item::Weapon { .. }))).unwrap();
        g.select_slot(PLAYER, rifle);
        for _ in 0..40 {
            g.update(DT, &idle());
        }
        stand(&mut g, 1, p.x + 2.5, p.z - 1.0);
        stand(&mut g, 2, p.x + 20.0, p.z);
        about_to_land(&mut g, UtilityKind::BoogieBomb, p.x + 1.2, p.z - 1.5, 0.5, 1);
        g.update(DT, &idle());
        assert!(g.events.iter().any(|e| matches!(e, Event::Boogie { .. })));
        for k in [PLAYER, 1] {
            assert!(g.actors[k].boogie > BOOGIE_TIME - 0.1 && g.actors[k].emoting, "actor {k} dances");
        }
        assert_eq!(g.actors[2].boogie, 0.0, "too far away to be caught");
        // while dancing: no walking, no shooting
        let from = g.actors[PLAYER].pos;
        let (mut shots, mut t) = (0, 0.0);
        let push = PlayerInput { move_axis: Vec2::new(0.0, 1.0), fire: true, fire_pressed: true, ..Default::default() };
        while t < BOOGIE_TIME - 0.3 {
            g.update(DT, &push);
            shots += g.events.iter().filter(|e| matches!(e, Event::Shot { actor: PLAYER, .. })).count();
            t += DT;
        }
        assert_eq!(shots, 0, "no shooting while dancing");
        assert!(g.actors[PLAYER].pos.distance(from) < 0.3, "no walking while dancing");
        assert!(g.actors[PLAYER].emoting);
        // and then it wears off
        for _ in 0..30 {
            g.update(DT, &idle());
        }
        assert_eq!(g.actors[PLAYER].boogie, 0.0);
        let from = g.actors[PLAYER].pos;
        for _ in 0..60 {
            g.update(DT, &push);
        }
        assert!(!g.actors[PLAYER].emoting && g.actors[PLAYER].pos.distance(from) > 2.0, "free to move again");
    }

    #[test]
    fn a_launch_pad_is_set_down_ahead_and_throws_whoever_steps_on_it_into_a_safe_glide() {
        let (mut g, p) = holding(UtilityKind::LaunchPad, 1);
        g.update(DT, &click());
        assert_eq!(g.traps.len(), 1, "the pad is down");
        assert!(g.events.iter().any(|e| matches!(e, Event::TrapPlaced { actor: PLAYER, .. })));
        assert_eq!(count_of(&g, UtilityKind::LaunchPad), 0, "used up");
        let pad = g.traps[0].clone();
        assert!((p.z - pad.pos.z - 1.6).abs() < 0.2 && (pad.pos.y - g.world.hm.height_at(pad.pos.x, pad.pos.z)).abs() < 0.3, "just ahead, on the ground: {:?}", pad.pos);
        // walk onto it
        let walk = PlayerInput { move_axis: Vec2::new(0.0, 1.0), ..Default::default() };
        let mut launched = false;
        for _ in 0..60 {
            g.update(DT, &walk);
            if g.events.iter().any(|e| matches!(e, Event::Launch { actor: PLAYER, .. })) {
                launched = true;
                break;
            }
        }
        assert!(launched, "stepping on the pad launches");
        assert_eq!(g.actors[PLAYER].mode, MoveMode::Glide);
        assert!(g.traps[0].bounce > 0.5, "the plate springs");
        let mut top = 0.0f32;
        let mut landed = false;
        for _ in 0..(60 * 25) {
            g.update(DT, &idle());
            top = top.max(g.actors[PLAYER].pos.y - pad.pos.y);
            if g.actors[PLAYER].mode == MoveMode::Ground && g.actors[PLAYER].on_ground {
                landed = true;
                break;
            }
        }
        assert!(top > 20.0, "high into the air: {top:.1} m");
        assert!(landed, "and back down");
        assert_eq!(g.actors[PLAYER].hp, 100.0, "without fall damage");
        // it stays for the next one: a bot that stands on it flies too
        stand(&mut g, 1, pad.pos.x, pad.pos.z);
        g.update(DT, &idle());
        assert_eq!(g.actors[1].mode, MoveMode::Glide, "the pad works for everyone");
        assert_eq!(g.traps.len(), 1);
    }

    #[test]
    fn a_launch_pad_on_a_floor_goes_when_the_floor_does() {
        use crate::game::pieces::{cell_of, structure_base, LEVEL_H, TILE};
        let (mut g, p) = holding(UtilityKind::LaunchPad, 1);
        // a floor one storey up (on a wall), with the player standing on it
        let cell = cell_of(p);
        let base = structure_base(&g.pieces, &g.env(), cell.0, cell.1);
        g.pieces.insert(PieceKey { kind: PieceKind::Wall, x: cell.0, z: cell.1, level: 0, dir: 0 }, Mat::Wood, base, PLAYER);
        let floor = g.pieces.insert(PieceKey { kind: PieceKind::Floor, x: cell.0, z: cell.1, level: 1, dir: 0 }, Mat::Wood, base, PLAYER);
        let top = base + LEVEL_H;
        let (cx, cz) = ((cell.0 as f32 + 0.5) * TILE, (cell.1 as f32 + 0.5) * TILE);
        stand(&mut g, PLAYER, cx, cz + 0.9);
        g.actors[PLAYER].pos.y = top;
        g.actors[PLAYER].peak_y = top;
        g.actors[PLAYER].eye_smooth = top;
        g.update(DT, &idle());
        g.update(DT, &click());
        assert_eq!(g.traps.len(), 1, "set down on the floor");
        assert!((g.traps[0].pos.y - top).abs() < 0.05);
        g.damage_piece(floor, 10_000.0, Some(1));
        g.update(DT, &idle());
        assert!(g.pieces.get(floor).is_none());
        assert!(g.traps.is_empty(), "the pad went with the floor");
    }

    #[test]
    fn a_launch_pad_needs_level_open_ground() {
        let (mut g, p) = holding(UtilityKind::LaunchPad, 1);
        // a wall right in front
        let cell = crate::game::pieces::cell_of(p);
        let base = crate::game::pieces::structure_base(&g.pieces, &g.env(), cell.0, cell.1);
        g.pieces.insert(PieceKey { kind: PieceKind::Wall, x: cell.0, z: cell.1, level: 0, dir: 0 }, Mat::Wood, base, PLAYER);
        let wall_z = cell.1 as f32 * crate::game::pieces::TILE;
        stand(&mut g, PLAYER, p.x, wall_z + 1.0);
        g.update(DT, &click());
        assert!(g.traps.is_empty(), "no pad inside a wall");
        assert_eq!(count_of(&g, UtilityKind::LaunchPad), 1, "kept for later");
        assert!(g.events.iter().any(|e| matches!(e, Event::Toast { actor: Some(PLAYER), text, .. } if text.contains("No room"))), "and the player is told why");
    }

    #[test]
    fn the_throw_preview_follows_the_same_path_as_the_throw() {
        let (mut g, _) = holding(UtilityKind::FragGrenade, 1);
        let a = &g.actors[PLAYER];
        let (_, dir) = g.shot_ray(PLAYER);
        let (o, v) = throw_start(a.pos, a.yaw, dir, a.vel);
        let arc = predict_arc(&g.env(), o, v, 3.0);
        g.update(DT, &click());
        let mut first_hit = None;
        for _ in 0..180 {
            let before = g.thrown[0].bounced;
            g.update(DT, &idle());
            if !before && g.thrown.first().is_some_and(|t| t.bounced) {
                first_hit = Some(g.thrown[0].pos);
                break;
            }
        }
        let hit = first_hit.expect("it hit the ground");
        let end = *arc.last().unwrap();
        assert!(hit.distance(end) < 1.5, "the preview ends where the grenade lands: {end:?} vs {hit:?}");
    }
}
