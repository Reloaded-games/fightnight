//! The host's side: a room that fills up with players, then the authoritative match.
//!
//! The host runs the whole simulation. People who joined are extra human actors (see `game::remote`) whose commands arrive
//! over the network; everything that happens is sent back as snapshots of the actors, changes to the loot, chests and
//! buildings, and one-shot events.

use super::proto::*;
use crate::game::actor::{Action, Actor, MoveMode};
use crate::game::events::Event;
use crate::game::items::Item;
use crate::game::pieces::PieceKey;
use crate::game::{Difficulty, Game, GameMode, PlayerInput, Storm, StormState};
use crate::math::*;
use crate::world::World;
use std::collections::{HashMap, HashSet};

/// Identifies one connection (the transport hands these out).
pub type PeerId = u32;

/// A message to send to one peer.
#[derive(Clone, Debug)]
pub struct Outgoing {
    pub peer: PeerId,
    pub reliable: bool,
    pub bytes: Vec<u8>,
}

/// How often snapshots go out (per second).
pub const SNAPSHOT_RATE: f32 = 30.0;
/// A player who sent nothing for this long (seconds) is gone.
pub const TIMEOUT: f32 = 8.0;
/// How long the host waits for everyone's island to be built before starting without the stragglers.
pub const READY_TIMEOUT: f32 = 45.0;
/// Most operations in one reliable message (the first, full picture is cut into several).
const OPS_PER_MESSAGE: usize = 300;

/// What the host chooses when it starts the match.
#[derive(Clone, Debug)]
pub struct StartParams {
    pub seed: u32,
    pub mode: GameMode,
    pub bots: u16,
    pub difficulty: Difficulty,
    pub skip_bus: bool,
    pub storm_speed: f32,
    pub start_mats: u16,
}

impl Default for StartParams {
    fn default() -> Self {
        Self { seed: 1234, mode: GameMode::BattleRoyale, bots: 39, difficulty: Difficulty::Normal, skip_bus: false, storm_speed: 1.0, start_mats: 100 }
    }
}

/// A name that is safe to show everywhere: printable, trimmed, at most 16 characters.
pub fn clean_name(raw: &str, fallback: &str) -> String {
    let s: String = raw.chars().filter(|c| !c.is_control()).take(16).collect();
    let s = s.trim().to_string();
    if s.is_empty() {
        fallback.to_string()
    } else {
        s
    }
}

// ---- the room -------------------------------------------------------------------------------------------------------

struct RoomPeer {
    id: PeerId,
    /// `None` until the peer said hello.
    name: Option<String>,
}

/// Before the match: the host collects the people who join.
pub struct Room {
    host_name: String,
    peers: Vec<RoomPeer>,
    out: Vec<Outgoing>,
    closed: bool,
}

impl Room {
    pub fn new(host_name: &str) -> Room {
        Room { host_name: clean_name(host_name, "Host"), peers: vec![], out: vec![], closed: false }
    }

    /// The people in the room, the host first.
    pub fn names(&self) -> Vec<String> {
        std::iter::once(self.host_name.clone()).chain(self.peers.iter().filter_map(|p| p.name.clone())).collect()
    }

    fn send(&mut self, peer: PeerId, m: &ServerMsg) {
        self.out.push(Outgoing { peer, reliable: m.reliable(), bytes: m.encode() });
    }

    fn broadcast_lobby(&mut self) {
        let names = self.names();
        let ids: Vec<PeerId> = self.peers.iter().filter(|p| p.name.is_some()).map(|p| p.id).collect();
        for id in ids {
            self.send(id, &ServerMsg::Lobby { names: names.clone() });
        }
    }

    /// A connection to a player has opened (they still have to say hello).
    pub fn connect(&mut self, peer: PeerId) {
        if !self.peers.iter().any(|p| p.id == peer) {
            self.peers.push(RoomPeer { id: peer, name: None });
        }
    }

    pub fn disconnect(&mut self, peer: PeerId) {
        let was_in = self.peers.iter().any(|p| p.id == peer && p.name.is_some());
        self.peers.retain(|p| p.id != peer);
        if was_in && !self.closed {
            self.broadcast_lobby();
        }
    }

