//! The messages between a host and the players in its room, and how they look on the wire.
//!
//! Two channels carry them. The *reliable, ordered* one has everything that must not get lost: joining, the lobby, the
//! start of the match, changes to the loot, chests and buildings, and the one-shot events (shots, hits, pickups, ...). The
//! *unreliable* one has what is replaced every few milliseconds anyway: a player's commands going up, and snapshots of the
//! actors, the storm and the player's own state coming down.
//!
//! Every decoder checks what it reads (counts against limits, indices against tables, floats for NaN) and returns an
//! error rather than panicking, because the bytes come from another machine.

use super::wire::*;
use crate::game::actor::{Action, MoveMode, PieceKind};
use crate::game::cmd::{btn, Cmd};
use crate::game::events::*;
use crate::game::items::*;
use crate::game::pieces::PieceKey;
use crate::game::{Difficulty, GameConfig, GameMode, Phase, PickupKind};
use crate::game::vehicles::Vehicle;
use crate::math::*;

/// Bump when anything in this file changes shape; hosts and clients of different versions refuse each other.
pub const VERSION: u16 = 2;
/// Most people in one room, the host included.
pub const MAX_HUMANS: usize = 8;
/// Most commands a single packet may carry.
pub const MAX_CMDS: usize = 16;
const MAX_ACTORS: usize = 160;
const MAX_PROJECTILES: usize = 64;
const MAX_VEHICLES: usize = 64;
const MAX_OPS: usize = 4096;
const MAX_EVENTS: usize = 1024;

// ---- the match every player builds the same way ---------------------------------------------------------------------

/// Everything needed to build the same [`crate::game::Game`] on every machine.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchSetup {
    pub seed: u32,
    pub mode: GameMode,
    pub bots: u16,
    pub difficulty: Difficulty,
    pub skip_bus: bool,
    pub storm_speed: f32,
    pub start_mats: u16,
    /// The humans' names in actor order: the host first.
    pub names: Vec<String>,
}

impl MatchSetup {
    pub fn to_config(&self) -> GameConfig {
        GameConfig {
            seed: self.seed as u64,
            mode: self.mode,
            bots: self.bots as usize,
            difficulty: self.difficulty,
            player_name: self.names.first().cloned().unwrap_or_else(|| "Host".into()),
            player_outfit: 0,
            humans: self.names.len().max(1),
            human_names: self.names.iter().skip(1).cloned().collect(),
            start_mats: self.start_mats as u32,
            skip_bus: self.skip_bus,
            storm_speed: self.storm_speed,
            god_mode: false,
        }
    }

    fn write(&self, w: &mut Writer) {
        w.u32(self.seed);
        w.u8(match self.mode { GameMode::BattleRoyale => 0, GameMode::ZeroBuild => 1, GameMode::Lego => 2 });
        w.u16(self.bots);
        w.u8(match self.difficulty {
            Difficulty::Easy => 0,
            Difficulty::Normal => 1,
            Difficulty::Hard => 2,
        });
        w.bool(self.skip_bus);
        w.f32(self.storm_speed);
        w.u16(self.start_mats);
        w.u8(self.names.len() as u8);
        for n in &self.names {
            w.str(n);
        }
    }

    fn read(r: &mut Reader) -> Result<Self> {
        let seed = r.u32()?;
        let mode = match r.u8()? { 0 => GameMode::BattleRoyale, 1 => GameMode::ZeroBuild, 2 => GameMode::Lego, _ => return Err(WireError::Invalid("game mode")) };
        let bots = r.u16()?;
        if bots > 120 {
            return Err(WireError::Invalid("bots"));
        }
        let difficulty = match r.u8()? {
            0 => Difficulty::Easy,
            1 => Difficulty::Normal,
            2 => Difficulty::Hard,
            _ => return Err(WireError::Invalid("difficulty")),
        };
        let skip_bus = r.bool()?;
        let storm_speed = r.f32()?;
        if !(0.05..=100.0).contains(&storm_speed) {
            return Err(WireError::Invalid("storm speed"));
        }
        let start_mats = r.u16()?;
        let n = r.u8()? as usize;
        if n == 0 || n > MAX_HUMANS {
            return Err(WireError::Invalid("players"));
        }
        let names = (0..n).map(|_| r.str()).collect::<Result<Vec<_>>>()?;
        Ok(MatchSetup { seed, mode, bots, difficulty, skip_bus, storm_speed, start_mats, names })
    }
}

// ---- pieces of state ------------------------------------------------------------------------------------------------

/// What a snapshot says about the storm; the rest is the same on every machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StormNet {
    pub active: bool,
    pub phase: u8,
    pub shrinking: bool,
    pub done: bool,
    pub timer: f32,
    pub duration: f32,
    pub center: Vec2,
    pub radius: f32,
    pub from_center: Vec2,
    pub from_radius: f32,
    pub to_center: Vec2,
    pub to_radius: f32,
    pub dmg: f32,
}

/// Everything about an actor's movement that decides where its next step lands (see `game::cmd`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveState {
    pub pos: Vec3,
    pub vel: Vec3,
    pub mode: MoveMode,
    pub on_ground: bool,
    pub crouching: bool,
    pub sprinting: bool,
    pub ads: bool,
    pub glide_deployed: bool,
    pub emoting: bool,
    pub coyote: f32,
    pub jump_buffer: f32,
    pub peak_y: f32,
    pub steps: f32,
    /// The animation clock: swimming bobs on it, so where the actor floats depends on it.
    pub anim_time: f32,
}

/// A player's own actor in full: what the HUD shows and what their prediction starts from.
#[derive(Clone, Debug, PartialEq)]
pub struct Own {
    pub hp: f32,
    pub shield: f32,
    pub alive: bool,
    pub kills: u16,
    pub placement: u16,
    pub damage_dealt: f32,
    pub survived: f32,
    pub slots: [Option<Item>; 6],
    pub selected: u8,
    pub ammo: [u16; 5],
    pub mats: [u16; 3],
    pub action: Action,
    pub build_mode: bool,
    pub build_piece: PieceKind,
    pub build_mat: Mat,
    pub mv: MoveState,
}

/// The item an actor holds, as others see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Held {
    Nothing,
    Pickaxe,
    Weapon { kind: WeaponKind, rarity: Rarity },
    Consumable { kind: ConsumableKind },
}

/// What a snapshot says about any actor: enough to draw and animate it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorNet {
    pub id: u8,
    pub alive: bool,
    pub mode: MoveMode,
    pub on_ground: bool,
    pub crouching: bool,
    pub sprinting: bool,
    pub ads: bool,
    pub build_mode: bool,
    pub emoting: bool,
    pub glide_deployed: bool,
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub body_yaw: f32,
    pub hp: u8,
    pub shield: u8,
    pub held: Held,
    /// 0 nothing, 1 reloading, 2 healing, 3 swapping weapons.
    pub action: u8,
    /// How far along the action is, 0..=1.
    pub progress: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjNet {
    pub pos: Vec3,
    pub vel: Vec3,
    pub owner: u8,
    pub kind: WeaponKind,
    pub rarity: Rarity,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// The host's game clock.
    pub time: f32,
    /// The `time` of the newest command packet the host had from this client when the snapshot was made (round-trip time).
    pub echo: u32,
    /// The newest command of this client the host has dealt with.
    pub ack: u32,
    pub phase: Phase,
    pub tie: bool,
    pub winner: Option<usize>,
    pub match_time: f32,
    pub bus_active: bool,
    pub bus_t: f32,
    pub storm: StormNet,
    pub own: Own,
    pub actors: Vec<ActorNet>,
    pub projectiles: Vec<ProjNet>,
    /// Host-owned cars, including their exclusive driver seat.
    pub vehicles: Vec<Vehicle>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickupNet {
    pub id: u32,
    pub pos: Vec3,
    pub vel: Vec3,
    pub kind: PickupKind,
    pub grounded: bool,
    pub spin: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieceNet {
    pub id: u32,
    pub key: PieceKey,
    pub mat: Mat,
    pub base_y: f32,
    pub owner: u8,
    pub footing: f32,
    pub hp: f32,
}

/// A change to the things lying around the island, for everyone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SyncOp {
    /// Forget every pickup, chest, piece and felled tree: a full picture follows.
    Reset,
    PickupAdd(PickupNet),
    PickupRemove(u32),
    /// The amount left in an ammo box or a stack of consumables.
    PickupAmount { id: u32, amount: u32 },
    ChestOpen(u32),
    PieceAdd(PieceNet),
    PieceRemove(u32),
    PieceHp { id: u32, hp: f32 },
    /// A tree, rock or ore vein was broken: the index in the world's list of harvestables, and which way a tree falls.
    HarvestBroken { idx: u32, dir: Vec2 },
}

// ---- messages -------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum ClientMsg {
    Hello { version: u16, name: String },
    /// Commands for the host to apply (see `game::cmd`): the newest ones, repeated in every packet so a lost packet costs nothing.
    Cmds { time: u32, cmds: Vec<Cmd> },
    Bye,
    /// The island is built and the match can begin.
    Ready,
}

