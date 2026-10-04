//! The host and its players, talking through an in-memory network with latency, jitter and loss.

use super::client::*;
use super::host::*;
use super::proto::*;
use crate::game::actor::*;
use crate::game::{Difficulty, PlayerInput};
use crate::math::*;
use crate::rng::Rng;
use crate::world::World;

#[derive(Clone, Copy, Debug)]
struct Net {
    /// One way, seconds.
    latency: f32,
    jitter: f32,
    /// Share of unreliable packets lost.
    loss: f32,
}

const GOOD: Net = Net { latency: 0.03, jitter: 0.0, loss: 0.0 };

#[derive(Default)]
struct Pipe {
    q: Vec<(f32, Vec<u8>)>,
    last_reliable: f32,
}

impl Pipe {
    fn push(&mut self, now: f32, reliable: bool, bytes: Vec<u8>, net: &Net, rng: &mut Rng) {
        if !reliable && rng.f32() < net.loss {
            return;
        }
        let mut t = now + net.latency + rng.f32() * net.jitter;
        if reliable {
            // ordered: never overtakes an earlier one
            t = t.max(self.last_reliable);
            self.last_reliable = t;
        }
        self.q.push((t, bytes));
    }

    fn ready(&mut self, now: f32) -> Vec<Vec<u8>> {
        self.q.sort_by(|a, b| a.0.total_cmp(&b.0));
        let n = self.q.iter().take_while(|x| x.0 <= now).count();
        self.q.drain(..n).map(|x| x.1).collect()
    }
}

struct Harness {
    host: Host,
    clients: Vec<Client>,
    up: Vec<Pipe>,
    down: Vec<Pipe>,
    net: Net,
    now: f32,
    rng: Rng,
}

fn quick_params(bots: u16, skip_bus: bool) -> StartParams {
    StartParams { seed: 1234, bots, difficulty: Difficulty::Normal, skip_bus, storm_speed: 1.0, start_mats: 300 }
}

impl Harness {
    /// A host and `guests` players who joined through the lobby, on a good connection.
    fn new(guests: usize, bots: u16, skip_bus: bool, net: Net) -> Harness {
        Self::with_params(guests, quick_params(bots, skip_bus), net)
    }

    fn with_params(guests: usize, params: StartParams, net: Net) -> Harness {
        let mut room = Room::new("Host");
        let mut hello = vec![];
        for k in 0..guests {
            let id = (k + 1) as PeerId;
            room.connect(id);
            room.on_message(id, &ClientMsg::Hello { version: VERSION, name: format!("Guest{}", k + 1) }.encode());
            hello.push(id);
        }
        let setup = room.begin(&params);
        // the start messages the guests got
        let mut starts = vec![];
        for o in room.drain() {
            if let Ok(ServerMsg::Start { you, setup }) = ServerMsg::decode(&o.bytes) {
                starts.push((o.peer, you, setup));
            }
        }
        assert_eq!(starts.len(), guests, "everybody is told the match starts");
        let host = Host::new(World::generate(params.seed), setup.clone(), room);
        let clients: Vec<Client> = starts.iter().map(|(_, you, setup)| Client::new(World::generate(setup.seed), *you as usize, setup.clone())).collect();
        Harness { host, clients, up: (0..guests).map(|_| Pipe::default()).collect(), down: (0..guests).map(|_| Pipe::default()).collect(), net, now: 0.0, rng: Rng::new(77) }
    }

    /// One frame of `dt` seconds for everybody.
    fn step(&mut self, dt: f32, host_in: &PlayerInput, client_in: &[PlayerInput]) {
        self.now += dt;
        let now_ms = (self.now * 1000.0) as u32 + 1;
        // what the host got
        for (k, pipe) in self.up.iter_mut().enumerate() {
            for b in pipe.ready(self.now) {
                self.host.on_message((k + 1) as PeerId, &b);
            }
        }
        self.host.update(dt, host_in);
        for o in self.host.drain() {
            let k = o.peer as usize - 1;
            self.down[k].push(self.now, o.reliable, o.bytes, &self.net, &mut self.rng);
        }
        for k in 0..self.clients.len() {
            for b in self.down[k].ready(self.now) {
                self.clients[k].on_message(&b, now_ms);
            }
            let inp = client_in.get(k).cloned().unwrap_or_default();
            self.clients[k].update(dt, &inp, now_ms);
            for o in self.clients[k].drain() {
                self.up[k].push(self.now, o.reliable, o.bytes, &self.net, &mut self.rng);
            }
        }
    }

    fn run(&mut self, secs: f32, host_in: &PlayerInput, client_in: &[PlayerInput]) {
        let n = (secs * 60.0) as usize;
        for _ in 0..n {
            self.step(1.0 / 60.0, host_in, client_in);
        }
    }
}

