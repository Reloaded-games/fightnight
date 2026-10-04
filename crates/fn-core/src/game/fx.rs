//! Visual effects driven by game events: muzzle flashes, tracers, impacts,
//! explosions, sparkles, debris and floating damage numbers. Pure simulation: the
//! renderer only receives the final billboard list from `emit`.

use super::actor::*;
use super::events::*;
use super::items::*;
use crate::math::*;
use crate::mesh::{shape, Particle};
use crate::rng::Rng;

#[derive(Clone, Copy)]
struct Part {
    pos: Vec3,
    vel: Vec3,
    size0: f32,
    size1: f32,
    age: f32,
    life: f32,
    c0: [f32; 4],
    c1: [f32; 4],
    shape: f32,
    grav: f32,
    drag: f32,
    add: bool,
    rot: f32,
    rot_v: f32,
    seed: f32,
    /// Spark streak length per m/s of speed (0 = plain billboard).
    stretch: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct TracerFx {
    pub from: Vec3,
    pub to: Vec3,
    pub age: f32,
    pub life: f32,
    pub color: [f32; 3],
}

#[derive(Clone, Copy, Debug)]
pub struct DamageNumber {
    pub pos: Vec3,
    pub amount: f32,
    /// 0 health, 1 shield, 2 critical (headshot)
    pub kind: u8,
    pub age: f32,
    pub drift: f32,
}

pub struct Fx {
    parts: Vec<Part>,
    pub tracers: Vec<TracerFx>,
    pub numbers: Vec<DamageNumber>,
    rng: Rng,
}

const MAX_PARTS: usize = 5000;

impl Default for Fx {
    fn default() -> Self {
        Self::new()
    }
}

fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t), lerp(a[3], b[3], t)]
}

impl Fx {
    pub fn new() -> Self {
        Self { parts: Vec::with_capacity(1024), tracers: vec![], numbers: vec![], rng: Rng::new(0xFA11) }
    }

    pub fn count(&self) -> usize {
        self.parts.len()
    }

