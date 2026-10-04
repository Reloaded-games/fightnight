//! Combat: firing, melee and harvesting, projectiles, damage, eliminations, healing and reloading.

use super::actor::*;
use super::env::*;
use super::events::*;
use super::intent::Intent;
use super::items::*;
use super::*;
use crate::math::*;
use crate::world::collision::Tag;
use crate::world::props::HarvestKind;

pub const PICKAXE_REACH: f32 = 2.7;
pub const PICKAXE_COOLDOWN: f32 = 0.55;
pub const PICKAXE_DAMAGE: f32 = 20.0;
pub const HARVEST_DAMAGE: f32 = 25.0;
pub const SWING_DELAY: f32 = 0.17;

#[derive(Clone, Copy, Debug)]
pub enum TraceHit {
    None,
    Env(RayHit),
    Actor { idx: usize, head: bool },
}

#[derive(Clone, Copy, Debug)]
pub struct Trace {
    pub t: f32,
    pub hit: TraceHit,
    pub point: Vec3,
}

/// Share of an explosion's damage and knock-back that reaches a target hidden behind a wall or a hill.
pub const SPLASH_COVER: f32 = 0.25;

pub fn noise_radius(kind: WeaponKind) -> f32 {
    match kind {
        WeaponKind::Pistol => 60.0,
        WeaponKind::Smg => 70.0,
        WeaponKind::AssaultRifle => 90.0,
        WeaponKind::Shotgun => 100.0,
        WeaponKind::Sniper => 150.0,
        WeaponKind::RocketLauncher => 120.0,
    }
}

fn can_use(kind: ConsumableKind, a: &Actor) -> bool {
    let d = kind.def();
    (d.heal > 0.0 && a.hp < d.max_hp - 0.01) || (d.shield > 0.0 && a.shield < d.max_shield - 0.01)
}

impl Game {
    /// Where a shot starts and which way it points (shoulder for the human, eye for bots).
    pub fn shot_ray(&self, idx: usize) -> (Vec3, Vec3) {
        let a = &self.actors[idx];
        if a.human {
            self.aim_ray(idx)
        } else {
            (a.eye_pos(), look_dir(a.yaw, a.pitch))
        }
    }

    /// Where the weapon's muzzle is right now (tracers and flashes start here).
    pub fn muzzle_pos(&self, idx: usize) -> Vec3 {
        let a = &self.actors[idx];
        if a.inv.selected_weapon().is_some() && matches!(a.mode, MoveMode::Ground | MoveMode::Swim) {
            let m = rig::pose(a).muzzle;
            if m.is_finite() {
                return m;
            }
        }
        let dir = look_dir(a.yaw, a.pitch);
        let right = yaw_right(a.yaw);
        a.eye_pos() + right * 0.30 - Vec3::Y * 0.32 + dir * 0.9
    }

    /// Cast a ray against the environment and all actors except `shooter`.
    pub fn trace(&self, shooter: usize, origin: Vec3, dir: Vec3, range: f32) -> Trace {
        let env = Env::new(&self.world, &self.pieces.grid);
        let mut best_t = range;
        let mut hit = TraceHit::None;
        if let Some(h) = env.raycast(origin, dir, range, true) {
            best_t = h.t;
            hit = TraceHit::Env(h);
        }
        for a in &self.actors {
            if !a.alive || a.id == shooter || a.mode == MoveMode::Bus {
                continue;
            }
            // where this actor is, or was on the shooter's screen (see `Game::rewound`)
            let pos = self.rewound.as_ref().and_then(|r| r.get(a.id)).copied().unwrap_or(a.pos);
            // quick reject by distance to the ray
            let to = pos + Vec3::Y * (a.height() * 0.62) - origin;
            let along = to.dot(dir);
            if along < -1.0 || along > best_t + 2.0 {
                continue;
            }
            if (to - dir * along).length() > 2.2 {
                continue;
            }
            let base = pos + Vec3::Y * 0.33;
            // the body capsule stops at the neck so the head sphere owns everything above the shoulders
            let top = pos + Vec3::Y * (a.height() - 0.62);
            let head = ray_sphere(origin, dir, pos + Vec3::Y * (a.height() - 0.2), 0.23);
            let body = ray_capsule_y(origin, dir, base, top, 0.33);
            match (head, body) {
                (Some(th), Some(tb)) if th <= tb + 0.05 && th < best_t => {
                    best_t = th;
                    hit = TraceHit::Actor { idx: a.id, head: true };
                }
                (Some(th), None) if th < best_t => {
                    best_t = th;
                    hit = TraceHit::Actor { idx: a.id, head: true };
                }
                (_, Some(tb)) if tb < best_t => {
                    best_t = tb;
                    hit = TraceHit::Actor { idx: a.id, head: false };
                }
                _ => {}
            }
        }
        Trace { t: best_t, hit, point: origin + dir * best_t }
    }

    // ------------------------------------------------------------------------------
    // The per-tick item handler: selection, build mode, interaction, reload, fire, heal
    // ------------------------------------------------------------------------------