fn idle() -> PlayerInput {
    PlayerInput::default()
}


#[test]
fn the_lobby_collects_names_and_hands_out_places() {
    let mut room = Room::new("  Hosty McHostface and a very long name ");
    let host_name = room.names()[0].clone();
    assert!(host_name.starts_with("Hosty") && host_name.chars().count() <= 16 && host_name == host_name.trim(), "{host_name:?}");
    room.connect(1);
    room.connect(2);
    room.on_message(1, &ClientMsg::Hello { version: VERSION, name: "Ada".into() }.encode());
    room.on_message(2, &ClientMsg::Hello { version: VERSION, name: "\u{7}  ".into() }.encode());
    let names = room.names();
    assert_eq!(names.len(), 3);
    assert_eq!(names[1], "Ada");
    assert_eq!(names[2], "Player 3", "an empty name gets a placeholder");
    let msgs: Vec<ServerMsg> = room.drain().iter().map(|o| ServerMsg::decode(&o.bytes).unwrap()).collect();
    assert!(matches!(msgs[0], ServerMsg::Welcome { you: 1, .. }));
    assert!(msgs.iter().any(|m| matches!(m, ServerMsg::Lobby { names } if names.len() == 3)));
    // somebody leaves: everyone who is left hears
    room.disconnect(1);
    assert_eq!(room.names().len(), 2);
    let msgs: Vec<(PeerId, ServerMsg)> = room.drain().iter().map(|o| (o.peer, ServerMsg::decode(&o.bytes).unwrap())).collect();
    assert!(msgs.iter().any(|(p, m)| *p == 2 && matches!(m, ServerMsg::Lobby { names } if names.len() == 2)));
    // starting tells each player which actor is theirs
    room.connect(3);
    room.on_message(3, &ClientMsg::Hello { version: VERSION, name: "Cy".into() }.encode());
    room.drain();
    let setup = room.begin(&StartParams::default());
    assert_eq!(setup.names, vec![host_name, "Player 3".to_string(), "Cy".to_string()]);
    let starts: Vec<(PeerId, u8)> = room.drain().iter().filter_map(|o| match ServerMsg::decode(&o.bytes) {
        Ok(ServerMsg::Start { you, .. }) => Some((o.peer, you)),
        _ => None,
    }).collect();
    assert_eq!(starts, vec![(2, 1), (3, 2)]);
}

#[test]
fn the_lobby_refuses_the_wrong_version_a_full_room_and_late_joiners() {
    let mut room = Room::new("Host");
    room.connect(1);
    room.on_message(1, &ClientMsg::Hello { version: VERSION + 1, name: "Old".into() }.encode());
    let m = ServerMsg::decode(&room.drain()[0].bytes).unwrap();
    assert!(matches!(m, ServerMsg::Reject { ref reason } if reason.contains("version")), "{m:?}");
    assert_eq!(room.names().len(), 1);
    for id in 10..10 + MAX_HUMANS as u32 {
        room.connect(id);
        room.on_message(id, &ClientMsg::Hello { version: VERSION, name: "x".into() }.encode());
    }
    assert_eq!(room.names().len(), MAX_HUMANS, "the room holds {MAX_HUMANS} people");
    let last = room.drain().pop().unwrap();
    assert!(matches!(ServerMsg::decode(&last.bytes), Ok(ServerMsg::Lobby { .. }) | Ok(ServerMsg::Reject { .. })));
    // garbage is ignored
    room.on_message(10, &[1, 2, 3, 4]);
    room.on_message(999, &ClientMsg::Hello { version: VERSION, name: "ghost".into() }.encode());
    assert_eq!(room.names().len(), MAX_HUMANS);
    room.begin(&StartParams::default());
    room.drain();
    room.connect(500);
    room.on_message(500, &ClientMsg::Hello { version: VERSION, name: "Late".into() }.encode());
    let m = ServerMsg::decode(&room.drain()[0].bytes).unwrap();
    assert!(matches!(m, ServerMsg::Reject { ref reason } if reason.contains("started")), "{m:?}");
}

#[test]
fn the_match_waits_for_every_island_then_starts() {
    let mut h = Harness::new(2, 4, true, GOOD);
    assert!(!h.host.has_started());
    assert_eq!(h.host.waiting_for(), 2);
    // the match does not move until both have said they are ready (the clients do as soon as their island is built)
    let t0 = h.host.game.time;
    h.step(0.01, &idle(), &[]);
    assert!(!h.host.has_started() && h.host.game.time == t0, "the Ready messages are still on their way");
    h.run(0.5, &idle(), &[]);
    assert!(h.host.has_started());
    assert!(h.host.game.time > t0 + 0.3);
    assert_eq!(h.host.waiting_for(), 0);
}

