//! Bot brains. Every bot is an `Actor` driven by an `Intent` produced here:
//! choosing a landing spot and riding the bus, skydiving, looting, harvesting,
//! outrunning the storm, healing, and fighting (aiming with human-like error,
//! strafing, bursting, switching weapons, building cover and ramps).
//!
//! The brain only ever *reads* the game and returns an `Intent`; the same
//! simulation rules that apply to the human apply to bots.

use super::actor::*;
use super::events::Event;
use super::intent::Intent;
use super::items::*;
use super::loot::INTERACT_RANGE;
use super::{Difficulty, Game, PickupKind};
use crate::math::*;
use crate::rng::Rng;
use crate::world::collision::Tag;
use crate::world::gen::PoiKind;
use crate::world::props::HarvestKind;

/// How far bots can see.
const SIGHT: f32 = 210.0;
/// A bot without usable ammo still goes for an enemy this close, pickaxe in hand.
const BRAWL_RANGE: f32 = 32.0;
/// Seconds between "slow" decisions (perception, goal choice).
const THINK: f32 = 0.2;
/// Free inventory slots bots try to keep for healing items.
const MAX_WEAPONS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct Skill {
    /// Seconds between spotting an enemy and the first shot.
    pub react: f32,
    /// Typical aim error at the target, in metres.
    pub aim_m: f32,
    /// Maximum aim turn rate (rad/s).
    pub turn: f32,
    /// 0..1: how eagerly the bot picks fights.
    pub aggression: f32,
    /// 0..1: how willing and able the bot is to build.
    pub builder: f32,
    /// 0..1: how reliably it notices enemies.
    pub awareness: f32,
}