    pub fn on_message(&mut self, peer: PeerId, bytes: &[u8]) {
        let Ok(msg) = ClientMsg::decode(bytes) else { return };
        match msg {
            ClientMsg::Hello { version, name } => {
                let Some(i) = self.peers.iter().position(|p| p.id == peer) else { return };
                if self.peers[i].name.is_some() {
                    return;
                }
                if version != VERSION {
                    self.send(peer, &ServerMsg::Reject { reason: format!("This room runs version {VERSION} of the game, you have version {version}. Reload the page.") });
                    self.peers.remove(i);
                    return;
                }
                if self.closed {
                    self.send(peer, &ServerMsg::Reject { reason: "The match has already started.".into() });
                    self.peers.remove(i);
                    return;
                }
                if self.names().len() >= MAX_HUMANS {
                    self.send(peer, &ServerMsg::Reject { reason: "The room is full.".into() });
                    self.peers.remove(i);
                    return;
                }
                let n = self.names().len();
                self.peers[i].name = Some(clean_name(&name, &format!("Player {}", n + 1)));
                let names = self.names();
                self.send(peer, &ServerMsg::Welcome { you: (names.len() - 1) as u8, names });
                self.broadcast_lobby();
            }
            ClientMsg::Bye => self.disconnect(peer),
            ClientMsg::Cmds { .. } | ClientMsg::Ready => {}
        }
    }

    pub fn drain(&mut self) -> Vec<Outgoing> {
        std::mem::take(&mut self.out)
    }

    /// Close the room and tell everyone the match is starting. Build the island, then hand the setup and the room to
    /// [`Host::new`].
    pub fn begin(&mut self, p: &StartParams) -> MatchSetup {
        self.closed = true;
        // connections that never said hello are not part of the match
        let strays: Vec<PeerId> = self.peers.iter().filter(|p| p.name.is_none()).map(|p| p.id).collect();
        for id in strays {
            self.send(id, &ServerMsg::Reject { reason: "The match has already started.".into() });
        }
        self.peers.retain(|p| p.name.is_some());
        let setup = MatchSetup { seed: p.seed, mode: p.mode, bots: p.bots, difficulty: p.difficulty, skip_bus: p.skip_bus, storm_speed: p.storm_speed, start_mats: p.start_mats, names: self.names() };
        let ids: Vec<PeerId> = self.peers.iter().map(|p| p.id).collect();
        for (k, id) in ids.into_iter().enumerate() {
            self.send(id, &ServerMsg::Start { you: (k + 1) as u8, setup: setup.clone() });
        }
        setup
    }

    /// Tell everybody the room is gone (the host backed out before starting).
    pub fn close(&mut self) {
        self.closed = true;
        let ids: Vec<PeerId> = self.peers.iter().map(|p| p.id).collect();
        for id in ids {
            self.send(id, &ServerMsg::Closed);
        }
        self.peers.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.peers.iter().all(|p| p.name.is_none())
    }

    pub fn player_count(&self) -> usize {
        self.names().len()
    }
}

// ---- the match ------------------------------------------------------------------------------------------------------

/// How a peer is doing, for the lobby and the HUD.
#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub peer: PeerId,
    pub actor: usize,
    pub name: String,
    pub ready: bool,
    pub connected: bool,
    /// Milliseconds the round trip takes (0 until measured).
    pub ping_ms: f32,
}

struct HostPeer {
    id: PeerId,
    actor: usize,
    name: String,
    ready: bool,
    connected: bool,
    /// Host clock of the last packet that came from this peer.
    last_heard: f32,
    last_client_time: u32,
    ping_ms: f32,
}

/// What the players were last told about the things lying around the island; the next message holds the difference.
#[derive(Default)]
struct Shadow {
    pickups: HashMap<u32, (bool, u32)>,
    chests: Vec<bool>,
    pieces: HashMap<u32, (PieceKey, f32)>,
}

fn pickup_amount(k: &crate::game::PickupKind) -> u32 {
    match k {
        crate::game::PickupKind::Weapon { .. } => 1,
        crate::game::PickupKind::Ammo { amount, .. } => *amount,
        crate::game::PickupKind::Consumable { count, .. } => *count,
    }
}

