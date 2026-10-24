//! Turns game events into positional audio cues (pan, gain, low-pass, travel delay) for the
//! platform layer to play, and computes the levels of the continuous loops.

use super::actor::*;
use super::events::*;
use super::items::*;
use super::*;
use crate::audio_synth::Sfx;
use crate::camera::Camera;
use crate::math::*;
use crate::rng::Rng;

#[derive(Clone, Copy, Debug)]
pub struct AudioCue {
    pub sfx: Sfx,
    pub variant: u32,
    /// -1 (left) .. 1 (right)
    pub pan: f32,
    pub gain: f32,
    pub pitch: f32,
    /// Low-pass cutoff in Hz (>= 18000 means unfiltered).
    pub lowpass: f32,
    /// Seconds of travel time.
    pub delay: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LoopLevels {
    pub bus: f32,
    pub wind: f32,
    pub glider: f32,
    pub storm: f32,
    pub hum: f32,
    pub hum_pan: f32,
}

/// What a sound direction marker on the HUD stands for: the sounds a player most needs to place, somebody walking and
/// somebody shooting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundKind {
    Steps,
    Shot,
}

impl SoundKind {
    /// Seconds a marker stays up after the last sound.
    pub fn life(self) -> f32 {
        match self {
            SoundKind::Steps => 1.2,
            SoundKind::Shot => 2.0,
        }
    }
}

/// A sound the listener heard, for the HUD to point at. One per actor and kind: every new step or shot moves and refreshes
/// it, so a running enemy is a single marker that follows them around.
#[derive(Clone, Copy, Debug)]
pub struct SoundMarker {
    pub kind: SoundKind,
    pub actor: usize,
    pub pos: Vec3,
    /// How close it was: 1 beside the listener, 0 at the edge of its marker range.
    pub level: f32,
    /// `Game::time` the last sound reached the listener (sound takes time to travel, and the marker waits for it like
    /// the cue does).
    pub at: f32,
}

/// More markers than this at once is noise, not information: the weakest goes.
const MAX_MARKERS: usize = 12;

/// Shots leave a marker out to this distance: as far as the host sends shots to the other players (see
/// `net::host::audience`), so a guest's ring shows the same as the host's.
pub const SHOT_MARKER_RANGE: f32 = 220.0;

pub struct Mixer {
    rng: Rng,
    started: bool,
    markers: Vec<SoundMarker>,
    /// Sounds still on their way to the listener: each becomes a marker when it arrives (at its `at`).
    pending: Vec<SoundMarker>,
    /// The actor the markers were heard through (spectating someone else starts afresh).
    marker_listener: usize,
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

struct Spatial {
    pan: f32,
    gain: f32,
    lowpass: f32,
    delay: f32,
}

fn spatial(listener: Vec3, cam: &Camera, pos: Vec3, ref_d: f32, max_d: f32, base: f32) -> Option<Spatial> {
    let to = pos - listener;
    let d = to.length();
    if d > max_d {
        return None;
    }
    let fall = ref_d / (ref_d + (d - ref_d).max(0.0) * 1.15);
    let fade = smoothstep(max_d, max_d * 0.75, d);
    let gain = base * fall * fade;
    if gain < 0.004 {
        return None;
    }
    let flat = Vec2::new(to.x, to.z);
    let right = Vec2::new(cam.right.x, cam.right.z).normalize_or_zero();
    let pan = if flat.length() > 0.01 { flat.normalize().dot(right) * (d / 4.0).min(1.0) } else { 0.0 };
    Some(Spatial { pan: pan.clamp(-0.92, 0.92), gain, lowpass: (19000.0 / (1.0 + d / 40.0)).clamp(900.0, 19000.0), delay: (d / 343.0).min(2.0) })
}

fn shot_sfx(w: WeaponKind) -> Sfx {
    match w {
        WeaponKind::Pistol => Sfx::ShotPistol,
        WeaponKind::Smg => Sfx::ShotSmg,
        WeaponKind::AssaultRifle => Sfx::ShotAr,
        WeaponKind::Shotgun => Sfx::ShotShotgun,
        WeaponKind::Sniper => Sfx::ShotSniper,
        WeaponKind::RocketLauncher => Sfx::RocketFire,
    }
}

impl Mixer {
    pub fn new() -> Mixer {
        Mixer { rng: Rng::new(0xA0D10), started: false, markers: vec![], pending: vec![], marker_listener: usize::MAX }
    }