impl Skill {
    fn new(d: Difficulty, rng: &mut Rng) -> Skill {
        let (react, aim_m, turn, builder, aware) = match d {
            Difficulty::Easy => (0.95, 1.7, 4.0, 0.12, 0.5),
            Difficulty::Normal => (0.6, 1.05, 6.0, 0.4, 0.75),
            Difficulty::Hard => (0.3, 0.55, 9.5, 0.85, 1.0),
        };
        let mut j = |v: f32| v * rng.range(0.8, 1.25);
        Skill { react: j(react), aim_m: j(aim_m), turn: j(turn), aggression: rng.range(0.25, 1.0), builder: (builder * rng.range(0.6, 1.4)).clamp(0.0, 1.0), awareness: aware }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LootRef {
    Pickup(u32),
    Chest(u32),
}

impl LootRef {
    fn id(self) -> u32 {
        match self {
            LootRef::Pickup(i) | LootRef::Chest(i) => i,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Goal {
    Idle,
    Loot { what: LootRef, pos: Vec3 },
    Harvest { idx: usize },
    Travel { to: Vec2, sprint: bool },
    Fight,
    Heal,
}

/// A short scripted building sequence.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Job {
    None,
    /// Put a wall between us and the enemy.
    Cover { stage: u8, t: f32 },
    /// Run up ramps toward an enemy on higher ground.
    Ramp { t: f32, placed: u8 },
}

#[derive(Clone, Debug)]
pub struct Brain {
    pub last_attacker: Option<usize>,
    rng: Rng,
    skill: Skill,
    // landing
    land: Option<Vec2>,
    jump_dist: f32,
    // clocks
    think_t: f32,
    since_think: f32,
    ev_seen: usize,
    goal_t: f32,
    hurt_t: f32,
    // perception
    enemy: Option<usize>,
    enemy_pos: Vec3,
    seen_t: f32,
    visible: bool,
    react_t: f32,
    // goals
    goal: Goal,
    blacklist: Vec<(u32, f32)>,
    investigate: Option<(Vec3, f32)>,
    roam_cd: f32,
    zone_off: Vec2,
    zone_phase: usize,
    zone_lead: f32,
    // pathing
    path: Vec<Vec2>,
    path_i: usize,
    path_goal: Vec2,
    repath_t: f32,
    // stuck handling
    stuck_ref: Vec2,
    stuck_clock: f32,
    stuck_count: u32,
    unstick_t: f32,
    unstick_sign: f32,
    giveups: u32,
    // combat
    strafe_sign: f32,
    strafe_t: f32,
    burst_t: f32,
    pause_t: f32,
    tap_cd: f32,
    aim_off: Vec2,
    aim_off_goal: Vec2,
    aim_off_t: f32,
    head_bias: bool,
    jump_cd: f32,
    use_ads: bool,
    hold_t: f32,
    // building
    job: Job,
    cover_cd: f32,
    ramp_cd: f32,
    // misc
    interact_cd: f32,
    swim_to: Vec2,
    swim_t: f32,
    avoid_side: f32,
}

impl Brain {
    pub fn new(rng: &mut Rng, d: Difficulty) -> Brain {
        let mut r = rng.fork(0xB07);
        let skill = Skill::new(d, &mut r);
        Brain {
            last_attacker: None,
            think_t: r.range(0.0, THINK),
            skill,
            land: None,
            jump_dist: 0.0,
            since_think: 0.0,
            ev_seen: 0,
            goal_t: 0.0,
            hurt_t: 99.0,
            enemy: None,
            enemy_pos: Vec3::ZERO,
            seen_t: 99.0,
            visible: false,
            react_t: 0.0,
            goal: Goal::Idle,
            blacklist: vec![],
            investigate: None,
            roam_cd: 0.0,
            zone_off: Vec2::ZERO,
            zone_phase: usize::MAX,
            zone_lead: r.range(12.0, 45.0),
            path: vec![],
            path_i: 0,
            path_goal: Vec2::splat(1e9),
            repath_t: 0.0,
            stuck_ref: Vec2::ZERO,
            stuck_clock: 0.0,
            stuck_count: 0,
            unstick_t: 0.0,
            unstick_sign: 1.0,
            giveups: 0,
            strafe_sign: 1.0,
            strafe_t: 0.0,
            burst_t: 0.0,
            pause_t: 0.0,
            tap_cd: 0.0,
            aim_off: Vec2::ZERO,
            aim_off_goal: Vec2::ZERO,
            aim_off_t: 0.0,
            head_bias: false,
            jump_cd: 1.5,
            use_ads: false,
            hold_t: 0.0,
            job: Job::None,
            cover_cd: 0.0,
            ramp_cd: 0.0,
            interact_cd: 0.0,
            swim_to: Vec2::ZERO,
            swim_t: 0.0,
            avoid_side: 1.0,
            rng: r,
        }
    }

    /// Called by the combat code when this bot takes damage.
    pub fn on_damaged(&mut self, attacker: usize) {
        self.last_attacker = Some(attacker);
        self.hurt_t = 0.0;
    }

    pub fn skill(&self) -> Skill {
        self.skill
    }

    /// One-line dump of the interesting state.
    pub fn debug(&self) -> String {
        format!(
            "goal={:?} enemy={:?} vis={} seen_t={:.1} path={}/{} giveups={} unstick={:.1} black={} job={:?} investigate={:?}",
            self.goal,
            self.enemy,
            self.visible,
            self.seen_t,
            self.path_i,
            self.path.len(),
            self.giveups,
            self.unstick_t,
            self.blacklist.len(),
            self.job,
            self.investigate.map(|x| x.0)
        )
    }

    /// What the bot is currently doing (for debugging overlays and tests).
    pub fn goal_name(&self) -> &'static str {
        match self.goal {
            Goal::Idle => "idle",
            Goal::Loot { .. } => "loot",
            Goal::Harvest { .. } => "harvest",
            Goal::Travel { .. } => "travel",
            Goal::Fight => "fight",
            Goal::Heal => "heal",
        }
    }
}

/// Entry point used by `Game::step`.
pub fn think(g: &mut Game, i: usize, dt: f32) -> Intent {
    let Some(mut brain) = g.actors[i].brain.take() else {
        let a = &g.actors[i];
        return Intent { yaw: a.yaw, pitch: a.pitch, ..Default::default() };
    };
    let it = brain.run(g, i, dt);
    g.actors[i].brain = Some(brain);
    it
}

// ---------------------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------------------

fn base_intent(a: &Actor) -> Intent {
    Intent { yaw: a.yaw, pitch: a.pitch, ..Default::default() }
}

fn yaw_to(from: Vec3, to: Vec3) -> f32 {
    yaw_of(xz(to - from))
}

fn pitch_to(from: Vec3, to: Vec3) -> f32 {
    let d = to - from;
    d.y.atan2(xz(d).length().max(0.001))
}

fn rotate(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// Turn `cur` toward `target` with exponential smoothing, capped by `max_rate` rad/s.
fn turn_toward(cur: f32, target: f32, max_rate: f32, dt: f32) -> f32 {
    let d = angle_diff(cur, target);
    let step = (d * damp(14.0, dt)).clamp(-max_rate * dt, max_rate * dt);
    wrap_pi(cur + step)
}

/// The building whose walls enclose `p`. The collider box also covers the roof overhang, so it is shrunk by the
/// overhang: a bot outside the wall but under the eaves is outside (routing it through the door would walk it into the wall).
fn building_of(g: &Game, p: Vec3) -> Option<usize> {
    const EAVES: f32 = 0.9;
    g.world.buildings.iter().position(|b| {
        p.x > b.aabb.min.x + EAVES && p.x < b.aabb.max.x - EAVES && p.z > b.aabb.min.z + EAVES && p.z < b.aabb.max.z - EAVES && p.y >= b.aabb.min.y - 1.0 && p.y <= b.aabb.max.y + 0.5
    })
}

fn weapon_slots(a: &Actor) -> impl Iterator<Item = (usize, WeaponKind, Rarity, u32)> + '_ {
    (1..6).filter_map(move |s| match a.inv.slots[s] {
        Some(Item::Weapon { kind, rarity, ammo }) => Some((s, kind, rarity, ammo)),
        _ => None,
    })
}

/// Rounds available for a weapon (in the magazine plus the reserve).
fn rounds(a: &Actor, kind: WeaponKind, mag: u32) -> u32 {
    mag + a.inv.ammo[kind.def().ammo.index()]
}

fn is_armed(a: &Actor) -> bool {
    weapon_slots(a).any(|(_, k, _, m)| rounds(a, k, m) > 0)
}

fn count_of(a: &Actor, kind: ConsumableKind) -> u32 {
    a.inv.slots.iter().flatten().map(|it| if let Item::Consumable { kind: k, count } = it { if *k == kind { *count } else { 0 } } else { 0 }).sum()
}

/// How much `item` would help if used now.
fn heal_benefit(a: &Actor, kind: ConsumableKind) -> f32 {
    let d = kind.def();
    let hp = if d.heal > 0.0 { d.heal.min((d.max_hp - a.hp).max(0.0)) } else { 0.0 };
    let sh = if d.shield > 0.0 { d.shield.min((d.max_shield - a.shield).max(0.0)) } else { 0.0 };
    hp + sh
}

/// Best consumable slot to use right now (value per second, ignoring tiny top-ups).
fn best_heal_slot(a: &Actor) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for s in 1..6 {
        if let Some(Item::Consumable { kind, .. }) = a.inv.slots[s] {
            let b = heal_benefit(a, kind);
            let min = match kind {
                ConsumableKind::Bandage => 12.0,
                ConsumableKind::ShieldSmall => 15.0,
                ConsumableKind::MedKit => 45.0,
                ConsumableKind::ShieldBig => 30.0,
                ConsumableKind::ChugJug => 100.0,
            };
            if b < min {
                continue;
            }
            let v = b / kind.def().use_time;
            if best.is_none_or(|x| v > x.0) {
                best = Some((v, s));
            }
        }
    }
    best.map(|x| x.1)
}

fn needs_heal(a: &Actor) -> bool {
    (a.hp < 72.0 || a.shield < 45.0) && best_heal_slot(a).is_some()
}

/// How good a weapon is at a given distance (1.0 = decent).
fn range_fit(kind: WeaponKind, d: f32) -> f32 {
    match kind {
        WeaponKind::Shotgun => {
            if d < 10.0 {
                1.6
            } else if d < 20.0 {
                1.0
            } else if d < 35.0 {
                0.4
            } else {
                0.1
            }
        }
        WeaponKind::Smg => {
            if d < 28.0 {
                1.25
            } else if d < 50.0 {
                0.85
            } else {
                0.45
            }
        }
        WeaponKind::AssaultRifle => {
            if d < 10.0 {
                0.9
            } else if d < 95.0 {
                1.25
            } else {
                0.9
            }
        }
        WeaponKind::Sniper => {
            if d < 20.0 {
                0.25
            } else if d < 50.0 {
                0.7
            } else {
                1.6
            }
        }
        WeaponKind::Pistol => 0.7,
        WeaponKind::RocketLauncher => {
            if d < 9.0 {
                0.0
            } else if d < 65.0 {
                1.0
            } else {
                0.45
            }
        }
    }
}

fn preferred_range(kind: WeaponKind) -> f32 {
    match kind {
        WeaponKind::Shotgun => 8.0,
        WeaponKind::Smg => 15.0,
        WeaponKind::Pistol => 17.0,
        WeaponKind::AssaultRifle => 27.0,
        WeaponKind::Sniper => 70.0,
        WeaponKind::RocketLauncher => 30.0,
    }
}

// ---------------------------------------------------------------------------------------
// Brain
// ---------------------------------------------------------------------------------------

impl Brain {
    fn run(&mut self, g: &mut Game, i: usize, dt: f32) -> Intent {
        self.hurt_t += dt;
        self.goal_t += dt;
        self.since_think += dt;
        self.cover_cd = (self.cover_cd - dt).max(0.0);
        self.ramp_cd = (self.ramp_cd - dt).max(0.0);
        self.interact_cd = (self.interact_cd - dt).max(0.0);
        self.jump_cd = (self.jump_cd - dt).max(0.0);
        self.roam_cd = (self.roam_cd - dt).max(0.0);
        self.hold_t = (self.hold_t - dt).max(0.0);
        self.repath_t -= dt;
        match g.actors[i].mode {
            MoveMode::Bus => self.on_bus(g, i),
            MoveMode::Freefall | MoveMode::Glide => self.in_sky(g, i),
            MoveMode::Swim => self.swim(g, i, dt),
            MoveMode::Ground => self.on_ground(g, i, dt),
            MoveMode::Dead => base_intent(&g.actors[i]),
        }
    }

    // ---- bus & sky ------------------------------------------------------------------------

    fn choose_landing(&mut self, g: &Game) -> Vec2 {
        let (s, e) = (g.bus.start, g.bus.end);
        let mut cands: Vec<(Vec2, f32, f32)> = vec![];
        for poi in &g.world.layout.pois {
            let (d, _) = point_segment_dist(poi.center, s, e);
            if d > 300.0 {
                continue;
            }
            let kind_w = match poi.kind {
                PoiKind::Town => 3.0,
                PoiKind::Coastal => 2.0,
                PoiKind::Lakeside => 1.6,
                PoiKind::Lodge => 1.3,
                PoiKind::Farm => 1.0,
                PoiKind::Station => 1.0,
                PoiKind::Hamlet => 0.8,
            };
            let crowd = g.actors.iter().filter(|o| o.brain.as_ref().and_then(|b| b.land).is_some_and(|l| l.distance(poi.center) < poi.radius + 60.0)).count() as f32;
            let w = kind_w / (1.0 + d / 140.0) / (1.0 + 0.55 * crowd);
            let p = poi.center + self.rng.in_disc(poi.radius * 0.6);
            cands.push((p, d, w));
        }
        // sometimes drop somewhere quiet instead
        for _ in 0..3 {
            let t = self.rng.f32();
            let on_line = s.lerp(e, t);
            let p = on_line + self.rng.in_disc(220.0);
            if g.world.hm.height_at(p.x, p.y) > 4.0 {
                let (d, _) = point_segment_dist(p, s, e);
                cands.push((p, d, 0.45));
            }
        }
        if cands.is_empty() {
            self.jump_dist = 120.0;
            return Vec2::ZERO;
        }
        let weights: Vec<f32> = cands.iter().map(|c| c.2).collect();
        let k = self.rng.weighted(&weights);
        let (p, d, _) = cands[k];
        self.jump_dist = (d.max(70.0) + self.rng.range(15.0, 150.0)).min(370.0);
        p
    }

    fn on_bus(&mut self, g: &mut Game, i: usize) -> Intent {
        if self.land.is_none() {
            let l = self.choose_landing(g);
            self.land = Some(l);
        }
        let land = self.land.unwrap();
        let bus_xz = Vec2::new(g.bus.pos.x, g.bus.pos.z);
        let mut it = base_intent(&g.actors[i]);
        it.yaw = yaw_of(land - bus_xz);
        it.pitch = -0.25;
        if bus_xz.distance(land) <= self.jump_dist && g.bus.t > 2.5 + (i % 9) as f32 * 0.25 {
            it.exit_bus = true;
        }
        it
    }

    fn in_sky(&mut self, g: &mut Game, i: usize) -> Intent {
        let a = &g.actors[i];
        let land = self.land.unwrap_or(Vec2::ZERO);
        let pos = xz(a.pos);
        let to = land - pos;
        let d = to.length();
        let target_y = g.world.hm.height_at(land.x, land.y).max(0.0);
        let h = (a.pos.y - target_y).max(1.0);
        let mut it = base_intent(a);
        it.yaw = yaw_of(to);
        if a.mode == MoveMode::Freefall {
            // burn altitude down to the auto-deploy height while covering the distance to a point
            // ~150 m short of the landing spot, then glide the rest
            let spare = (h - super::movement::GLIDER_AUTO_DEPLOY).max(1.0);
            let need = (d - 150.0).max(0.0);
            let flat_reach = 1.15 * spare;
            let dive_reach = 0.15 * spare;
            let dive = ((flat_reach - need) / (flat_reach - dive_reach).max(1.0)).clamp(0.0, 1.0);
            it.pitch = lerp(-0.1, -1.3, dive);
        } else {
            let ratio = d / h;
            it.pitch = if ratio > 2.1 {
                0.3
            } else if ratio < 1.5 {
                -0.9
            } else {
                -0.2
            };
            if d < 30.0 && h > 25.0 {
                // circle down over the spot
                it.wish = rotate(to.normalize_or_zero(), 1.5);
                it.pitch = -1.0;
            }
        }
        it
    }

    fn swim(&mut self, g: &mut Game, i: usize, dt: f32) -> Intent {
        let a = &g.actors[i];
        self.swim_t -= dt;
        if self.swim_t <= 0.0 {
            self.swim_t = 0.6;
            let env = g.env();
            let p = xz(a.pos);
            let mut best: Option<(f32, Vec2)> = None;
            for k in 0..24 {
                let ang = k as f32 / 24.0 * TAU_F;
                let dir = Vec2::new(ang.cos(), ang.sin());
                let mut r = 6.0;
                while r < 160.0 {
                    let q = p + dir * r;
                    if env.water_depth(q.x, q.y) < 0.6 && g.world.hm.height_at(q.x, q.y) > 0.3 {
                        if best.is_none_or(|b| r < b.0) {
                            best = Some((r, q));
                        }
                        break;
                    }
                    r += 6.0;
                }
            }
            self.swim_to = best.map(|b| b.1).unwrap_or(Vec2::ZERO);
        }
        let a = &g.actors[i];
        let to = self.swim_to - xz(a.pos);
        let mut it = base_intent(a);
        it.wish = to.normalize_or_zero();
        if to.length() > 0.5 {
            it.yaw = turn_toward(a.yaw, yaw_of(to), 6.0, dt);
        }
        it
    }

    // ---- main ground logic ---------------------------------------------------------------------

    fn on_ground(&mut self, g: &mut Game, i: usize, dt: f32) -> Intent {
        if self.think_t <= 0.0 || self.since_think > 0.5 {
            self.think_t = THINK + self.rng.range(0.0, 0.1);
            self.decide(g, i);
            self.since_think = 0.0;
        }
        self.think_t -= dt;
        let mut it = base_intent(&g.actors[i]);
        if let Job::None = self.job {
        } else if self.run_job(g, i, dt, &mut it) {
            return it;
        }
        match self.goal {
            Goal::Fight => self.act_fight(g, i, dt, &mut it),
            Goal::Heal => self.act_heal(g, i, dt, &mut it),
            Goal::Loot { what, pos } => self.act_loot(g, i, dt, &mut it, what, pos),
            Goal::Harvest { idx } => self.act_harvest(g, i, dt, &mut it, idx),
            Goal::Travel { to, sprint } => self.act_travel(g, i, dt, &mut it, to, sprint),
            Goal::Idle => self.act_idle(g, i, dt, &mut it),
        }
        it
    }

    fn set_goal(&mut self, goal: Goal) {
        // keep walking the existing route when the new destination is practically the same place
        if let (Goal::Travel { to: old, .. }, Goal::Travel { to: new, sprint }) = (self.goal, goal) {
            if old.distance(new) < 14.0 {
                self.goal = Goal::Travel { to: old, sprint };
                return;
            }
        }
        if self.goal != goal {
            let same_kind = std::mem::discriminant(&self.goal) == std::mem::discriminant(&goal);
            if !same_kind {
                self.goal_t = 0.0;
                self.giveups = 0;
            }
            self.goal = goal;
        }
    }

    // ---- perception -----------------------------------------------------------------------------

    fn perceive(&mut self, g: &Game, i: usize, td: f32) {
        let a = &g.actors[i];
        let env = g.env();
        let eye = a.eye_pos();
        let f = yaw_forward(a.yaw);
        let fwd = Vec2::new(f.x, f.z);
        let mut best: Option<(f32, usize)> = None;
        for o in &g.actors {
            if o.id == i || !o.alive || matches!(o.mode, MoveMode::Bus) {
                continue;
            }
            let d = o.chest() - eye;
            let dist = d.length();
            if dist > SIGHT {
                continue;
            }
            let d2 = Vec2::new(d.x, d.z);
            let facing = if d2.length() > 0.01 { d2.normalize().dot(fwd) } else { 1.0 };
            if dist > 14.0 && facing < -0.25 {
                continue;
            }
            let known = self.enemy == Some(o.id) && self.seen_t < 4.0;
            let p = ((1.5 - dist / 170.0) * self.skill.awareness).clamp(0.12, 1.0);
            if !known && !self.rng.chance(p) {
                continue;
            }
            if !env.line_clear(eye, o.chest()) && !env.line_clear(eye, o.head_pos()) {
                continue;
            }
            let score = dist - if known { 20.0 } else { 0.0 };
            if best.is_none_or(|b| score < b.0) {
                best = Some((score, o.id));
            }
        }
        if let Some((_, id)) = best {
            let o = &g.actors[id];
            if self.enemy != Some(id) || self.seen_t > 2.5 {
                self.react_t = self.skill.react * self.rng.range(0.7, 1.4);
                self.burst_t = 0.0;
                self.pause_t = 0.0;
            }
            self.enemy = Some(id);
            self.visible = true;
            self.seen_t = 0.0;
            self.enemy_pos = o.chest();
        } else {
            self.visible = false;
            self.seen_t += td;
        }
        // someone hit us: turn on them even without a line of sight
        if let Some(att) = self.last_attacker.take() {
            if g.actors.get(att).is_some_and(|o| o.alive) && (!self.visible || self.enemy.is_none()) {
                let o = &g.actors[att];
                if self.enemy != Some(att) {
                    self.react_t = self.skill.react * 0.6;
                }
                self.enemy = Some(att);
                self.enemy_pos = o.chest();
                self.seen_t = 0.6;
            }
        }
        if self.seen_t > 9.0 {
            self.enemy = None;
        }
        if let Some(e) = self.enemy {
            if !g.actors[e].alive {
                self.enemy = None;
                self.visible = false;
            }
        }
    }

    fn listen(&mut self, g: &Game, i: usize) {
        if self.ev_seen > g.events.len() {
            self.ev_seen = 0;
        }
        let me = g.actors[i].pos;
        for e in &g.events[self.ev_seen..] {
            if let Event::Noise { pos, radius, source } = e {
                if *source == i || *radius < 50.0 {
                    continue;
                }
                let d = pos.distance(me);
                if d < radius * 0.75 && d > 12.0 && self.enemy.is_none() && self.investigate.is_none() && self.rng.chance(0.15 + 0.45 * self.skill.aggression) {
                    let spot = *pos + Vec3::new(self.rng.range(-8.0, 8.0), 0.0, self.rng.range(-8.0, 8.0));
                    self.investigate = Some((spot, 14.0));
                }
            }
        }
        self.ev_seen = g.events.len();
    }

    // ---- decisions ---------------------------------------------------------------------------------

    /// If the bot should be heading for the next safe circle, where to.
    fn zone_target(&mut self, g: &Game, i: usize) -> Option<(Vec2, bool)> {
        let s = &g.storm;
        if !s.active {
            return None;
        }
        let a = &g.actors[i];
        let pos = xz(a.pos);
        if self.zone_phase != s.phase {
            self.zone_phase = s.phase;
            self.zone_off = self.rng.in_disc(0.55);
            self.zone_lead = self.rng.range(12.0, 50.0);
        }
        let dist_now = pos.distance(s.center);
        let in_now = dist_now < s.radius - 3.0;
        let dist_next = pos.distance(s.to_center);
        let in_next = dist_next < s.to_radius * 0.9;
        let aim = s.to_center + self.zone_off * s.to_radius;
        match s.state {
            super::StormState::Waiting => {
                let go = !in_now || (!in_next && (s.timer < self.zone_lead + s.phase as f32 * 6.0 || s.phase >= 3));
                if go {
                    Some((aim, !in_now || s.timer < 25.0))
                } else {
                    None
                }
            }
            super::StormState::Shrinking => {
                if !in_next || !in_now {
                    Some((aim, true))
                } else {
                    None
                }
            }
            super::StormState::Done => None,
        }
    }

    fn decide(&mut self, g: &mut Game, i: usize) {
        let td = self.since_think.max(0.05);
        self.perceive(g, i, td);
        self.listen(g, i);
        if let Some((_, t)) = &mut self.investigate {
            *t -= td;
            if *t <= 0.0 {
                self.investigate = None;
            }
        }
        self.blacklist.retain(|b| b.1 > g.time);
        let a = &g.actors[i];
        let armed = is_armed(a);
        let zone = self.zone_target(g, i);
        let zone_urgent = zone.is_some_and(|z| z.1) && !(g.storm.state == super::StormState::Waiting && g.storm.timer > 25.0);
        let has_enemy = self.enemy.is_some() && self.seen_t < 7.0;

        // 1. fight. With dry guns only late in the match, once the loot is gone, and only against an enemy close by: after the
        // landing an unarmed bot should be looking for a weapon, not trading pickaxe blows.
        let alive = g.alive_count();
        let late = alive <= 7 || g.storm.phase >= 4;
        let brawl = late && !armed && self.visible && a.pos.distance(self.enemy_pos) < BRAWL_RANGE;
        if has_enemy && (armed || brawl) {
            let dist = a.pos.distance(self.enemy_pos);
            let range = 55.0 + self.skill.aggression * 150.0;
            let attacked = self.hurt_t < 5.0;
            let wants = (self.visible && dist < range) || attacked || (self.seen_t < 3.0 && dist < range);
            if wants && !(zone_urgent && dist > 30.0 && !attacked) {
                self.set_goal(Goal::Fight);
                return;
            }
        }
        if matches!(self.goal, Goal::Fight) {
            self.set_goal(Goal::Idle);
        }

        // 2. heal when it is safe
        if self.seen_t > 2.5 && self.hurt_t > 2.5 && needs_heal(a) && !zone_urgent {
            self.set_goal(Goal::Heal);
            return;
        }
        if matches!(self.goal, Goal::Heal) {
            self.set_goal(Goal::Idle);
        }

        // 3. the storm
        if let Some((to, sprint)) = zone {
            self.set_goal(Goal::Travel { to, sprint });
            return;
        }

        // 4. something interesting nearby: loot
        if let Some((what, pos)) = self.pick_loot(g, i) {
            self.set_goal(Goal::Loot { what, pos });
            return;
        }
        if matches!(self.goal, Goal::Loot { .. }) {
            self.set_goal(Goal::Idle);
        }

        // 5. investigate noises
        if let Some((spot, _)) = self.investigate {
            if armed {
                self.set_goal(Goal::Travel { to: xz(spot), sprint: false });
                return;
            }
        }

        // 5b. late in the match everyone closes in on the remaining players
        // (unarmed ones as well: with the loot gone, the pickaxe is all that is left to settle it with)
        if late && self.enemy.is_none() && alive > 1 {
            let me = xz(a.pos);
            let mut best: Option<(f32, Vec2)> = None;
            for o in &g.actors {
                if o.id == i || !o.alive || matches!(o.mode, MoveMode::Bus) {
                    continue;
                }
                let d = xz(o.pos).distance(me);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, xz(o.pos)));
                }
            }
            if let Some((d, p)) = best {
                if d > 25.0 {
                    let to = p + self.rng.in_disc(10.0);
                    self.set_goal(Goal::Travel { to, sprint: true });
                    return;
                }
            }
        }