#[derive(Clone, Debug)]
pub enum ServerMsg {
    /// The room has a place for you: you are this actor (counting the host as 0) and these people are in.
    Welcome { you: u8, names: Vec<String> },
    /// The people in the room changed.
    Lobby { names: Vec<String> },
    Start { you: u8, setup: MatchSetup },
    Snapshot(Box<Snapshot>),
    Sync(Vec<SyncOp>),
    Events { time: f32, events: Vec<Event> },
    Reject { reason: String },
    /// The host closed the room.
    Closed,
}

// ---- helpers: tables and enums --------------------------------------------------------------------------------------

/// Names an elimination can be blamed on (see `Event::Eliminated`).
const SOURCES: [&str; 9] = ["Pistol", "Submachine Gun", "Assault Rifle", "Pump Shotgun", "Bolt-Action Sniper", "Rocket Launcher", "Harvesting Tool", "The Storm", "Fall damage"];

fn source_id(name: &str) -> u8 {
    SOURCES.iter().position(|s| *s == name).map_or(255, |i| i as u8)
}

fn pickup_names() -> Vec<&'static str> {
    WeaponKind::ALL.iter().map(|k| k.name()).chain(AmmoKind::ALL.iter().map(|k| k.name())).chain(ConsumableKind::ALL.iter().map(|k| k.name())).collect()
}

fn pickup_name_id(name: &str) -> u8 {
    pickup_names().iter().position(|s| *s == name).map_or(255, |i| i as u8)
}

fn read_weapon(r: &mut Reader) -> Result<WeaponKind> {
    Ok(WeaponKind::ALL[r.index(WeaponKind::ALL.len(), "weapon")?])
}
fn read_rarity(r: &mut Reader) -> Result<Rarity> {
    Ok(Rarity::ALL[r.index(Rarity::ALL.len(), "rarity")?])
}
fn read_consumable(r: &mut Reader) -> Result<ConsumableKind> {
    Ok(ConsumableKind::ALL[r.index(ConsumableKind::ALL.len(), "consumable")?])
}
fn read_ammo(r: &mut Reader) -> Result<AmmoKind> {
    Ok(AmmoKind::ALL[r.index(AmmoKind::ALL.len(), "ammo")?])
}
fn read_mat(r: &mut Reader) -> Result<Mat> {
    Ok(Mat::ALL[r.index(Mat::ALL.len(), "material")?])
}
fn read_piece(r: &mut Reader) -> Result<PieceKind> {
    Ok(PieceKind::ALL[r.index(PieceKind::ALL.len(), "piece")?])
}
fn read_mode(r: &mut Reader) -> Result<MoveMode> {
    Ok(MoveMode::ALL[r.index(MoveMode::ALL.len(), "move mode")?])
}
fn read_surface(r: &mut Reader) -> Result<Surface> {
    Ok(Surface::ALL[r.index(Surface::ALL.len(), "surface")?])
}

fn idx<T: Copy + PartialEq>(all: &[T], v: T) -> u8 {
    all.iter().position(|x| *x == v).unwrap_or(0) as u8
}

// ---- items, pickups -------------------------------------------------------------------------------------------------

fn write_item(w: &mut Writer, it: &Option<Item>) {
    match it {
        None => w.u8(0),
        Some(Item::Pickaxe) => w.u8(1),
        Some(Item::Weapon { kind, rarity, ammo }) => {
            w.u8(2);
            w.u8(idx(&WeaponKind::ALL, *kind));
            w.u8(idx(&Rarity::ALL, *rarity));
            w.u16((*ammo).min(65535) as u16);
        }
        Some(Item::Consumable { kind, count }) => {
            w.u8(3);
            w.u8(idx(&ConsumableKind::ALL, *kind));
            w.u16((*count).min(65535) as u16);
        }
    }
}

fn read_item(r: &mut Reader) -> Result<Option<Item>> {
    Ok(match r.u8()? {
        0 => None,
        1 => Some(Item::Pickaxe),
        2 => {
            let kind = read_weapon(r)?;
            let rarity = read_rarity(r)?;
            Some(Item::Weapon { kind, rarity, ammo: r.u16()? as u32 })
        }
        3 => {
            let kind = read_consumable(r)?;
            Some(Item::Consumable { kind, count: r.u16()? as u32 })
        }
        _ => return Err(WireError::Invalid("item")),
    })
}

fn write_pickup_kind(w: &mut Writer, k: &PickupKind) {
    match k {
        PickupKind::Weapon { kind, rarity, ammo } => {
            w.u8(0);
            w.u8(idx(&WeaponKind::ALL, *kind));
            w.u8(idx(&Rarity::ALL, *rarity));
            w.u16((*ammo).min(65535) as u16);
        }
        PickupKind::Ammo { kind, amount } => {
            w.u8(1);
            w.u8(idx(&AmmoKind::ALL, *kind));
            w.u16((*amount).min(65535) as u16);
        }
        PickupKind::Consumable { kind, count } => {
            w.u8(2);
            w.u8(idx(&ConsumableKind::ALL, *kind));
            w.u16((*count).min(65535) as u16);
        }
    }
}

fn read_pickup_kind(r: &mut Reader) -> Result<PickupKind> {
    Ok(match r.u8()? {
        0 => {
            let kind = read_weapon(r)?;
            let rarity = read_rarity(r)?;
            PickupKind::Weapon { kind, rarity, ammo: r.u16()? as u32 }
        }
        1 => {
            let kind = read_ammo(r)?;
            PickupKind::Ammo { kind, amount: r.u16()? as u32 }
        }
        2 => {
            let kind = read_consumable(r)?;
            PickupKind::Consumable { kind, count: r.u16()? as u32 }
        }
        _ => return Err(WireError::Invalid("pickup kind")),
    })
}

fn write_action(w: &mut Writer, a: &Action) {
    match *a {
        Action::None => w.u8(0),
        Action::Reload { t, dur } => {
            w.u8(1);
            w.f32(t);
            w.f32(dur);
        }
        Action::Heal { slot, t, dur } => {
            w.u8(2);
            w.u8(slot.min(5) as u8);
            w.f32(t);
            w.f32(dur);
        }
        Action::Swap { t } => {
            w.u8(3);
            w.f32(t);
        }
    }
}

