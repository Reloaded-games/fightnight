//! A player's side: their own copy of the match, kept in step with the host's.
//!
//! * The player's own actor is **predicted**: every step they take is simulated here at once with the same code the host
//!   runs, and sent to the host as a numbered command. When a snapshot says where the host has the actor after command N,
//!   the actor is put there and the commands after N are replayed on top, so a disagreement (a wall someone else built, a
//!   rocket's push) is corrected without the controls ever feeling delayed. The correction is faded in, not jumped to.
//! * Everyone else is **interpolated** between the host's snapshots, about a tenth of a second in the past, which hides
//!   the gaps between them.
//! * The loot, chests and buildings follow the host's change messages; one-shot events (shots, hits, pickups) are played
//!   back when their moment comes.
//!
//! The copy is a full [`Game`], so the renderer, the HUD and the sound mixer work on it exactly as they do on a host.

use super::host::Outgoing;
use super::proto::*;
use crate::game::actor::*;
use crate::game::cmd::{seq_after, Cmd};
use crate::game::env::Env;
use crate::game::events::Event;
use crate::game::items::*;
use crate::game::loot::step_loose_items;
use crate::game::matchflow::BUS_ALTITUDE;
use crate::game::{movement, Game, Phase, PickupKind, PlayerInput, StormState};
use crate::math::*;
use crate::world::{World, WORLD_HALF};
use std::collections::VecDeque;

/// How far behind the host's clock other players are drawn (seconds): three snapshot intervals.
pub const INTERP_DELAY: f32 = 0.1;
/// Other players are extrapolated this far (seconds) when the next snapshot is late.
const MAX_EXTRAPOLATION: f32 = 0.12;
/// A prediction error bigger than this (metres) is a teleport, not something to fade.
const SNAP_DISTANCE: f32 = 2.5;
/// How quickly a fading correction dies away (per second).
const CORRECTION_RATE: f32 = 12.0;
/// The commands kept for replay; the host acknowledges them within a round trip.
const MAX_PENDING: usize = 180;
/// Keep-alive: at least this often (seconds) the host hears from a player who has nothing to say.
const KEEPALIVE: f32 = 0.1;

#[derive(Clone, Copy, Debug, Default)]
pub struct NetStats {
    pub snapshots: u32,
    /// How many times a snapshot moved the predicted actor by more than 5 cm.
    pub corrections: u32,
    pub teleports: u32,
    pub late_events: u32,
}

pub struct Client {
    pub game: Game,
    pub you: usize,
    pub setup: MatchSetup,
    pub stats: NetStats,
    /// Round trip to the host in milliseconds (smoothed).
    pub ping_ms: f32,
    out: Vec<Outgoing>,
    closed: Option<String>,
    // --- prediction
    next_seq: u32,
    pending: VecDeque<Cmd>,
    /// The simulated body of the player: where the prediction has it.
    pred: Actor,
    last_intent: crate::game::intent::Intent,
    /// What the screen shows minus where the prediction has the actor (a correction that is fading away).
    offset: Vec3,
    view_yaw: f32,
    view_pitch: f32,
    recoil_kick: f32,
    since_send: f32,
    // --- time
    clock: f32,
    host_offset: f32,
    offsets: VecDeque<(f32, f32)>,
    render_time: f32,
    have_clock: bool,
    // --- what the host said
    last_snap: f32,
    samples: Vec<VecDeque<(f32, ActorNet)>>,
    pending_events: VecDeque<(f32, Event)>,
    bus_base: (f32, f32),
}

impl Client {
    pub fn new(world: World, you: usize, setup: MatchSetup) -> Client {
        let game = Game::new_replica(world, setup.to_config(), you);
        let a = &game.actors[you];
        let mut pred = Actor::new(you, &a.name, true, a.outfit);
        pred.pos = a.pos;
        pred.mode = a.mode;
        pred.on_ground = a.on_ground;
        pred.yaw = a.yaw;
        pred.pitch = a.pitch;
        pred.inv = a.inv.clone();
        pred.eye_smooth = a.eye_smooth;
        let n = game.actors.len();
        let mut c = Client {
            you,
            setup,
            stats: NetStats::default(),
            ping_ms: 0.0,
            out: vec![],
            closed: None,
            next_seq: 1,
            pending: VecDeque::new(),
            view_yaw: a.yaw,
            view_pitch: a.pitch,
            pred,
            last_intent: Default::default(),
            offset: Vec3::ZERO,
            recoil_kick: 0.0,
            since_send: 0.0,
            clock: 0.0,
            host_offset: 0.0,
            offsets: VecDeque::new(),
            render_time: 0.0,
            have_clock: false,
            last_snap: -1.0,
            samples: (0..n).map(|_| VecDeque::new()).collect(),
            pending_events: VecDeque::new(),
            bus_base: (0.0, 0.0),
            game,
        };
        c.send(&ClientMsg::Ready);
        c
    }