        // 6. materials
        let a = &g.actors[i];
        if a.inv.mats[0] < 60 && self.goal_t > 0.0 {
            if let Goal::Harvest { idx } = self.goal {
                if g.world.harvest[idx].alive && self.goal_t < 14.0 && a.inv.mats[0] < 150 {
                    return;
                }
            } else if let Some(idx) = self.pick_tree(g, i) {
                self.set_goal(Goal::Harvest { idx });
                return;
            }
        }
        if matches!(self.goal, Goal::Harvest { .. }) {
            self.set_goal(Goal::Idle);
        }

        // 7. keep moving somewhere useful
        if let Goal::Travel { to, .. } = self.goal {
            if xz(g.actors[i].pos).distance(to) > 6.0 && self.goal_t < 45.0 {
                return;
            }
        }
        if self.roam_cd <= 0.0 || !matches!(self.goal, Goal::Travel { .. }) {
            let to = self.pick_roam(g, i);
            self.set_goal(Goal::Travel { to, sprint: false });
            self.goal_t = 0.0;
        }
    }

    fn pickup_value(&self, a: &Actor, pk: &PickupKind) -> f32 {
        match *pk {
            PickupKind::Weapon { kind, rarity, ammo } => {
                let sc = weapon_score(kind, rarity);
                let have = a.inv.weapon_count();
                if have == 0 {
                    return 130.0 + sc * 0.3;
                }
                // empty guns are only worth it when we can feed them
                let usable = ammo > 0 || a.inv.has_ammo_for(kind);
                if !usable {
                    return 0.0;
                }
                let worst = weapon_slots(a).map(|(_, k, r, _)| weapon_score(k, r)).fold(f32::MAX, f32::min);
                let same_kind_better = weapon_slots(a).any(|(_, k, r, _)| k == kind && r >= rarity);
                if same_kind_better {
                    return 0.0;
                }
                if have < MAX_WEAPONS && a.inv.free_slot().is_some() {
                    return 48.0 + sc * 0.25;
                }
                if sc > worst * 1.12 {
                    return 26.0 + (sc - worst) * 0.6;
                }
                0.0
            }
            PickupKind::Ammo { kind, .. } => {
                let uses = weapon_slots(a).any(|(_, k, _, _)| k.def().ammo == kind);
                let frac = a.inv.ammo[kind.index()] as f32 / kind.cap() as f32;
                if uses && frac < 0.7 {
                    32.0 * (1.0 - frac)
                } else {
                    0.0
                }
            }
            PickupKind::Consumable { kind, .. } => {
                let have = count_of(a, kind);
                let want = match kind {
                    ConsumableKind::Bandage => 8,
                    ConsumableKind::MedKit => 2,
                    ConsumableKind::ShieldSmall => 4,
                    ConsumableKind::ShieldBig => 2,
                    ConsumableKind::ChugJug => 1,
                };
                if have >= want {
                    return 0.0;
                }
                let room = a.inv.free_slot().is_some() || a.inv.slots.iter().flatten().any(|it| matches!(it, Item::Consumable { kind: k, count } if *k == kind && *count < kind.def().stack));
                if !room {
                    return 0.0;
                }
                match kind {
                    ConsumableKind::Bandage => 20.0,
                    ConsumableKind::MedKit => 36.0,
                    ConsumableKind::ShieldSmall => 32.0,
                    ConsumableKind::ShieldBig => 42.0,
                    ConsumableKind::ChugJug => 58.0,
                }
            }
        }
    }

    fn pick_loot(&mut self, g: &Game, i: usize) -> Option<(LootRef, Vec3)> {
        let a = &g.actors[i];
        let pos = a.pos;
        let current = if let Goal::Loot { what, .. } = self.goal { Some(what) } else { None };
        let mut best: Option<(f32, LootRef, Vec3)> = None;
        let unarmed = !is_armed(a);
        let reach = if unarmed { 120.0 } else { 75.0 };
        for p in &g.pickups {
            if !p.grounded && p.age < 0.6 {
                continue;
            }
            let d = p.pos.distance(pos);
            if d > reach || self.blacklist.iter().any(|b| b.0 == p.id) {
                continue;
            }
            // upstairs rooms are out of reach for bots
            if let Some(b) = building_of(g, p.pos) {
                if p.pos.y > g.world.buildings[b].door_in.y + 2.0 {
                    continue;
                }
            }
            let mut v = self.pickup_value(a, &p.kind);
            if v <= 0.0 {
                continue;
            }
            if current == Some(LootRef::Pickup(p.id)) {
                v *= 1.35;
            }
            let s = v / (1.0 + d / 22.0);
            if best.is_none_or(|b| s > b.0) {
                best = Some((s, LootRef::Pickup(p.id), p.pos));
            }
        }
        for c in &g.chests {
            if c.opened || self.blacklist.iter().any(|b| b.0 == c.id) {
                continue;
            }
            let d = c.pos.distance(pos);
            if d > reach * 1.6 {
                continue;
            }
            if let Some(b) = building_of(g, c.pos) {
                if c.pos.y > g.world.buildings[b].door_in.y + 2.0 {
                    continue;
                }
            }
            let mut v = if unarmed { 90.0 } else { 62.0 };
            if current == Some(LootRef::Chest(c.id)) {
                v *= 1.35;
            }
            let s = v / (1.0 + d / 22.0);
            if best.is_none_or(|b| s > b.0) {
                best = Some((s, LootRef::Chest(c.id), c.pos));
            }
        }
        // ignore weak scores far away
        best.filter(|b| b.0 > 4.0).map(|b| (b.1, b.2))
    }

    fn pick_tree(&mut self, g: &Game, i: usize) -> Option<usize> {
        let p = g.actors[i].pos;
        let mut best: Option<(f32, usize)> = None;
        for (k, h) in g.world.harvest.iter().enumerate() {
            if !h.alive || h.kind != HarvestKind::Tree {
                continue;
            }
            let dx = (h.pos.x - p.x).abs();
            let dz = (h.pos.z - p.z).abs();
            if dx > 40.0 || dz > 40.0 {
                continue;
            }
            let d = dx * dx + dz * dz;
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, k));
            }
        }
        best.map(|b| b.1)
    }

    fn pick_roam(&mut self, g: &Game, i: usize) -> Vec2 {
        let a = &g.actors[i];
        let pos = xz(a.pos);
        self.roam_cd = self.rng.range(8.0, 20.0);
        // go where loot or people are likely: weight towns by remaining loot and distance
        let mut cands: Vec<(Vec2, f32)> = vec![];
        for poi in &g.world.layout.pois {
            let d = poi.center.distance(pos);
            if d < poi.radius * 0.5 {
                continue;
            }
            let loot = g.pickups.iter().filter(|p| xz(p.pos).distance(poi.center) < poi.radius).count() as f32;
            let chests = g.chests.iter().filter(|c| !c.opened && xz(c.pos).distance(poi.center) < poi.radius).count() as f32;
            let w = (2.0 + loot * 0.5 + chests * 3.0) / (80.0 + d);
            cands.push((poi.center + self.rng.in_disc(poi.radius * 0.5), w));
        }
        // keep roaming inside the safe zone
        let s = &g.storm;
        let inside = |p: Vec2| !s.active || p.distance(s.to_center) < s.to_radius * 0.95;
        cands.retain(|c| inside(c.0));
        if cands.is_empty() {
            let c = if s.active { s.to_center } else { Vec2::ZERO };
            let r = if s.active { s.to_radius * 0.7 } else { 300.0 };
            return c + self.rng.in_disc(r);
        }
        let w: Vec<f32> = cands.iter().map(|c| c.1).collect();
        cands[self.rng.weighted(&w)].0
    }

    // ---- movement ---------------------------------------------------------------------------------------

    /// Build a path to `goal` (taking doors into account).
    fn plan(&mut self, g: &mut Game, i: usize, goal: Vec3) {
        let a = &g.actors[i];
        let pos = xz(a.pos);
        let gxz = xz(goal);
        self.path.clear();
        self.path_i = 0;
        self.path_goal = gxz;
        self.repath_t = 3.5 + self.rng.range(0.0, 2.0);
        let from_b = building_of(g, a.pos);
        let to_b = building_of(g, goal);
        let mut pre: Vec<Vec2> = vec![];
        let mut start = pos;
        if let Some(b) = from_b {
            if Some(b) != to_b {
                let bi = &g.world.buildings[b];
                pre.push(xz(bi.door_in));
                pre.push(xz(bi.door_out));
                start = xz(bi.door_out);
            }
        }
        let mut post: Vec<Vec2> = vec![];
        let mut dest = gxz;
        if let Some(b) = to_b {
            if from_b != Some(b) {
                let bi = &g.world.buildings[b];
                dest = xz(bi.door_out);
                post.push(xz(bi.door_in));
                post.push(gxz);
            }
        }
        let mut mid: Vec<Vec2>;
        if start.distance(dest) > 7.0 {
            if g.ai_paths_left > 0 {
                g.ai_paths_left -= 1;
                let nav = &g.world.nav;
                // pressed against a wall the bot may stand in a blocked cell: start from a free cell it can actually see
                let env = g.env();
                let eye = Vec3::new(start.x, g.actors[i].pos.y + 0.7, start.y);
                let visible = |c: Vec2| {
                    let to = Vec3::new(c.x, eye.y, c.y) - eye;
                    let l = to.length();
                    l < 0.1 || env.probe(eye, to / l, l).is_none()
                };
                match nav.find_path_visible(start, dest, 9000, &visible) {
                    Some(raw) => {
                        mid = nav.smooth(&raw);
                        // the first node is the start cell centre; drop it when we are already past it
                        if mid.len() > 1 && mid[0].distance(start) < 3.0 {
                            mid.remove(0);
                        }
                    }
                    None => mid = vec![dest],
                }
            } else {
                // out of budget this frame: head straight and retry soon
                mid = vec![dest];
                self.repath_t = 0.15;
            }
        } else {
            mid = vec![dest];
        }
        self.path.extend(pre);
        self.path.extend(mid);
        self.path.extend(post);
        if self.path.is_empty() {
            self.path.push(gxz);
        }
    }

    /// Walk toward `goal`. Returns (wish direction, arrived).
    fn walk_to(&mut self, g: &mut Game, i: usize, goal: Vec3, arrive: f32) -> (Vec2, bool) {
        let pos = xz(g.actors[i].pos);
        let gxz = xz(goal);
        if pos.distance(gxz) < arrive && (goal.y - g.actors[i].pos.y).abs() < 3.0 {
            return (Vec2::ZERO, true);
        }
        if self.path.is_empty() || gxz.distance(self.path_goal) > 3.0 || self.repath_t <= 0.0 {
            self.plan(g, i, goal);
        }
        while self.path_i + 1 < self.path.len() && pos.distance(self.path[self.path_i]) < 1.7 {
            self.path_i += 1;
        }
        let wp = self.path.get(self.path_i).copied().unwrap_or(gxz);
        let d = wp - pos;
        (d.normalize_or_zero(), false)
    }

    /// Steer around walls, fences and trunks: probe ahead and rotate the heading to the first clear one.
    fn avoid(&mut self, g: &Game, i: usize, wish: Vec2) -> Vec2 {
        if wish.length_squared() < 0.01 {
            return wish;
        }
        let a = &g.actors[i];
        let env = g.env();
        let o = a.pos + Vec3::Y * 0.7;
        let look = 1.7 + a.speed_xz() * 0.1;
        let clear = |d: Vec2| env.probe(o, v3(d, 0.0), look).is_none();
        if clear(wish) {
            return wish;
        }
        for k in 1..=4 {
            let ang = 0.45 * k as f32;
            for s in [self.avoid_side, -self.avoid_side] {
                let d = rotate(wish, ang * s);
                if clear(d) {
                    self.avoid_side = s;
                    return d;
                }
            }
        }
        wish
    }

    /// Detect being wedged against something and wiggle free.
    fn unstick(&mut self, a: &Actor, wish: Vec2, wants_move: bool, dt: f32, it: &mut Intent) -> Vec2 {
        self.stuck_clock += dt;
        if self.stuck_clock >= 0.5 {
            let moved = xz(a.pos).distance(self.stuck_ref);
            if wants_move && moved < 0.4 {
                self.stuck_count += 1;
            } else {
                self.stuck_count = 0;
            }
            self.stuck_ref = xz(a.pos);
            self.stuck_clock = 0.0;
            if self.stuck_count >= 2 {
                self.stuck_count = 0;
                self.unstick_t = 0.9;
                self.unstick_sign = self.rng.sign();
                self.repath_t = 0.0;
                self.giveups += 1;
            }
        }
        if self.unstick_t > 0.0 {
            self.unstick_t -= dt;
            it.jump = true;
            return rotate(wish, self.unstick_sign * 1.35);
        }
        wish
    }

    fn face_move(&self, a: &Actor, wish: Vec2, dt: f32, it: &mut Intent) {
        if wish.length() > 0.1 {
            it.yaw = turn_toward(a.yaw, yaw_of(wish), 7.0, dt);
        }
        it.pitch = turn_toward(a.pitch, -0.05, 4.0, dt);
    }

    // ---- idle / travel / loot / harvest ------------------------------------------------------------------

    fn equip_best(&self, a: &Actor, it: &mut Intent) {
        if !matches!(a.action, Action::None) {
            return;
        }
        if let Some(best) = a.inv.best_weapon_slot() {
            let usable = matches!(a.inv.slots[best], Some(Item::Weapon { kind, ammo, .. }) if rounds(a, kind, ammo) > 0);
            if usable && a.inv.selected != best && !a.build_mode {
                it.select = Some(best);
            }
        }
    }

    fn idle_upkeep(&mut self, a: &Actor, it: &mut Intent) {
        // top up the magazine while nothing is going on
        if let Some((kind, _, ammo)) = a.inv.selected_weapon() {
            let def = kind.def();
            if matches!(a.action, Action::None) && ammo * 2 < def.mag && a.inv.ammo[def.ammo.index()] > 0 && !self.visible {
                it.reload = true;
            }
        }
    }

    fn act_idle(&mut self, g: &mut Game, i: usize, dt: f32, it: &mut Intent) {
        let a = &g.actors[i];
        self.equip_best(a, it);
        self.idle_upkeep(a, it);
        self.face_move(a, Vec2::ZERO, dt, it);
    }

    fn act_travel(&mut self, g: &mut Game, i: usize, dt: f32, it: &mut Intent, to: Vec2, sprint: bool) {
        let goal = v3(to, g.world.hm.height_at(to.x, to.y));
        let (wish, arrived) = self.walk_to(g, i, goal, 5.0);
        let a = &g.actors[i];
        self.equip_best(a, it);
        self.idle_upkeep(a, it);
        if arrived || self.goal_t > 60.0 {
            self.set_goal(Goal::Idle);
            self.roam_cd = 0.0;
            return;
        }
        let wish = self.avoid(g, i, wish);
        let a = &g.actors[i];
        let wish = self.unstick(a, wish, true, dt, it);
        it.wish = wish;
        it.sprint = sprint || self.investigate.is_none();
        self.face_move(a, wish, dt, it);
        if self.giveups >= 4 {
            self.giveups = 0;
            self.set_goal(Goal::Idle);
            self.roam_cd = 0.0;
        }
        // an investigation ends when we get there
        if let Some((spot, _)) = self.investigate {
            if xz(spot).distance(xz(a.pos)) < 10.0 {
                self.investigate = None;
            }
        }
    }

    fn act_loot(&mut self, g: &mut Game, i: usize, dt: f32, it: &mut Intent, what: LootRef, pos: Vec3) {
        // is the target still there?
        let (still, tpos) = match what {
            LootRef::Pickup(id) => match g.pickup_index(id) {
                Some(k) => (true, g.pickups[k].pos),
                None => (false, pos),
            },
            LootRef::Chest(id) => match g.chest_index(id) {
                Some(k) => (!g.chests[k].opened, g.chests[k].pos),
                None => (false, pos),
            },
        };
        if !still {
            self.set_goal(Goal::Idle);
            self.think_t = 0.0;
            return;
        }
        let a = &g.actors[i];
        let reach = INTERACT_RANGE - 0.7;
        let center = a.pos + Vec3::Y * 0.9;
        if center.distance(tpos) < reach && self.interact_cd <= 0.0 {
            // make room by choosing which weapon to give up when a weapon is picked up with full slots
            if let LootRef::Pickup(id) = what {
                if let Some(k) = g.pickup_index(id) {
                    if matches!(g.pickups[k].kind, PickupKind::Weapon { .. }) && a.inv.free_slot().is_none() {
                        let worst = weapon_slots(a).min_by(|x, y| weapon_score(x.1, x.2).partial_cmp(&weapon_score(y.1, y.2)).unwrap()).map(|x| x.0);
                        if let Some(w) = worst {
                            if a.inv.selected != w {
                                it.select = Some(w);
                                self.interact_cd = 0.25;
                                return;
                            }
                        }
                    }
                }
            }
            it.interact = true;
            self.interact_cd = 0.4;
            self.think_t = 0.0;
            // consecutive failures (e.g. a full inventory) blacklist the target
            self.giveups += 1;
            if self.giveups > 5 {
                self.blacklist.push((what.id(), g.time + 40.0));
                self.set_goal(Goal::Idle);
            }
            return;
        }
        let (wish, _arrived) = self.walk_to(g, i, tpos, reach * 0.6);
        let a = &g.actors[i];
        let wish = self.unstick(a, wish, true, dt, it);
        it.wish = wish;
        it.sprint = true;
        self.face_move(a, wish, dt, it);
        self.equip_best(a, it);
        if self.goal_t > 40.0 || self.giveups >= 4 {
            self.blacklist.push((what.id(), g.time + 40.0));
            self.set_goal(Goal::Idle);
        }
    }

    fn act_harvest(&mut self, g: &mut Game, i: usize, dt: f32, it: &mut Intent, idx: usize) {
        let h = &g.world.harvest[idx];
        if !h.alive {
            self.set_goal(Goal::Idle);
            self.think_t = 0.0;
            return;
        }
        let hpos = h.pos;
        let a = &g.actors[i];
        let d = xz(hpos - a.pos).length();
        if d > 1.9 {
            let (wish, _) = self.walk_to(g, i, hpos, 1.7);
            let a = &g.actors[i];
            let wish = self.unstick(a, wish, true, dt, it);
            it.wish = wish;
            it.sprint = true;
            self.face_move(a, wish, dt, it);
        } else {
            let a = &g.actors[i];
            if a.inv.selected != 0 && matches!(a.action, Action::None) {
                it.select = Some(0);
            }
            let eye = a.eye_pos();
            let target = hpos + Vec3::Y * 1.3;
            it.yaw = turn_toward(a.yaw, yaw_to(eye, target), 9.0, dt);
            it.pitch = turn_toward(a.pitch, pitch_to(eye, target), 9.0, dt);
            if a.inv.selected == 0 {
                it.fire = true;
            }
        }
        if self.goal_t > 16.0 {
            self.set_goal(Goal::Idle);
        }
    }

    // ---- healing ------------------------------------------------------------------------------------------

    fn act_heal(&mut self, g: &mut Game, i: usize, dt: f32, it: &mut Intent) {
        let a = &g.actors[i];
        self.face_move(a, Vec2::ZERO, dt, it);
        let Some(slot) = best_heal_slot(a) else {
            self.set_goal(Goal::Idle);
            return;
        };
        match a.action {
            Action::Heal { .. } => {}
            Action::Reload { .. } => {}
            Action::Swap { .. } => {}
            Action::None => {
                if a.inv.selected != slot || a.build_mode {
                    it.select = Some(slot);
                } else {
                    it.fire = true;
                    it.fire_pressed = true;
                }
            }
        }
        it.crouch = matches!(a.action, Action::Heal { .. });
        if self.goal_t > 20.0 {
            self.set_goal(Goal::Idle);
        }
    }

    // ---- building jobs --------------------------------------------------------------------------------------

    /// Run a scripted build job. Returns true when the job owns this tick's intent.
    fn run_job(&mut self, g: &mut Game, i: usize, dt: f32, it: &mut Intent) -> bool {
        let a = &g.actors[i];
        let target = self.enemy_pos;
        let eye = a.eye_pos();
        match self.job {
            Job::None => false,
            Job::Cover { stage, t } => {
                it.yaw = turn_toward(a.yaw, yaw_to(eye, target), 20.0, dt);
                it.pitch = 0.0;
                match stage {
                    0 => {
                        it.piece = Some(PieceKind::Wall);
                        self.job = Job::Cover { stage: 1, t: 0.0 };
                    }
                    1 => {
                        if t > 0.05 {
                            it.place = true;
                            it.fire = true;
                            self.job = Job::Cover { stage: 2, t: 0.0 };
                        } else {
                            self.job = Job::Cover { stage: 1, t: t + dt };
                        }
                    }
                    _ => {
                        if t > 0.18 {
                            it.toggle_build = true;
                            self.job = Job::None;
                            self.cover_cd = 6.0 + self.rng.range(0.0, 4.0);
                            self.hold_t = 3.5;
                        } else {
                            self.job = Job::Cover { stage: 2, t: t + dt };
                        }
                    }
                }
                true
            }
            Job::Ramp { t, placed } => {
                let to = target - a.pos;
                it.yaw = turn_toward(a.yaw, yaw_of(xz(to)), 14.0, dt);
                it.pitch = 0.0;
                if !a.build_mode {
                    it.piece = Some(PieceKind::Ramp);
                }
                it.wish = xz(to).normalize_or_zero();
                it.place = true;
                it.sprint = false;
                let mats_left = a.inv.mats[a.build_mat.index()];
                let high_enough = target.y - a.pos.y < 1.8;
                let placed_now = placed;
                if t > 4.0 || placed_now >= 6 || mats_left < 10 || high_enough {
                    it.toggle_build = a.build_mode;
                    self.job = Job::None;
                    self.ramp_cd = 10.0;
                } else {
                    let placed_new = if a.anim.build > 0.9 { placed + 1 } else { placed };
                    self.job = Job::Ramp { t: t + dt, placed: placed_new };
                }
                true
            }
        }
    }

    // ---- combat ------------------------------------------------------------------------------------------------

    fn pick_weapon(&self, a: &Actor, dist: f32) -> Option<usize> {
        let sel = a.inv.selected;
        let mut best: Option<(f32, usize)> = None;
        for (s, kind, rarity, ammo) in weapon_slots(a) {
            let rds = rounds(a, kind, ammo);
            if rds == 0 {
                continue;
            }
            let mut sc = weapon_score(kind, rarity) * range_fit(kind, dist);
            if ammo == 0 {
                sc *= 0.55;
            }
            if s == sel {
                sc *= 1.2;
            }
            if best.is_none_or(|b| sc > b.0) {
                best = Some((sc, s));
            }
        }
        best.map(|b| b.1)
    }

    /// Out of ammo: run at the enemy with the pickaxe and swing when in reach.
    fn brawl(&mut self, g: &mut Game, i: usize, dt: f32, it: &mut Intent, eid: usize) {
        let (a, e) = (&g.actors[i], &g.actors[eid]);
        // the pickaxe in hand (an empty gun is dead weight), build mode off
        if a.build_mode {
            it.toggle_build = true;
        } else if a.inv.selected != 0 && matches!(a.action, Action::None) {
            it.select = Some(0);
        }
        let eye = a.eye_pos();
        let target = if self.visible { e.chest() } else { self.enemy_pos };
        let to = target - eye;
        let dist_xz = xz(to).length();
        let (yaw, pitch) = (a.yaw, a.pitch);
        let visible = self.visible;
        it.yaw = turn_toward(yaw, yaw_to(eye, target), self.skill.turn.max(7.0), dt);
        it.pitch = turn_toward(pitch, pitch_to(eye, target), self.skill.turn.max(7.0), dt);
        let mut wish = Vec2::ZERO;
        if dist_xz > 1.4 {
            wish = if visible { xz(to).normalize_or_zero() } else { self.walk_to(g, i, self.enemy_pos, 2.0).0 };
        }
        let a = &g.actors[i];
        wish = self.unstick(a, wish, wish.length() > 0.1, dt, it);
        it.wish = wish;
        it.sprint = dist_xz > 7.0;
        // swing once the crosshair is on them and they are within the reach of the tool
        let aimed = look_dir(yaw, pitch).dot(to.normalize_or_zero()) > 0.96;
        if visible && aimed && to.length() < super::combat::PICKAXE_REACH - 0.35 {
            it.fire = true;
        }
    }

    fn act_fight(&mut self, g: &mut Game, i: usize, dt: f32, it: &mut Intent) {
        let Some(eid) = self.enemy else {
            self.set_goal(Goal::Idle);
            return;
        };
        let (a, e) = (&g.actors[i], &g.actors[eid]);
        if !e.alive {
            self.enemy = None;
            self.set_goal(Goal::Idle);
            return;
        }
        let eye = a.eye_pos();
        let visible = self.visible;
        // what we aim at: the real target while visible, otherwise where we last saw it
        let target_pos = if visible { if self.head_bias { e.head_pos() } else { e.chest() } } else { self.enemy_pos };
        let to = target_pos - eye;
        let dist = to.length();
        let dist_xz = xz(to).length();
        let e_speed = if visible { e.speed_xz() } else { 0.0 };
        let e_y = e.pos.y;

        // nothing left to shoot with
        if !is_armed(a) {
            self.brawl(g, i, dt, it, eid);
            return;
        }

        // ---- weapon selection --------------------------------------------------------------
        let busy = matches!(a.action, Action::Swap { .. } | Action::Reload { .. });
        if let Some(best) = self.pick_weapon(a, dist) {
            if a.inv.selected != best && !busy && a.fire_cd <= 0.0 {
                it.select = Some(best);
            }
        }
        let Some((kind, _rarity, ammo)) = a.inv.selected_weapon() else {
            // holding a pickaxe/heal item/build mode while fighting: the select above fixes it next tick
            if a.build_mode {
                it.toggle_build = true;
            }
            return;
        };
        let def = kind.def();
        let reserve = a.inv.ammo[def.ammo.index()];

        // ---- aim error: a slowly wandering offset in metres at the target ---------------------------
        self.aim_off_t -= dt;
        if self.aim_off_t <= 0.0 {
            self.aim_off_t = self.rng.range(0.25, 0.6);
            let m = self.skill.aim_m * (1.0 + e_speed * 0.12) * (0.6 + (dist / 80.0).min(1.0) * 0.6);
            let v = self.rng.in_disc(m);
            self.aim_off_goal = v;
            self.head_bias = self.rng.chance(0.1 + 0.25 * (1.0 - (self.skill.aim_m / 1.7).min(1.0)));
        }
        self.aim_off = self.aim_off.lerp(self.aim_off_goal, damp(5.0, dt));
        let right = yaw_right(a.yaw);
        let lead = if def.projectile_speed > 0.0 { e.vel * (dist / def.projectile_speed.max(1.0)) * 0.7 } else { Vec3::ZERO };
        let aim_point = target_pos + right * self.aim_off.x + Vec3::Y * self.aim_off.y + if visible { lead } else { Vec3::ZERO };
        let want_yaw = yaw_to(eye, aim_point);
        let want_pitch = pitch_to(eye, aim_point);
        it.yaw = turn_toward(a.yaw, want_yaw, self.skill.turn, dt);
        it.pitch = turn_toward(a.pitch, want_pitch, self.skill.turn, dt);
        // aim error for the trigger decision
        let ang_err = {
            let cur = look_dir(a.yaw, a.pitch);
            let want = (aim_point - eye).normalize_or_zero();
            cur.dot(want).clamp(-1.0, 1.0).acos()
        };
        let hit_tol = (0.5 / dist.max(1.0)).atan().clamp(0.02, 0.12) * if kind == WeaponKind::Shotgun { 1.8 } else { 1.0 };

        // ---- movement -------------------------------------------------------------------------------------
        self.strafe_t -= dt;
        if self.strafe_t <= 0.0 {
            self.strafe_t = self.rng.range(0.6, 1.7);
            if self.rng.chance(0.75) {
                self.strafe_sign = -self.strafe_sign;
            }
        }
        let want_range = preferred_range(kind) * lerp(1.15, 0.75, self.skill.aggression);
        let fwd2 = xz(to).normalize_or_zero();
        let side = Vec2::new(-fwd2.y, fwd2.x) * self.strafe_sign;
        let mut wish = Vec2::ZERO;
        let hold = self.hold_t > 0.0;
        if hold {
            // sitting behind cover: stay put (crouched) and patch up
            it.crouch = true;
            if needs_heal(a) && matches!(a.action, Action::None) && !visible {
                self.set_goal(Goal::Heal);
                return;
            }
        } else if visible {
            let radial = if dist_xz > want_range * 1.35 {
                1.0
            } else if dist_xz < want_range * 0.6 {
                -1.0
            } else {
                0.0
            };
            wish = fwd2 * radial + side * (0.9 - 0.35 * radial.abs());
            wish = wish.normalize_or_zero();
            // snipers hold still
            if kind == WeaponKind::Sniper && dist > 45.0 {
                wish = Vec2::ZERO;
            }
        } else if self.seen_t > 0.8 {
            // lost sight: advance toward where the enemy was
            let (w, arrived) = self.walk_to(g, i, self.enemy_pos, 4.0);
            wish = w;
            if arrived && self.seen_t > 3.0 {
                self.enemy = None;
            }
        }
        let a = &g.actors[i];
        wish = self.unstick(a, wish, wish.length() > 0.1, dt, it);
        it.wish = wish;
        // run when closing a long distance, otherwise fight on foot
        it.sprint = !visible && dist > 20.0;

        // occasional hops while strafing (but not while shooting accurately)
        if visible && !hold && self.jump_cd <= 0.0 && dist > 9.0 && ammo > 0 && self.burst_t <= 0.0 && self.rng.chance(0.03 + 0.04 * self.skill.aggression) {
            it.jump = true;
            self.jump_cd = self.rng.range(1.0, 2.8);
        }

        // ---- building: cover and high ground ---------------------------------------------------------------
        let total = a.total_hp();
        let mats = a.inv.mats[a.build_mat.index()];
        if visible && self.cover_cd <= 0.0 && mats >= 10 && self.hurt_t < 1.5 && total < 75.0 && self.rng.chance(0.03 + 0.12 * self.skill.builder) && self.skill.builder > 0.25 && a.mode == MoveMode::Ground {
            self.job = Job::Cover { stage: 0, t: 0.0 };
            return;
        }
        let dy = e_y - a.pos.y;
        if visible && self.ramp_cd <= 0.0 && mats >= 40 && dy > 4.5 && dist_xz < 32.0 && self.skill.builder > 0.45 && self.rng.chance(0.2) {
            self.job = Job::Ramp { t: 0.0, placed: 0 };
            return;
        }

        // ---- firing ---------------------------------------------------------------------------------------------
        let reloading = matches!(a.action, Action::Reload { .. });
        let swapping = matches!(a.action, Action::Swap { .. });
        self.react_t = (self.react_t - dt).max(0.0);
        self.burst_t = (self.burst_t - dt).max(0.0);
        self.pause_t = (self.pause_t - dt).max(0.0);
        self.tap_cd = (self.tap_cd - dt).max(0.0);
        // aim down sights at range, or when scoped
        self.use_ads = visible && (kind == WeaponKind::Sniper || (dist > 22.0 && kind != WeaponKind::Shotgun) || (self.skill.builder > 0.7 && dist > 14.0 && kind != WeaponKind::Shotgun));
        it.ads = self.use_ads && !reloading;

        if !reloading && ammo == 0 {
            if reserve > 0 {
                it.reload = true;
            } else {
                // dry: swap away next think tick
            }
            return;
        }
        if visible && self.react_t <= 0.0 && !reloading && !swapping && ammo > 0 && dist < def.range * 0.95 && a.fire_cd <= 0.0 {
            let ready = ang_err < hit_tol * 1.6 + 0.01 && !(kind == WeaponKind::Sniper && a.speed_xz() > 1.5);
            // explosives are not for point-blank use
            let safe = kind != WeaponKind::RocketLauncher || dist > 8.0;
            if ready && safe {
                if def.auto {
                    if self.pause_t <= 0.0 {
                        if self.burst_t <= 0.0 {
                            self.burst_t = self.rng.range(0.45, 1.5);
                            it.fire_pressed = true;
                        }
                        it.fire = true;
                        if self.burst_t <= dt * 1.5 {
                            self.pause_t = self.rng.range(0.12, 0.5) * (1.4 - self.skill.aggression * 0.5);
                        }
                    }
                } else if self.tap_cd <= 0.0 {
                    it.fire = true;
                    it.fire_pressed = true;
                    self.tap_cd = match kind {
                        WeaponKind::Pistol => self.rng.range(0.12, 0.38),
                        _ => 0.0,
                    };
                }
            }
        } else if !visible || self.react_t > 0.0 {
            // top up while we have a moment (not while being shot at point blank)
            if ammo < def.mag / 3 && reserve > 0 && !reloading && self.seen_t > 1.5 {
                it.reload = true;
            }
        }
        // standing target with a crouch for accuracy at range
        if visible && dist > 45.0 && kind != WeaponKind::Shotgun && wish == Vec2::ZERO {
            it.crouch = true;
        }
    }
}