fn read_action(r: &mut Reader) -> Result<Action> {
    Ok(match r.u8()? {
        0 => Action::None,
        1 => Action::Reload { t: r.f32()?, dur: r.f32()?.max(0.01) },
        2 => {
            let slot = r.index(6, "slot")?;
            Action::Heal { slot, t: r.f32()?, dur: r.f32()?.max(0.01) }
        }
        3 => Action::Swap { t: r.f32()? },
        _ => return Err(WireError::Invalid("action")),
    })
}

// ---- snapshot -------------------------------------------------------------------------------------------------------

impl MoveState {
    fn write(&self, w: &mut Writer) {
        w.vec3(self.pos);
        w.vec3(self.vel);
        w.u8(idx(&MoveMode::ALL, self.mode));
        let mut f = 0u8;
        for (k, on) in [self.on_ground, self.crouching, self.sprinting, self.ads, self.glide_deployed, self.emoting].into_iter().enumerate() {
            f |= (on as u8) << k;
        }
        w.u8(f);
        w.f32(self.coyote);
        w.f32(self.jump_buffer);
        w.f32(self.peak_y);
        w.f32(self.steps);
        w.f32(self.anim_time);
    }

    fn read(r: &mut Reader) -> Result<Self> {
        let pos = r.vec3()?;
        let vel = r.vec3()?;
        let mode = read_mode(r)?;
        let f = r.u8()?;
        let on = |k: u8| f & (1 << k) != 0;
        Ok(MoveState { pos, vel, mode, on_ground: on(0), crouching: on(1), sprinting: on(2), ads: on(3), glide_deployed: on(4), emoting: on(5), coyote: r.f32()?, jump_buffer: r.f32()?, peak_y: r.f32()?, steps: r.f32()?, anim_time: r.f32()? })
    }
}

impl Own {
    fn write(&self, w: &mut Writer) {
        w.f32(self.hp);
        w.f32(self.shield);
        w.bool(self.alive);
        w.u16(self.kills);
        w.u16(self.placement);
        w.f32(self.damage_dealt);
        w.f32(self.survived);
        for s in &self.slots {
            write_item(w, s);
        }
        w.u8(self.selected);
        for a in self.ammo {
            w.u16(a);
        }
        for m in self.mats {
            w.u16(m);
        }
        write_action(w, &self.action);
        w.bool(self.build_mode);
        w.u8(idx(&PieceKind::ALL, self.build_piece));
        w.u8(idx(&Mat::ALL, self.build_mat));
        self.mv.write(w);
    }

    fn read(r: &mut Reader) -> Result<Self> {
        let hp = r.f32()?;
        let shield = r.f32()?;
        let alive = r.bool()?;
        let kills = r.u16()?;
        let placement = r.u16()?;
        let damage_dealt = r.f32()?;
        let survived = r.f32()?;
        let mut slots = [None; 6];
        for s in &mut slots {
            *s = read_item(r)?;
        }
        let selected = r.index(6, "selected slot")? as u8;
        let mut ammo = [0u16; 5];
        for a in &mut ammo {
            *a = r.u16()?;
        }
        let mut mats = [0u16; 3];
        for m in &mut mats {
            *m = r.u16()?;
        }
        let action = read_action(r)?;
        let build_mode = r.bool()?;
        let build_piece = read_piece(r)?;
        let build_mat = read_mat(r)?;
        let mv = MoveState::read(r)?;
        Ok(Own { hp, shield, alive, kills, placement, damage_dealt, survived, slots, selected, ammo, mats, action, build_mode, build_piece, build_mat, mv })
    }
}

impl ActorNet {
    fn write(&self, w: &mut Writer) {
        w.u8(self.id);
        let mut f = 0u8;
        for (k, on) in [self.alive, self.on_ground, self.crouching, self.sprinting, self.ads, self.build_mode, self.emoting, self.glide_deployed].into_iter().enumerate() {
            f |= (on as u8) << k;
        }
        w.u8(f);
        w.u8(idx(&MoveMode::ALL, self.mode));
        w.vec3(self.pos);
        for v in [self.vel.x, self.vel.y, self.vel.z] {
            w.i16((v * 32.0).round().clamp(-32767.0, 32767.0) as i16);
        }
        w.angle(self.yaw);
        w.i16((self.pitch * 10000.0).round().clamp(-15700.0, 15700.0) as i16);
        w.angle(self.body_yaw);
        w.u8(self.hp);
        w.u8(self.shield);
        match self.held {
            Held::Nothing => w.u8(0),
            Held::Pickaxe => w.u8(1),
            Held::Weapon { kind, rarity } => {
                w.u8(2);
                w.u8(idx(&WeaponKind::ALL, kind));
                w.u8(idx(&Rarity::ALL, rarity));
            }
            Held::Consumable { kind } => {
                w.u8(3);
                w.u8(idx(&ConsumableKind::ALL, kind));
            }
        }
        w.u8(self.action);
        w.unit(self.progress);
    }

    fn read(r: &mut Reader) -> Result<Self> {
        let id = r.u8()?;
        let f = r.u8()?;
        let on = |k: u8| f & (1 << k) != 0;
        let mode = read_mode(r)?;
        let pos = r.vec3()?;
        let vel = Vec3::new(r.i16()? as f32 / 32.0, r.i16()? as f32 / 32.0, r.i16()? as f32 / 32.0);
        let yaw = r.angle()?;
        let pitch = r.i16()? as f32 / 10000.0;
        let body_yaw = r.angle()?;
        let hp = r.u8()?;
        let shield = r.u8()?;
        let held = match r.u8()? {
            0 => Held::Nothing,
            1 => Held::Pickaxe,
            2 => {
                let kind = read_weapon(r)?;
                Held::Weapon { kind, rarity: read_rarity(r)? }
            }
            3 => Held::Consumable { kind: read_consumable(r)? },
            _ => return Err(WireError::Invalid("held item")),
        };
        let action = r.u8()?;
        if action > 3 {
            return Err(WireError::Invalid("action"));
        }
        let progress = r.unit()?;
        Ok(ActorNet { id, alive: on(0), mode, on_ground: on(1), crouching: on(2), sprinting: on(3), ads: on(4), build_mode: on(5), emoting: on(6), glide_deployed: on(7), pos, vel, yaw, pitch, body_yaw, hp, shield, held, action, progress })
    }
}

impl Snapshot {
    fn write(&self, w: &mut Writer) {
        w.f32(self.time);
        w.u32(self.echo);
        w.u32(self.ack);
        w.u8(match self.phase {
            Phase::Bus => 0,
            Phase::Playing => 1,
            Phase::Over => 2,
        });
        w.bool(self.tie);
        w.opt_u8(self.winner);
        w.f32(self.match_time);
        w.bool(self.bus_active);
        w.f32(self.bus_t);
        let s = &self.storm;
        w.bool(s.active);
        w.u8(s.phase);
        w.bool(s.shrinking);
        w.bool(s.done);
        w.f32(s.timer);
        w.f32(s.duration);
        w.vec2(s.center);
        w.f32(s.radius);
        w.vec2(s.from_center);
        w.f32(s.from_radius);
        w.vec2(s.to_center);
        w.f32(s.to_radius);
        w.f32(s.dmg);
        self.own.write(w);
        w.u16(self.actors.len() as u16);
        for a in &self.actors {
            a.write(w);
        }
        w.u16(self.projectiles.len() as u16);
        for p in &self.projectiles {
            w.vec3(p.pos);
            w.vec3(p.vel);
            w.u8(p.owner);
            w.u8(idx(&WeaponKind::ALL, p.kind));
            w.u8(idx(&Rarity::ALL, p.rarity));
        }
        w.u16(self.vehicles.len() as u16);
        for v in &self.vehicles {
            w.u32(v.id);
            w.vec3(v.pos);
            w.f32(v.yaw);
            w.f32(v.speed);
            w.f32(v.steer);
            w.opt_u8(v.driver);
        }
    }