fn pickup_net(p: &crate::game::Pickup) -> PickupNet {
    PickupNet { id: p.id, pos: p.pos, vel: p.vel, kind: p.kind, grounded: p.grounded, spin: p.spin }
}

fn piece_net(p: &crate::game::pieces::Piece) -> PieceNet {
    PieceNet { id: p.id, key: p.key, mat: p.mat, base_y: p.base_y, owner: p.owner.min(255) as u8, footing: p.footing, hp: p.hp }
}

impl Shadow {
    /// The operations that turn what the players know into the game's present state; remembers the new state.
    fn diff(&mut self, g: &Game, ops: &mut Vec<SyncOp>) {
        // loose items
        let mut seen: HashSet<u32> = HashSet::with_capacity(g.pickups.len());
        for p in &g.pickups {
            seen.insert(p.id);
            let amount = pickup_amount(&p.kind);
            match self.pickups.get(&p.id).copied() {
                None => ops.push(SyncOp::PickupAdd(pickup_net(p))),
                Some((was_grounded, was_amount)) => {
                    if p.grounded && !was_grounded {
                        // it came to rest: tell them exactly where
                        ops.push(SyncOp::PickupAdd(pickup_net(p)));
                    }
                    if amount != was_amount {
                        ops.push(SyncOp::PickupAmount { id: p.id, amount });
                    }
                }
            }
            self.pickups.insert(p.id, (p.grounded, amount));
        }
        self.pickups.retain(|id, _| {
            let keep = seen.contains(id);
            if !keep {
                ops.push(SyncOp::PickupRemove(*id));
            }
            keep
        });
        // chests
        if self.chests.len() != g.chests.len() {
            self.chests = vec![false; g.chests.len()];
        }
        for (i, c) in g.chests.iter().enumerate() {
            if c.opened && !self.chests[i] {
                ops.push(SyncOp::ChestOpen(c.id));
            }
            self.chests[i] = c.opened;
        }
        // buildings
        let mut seen: HashSet<u32> = HashSet::new();
        for p in g.pieces.iter() {
            seen.insert(p.id);
            match self.pieces.get(&p.id).copied() {
                None => ops.push(SyncOp::PieceAdd(piece_net(p))),
                Some((key, _)) if key != p.key => {
                    ops.push(SyncOp::PieceRemove(p.id));
                    ops.push(SyncOp::PieceAdd(piece_net(p)));
                }
                Some((_, hp)) if hp != p.hp => ops.push(SyncOp::PieceHp { id: p.id, hp: p.hp }),
                _ => {}
            }
            self.pieces.insert(p.id, (p.key, p.hp));
        }
        self.pieces.retain(|id, _| {
            let keep = seen.contains(id);
            if !keep {
                ops.push(SyncOp::PieceRemove(*id));
            }
            keep
        });
    }
}

pub struct Host {
    pub game: Game,
    pub setup: MatchSetup,
    peers: Vec<HostPeer>,
    out: Vec<Outgoing>,
    shadow: Shadow,
    /// Host wall clock, seconds since the match was created.
    clock: f32,
    snap_acc: f32,
    tick_no: u32,
    started: bool,
    closed: bool,
    /// Events the host itself made up between simulation steps (a player left): sent along with the next step's.
    extra_events: Vec<Event>,
}

impl Host {
    /// The match for the people in `room`, on an island the caller built from `setup.seed`.
    pub fn new(world: World, setup: MatchSetup, room: Room) -> Host {
        let cfg = setup.to_config();
        let mut game = Game::new(world, cfg);
        let mut peers = vec![];
        for (k, p) in room.peers.iter().filter(|p| p.name.is_some()).enumerate() {
            let actor = k + 1;
            game.make_remote(actor);
            peers.push(HostPeer { id: p.id, actor, name: p.name.clone().unwrap(), ready: false, connected: true, last_heard: 0.0, last_client_time: 0, ping_ms: 0.0 });
        }
        let out = room.out;
        let started = peers.is_empty();
        let mut h = Host { game, setup, peers, out, shadow: Shadow::default(), clock: 0.0, snap_acc: 0.0, tick_no: 0, started, closed: false, extra_events: vec![] };
        if started {
            h.begin_match();
        }
        h
    }