    /// The footsteps and shots heard lately, for the HUD's direction markers.
    pub fn markers(&self) -> &[SoundMarker] {
        &self.markers
    }

    /// A sound that reaches the listener at `at`, to be marked then; none beyond `range`.
    fn hear(&mut self, kind: SoundKind, actor: usize, pos: Vec3, listener: Vec3, range: f32, at: f32) {
        let d = pos.distance(listener);
        if d <= range {
            self.pending.push(SoundMarker { kind, actor, pos, level: 1.0 - d / range, at });
        }
    }

    fn mark(&mut self, new: SoundMarker, now: f32) {
        if let Some(m) = self.markers.iter_mut().find(|m| m.actor == new.actor && m.kind == new.kind) {
            if new.at >= m.at {
                *m = new;
            }
            return;
        }
        if self.markers.len() >= MAX_MARKERS {
            // full: the weakest goes (faint and nearly faded), unless the new one is weaker still
            let strength = |m: &SoundMarker| m.level * (1.0 - (now - m.at) / m.kind.life());
            let Some(weakest) = (0..self.markers.len()).min_by(|&a, &b| strength(&self.markers[a]).total_cmp(&strength(&self.markers[b]))) else { return };
            if strength(&self.markers[weakest]) >= strength(&new) {
                return;
            }
            self.markers.swap_remove(weakest);
        }
        self.markers.push(new);
    }

    fn variant(&mut self, sfx: Sfx) -> u32 {
        let n = sfx.variants().max(1);
        self.rng.below(n as usize) as u32
    }

    fn two_d(&mut self, out: &mut Vec<AudioCue>, sfx: Sfx, gain: f32) {
        let variant = self.variant(sfx);
        let pitch = self.rng.range(0.97, 1.03);
        out.push(AudioCue { sfx, variant, pan: 0.0, gain, pitch, lowpass: 20000.0, delay: 0.0 });
    }

    /// Queue a positional cue; its travel time, or `None` when it is too far away to be heard.
    #[allow(clippy::too_many_arguments)]
    fn at(&mut self, out: &mut Vec<AudioCue>, listener: Vec3, cam: &Camera, sfx: Sfx, pos: Vec3, ref_d: f32, max_d: f32, base: f32) -> Option<f32> {
        let s = spatial(listener, cam, pos, ref_d, max_d, base)?;
        let variant = self.variant(sfx);
        let pitch = self.rng.range(0.96, 1.04);
        out.push(AudioCue { sfx, variant, pan: s.pan, gain: s.gain, pitch, lowpass: s.lowpass, delay: s.delay });
        Some(s.delay)
    }