    pub fn handle_items(&mut self, i: usize, it: &Intent, dt: f32) {
        // ---- timers -----------------------------------------------------------
        {
            let a = &mut self.actors[i];
            a.fire_cd = (a.fire_cd - dt).max(0.0);
            let decay = if a.speed_xz() < 0.5 { 1.7 } else { 1.0 } * if a.ads { 1.4 } else { 1.0 };
            a.bloom = (a.bloom - decay * dt).max(0.0);
            if a.recoil_kick > 0.0 {
                let r = a.recoil_kick.min(dt * (0.5 + a.recoil_kick * 4.0));
                a.recoil_kick -= r;
                if a.human {
                    a.pitch -= r;
                }
            }
        }
        let swim = self.actors[i].mode == MoveMode::Swim;

        // ---- selection ------------------------------------------------------------
        if let Some(s) = it.select {
            if s < 6 && self.actors[i].inv.slots[s].is_some() {
                self.select_slot(i, s);
            }
        }
        if it.cycle != 0 {
            let mut s = self.actors[i].inv.selected as i32;
            for _ in 0..6 {
                s = (s + it.cycle).rem_euclid(6);
                if self.actors[i].inv.slots[s as usize].is_some() {
                    break;
                }
            }
            self.select_slot(i, s as usize);
        }

        // ---- emote -------------------------------------------------------------------------
        {
            let a = &mut self.actors[i];
            let calm = a.alive && a.mode == MoveMode::Ground && a.on_ground && !a.build_mode && matches!(a.action, Action::None);
            if it.emote && calm {
                a.emoting = !a.emoting;
                a.anim.emote_clock = 0.0;
            }
            // anything else the actor does ends the dance
            let busy = it.wish.length_squared() > 0.01 || it.jump || it.fire || it.reload || it.crouch || it.toggle_build || it.interact || it.select.is_some() || it.cycle != 0 || it.piece.is_some() || it.drop_selected;
            if a.emoting && (!calm || busy) {
                a.emoting = false;
            }
        }

        // ---- building ------------------------------------------------------------------
        self.handle_building(i, it);
        let building = self.actors[i].build_mode;

        if it.drop_selected && !building {
            self.drop_selected(i);
        }
        if it.interact {
            self.interact(i);
        }

        // ---- ADS ------------------------------------------------------------------------
        {
            let a = &mut self.actors[i];
            let has_weapon = a.inv.selected_weapon().is_some();
            let reloading = matches!(a.action, Action::Reload { .. });
            a.ads = it.ads && has_weapon && !building && !swim && !reloading && matches!(a.mode, MoveMode::Ground);
        }

        // ---- action progress ---------------------------------------------------------------
        let action = self.actors[i].action;
        match action {
            Action::Swap { t } => {
                let t = t - dt;
                self.actors[i].action = if t <= 0.0 { Action::None } else { Action::Swap { t } };
            }
            Action::Reload { t, dur } => {
                let t = t + dt;
                if t >= dur {
                    self.finish_reload(i);
                } else {
                    self.actors[i].action = Action::Reload { t, dur };
                }
            }
            Action::Heal { slot, t, dur } => {
                // cancelled when the item is no longer selected
                if self.actors[i].inv.selected != slot || !matches!(self.actors[i].inv.slots[slot], Some(Item::Consumable { .. })) || building {
                    self.actors[i].action = Action::None;
                } else {
                    let t = t + dt;
                    if t >= dur {
                        self.finish_heal(i, slot);
                    } else {
                        self.actors[i].action = Action::Heal { slot, t, dur };
                    }
                }
            }
            Action::None => {}
        }

        // ---- melee timer -------------------------------------------------------------------
        if self.actors[i].melee_pending {
            self.actors[i].melee_t -= dt;
            if self.actors[i].melee_t <= 0.0 {
                self.actors[i].melee_pending = false;
                self.melee_hit(i);
            }
        }
        if building || swim {
            return;
        }

        // ---- reload / fire ---------------------------------------------------------------------
        let sel = self.actors[i].inv.selected_item().copied();
        let busy = matches!(self.actors[i].action, Action::Swap { .. } | Action::Reload { .. } | Action::Heal { .. });
        match sel {
            Some(Item::Weapon { kind, rarity, ammo }) => {
                let def = kind.def();
                let reserve = self.actors[i].inv.ammo[def.ammo.index()];
                let reloading = matches!(self.actors[i].action, Action::Reload { .. });
                let swapping = matches!(self.actors[i].action, Action::Swap { .. });
                if it.reload && !busy && ammo < def.mag && reserve > 0 {
                    self.start_reload(i);
                }
                let wants_fire = if def.auto { it.fire } else { it.fire_pressed };
                if wants_fire && !reloading && !swapping && self.actors[i].fire_cd <= 0.0 {
                    if ammo == 0 {
                        if reserve > 0 {
                            self.start_reload(i);
                        } else if it.fire_pressed {
                            let pos = self.actors[i].pos;
                            self.events.push(Event::EmptyClick { actor: i, pos });
                            self.actors[i].fire_cd = 0.25;
                        }
                    } else {
                        self.fire_weapon(i, kind, rarity);
                    }
                }
            }
            Some(Item::Pickaxe) => {
                let swapping = matches!(self.actors[i].action, Action::Swap { .. });
                if it.fire && self.actors[i].fire_cd <= 0.0 && !swapping && !self.actors[i].melee_pending {
                    let a = &mut self.actors[i];
                    a.fire_cd = PICKAXE_COOLDOWN;
                    a.melee_t = SWING_DELAY;
                    a.melee_pending = true;
                    a.anim.swing = 1.0;
                    a.shot_flash = 0.3;
                    self.events.push(Event::Swing { actor: i });
                }
            }
            Some(Item::Consumable { kind, .. }) => {
                let a = &self.actors[i];
                if it.fire_pressed && matches!(a.action, Action::None) && can_use(kind, a) {
                    let slot = a.inv.selected;
                    let dur = kind.def().use_time;
                    let pos = a.pos;
                    self.actors[i].action = Action::Heal { slot, t: 0.0, dur };
                    self.events.push(Event::HealStart { actor: i, pos });
                }
            }
            None => {}
        }
    }

    fn start_reload(&mut self, i: usize) {
        let Some((kind, rarity, _)) = self.actors[i].inv.selected_weapon() else { return };
        let dur = kind.def().reload * rarity.reload_mul();
        let pos = self.actors[i].pos;
        self.actors[i].action = Action::Reload { t: 0.0, dur };
        self.actors[i].ads = false;
        self.events.push(Event::Reload { actor: i, pos, kind });
    }

    fn finish_reload(&mut self, i: usize) {
        let a = &mut self.actors[i];
        a.action = Action::None;
        let sel = a.inv.selected;
        if let Some(Item::Weapon { kind, ammo, .. }) = &mut a.inv.slots[sel] {
            let def = kind.def();
            let need = def.mag.saturating_sub(*ammo);
            let take = need.min(a.inv.ammo[def.ammo.index()]);
            *ammo += take;
            a.inv.ammo[def.ammo.index()] -= take;
        }
    }

    fn finish_heal(&mut self, i: usize, slot: usize) {
        let Some(Item::Consumable { kind, .. }) = self.actors[i].inv.slots[slot] else {
            self.actors[i].action = Action::None;
            return;
        };
        let d = kind.def();
        let a = &mut self.actors[i];
        if d.heal > 0.0 {
            a.hp = (a.hp + d.heal).min(d.max_hp.max(a.hp));
        }
        if d.shield > 0.0 {
            a.shield = (a.shield + d.shield).min(d.max_shield.max(a.shield));
        }
        a.action = Action::None;
        a.inv.use_one(slot);
        let pos = a.pos;
        self.events.push(Event::HealDone { actor: i, pos });
        // keep drinking while more is needed and the player holds the button? (one item at a time)
    }

    // ------------------------------------------------------------------------------
    // Weapons
    // ------------------------------------------------------------------------------

    fn fire_weapon(&mut self, i: usize, kind: WeaponKind, rarity: Rarity) {
        let def = kind.def();
        let (origin, aim) = self.shot_ray(i);
        let muzzle = self.muzzle_pos(i);
        // ---- spread ---------------------------------------------------------------
        let spread_deg = {
            let a = &self.actors[i];
            let base = if a.ads { def.spread_ads } else { def.spread_hip };
            let mut s = base * (1.0 + a.bloom);
            if !a.ads {
                s *= 1.0 + 0.6 * (a.speed_xz() / movement::RUN_SPEED).clamp(0.0, 1.0);
            }
            if !a.on_ground {
                s *= 1.8;
            } else if a.crouching {
                s *= 0.75;
            }
            s
        };
        let spread = spread_deg * DEG;
        // ---- consume ammo / set cooldown ---------------------------------------------
        {
            let a = &mut self.actors[i];
            let sel = a.inv.selected;
            if let Some(Item::Weapon { ammo, .. }) = &mut a.inv.slots[sel] {
                *ammo = ammo.saturating_sub(1);
            }
            a.fire_cd = 1.0 / def.rate;
            a.bloom = (a.bloom + def.bloom_per_shot).min(3.0);
            a.shot_flash = 0.14;
            a.anim.recoil = 1.0;
            let kick = def.recoil * DEG;
            a.recoil_kick += kick;
            if a.human {
                a.pitch = (a.pitch + kick).min(1.5);
            }
        }
        let pos = self.actors[i].pos;
        self.events.push(Event::Noise { pos, radius: noise_radius(kind), source: i });

        // ---- projectile weapons ---------------------------------------------------------
        if def.projectile_speed > 0.0 {
            let tr = self.trace(i, origin, aim, def.range);
            let target = tr.point;
            let dir = (target - muzzle).normalize_or_zero();
            let dir = if dir == Vec3::ZERO { aim } else { dir };
            // a little spread for rockets too
            let dir = perturb(dir, spread * 0.5, &mut self.rng);
            self.projectiles.push(Projectile { pos: muzzle, vel: dir * def.projectile_speed, owner: i, kind, rarity, life: 6.0 });
            self.events.push(Event::Shot { actor: i, pos: muzzle, weapon: kind, end: target, hit_actor: false });
            return;
        }

        // ---- hitscan ------------------------------------------------------------------------
        let mut any_actor = false;
        let mut first_end = origin + aim * def.range;
        for p in 0..def.pellets {
            let dir = if spread > 0.0 { perturb(aim, spread, &mut self.rng) } else { aim };
            let tr = self.trace(i, origin, dir, def.range);
            if p == 0 {
                first_end = tr.point;
            }
            self.events.push(Event::Tracer { from: muzzle, to: tr.point, weapon: kind });
            let falloff = {
                let d = tr.t;
                if d <= def.falloff_start || def.range <= def.falloff_start {
                    1.0
                } else {
                    lerp(1.0, def.falloff_min, ((d - def.falloff_start) / (def.range - def.falloff_start)).clamp(0.0, 1.0))
                }
            };
            let base = def.damage * rarity.damage_mul() * falloff;
            match tr.hit {
                TraceHit::Actor { idx, head } => {
                    any_actor = true;
                    let dmg = if head { base * def.head_mult } else { base };
                    self.damage_actor(idx, dmg, Some(i), def.name, head, tr.point, false);
                }
                TraceHit::Env(h) => {
                    let kind_hit = self.impact_kind(h.tag);
                    self.events.push(Event::Impact { pos: tr.point, normal: h.normal, kind: kind_hit });
                    if let Some(Tag::Piece(id)) = h.tag {
                        self.damage_piece(id, base * 0.6, Some(i));
                    }
                }
                TraceHit::None => {}
            }
        }
        self.events.push(Event::Shot { actor: i, pos: muzzle, weapon: kind, end: first_end, hit_actor: any_actor });
    }