    /// Who is in the match and how they are doing.
    pub fn peers(&self) -> Vec<PeerInfo> {
        self.peers.iter().map(|p| PeerInfo { peer: p.id, actor: p.actor, name: p.name.clone(), ready: p.ready, connected: p.connected, ping_ms: p.ping_ms }).collect()
    }

    /// The match has begun (everyone's island is built, or the wait ran out).
    pub fn has_started(&self) -> bool {
        self.started
    }

    /// People whose island is not built yet.
    pub fn waiting_for(&self) -> usize {
        self.peers.iter().filter(|p| p.connected && !p.ready).count()
    }

    fn send(&mut self, peer: PeerId, m: &ServerMsg) {
        self.out.push(Outgoing { peer, reliable: m.reliable(), bytes: m.encode() });
    }

    fn broadcast(&mut self, m: &ServerMsg) {
        let ids: Vec<PeerId> = self.peers.iter().filter(|p| p.connected).map(|p| p.id).collect();
        let (reliable, bytes) = (m.reliable(), m.encode());
        for id in ids {
            self.out.push(Outgoing { peer: id, reliable, bytes: bytes.clone() });
        }
    }

    /// Everyone is in: tell them the whole picture of the island and start the clock.
    fn begin_match(&mut self) {
        self.started = true;
        // the wait for the slowest island must not count as silence
        for p in &mut self.peers {
            p.last_heard = self.clock;
        }
        let mut ops = vec![SyncOp::Reset];
        self.shadow = Shadow::default();
        self.shadow.diff(&self.game, &mut ops);
        for chunk in ops.chunks(OPS_PER_MESSAGE) {
            self.broadcast(&ServerMsg::Sync(chunk.to_vec()));
        }
    }

    pub fn on_message(&mut self, peer: PeerId, bytes: &[u8]) {
        let Some(i) = self.peers.iter().position(|p| p.id == peer) else { return };
        if !self.peers[i].connected {
            return;
        }
        let Ok(msg) = ClientMsg::decode(bytes) else { return };
        self.peers[i].last_heard = self.clock;
        match msg {
            ClientMsg::Ready => self.peers[i].ready = true,
            ClientMsg::Cmds { time, cmds } => {
                self.peers[i].ready = true;
                self.peers[i].last_client_time = time;
                if self.started {
                    let actor = self.peers[i].actor;
                    self.game.push_cmds(actor, &cmds);
                }
            }
            ClientMsg::Bye => self.drop_peer(i, "left"),
            ClientMsg::Hello { .. } => {
                self.send(peer, &ServerMsg::Reject { reason: "The match has already started.".into() });
            }
        }
    }

    pub fn on_disconnect(&mut self, peer: PeerId) {
        if let Some(i) = self.peers.iter().position(|p| p.id == peer) {
            self.drop_peer(i, "left");
        }
    }

    fn drop_peer(&mut self, i: usize, why: &str) {
        if !self.peers[i].connected {
            return;
        }
        self.peers[i].connected = false;
        let actor = self.peers[i].actor;
        let name = self.peers[i].name.clone();
        self.game.drop_remote(actor);
        let text = format!("{name} {why}");
        self.game.show_toast(text.clone(), 3.0, 1);
        self.extra_events.push(Event::Toast { actor: None, text, secs: 3.0, style: 1 });
    }