    /// Why the match ended for this player, if it did (the host closed the room or refused us).
    pub fn closed(&self) -> Option<&str> {
        self.closed.as_deref()
    }

    /// The host has spoken since the match began (the first snapshot arrived).
    pub fn is_synced(&self) -> bool {
        self.have_clock
    }

    fn send(&mut self, m: &ClientMsg) {
        self.out.push(Outgoing { peer: 0, reliable: m.reliable(), bytes: m.encode() });
    }

    /// The connection to the host broke.
    pub fn connection_lost(&mut self) {
        if self.closed.is_none() {
            self.closed = Some("Lost the connection to the host.".into());
        }
    }

    /// Say goodbye (when the player leaves the match).
    pub fn leave(&mut self) {
        self.send(&ClientMsg::Bye);
    }

    pub fn drain(&mut self) -> Vec<Outgoing> {
        std::mem::take(&mut self.out)
    }

    // ---- receiving ----------------------------------------------------------------------------------------------------

    pub fn on_message(&mut self, bytes: &[u8], now_ms: u32) {
        let Ok(msg) = ServerMsg::decode(bytes) else { return };
        match msg {
            ServerMsg::Snapshot(s) => self.on_snapshot(*s, now_ms),
            ServerMsg::Sync(ops) => {
                for op in ops {
                    self.apply_op(op);
                }
            }
            ServerMsg::Events { time, events } => {
                for e in events {
                    // what happens to this player themselves is shown at once; the world around them keeps the host's pace
                    let at = if self.is_personal(&e) { 0.0 } else { time };
                    self.pending_events.push_back((at, e));
                }
            }
            ServerMsg::Closed => self.closed = Some("The host closed the room.".into()),
            ServerMsg::Reject { reason } => self.closed = Some(reason),
            ServerMsg::Welcome { .. } | ServerMsg::Lobby { .. } | ServerMsg::Start { .. } => {}
        }
    }

    fn is_personal(&self, e: &Event) -> bool {
        let me = self.you;
        match e {
            Event::HitConfirm { .. } | Event::Hurt { .. } | Event::Toast { .. } => true,
            Event::Shot { actor, .. } | Event::Reload { actor, .. } | Event::EmptyClick { actor, .. } | Event::WeaponSwitch { actor, .. } | Event::Pickup { actor, .. } | Event::Harvest { actor, .. } | Event::Built { actor, .. } | Event::HealStart { actor, .. } | Event::HealDone { actor, .. } | Event::Swing { actor } | Event::BusJump { actor, .. } | Event::GliderDeploy { actor, .. } => *actor == me,
            _ => false,
        }
    }