    fn impact_kind(&self, tag: Option<Tag>) -> ImpactKind {
        match tag {
            None => ImpactKind::Dirt,
            Some(Tag::Tree(_)) => ImpactKind::Wood,
            Some(Tag::Rock(_)) => ImpactKind::Stone,
            Some(Tag::Piece(id)) => match self.pieces.get(id).map(|p| p.mat) {
                Some(Mat::Wood) => ImpactKind::Wood,
                Some(Mat::Stone) => ImpactKind::Stone,
                _ => ImpactKind::Metal,
            },
            Some(Tag::Building(_)) => ImpactKind::Stone,
            _ => ImpactKind::Stone,
        }
    }

    // ------------------------------------------------------------------------------
    // Pickaxe
    // ------------------------------------------------------------------------------

    fn melee_hit(&mut self, i: usize) {
        let (origin, dir) = self.shot_ray(i);
        // The aim ray starts at the shoulder (where the crosshair points), so reach is measured from
        // there. A small fan of rays makes swings forgiving: the crosshair rarely sits exactly on a trunk.
        let side = dir.cross(Vec3::Y).normalize_or_zero();
        let mut tr = self.trace(i, origin, dir, PICKAXE_REACH);
        if matches!(tr.hit, TraceHit::None) {
            let env = self.env();
            for off in [side * 0.32, -side * 0.32, Vec3::Y * 0.4, -Vec3::Y * 0.4] {
                // a fan ray must not start on the far side of a wall
                let len = off.length();
                if len > 1e-3 && env.probe(origin, off / len, len).is_some() {
                    continue;
                }
                let t = self.trace(i, origin + off, dir, PICKAXE_REACH);
                if !matches!(t.hit, TraceHit::None) && (matches!(tr.hit, TraceHit::None) || t.t < tr.t) {
                    tr = t;
                }
            }
        }
        match tr.hit {
            TraceHit::Actor { idx, head } => {
                let dmg = PICKAXE_DAMAGE * if head { 1.5 } else { 1.0 };
                self.damage_actor(idx, dmg, Some(i), "Harvesting Tool", head, tr.point, false);
            }
            TraceHit::Env(h) => {
                self.events.push(Event::HarvestHit { pos: tr.point, normal: h.normal, wood: matches!(h.tag, Some(Tag::Tree(_))) });
                match h.tag {
                    Some(Tag::Tree(idx)) | Some(Tag::Rock(idx)) => self.harvest(i, idx as usize, tr.point),
                    Some(Tag::Piece(id)) => {
                        self.damage_piece(id, 75.0, Some(i));
                    }
                    _ => {}
                }
            }
            TraceHit::None => {}
        }
    }

    pub(crate) fn harvest(&mut self, who: usize, idx: usize, point: Vec3) {
        let Some(h) = self.world.harvest.get_mut(idx) else { return };
        if !h.alive {
            return;
        }
        h.hp -= HARVEST_DAMAGE;
        let (mat, per_hit) = match h.kind {
            HarvestKind::Tree => (Mat::Wood, 18 + (self.rng.below(7) as u32)),
            HarvestKind::Rock => (Mat::Stone, 14 + (self.rng.below(6) as u32)),
            HarvestKind::Ore => (Mat::Metal, 12 + (self.rng.below(6) as u32)),
        };
        let mut amount = per_hit;
        let broke = h.hp <= 0.0;
        let (pos, chunk, slot, collider, kind, prop_kind) = (h.pos, h.chunk as usize, h.slot as usize, h.collider, h.kind, h.prop_kind);
        if broke {
            h.alive = false;
            amount += match kind {
                HarvestKind::Tree => 30,
                HarvestKind::Rock => 25,
                HarvestKind::Ore => 22,
            };
        }
        self.actors[who].inv.add_mats(mat, amount);
        self.events.push(Event::Harvest { actor: who, pos: point, mat, amount });
        if broke {
            self.world.statics.remove(collider);
            self.events.push(Event::TreeFelled { pos, chunk, slot });
            let from = self.actors[who].pos;
            let away = Vec2::new(pos.x - from.x, pos.z - from.z).normalize_or_zero();
            self.harvest_log.push((idx as u32, away));
            if kind == HarvestKind::Tree {
                self.start_felling(pos, chunk, slot, prop_kind, away);
            }
        }
    }

    // ------------------------------------------------------------------------------
    // Damage
    // ------------------------------------------------------------------------------

    /// A tree that was just cut down starts to fall away from `dir`.
    pub fn start_felling(&mut self, pos: Vec3, chunk: usize, slot: usize, kind: crate::world::props::PropKind, dir: Vec2) {
        if let Some(p) = self.world.chunk_props.get(chunk).and_then(|c| c.get(slot)) {
            self.felled.push(Felled { pos, kind, scale: p.scale, yaw: p.yaw, tint: p.tint, t: 0.0, dir });
        }
    }

    pub fn damage_piece(&mut self, id: u32, amount: f32, _by: Option<usize>) {
        let pos_mat = self.pieces.get(id).map(|p| (shape_center(p), p.mat));
        if self.pieces.damage(id, amount).is_some() {
            if let Some((pos, mat)) = pos_mat {
                self.events.push(Event::PieceDestroyed { pos, mat });
            }
        }
    }