    fn read(r: &mut Reader) -> Result<Self> {
        let time = r.f32()?;
        let echo = r.u32()?;
        let ack = r.u32()?;
        let phase = match r.u8()? {
            0 => Phase::Bus,
            1 => Phase::Playing,
            2 => Phase::Over,
            _ => return Err(WireError::Invalid("phase")),
        };
        let tie = r.bool()?;
        let winner = r.opt_u8()?;
        let match_time = r.f32()?;
        let bus_active = r.bool()?;
        let bus_t = r.f32()?;
        let storm = StormNet {
            active: r.bool()?,
            phase: r.u8()?,
            shrinking: r.bool()?,
            done: r.bool()?,
            timer: r.f32()?,
            duration: r.f32()?,
            center: r.vec2()?,
            radius: r.f32()?,
            from_center: r.vec2()?,
            from_radius: r.f32()?,
            to_center: r.vec2()?,
            to_radius: r.f32()?,
            dmg: r.f32()?,
        };
        let own = Own::read(r)?;
        let n = r.count(MAX_ACTORS, "actors")?;
        let actors = (0..n).map(|_| ActorNet::read(r)).collect::<Result<Vec<_>>>()?;
        let n = r.count(MAX_PROJECTILES, "projectiles")?;
        let mut projectiles = Vec::with_capacity(n);
        for _ in 0..n {
            projectiles.push(ProjNet { pos: r.vec3()?, vel: r.vec3()?, owner: r.u8()?, kind: read_weapon(r)?, rarity: read_rarity(r)? });
        }
        let n = r.count(MAX_VEHICLES, "vehicles")?;
        let mut vehicles = Vec::with_capacity(n);
        let mut drivers = std::collections::HashSet::new();
        for _ in 0..n {
            let id = r.u32()?;
            let pos = r.vec3()?;
            let yaw = r.f32()?;
            let speed = r.f32()?;
            let steer = r.f32()?;
            let driver = r.opt_u8()?;
            if speed.abs() > 100.0 || steer.abs() > 1.01 || driver.is_some_and(|d| d >= MAX_HUMANS || !drivers.insert(d)) || vehicles.iter().any(|v: &Vehicle| v.id == id) {
                return Err(WireError::Invalid("vehicle state"));
            }
            vehicles.push(Vehicle { id, pos, yaw, speed, steer, driver });
        }
        Ok(Snapshot { time, echo, ack, phase, tie, winner, match_time, bus_active, bus_t, storm, own, actors, projectiles, vehicles })
    }
}

// ---- world changes --------------------------------------------------------------------------------------------------

impl SyncOp {
    fn write(&self, w: &mut Writer) {
        match self {
            SyncOp::Reset => w.u8(0),
            SyncOp::PickupAdd(p) => {
                w.u8(1);
                w.u32(p.id);
                w.vec3(p.pos);
                w.vec3(p.vel);
                write_pickup_kind(w, &p.kind);
                w.bool(p.grounded);
                w.angle(p.spin);
            }
            SyncOp::PickupRemove(id) => {
                w.u8(2);
                w.u32(*id);
            }
            SyncOp::PickupAmount { id, amount } => {
                w.u8(3);
                w.u32(*id);
                w.u16((*amount).min(65535) as u16);
            }
            SyncOp::ChestOpen(id) => {
                w.u8(4);
                w.u32(*id);
            }
            SyncOp::PieceAdd(p) => {
                w.u8(5);
                w.u32(p.id);
                w.u8(idx(&PieceKind::ALL, p.key.kind));
                w.i32(p.key.x);
                w.i32(p.key.z);
                w.i32(p.key.level);
                w.u8(p.key.dir);
                w.u8(idx(&Mat::ALL, p.mat));
                w.f32(p.base_y);
                w.u8(p.owner);
                w.f32(p.footing);
                w.f32(p.hp);
            }
            SyncOp::PieceRemove(id) => {
                w.u8(6);
                w.u32(*id);
            }
            SyncOp::PieceHp { id, hp } => {
                w.u8(7);
                w.u32(*id);
                w.f32(*hp);
            }
            SyncOp::HarvestBroken { idx, dir } => {
                w.u8(8);
                w.u32(*idx);
                w.vec2(*dir);
            }
        }
    }

    fn read(r: &mut Reader) -> Result<Self> {
        Ok(match r.u8()? {
            0 => SyncOp::Reset,
            1 => {
                let id = r.u32()?;
                let pos = r.vec3()?;
                let vel = r.vec3()?;
                let kind = read_pickup_kind(r)?;
                let grounded = r.bool()?;
                let spin = r.angle()?;
                SyncOp::PickupAdd(PickupNet { id, pos, vel, kind, grounded, spin })
            }
            2 => SyncOp::PickupRemove(r.u32()?),
            3 => SyncOp::PickupAmount { id: r.u32()?, amount: r.u16()? as u32 },
            4 => SyncOp::ChestOpen(r.u32()?),
            5 => {
                let id = r.u32()?;
                let kind = read_piece(r)?;
                let key = PieceKey { kind, x: r.i32()?, z: r.i32()?, level: r.i32()?, dir: r.u8()? & 3 };
                let mat = read_mat(r)?;
                SyncOp::PieceAdd(PieceNet { id, key, mat, base_y: r.f32()?, owner: r.u8()?, footing: r.f32()?, hp: r.f32()? })
            }
            6 => SyncOp::PieceRemove(r.u32()?),
            7 => SyncOp::PieceHp { id: r.u32()?, hp: r.f32()? },
            8 => SyncOp::HarvestBroken { idx: r.u32()?, dir: r.vec2()? },
            _ => return Err(WireError::Invalid("sync op")),
        })
    }
}

// ---- events ---------------------------------------------------------------------------------------------------------

/// Events that only the host's bots care about are not worth the bandwidth.
pub fn is_sent(e: &Event) -> bool {
    !matches!(e, Event::Noise { .. })
}

