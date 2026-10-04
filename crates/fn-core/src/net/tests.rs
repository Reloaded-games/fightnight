//! The host and its players, talking through an in-memory network with latency, jitter and loss.

use super::client::*;
use super::host::*;
use super::proto::*;
use crate::game::actor::*;
use crate::game::events::Event;
use crate::game::items::*;
use crate::game::{Difficulty, Game, Phase, PlayerInput};
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
    bytes_down: usize,
    bytes_up: usize,
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
        Harness { host, clients, up: (0..guests).map(|_| Pipe::default()).collect(), down: (0..guests).map(|_| Pipe::default()).collect(), net, now: 0.0, rng: Rng::new(77), bytes_down: 0, bytes_up: 0 }
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
            self.bytes_down += o.bytes.len();
            self.down[k].push(self.now, o.reliable, o.bytes, &self.net, &mut self.rng);
        }
        for k in 0..self.clients.len() {
            for b in self.down[k].ready(self.now) {
                self.clients[k].on_message(&b, now_ms);
            }
            let inp = client_in.get(k).cloned().unwrap_or_default();
            self.clients[k].update(dt, &inp, now_ms);
            for o in self.clients[k].drain() {
                self.bytes_up += o.bytes.len();
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

/// Wait until the host has started the match.
fn started(h: &mut Harness) {
    for _ in 0..120 {
        if h.host.has_started() {
            return;
        }
        h.step(1.0 / 60.0, &idle(), &[]);
    }
    panic!("the match never started");
}

/// A random but sticky sequence of inputs, like a person running around.
fn wander(rng: &mut Rng, k: usize) -> PlayerInput {
    let phase = k / 45;
    let mut s = Rng::new(phase as u64 * 7919 + 13);
    PlayerInput {
        move_axis: Vec2::new(s.range(-1.0, 1.0).round(), if s.chance(0.8) { 1.0 } else { 0.0 }),
        look: Vec2::new(rng.range(-0.03, 0.03), rng.range(-0.004, 0.004)),
        jump: rng.chance(0.03),
        sprint: s.chance(0.5),
        crouch: s.chance(0.1),
        ..Default::default()
    }
}

/// Put the guest on a flat open place, on both machines.
fn plant_guest(h: &mut Harness) -> Vec3 {
    let p = crate::game::testutil::free_spot(&h.host.game);
    for who in [&mut h.host.game.actors[1], &mut h.clients[0].game.actors[1]] {
        who.pos = p;
        who.mode = MoveMode::Ground;
        who.on_ground = true;
        who.peak_y = p.y;
    }
    p
}

#[test]
fn jitter_and_packet_loss_do_not_break_prediction() {
    let net = Net { latency: 0.05, jitter: 0.04, loss: 0.15 };
    let mut h = Harness::new(1, 6, true, net);
    started(&mut h);
    plant_guest(&mut h);
    h.run(1.0, &idle(), &[idle()]);
    let mut rng = Rng::new(5);
    for k in 0..(20 * 60) {
        let inp = wander(&mut rng, k);
        h.step(1.0 / 60.0, &idle(), &[inp]);
        let a = &h.clients[0].game.actors[1];
        assert!(a.pos.is_finite() && a.pos.y > -50.0, "k {k}: {:?}", a.pos);
    }
    h.run(3.0, &idle(), &[idle()]);
    let (host, client) = (h.host.game.actors[1].pos, h.clients[0].game.actors[1].pos);
    assert!((host - client).length() < 0.1, "after the dust settled: host {host:?} client {client:?}");
    assert!(h.clients[0].stats.snapshots > 400, "snapshots {}", h.clients[0].stats.snapshots);
    let skipped = h.host.game.remotes[1].as_ref().map_or(0, |r| r.skipped);
    assert!(skipped < 30, "commands the host had to skip: {skipped}");
}

#[test]
fn a_push_the_client_could_not_know_about_is_corrected_without_a_jump() {
    let mut h = Harness::new(1, 0, true, Net { latency: 0.05, jitter: 0.0, loss: 0.0 });
    started(&mut h);
    let p = plant_guest(&mut h);
    h.run(1.0, &idle(), &[idle()]);
    // (placing the guest took one jump, which the client's first snapshot corrected)
    let (corrections0, teleports0) = (h.clients[0].stats.corrections, h.clients[0].stats.teleports);
    // an explosion beside the guest, on the host only
    let blast = p + Vec3::new(1.2, 0.0, 0.0);
    h.host.game.explode(blast, 0, WeaponKind::RocketLauncher, Rarity::Common);
    let mut max_step: f32 = 0.0;
    let mut last = h.clients[0].game.actors[1].pos;
    for _ in 0..180 {
        h.step(1.0 / 60.0, &idle(), &[idle()]);
        let now = h.clients[0].game.actors[1].pos;
        max_step = max_step.max((now - last).length());
        last = now;
    }
    let (host, client) = (h.host.game.actors[1].pos, h.clients[0].game.actors[1].pos);
    assert!((host - client).length() < 0.05, "host {host:?} client {client:?}");
    assert!((host - p).length() > 0.5, "the blast threw the player: {:?} -> {host:?}", p);
    assert!(h.clients[0].stats.corrections > corrections0, "the client had to be corrected");
    assert_eq!(h.clients[0].stats.teleports, teleports0);
    assert!(max_step < 1.0, "the screen never jumped: biggest step in a frame {max_step}");
}

#[test]
fn what_lies_around_the_island_follows_the_host() {
    let mut h = Harness::new(1, 6, true, GOOD);
    started(&mut h);
    let spot = crate::game::testutil::free_spot(&h.host.game);
    {
        let g = &mut h.host.game;
        // a chest is opened, loot is dropped and picked up, walls go up and take damage, a tree falls
        let c = g.chests.len() / 2;
        g.open_chest(c, 0);
        g.actors[0].pos = spot;
        g.actors[0].mode = MoveMode::Ground;
        g.actors[0].inv.mats = [500, 0, 0];
        let id = g.spawn_pickup(spot + Vec3::Y, crate::game::PickupKind::Ammo { kind: AmmoKind::Medium, amount: 30 }, true);
        g.spawn_pickup(spot + Vec3::new(3.0, 1.0, 0.0), crate::game::PickupKind::Weapon { kind: WeaponKind::Sniper, rarity: Rarity::Legendary, ammo: 1 }, true);
        g.pickups.retain(|p| p.id != id);
        let key = crate::game::pieces::PieceKey { kind: PieceKind::Wall, x: 4, z: 4, level: 0, dir: 0 };
        let wall = g.pieces.insert(key, Mat::Stone, spot.y, 0);
        g.pieces.insert(crate::game::pieces::PieceKey { kind: PieceKind::Floor, x: 4, z: 4, level: 0, dir: 0 }, Mat::Wood, spot.y, 0);
        g.damage_piece(wall, 50.0, Some(0));
        let idx = g.world.harvest.iter().position(|t| t.alive && t.kind == crate::world::props::HarvestKind::Tree).unwrap();
        for _ in 0..12 {
            let at = g.world.harvest[idx].pos;
            g.harvest(0, idx, at);
        }
        assert!(!g.world.harvest[idx].alive, "the tree is down");
    }
    h.run(1.0, &idle(), &[idle()]);
    // mutate more while the match runs: a piece disappears, a stack shrinks
    let wall_id = h.host.game.pieces.iter().find(|p| p.key.kind == PieceKind::Wall).map(|p| p.id).unwrap();
    h.host.game.pieces.remove(wall_id);
    if let Some(p) = h.host.game.pickups.iter_mut().find(|p| matches!(p.kind, crate::game::PickupKind::Weapon { .. })) {
        p.grounded = true;
    }
    h.run(2.0, &idle(), &[idle()]);
    let (host, client) = (&h.host.game, &h.clients[0].game);
    let mut hp: Vec<(u32, String)> = host.pickups.iter().map(|p| (p.id, format!("{:?}", p.kind))).collect();
    let mut cp: Vec<(u32, String)> = client.pickups.iter().map(|p| (p.id, format!("{:?}", p.kind))).collect();
    hp.sort();
    cp.sort();
    assert_eq!(hp, cp, "the same loose items");
    assert_eq!(host.chests.iter().map(|c| c.opened).collect::<Vec<_>>(), client.chests.iter().map(|c| c.opened).collect::<Vec<_>>());
    assert!(client.chests.iter().any(|c| c.opened), "the chest opened on the host is open here");
    let pieces = |g: &Game| {
        let mut v: Vec<(u32, String, u32)> = g.pieces.iter().map(|p| (p.id, format!("{:?}{:?}", p.key, p.mat), (p.hp * 10.0) as u32)).collect();
        v.sort();
        v
    };
    assert_eq!(pieces(host), pieces(client));
    assert_eq!(host.pieces.count(), 1);
    let felled = |g: &Game| g.world.harvest.iter().enumerate().filter(|(_, t)| !t.alive).map(|(i, _)| i).collect::<Vec<_>>();
    assert_eq!(felled(host), felled(client), "the same trees are down");
    assert_eq!(felled(host).len(), 1);
    // and a fallen tree no longer blocks the way on the guest's copy
    let t = &client.world.harvest[felled(client)[0]];
    let mut blocked = false;
    client.world.statics.query(&crate::math::Aabb::new(t.pos - Vec3::splat(0.2), t.pos + Vec3::splat(0.2)), |_, c| blocked |= matches!(c.tag, crate::world::collision::Tag::Tree(_)));
    assert!(!blocked, "its collider is gone");
}

#[test]
fn a_guest_can_shoot_a_bot_and_only_the_guest_gets_the_hit_marker() {
    let mut h = Harness::new(1, 3, true, GOOD);
    started(&mut h);
    let p = plant_guest(&mut h);
    {
        let g = &mut h.host.game;
        g.actors[1].inv.add_weapon(WeaponKind::AssaultRifle, Rarity::Epic, 30);
        g.actors[1].inv.ammo[AmmoKind::Medium.index()] = 120;
        g.select_slot(1, 1);
        // a bot 14 m ahead (the guest looks towards -Z), standing still
        let w = &g.world;
        let q = Vec3::new(p.x, w.hm.height_at(p.x, p.z - 14.0), p.z - 14.0);
        let b = &mut g.actors[2];
        b.pos = q;
        b.mode = MoveMode::Ground;
        b.on_ground = true;
        b.brain = None;
        b.hp = 100.0;
        for k in [0usize, 3, 4] {
            if k != 2 {
                g.actors[k].brain = None;
                g.actors[k].pos = Vec3::new(300.0, 5.0, 300.0);
            }
        }
    }
    h.run(1.0, &idle(), &[idle()]);
    let ammo0 = h.clients[0].game.actors[1].inv.selected_weapon().map(|w| w.2).unwrap();
    assert_eq!(ammo0, 30);
    let aim = PlayerInput { fire: true, fire_pressed: true, ads: true, ..Default::default() };
    let (mut hit_marker_guest, mut hit_marker_host) = (0.0f32, 0.0f32);
    // look at the bot: the aim ray starts at the shoulder, so aim a little to the side
    let before = h.host.game.actors[2].hp + h.host.game.actors[2].shield;
    for _ in 0..90 {
        let to = h.host.game.actors[2].chest() - h.host.game.aim_ray(1).0;
        let want_yaw = yaw_of(Vec2::new(to.x, to.z));
        let want_pitch = to.y.atan2(Vec2::new(to.x, to.z).length());
        let c = &h.clients[0].game.actors[1];
        let look = Vec2::new(-angle_diff(c.yaw, want_yaw).clamp(-0.2, 0.2), (want_pitch - c.pitch).clamp(-0.1, 0.1));
        h.step(1.0 / 60.0, &idle(), &[PlayerInput { look, ..aim.clone() }]);
        hit_marker_guest = hit_marker_guest.max(h.clients[0].game.hit_marker);
        hit_marker_host = hit_marker_host.max(h.host.game.hit_marker);
    }
    let after = h.host.game.actors[2].hp + h.host.game.actors[2].shield;
    assert!(after < before - 20.0, "the guest's bullets hit the bot on the host: {before} -> {after}");
    let a = &h.clients[0].game.actors[1];
    assert!(a.inv.selected_weapon().map(|w| w.2).unwrap() < 30, "the guest's ammo went down on its screen");
    assert!(hit_marker_guest > 0.1, "the guest saw its hit marker");
    assert_eq!(hit_marker_host, 0.0, "the host did not");
    assert!(h.clients[0].game.fx.numbers.iter().any(|n| n.amount > 0.0) || h.clients[0].game.fx.count() > 0, "effects of the shots play on the guest");
}

#[test]
fn events_go_only_to_those_who_could_see_or_hear_them() {
    let mut h = Harness::new(2, 2, true, GOOD);
    started(&mut h);
    let g = &mut h.host.game;
    for (i, x) in [(0usize, 0.0f32), (1, 40.0), (2, 600.0), (3, 0.0), (4, 0.0)] {
        g.actors[i].pos = Vec3::new(x, 10.0, 0.0);
        g.actors[i].alive = true;
        g.actors[i].mode = MoveMode::Ground;
    }
    let p = Vec3::new(30.0, 10.0, 0.0);
    let shot = Event::Shot { actor: 3, pos: p, weapon: WeaponKind::Pistol, end: p, hit_actor: false };
    use crate::net::host::audience;
    assert!(audience(g, &shot, 1), "guest 1 is 10 m away");
    assert!(!audience(g, &shot, 2), "guest 2 is 570 m away");
    // personal events only for the person they are about
    assert!(audience(g, &Event::Hurt { actor: 2, amount: 5.0, from: None }, 2));
    assert!(!audience(g, &Event::Hurt { actor: 2, amount: 5.0, from: None }, 1));
    assert!(audience(g, &Event::HitConfirm { actor: 1, head: false, shield: false, kill: false }, 1));
    assert!(!audience(g, &Event::HitConfirm { actor: 1, head: false, shield: false, kill: false }, 2));
    // a toast for everybody, or for one
    assert!(audience(g, &Event::Toast { actor: None, text: String::new(), secs: 1.0, style: 0 }, 2));
    assert!(!audience(g, &Event::Toast { actor: Some(1), text: String::new(), secs: 1.0, style: 0 }, 2));
    // elimination and storm news reach everyone; the player's own steps do not come back (they are predicted)
    assert!(audience(g, &Event::Eliminated { victim: 3, killer: None, weapon: None, storm: true }, 2));
    assert!(audience(g, &Event::StormPhase { phase: 1, shrinking: true }, 2));
    assert!(!audience(g, &Event::Footstep { actor: 1, pos: Vec3::new(40.0, 10.0, 0.0), surface: crate::game::events::Surface::Grass }, 1));
    assert!(audience(g, &Event::Footstep { actor: 3, pos: Vec3::new(40.0, 10.0, 0.0), surface: crate::game::events::Surface::Grass }, 1));
    assert!(!audience(g, &Event::Footstep { actor: 3, pos: Vec3::new(500.0, 10.0, 0.0), surface: crate::game::events::Surface::Grass }, 1));
    // a fallen player watches the whole match
    g.actors[2].alive = false;
    assert!(audience(g, &shot, 2));
    // noise is for the bots
    assert!(!audience(g, &Event::Noise { pos: p, radius: 100.0, source: 0 }, 1));
}

#[test]
fn a_whole_match_with_two_guests_ends_with_the_same_winner_everywhere() {
    let mut params = quick_params(10, true);
    params.storm_speed = 30.0;
    let mut h = Harness::with_params(2, params, GOOD);
    let mut rng = Rng::new(21);
    let mut k = 0;
    while h.host.game.phase != Phase::Over && k < 40 * 60 * 10 {
        let inputs = [wander(&mut rng, k), wander(&mut rng, k + 99999)];
        h.step(1.0 / 20.0, &idle(), &inputs);
        k += 1;
    }
    assert_eq!(h.host.game.phase, Phase::Over, "the match finished");
    h.run(3.0, &idle(), &[idle(), idle()]);
    let host = &h.host.game;
    for c in &h.clients {
        let g = &c.game;
        assert_eq!(g.phase, Phase::Over);
        assert_eq!(g.winner, host.winner, "everybody agrees who won");
        assert_eq!(g.tie, host.tie);
        let me = &g.actors[g.local];
        let theirs = &host.actors[g.local];
        assert_eq!(me.alive, theirs.alive);
        assert_eq!(me.placement, theirs.placement, "the same placement on the end screen");
        assert_eq!(me.kills, theirs.kills);
        let json = crate::game::hud::hud_json(g, &g.camera(1.6), false);
        assert!(json.contains("\"ph\":2"), "{json}");
        assert!(g.alive_cache <= 1 || host.tie);
    }
    assert!(h.clients.iter().all(|c| c.stats.snapshots > 100));
}

#[test]
fn the_guest_rides_the_bus_jumps_and_glides_down_with_the_host_agreeing() {
    let mut h = Harness::new(1, 4, false, GOOD);
    started(&mut h);
    h.run(8.0, &idle(), &[idle()]);
    {
        let (host, client) = (&h.host.game, &h.clients[0].game);
        assert_eq!(host.actors[1].mode, MoveMode::Bus);
        assert_eq!(client.actors[1].mode, MoveMode::Bus);
        assert!(client.bus.active && host.bus.active);
        assert!(client.bus.pos.distance(host.bus.pos) < 5.0, "the bus is where the host has it, within a lag's worth of travel: {}", client.bus.pos.distance(host.bus.pos));
        assert!(client.actors[1].pos.distance(client.bus.pos - Vec3::Y) < 0.5, "the guest rides along");
    }
    // jump: Space leaves the bus, then the guest dives and the glider opens by itself
    let jump = PlayerInput { exit_bus: true, ..Default::default() };
    h.step(1.0 / 60.0, &idle(), &[jump]);
    let mut modes = vec![];
    for _ in 0..(60 * 60) {
        let look_down = PlayerInput { look: Vec2::new(0.0, -0.01), move_axis: Vec2::new(0.0, 1.0), ..Default::default() };
        h.step(1.0 / 60.0, &idle(), &[look_down]);
        let m = h.clients[0].game.actors[1].mode;
        if modes.last() != Some(&m) {
            modes.push(m);
        }
        if m == MoveMode::Ground && h.host.game.actors[1].mode == MoveMode::Ground {
            break;
        }
    }
    assert_eq!(modes, vec![MoveMode::Bus, MoveMode::Freefall, MoveMode::Glide, MoveMode::Ground], "modes seen on the guest's screen");
    h.run(2.0, &idle(), &[idle()]);
    let (host, client) = (h.host.game.actors[1].pos, h.clients[0].game.actors[1].pos);
    assert!((host - client).length() < 1.0, "landed together: host {host:?} client {client:?}");
    assert!(h.clients[0].stats.teleports <= 1, "teleports {}", h.clients[0].stats.teleports);
}

#[test]
fn a_guest_who_says_goodbye_or_goes_quiet_is_replaced_by_a_bot() {
    // goodbye
    let mut h = Harness::new(2, 4, true, GOOD);
    started(&mut h);
    h.run(0.5, &idle(), &[idle(), idle()]);
    h.clients[0].leave();
    h.run(0.5, &idle(), &[idle(), idle()]);
    assert!(!h.host.game.is_remote(1), "guest 1 left");
    assert!(h.host.game.actors[1].brain.is_some() && h.host.game.actors[1].name.ends_with("(bot)"));
    assert!(h.host.game.is_remote(2));
    assert!(h.host.peers().iter().any(|p| p.actor == 1 && !p.connected));
    // going quiet: no packets from guest 2 at all
    for _ in 0..(TIMEOUT as usize + 2) * 60 {
        h.now += 1.0 / 60.0;
        h.host.update(1.0 / 60.0, &idle());
        h.host.drain();
    }
    assert!(!h.host.game.is_remote(2), "the silent guest was dropped");
    assert!(h.host.game.actors[2].brain.is_some());
}

#[test]
fn the_guest_learns_that_the_host_closed_the_room() {
    let mut h = Harness::new(1, 2, true, GOOD);
    started(&mut h);
    h.run(0.3, &idle(), &[idle()]);
    assert!(h.clients[0].closed().is_none());
    h.host.close();
    h.run(0.3, &idle(), &[idle()]);
    assert!(h.clients[0].closed().is_some());
}

#[test]
fn a_fallen_guest_spectates_and_keeps_seeing_the_match() {
    let mut h = Harness::new(1, 6, true, GOOD);
    started(&mut h);
    plant_guest(&mut h);
    h.run(1.0, &idle(), &[idle()]);
    // a bot's rocket finishes the guest off
    {
        let g = &mut h.host.game;
        g.actors[1].hp = 10.0;
        g.actors[1].shield = 0.0;
        let chest = g.actors[1].chest();
        g.projectiles.push(crate::game::Projectile { pos: chest + Vec3::Y * 0.5, vel: Vec3::NEG_Y * 58.0, owner: 2, kind: WeaponKind::RocketLauncher, rarity: Rarity::Common, life: 6.0 });
    }
    h.run(2.0, &idle(), &[idle()]);
    let c = &h.clients[0].game;
    assert!(!c.actors[1].alive);
    let cam = c.camera_actor();
    assert_ne!(cam, 1, "spectating somebody else");
    assert!(c.actors[cam].alive);
    let json = crate::game::hud::hud_json(c, &c.camera(1.6), false);
    assert!(json.contains("\"dead\":true") && json.contains("\"spec\":"), "{json}");
    // the person they watch is where the host has them (within the interpolation lag)
    assert!(c.actors[cam].pos.distance(h.host.game.actors[cam].pos) < 4.0);
    // the kill feed shows how they went
    assert!(c.feed.iter().any(|f| f.victim_is_player), "{:?}", c.feed);
}

#[test]
fn a_client_cannot_move_faster_than_real_time_by_sending_a_flood_of_commands() {
    let mut h = Harness::new(1, 0, true, GOOD);
    started(&mut h);
    let p = plant_guest(&mut h);
    h.run(1.0, &idle(), &[idle()]);
    let start = h.host.game.actors[1].pos;
    // ten seconds of running, sent at once, over and over for one second of host time
    let cmds: Vec<crate::game::cmd::Cmd> = (0..600u32).map(|k| crate::game::cmd::Cmd { seq: 100_000 + k, axis: [0, 127], ..Default::default() }).collect();
    for chunk in cmds.chunks(MAX_CMDS) {
        h.host.on_message(1, &ClientMsg::Cmds { time: 5, cmds: chunk.to_vec() }.encode());
    }
    for _ in 0..60 {
        h.host.update(1.0 / 60.0, &idle());
        h.host.drain();
    }
    let ran = (h.host.game.actors[1].pos - start).length();
    assert!(ran < 8.5, "a second of host time lets a player run about 6.4 m, not {ran} (from {p:?})");
}

#[test]
fn the_network_traffic_of_a_busy_match_is_modest() {
    let mut h = Harness::new(2, 40, true, GOOD);
    started(&mut h);
    plant_guest(&mut h);
    h.run(2.0, &idle(), &[idle(), idle()]);
    let (d0, u0) = (h.bytes_down, h.bytes_up);
    let mut rng = Rng::new(3);
    for k in 0..(10 * 60) {
        let a = wander(&mut rng, k);
        let b = wander(&mut rng, k + 5000);
        h.step(1.0 / 60.0, &idle(), &[a, b]);
    }
    let down = (h.bytes_down - d0) as f32 / 10.0 / 2.0;
    let up = (h.bytes_up - u0) as f32 / 10.0 / 2.0;
    assert!(down < 70_000.0, "host to each guest: {down:.0} bytes/s");
    assert!(up < 14_000.0, "each guest to the host: {up:.0} bytes/s");
}

#[test]
fn old_and_garbled_messages_change_nothing() {
    let mut h = Harness::new(1, 4, true, GOOD);
    started(&mut h);
    h.run(2.0, &idle(), &[idle()]);
    let snaps = h.clients[0].stats.snapshots;
    // a snapshot from the past, delivered late
    let stale = {
        let g = &h.host.game;
        ServerMsg::Snapshot(Box::new(Snapshot {
            time: 0.01,
            echo: 0,
            ack: 0,
            phase: Phase::Over,
            tie: true,
            winner: Some(3),
            match_time: 0.0,
            bus_active: true,
            bus_t: 0.0,
            storm: storm_net(&g.storm),
            own: own_of(&g.actors[1]),
            actors: vec![],
            projectiles: vec![],
        }))
        .encode()
    };
    use crate::net::host::{own_of, storm_net};
    h.clients[0].on_message(&stale, 99);
    assert_eq!(h.clients[0].stats.snapshots, snaps, "ignored");
    assert_eq!(h.clients[0].game.phase, Phase::Playing);
    // noise in both directions
    let mut rng = Rng::new(9);
    for _ in 0..3000 {
        let n = rng.below(200);
        let b: Vec<u8> = (0..n).map(|_| rng.below(256) as u8).collect();
        h.clients[0].on_message(&b, 1);
        h.host.on_message(1, &b);
        h.host.on_message(77, &b);
    }
    h.run(1.0, &idle(), &[idle()]);
    assert!(h.host.game.is_remote(1) && h.clients[0].game.actors[1].pos.is_finite());
}