    fn on_snapshot(&mut self, s: Snapshot, now_ms: u32) {
        if self.have_clock && s.time <= self.last_snap {
            return; // an older one that overtook a newer one
        }
        self.stats.snapshots += 1;
        self.last_snap = s.time;
        if s.echo != 0 {
            let rtt = now_ms.wrapping_sub(s.echo) as f32;
            if rtt < 5000.0 {
                self.ping_ms = if self.ping_ms == 0.0 { rtt } else { self.ping_ms + (rtt - self.ping_ms) * 0.1 };
            }
        }
        // the host's clock against ours
        let k = s.time - self.clock;
        self.offsets.push_back((self.clock, k));
        while self.offsets.front().is_some_and(|o| self.clock - o.0 > 3.0) {
            self.offsets.pop_front();
        }
        if !self.have_clock {
            self.have_clock = true;
            self.host_offset = k;
            self.render_time = s.time - INTERP_DELAY;
        }
        // the world's own state
        let g = &mut self.game;
        g.phase = s.phase;
        g.tie = s.tie;
        g.winner = s.winner;
        g.match_time = s.match_time;
        g.bus.active = s.bus_active;
        self.bus_base = (s.bus_t, self.clock);
        let st = &mut g.storm;
        st.active = s.storm.active;
        st.phase = s.storm.phase as usize;
        st.state = if s.storm.done {
            StormState::Done
        } else if s.storm.shrinking {
            StormState::Shrinking
        } else {
            StormState::Waiting
        };
        st.timer = s.storm.timer;
        st.duration = s.storm.duration;
        st.center = s.storm.center;
        st.radius = s.storm.radius;
        st.from_center = s.storm.from_center;
        st.from_radius = s.storm.from_radius;
        st.to_center = s.storm.to_center;
        st.to_radius = s.storm.to_radius;
        st.dmg = s.storm.dmg;
        g.projectiles = s
            .projectiles
            .iter()
            .map(|p| crate::game::Projectile { pos: p.pos, vel: p.vel, owner: p.owner as usize, kind: p.kind, rarity: p.rarity, life: 1.0 })
            .collect();
        // everybody else
        for a in s.actors {
            let i = a.id as usize;
            if i == self.you || i >= self.samples.len() {
                continue;
            }
            let buf = &mut self.samples[i];
            buf.push_back((s.time, a));
            while buf.front().is_some_and(|f| s.time - f.0 > 1.5) && buf.len() > 2 {
                buf.pop_front();
            }
        }
        self.reconcile(&s.own, s.ack);
    }

    /// Take the host's word for the player's own actor, and replay what the host has not seen yet.
    fn reconcile(&mut self, own: &Own, ack: u32) {
        while self.pending.front().is_some_and(|c| !seq_after(c.seq, ack)) {
            self.pending.pop_front();
        }
        let you = self.you;
        // what the HUD and the rules say about the player (not about where they stand)
        {
            let a = &mut self.game.actors[you];
            let was_alive = a.alive;
            a.hp = own.hp;
            a.shield = own.shield;
            a.alive = own.alive;
            if was_alive && !own.alive {
                a.dead_time = 0.0;
            }
            a.kills = own.kills as u32;
            a.placement = own.placement as u32;
            a.damage_dealt = own.damage_dealt;
            a.survived = own.survived;
            a.inv.slots = own.slots;
            a.inv.selected = own.selected as usize;
            a.inv.ammo = own.ammo.map(|v| v as u32);
            a.inv.mats = own.mats.map(|v| v as u32);
            a.action = own.action;
            a.build_mode = own.build_mode;
            a.build_piece = own.build_piece;
            a.build_mat = own.build_mat;
        }
        let p = &mut self.pred;
        let old = (p.pos, p.mode);
        p.inv.slots = own.slots;
        p.inv.selected = own.selected as usize;
        p.action = own.action;
        p.build_mode = own.build_mode;
        p.alive = own.alive;
        p.hp = own.hp;
        let m = &own.mv;
        p.pos = m.pos;
        p.vel = m.vel;
        p.mode = m.mode;
        p.on_ground = m.on_ground;
        p.crouching = m.crouching;
        p.sprinting = m.sprinting;
        p.ads = m.ads;
        p.glide_deployed = m.glide_deployed;
        p.emoting = m.emoting;
        p.coyote = m.coyote;
        p.jump_buffer = m.jump_buffer;
        p.peak_y = m.peak_y;
        p.steps = m.steps;
        p.anim.time = m.anim_time;
        if !own.alive || m.mode == MoveMode::Dead {
            self.pending.clear();
            self.offset = Vec3::ZERO;
            return;
        }
        if m.mode == MoveMode::Bus {
            // riding the bus is not predicted (the screen follows the bus), but what the player pressed - the jump - must
            // stay in the commands until the host has acknowledged it
            self.offset = Vec3::ZERO;
            return;
        }
        let replay: Vec<Cmd> = self.pending.iter().copied().collect();
        for c in &replay {
            self.predict(c, false);
        }
        if old.1 == self.pred.mode {
            let err = old.0 - self.pred.pos;
            if err.length() > SNAP_DISTANCE {
                self.offset = Vec3::ZERO;
                self.stats.teleports += 1;
            } else {
                if err.length() > 0.05 {
                    self.stats.corrections += 1;
                }
                self.offset += err;
            }
        } else {
            self.offset = Vec3::ZERO;
        }
    }

