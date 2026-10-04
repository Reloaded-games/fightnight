//! Multiplayer glue: the page owns the WebRTC connections and hands bytes in and out; this is where they meet the lobby,
//! the host's match and the guest's copy of it.

use crate::app::{now_ms, App, Lobby, Mode};
use fn_core::game::{GameConfig, PlayerInput};
use fn_core::net::client::Client;
use fn_core::net::guest::GuestRoom;
use fn_core::net::host::{Host, PeerId, Room, StartParams};
use fn_core::world::World;
use std::fmt::Write;

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {}
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn names_json(names: &[String]) -> String {
    format!("[{}]", names.iter().map(|n| esc(n)).collect::<Vec<_>>().join(","))
}

impl App {
    /// Open a room that other players can join.
    pub fn net_host_open(&mut self, name: &str) {
        self.net_reset();
        self.lobby = Lobby::Host(Room::new(name));
    }

    /// Prepare to join somebody's room (the hello goes out as soon as the page has a connection).
    pub fn net_guest_open(&mut self, name: &str) {
        self.net_reset();
        let mut g = GuestRoom::new(name);
        self.net_out.extend(g.drain());
        self.lobby = Lobby::Guest(g);
    }

    /// Leave whatever multiplayer thing is going on; the goodbyes wait in the outbox for the page to send.
    pub fn net_reset(&mut self) {
        match &mut self.mode {
            Mode::Host(h) if self.net_live => {
                h.close();
                self.net_out.extend(h.drain());
            }
            Mode::Guest(c) if self.net_live => {
                c.leave();
                self.net_out.extend(c.drain());
            }
            _ => {}
        }
        match &mut self.lobby {
            Lobby::Host(r) => {
                r.close();
                self.net_out.extend(r.drain());
            }
            Lobby::Guest(g) => {
                g.leave();
                self.net_out.extend(g.drain());
            }
            Lobby::None => {}
        }
        self.lobby = Lobby::None;
        self.pending_host = None;
        self.net_live = false;
    }

    pub fn net_connected(&mut self, peer: PeerId) {
        if let Lobby::Host(r) = &mut self.lobby {
            r.connect(peer);
        }
    }

    pub fn net_disconnected(&mut self, peer: PeerId) {
        match (&mut self.mode, self.net_live) {
            (Mode::Host(h), true) => return h.on_disconnect(peer),
            (Mode::Guest(c), true) => return c.connection_lost(),
            _ => {}
        }
        match &mut self.lobby {
            Lobby::Host(r) => r.disconnect(peer),
            Lobby::Guest(g) => g.disconnected(),
            Lobby::None => {}
        }
    }

    pub fn net_receive(&mut self, peer: PeerId, bytes: &[u8], now_ms: u32) {
        match (&mut self.mode, self.net_live) {
            (Mode::Host(h), true) => return h.on_message(peer, bytes),
            (Mode::Guest(c), true) => return c.on_message(bytes, now_ms),
            _ => {}
        }
        match &mut self.lobby {
            Lobby::Host(r) => r.on_message(peer, bytes),
            Lobby::Guest(g) => g.on_message(bytes),
            Lobby::None => {}
        }
    }