fn write_event(w: &mut Writer, e: &Event) {
    let actor = |w: &mut Writer, a: usize| w.u8(a.min(254) as u8);
    match e {
        Event::Footstep { actor: a, pos, surface } => {
            w.u8(0);
            actor(w, *a);
            w.vec3(*pos);
            w.u8(idx(&Surface::ALL, *surface));
        }
        Event::Jump { actor: a, pos } => {
            w.u8(1);
            actor(w, *a);
            w.vec3(*pos);
        }
        Event::Land { actor: a, pos, speed, surface } => {
            w.u8(2);
            actor(w, *a);
            w.vec3(*pos);
            w.f32(*speed);
            w.u8(idx(&Surface::ALL, *surface));
        }
        Event::Shot { actor: a, pos, weapon, end, hit_actor } => {
            w.u8(3);
            actor(w, *a);
            w.vec3(*pos);
            w.u8(idx(&WeaponKind::ALL, *weapon));
            w.vec3(*end);
            w.bool(*hit_actor);
        }
        Event::Impact { pos, normal, kind } => {
            w.u8(4);
            w.vec3(*pos);
            w.vec3(*normal);
            w.u8(idx(&ImpactKind::ALL, *kind));
        }
        Event::Tracer { from, to, weapon } => {
            w.u8(5);
            w.vec3(*from);
            w.vec3(*to);
            w.u8(idx(&WeaponKind::ALL, *weapon));
        }
        Event::Damage { target, attacker, amount, on_shield, headshot, pos } => {
            w.u8(6);
            actor(w, *target);
            w.opt_u8(*attacker);
            w.f32(*amount);
            w.bool(*on_shield);
            w.bool(*headshot);
            w.vec3(*pos);
        }
        Event::HitConfirm { actor: a, head, shield, kill } => {
            w.u8(7);
            actor(w, *a);
            w.bool(*head);
            w.bool(*shield);
            w.bool(*kill);
        }
        Event::Hurt { actor: a, amount, from } => {
            w.u8(8);
            actor(w, *a);
            w.f32(*amount);
            w.bool(from.is_some());
            if let Some(f) = from {
                w.vec3(*f);
            }
        }
        Event::Toast { actor: a, text, secs, style } => {
            w.u8(9);
            w.opt_u8(*a);
            w.str(text);
            w.f32(*secs);
            w.u8(*style);
        }
        Event::Eliminated { victim, killer, weapon, storm } => {
            w.u8(10);
            actor(w, *victim);
            w.opt_u8(*killer);
            w.u8(weapon.map_or(255, source_id));
            w.bool(*storm);
        }
        Event::Reload { actor: a, pos, kind } => {
            w.u8(11);
            actor(w, *a);
            w.vec3(*pos);
            w.u8(idx(&WeaponKind::ALL, *kind));
        }
        Event::EmptyClick { actor: a, pos } => {
            w.u8(12);
            actor(w, *a);
            w.vec3(*pos);
        }
        Event::WeaponSwitch { actor: a, pos } => {
            w.u8(13);
            actor(w, *a);
            w.vec3(*pos);
        }
        Event::Pickup { actor: a, pos, sound, name, rarity, count } => {
            w.u8(14);
            actor(w, *a);
            w.vec3(*pos);
            w.u8(idx(&PickupSound::ALL, *sound));
            w.u8(pickup_name_id(name));
            w.u8(idx(&Rarity::ALL, *rarity));
            w.u16((*count).min(65535) as u16);
        }
        Event::ChestOpen { pos } => {
            w.u8(15);
            w.vec3(*pos);
        }
        Event::Harvest { actor: a, pos, mat, amount } => {
            w.u8(16);
            actor(w, *a);
            w.vec3(*pos);
            w.u8(idx(&Mat::ALL, *mat));
            w.u16((*amount).min(65535) as u16);
        }
        Event::HarvestHit { pos, normal, wood } => {
            w.u8(17);
            w.vec3(*pos);
            w.vec3(*normal);
            w.bool(*wood);
        }
        Event::TreeFelled { pos, chunk, slot } => {
            w.u8(18);
            w.vec3(*pos);
            w.u32(*chunk as u32);
            w.u32(*slot as u32);
        }
        Event::Built { actor: a, pos, piece, mat } => {
            w.u8(19);
            actor(w, *a);
            w.vec3(*pos);
            w.u8(idx(&PieceKind::ALL, *piece));
            w.u8(idx(&Mat::ALL, *mat));
        }
        Event::PieceDestroyed { pos, mat } => {
            w.u8(20);
            w.vec3(*pos);
            w.u8(idx(&Mat::ALL, *mat));
        }
        Event::Explosion { pos, radius } => {
            w.u8(21);
            w.vec3(*pos);
            w.f32(*radius);
        }
        Event::BusJump { actor: a, pos } => {
            w.u8(22);
            actor(w, *a);
            w.vec3(*pos);
        }
        Event::GliderDeploy { actor: a, pos } => {
            w.u8(23);
            actor(w, *a);
            w.vec3(*pos);
        }
        Event::HealStart { actor: a, pos } => {
            w.u8(24);
            actor(w, *a);
            w.vec3(*pos);
        }
        Event::HealDone { actor: a, pos } => {
            w.u8(25);
            actor(w, *a);
            w.vec3(*pos);
        }
        Event::StormPhase { phase, shrinking } => {
            w.u8(26);
            w.u8((*phase).min(255) as u8);
            w.bool(*shrinking);
        }
        Event::Victory { winner } => {
            w.u8(27);
            actor(w, *winner);
        }
        Event::Swing { actor: a } => {
            w.u8(28);
            actor(w, *a);
        }
        Event::Noise { .. } => unreachable!("noise is not sent"),
    }
}

fn read_event(r: &mut Reader) -> Result<Event> {
    let actor = |r: &mut Reader| -> Result<usize> { Ok(r.u8()? as usize) };
    Ok(match r.u8()? {
        0 => Event::Footstep { actor: actor(r)?, pos: r.vec3()?, surface: read_surface(r)? },
        1 => Event::Jump { actor: actor(r)?, pos: r.vec3()? },
        2 => Event::Land { actor: actor(r)?, pos: r.vec3()?, speed: r.f32()?, surface: read_surface(r)? },
        3 => Event::Shot { actor: actor(r)?, pos: r.vec3()?, weapon: read_weapon(r)?, end: r.vec3()?, hit_actor: r.bool()? },
        4 => Event::Impact { pos: r.vec3()?, normal: r.vec3()?, kind: ImpactKind::ALL[r.index(ImpactKind::ALL.len(), "impact")?] },
        5 => Event::Tracer { from: r.vec3()?, to: r.vec3()?, weapon: read_weapon(r)? },
        6 => Event::Damage { target: actor(r)?, attacker: r.opt_u8()?, amount: r.f32()?, on_shield: r.bool()?, headshot: r.bool()?, pos: r.vec3()? },
        7 => Event::HitConfirm { actor: actor(r)?, head: r.bool()?, shield: r.bool()?, kill: r.bool()? },
        8 => {
            let a = actor(r)?;
            let amount = r.f32()?;
            let from = if r.bool()? { Some(r.vec3()?) } else { None };
            Event::Hurt { actor: a, amount, from }
        }
        9 => Event::Toast { actor: r.opt_u8()?, text: r.str()?, secs: r.f32()?, style: r.u8()? },
        10 => {
            let victim = actor(r)?;
            let killer = r.opt_u8()?;
            let weapon = match r.u8()? {
                255 => None,
                i => Some(*SOURCES.get(i as usize).ok_or(WireError::Invalid("damage source"))?),
            };
            Event::Eliminated { victim, killer, weapon, storm: r.bool()? }
        }
        11 => Event::Reload { actor: actor(r)?, pos: r.vec3()?, kind: read_weapon(r)? },
        12 => Event::EmptyClick { actor: actor(r)?, pos: r.vec3()? },
        13 => Event::WeaponSwitch { actor: actor(r)?, pos: r.vec3()? },
        14 => {
            let a = actor(r)?;
            let pos = r.vec3()?;
            let sound = PickupSound::ALL[r.index(PickupSound::ALL.len(), "pickup sound")?];
            let name = match r.u8()? {
                255 => "",
                i => pickup_names().get(i as usize).copied().ok_or(WireError::Invalid("pickup name"))?,
            };
            Event::Pickup { actor: a, pos, sound, name, rarity: read_rarity(r)?, count: r.u16()? as u32 }
        }
        15 => Event::ChestOpen { pos: r.vec3()? },
        16 => Event::Harvest { actor: actor(r)?, pos: r.vec3()?, mat: read_mat(r)?, amount: r.u16()? as u32 },
        17 => Event::HarvestHit { pos: r.vec3()?, normal: r.vec3()?, wood: r.bool()? },
        18 => Event::TreeFelled { pos: r.vec3()?, chunk: r.u32()? as usize, slot: r.u32()? as usize },
        19 => Event::Built { actor: actor(r)?, pos: r.vec3()?, piece: read_piece(r)?, mat: read_mat(r)? },
        20 => Event::PieceDestroyed { pos: r.vec3()?, mat: read_mat(r)? },
        21 => Event::Explosion { pos: r.vec3()?, radius: r.f32()? },
        22 => Event::BusJump { actor: actor(r)?, pos: r.vec3()? },
        23 => Event::GliderDeploy { actor: actor(r)?, pos: r.vec3()? },
        24 => Event::HealStart { actor: actor(r)?, pos: r.vec3()? },
        25 => Event::HealDone { actor: actor(r)?, pos: r.vec3()? },
        26 => Event::StormPhase { phase: r.u8()? as usize, shrinking: r.bool()? },
        27 => Event::Victory { winner: actor(r)? },
        28 => Event::Swing { actor: actor(r)? },
        _ => return Err(WireError::Invalid("event")),
    })
}