    /// Tell everybody the room is shut (the host went back to the menu).
    pub fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            self.broadcast(&ServerMsg::Closed);
        }
    }

    pub fn drain(&mut self) -> Vec<Outgoing> {
        std::mem::take(&mut self.out)
    }

    /// Advance the match by `dt` seconds with the host's own input, and queue what the players have to be told.
    pub fn update(&mut self, dt: f32, input: &PlayerInput) {
        self.clock += dt;
        if !self.started {
            if self.peers.iter().all(|p| !p.connected || p.ready) || self.clock > READY_TIMEOUT {
                for i in 0..self.peers.len() {
                    if self.peers[i].connected && !self.peers[i].ready {
                        self.drop_peer(i, "did not make it");
                    }
                }
                self.begin_match();
            }
            // keep the host's own view alive without moving the match along
            return;
        }
        for i in 0..self.peers.len() {
            if self.peers[i].connected && self.clock - self.peers[i].last_heard > TIMEOUT {
                self.drop_peer(i, "lost the connection");
            }
        }
        self.game.update(dt, input);
        self.send_events();
        self.send_sync();
        self.snap_acc += dt;
        let step = 1.0 / SNAPSHOT_RATE;
        if self.snap_acc >= step {
            self.snap_acc = (self.snap_acc - step).min(step);
            self.send_snapshots();
        }
    }

    // ---- what the players are told --------------------------------------------------------------------------------------

    fn send_events(&mut self) {
        if self.game.events.is_empty() && self.extra_events.is_empty() {
            return;
        }
        let extra = std::mem::take(&mut self.extra_events);
        let time = self.game.time;
        for k in 0..self.peers.len() {
            if !self.peers[k].connected {
                continue;
            }
            let actor = self.peers[k].actor;
            let events: Vec<Event> = self.game.events.iter().chain(&extra).filter(|e| is_sent(e) && audience(&self.game, e, actor)).cloned().collect();
            if !events.is_empty() {
                let id = self.peers[k].id;
                self.send(id, &ServerMsg::Events { time, events });
            }
        }
    }

    fn send_sync(&mut self) {
        let mut ops = vec![];
        self.shadow.diff(&self.game, &mut ops);
        for (idx, dir) in std::mem::take(&mut self.game.harvest_log) {
            ops.push(SyncOp::HarvestBroken { idx, dir });
        }
        for chunk in ops.chunks(OPS_PER_MESSAGE) {
            self.broadcast(&ServerMsg::Sync(chunk.to_vec()));
        }
    }

    fn send_snapshots(&mut self) {
        self.tick_no = self.tick_no.wrapping_add(1);
        let g = &self.game;
        let storm = storm_net(&g.storm);
        let projectiles: Vec<ProjNet> = g.projectiles.iter().take(60).map(|p| ProjNet { pos: p.pos, vel: p.vel, owner: p.owner.min(255) as u8, kind: p.kind, rarity: p.rarity }).collect();
        let mut msgs = vec![];
        for p in &self.peers {
            if !p.connected {
                continue;
            }
            let me = &g.actors[p.actor];
            let watching_all = !me.alive;
            let mut actors = vec![];
            for a in &g.actors {
                if a.id == p.actor {
                    continue;
                }
                let period = if watching_all {
                    1
                } else if a.mode == MoveMode::Bus {
                    8
                } else {
                    let d = a.pos.distance(me.pos);
                    if d < 150.0 {
                        1
                    } else if d < 350.0 {
                        2
                    } else {
                        4
                    }
                };
                if (self.tick_no as usize + a.id) % period == 0 {
                    actors.push(actor_net(a));
                }
            }
            let snap = Snapshot {
                time: g.time,
                echo: p.last_client_time,
                ack: g.remote_ack(p.actor),
                phase: g.phase,
                tie: g.tie,
                winner: g.winner,
                match_time: g.match_time,
                bus_active: g.bus.active,
                bus_t: g.bus.t,
                storm,
                own: own_of(me),
                actors,
                projectiles: projectiles.clone(),
                vehicles: g.vehicles.clone(),
            };
            msgs.push((p.id, ServerMsg::Snapshot(Box::new(snap))));
        }
        for (id, m) in msgs {
            self.send(id, &m);
        }
    }
}