    /// Apply damage to an actor: shield first (unless bypassed), then health.
    #[allow(clippy::too_many_arguments)]
    pub fn damage_actor(&mut self, victim: usize, amount: f32, attacker: Option<usize>, weapon: &'static str, headshot: bool, hit_pos: Vec3, bypass_shield: bool) -> bool {
        if amount <= 0.0 {
            return false;
        }
        {
            let a = &self.actors[victim];
            if !a.alive || a.mode == MoveMode::Bus {
                return false;
            }
        }
        if self.cfg.god_mode && victim == self.local {
            return false;
        }
        if let Some(at) = attacker {
            // no friendly fire on yourself with bullets (explosions can hurt)
            if at == victim && weapon != "Rocket Launcher" {
                return false;
            }
        }
        let (on_shield, hp_after, dealt);
        {
            let a = &mut self.actors[victim];
            // what this hit can actually take off the victim: overkill is not damage dealt
            dealt = amount.min(a.hp.max(0.0) + if bypass_shield { 0.0 } else { a.shield });
            let mut remain = amount;
            let mut absorbed = 0.0;
            if !bypass_shield && a.shield > 0.0 {
                absorbed = a.shield.min(remain);
                a.shield -= absorbed;
                remain -= absorbed;
            }
            a.hp -= remain;
            a.hit_flash = 1.0;
            a.emoting = false;
            a.last_damage_from = attacker.filter(|&x| x != victim);
            a.last_damage_time = self.time;
            on_shield = absorbed > 0.0 && remain <= 0.0;
            hp_after = a.hp;
        }
        let killed = hp_after <= 0.0;
        let from = attacker.map(|x| self.actors[x].pos);
        self.events.push(Event::Damage { target: victim, attacker, amount, on_shield, headshot, pos: hit_pos });
        if let Some(at) = attacker {
            if at != victim {
                self.actors[at].damage_dealt += dealt;
                if self.actors[at].human {
                    self.events.push(Event::HitConfirm { actor: at, head: headshot, shield: on_shield, kill: killed });
                }
                // the victim notices who hit them (bots react)
                if let Some(b) = self.actors[victim].brain.as_mut() {
                    b.on_damaged(at);
                }
            }
        }
        if self.actors[victim].human {
            self.events.push(Event::Hurt { actor: victim, amount, from });
        }
        if killed {
            self.eliminate(victim, attacker.filter(|&x| x != victim), weapon, weapon == "The Storm");
        }
        killed
    }

    pub fn eliminate(&mut self, victim: usize, killer: Option<usize>, weapon: &'static str, storm: bool) {
        let alive_before = self.alive_count();
        {
            let a = &mut self.actors[victim];
            if !a.alive {
                return;
            }
            a.alive = false;
            a.mode = MoveMode::Dead;
            a.hp = 0.0;
            a.shield = 0.0;
            a.placement = alive_before as u32;
            a.action = Action::None;
            a.ads = false;
            a.dead_time = 0.0;
            a.death_step = self.step_no;
            a.vel = Vec3::ZERO;
            a.build_mode = false;
            a.emoting = false;
        }
        self.drop_inventory(victim);
        if let Some(k) = killer {
            self.actors[k].kills += 1;
        }
        self.actors[victim].survived = self.match_time;
        self.events.push(Event::Eliminated { victim, killer, weapon: Some(weapon), storm });
    }

    pub fn explode(&mut self, pos: Vec3, owner: usize, kind: WeaponKind, rarity: Rarity) {
        let def = kind.def();
        let radius = def.explosion_radius;
        self.events.push(Event::Explosion { pos, radius });
        self.events.push(Event::Noise { pos, radius: 140.0, source: owner });
        let base = def.damage * rarity.damage_mul();
        // (actor, damage, push direction, knock-back strength, chest) for everyone the blast reaches; walls and hills between
        // the blast and a target soak most of it, so building cover works against rockets too
        let mut hits = Vec::new();
        {
            let env = Env::new(&self.world, &self.pieces.grid);
            for (j, a) in self.actors.iter().enumerate() {
                if !a.alive || a.mode == MoveMode::Bus {
                    continue;
                }
                let chest = a.chest();
                let d = chest.distance(pos);
                if d >= radius + 0.4 {
                    continue;
                }
                let cover = if env.line_clear(pos, chest) { 1.0 } else { SPLASH_COVER };
                let k = (1.0 - d / (radius + 0.4)).clamp(0.0, 1.0) * cover;
                let mut dmg = base * (0.15 * cover + 0.85 * k);
                if j == owner {
                    dmg *= 0.5;
                }
                hits.push((j, dmg, (chest - pos).normalize_or_zero(), k, chest));
            }
        }
        for (j, dmg, push, k, chest) in hits {
            self.damage_actor(j, dmg, Some(owner), def.name, false, chest, false);
            if self.actors[j].alive && self.actors[j].mode == MoveMode::Ground {
                let a = &mut self.actors[j];
                a.vel += push * 7.0 * k + Vec3::Y * 3.0 * k;
                a.on_ground = false;
            }
        }
        // structures take heavy damage
        let ids: Vec<u32> = self.pieces.iter().filter(|p| shape_center(p).distance(pos) < radius + 2.5).map(|p| p.id).collect();
        for id in ids {
            let c = self.pieces.get(id).map(shape_center).unwrap_or(pos);
            let k = (1.0 - c.distance(pos) / (radius + 2.5)).clamp(0.2, 1.0);
            self.damage_piece(id, 220.0 * k, Some(owner));
        }
    }
}

pub fn shape_center(p: &Piece) -> Vec3 {
    crate::game::pieces::shape_of(&p.key, p.base_y).aabb().center()
}

/// Rotate `dir` by a random angle within `cone` radians (uniform over the disc).
pub fn perturb(dir: Vec3, cone: f32, rng: &mut crate::rng::Rng) -> Vec3 {
    if cone <= 0.0 {
        return dir;
    }
    let up = if dir.y.abs() > 0.95 { Vec3::X } else { Vec3::Y };
    let right = dir.cross(up).normalize();
    let up = right.cross(dir).normalize();
    let ang = cone * rng.f32().sqrt();
    let az = rng.range(0.0, std::f32::consts::TAU);
    (dir * ang.cos() + (right * az.cos() + up * az.sin()) * ang.sin()).normalize()
}