    /// Cues for everything that happened during the last `Game::update`.
    pub fn process(&mut self, g: &Game, cam: &Camera, out: &mut Vec<AudioCue>) -> LoopLevels {
        let me = g.camera_actor();
        let listener = g.actors[me].eye_pos();
        if !self.started {
            self.started = true;
            self.two_d(out, Sfx::DropIn, 0.9);
        }
        if self.marker_listener != me {
            self.marker_listener = me;
            self.markers.clear();
            self.pending.clear();
        }
        let now = g.time;
        let alive = |actor: usize| g.actors.get(actor).is_some_and(|a| a.alive);
        self.markers.retain(|m| now >= m.at && now - m.at < m.kind.life() && alive(m.actor));
        // (a sound is never more than 2 s away; one due later than that is from a clock that went back)
        self.pending.retain(|p| p.at - now <= 2.0 && alive(p.actor));
        for e in &g.events {
            match e {
                Event::Shot { actor, pos, weapon, .. } => {
                    if *actor == me {
                        let variant = self.variant(shot_sfx(*weapon));
                        out.push(AudioCue { sfx: shot_sfx(*weapon), variant, pan: 0.0, gain: 0.95, pitch: self.rng.range(0.98, 1.02), lowpass: 20000.0, delay: 0.0 });
                    } else if let Some(delay) = self.at(out, listener, cam, shot_sfx(*weapon), *pos, 10.0, 420.0, 1.0) {
                        self.hear(SoundKind::Shot, *actor, *pos, listener, SHOT_MARKER_RANGE, now + delay);
                    }
                }
                Event::Impact { pos, kind, .. } => {
                    if matches!(kind, ImpactKind::Stone | ImpactKind::Metal | ImpactKind::Wood) && pos.distance(listener) < 30.0 {
                        self.at(out, listener, cam, Sfx::HarvestHit, *pos, 3.0, 30.0, 0.22);
                    }
                }
                Event::HitConfirm { actor, head, shield, kill } if *actor == g.local => {
                    let sfx = if *kill {
                        Sfx::Kill
                    } else if *head {
                        Sfx::HitHead
                    } else if *shield {
                        Sfx::HitShield
                    } else {
                        Sfx::HitMarker
                    };
                    self.two_d(out, sfx, if *kill { 0.9 } else { 0.7 });
                }
                Event::Hurt { actor, from, .. } if *actor == g.local => {
                    let storm = from.is_none() && super::matchflow::in_storm(g, g.actors[me].pos);
                    if storm {
                        self.two_d(out, Sfx::StormDamage, 0.6);
                    } else {
                        self.two_d(out, Sfx::PlayerHurt, 0.85);
                    }
                }
                Event::Eliminated { victim, .. } => {
                    if *victim == g.local {
                        self.two_d(out, Sfx::Defeat, 0.8);
                    }
                }
                Event::Footstep { actor, pos, surface } => {
                    let sfx = match surface {
                        Surface::Grass => Sfx::StepGrass,
                        Surface::Sand => Sfx::StepSand,
                        Surface::Stone => Sfx::StepStone,
                        Surface::Wood => Sfx::StepWood,
                        Surface::Metal => Sfx::StepMetal,
                        Surface::Water => Sfx::StepWater,
                    };
                    if *actor == me {
                        self.two_d(out, sfx, 0.32);
                    } else if let Some(delay) = self.at(out, listener, cam, sfx, *pos, 2.5, 38.0, 0.55) {
                        self.hear(SoundKind::Steps, *actor, *pos, listener, 38.0, now + delay);
                    }
                }
                Event::Jump { actor, pos } => {
                    if *actor == me {
                        self.two_d(out, Sfx::Jump, 0.4);
                    } else if let Some(delay) = self.at(out, listener, cam, Sfx::Jump, *pos, 2.5, 25.0, 0.4) {
                        // a jump or a landing is somebody moving too (a player hopping along makes no footsteps)
                        self.hear(SoundKind::Steps, *actor, *pos, listener, 25.0, now + delay);
                    }
                }
                Event::Land { actor, pos, speed, .. } => {
                    let g_ = (speed / 14.0).clamp(0.25, 1.0) * 0.8;
                    if *actor == me {
                        self.two_d(out, Sfx::Land, g_);
                    } else if let Some(delay) = self.at(out, listener, cam, Sfx::Land, *pos, 3.0, 40.0, g_) {
                        self.hear(SoundKind::Steps, *actor, *pos, listener, 40.0, now + delay);
                    }
                }
                Event::Reload { actor, pos, kind } => {
                    let sfx = if *kind == WeaponKind::Shotgun { Sfx::ReloadShell } else { Sfx::ReloadMag };
                    if *actor == me {
                        self.two_d(out, sfx, 0.8);
                    } else {
                        self.at(out, listener, cam, sfx, *pos, 4.0, 50.0, 0.7);
                    }
                }
                Event::EmptyClick { actor, pos } => {
                    if *actor == me {
                        self.two_d(out, Sfx::EmptyClick, 0.8);
                    } else {
                        self.at(out, listener, cam, Sfx::EmptyClick, *pos, 3.0, 30.0, 0.5);
                    }
                }
                Event::WeaponSwitch { actor, pos } => {
                    if *actor == me {
                        self.two_d(out, Sfx::WeaponSwap, 0.6);
                    } else {
                        self.at(out, listener, cam, Sfx::WeaponSwap, *pos, 3.0, 30.0, 0.45);
                    }
                }
                Event::Pickup { actor, pos, sound, .. } => {
                    let sfx = match sound {
                        PickupSound::Weapon => Sfx::PickupWeapon,
                        PickupSound::Ammo | PickupSound::Material => Sfx::PickupAmmo,
                        PickupSound::Heal => Sfx::PickupHeal,
                    };
                    if *actor == me {
                        self.two_d(out, sfx, 0.8);
                    } else {
                        self.at(out, listener, cam, sfx, *pos, 3.0, 35.0, 0.5);
                    }
                }
                Event::ChestOpen { pos } => {
                    self.at(out, listener, cam, Sfx::ChestOpen, *pos, 8.0, 140.0, 1.0);
                }
                Event::HarvestHit { pos, .. } => {
                    self.at(out, listener, cam, Sfx::HarvestHit, *pos, 5.0, 60.0, 0.9);
                }
                Event::TreeFelled { pos, .. } => {
                    self.at(out, listener, cam, Sfx::HarvestBreak, *pos, 8.0, 100.0, 0.9);
                    self.at(out, listener, cam, Sfx::TreeFall, *pos, 12.0, 170.0, 0.9);
                }
                Event::Built { actor, pos, mat, .. } => {
                    let sfx = match mat {
                        Mat::Wood => Sfx::BuildWood,
                        Mat::Stone => Sfx::BuildStone,
                        Mat::Metal => Sfx::BuildMetal,
                    };
                    if *actor == me {
                        self.two_d(out, sfx, 0.8);
                    } else {
                        self.at(out, listener, cam, sfx, *pos, 6.0, 75.0, 0.8);
                    }
                }
                Event::PieceDestroyed { pos, .. } => {
                    self.at(out, listener, cam, Sfx::PieceDestroy, *pos, 8.0, 110.0, 0.9);
                }
                Event::Explosion { pos, .. } => {
                    self.at(out, listener, cam, Sfx::Explosion, *pos, 25.0, 750.0, 1.0);
                }
                // an impulse is a deep thump and a rush of air, a Boogie Bomb a sparkle (and then the music is the dance)
                Event::Impulse { pos } => {
                    self.at(out, listener, cam, Sfx::Explosion, *pos, 12.0, 300.0, 0.45);
                    self.at(out, listener, cam, Sfx::GliderDeploy, *pos, 8.0, 120.0, 0.9);
                }
                Event::Boogie { pos } => {
                    self.at(out, listener, cam, Sfx::ChestOpen, *pos, 8.0, 140.0, 1.0);
                    self.at(out, listener, cam, Sfx::PickupHeal, *pos, 6.0, 80.0, 0.8);
                }
                Event::Launch { actor, pos } => {
                    if *actor == me {
                        self.two_d(out, Sfx::Jump, 1.0);
                        self.two_d(out, Sfx::GliderDeploy, 0.8);
                    } else {
                        self.at(out, listener, cam, Sfx::Jump, *pos, 6.0, 90.0, 0.9);
                        self.at(out, listener, cam, Sfx::GliderDeploy, *pos, 6.0, 90.0, 0.6);
                    }
                }
                Event::TrapPlaced { actor, pos } => {
                    if *actor == me {
                        self.two_d(out, Sfx::BuildMetal, 0.8);
                    } else {
                        self.at(out, listener, cam, Sfx::BuildMetal, *pos, 6.0, 75.0, 0.7);
                    }
                }
                Event::GliderDeploy { actor, pos } => {
                    if *actor == me {
                        self.two_d(out, Sfx::GliderDeploy, 0.8);
                    } else {
                        self.at(out, listener, cam, Sfx::GliderDeploy, *pos, 6.0, 90.0, 0.6);
                    }
                }
                Event::HealStart { actor, pos } => {
                    let sfx = match g.actors[*actor].inv.selected_item() {
                        Some(Item::Consumable { kind, .. }) => match kind {
                            ConsumableKind::Bandage | ConsumableKind::MedKit => Sfx::HealBandage,
                            ConsumableKind::ShieldSmall | ConsumableKind::ShieldBig => Sfx::ShieldUse,
                            ConsumableKind::ChugJug | ConsumableKind::Slurp => Sfx::HealPotion,
                        },
                        _ => Sfx::HealBandage,
                    };
                    if *actor == me {
                        self.two_d(out, sfx, 0.75);
                    } else {
                        self.at(out, listener, cam, sfx, *pos, 3.0, 35.0, 0.5);
                    }
                }
                Event::StormPhase { phase, shrinking } => {
                    if *shrinking || *phase > 0 {
                        self.two_d(out, Sfx::StormWarning, 0.7);
                    }
                }
                Event::Victory { winner }
                    if *winner == me => {
                        self.two_d(out, Sfx::Victory, 1.0);
                    }
                _ => {}
            }
        }
        // the sounds that have reached the listener become markers
        let (arrived, on_the_way): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending).into_iter().partition(|p| p.at <= now);
        self.pending = on_the_way;
        for p in arrived {
            self.mark(p, now);
        }
        self.loops(g, me, listener, cam)
    }