// ---- messages <-> bytes ---------------------------------------------------------------------------------------------

impl ClientMsg {
    /// Whether this message goes on the reliable channel.
    pub fn reliable(&self) -> bool {
        !matches!(self, ClientMsg::Cmds { .. })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            ClientMsg::Hello { version, name } => {
                w.u8(0);
                w.u16(*version);
                w.str(name);
            }
            ClientMsg::Cmds { time, cmds } => {
                w.u8(1);
                w.u32(*time);
                w.u8(cmds.len().min(MAX_CMDS) as u8);
                for c in cmds.iter().take(MAX_CMDS) {
                    w.u32(c.seq);
                    w.f32(c.dt);
                    w.i8(c.axis[0]);
                    w.i8(c.axis[1]);
                    w.f32(c.yaw);
                    w.f32(c.pitch);
                    w.u16(c.buttons as u16);
                    w.u8(c.select);
                    w.i8(c.cycle);
                    w.u8(c.piece);
                    w.f32(c.view);
                }
            }
            ClientMsg::Bye => w.u8(2),
            ClientMsg::Ready => w.u8(3),
        }
        w.buf
    }

    pub fn decode(bytes: &[u8]) -> Result<ClientMsg> {
        let mut r = Reader::new(bytes);
        let msg = match r.u8()? {
            0 => ClientMsg::Hello { version: r.u16()?, name: r.str()? },
            1 => {
                let time = r.u32()?;
                let n = r.u8()? as usize;
                if n > MAX_CMDS {
                    return Err(WireError::Invalid("commands"));
                }
                let mut cmds = Vec::with_capacity(n);
                for _ in 0..n {
                    let seq = r.u32()?;
                    let dt = r.f32()?;
                    let axis = [r.i8()?, r.i8()?];
                    let (yaw, pitch) = (r.f32()?, r.f32()?);
                    let buttons = r.u16()? as u32 & btn::ALL;
                    let (select, cycle, piece) = (r.u8()?, r.i8()?, r.u8()?);
                    cmds.push(Cmd { seq, dt, axis, yaw, pitch, buttons, select, cycle, piece, view: r.f32()? });
                }
                ClientMsg::Cmds { time, cmds }
            }
            2 => ClientMsg::Bye,
            3 => ClientMsg::Ready,
            _ => return Err(WireError::Invalid("client message")),
        };
        if !r.finished() {
            return Err(WireError::Invalid("trailing bytes"));
        }
        Ok(msg)
    }
}