/// Move rockets, detonate on contact.
pub fn update_projectiles(g: &mut Game, dt: f32) {
    let mut i = 0;
    while i < g.projectiles.len() {
        let (pos, vel, owner, kind, rarity) = {
            let p = &g.projectiles[i];
            (p.pos, p.vel, p.owner, p.kind, p.rarity)
        };
        let step = vel * dt;
        let len = step.length();
        let dir = vel.normalize_or_zero();
        let tr = g.trace(owner, pos, dir, len.max(0.01));
        let hit = !matches!(tr.hit, TraceHit::None);
        g.projectiles[i].life -= dt;
        if hit {
            let at = tr.point - dir * 0.15;
            g.projectiles.remove(i);
            g.explode(at, owner, kind, rarity);
            continue;
        }
        g.projectiles[i].pos += step;
        // ground proximity fuse
        let p = g.projectiles[i].pos;
        if g.projectiles[i].life <= 0.0 || p.y < g.world.hm.height_at(p.x, p.z) - 0.1 {
            let at = p;
            g.projectiles.remove(i);
            g.explode(at, owner, kind, rarity);
            continue;
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{free_spot, game};
    use super::*;

    /// Put `a` and `b` facing each other on open ground, `d` metres apart.
    fn duel(g: &mut Game, d: f32) -> (usize, usize) {
        let w = &g.world;
        let p = w.layout.pois[0].center + Vec2::new(w.layout.pois[0].radius * 0.6, 40.0);
        let h = w.hm.height_at(p.x, p.y);
        let (a, b) = (PLAYER, 1);
        g.actors[a].pos = Vec3::new(p.x, h, p.y);
        g.actors[b].pos = Vec3::new(p.x, w.hm.height_at(p.x, p.y - d), p.y - d);
        g.actors[a].yaw = 0.0; // facing -Z
        g.actors[a].pitch = 0.0;
        for k in [a, b] {
            g.actors[k].mode = MoveMode::Ground;
            g.actors[k].on_ground = true;
            g.actors[k].brain = None;
        }
        // one idle tick settles the smoothed eye height so aim rays are stable afterwards
        g.update(1.0 / 60.0, &PlayerInput::default());
        (a, b)
    }

    #[test]
    fn the_emote_toggles_on_the_ground_and_ends_when_the_player_does_anything_else() {
        let mut g = game(2, true);
        let (a, _) = duel(&mut g, 30.0);
        let press = PlayerInput { emote: true, ..Default::default() };
        let idle = PlayerInput::default();
        g.update(1.0 / 60.0, &press);
        assert!(g.actors[a].emoting, "B starts the dance");
        for _ in 0..30 {
            g.update(1.0 / 60.0, &idle);
        }
        assert!(g.actors[a].emoting, "the dance keeps going while idle");
        assert!(g.actors[a].anim.emote > 0.5 && g.actors[a].anim.emote_clock > 0.3, "the pose blends in");
        g.update(1.0 / 60.0, &press);
        assert!(!g.actors[a].emoting, "pressing B again stops it");

        // moving, jumping, shooting, crouching, reloading and building all end the dance
        let interrupts = [
            PlayerInput { move_axis: Vec2::new(0.0, 1.0), ..Default::default() },
            PlayerInput { jump: true, ..Default::default() },
            PlayerInput { fire: true, fire_pressed: true, ..Default::default() },
            PlayerInput { crouch: true, ..Default::default() },
            PlayerInput { toggle_build: true, ..Default::default() },
            PlayerInput { select: Some(0), ..Default::default() },
        ];
        for (k, i) in interrupts.iter().enumerate() {
            // land, settle and drop whatever the previous case started
            for _ in 0..120 {
                g.update(1.0 / 60.0, &idle);
            }
            g.actors[a].build_mode = false;
            g.actors[a].action = Action::None;
            g.actors[a].emoting = false;
            g.update(1.0 / 60.0, &press);
            assert!(g.actors[a].emoting, "case {k}: start");
            g.update(1.0 / 60.0, i);
            assert!(!g.actors[a].emoting, "case {k}: interrupted by {i:?}");
        }
        for _ in 0..120 {
            g.update(1.0 / 60.0, &idle);
        }

        // damage ends it too
        g.actors[a].emoting = true;
        g.damage_actor(a, 10.0, Some(1), "Test", false, g.actors[a].pos, false);
        assert!(!g.actors[a].emoting);
    }

    #[test]
    fn the_emote_needs_solid_ground_and_a_calm_actor() {
        let mut g = game(2, true);
        let (a, _) = duel(&mut g, 30.0);
        let press = PlayerInput { emote: true, ..Default::default() };
        g.actors[a].on_ground = false;
        g.actors[a].mode = MoveMode::Freefall;
        g.update(1.0 / 60.0, &press);
        assert!(!g.actors[a].emoting, "no dancing in the sky");
        g.actors[a].mode = MoveMode::Ground;
        g.actors[a].on_ground = true;
        g.actors[a].action = Action::Reload { t: 0.0, dur: 2.0 };
        g.update(1.0 / 60.0, &press);
        assert!(!g.actors[a].emoting, "no dancing mid-reload");
    }

    /// A single trigger pull while aiming down sights (small, predictable spread).
    fn shoot() -> PlayerInput {
        PlayerInput { fire: true, fire_pressed: true, ads: true, ..Default::default() }
    }

    fn equip(g: &mut Game, i: usize, kind: WeaponKind, rarity: Rarity) {
        g.actors[i].inv.add_weapon(kind, rarity, kind.def().mag);
        let slot = g.actors[i].inv.slots.iter().position(|s| matches!(s, Some(Item::Weapon { kind: k, .. }) if *k == kind)).unwrap();
        g.actors[i].inv.selected = slot;
        g.actors[i].action = Action::None;
        g.actors[i].inv.ammo[kind.def().ammo.index()] = 200.min(kind.def().ammo.cap());
    }

    fn fire_inputs(n: usize, g: &mut Game) {
        for _ in 0..n {
            g.update(1.0 / 60.0, &PlayerInput { fire: true, fire_pressed: true, ..Default::default() });
        }
    }

    #[test]
    fn rifle_shots_hit_and_shield_absorbs_first() {
        let mut g = game(2, true);
        let (a, b) = duel(&mut g, 14.0);
        equip(&mut g, a, WeaponKind::AssaultRifle, Rarity::Common);
        g.actors[b].shield = 50.0;
        let hp0 = g.actors[b].hp;
        // aim slightly to compensate for the shoulder offset & height: face the target
        face_target(&mut g, a, b);
        g.update(1.0 / 60.0, &shoot());
        let t = &g.actors[b];
        assert!(t.shield < 50.0 || t.hp < hp0, "the first bullet must hit the target (shield {} hp {})", t.shield, t.hp);
        assert_eq!(t.hp, hp0, "shield absorbs the first hit entirely");
        assert!(g.events.iter().any(|e| matches!(e, Event::Damage { on_shield: true, .. })));
        assert!(g.events.iter().any(|e| matches!(e, Event::HitConfirm { shield: true, .. })));
    }

    fn face_target(g: &mut Game, a: usize, b: usize) {
        // the player's ray starts at the shoulder; solve yaw/pitch so the ray passes through the target chest
        g.actors[a].ads = true;
        for _ in 0..4 {
            let (o, _) = g.aim_ray(a);
            let to = g.actors[b].chest() - o;
            g.actors[a].yaw = yaw_of(Vec2::new(to.x, to.z));
            g.actors[a].pitch = (to.y / Vec2::new(to.x, to.z).length()).atan();
        }
    }

    #[test]
    fn a_wall_at_the_shooters_shoulder_stops_their_bullets() {
        // the aim ray starts at the right shoulder, 0.62 m from the body axis; pressed against a 0.26 m wall that point
        // is inside the wall, and a ray that starts inside a box never hits it
        for ads in [true, false] {
            let mut g = game(2, true);
            let spot = free_spot(&g);
            let (cx, cz) = pieces::cell_of(spot);
            let base = pieces::structure_base(&g.pieces, &g.env(), cx, cz);
            // a wall along Z on the east edge of the cell; the shooter faces -Z with it on their right, an enemy stands on the far side
            let key = pieces::PieceKey { kind: PieceKind::Wall, x: cx + 1, z: cz, level: 0, dir: 1 };
            let wall = g.pieces.insert(key, Mat::Wood, base, PLAYER);
            let bb = pieces::shape_of(&key, base).aabb();
            let (a, b) = (PLAYER, 1);
            let (px, pz) = (bb.min.x - 0.40, bb.max.z - 0.7);
            let (ex, ez) = (bb.max.x + 2.0, pz - 3.0);
            for (k, (x, z)) in [(a, (px, pz)), (b, (ex, ez))] {
                let h = g.world.hm.height_at(x, z);
                g.actors[k].pos = Vec3::new(x, h, z);
                g.actors[k].mode = MoveMode::Ground;
                g.actors[k].on_ground = true;
                g.actors[k].brain = None;
            }
            equip(&mut g, a, WeaponKind::AssaultRifle, Rarity::Common);
            g.update(1.0 / 60.0, &PlayerInput::default());
            let hp0 = g.actors[b].hp;
            let wall_hp0 = g.pieces.get(wall).unwrap().hp;
            for _ in 0..40 {
                for _ in 0..4 {
                    g.actors[a].ads = ads;
                    let (o, _) = g.aim_ray(a);
                    let to = g.actors[b].chest() - o;
                    g.actors[a].yaw = yaw_of(Vec2::new(to.x, to.z));
                    g.actors[a].pitch = (to.y / Vec2::new(to.x, to.z).length()).atan();
                }
                let (o, _) = g.aim_ray(a);
                assert!(!(o.x > bb.min.x && o.x < bb.max.x && o.z > bb.min.z && o.z < bb.max.z && o.y > bb.min.y && o.y < bb.max.y), "ads {ads}: the aim origin {o:?} is inside the wall {bb:?}");
                g.update(1.0 / 60.0, &PlayerInput { fire: true, ads, ..Default::default() });
            }
            assert_eq!(g.actors[b].hp, hp0, "ads {ads}: the enemy behind the wall must not be hurt");
            assert!(g.pieces.get(wall).is_none_or(|p| p.hp < wall_hp0), "ads {ads}: the wall takes the bullets");
        }
    }

    #[test]
    fn the_camera_pivot_eases_past_the_end_of_a_wall_while_the_aim_origin_stays_exact() {
        // walking along a wall with a gap in it, the wall on the right: the shoulder is cut short at the wall and free at
        // the gap. The aim origin must follow that exactly (a shot may not start inside a wall) but the camera must not jump
        let mut g = game(2, true);
        let spot = free_spot(&g);
        let (cx, cz) = pieces::cell_of(spot);
        let base = pieces::structure_base(&g.pieces, &g.env(), cx, cz);
        let mut wall_x = 0.0;
        for z in [cz - 1, cz + 1] {
            let key = pieces::PieceKey { kind: PieceKind::Wall, x: cx + 1, z, level: 0, dir: 1 };
            g.pieces.insert(key, Mat::Wood, base, PLAYER);
            wall_x = pieces::shape_of(&key, base).aabb().min.x;
        }
        // start south of the walls, facing north (-Z) with the wall on the right
        let (x, z) = (wall_x - 0.4, (cz + 3) as f32 * pieces::TILE);
        let h = g.world.hm.height_at(x, z);
        let a = &mut g.actors[PLAYER];
        a.pos = Vec3::new(x, h, z);
        a.mode = MoveMode::Ground;
        a.on_ground = true;
        a.yaw = 0.0;
        a.inv.selected = 0;
        g.actors[1].brain = None;
        g.update(1.0 / 60.0, &PlayerInput::default());
        g.cam_reach = g.shoulder_reach(PLAYER);
        let (mut prev_hard, mut prev_cam) = (g.shoulder_reach(PLAYER), g.cam_reach);
        let (mut max_hard, mut max_cam, mut gap_seen, mut wall_seen) = (0.0f32, 0.0f32, false, false);
        for _ in 0..260 {
            g.update(1.0 / 60.0, &PlayerInput { move_axis: Vec2::new(0.0, 1.0), ..Default::default() });
            g.tick_camera(1.0 / 60.0);
            let hard = g.shoulder_reach(PLAYER);
            max_hard = max_hard.max((hard - prev_hard).abs());
            max_cam = max_cam.max((g.cam_reach - prev_cam).abs());
            gap_seen |= hard > 0.6;
            wall_seen |= hard < 0.45;
            (prev_hard, prev_cam) = (hard, g.cam_reach);
        }
        assert!(gap_seen && wall_seen, "the walk must pass both wall and gap (hard reach {prev_hard})");
        assert!(max_hard > 0.2, "the aim origin follows the wall exactly: it jumps {max_hard:.2} m at a wall end");
        assert!(max_cam < 0.1, "the camera pivot must ease instead: it jumped {max_cam:.2} m in one frame");
        // and it settles on the aim origin once the player stands still beside the wall
        for _ in 0..120 {
            g.update(1.0 / 60.0, &PlayerInput::default());
            g.tick_camera(1.0 / 60.0);
        }
        assert!((g.cam_reach - g.shoulder_reach(PLAYER)).abs() < 0.02, "{} vs {}", g.cam_reach, g.shoulder_reach(PLAYER));
    }

    #[test]
    fn damage_dealt_does_not_count_overkill() {
        let mut g = game(2, true);
        let (a, b) = duel(&mut g, 14.0);
        g.actors[b].hp = 5.0;
        g.actors[b].shield = 10.0;
        let before = g.actors[a].damage_dealt;
        let chest = g.actors[b].chest();
        assert!(g.damage_actor(b, 105.0, Some(a), "Test", false, chest, false));
        let dealt = g.actors[a].damage_dealt - before;
        assert!((dealt - 15.0).abs() < 1e-3, "a 105 hit on 5 hp + 10 shield deals 15, not {dealt}");
        // a hit that pierces the shield only counts the hit points
        g.actors[b].alive = true;
        g.actors[b].mode = MoveMode::Ground;
        g.actors[b].hp = 5.0;
        g.actors[b].shield = 10.0;
        let before = g.actors[a].damage_dealt;
        g.damage_actor(b, 105.0, Some(a), "The Storm", false, chest, true);
        assert!((g.actors[a].damage_dealt - before - 5.0).abs() < 1e-3);
    }

    #[test]
    fn walls_shield_the_target_from_rocket_splash() {
        let mut g = game(2, true);
        let spot = free_spot(&g);
        let (cx, cz) = pieces::cell_of(spot);
        let base = pieces::structure_base(&g.pieces, &g.env(), cx, cz);
        let key = pieces::PieceKey { kind: PieceKind::Wall, x: cx + 1, z: cz, level: 0, dir: 1 };
        let bb = pieces::shape_of(&key, base).aabb();
        let z = (bb.min.z + bb.max.z) / 2.0;
        let (a, b) = (PLAYER, 1);
        let (bx, vx) = (bb.min.x - 0.6, bb.max.x + 1.6);
        let h = g.world.hm.height_at(bx, z);
        for (k, x) in [(a, bx - 6.0), (b, vx)] {
            g.actors[k].pos = Vec3::new(x, g.world.hm.height_at(x, z), z);
            g.actors[k].mode = MoveMode::Ground;
            g.actors[k].on_ground = true;
            g.actors[k].brain = None;
        }
        g.update(1.0 / 60.0, &PlayerInput::default());
        let blast = Vec3::new(bx, h + 1.2, z);
        // open ground first
        g.actors[b].hp = 100.0;
        g.actors[b].shield = 0.0;
        g.explode(blast, a, WeaponKind::RocketLauncher, Rarity::Common);
        let open = 100.0 - g.actors[b].hp;
        assert!(open > 50.0, "a rocket 2.5 m away hurts: {open}");
        // the same blast with a wall in between
        let wall = g.pieces.insert(key, Mat::Stone, base, PLAYER);
        g.actors[b].alive = true;
        g.actors[b].mode = MoveMode::Ground;
        g.actors[b].hp = 100.0;
        g.actors[b].vel = Vec3::ZERO;
        g.explode(blast, a, WeaponKind::RocketLauncher, Rarity::Common);
        let covered = 100.0 - g.actors[b].hp;
        assert!(covered > 0.0 && covered < open * 0.4, "the wall soaks the blast: {covered} behind it vs {open} in the open");
        assert!(g.actors[b].vel.length() < 3.0, "and the target is not thrown through it: {:?}", g.actors[b].vel);
        let _ = wall;
    }

    #[test]
    fn full_auto_rate_and_magazine_depletion() {
        let mut g = game(2, true);
        let (a, b) = duel(&mut g, 30.0);
        g.actors[b].hp = 10_000.0;
        equip(&mut g, a, WeaponKind::AssaultRifle, Rarity::Common);
        face_target(&mut g, a, b);
        // hold fire for 2 seconds: 5.5 shots/s => ~11 shots
        let mut shots = 0;
        for _ in 0..120 {
            g.update(1.0 / 60.0, &PlayerInput { fire: true, ..Default::default() });
            shots += g.events.iter().filter(|e| matches!(e, Event::Shot { .. })).count();
            face_target(&mut g, a, b);
        }
        assert!((9..=13).contains(&shots), "{shots} shots in 2 s");
        let (_, _, ammo) = g.actors[a].inv.selected_weapon().unwrap();
        assert_eq!(ammo as usize, 30 - shots);
    }

    #[test]
    fn reload_takes_time_and_moves_reserve_ammo_into_the_magazine() {
        let mut g = game(2, true);
        let (a, _) = duel(&mut g, 40.0);
        equip(&mut g, a, WeaponKind::Smg, Rarity::Common);
        let sel = g.actors[a].inv.selected;
        if let Some(Item::Weapon { ammo, .. }) = &mut g.actors[a].inv.slots[sel] {
            *ammo = 4;
        }
        g.actors[a].inv.ammo[AmmoKind::Light.index()] = 100;
        g.update(1.0 / 60.0, &PlayerInput { reload: true, ..Default::default() });
        assert!(matches!(g.actors[a].action, Action::Reload { .. }));
        // firing during the reload is impossible
        fire_inputs(10, &mut g);
        let (_, _, ammo) = g.actors[a].inv.selected_weapon().unwrap();
        assert_eq!(ammo, 4, "cannot shoot while reloading");
        for _ in 0..180 {
            g.update(1.0 / 60.0, &PlayerInput::default());
        }
        let (_, _, ammo) = g.actors[a].inv.selected_weapon().unwrap();
        assert_eq!(ammo, 30);
        assert_eq!(g.actors[a].inv.ammo[AmmoKind::Light.index()], 100 - 26);
        assert!(matches!(g.actors[a].action, Action::None));
    }

    #[test]
    fn headshots_do_more_and_distance_reduces_damage() {
        let mut g = game(2, true);
        let (a, b) = duel(&mut g, 10.0);
        equip(&mut g, a, WeaponKind::Sniper, Rarity::Common);
        // body shot
        face_target(&mut g, a, b);
        g.actors[b].hp = 500.0;
        g.update(1.0 / 60.0, &shoot());
        let body = 500.0 - g.actors[b].hp;
        assert!((body - 105.0).abs() < 0.5, "sniper body shot {body}");
        // headshot
        g.actors[a].fire_cd = 0.0;
        let sel = g.actors[a].inv.selected;
        if let Some(Item::Weapon { ammo, .. }) = &mut g.actors[a].inv.slots[sel] {
            *ammo = 1;
        }
        g.actors[b].hp = 500.0;
        g.actors[a].ads = true;
        for _ in 0..4 {
            let (o, _) = g.aim_ray(a);
            let to = g.actors[b].head_pos() - o;
            g.actors[a].yaw = yaw_of(Vec2::new(to.x, to.z));
            g.actors[a].pitch = (to.y / Vec2::new(to.x, to.z).length()).atan();
        }
        g.update(1.0 / 60.0, &shoot());
        let head = 500.0 - g.actors[b].hp;
        assert!(head > body * 2.0, "headshot {head} vs body {body}");
        assert!(g.events.iter().any(|e| matches!(e, Event::Damage { headshot: true, .. })));
    }

    #[test]
    fn shotgun_is_strong_up_close_and_weak_far_away() {
        let mut dmg_close = 0.0;
        let mut dmg_far = 0.0;
        for (d, out) in [(4.0f32, &mut dmg_close), (45.0, &mut dmg_far)] {
            let mut g = game(2, true);
            let (a, b) = duel(&mut g, d);
            equip(&mut g, a, WeaponKind::Shotgun, Rarity::Common);
            g.actors[b].hp = 1000.0;
            face_target(&mut g, a, b);
            // average several shots (pellets are random)
            let mut total = 0.0;
            for _ in 0..8 {
                g.actors[a].fire_cd = 0.0;
                g.actors[a].bloom = 0.0;
                let sel = g.actors[a].inv.selected;
                if let Some(Item::Weapon { ammo, .. }) = &mut g.actors[a].inv.slots[sel] {
                    *ammo = 5;
                }
                let before = g.actors[b].hp;
                g.update(1.0 / 60.0, &shoot());
                total += before - g.actors[b].hp;
                face_target(&mut g, a, b);
            }
            *out = total / 8.0;
        }
        assert!(dmg_close > 55.0, "close shotgun damage {dmg_close}");
        assert!(dmg_far < dmg_close * 0.35, "far {dmg_far} vs close {dmg_close}");
    }

    #[test]
    fn elimination_drops_loot_updates_feed_and_placement() {
        let mut g = game(3, true);
        let (a, b) = duel(&mut g, 12.0);
        equip(&mut g, a, WeaponKind::Pistol, Rarity::Common);
        g.actors[b].inv.add_weapon(WeaponKind::Smg, Rarity::Rare, 20);
        g.actors[b].hp = 10.0;
        face_target(&mut g, a, b);
        let pickups = g.pickups.len();
        g.update(1.0 / 60.0, &shoot());
        assert!(!g.actors[b].alive, "target must die");
        assert_eq!(g.actors[b].mode, MoveMode::Dead);
        assert_eq!(g.actors[a].kills, 1);
        assert!(g.pickups.len() > pickups, "the victim's weapon drops");
        assert_eq!(g.feed.len(), 1);
        assert!(g.feed[0].by_player);
        assert_eq!(g.actors[b].placement, 4, "4 alive before => placed 4th");
        assert!(g.events.iter().any(|e| matches!(e, Event::HitConfirm { kill: true, .. })));
    }

    #[test]
    fn healing_items_restore_health_and_shield_with_caps() {
        let mut g = game(2, true);
        let a = PLAYER;
        g.actors[a].inv.add_consumable(ConsumableKind::Bandage, 3);
        g.actors[a].hp = 50.0;
        g.select_slot(a, 1);
        for _ in 0..30 {
            g.update(1.0 / 60.0, &PlayerInput::default());
        }
        g.update(1.0 / 60.0, &PlayerInput { fire: true, fire_pressed: true, ..Default::default() });
        assert!(matches!(g.actors[a].action, Action::Heal { .. }));
        for _ in 0..(60 * 4) {
            g.update(1.0 / 60.0, &PlayerInput::default());
        }
        assert_eq!(g.actors[a].hp, 65.0, "bandage heals 15");
        // bandages cap at 75
        g.actors[a].hp = 72.0;
        g.actors[a].action = Action::None;
        g.update(1.0 / 60.0, &PlayerInput { fire: true, fire_pressed: true, ..Default::default() });
        for _ in 0..(60 * 4) {
            g.update(1.0 / 60.0, &PlayerInput::default());
        }
        assert_eq!(g.actors[a].hp, 75.0);
        // at the cap they can't be used
        g.actors[a].action = Action::None;
        g.update(1.0 / 60.0, &PlayerInput { fire: true, fire_pressed: true, ..Default::default() });
        assert!(matches!(g.actors[a].action, Action::None), "no healing when at the cap");
        // shield potion
        g.actors[a].inv.add_consumable(ConsumableKind::ShieldBig, 1);
        let slot = g.actors[a].inv.slots.iter().position(|s| matches!(s, Some(Item::Consumable { kind: ConsumableKind::ShieldBig, .. }))).unwrap();
        g.select_slot(a, slot);
        for _ in 0..30 {
            g.update(1.0 / 60.0, &PlayerInput::default());
        }
        g.update(1.0 / 60.0, &PlayerInput { fire: true, fire_pressed: true, ..Default::default() });
        for _ in 0..(60 * 6) {
            g.update(1.0 / 60.0, &PlayerInput::default());
        }
        assert_eq!(g.actors[a].shield, 50.0);
    }

    #[test]
    fn pickaxe_harvests_trees_and_fells_them() {
        let mut g = game(1, true);
        let tree = g.world.harvest.iter().position(|h| h.kind == HarvestKind::Tree && h.alive).unwrap();
        let pos = g.world.harvest[tree].pos;
        let a = PLAYER;
        // stand next to the trunk, facing it
        g.actors[a].pos = Vec3::new(pos.x + 1.4, g.world.hm.height_at(pos.x + 1.4, pos.z), pos.z);
        g.actors[a].mode = MoveMode::Ground;
        g.actors[a].on_ground = true;
        g.actors[a].inv.selected = 0;
        g.actors[a].action = Action::None;
        let wood0 = g.actors[a].inv.mats[Mat::Wood.index()];
        let mut hits = 0;
        for _ in 0..600 {
            // keep aiming at the trunk
            let (o, _) = g.aim_ray(a);
            let to = Vec3::new(pos.x, pos.y + 1.2, pos.z) - o;
            g.actors[a].yaw = yaw_of(Vec2::new(to.x, to.z));
            g.actors[a].pitch = (to.y / Vec2::new(to.x, to.z).length()).atan();
            g.update(1.0 / 60.0, &PlayerInput { fire: true, ..Default::default() });
            hits += g.events.iter().filter(|e| matches!(e, Event::Harvest { .. })).count();
            if !g.world.harvest[tree].alive {
                break;
            }
        }
        assert!(!g.world.harvest[tree].alive, "tree must fall after ~4 hits ({hits} hits)");
        assert!((3..=6).contains(&hits), "{hits} hits");
        assert!(g.actors[a].inv.mats[Mat::Wood.index()] >= wood0 + 60, "wood gained");
        assert!(g.world.statics.get(g.world.harvest[tree].collider).is_none(), "its collider is gone");
        assert_eq!(g.felled.len(), 1);
    }

    #[test]
    fn trees_rocks_and_ore_each_give_their_own_building_material() {
        // metal used to be promised (T cycles to it, the README listed it) but nothing in the world gave it
        let mut g = game(1, true);
        for (kind, mat) in [(HarvestKind::Tree, Mat::Wood), (HarvestKind::Rock, Mat::Stone), (HarvestKind::Ore, Mat::Metal)] {
            let candidates: Vec<usize> = g.world.harvest.iter().enumerate().filter(|(_, h)| h.kind == kind && h.alive).map(|(i, _)| i).take(60).collect();
            assert!(!candidates.is_empty(), "no {kind:?} in the world");
            let mut gained = false;
            'search: for idx in candidates {
                let h = g.world.harvest[idx].clone();
                let r = match g.world.statics.get(h.collider).map(|c| c.shape) {
                    Some(crate::world::collision::Shape::Cyl { r, .. }) => r,
                    _ => continue,
                };
                for k in 0..8 {
                    let ang = k as f32 * std::f32::consts::FRAC_PI_4;
                    let (sx, sz) = (h.pos.x + ang.cos() * (r + 0.95), h.pos.z + ang.sin() * (r + 0.95));
                    let feet = Vec3::new(sx, g.world.hm.height_at(sx, sz), sz);
                    let env = g.env();
                    // a free spot to stand on, with a clear line to the surface of the trunk or rock (not to its centre)
                    let surface = Vec3::new(h.pos.x + ang.cos() * (r + 0.05), feet.y + 1.2, h.pos.z + ang.sin() * (r + 0.05));
                    let open = env.push_out(feet, RADIUS, HEIGHT, 0.55).length() < 1e-3 && g.world.hm.slope_at(sx, sz) < 0.5 && env.line_clear(feet + Vec3::Y * 1.2, surface);
                    if !open {
                        continue;
                    }
                    let a = PLAYER;
                    g.actors[a].pos = feet;
                    g.actors[a].mode = MoveMode::Ground;
                    g.actors[a].on_ground = true;
                    g.actors[a].inv.selected = 0;
                    g.actors[a].action = Action::None;
                    let before = g.actors[a].inv.mats[mat.index()];
                    for _ in 0..900 {
                        let (o, _) = g.aim_ray(a);
                        let to = Vec3::new(h.pos.x, feet.y + 1.0, h.pos.z) - o;
                        g.actors[a].yaw = yaw_of(Vec2::new(to.x, to.z));
                        g.actors[a].pitch = (to.y / Vec2::new(to.x, to.z).length()).atan();
                        g.update(1.0 / 60.0, &PlayerInput { fire: true, ..Default::default() });
                        if !g.world.harvest[idx].alive {
                            break;
                        }
                    }
                    if !g.world.harvest[idx].alive {
                        assert!(g.actors[a].inv.mats[mat.index()] >= before + 40, "{kind:?} should give {mat:?}: {before} -> {}", g.actors[a].inv.mats[mat.index()]);
                        gained = true;
                        break 'search;
                    }
                }
            }
            assert!(gained, "no {kind:?} could be broken with the pickaxe");
        }
    }

    #[test]
    fn rockets_explode_on_impact_and_damage_nearby_actors() {
        let mut g = game(3, true);
        let (a, b) = duel(&mut g, 30.0);
        equip(&mut g, a, WeaponKind::RocketLauncher, Rarity::Common);
        g.actors[b].hp = 500.0;
        face_target(&mut g, a, b);
        g.update(1.0 / 60.0, &shoot());
        assert_eq!(g.projectiles.len(), 1);
        let mut exploded = false;
        for _ in 0..180 {
            g.update(1.0 / 60.0, &PlayerInput::default());
            if g.events.iter().any(|e| matches!(e, Event::Explosion { .. })) {
                exploded = true;
                break;
            }
        }
        assert!(exploded, "rocket must detonate");
        assert!(g.actors[b].hp < 500.0 - 40.0, "target took splash damage: hp {}", g.actors[b].hp);
        assert!(g.projectiles.is_empty());
    }

    #[test]
    fn bullets_damage_and_destroy_player_walls() {
        let mut g = game(2, true);
        let (a, b) = duel(&mut g, 14.0);
        equip(&mut g, a, WeaponKind::AssaultRifle, Rarity::Legendary);
        // a wall right in front of the target (between the two)
        let mid = (g.actors[a].pos + g.actors[b].pos) * 0.5;
        let cell = pieces::cell_of(mid);
        let key = PieceKey { kind: PieceKind::Wall, x: cell.0, z: cell.1, level: 0, dir: 0 };
        let base = pieces::structure_base(&g.pieces, &g.env(), cell.0, cell.1);
        // make sure the wall edge is crossed by the line between them: put it at the z of the midpoint
        let wall_z = mid.z;
        let key = PieceKey { z: (wall_z / TILE).round() as i32, x: ((g.actors[a].pos.x) / TILE).floor() as i32, ..key };
        g.pieces.insert(key, Mat::Wood, base, a);
        g.actors[b].hp = 500.0;
        face_target(&mut g, a, b);
        let hp0 = g.pieces.iter().next().unwrap().hp;
        for _ in 0..30 {
            g.update(1.0 / 60.0, &PlayerInput { fire: true, ..Default::default() });
        }
        assert_eq!(g.actors[b].hp, 500.0, "the wall protects the target");
        let left = g.pieces.iter().next().map(|p| p.hp);
        assert!(left.is_none_or(|l| l < hp0), "bullets damage the wall");
    }
}