const TAU_F: f32 = std::f32::consts::TAU;

#[allow(dead_code)]
fn _tags(_: Tag) {}

#[cfg(test)]
mod tests {
    use super::super::testutil::{game, game_cfg};
    use super::super::*;
    use super::*;

    #[test]
    fn bots_ride_the_bus_jump_glide_and_land_on_the_island() {
        let mut g = game(24, false);
        let inp = PlayerInput::default();
        let mut jumped = 0usize;
        let mut t = 0.0;
        while t < 90.0 {
            g.update(0.05, &inp);
            t += 0.05;
            jumped = g.actors.iter().skip(1).filter(|a| a.mode != MoveMode::Bus).count();
            if t > 40.0 && g.actors.iter().skip(1).all(|a| matches!(a.mode, MoveMode::Ground | MoveMode::Swim | MoveMode::Dead)) {
                break;
            }
        }
        assert!(jumped >= 22, "{jumped} bots left the bus");
        let landed = g.actors.iter().skip(1).filter(|a| matches!(a.mode, MoveMode::Ground | MoveMode::Swim | MoveMode::Dead)).count();
        assert!(landed >= 22, "{landed} bots landed after {t}s");
        let standing = g.actors.iter().skip(1).filter(|a| a.mode == MoveMode::Ground).count();
        assert!(standing >= 12, "{standing} bots standing on the island");
        // they spread out over several places rather than all dropping in one spot
        let mut cells = std::collections::HashSet::new();
        for a in g.actors.iter().skip(1) {
            cells.insert(((a.pos.x / 120.0).floor() as i32, (a.pos.z / 120.0).floor() as i32));
        }
        assert!(cells.len() >= 4, "bots landed in {} regions", cells.len());
    }