impl ServerMsg {
    pub fn reliable(&self) -> bool {
        !matches!(self, ServerMsg::Snapshot(_))
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            ServerMsg::Welcome { you, names } => {
                w.u8(0);
                w.u8(*you);
                w.u8(names.len() as u8);
                for n in names {
                    w.str(n);
                }
            }
            ServerMsg::Lobby { names } => {
                w.u8(1);
                w.u8(names.len() as u8);
                for n in names {
                    w.str(n);
                }
            }
            ServerMsg::Start { you, setup } => {
                w.u8(2);
                w.u8(*you);
                setup.write(&mut w);
            }
            ServerMsg::Snapshot(s) => {
                w.u8(3);
                s.write(&mut w);
            }
            ServerMsg::Sync(ops) => {
                w.u8(4);
                w.u16(ops.len() as u16);
                for o in ops {
                    o.write(&mut w);
                }
            }
            ServerMsg::Events { time, events } => {
                w.u8(5);
                w.f32(*time);
                let sent: Vec<&Event> = events.iter().filter(|e| is_sent(e)).collect();
                w.u16(sent.len() as u16);
                for e in sent {
                    write_event(&mut w, e);
                }
            }
            ServerMsg::Reject { reason } => {
                w.u8(6);
                w.str(reason);
            }
            ServerMsg::Closed => w.u8(7),
        }
        w.buf
    }

    pub fn decode(bytes: &[u8]) -> Result<ServerMsg> {
        let mut r = Reader::new(bytes);
        let names = |r: &mut Reader| -> Result<Vec<String>> {
            let n = r.u8()? as usize;
            if n > MAX_HUMANS {
                return Err(WireError::Invalid("players"));
            }
            (0..n).map(|_| r.str()).collect()
        };
        let msg = match r.u8()? {
            0 => ServerMsg::Welcome { you: r.u8()?, names: names(&mut r)? },
            1 => ServerMsg::Lobby { names: names(&mut r)? },
            2 => ServerMsg::Start { you: r.u8()?, setup: MatchSetup::read(&mut r)? },
            3 => ServerMsg::Snapshot(Box::new(Snapshot::read(&mut r)?)),
            4 => {
                let n = r.count(MAX_OPS, "sync ops")?;
                ServerMsg::Sync((0..n).map(|_| SyncOp::read(&mut r)).collect::<Result<Vec<_>>>()?)
            }
            5 => {
                let time = r.f32()?;
                let n = r.count(MAX_EVENTS, "events")?;
                ServerMsg::Events { time, events: (0..n).map(|_| read_event(&mut r)).collect::<Result<Vec<_>>>()? }
            }
            6 => ServerMsg::Reject { reason: r.str()? },
            7 => ServerMsg::Closed,
            _ => return Err(WireError::Invalid("server message")),
        };
        if !r.finished() {
            return Err(WireError::Invalid("trailing bytes"));
        }
        Ok(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn setup() -> MatchSetup {
        MatchSetup { seed: 1234, mode: GameMode::Lego, bots: 30, difficulty: Difficulty::Hard, skip_bus: true, storm_speed: 2.5, start_mats: 150, names: vec!["Host".into(), "Ada".into(), "ZoÃ«".into()] }
    }

    fn own() -> Own {
        let mut slots = [None; 6];
        slots[0] = Some(Item::Pickaxe);
        slots[1] = Some(Item::Weapon { kind: WeaponKind::Sniper, rarity: Rarity::Epic, ammo: 1 });
        slots[3] = Some(Item::Consumable { kind: ConsumableKind::ChugJug, count: 1 });
        Own {
            hp: 87.5,
            shield: 40.0,
            alive: true,
            kills: 3,
            placement: 12,
            damage_dealt: 321.5,
            survived: 12.25,
            slots,
            selected: 1,
            ammo: [10, 20, 30, 40, 50],
            mats: [100, 200, 300],
            action: Action::Heal { slot: 3, t: 1.5, dur: 10.0 },
            build_mode: true,
            build_piece: PieceKind::Ramp,
            build_mat: Mat::Metal,
            mv: MoveState { pos: v(1.0, 2.0, 3.0), vel: v(-4.0, 0.5, 6.0), mode: MoveMode::Glide, on_ground: false, crouching: true, sprinting: false, ads: true, glide_deployed: true, emoting: false, coyote: 0.05, jump_buffer: 0.1, peak_y: 77.0, steps: 1.25, anim_time: 31.5 },
        }
    }

    fn actor(id: u8) -> ActorNet {
        ActorNet {
            id,
            alive: id % 2 == 0,
            mode: MoveMode::Ground,
            on_ground: true,
            crouching: false,
            sprinting: true,
            ads: false,
            build_mode: false,
            emoting: id == 3,
            glide_deployed: false,
            pos: v(id as f32 * 3.0, 10.0, -5.5),
            vel: v(1.5, 0.0, -6.25),
            yaw: 1.25,
            pitch: -0.4,
            body_yaw: 5.5,
            hp: 77,
            shield: 12,
            held: if id % 3 == 0 { Held::Weapon { kind: WeaponKind::Smg, rarity: Rarity::Rare } } else { Held::Pickaxe },
            action: 1,
            progress: 0.5,
        }
    }

    fn snapshot(n: usize) -> Snapshot {
        Snapshot {
            time: 123.5,
            echo: 9999,
            ack: 77,
            phase: Phase::Playing,
            tie: false,
            winner: None,
            match_time: 99.25,
            bus_active: false,
            bus_t: 12.0,
            storm: StormNet { active: true, phase: 2, shrinking: true, done: false, timer: 20.5, duration: 45.0, center: Vec2::new(10.0, -20.0), radius: 250.0, from_center: Vec2::ZERO, from_radius: 340.0, to_center: Vec2::new(30.0, 30.0), to_radius: 210.0, dmg: 2.0 },
            own: own(),
            actors: (0..n as u8).map(actor).collect(),
            projectiles: vec![ProjNet { pos: v(1.0, 2.0, 3.0), vel: v(0.0, 0.0, -58.0), owner: 2, kind: WeaponKind::RocketLauncher, rarity: Rarity::Legendary }],
            vehicles: vec![Vehicle { id: 17, pos: v(20.0, 2.0, 30.0), yaw: 1.2, speed: 12.0, steer: 0.3, driver: Some(1) }],
        }
    }

    fn all_events() -> Vec<Event> {
        let p = v(1.5, 2.5, -3.5);
        vec![
            Event::Footstep { actor: 3, pos: p, surface: Surface::Water },
            Event::Jump { actor: 4, pos: p },
            Event::Land { actor: 5, pos: p, speed: 12.5, surface: Surface::Stone },
            Event::Shot { actor: 6, pos: p, weapon: WeaponKind::Shotgun, end: v(9.0, 9.0, 9.0), hit_actor: true },
            Event::Impact { pos: p, normal: v(0.0, 1.0, 0.0), kind: ImpactKind::Foliage },
            Event::Tracer { from: p, to: v(50.0, 1.0, 2.0), weapon: WeaponKind::Sniper },
            Event::Damage { target: 7, attacker: Some(1), amount: 31.5, on_shield: true, headshot: false, pos: p },
            Event::Damage { target: 7, attacker: None, amount: 1.0, on_shield: false, headshot: true, pos: p },
            Event::HitConfirm { actor: 1, head: true, shield: false, kill: true },
            Event::Hurt { actor: 2, amount: 20.0, from: Some(v(5.0, 1.0, 5.0)) },
            Event::Hurt { actor: 2, amount: 7.0, from: None },
            Event::Toast { actor: Some(1), text: "Not enough wood".into(), secs: 1.5, style: 3 },
            Event::Toast { actor: None, text: "The storm is closing in".into(), secs: 4.0, style: 2 },
            Event::Eliminated { victim: 8, killer: Some(2), weapon: Some("Assault Rifle"), storm: false },
            Event::Eliminated { victim: 9, killer: None, weapon: Some("The Storm"), storm: true },
            Event::Reload { actor: 1, pos: p, kind: WeaponKind::Pistol },
            Event::EmptyClick { actor: 1, pos: p },
            Event::WeaponSwitch { actor: 1, pos: p },
            Event::Pickup { actor: 1, pos: p, sound: PickupSound::Heal, name: "Chug Jug", rarity: Rarity::Epic, count: 1 },
            Event::ChestOpen { pos: p },
            Event::Harvest { actor: 1, pos: p, mat: Mat::Stone, amount: 25 },
            Event::HarvestHit { pos: p, normal: v(1.0, 0.0, 0.0), wood: true },
            Event::TreeFelled { pos: p, chunk: 17, slot: 4 },
            Event::Built { actor: 1, pos: p, piece: PieceKind::Roof, mat: Mat::Wood },
            Event::PieceDestroyed { pos: p, mat: Mat::Metal },
            Event::Explosion { pos: p, radius: 7.0 },
            Event::BusJump { actor: 1, pos: p },
            Event::GliderDeploy { actor: 1, pos: p },
            Event::HealStart { actor: 1, pos: p },
            Event::HealDone { actor: 1, pos: p },
            Event::StormPhase { phase: 3, shrinking: true },
            Event::Victory { winner: 2 },
            Event::Swing { actor: 3 },
        ]
    }

    fn dbg(e: &[Event]) -> String {
        format!("{e:?}")
    }

    #[test]
    fn client_messages_round_trip() {
        let msgs = vec![
            ClientMsg::Hello { version: VERSION, name: "Ada".into() },
            ClientMsg::Cmds { time: 123456, cmds: vec![Cmd { seq: 5, dt: 1.0 / 60.0, axis: [-127, 127], yaw: 2.5, pitch: -0.5, buttons: btn::JUMP | btn::FIRE | btn::EMOTE, select: 3, cycle: -1, piece: 2, view: 99.5 }, Cmd { seq: 6, ..Default::default() }] },
            ClientMsg::Bye,
            ClientMsg::Ready,
        ];
        for m in msgs {
            assert_eq!(ClientMsg::decode(&m.encode()), Ok(m.clone()), "{m:?}");
        }
        assert!(ClientMsg::Hello { version: 1, name: String::new() }.reliable());
        assert!(!ClientMsg::Cmds { time: 0, cmds: vec![] }.reliable());
    }

    #[test]
    fn every_event_survives_the_trip() {
        let evs = all_events();
        let bytes = ServerMsg::Events { time: 42.5, events: evs.clone() }.encode();
        match ServerMsg::decode(&bytes).unwrap() {
            ServerMsg::Events { time, events } => {
                assert_eq!(time, 42.5);
                assert_eq!(dbg(&events), dbg(&evs));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn noise_is_not_sent() {
        let mut evs = all_events();
        evs.insert(3, Event::Noise { pos: Vec3::ZERO, radius: 50.0, source: 1 });
        let bytes = ServerMsg::Events { time: 1.0, events: evs.clone() }.encode();
        let ServerMsg::Events { events, .. } = ServerMsg::decode(&bytes).unwrap() else { panic!() };
        assert_eq!(events.len(), evs.len() - 1);
        assert!(!events.iter().any(|e| matches!(e, Event::Noise { .. })));
    }

    #[test]
    fn a_snapshot_round_trips_within_the_precision_of_the_wire() {
        let s = snapshot(5);
        let bytes = ServerMsg::Snapshot(Box::new(s.clone())).encode();
        let ServerMsg::Snapshot(back) = ServerMsg::decode(&bytes).unwrap() else { panic!() };
        assert_eq!(back.own, s.own);
        assert_eq!(back.storm, s.storm);
        assert_eq!((back.time, back.echo, back.ack, back.phase, back.winner, back.match_time, back.bus_t), (s.time, s.echo, s.ack, s.phase, s.winner, s.match_time, s.bus_t));
        assert_eq!(back.projectiles, s.projectiles);
        assert_eq!(back.vehicles, s.vehicles);
        assert_eq!(back.actors.len(), 5);
        for (a, b) in s.actors.iter().zip(&back.actors) {
            // everything else is exact; these are kept to the precision of the wire
            assert!((a.vel - b.vel).length() < 0.04, "{:?} {:?}", a.vel, b.vel);
            assert!((a.pitch - b.pitch).abs() < 1e-3);
            assert!((a.progress - b.progress).abs() < 0.005);
            for (x, y) in [(a.yaw, b.yaw), (a.body_yaw, b.body_yaw)] {
                let d = (x - y).rem_euclid(std::f32::consts::TAU);
                assert!(d.min(std::f32::consts::TAU - d) < 1e-3, "{x} vs {y}");
            }
            let same = ActorNet { vel: a.vel, pitch: a.pitch, progress: a.progress, yaw: a.yaw, body_yaw: a.body_yaw, ..*b };
            assert_eq!(*a, same);
        }
    }

    #[test]
    fn a_snapshot_for_a_full_match_fits_in_a_few_kilobytes() {
        let n = snapshot(60).encode_len();
        assert!(n < 2600, "{n} bytes for 60 actors");
        assert!(snapshot(10).encode_len() < 700);
    }

    impl Snapshot {
        fn encode_len(&self) -> usize {
            ServerMsg::Snapshot(Box::new(self.clone())).encode().len()
        }
    }

    #[test]
    fn world_changes_round_trip() {
        let ops = vec![
            SyncOp::Reset,
            SyncOp::PickupAdd(PickupNet { id: 12, pos: v(1.0, 2.0, 3.0), vel: v(0.5, 4.0, -0.5), kind: PickupKind::Weapon { kind: WeaponKind::AssaultRifle, rarity: Rarity::Legendary, ammo: 30 }, grounded: false, spin: 1.0 }),
            SyncOp::PickupAdd(PickupNet { id: 13, pos: v(1.0, 2.0, 3.0), vel: Vec3::ZERO, kind: PickupKind::Ammo { kind: AmmoKind::Shells, amount: 8 }, grounded: true, spin: 0.0 }),
            SyncOp::PickupAdd(PickupNet { id: 14, pos: v(1.0, 2.0, 3.0), vel: Vec3::ZERO, kind: PickupKind::Consumable { kind: ConsumableKind::MedKit, count: 2 }, grounded: true, spin: 6.0 }),
            SyncOp::PickupRemove(12),
            SyncOp::PickupAmount { id: 13, amount: 3 },
            SyncOp::ChestOpen(77),
            SyncOp::PieceAdd(PieceNet { id: 5, key: PieceKey { kind: PieceKind::Ramp, x: -3, z: 12, level: 2, dir: 3 }, mat: Mat::Stone, base_y: 20.5, owner: 2, footing: 0.75, hp: 260.0 }),
            SyncOp::PieceRemove(5),
            SyncOp::PieceHp { id: 6, hp: 33.5 },
            SyncOp::HarvestBroken { idx: 4242, dir: Vec2::new(0.6, -0.8) },
        ];
        let ServerMsg::Sync(back) = ServerMsg::decode(&ServerMsg::Sync(ops.clone()).encode()).unwrap() else { panic!() };
        // the spin is kept to 1/65536 of a turn
        assert_eq!(back.len(), ops.len());
        for (a, b) in ops.iter().zip(&back) {
            match (a, b) {
                (SyncOp::PickupAdd(x), SyncOp::PickupAdd(y)) => {
                    assert_eq!((x.id, x.pos, x.vel, x.kind, x.grounded), (y.id, y.pos, y.vel, y.kind, y.grounded));
                    assert!((x.spin - y.spin).abs() < 1e-3);
                }
                _ => assert_eq!(a, b),
            }
        }
    }

    #[test]
    fn the_lobby_and_the_start_messages_round_trip() {
        let m = ServerMsg::Welcome { you: 2, names: vec!["Host".into(), "Ada".into(), "Me".into()] };
        let ServerMsg::Welcome { you, names } = ServerMsg::decode(&m.encode()).unwrap() else { panic!() };
        assert_eq!((you, names.len()), (2, 3));
        let ServerMsg::Start { you, setup: back } = ServerMsg::decode(&ServerMsg::Start { you: 1, setup: setup() }.encode()).unwrap() else { panic!() };
        assert_eq!((you, back), (1, setup()));
        assert!(matches!(ServerMsg::decode(&ServerMsg::Closed.encode()), Ok(ServerMsg::Closed)));
        let ServerMsg::Reject { reason } = ServerMsg::decode(&ServerMsg::Reject { reason: "Room is full".into() }.encode()).unwrap() else { panic!() };
        assert_eq!(reason, "Room is full");
        assert!(ServerMsg::Closed.reliable() && !ServerMsg::Snapshot(Box::new(snapshot(1))).reliable());
    }

    #[test]
    fn the_setup_builds_the_same_game_config_everywhere() {
        let cfg = setup().to_config();
        assert_eq!((cfg.seed, cfg.bots, cfg.humans, cfg.skip_bus, cfg.start_mats), (1234, 30, 3, true, 150));
        assert_eq!(cfg.player_name, "Host");
        assert_eq!(cfg.human_names, vec!["Ada".to_string(), "ZoÃ«".to_string()]);
        assert_eq!(cfg.difficulty, Difficulty::Hard);
    }

    #[test]
    fn decoding_garbage_never_panics() {
        // every prefix of valid messages, every single-byte corruption of them, and plain noise
        let mut samples: Vec<Vec<u8>> = vec![
            ServerMsg::Snapshot(Box::new(snapshot(6))).encode(),
            ServerMsg::Events { time: 1.0, events: all_events() }.encode(),
            ServerMsg::Start { you: 1, setup: setup() }.encode(),
            ServerMsg::Welcome { you: 1, names: vec!["a".into()] }.encode(),
            ClientMsg::Cmds { time: 5, cmds: vec![Cmd::default(); 3] }.encode(),
            ClientMsg::Hello { version: 1, name: "x".into() }.encode(),
        ];
        samples.push(ServerMsg::Sync(vec![SyncOp::PickupRemove(1), SyncOp::HarvestBroken { idx: 1, dir: Vec2::X }]).encode());
        let mut rng = crate::rng::Rng::new(99);
        let mut decoded = 0;
        for s in &samples {
            for cut in 0..s.len() {
                let _ = ServerMsg::decode(&s[..cut]);
                let _ = ClientMsg::decode(&s[..cut]);
            }
            for _ in 0..400 {
                let mut b = s.clone();
                let i = rng.below(b.len());
                b[i] = rng.below(256) as u8;
                decoded += ServerMsg::decode(&b).is_ok() as usize + ClientMsg::decode(&b).is_ok() as usize;
            }
        }
        for _ in 0..2000 {
            let n = rng.below(300);
            let b: Vec<u8> = (0..n).map(|_| rng.below(256) as u8).collect();
            let _ = ServerMsg::decode(&b);
            let _ = ClientMsg::decode(&b);
        }
        assert!(decoded > 0, "some corruptions are harmless and still decode");
    }

    #[test]
    fn hostile_counts_and_values_are_refused() {
        // 65535 actors announced in a snapshot that has none
        let mut bytes = ServerMsg::Snapshot(Box::new(snapshot(0))).encode();
        let n = bytes.len();
        // counts, one projectile (29 bytes), and one vehicle (29 bytes)
        let at = n - 2 - 29 - 2 - 29 - 2;
        bytes[at] = 0xff;
        bytes[at + 1] = 0xff;
        assert!(ServerMsg::decode(&bytes).is_err());
        // a command packet with too many commands
        let mut b = ClientMsg::Cmds { time: 0, cmds: vec![] }.encode();
        b[5] = 200;
        assert!(ClientMsg::decode(&b).is_err());
        // unknown message kinds
        assert!(ServerMsg::decode(&[99]).is_err() && ClientMsg::decode(&[99]).is_err());
        assert!(ServerMsg::decode(&[]).is_err());
        // trailing junk
        let mut b = ServerMsg::Closed.encode();
        b.push(0);
        assert!(ServerMsg::decode(&b).is_err());
        // a storm speed that would freeze or explode the match
        let mut s = setup();
        s.storm_speed = 0.0;
        assert!(ServerMsg::decode(&ServerMsg::Start { you: 1, setup: s }.encode()).is_err());
    }
}