    // ---- the world around ----------------------------------------------------------------------------------------------

    fn apply_op(&mut self, op: SyncOp) {
        let g = &mut self.game;
        match op {
            SyncOp::Reset => {
                g.pickups.clear();
                for c in &mut g.chests {
                    c.opened = false;
                    c.open_t = 0.0;
                }
                g.pieces.clear();
            }
            SyncOp::PickupAdd(p) => {
                let fresh = crate::game::Pickup { id: p.id, pos: p.pos, vel: p.vel, kind: p.kind, age: 0.0, grounded: p.grounded, spin: p.spin };
                match g.pickup_index(p.id) {
                    Some(i) => {
                        let age = g.pickups[i].age;
                        g.pickups[i] = crate::game::Pickup { age, ..fresh };
                    }
                    None => g.pickups.push(fresh),
                }
                g.next_id = g.next_id.max(p.id);
            }
            SyncOp::PickupRemove(id) => {
                if let Some(i) = g.pickup_index(id) {
                    g.pickups.remove(i);
                }
            }
            SyncOp::PickupAmount { id, amount } => {
                if let Some(i) = g.pickup_index(id) {
                    match &mut g.pickups[i].kind {
                        PickupKind::Ammo { amount: a, .. } => *a = amount,
                        PickupKind::Consumable { count, .. } => *count = amount,
                        PickupKind::Weapon { .. } => {}
                    }
                }
            }
            SyncOp::ChestOpen(id) => {
                if let Some(i) = g.chest_index(id) {
                    g.chests[i].opened = true;
                }
            }
            SyncOp::PieceAdd(p) => {
                g.pieces.insert_with_id(p.id, p.key, p.mat, p.base_y, p.owner as usize, p.footing);
                if let Some(Some(piece)) = g.pieces.list.get_mut(p.id as usize) {
                    piece.hp = p.hp;
                }
            }
            SyncOp::PieceRemove(id) => {
                g.pieces.remove(id);
            }
            SyncOp::PieceHp { id, hp } => {
                if let Some(Some(piece)) = g.pieces.list.get_mut(id as usize) {
                    piece.hp = hp;
                }
            }
            SyncOp::HarvestBroken { idx, dir } => {
                let Some(h) = g.world.harvest.get_mut(idx as usize) else { return };
                if !h.alive {
                    return;
                }
                h.alive = false;
                h.hp = 0.0;
                let (collider, pos, chunk, slot, kind, prop_kind) = (h.collider, h.pos, h.chunk as usize, h.slot as usize, h.kind, h.prop_kind);
                g.world.statics.remove(collider);
                if kind == crate::world::props::HarvestKind::Tree {
                    g.start_felling(pos, chunk, slot, prop_kind, dir);
                }
            }
        }
    }

    // ---- prediction ----------------------------------------------------------------------------------------------------

    /// Run one command on the predicted body, exactly as the host does for this player (see `Game::apply_intent`).
    /// `live` steps also play the sounds of the player's own footsteps, jumps and landings.
    fn predict(&mut self, c: &Cmd, live: bool) {
        let it = c.to_intent();
        let dt = c.dt;
        let a = &mut self.pred;
        a.yaw = it.yaw;
        a.pitch = it.pitch.clamp(-1.5, 1.5);
        // a heal in progress slows the player; the host runs its clock, so does the prediction
        if let Action::Heal { slot, t, dur } = a.action {
            a.action = if t + dt >= dur { Action::None } else { Action::Heal { slot, t: t + dt, dur } };
        }
        let env = Env::new(&self.game.world, &self.game.pieces.grid);
        let mut ev = vec![];
        match a.mode {
            MoveMode::Freefall => {
                movement::step_freefall(a, &it, &env, dt, &mut ev);
            }
            MoveMode::Glide => {
                movement::step_glide(a, &it, &env, dt, &mut ev);
            }
            MoveMode::Ground => {
                movement::step_ground(a, &it, &env, dt, &mut ev);
            }
            MoveMode::Swim => movement::step_swim(a, &it, &env, dt, &mut ev),
            MoveMode::Bus | MoveMode::Dead => {}
        }
        if matches!(a.mode, MoveMode::Ground | MoveMode::Swim) {
            let lim = WORLD_HALF + 110.0;
            a.pos.x = a.pos.x.clamp(-lim, lim);
            a.pos.z = a.pos.z.clamp(-lim, lim);
        }
        movement::update_body(a, &it, dt);
        movement::update_anim(a, dt);
        if matches!(a.mode, MoveMode::Ground | MoveMode::Swim) {
            let has_weapon = a.inv.selected_weapon().is_some();
            let reloading = matches!(a.action, Action::Reload { .. });
            a.ads = it.ads && has_weapon && !a.build_mode && a.mode == MoveMode::Ground && !reloading;
        }
        if live {
            self.last_intent = it;
            for e in ev {
                if matches!(e, Event::Footstep { .. } | Event::Jump { .. } | Event::Land { .. }) {
                    self.game.events.push(e);
                }
            }
        }
    }