    fn push(&mut self, p: Part) {
        if self.parts.len() >= MAX_PARTS {
            self.parts.swap_remove(0);
        }
        self.parts.push(p);
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn(&mut self, pos: Vec3, vel: Vec3, size0: f32, size1: f32, life: f32, c0: [f32; 4], c1: [f32; 4], shape: f32, add: bool, grav: f32, drag: f32) {
        let rot = self.rng.range(0.0, std::f32::consts::TAU);
        let rot_v = self.rng.range(-2.0, 2.0);
        let seed = self.rng.f32();
        self.push(Part { pos, vel, size0, size1, age: 0.0, life, c0, c1, shape, grav, drag, add, rot, rot_v, seed, stretch: 0.0 });
    }

    fn spark(&mut self, pos: Vec3, vel: Vec3, width: f32, life: f32, color: [f32; 4], grav: f32) {
        let seed = self.rng.f32();
        let c1 = [color[0], color[1] * 0.6, color[2] * 0.3, 0.0];
        self.push(Part { pos, vel, size0: width, size1: width * 0.5, age: 0.0, life, c0: color, c1, shape: shape::SPARK, grav, drag: 0.5, add: true, rot: 0.0, rot_v: 0.0, seed, stretch: 0.05 });
    }

    fn rand_dir(&mut self) -> Vec3 {
        self.rng.unit_vec3()
    }

    // ---- recipes ------------------------------------------------------------------------
    fn muzzle_flash(&mut self, pos: Vec3, dir: Vec3, weapon: WeaponKind) {
        let big = matches!(weapon, WeaponKind::Shotgun | WeaponKind::Sniper | WeaponKind::RocketLauncher);
        let s = if big { 0.95 } else { 0.55 };
        self.spawn(pos + dir * 0.15, Vec3::ZERO, s, s * 0.6, 0.06, [3.0, 2.3, 1.1, 1.0], [2.0, 1.0, 0.3, 0.0], shape::STAR, true, 0.0, 0.0);
        self.spawn(pos, Vec3::ZERO, s * 0.9, s * 0.4, 0.07, [3.0, 2.4, 1.4, 1.0], [1.5, 0.7, 0.2, 0.0], shape::GLOW, true, 0.0, 0.0);
        // a puff of smoke
        let drift = dir * 1.2 + Vec3::Y * 0.5;
        self.spawn(pos + dir * 0.4, drift, 0.15, 0.7, 0.55, [0.8, 0.8, 0.8, 0.35], [0.7, 0.7, 0.72, 0.0], shape::SMOKE, false, -0.5, 1.5);
        for _ in 0..(if big { 6 } else { 3 }) {
            let d = (dir + self.rand_dir() * 0.35).normalize();
            let v = d * self.rng.range(8.0, 18.0);
            self.spark(pos + dir * 0.3, v, 0.05, 0.12, [3.0, 2.0, 0.8, 1.0], 6.0);
        }
    }

    fn impact(&mut self, pos: Vec3, n: Vec3, kind: ImpactKind) {
        let (dust, chip, spark) = match kind {
            ImpactKind::Dirt => ([0.62, 0.48, 0.30, 0.55], [0.5, 0.38, 0.22, 1.0], false),
            ImpactKind::Stone => ([0.75, 0.75, 0.78, 0.5], [0.8, 0.8, 0.82, 1.0], true),
            ImpactKind::Wood => ([0.75, 0.58, 0.36, 0.5], [0.85, 0.65, 0.38, 1.0], false),
            ImpactKind::Metal => ([0.7, 0.7, 0.75, 0.35], [0.9, 0.9, 1.0, 1.0], true),
            ImpactKind::Flesh | ImpactKind::Shield => ([0.9, 0.9, 1.0, 0.4], [1.0, 1.0, 1.0, 1.0], true),
            ImpactKind::Foliage => ([0.4, 0.7, 0.3, 0.6], [0.35, 0.65, 0.25, 1.0], false),
            ImpactKind::Water => ([0.8, 0.9, 1.0, 0.6], [0.8, 0.9, 1.0, 1.0], false),
        };
        self.spawn(pos, n * 0.8, 0.12, 0.7, 0.45, dust, [dust[0], dust[1], dust[2], 0.0], shape::SMOKE, false, -0.3, 2.0);
        for _ in 0..4 {
            let v = (n + self.rand_dir() * 0.8).normalize() * self.rng.range(2.0, 6.0);
            self.spawn(pos, v, 0.07, 0.02, 0.5, chip, [chip[0], chip[1], chip[2], 0.0], shape::GLOW, false, 14.0, 0.5);
        }
        if spark {
            for _ in 0..5 {
                let v = (n + self.rand_dir() * 0.9).normalize() * self.rng.range(4.0, 11.0);
                self.spark(pos, v, 0.04, 0.2, [3.0, 2.2, 1.0, 1.0], 12.0);
            }
            self.spawn(pos + n * 0.05, Vec3::ZERO, 0.4, 0.15, 0.07, [3.0, 2.5, 1.5, 1.0], [1.0, 0.6, 0.2, 0.0], shape::STAR, true, 0.0, 0.0);
        }
    }

    fn explosion(&mut self, pos: Vec3, radius: f32) {
        let k = radius / 7.0;
        // fireball
        for _ in 0..14 {
            let v = self.rand_dir() * self.rng.range(2.0, 8.0) * k;
            self.spawn(pos + v * 0.05, v, 1.4 * k, 4.2 * k, 0.55, [3.5, 1.8, 0.5, 1.0], [0.9, 0.2, 0.05, 0.0], shape::GLOW, true, -2.0, 2.0);
        }
        self.spawn(pos, Vec3::ZERO, 1.0, radius * 1.9, 0.18, [4.0, 3.0, 1.5, 1.0], [2.0, 0.8, 0.2, 0.0], shape::GLOW, true, 0.0, 0.0);
        self.spawn(pos + Vec3::Y * 0.3, Vec3::ZERO, 0.5, radius * 2.4, 0.4, [1.5, 1.2, 0.8, 0.9], [0.8, 0.5, 0.2, 0.0], shape::RING, true, 0.0, 0.0);
        // smoke
        for _ in 0..16 {
            let v = Vec3::new(self.rng.range(-3.0, 3.0), self.rng.range(1.0, 5.0), self.rng.range(-3.0, 3.0)) * k;
            let life = self.rng.range(1.4, 2.4);
            self.spawn(pos + v * 0.1, v, 1.2 * k, 5.0 * k, life, [0.18, 0.17, 0.17, 0.75], [0.35, 0.34, 0.34, 0.0], shape::SMOKE, false, -0.6, 1.4);
        }
        for _ in 0..28 {
            let v = self.rand_dir() * self.rng.range(8.0, 22.0) * k;
            self.spark(pos, v, 0.07, 0.6, [3.0, 1.6, 0.4, 1.0], 12.0);
        }
        for _ in 0..12 {
            let v = Vec3::new(self.rng.range(-6.0, 6.0), self.rng.range(5.0, 12.0), self.rng.range(-6.0, 6.0));
            self.spawn(pos, v, 0.18, 0.1, 1.0, [0.35, 0.28, 0.2, 1.0], [0.2, 0.17, 0.12, 0.0], shape::GLOW, false, 16.0, 0.2);
        }
    }

    fn sparkle_burst(&mut self, pos: Vec3, color: Vec3, n: usize, power: f32) {
        for _ in 0..n {
            let v = Vec3::new(self.rng.range(-1.0, 1.0), self.rng.range(0.6, 2.0), self.rng.range(-1.0, 1.0)) * power;
            let s = self.rng.range(0.1, 0.28);
            let life = self.rng.range(0.6, 1.2);
            self.spawn(pos, v, s, s * 0.2, life, [color.x * 3.0, color.y * 3.0, color.z * 3.0, 1.0], [color.x, color.y, color.z, 0.0], shape::STAR, true, 5.0, 1.2);
        }
        self.spawn(pos, Vec3::ZERO, 0.3, 2.2 * power.min(2.0), 0.35, [color.x * 2.0, color.y * 2.0, color.z * 2.0, 0.9], [color.x, color.y, color.z, 0.0], shape::RING, true, 0.0, 0.0);
    }

    fn debris(&mut self, pos: Vec3, color: [f32; 4], n: usize, power: f32) {
        for _ in 0..n {
            let v = Vec3::new(self.rng.range(-1.0, 1.0), self.rng.range(0.4, 1.6), self.rng.range(-1.0, 1.0)) * power;
            let s = self.rng.range(0.06, 0.16);
            let life = self.rng.range(0.6, 1.1);
            self.spawn(pos, v, s, s * 0.8, life, color, [color[0], color[1], color[2], 0.0], shape::GLOW, false, 15.0, 0.3);
        }
        self.spawn(pos, Vec3::Y * 0.5, 0.3, 1.4 * power * 0.3, 0.7, [0.7, 0.65, 0.55, 0.4], [0.7, 0.65, 0.55, 0.0], shape::SMOKE, false, -0.2, 1.5);
    }

    pub fn on_event(&mut self, e: &Event, actors: &[Actor], _time: f32) {
        match e {
            Event::Shot { actor, pos, weapon, .. } => {
                if let Some(a) = actors.get(*actor) {
                    self.muzzle_flash(*pos, look_dir(a.yaw, a.pitch), *weapon);
                }
            }
            Event::Tracer { from, to, weapon } => {
                let len = from.distance(*to);
                if len > 2.0 {
                    let col = match weapon {
                        WeaponKind::Sniper => [1.0, 0.9, 0.6],
                        WeaponKind::Shotgun => [1.0, 0.8, 0.4],
                        _ => [1.0, 0.85, 0.5],
                    };
                    let life = (len / 260.0).clamp(0.05, 0.25);
                    self.tracers.push(TracerFx { from: *from, to: *to, age: 0.0, life, color: col });
                }
            }
            Event::Impact { pos, normal, kind } => self.impact(*pos, *normal, *kind),
            Event::Damage { attacker, amount, on_shield, headshot, pos, .. } => {
                if *attacker == Some(PLAYER_ID) {
                    let kind = if *headshot { 2 } else if *on_shield { 1 } else { 0 };
                    let drift = self.rng.range(-0.8, 0.8);
                    self.numbers.push(DamageNumber { pos: *pos, amount: *amount, kind, age: 0.0, drift });
                    if self.numbers.len() > 24 {
                        self.numbers.remove(0);
                    }
                }
                let c = if *on_shield { [0.3, 0.7, 3.0, 1.0] } else { [3.0, 2.4, 1.6, 1.0] };
                for _ in 0..4 {
                    let v = self.rand_dir() * self.rng.range(2.0, 6.0);
                    self.spark(*pos, v, 0.04, 0.25, c, 8.0);
                }
            }
            Event::Eliminated { victim, .. } => {
                if let Some(a) = actors.get(*victim) {
                    let p = a.pos + Vec3::Y * 0.9;
                    self.sparkle_burst(p, Vec3::new(0.5, 0.8, 1.0), 26, 3.5);
                    self.spawn(p, Vec3::Y * 1.0, 0.8, 3.2, 0.7, [1.5, 2.2, 3.5, 0.8], [0.5, 1.0, 2.0, 0.0], shape::GLOW, true, -1.0, 1.0);
                }
            }
            Event::Explosion { pos, radius } => self.explosion(*pos, *radius),
            Event::HarvestHit { pos, normal, wood } => {
                let c = if *wood { [0.82, 0.62, 0.36, 1.0] } else { [0.75, 0.75, 0.78, 1.0] };
                for _ in 0..6 {
                    let v = (*normal + self.rand_dir() * 0.9).normalize() * self.rng.range(2.0, 6.0);
                    self.spawn(*pos, v, 0.09, 0.07, 0.6, c, [c[0], c[1], c[2], 0.0], shape::GLOW, false, 14.0, 0.3);
                }
                if !*wood {
                    self.spawn(*pos, Vec3::ZERO, 0.4, 0.1, 0.07, [3.0, 2.5, 1.5, 1.0], [1.0, 0.6, 0.2, 0.0], shape::STAR, true, 0.0, 0.0);
                }
            }
            Event::TreeFelled { pos, .. } => {
                let p = *pos + Vec3::Y * 3.0;
                for _ in 0..22 {
                    let v = Vec3::new(self.rng.range(-3.0, 3.0), self.rng.range(0.0, 3.0), self.rng.range(-3.0, 3.0));
                    let g = self.rng.range(0.25, 0.6);
                    let life = self.rng.range(1.0, 1.8);
                    self.spawn(p + v * 0.3, v, 0.22, 0.12, life, [g * 0.5, g, g * 0.25, 1.0], [g * 0.5, g, g * 0.25, 0.0], shape::GLOW, false, 3.0, 0.8);
                }
                self.debris(*pos + Vec3::Y * 0.4, [0.55, 0.4, 0.25, 1.0], 8, 3.0);
            }
            Event::PieceDestroyed { pos, mat } => {
                let c = match mat {
                    Mat::Wood => [0.8, 0.58, 0.3, 1.0],
                    Mat::Stone => [0.7, 0.7, 0.72, 1.0],
                    Mat::Metal => [0.55, 0.65, 0.8, 1.0],
                };
                self.debris(*pos, c, 22, 4.5);
            }
            Event::Built { pos, mat, .. } => {
                let c = mat.color();
                self.sparkle_burst(*pos, c, 6, 1.2);
            }
            Event::Land { pos, speed, .. } => {
                if *speed > 7.0 {
                    let k = (*speed / 14.0).clamp(0.4, 1.4);
                    for _ in 0..5 {
                        let a = self.rng.range(0.0, std::f32::consts::TAU);
                        let v = Vec3::new(a.cos(), 0.1, a.sin()) * 2.0 * k;
                        self.spawn(*pos + Vec3::Y * 0.1, v, 0.25, 0.9 * k, 0.5, [0.7, 0.62, 0.5, 0.5], [0.7, 0.62, 0.5, 0.0], shape::SMOKE, false, 0.0, 2.0);
                    }
                }
            }
            Event::Pickup { pos, rarity, .. } => {
                let c = rarity.color();
                self.sparkle_burst(*pos + Vec3::Y * 0.6, Vec3::new(c.x.powf(2.2), c.y.powf(2.2), c.z.powf(2.2)).max(Vec3::splat(0.08)) * 1.5, 10, 1.4);
            }
            Event::ChestOpen { pos } => {
                self.sparkle_burst(*pos + Vec3::Y * 0.9, Vec3::new(1.0, 0.7, 0.12), 36, 3.4);
                self.spawn(*pos + Vec3::Y * 0.8, Vec3::Y * 1.5, 0.6, 3.5, 0.6, [3.0, 2.0, 0.6, 1.0], [1.5, 0.8, 0.1, 0.0], shape::GLOW, true, -1.0, 1.0);
            }
            Event::GliderDeploy { pos, .. } => {
                for _ in 0..6 {
                    let v = self.rand_dir() * 2.0;
                    self.spawn(*pos + v * 0.3, v, 0.5, 2.2, 0.8, [1.0, 1.0, 1.0, 0.5], [1.0, 1.0, 1.0, 0.0], shape::SMOKE, false, 0.0, 1.5);
                }
            }
            Event::HealDone { actor, .. } => {
                if let Some(a) = actors.get(*actor) {
                    self.sparkle_burst(a.pos + Vec3::Y, Vec3::new(0.4, 1.0, 0.5), 14, 1.8);
                }
            }
            _ => {}
        }
    }

    pub fn update(&mut self, dt: f32) {
        let mut i = 0;
        while i < self.parts.len() {
            let p = &mut self.parts[i];
            p.age += dt;
            if p.age >= p.life {
                self.parts.swap_remove(i);
                continue;
            }
            p.vel.y -= p.grav * dt;
            p.vel *= (1.0 - p.drag * dt).max(0.0);
            p.pos += p.vel * dt;
            p.rot += p.rot_v * dt;
            i += 1;
        }
        for t in &mut self.tracers {
            t.age += dt;
        }
        self.tracers.retain(|t| t.age < t.life);
        for n in &mut self.numbers {
            n.age += dt;
        }
        self.numbers.retain(|n| n.age < 1.05);
    }

    /// Append billboards for everything alive: alpha-blended ones and additive ones.
    pub fn emit(&self, alpha: &mut Vec<Particle>, add: &mut Vec<Particle>) {
        for p in &self.parts {
            let t = (p.age / p.life).clamp(0.0, 1.0);
            let size = lerp(p.size0, p.size1, t);
            let c = lerp4(p.c0, p.c1, t);
            let part = if p.stretch > 0.0 {
                let speed = p.vel.length().max(0.01);
                Particle::stretched(p.pos, p.vel / speed, (speed * p.stretch).max(0.08), size, c, p.shape, p.seed)
            } else {
                Particle::billboard(p.pos, size, c, p.shape, p.rot, p.seed)
            };
            if p.add {
                add.push(part);
            } else {
                alpha.push(part);
            }
        }
        for t in &self.tracers {
            let k = (t.age / t.life).clamp(0.0, 1.0);
            let total = t.from.distance(t.to).max(0.01);
            let dir = (t.to - t.from) / total;
            let head = lerp(0.0, total, k.min(1.0));
            let len = 9.0f32.min(head.max(1.0));
            let pos = t.from + dir * (head - len).max(0.0);
            let a = 1.0 - k * 0.6;
            add.push(Particle::stretched(pos, dir, len, 0.09, [t.color[0] * 3.0 * a, t.color[1] * 3.0 * a, t.color[2] * 3.0 * a, 1.0], shape::TRACER, 0.0));
        }
    }
}

/// The local player's actor id (kept here to avoid a module cycle).
pub const PLAYER_ID: usize = 0;

#[cfg(test)]
mod tests {
    use super::*;