/// Should this player hear about / see this event? Things close to them and things that concern them, and the few that
/// concern everybody.
pub(crate) fn audience(g: &Game, e: &Event, who: usize) -> bool {
    let me = &g.actors[who];
    let near = |pos: Vec3, range: f32| !me.alive || me.pos.distance(pos) < range;
    match e {
        Event::HitConfirm { actor, .. } | Event::Hurt { actor, .. } => *actor == who,
        Event::Toast { actor, .. } => actor.is_none_or(|a| a == who),
        // their own steps are predicted on their machine
        Event::Footstep { actor, pos, .. } | Event::Jump { actor, pos } | Event::Land { actor, pos, .. } => *actor != who && me.alive && near(*pos, 45.0),
        Event::Eliminated { .. } | Event::Victory { .. } | Event::StormPhase { .. } | Event::TreeFelled { .. } => true,
        Event::Explosion { pos, .. } => near(*pos, 300.0),
        Event::Shot { pos, .. } => near(*pos, 220.0),
        Event::Tracer { from, .. } => near(*from, 220.0),
        Event::Damage { attacker, pos, .. } => *attacker == Some(who) || near(*pos, 160.0),
        Event::Pickup { actor, pos, .. } => *actor == who || near(*pos, 80.0),
        Event::Swing { actor } => *actor == who || near(g.actors[*actor].pos, 100.0),
        Event::Impact { pos, .. } | Event::HarvestHit { pos, .. } | Event::PieceDestroyed { pos, .. } | Event::ChestOpen { pos } | Event::Built { pos, .. } => near(*pos, 160.0),
        Event::Reload { pos, .. } | Event::EmptyClick { pos, .. } | Event::WeaponSwitch { pos, .. } | Event::HealStart { pos, .. } | Event::HealDone { pos, .. } | Event::Harvest { pos, .. } | Event::BusJump { pos, .. } | Event::GliderDeploy { pos, .. } => near(*pos, 160.0),
        Event::Noise { .. } => false,
    }
}

pub fn storm_net(s: &Storm) -> StormNet {
    StormNet {
        active: s.active,
        phase: s.phase.min(255) as u8,
        shrinking: s.state == StormState::Shrinking,
        done: s.state == StormState::Done,
        timer: s.timer,
        duration: s.duration,
        center: s.center,
        radius: s.radius,
        from_center: s.from_center,
        from_radius: s.from_radius,
        to_center: s.to_center,
        to_radius: s.to_radius,
        dmg: s.dmg,
    }
}

pub fn move_state(a: &Actor) -> MoveState {
    MoveState {
        pos: a.pos,
        vel: a.vel,
        mode: a.mode,
        on_ground: a.on_ground,
        crouching: a.crouching,
        sprinting: a.sprinting,
        ads: a.ads,
        glide_deployed: a.glide_deployed,
        emoting: a.emoting,
        coyote: a.coyote,
        jump_buffer: a.jump_buffer,
        peak_y: a.peak_y,
        steps: a.steps,
        anim_time: a.anim.time,
    }
}

pub fn own_of(a: &Actor) -> Own {
    Own {
        hp: a.hp,
        shield: a.shield,
        alive: a.alive,
        kills: a.kills.min(65535) as u16,
        placement: a.placement.min(65535) as u16,
        damage_dealt: a.damage_dealt,
        survived: a.survived,
        slots: a.inv.slots,
        selected: a.inv.selected as u8,
        ammo: a.inv.ammo.map(|v| v.min(65535) as u16),
        mats: a.inv.mats.map(|v| v.min(65535) as u16),
        action: a.action,
        build_mode: a.build_mode,
        build_piece: a.build_piece,
        build_mat: a.build_mat,
        mv: move_state(a),
    }
}

pub fn actor_net(a: &Actor) -> ActorNet {
    let held = match a.inv.selected_item() {
        Some(Item::Pickaxe) => Held::Pickaxe,
        Some(Item::Weapon { kind, rarity, .. }) => Held::Weapon { kind: *kind, rarity: *rarity },
        Some(Item::Consumable { kind, .. }) => Held::Consumable { kind: *kind },
        None => Held::Nothing,
    };
    let (action, progress) = match a.action {
        Action::None => (0, 0.0),
        Action::Reload { t, dur } => (1, (t / dur).clamp(0.0, 1.0)),
        Action::Heal { t, dur, .. } => (2, (t / dur).clamp(0.0, 1.0)),
        Action::Swap { .. } => (3, 0.5),
    };
    ActorNet {
        id: a.id.min(255) as u8,
        alive: a.alive,
        mode: a.mode,
        on_ground: a.on_ground,
        crouching: a.crouching,
        sprinting: a.sprinting,
        ads: a.ads,
        build_mode: a.build_mode,
        emoting: a.emoting,
        glide_deployed: a.glide_deployed,
        pos: a.pos,
        vel: a.vel,
        yaw: a.yaw,
        pitch: a.pitch,
        body_yaw: a.body_yaw,
        hp: a.hp.ceil().clamp(0.0, 255.0) as u8,
        shield: a.shield.ceil().clamp(0.0, 255.0) as u8,
        held,
        action,
        progress,
    }
}