    fn loops(&mut self, g: &Game, me: usize, listener: Vec3, cam: &Camera) -> LoopLevels {
        let a = &g.actors[me];
        let mut l = LoopLevels::default();
        // the bus: loud on board, fades with distance otherwise
        if g.bus.active {
            l.bus = if a.mode == MoveMode::Bus { 0.55 } else { (1.0 - g.bus.pos.distance(listener) / 320.0).clamp(0.0, 1.0).powf(1.5) * 0.7 };
        }
        match a.mode {
            MoveMode::Freefall => l.wind = (a.vel.length() / 55.0).clamp(0.35, 1.0),
            MoveMode::Glide => {
                l.glider = 0.6;
                l.wind = 0.25;
            }
            _ => {}
        }
        // the storm's hum: stronger the closer to the edge, constant inside it
        if g.storm.active {
            let d = Vec2::new(a.pos.x, a.pos.z).distance(g.storm.center);
            let outside = d > g.storm.radius;
            l.storm = if outside { 0.95 } else { smoothstep(160.0, 0.0, g.storm.radius - d) * 0.55 };
        }
        // unopened chests hum
        let mut best: Option<(f32, Vec3)> = None;
        for c in &g.chests {
            // the golden chests hum; ammo boxes and llamas are quiet, a crate still in the air too
            if !c.opened && !c.falling() && matches!(c.kind, crate::world::ChestKind::Treasure | crate::world::ChestKind::Supply) {
                let d = c.pos.distance(listener);
                if d < 40.0 && best.is_none_or(|b| d < b.0) {
                    best = Some((d, c.pos));
                }
            }
        }
        if let Some((d, p)) = best {
            l.hum = (1.0 - d / 40.0).clamp(0.0, 1.0).powf(1.6) * 0.8;
            let flat = Vec2::new(p.x - listener.x, p.z - listener.z);
            let right = Vec2::new(cam.right.x, cam.right.z).normalize_or_zero();
            l.hum_pan = if flat.length() > 0.1 { (flat.normalize().dot(right) * (d / 4.0).min(1.0)).clamp(-0.9, 0.9) } else { 0.0 };
        }
        l
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::game;
    use super::*;

    fn cam_at(p: Vec3) -> Camera {
        Camera::look(p, Vec3::NEG_Z, 0.0, 1.0, 1.6, 0.1)
    }

    #[test]
    fn distant_shots_are_quieter_delayed_and_panned() {
        let mut g = game(2, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let cam = cam_at(g.actors[PLAYER].eye_pos());
        let me = g.actors[PLAYER].pos;
        let mut m = Mixer::new();
        // near on the right
        g.events.push(Event::Shot { actor: 1, pos: me + Vec3::new(8.0, 1.0, 0.0), weapon: WeaponKind::AssaultRifle, end: me, hit_actor: false });
        // far ahead
        g.events.push(Event::Shot { actor: 1, pos: me + Vec3::new(0.0, 1.0, -300.0), weapon: WeaponKind::AssaultRifle, end: me, hit_actor: false });
        let mut out = vec![];
        m.process(&g, &cam, &mut out);
        let shots: Vec<_> = out.iter().filter(|c| c.sfx == Sfx::ShotAr).collect();
        assert_eq!(shots.len(), 2);
        let near = shots[0];
        let far = shots[1];
        assert!(near.pan > 0.4, "a shot on the right pans right: {}", near.pan);
        assert!(near.gain > far.gain * 2.0, "{} vs {}", near.gain, far.gain);
        assert!(far.delay > 0.7, "sound takes ~0.9 s to travel 300 m: {}", far.delay);
        assert!(far.lowpass < near.lowpass, "distance muffles");
    }

    #[test]
    fn own_events_play_in_2d_and_far_ones_are_dropped() {
        let mut g = game(2, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let cam = cam_at(g.actors[PLAYER].eye_pos());
        let mut m = Mixer::new();
        let mut out = vec![];
        m.process(&g, &cam, &mut out);
        out.clear();
        let me = g.actors[PLAYER].pos;
        g.events.push(Event::Shot { actor: PLAYER, pos: me, weapon: WeaponKind::Shotgun, end: me, hit_actor: false });
        g.events.push(Event::Footstep { actor: 1, pos: me + Vec3::new(200.0, 0.0, 0.0), surface: Surface::Grass });
        g.events.push(Event::HitConfirm { actor: PLAYER, head: true, shield: false, kill: false });
        m.process(&g, &cam, &mut out);
        assert!(out.iter().any(|c| c.sfx == Sfx::ShotShotgun && c.pan == 0.0 && c.delay == 0.0));
        assert!(out.iter().any(|c| c.sfx == Sfx::HitHead));
        assert!(!out.iter().any(|c| c.sfx == Sfx::StepGrass), "footsteps 200 m away are inaudible");
    }

    /// Advance the clock by `dt`, let the mixer hear the events queued since, then clear them.
    fn tick(g: &mut Game, m: &mut Mixer, dt: f32) {
        g.time += dt;
        let cam = cam_at(g.actors[PLAYER].eye_pos());
        m.process(g, &cam, &mut vec![]);
        g.events.clear();
    }

    fn marked(m: &Mixer) -> Vec<(SoundKind, usize)> {
        m.markers().iter().map(|k| (k.kind, k.actor)).collect()
    }

    fn steps(actor: usize, pos: Vec3) -> Event {
        Event::Footstep { actor, pos, surface: Surface::Grass }
    }

    #[test]
    fn heard_steps_and_shots_leave_direction_markers_that_fade() {
        let mut g = game(4, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        g.events.clear();
        let me = g.actors[PLAYER].pos;
        let mut m = Mixer::new();
        g.events.push(steps(1, me + Vec3::new(6.0, 0.0, 0.0)));
        g.events.push(steps(PLAYER, me));
        g.events.push(steps(2, me + Vec3::new(200.0, 0.0, 0.0)));
        g.events.push(Event::Shot { actor: 3, pos: me + Vec3::new(0.0, 1.0, -150.0), weapon: WeaponKind::Sniper, end: me, hit_actor: false });
        // audible, but further away than the host tells the other players about
        g.events.push(Event::Shot { actor: 2, pos: me + Vec3::new(-300.0, 1.0, 0.0), weapon: WeaponKind::Sniper, end: me, hit_actor: false });
        tick(&mut g, &mut m, 0.0);
        assert!(m.markers().is_empty(), "a sound is marked when it arrives, not when it is made");
        tick(&mut g, &mut m, 0.1);
        assert_eq!(marked(&m), vec![(SoundKind::Steps, 1)], "the steps 6 m away have arrived, the shot 150 m away (0.44 s) not yet");
        tick(&mut g, &mut m, 0.4);
        assert_eq!(marked(&m), vec![(SoundKind::Steps, 1), (SoundKind::Shot, 3)], "own steps, inaudible ones and shots beyond the marker range leave none");
        let first = m.markers()[0];
        assert!(first.level > 0.8, "6 m away is close: {}", first.level);
        assert!(m.markers()[1].level < 0.4, "150 m of the 220 m range is not: {}", m.markers()[1].level);

        // the same walker keeps one marker, which follows them
        g.events.push(steps(1, me + Vec3::new(20.0, 0.0, 0.0)));
        tick(&mut g, &mut m, 0.2);
        tick(&mut g, &mut m, 0.1);
        let walker: Vec<_> = m.markers().iter().filter(|k| k.actor == 1).collect();
        assert_eq!(walker.len(), 1);
        assert_eq!(walker[0].pos.x, me.x + 20.0);
        assert!(walker[0].level < first.level);

        // steps fade before shots, and both are gone after a few quiet seconds
        tick(&mut g, &mut m, 1.3);
        assert_eq!(marked(&m), vec![(SoundKind::Shot, 3)]);
        tick(&mut g, &mut m, 0.5);
        assert!(m.markers().is_empty());

        // a marker goes with the one who made it, and so does a sound still on its way
        g.events.push(Event::Footstep { actor: 2, pos: me + Vec3::new(0.0, 0.0, 5.0), surface: Surface::Wood });
        tick(&mut g, &mut m, 0.0);
        tick(&mut g, &mut m, 0.1);
        assert_eq!(marked(&m), vec![(SoundKind::Steps, 2)]);
        g.actors[2].alive = false;
        g.events.push(steps(1, me + Vec3::new(0.0, 0.0, 30.0)));
        tick(&mut g, &mut m, 0.0);
        g.actors[1].alive = false;
        tick(&mut g, &mut m, 0.2);
        assert!(m.markers().is_empty());
    }

    #[test]
    fn jumps_and_landings_count_as_steps() {
        // a player hopping along makes no footsteps at all, only jumps and landings
        let mut g = game(3, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        g.events.clear();
        let me = g.actors[PLAYER].pos;
        let mut m = Mixer::new();
        g.events.push(Event::Jump { actor: 1, pos: me + Vec3::new(8.0, 0.0, 0.0) });
        g.events.push(Event::Land { actor: 2, pos: me + Vec3::new(0.0, 0.0, 15.0), speed: 8.0, surface: Surface::Grass });
        g.events.push(Event::Jump { actor: 3, pos: me + Vec3::new(0.0, 0.0, -60.0) });
        tick(&mut g, &mut m, 0.0);
        tick(&mut g, &mut m, 0.1);
        assert_eq!(marked(&m), vec![(SoundKind::Steps, 1), (SoundKind::Steps, 2)], "a jump 60 m away is not heard");
    }

    #[test]
    fn a_full_ring_drops_its_weakest_marker() {
        let mut g = game(14, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        g.events.clear();
        let me = g.actors[PLAYER].pos;
        let mut m = Mixer::new();
        // eleven walkers close by and one a little further away
        for a in 1..=11 {
            g.events.push(steps(a, me + Vec3::new(5.0, 0.0, a as f32 * 0.1)));
        }
        g.events.push(steps(12, me + Vec3::new(0.0, 0.0, 15.0)));
        tick(&mut g, &mut m, 0.0);
        tick(&mut g, &mut m, 0.2);
        assert_eq!(m.markers().len(), MAX_MARKERS);
        // a faint shot far away pushes nobody out
        g.events.push(Event::Shot { actor: 13, pos: me + Vec3::new(0.0, 1.0, 210.0), weapon: WeaponKind::Pistol, end: me, hit_actor: false });
        tick(&mut g, &mut m, 0.0);
        tick(&mut g, &mut m, 0.7);
        assert!(!m.markers().iter().any(|k| k.actor == 13), "{:?}", marked(&m));
        // a shot close by does, and the one that goes is the faint walker
        g.events.push(Event::Shot { actor: 13, pos: me + Vec3::new(0.0, 1.0, -10.0), weapon: WeaponKind::Shotgun, end: me, hit_actor: false });
        tick(&mut g, &mut m, 0.0);
        tick(&mut g, &mut m, 0.1);
        assert_eq!(m.markers().len(), MAX_MARKERS);
        assert!(m.markers().iter().any(|k| k.actor == 13));
        assert!(!m.markers().iter().any(|k| k.actor == 12), "{:?}", marked(&m));
    }

    #[test]
    fn loops_follow_the_situation() {
        let mut g = game(2, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let cam = cam_at(g.actors[PLAYER].eye_pos());
        let mut m = Mixer::new();
        let mut out = vec![];
        let l = m.process(&g, &cam, &mut out);
        assert_eq!(l.wind, 0.0);
        assert_eq!(l.glider, 0.0);
        g.actors[PLAYER].mode = MoveMode::Glide;
        let l = m.process(&g, &cam, &mut out);
        assert!(l.glider > 0.3);
        g.actors[PLAYER].mode = MoveMode::Freefall;
        g.actors[PLAYER].vel = Vec3::new(0.0, -50.0, 0.0);
        let l = m.process(&g, &cam, &mut out);
        assert!(l.wind > 0.7);
        // standing beside a closed golden chest the hum is audible
        g.actors[PLAYER].mode = MoveMode::Ground;
        let chest = g.chests.iter().find(|c| c.kind == crate::world::ChestKind::Treasure).expect("a golden chest");
        g.actors[PLAYER].pos = chest.pos + Vec3::new(1.5, 0.0, 0.0);
        let l = m.process(&g, &cam, &mut out);
        assert!(l.hum > 0.3, "chest hum {}", l.hum);
    }
}