    fn actors() -> Vec<Actor> {
        let mut r = Rng::new(1);
        vec![Actor::new(0, "p", true, Outfit::random(&mut r)), Actor::new(1, "b", false, Outfit::random(&mut r))]
    }

    #[test]
    fn particles_are_emitted_and_expire() {
        let mut fx = Fx::new();
        let a = actors();
        fx.on_event(&Event::Explosion { pos: Vec3::new(0.0, 5.0, 0.0), radius: 7.0 }, &a, 0.0);
        assert!(fx.count() > 40);
        let (mut al, mut ad) = (vec![], vec![]);
        fx.emit(&mut al, &mut ad);
        assert!(!al.is_empty() && !ad.is_empty(), "explosions use smoke (alpha) and fire (additive)");
        for p in al.iter().chain(ad.iter()) {
            assert!(p.a.iter().all(|v| v.is_finite()) && p.color.iter().all(|v| v.is_finite()));
        }
        for _ in 0..400 {
            fx.update(0.02);
        }
        assert_eq!(fx.count(), 0, "all particles die eventually");
    }

    #[test]
    fn damage_numbers_only_for_the_players_hits() {
        let mut fx = Fx::new();
        let a = actors();
        fx.on_event(&Event::Damage { target: 1, attacker: Some(0), amount: 30.0, on_shield: false, headshot: true, pos: Vec3::ONE }, &a, 0.0);
        fx.on_event(&Event::Damage { target: 0, attacker: Some(1), amount: 30.0, on_shield: false, headshot: false, pos: Vec3::ONE }, &a, 0.0);
        assert_eq!(fx.numbers.len(), 1);
        assert_eq!(fx.numbers[0].kind, 2);
        for _ in 0..60 {
            fx.update(0.02);
        }
        assert!(fx.numbers.is_empty());
    }

    #[test]
    fn particle_budget_is_capped() {
        let mut fx = Fx::new();
        let a = actors();
        for _ in 0..400 {
            fx.on_event(&Event::Explosion { pos: Vec3::ZERO, radius: 7.0 }, &a, 0.0);
        }
        assert!(fx.count() <= MAX_PARTS);
    }

    #[test]
    fn tracers_travel_from_muzzle_to_target() {
        let mut fx = Fx::new();
        let a = actors();
        fx.on_event(&Event::Tracer { from: Vec3::ZERO, to: Vec3::new(0.0, 0.0, -100.0), weapon: WeaponKind::AssaultRifle }, &a, 0.0);
        assert_eq!(fx.tracers.len(), 1);
        let (mut al, mut ad) = (vec![], vec![]);
        fx.update(0.05);
        fx.emit(&mut al, &mut ad);
        let t = ad.last().unwrap();
        // pointing along -Z
        assert!(t.b[2] < -0.9);
        for _ in 0..50 {
            fx.update(0.02);
        }
        assert!(fx.tracers.is_empty());
    }
}