    fn soak(bots: usize, seconds: f32, speed: f32) -> Game {
        let mut g = game_cfg(GameConfig { bots, skip_bus: true, seed: 11, storm_speed: speed, god_mode: true, ..Default::default() });
        let inp = PlayerInput::default();
        let mut t = 0.0;
        while t < seconds && g.phase != Phase::Over {
            g.update(1.0 / 30.0, &inp);
            t += 1.0 / 30.0;
        }
        g
    }

    /// A bot with a far-away destination, started at `at` (a point on the seed-1234 island), for `secs` seconds.
    /// Returns the farthest it got from where it began.
    fn farthest_walk(at: Vec2, secs: f32) -> f32 {
        let mut g = game_cfg(GameConfig { bots: 1, skip_bus: true, seed: 3, god_mode: true, storm_speed: 1.0, ..Default::default() });
        g.pickups.clear();
        g.chests.clear();
        // the human stands far away on dry ground, out of sight
        let (px, pz) = (-380.0, 380.0);
        g.actors[PLAYER].pos = Vec3::new(px, g.world.hm.height_at(px, pz), pz);
        g.actors[PLAYER].brain = None;
        let h = g.world.hm.height_at(at.x, at.y);
        {
            let a = &mut g.actors[1];
            a.pos = Vec3::new(at.x, h, at.y);
            a.mode = MoveMode::Ground;
            a.on_ground = true;
            a.inv.add_weapon(WeaponKind::AssaultRifle, Rarity::Rare, 30);
            a.inv.ammo[AmmoKind::Medium.index()] = 100;
        }
        // the safe circle lies 300 m away: the bot has to get going
        g.storm.center = Vec2::new(at.x + 300.0, at.y);
        g.storm.radius = 150.0;
        g.storm.to_center = g.storm.center;
        g.storm.to_radius = 120.0;
        g.storm.from_center = g.storm.center;
        g.storm.from_radius = 150.0;
        g.storm.dmg = 0.0;
        let start = g.actors[1].pos;
        let mut farthest = 0.0f32;
        for _ in 0..(secs * 30.0) as usize {
            g.update(1.0 / 30.0, &PlayerInput::default());
            farthest = farthest.max(g.actors[1].pos.distance(start));
        }
        farthest
    }