    /// Turn this frame's input into commands, simulate them and queue them for the host.
    fn run_input(&mut self, dt: f32, input: &PlayerInput) {
        let steps = (dt / (1.0 / 60.0)).ceil().max(1.0) as usize;
        let h = dt / steps as f32;
        for k in 0..steps {
            let mut inp = input.clone();
            if k > 0 {
                // edge-triggered inputs apply to the first sub-step only
                inp.fire_pressed = false;
                inp.reload = false;
                inp.interact = false;
                inp.select = None;
                inp.cycle = 0;
                inp.drop_selected = false;
                inp.toggle_build = false;
                inp.piece = None;
                inp.place = false;
                inp.cycle_mat = false;
                inp.exit_bus = false;
                inp.deploy = false;
                inp.emote = false;
            }
            let look = input.look / steps as f32;
            let airborne = matches!(self.pred.mode, MoveMode::Freefall | MoveMode::Glide | MoveMode::Bus);
            self.view_yaw -= look.x;
            self.view_pitch = (self.view_pitch + look.y).clamp(if airborne { -1.45 } else { -1.5 }, 1.5);
            // the recoil of the player's shots lifts the view and settles again (see `Game::handle_items`)
            if self.recoil_kick > 0.0 {
                let r = self.recoil_kick.min(h * (0.5 + self.recoil_kick * 4.0));
                self.recoil_kick -= r;
                self.view_pitch = (self.view_pitch - r).clamp(-1.5, 1.5);
            }
            if !self.game.actors[self.you].alive {
                continue;
            }
            let mut cmd = Cmd::from_input(self.next_seq, h, &inp, self.view_yaw, self.view_pitch);
            // which moment of the host's match the player is looking at (the others are drawn that far in the past)
            cmd.view = if self.have_clock { self.render_time.max(0.001) } else { 0.0 };
            self.next_seq = self.next_seq.wrapping_add(1).max(1);
            self.pending.push_back(cmd);
            while self.pending.len() > MAX_PENDING {
                self.pending.pop_front();
            }
            if !matches!(self.pred.mode, MoveMode::Bus | MoveMode::Dead) {
                self.predict(&cmd, true);
            }
        }
    }

    fn send_cmds(&mut self, dt: f32, now_ms: u32) {
        self.since_send += dt;
        let n = self.pending.len().min(MAX_CMDS);
        if n == 0 && self.since_send < KEEPALIVE {
            return;
        }
        self.since_send = 0.0;
        let cmds: Vec<Cmd> = self.pending.iter().skip(self.pending.len() - n).copied().collect();
        self.send(&ClientMsg::Cmds { time: now_ms.max(1), cmds });
    }

    // ---- the frame -----------------------------------------------------------------------------------------------------

    /// Run one frame: the player's input, the prediction, what the host sent, and everything that merely moves on.
    pub fn update(&mut self, dt_real: f32, input: &PlayerInput, now_ms: u32) {
        self.game.events.clear();
        let dt = dt_real.clamp(0.0, 0.1);
        if dt <= 0.0 {
            return;
        }
        self.clock += dt;
        self.game.time += dt;
        self.game.step_no += 1;
        self.run_input(dt, input);
        self.send_cmds(dt, now_ms);
        if !self.game.actors[self.you].alive {
            self.game.update_spectate(input);
        }
        self.advance_clock(dt);
        self.advance_world(dt);
        self.show_local(dt);
        self.show_others(dt);
        self.play_events();
        self.game.finish_frame(dt);
    }