    /// Everything waiting to go out: for each message the peer (0 for the host), whether it must arrive reliably, its length
    /// (u32) and its bytes.
    pub fn net_poll(&mut self) -> Vec<u8> {
        match &mut self.lobby {
            Lobby::Host(r) => self.net_out.extend(r.drain()),
            Lobby::Guest(g) => self.net_out.extend(g.drain()),
            Lobby::None => {}
        }
        let mut out = Vec::new();
        for m in self.net_out.drain(..) {
            out.extend_from_slice(&m.peer.to_le_bytes());
            out.push(m.reliable as u8);
            out.extend_from_slice(&(m.bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(&m.bytes);
        }
        out
    }

    /// The state of the room or the match, as JSON for the lobby screens.
    pub fn net_status(&self) -> String {
        let mut s = String::new();
        match (&self.mode, self.net_live, &self.lobby) {
            (Mode::Host(h), true, _) => {
                let peers: Vec<String> = h.peers().iter().map(|p| format!("{{\"name\":{},\"ping\":{:.0},\"connected\":{},\"ready\":{}}}", esc(&p.name), p.ping_ms, p.connected, p.ready)).collect();
                let _ = write!(s, "{{\"state\":\"host\",\"started\":{},\"waiting\":{},\"names\":{},\"peers\":[{}]}}", h.has_started(), h.waiting_for(), names_json(&h.setup.names), peers.join(","));
            }
            (Mode::Guest(c), true, _) => {
                let _ = write!(s, "{{\"state\":\"guest\",\"started\":{},\"synced\":{},\"ping\":{:.0},\"names\":{},\"closed\":{}}}", true, c.is_synced(), c.ping_ms, names_json(&c.setup.names), c.closed().map_or("null".into(), esc));
            }
            (_, _, Lobby::Host(r)) => {
                let _ = write!(s, "{{\"state\":\"host-lobby\",\"names\":{}}}", names_json(&r.names()));
            }
            (_, _, Lobby::Guest(g)) => {
                let _ = write!(s, "{{\"state\":\"guest-lobby\",\"in\":{},\"names\":{},\"started\":{},\"closed\":{}}}", g.is_in(), names_json(g.names()), g.start().is_some(), g.closed().map_or("null".into(), esc));
            }
            _ => s.push_str("{\"state\":\"none\"}"),
        }
        s
    }

    /// Keep a multiplayer match ticking without drawing anything (the loading screen waits for the others this way).
    pub fn net_tick(&mut self, dt: f32) {
        if !self.net_live {
            return;
        }
        let idle = PlayerInput::default();
        match &mut self.mode {
            Mode::Host(h) => {
                h.update(dt, &idle);
                self.net_out.extend(h.drain());
            }
            Mode::Guest(c) => {
                c.update(dt, &idle, now_ms() as u32);
                self.net_out.extend(c.drain());
            }
            Mode::Solo(_) => {}
        }
    }

    /// The host presses start: close the room and tell the guests, so they begin building the island too. Follow with
    /// [`App::net_finish_host`] once the messages have been sent.
    pub fn net_begin_host(&mut self, cfg: &GameConfig) -> Result<(), String> {
        let Lobby::Host(mut room) = std::mem::replace(&mut self.lobby, Lobby::None) else { return Err("You are not hosting a room.".into()) };
        let params = StartParams { seed: cfg.seed as u32, bots: cfg.bots as u16, difficulty: cfg.difficulty, skip_bus: cfg.skip_bus, storm_speed: cfg.storm_speed, start_mats: cfg.start_mats.min(65535) as u16 };
        let setup = room.begin(&params);
        self.net_out.extend(room.drain());
        self.pending_host = Some((room, setup));
        Ok(())
    }

    /// Build the island and start the match for everyone in the room.
    pub fn net_finish_host(&mut self) -> Result<(), String> {
        let (room, setup) = self.pending_host.take().ok_or("The match has not been announced.")?;
        let world = World::generate(setup.seed);
        self.renderer.set_world(&world);
        if setup.seed as u64 != self.cfg.seed || self.minimap.is_empty() {
            self.minimap = fn_core::world::minimap::render(&world, 1024);
        }
        self.cfg.seed = setup.seed as u64;
        let fov = self.mode.game().fov_deg;
        let mut host = Host::new(world, setup, room);
        host.game.fov_deg = fov;
        self.mode = Mode::Host(Box::new(host));
        self.reset_for_match();
        self.net_live = true;
        Ok(())
    }

    /// The host started the match this page was waiting for: build the island and the player's copy of the match.
    pub fn net_start_guest(&mut self) -> Result<(), String> {
        let Lobby::Guest(g) = &self.lobby else { return Err("You are not in a room.".into()) };
        let Some((you, setup)) = g.start().cloned() else { return Err("The host has not started the match.".into()) };
        let world = World::generate(setup.seed);
        self.renderer.set_world(&world);
        if setup.seed as u64 != self.cfg.seed || self.minimap.is_empty() {
            self.minimap = fn_core::world::minimap::render(&world, 1024);
        }
        self.cfg.seed = setup.seed as u64;
        let fov = self.mode.game().fov_deg;
        let mut client = Client::new(world, you, setup);
        client.game.fov_deg = fov;
        self.net_out.extend(client.drain());
        self.mode = Mode::Guest(Box::new(client));
        self.lobby = Lobby::None;
        self.reset_for_match();
        self.net_live = true;
        Ok(())
    }
}