    #[test]
    fn bots_against_a_house_wall_or_in_a_door_alley_still_get_away() {
        // (house index, side of its footprint, started 0.2 m inside the footprint margin / 3 m in front of it).
        // Standing under the eaves the bot used to count as "inside the house" and was routed through the door into the wall,
        // and from a blocked navigation cell the closest free cell was the one inside the house; in the alley between two
        // facing doors every cell around was sealed off, so the closest reachable cell led back into the house.
        let cases = [(47, 0, true), (3, 0, true), (3, 2, true), (3, 3, true), (13, 1, true), (25, 3, true), (3, 1, false)];
        let w = crate::world::World::generate(1234);
        let mut spots = vec![];
        for (bi, side, under_eaves) in cases {
            let b = &w.buildings[bi];
            assert_eq!(b.kind, "house", "building {bi}");
            let c = Vec2::new((b.aabb.min.x + b.aabb.max.x) * 0.5, (b.aabb.min.z + b.aabb.max.z) * 0.5);
            let at = match (side, under_eaves) {
                (0, true) => Vec2::new(c.x, b.aabb.min.z + 0.2),
                (1, true) => Vec2::new(c.x, b.aabb.max.z - 0.2),
                (2, true) => Vec2::new(b.aabb.min.x + 0.2, c.y),
                (3, true) => Vec2::new(b.aabb.max.x - 0.2, c.y),
                (0, false) => Vec2::new(c.x, b.aabb.min.z - 3.0),
                (1, false) => Vec2::new(c.x, b.aabb.max.z + 3.0),
                (2, false) => Vec2::new(b.aabb.min.x - 3.0, c.y),
                _ => Vec2::new(b.aabb.max.x + 3.0, c.y),
            };
            spots.push((bi, side, under_eaves, at));
        }
        drop(w);
        for (bi, side, under_eaves, at) in spots {
            let far = farthest_walk(at, 45.0);
            assert!(far > 40.0, "house {bi}, side {side}, {}: the bot got no further than {far:.1} m in 45 s", if under_eaves { "under the eaves" } else { "in front of the door" });
        }
    }