    fn advance_clock(&mut self, dt: f32) {
        if !self.have_clock {
            return;
        }
        let target = self.offsets.iter().map(|o| o.1).fold(f32::MIN, f32::max);
        self.host_offset += (target - self.host_offset) * (1.0 - (-3.0 * dt).exp());
        self.render_time = self.clock + self.host_offset - INTERP_DELAY;
    }

    fn advance_world(&mut self, dt: f32) {
        let g = &mut self.game;
        g.match_time += if g.phase == Phase::Over { 0.0 } else { dt };
        step_loose_items(g, dt);
        g.pieces.tick(dt);
        g.update_felled(dt);
        for p in &mut g.projectiles {
            p.pos += p.vel * dt;
        }
        if g.bus.active {
            let t = (self.bus_base.0 + (self.clock - self.bus_base.1)).clamp(0.0, g.bus.total);
            let p = g.bus.start.lerp(g.bus.end, t / g.bus.total);
            g.bus.t = t;
            g.bus.pos = Vec3::new(p.x, BUS_ALTITUDE, p.y);
        }
        let st = &mut g.storm;
        if st.active && st.state != StormState::Done {
            st.timer -= dt;
            if st.state == StormState::Shrinking && st.duration > 0.0 {
                let t = (1.0 - st.timer / st.duration).clamp(0.0, 1.0);
                st.center = st.from_center.lerp(st.to_center, t);
                st.radius = lerp(st.from_radius, st.to_radius, t);
            }
        }
    }

    /// Put the player's predicted actor on the screen.
    fn show_local(&mut self, dt: f32) {
        self.offset *= (-CORRECTION_RATE * dt).exp();
        let you = self.you;
        let bus_pos = self.game.bus.pos - Vec3::Y;
        let p = &self.pred;
        let a = &mut self.game.actors[you];
        a.yaw = self.view_yaw;
        a.pitch = self.view_pitch;
        if !a.alive {
            a.mode = MoveMode::Dead;
            a.vel = Vec3::ZERO;
            a.dead_time += dt;
            return;
        }
        a.mode = p.mode;
        a.vel = p.vel;
        a.on_ground = p.on_ground;
        a.crouching = p.crouching;
        a.sprinting = p.sprinting;
        a.ads = p.ads;
        a.glide_deployed = p.glide_deployed;
        a.emoting = p.emoting;
        a.pos = if p.mode == MoveMode::Bus { bus_pos } else { p.pos + self.offset };
        if p.mode == MoveMode::Bus {
            a.body_yaw = yaw_of(self.game.bus.dir);
        }
        let it = self.last_intent;
        movement::update_body(a, &it, dt);
        movement::update_anim(a, dt);
    }

    /// Put everyone else where they were a moment ago.
    fn show_others(&mut self, dt: f32) {
        let t = self.render_time;
        let bus_pos = self.game.bus.pos - Vec3::Y;
        for i in 0..self.game.actors.len() {
            if i == self.you {
                continue;
            }
            let a = &mut self.game.actors[i];
            let buf = &self.samples[i];
            if let Some(&(t1, ref s1)) = buf.back() {
                let (state, mix) = sample_at(buf, t, t1, s1);
                apply_sample(a, &state, mix);
            }
            if a.mode == MoveMode::Bus {
                a.pos = bus_pos;
                a.vel = Vec3::ZERO;
                a.body_yaw = yaw_of(self.game.bus.dir);
            }
            if !a.alive {
                a.dead_time += dt;
            }
            if a.mode != MoveMode::Dead {
                movement::update_anim(a, dt);
            }
        }
    }

    /// Let the events whose time has come happen.
    fn play_events(&mut self) {
        let now = self.render_time;
        while let Some((at, _)) = self.pending_events.front() {
            if *at > now && self.have_clock {
                break;
            }
            let (_, e) = self.pending_events.pop_front().unwrap();
            self.apply_event_effects(&e);
            self.game.events.push(e);
        }
    }