#[test]
fn a_late_player_is_dropped_after_the_wait_runs_out() {
    let mut room = Room::new("Host");
    room.connect(1);
    room.on_message(1, &ClientMsg::Hello { version: VERSION, name: "Slow".into() }.encode());
    let setup = room.begin(&quick_params(2, true));
    room.drain();
    let mut host = Host::new(World::generate(1234), setup, room);
    for _ in 0..(READY_TIMEOUT * 10.0) as usize + 20 {
        host.update(0.1, &idle());
        if host.has_started() {
            break;
        }
    }
    assert!(host.has_started());
    assert!(!host.game.is_remote(1), "the slow player's actor went to a bot");
    assert!(host.game.actors[1].brain.is_some());
}

#[test]
fn both_sides_agree_on_the_start_of_a_match() {
    let mut h = Harness::new(1, 8, true, GOOD);
    h.run(1.0, &idle(), &[]);
    let (host, client) = (&h.host.game, &h.clients[0].game);
    assert_eq!(host.actors.len(), client.actors.len());
    for (a, b) in host.actors.iter().zip(&client.actors) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.human, b.human || a.id == 1, "{}", a.id);
        assert_eq!(a.outfit.shirt, b.outfit.shirt, "outfits come out the same on every machine");
    }
    assert_eq!(client.local, 1);
    assert_eq!(host.pickups.len(), client.pickups.len());
    assert_eq!(host.chests.len(), client.chests.len());
    assert!(h.clients[0].is_synced());
    assert!(h.clients[0].stats.snapshots > 10);
}

#[test]
fn an_idle_player_and_the_bots_look_the_same_on_both_machines() {
    let mut h = Harness::new(1, 10, true, GOOD);
    h.run(8.0, &idle(), &[]);
    let (host, client) = (&h.host.game, &h.clients[0].game);
    // the player's own actor: predicted and acknowledged, nothing to fix
    let (a, b) = (&host.actors[1], &client.actors[1]);
    assert!((a.pos - b.pos).length() < 0.05, "own actor: host {:?} vs client {:?}", a.pos, b.pos);
    // everybody else is shown a moment in the past, so they trail by at most what they could walk in that time
    let mut worst: f32 = 0.0;
    for (a, b) in host.actors.iter().zip(&client.actors).filter(|(a, _)| a.id != 1) {
        worst = worst.max((a.pos - b.pos).length());
        assert_eq!(a.alive, b.alive, "actor {}", a.id);
        assert_eq!(a.inv.selected_item().map(|i| i.name()), b.inv.selected_item().map(|i| i.name()), "actor {} holds the same thing", a.id);
    }
    assert!(worst < 3.0, "worst lag behind the host: {worst} m");
    assert_eq!(h.clients[0].stats.teleports, 0);
}

#[test]
fn running_is_predicted_without_corrections_and_ends_where_the_host_has_it() {
    let mut h = Harness::new(1, 0, true, Net { latency: 0.06, jitter: 0.0, loss: 0.0 });
    // a flat open place for the guest (the host moves it, the client's replica learns from snapshots)
    let p = crate::game::testutil::free_spot(&h.host.game);
    for who in [&mut h.host.game.actors[1], &mut h.clients[0].game.actors[1]] {
        who.pos = p;
        who.mode = MoveMode::Ground;
        who.on_ground = true;
        who.peak_y = p.y;
    }
    h.run(1.0, &idle(), &[idle()]);
    let start = h.host.game.actors[1].pos;
    let input = PlayerInput { move_axis: Vec2::new(0.0, 1.0), look: Vec2::new(0.0, 0.0), ..Default::default() };
    for k in 0..240 {
        // zig-zag a bit with the mouse so the commands differ from each other
        let look = Vec2::new(if (k / 60) % 2 == 0 { 0.004 } else { -0.004 }, 0.0);
        h.step(1.0 / 60.0, &idle(), &[PlayerInput { look, ..input.clone() }]);
    }
    let c = &h.clients[0];
    assert!(c.stats.corrections == 0, "nothing disturbed the run, yet the prediction was corrected {} times", c.stats.corrections);
    h.run(1.0, &idle(), &[idle()]);
    let (host_pos, client_pos) = (h.host.game.actors[1].pos, h.clients[0].game.actors[1].pos);
    assert!((host_pos - start).length() > 10.0, "the player ran");
    assert!((host_pos - client_pos).length() < 0.02, "host {host_pos:?} vs client {client_pos:?}");
}