    #[test]
    fn bots_with_no_ammo_left_brawl_with_the_pickaxe_instead_of_standing_around() {
        // the last two players, both holding empty guns with nothing to feed them, in plain sight of each other:
        // they used to idle until the storm took them, because only armed bots ever chose to fight
        let mut g = game_cfg(GameConfig { bots: 2, skip_bus: true, seed: 5, storm_speed: 1.0, ..Default::default() });
        g.actors[PLAYER].alive = false;
        g.actors[PLAYER].mode = MoveMode::Dead;
        g.pickups.clear();
        g.chests.clear();
        g.storm.active = false;
        let spot = super::super::testutil::free_spot(&g);
        for (k, dx) in [(1, -6.0), (2, 6.0)] {
            let (x, z) = (spot.x + dx, spot.z);
            let h = g.world.hm.height_at(x, z);
            let a = &mut g.actors[k];
            a.pos = Vec3::new(x, h, z);
            a.mode = MoveMode::Ground;
            a.on_ground = true;
            a.inv.add_weapon(WeaponKind::AssaultRifle, Rarity::Common, 0);
            a.inv.ammo = [0; 5];
            a.inv.selected = 1;
        }
        for _ in 0..(120 * 30) {
            g.update(1.0 / 30.0, &PlayerInput::default());
            if g.phase == Phase::Over {
                break;
            }
        }
        let dealt = g.actors[1].damage_dealt + g.actors[2].damage_dealt;
        assert!(dealt > 0.0, "two unarmed bots in plain sight never touched each other in 120 s (phase {:?})", g.phase);
        assert!(g.phase == Phase::Over, "and the fight finishes: bot 1 hp {} bot 2 hp {}", g.actors[1].hp, g.actors[2].hp);
    }

    #[test]
    fn bots_loot_up_and_fight_each_other() {
        let g = soak(30, 150.0, 4.0);
        let armed = g.actors.iter().skip(1).filter(|a| a.alive && a.inv.weapon_count() > 0).count();
        let alive = g.actors.iter().skip(1).filter(|a| a.alive).count();
        let kills: u32 = g.actors.iter().map(|a| a.kills).sum();
        let dealt: f32 = g.actors.iter().map(|a| a.damage_dealt).sum();
        eprintln!("soak: alive {alive}, armed {armed}, kills {kills}, damage dealt {dealt:.0}, pieces {}", g.pieces.count());
        assert!(armed * 10 >= alive * 8, "most surviving bots should find a weapon: {armed}/{alive}");
        assert!(dealt > 300.0, "bots should be shooting each other: {dealt}");
    }
}