    fn apply_event_effects(&mut self, e: &Event) {
        let you = self.you;
        let g = &mut self.game;
        match e {
            Event::Shot { actor, weapon, .. } => {
                if let Some(a) = g.actors.get_mut(*actor) {
                    a.shot_flash = 0.14;
                    a.anim.recoil = 1.0;
                }
                if *actor == you {
                    let kick = weapon.def().recoil * DEG;
                    self.recoil_kick += kick;
                    self.view_pitch = (self.view_pitch + kick).min(1.5);
                }
            }
            Event::Swing { actor } => {
                if let Some(a) = g.actors.get_mut(*actor) {
                    a.anim.swing = 1.0;
                    a.shot_flash = 0.3;
                }
            }
            Event::Damage { target, .. } => {
                if let Some(a) = g.actors.get_mut(*target) {
                    a.hit_flash = 1.0;
                }
            }
            _ => {}
        }
    }
}

/// The state of an actor at host time `t` from its samples (oldest first), and how far `t` is towards the later one.
fn sample_at(buf: &VecDeque<(f32, ActorNet)>, t: f32, t_last: f32, last: &ActorNet) -> (ActorNet, f32) {
    // beyond the newest sample: carry on with its velocity for a moment
    if t >= t_last {
        let ahead = (t - t_last).min(MAX_EXTRAPOLATION);
        let mut s = *last;
        if s.mode != MoveMode::Bus && s.alive {
            s.pos += s.vel * ahead;
        }
        return (s, 1.0);
    }
    let Some(&(t0, first)) = buf.front() else { return (*last, 1.0) };
    if t <= t0 {
        return (first, 0.0);
    }
    // the pair of samples around `t`
    let mut prev = (t0, first);
    for &(ti, si) in buf.iter().skip(1) {
        if ti >= t {
            let span = (ti - prev.0).max(1e-4);
            let k = ((t - prev.0) / span).clamp(0.0, 1.0);
            let (a, b) = (prev.1, si);
            let mut s = if k < 0.5 { a } else { b };
            s.pos = a.pos.lerp(b.pos, k);
            s.vel = a.vel.lerp(b.vel, k);
            s.yaw = lerp_angle(a.yaw, b.yaw, k);
            s.pitch = lerp(a.pitch, b.pitch, k);
            s.body_yaw = lerp_angle(a.body_yaw, b.body_yaw, k);
            s.progress = lerp(a.progress, b.progress, k);
            return (s, k);
        }
        prev = (ti, si);
    }
    (*last, 1.0)
}

/// Write what a snapshot says about an actor onto the replica's copy of it.
fn apply_sample(a: &mut Actor, s: &ActorNet, _mix: f32) {
    let was_alive = a.alive;
    a.alive = s.alive;
    if was_alive && !s.alive {
        a.dead_time = 0.0;
    }
    a.mode = if s.alive { s.mode } else { MoveMode::Dead };
    a.pos = s.pos;
    a.vel = s.vel;
    a.yaw = s.yaw;
    a.pitch = s.pitch;
    a.body_yaw = s.body_yaw;
    a.on_ground = s.on_ground;
    a.crouching = s.crouching;
    a.sprinting = s.sprinting;
    a.ads = s.ads;
    a.build_mode = s.build_mode;
    a.emoting = s.emoting;
    a.glide_deployed = s.glide_deployed;
    a.hp = s.hp as f32;
    a.shield = s.shield as f32;
    // what is in their hands
    a.inv = Inventory::new();
    match s.held {
        Held::Nothing | Held::Pickaxe => {}
        Held::Weapon { kind, rarity } => {
            a.inv.slots[1] = Some(Item::Weapon { kind, rarity, ammo: kind.def().mag });
            a.inv.selected = 1;
        }
        Held::Consumable { kind } => {
            a.inv.slots[1] = Some(Item::Consumable { kind, count: 1 });
            a.inv.selected = 1;
        }
    }
    a.action = match (s.action, s.held) {
        (1, Held::Weapon { kind, rarity }) => {
            let dur = kind.def().reload * rarity.reload_mul();
            Action::Reload { t: s.progress * dur, dur }
        }
        (2, Held::Consumable { kind }) => {
            let dur = kind.def().use_time;
            Action::Heal { slot: 1, t: s.progress * dur, dur }
        }
        (3, _) => Action::Swap { t: 0.2 },
        _ => Action::None,
    };
}
