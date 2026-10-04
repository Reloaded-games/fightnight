//! Remote humans: the people playing over the network. The host runs their actors like any other, but instead of a keyboard
//! or a brain their intent comes from the [`Cmd`]s the client sends, applied one by one for as much simulated time as the
//! client says each one covers - never more than real time has passed.

use super::cmd::*;
use super::intent::Intent;
use super::*;
use std::collections::VecDeque;

/// More simulated time than this waiting in a queue means the client sent a burst (or was far behind): the oldest is skipped.
const MAX_BACKLOG: f32 = 0.5;
/// ... down to this much.
const KEEP_BACKLOG: f32 = 0.2;
/// Commands are applied at the speed of real time. Unused time may pile up to this much so that a short stall is made up for.
const MAX_BUDGET: f32 = 0.12;
/// The most a shot is rewound (seconds): a player on a very slow connection does not get to shoot into the distant past.
pub const MAX_REWIND: f32 = 0.25;
/// While more than this is queued the host applies commands a little faster than real time to work the queue down.
const CATCH_UP_AT: f32 = 0.1;
const CATCH_UP_RATE: f32 = 1.5;

#[derive(Debug, Default)]
pub struct Remote {
    queue: VecDeque<Cmd>,
    /// Sequence number of the last command that was applied or skipped (what the client is told it can forget).
    pub applied: u32,
    /// The newest command received so far.
    newest: u32,
    budget: f32,
    /// Commands thrown away because the queue got too long, for diagnostics.
    pub skipped: u32,
}

impl Remote {
    /// Simulated time waiting in the queue.
    pub fn backlog(&self) -> f32 {
        self.queue.iter().map(|c| c.dt).sum()
    }
}

impl Game {
    /// Hand an actor over to a person on the network: from now on only commands from [`Game::push_cmds`] move it.
    pub fn make_remote(&mut self, actor: usize) {
        self.actors[actor].human = true;
        self.actors[actor].brain = None;
        self.remotes[actor] = Some(Remote::default());
    }

    /// Whether the actor is played over the network.
    pub fn is_remote(&self, actor: usize) -> bool {
        self.remotes.get(actor).is_some_and(|r| r.is_some())
    }

    /// Queue commands received from a client. Duplicates and anything older than what was already received are dropped, so
    /// a client may repeat its recent commands in every packet to survive packet loss.
    pub fn push_cmds(&mut self, actor: usize, cmds: &[Cmd]) {
        let Some(Some(r)) = self.remotes.get_mut(actor) else { return };
        for c in cmds {
            if c.is_sane() && seq_after(c.seq, r.newest) {
                r.newest = c.seq;
                let mut c = *c;
                c.buttons &= btn::ALL;
                r.queue.push_back(c);
            }
        }
    }

    /// The last command of this player that the host has dealt with.
    pub fn remote_ack(&self, actor: usize) -> u32 {
        self.remotes.get(actor).and_then(|r| r.as_ref()).map_or(0, |r| r.applied)
    }

    /// Give the actor of a player who left (or timed out) to a bot, so the match goes on without a body standing around.
    pub fn drop_remote(&mut self, actor: usize) {
        if self.remotes.get(actor).is_none_or(|r| r.is_none()) {
            return;
        }
        self.remotes[actor] = None;
        let a = &mut self.actors[actor];
        a.human = false;
        a.name = format!("{} (bot)", a.name);
        let brain = Box::new(ai::Brain::new(&mut self.rng, self.cfg.difficulty));
        self.actors[actor].brain = Some(brain);
    }

    /// Remember where everybody is (while anyone plays over the network).
    pub(super) fn record_history(&mut self) {
        if !self.remotes.iter().any(|r| r.is_some()) {
            self.history.clear();
            return;
        }
        self.history.push_back((self.time, self.actors.iter().map(|a| a.pos).collect()));
        while self.history.front().is_some_and(|f| self.time - f.0 > MAX_REWIND + 0.1) {
            self.history.pop_front();
        }
    }

    /// Where every actor stood at host time `t`, or `None` if that is now (or later) or nothing is known.
    pub fn positions_at(&self, t: f32) -> Option<Vec<Vec3>> {
        let newest = self.history.back()?;
        if t >= newest.0 {
            return None;
        }
        let t = t.max(self.time - MAX_REWIND);
        let mut prev = self.history.front()?;
        if t <= prev.0 {
            return Some(prev.1.clone());
        }
        for f in self.history.iter().skip(1) {
            if f.0 >= t {
                let k = ((t - prev.0) / (f.0 - prev.0).max(1e-5)).clamp(0.0, 1.0);
                return Some(prev.1.iter().zip(&f.1).map(|(a, b)| a.lerp(*b, k)).collect());
            }
            prev = f;
        }
        None
    }

    /// Apply what the remote players asked for during this step of `dt` seconds.
    pub(super) fn apply_remote_cmds(&mut self, dt: f32) {
        for i in 0..self.remotes.len() {
            if self.remotes[i].is_none() {
                continue;
            }
            if !self.actors[i].alive {
                // nothing to steer: forget what they sent, but let them know it was dealt with
                let r = self.remotes[i].as_mut().unwrap();
                r.queue.clear();
                r.applied = r.newest;
                continue;
            }
            let r = self.remotes[i].as_mut().unwrap();
            let mut backlog = r.backlog();
            while backlog > MAX_BACKLOG && r.queue.len() > 1 {
                let c = r.queue.pop_front().unwrap();
                r.applied = c.seq;
                r.skipped += 1;
                backlog -= c.dt;
                if backlog <= KEEP_BACKLOG {
                    break;
                }
            }
            let rate = if backlog > CATCH_UP_AT { CATCH_UP_RATE } else { 1.0 };
            r.budget = (r.budget + dt * rate).min(MAX_BUDGET);
            loop {
                let r = self.remotes[i].as_mut().unwrap();
                let Some(c) = r.queue.front().copied() else { break };
                if c.dt > r.budget + 1e-6 {
                    break;
                }
                r.queue.pop_front();
                r.budget -= c.dt;
                r.applied = c.seq;
                if !self.actors[i].alive {
                    break;
                }
                let it: Intent = c.to_intent();
                // a shot is resolved against the world as its shooter saw it
                if self.lag_comp && c.view > 0.0 && c.buttons & (btn::FIRE | btn::FIRE_PRESSED) != 0 {
                    self.rewound = self.positions_at(c.view);
                }
                self.apply_intent(i, &it, c.dt);
                self.rewound = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::game_cfg;
    use super::*;

    /// A room of `humans` people (the second one on the network) and `bots` bots, everyone on the ground.
    fn room(humans: usize, bots: usize) -> Game {
        let mut g = game_cfg(GameConfig { humans, bots, skip_bus: true, seed: 7, ..Default::default() });
        for i in 1..humans {
            g.make_remote(i);
        }
        g
    }

    /// A stretch of flat open ground to run on.
    fn open_ground(g: &mut Game, who: usize) {
        let p = super::super::testutil::free_spot(g);
        let a = &mut g.actors[who];
        a.pos = p;
        a.mode = MoveMode::Ground;
        a.on_ground = true;
        a.peak_y = p.y;
        a.yaw = 0.0;
    }

    fn forward(seq: u32) -> Cmd {
        Cmd { seq, axis: [0, 127], ..Default::default() }
    }

    fn idle() -> PlayerInput {
        PlayerInput::default()
    }

    #[test]
    fn a_room_has_its_humans_first_and_the_bots_after() {
        let g = game_cfg(GameConfig { humans: 3, human_names: vec!["Ada".into()], bots: 5, skip_bus: true, ..Default::default() });
        assert_eq!(g.actors.len(), 8);
        assert_eq!(g.actors.iter().filter(|a| a.human).count(), 3);
        assert!(g.actors[..3].iter().all(|a| a.human) && g.actors[3..].iter().all(|a| !a.human));
        assert_eq!(g.actors[0].name, "You");
        assert_eq!(g.actors[1].name, "Ada");
        assert_eq!(g.actors[2].name, "Player 3");
        assert!(g.actors[3..].iter().all(|a| a.brain.is_some()));
        assert!(g.actors[..3].iter().all(|a| a.brain.is_none()));
    }

    #[test]
    fn commands_move_the_remote_actor_and_nobody_else() {
        let mut g = room(2, 3);
        open_ground(&mut g, 0);
        open_ground(&mut g, 1);
        g.actors[1].pos.x += 6.0;
        let (before0, before1) = (g.actors[0].pos, g.actors[1].pos);
        for k in 1..=60 {
            g.push_cmds(1, &[forward(k)]); // a client sends one for every step it takes
            g.update(1.0 / 60.0, &idle());
        }
        let moved = (g.actors[1].pos - before1).length();
        assert!(moved > 4.0 && moved < 7.0, "a second of running covers about 6 m, not {moved}");
        assert_eq!(g.remote_ack(1), 60, "all commands were applied");
        assert!((g.actors[0].pos - before0).length() < 0.01, "the local human did not move");
    }

    #[test]
    fn the_host_applies_what_a_client_predicted_step_for_step() {
        // the same commands on a copy of the actor with nothing else going on land in the same place
        let mut g = room(2, 0);
        open_ground(&mut g, 1);
        let mut twin = Actor::new(1, "twin", true, g.actors[1].outfit);
        twin.pos = g.actors[1].pos;
        twin.mode = MoveMode::Ground;
        twin.on_ground = true;
        twin.peak_y = twin.pos.y;
        twin.inv = g.actors[1].inv.clone();
        let mut cmds = vec![];
        for k in 0..90u32 {
            let yaw = (k as f32 * 0.03).sin();
            cmds.push(Cmd { seq: k + 1, dt: 1.0 / 60.0, axis: [(k % 40 < 20) as i8 * 127, 127], yaw, buttons: if k % 30 == 5 { btn::JUMP } else { 0 }, ..Default::default() });
        }
        {
            let env = Env::new(&g.world, &g.pieces.grid);
            let mut ev = vec![];
            for c in &cmds {
                let it = c.to_intent();
                twin.yaw = it.yaw;
                twin.pitch = it.pitch;
                movement::step_ground(&mut twin, &it, &env, c.dt, &mut ev);
                movement::update_body(&mut twin, &it, c.dt);
                movement::update_anim(&mut twin, c.dt);
            }
        }
        for c in &cmds {
            g.push_cmds(1, &[*c]);
            g.update(1.0 / 60.0, &idle());
        }
        assert_eq!(g.remote_ack(1), 90);
        let (a, b) = (g.actors[1].pos, twin.pos);
        assert!((a - b).length() < 1e-3, "host {a:?} vs prediction {b:?}");
        assert!((g.actors[1].vel - twin.vel).length() < 1e-3);
    }

    #[test]
    fn commands_are_applied_no_faster_than_real_time() {
        let mut g = room(2, 0);
        open_ground(&mut g, 1);
        let cmds: Vec<Cmd> = (1..=20).map(forward).collect(); // a third of a second, all at once
        g.push_cmds(1, &cmds);
        g.update(1.0 / 20.0, &idle());
        let done = g.remote_ack(1);
        assert!((1..=5).contains(&done), "50 ms of real time apply about 50 ms of commands, not {done} of 20");
        for _ in 0..30 {
            g.update(1.0 / 60.0, &idle());
        }
        assert_eq!(g.remote_ack(1), 20);
    }

    #[test]
    fn a_stall_is_made_up_for_but_only_a_little() {
        let mut g = room(2, 0);
        open_ground(&mut g, 1);
        // half a second with nothing from the client ...
        for _ in 0..30 {
            g.update(1.0 / 60.0, &idle());
        }
        // ... then a burst: only the stored-up time (a tenth of a second) is made up for at once
        let cmds: Vec<Cmd> = (1..=30).map(forward).collect();
        g.push_cmds(1, &cmds);
        g.update(1.0 / 60.0, &idle());
        let done = g.remote_ack(1);
        assert!((5..=9).contains(&done), "applied {done}");
    }

    #[test]
    fn a_flood_of_commands_is_cut_down() {
        let mut g = room(2, 0);
        open_ground(&mut g, 1);
        let cmds: Vec<Cmd> = (1..=120).map(forward).collect(); // two seconds in one go
        g.push_cmds(1, &cmds);
        g.update(1.0 / 60.0, &idle());
        let r = g.remotes[1].as_ref().unwrap();
        assert!(r.skipped > 30, "skipped {}", r.skipped);
        assert!(r.backlog() <= 0.5, "backlog {}", r.backlog());
        assert!(g.remote_ack(1) > 60, "the client is told which commands were dealt with, skipped ones too");
    }

    #[test]
    fn repeated_and_stale_commands_do_not_run_twice() {
        let mut g = room(2, 0);
        open_ground(&mut g, 1);
        g.push_cmds(1, &(1..=5).map(forward).collect::<Vec<_>>());
        g.push_cmds(1, &(3..=8).map(forward).collect::<Vec<_>>()); // overlaps with what came before
        g.push_cmds(1, &(1..=2).map(forward).collect::<Vec<_>>()); // late
        assert_eq!(g.remotes[1].as_ref().unwrap().backlog(), 8.0 / 60.0);
        for _ in 0..30 {
            g.update(1.0 / 60.0, &idle());
        }
        assert_eq!(g.remote_ack(1), 8);
    }

    #[test]
    fn nonsense_commands_are_refused() {
        let mut g = room(2, 0);
        open_ground(&mut g, 1);
        let p = g.actors[1].pos;
        g.push_cmds(1, &[Cmd { dt: 30.0, ..forward(1) }, Cmd { yaw: f32::NAN, ..forward(2) }, Cmd { dt: f32::INFINITY, ..forward(3) }]);
        g.push_cmds(0, &[forward(1)]); // not a remote player: ignored
        g.update(0.1, &idle());
        assert_eq!(g.remote_ack(1), 0);
        assert!((g.actors[1].pos - p).length() < 0.01);
        assert!(g.actors[1].pos.is_finite());
    }

    #[test]
    fn a_remote_player_rides_the_bus_and_jumps_when_asked() {
        let mut g = game_cfg(GameConfig { humans: 2, bots: 2, skip_bus: false, seed: 7, ..Default::default() });
        g.make_remote(1);
        for _ in 0..30 {
            g.update(0.1, &idle());
        }
        assert_eq!(g.actors[1].mode, MoveMode::Bus);
        assert!(g.actors[1].pos.distance(g.bus.pos - Vec3::Y) < 0.1, "rides along without sending a thing");
        g.push_cmds(1, &[Cmd { seq: 1, buttons: btn::EXIT_BUS, ..Default::default() }]);
        g.update(0.05, &idle());
        assert_eq!(g.actors[1].mode, MoveMode::Freefall);
        assert!(g.events.iter().any(|e| matches!(e, Event::BusJump { actor: 1, .. })));
    }

    #[test]
    fn hits_are_addressed_to_the_player_who_was_hit_or_who_hit() {
        let mut g = room(2, 1);
        // the bot (actor 2) shoots the remote human (1): a hurt for 1, nothing for 0
        g.actors[1].mode = MoveMode::Ground;
        g.damage_actor(1, 20.0, Some(2), "Pistol", false, Vec3::ZERO, false);
        assert!(g.events.iter().any(|e| matches!(e, Event::Hurt { actor: 1, .. })));
        assert!(!g.events.iter().any(|e| matches!(e, Event::Hurt { actor: 0, .. })));
        // the remote human shoots the bot: a hit marker for 1 only
        g.events.clear();
        g.actors[2].mode = MoveMode::Ground;
        g.damage_actor(2, 5.0, Some(1), "Pistol", false, Vec3::ZERO, false);
        assert!(g.events.iter().any(|e| matches!(e, Event::HitConfirm { actor: 1, .. })));
        assert!(!g.events.iter().any(|e| matches!(e, Event::HitConfirm { actor: 0, .. })));
        // a bot hurting a bot is nobody's business
        g.events.clear();
        g.actors[0].mode = MoveMode::Ground;
        g.actors[2].inv = Inventory::new();
        g.damage_actor(2, 5.0, Some(0), "Pistol", false, Vec3::ZERO, false);
        assert!(g.events.iter().any(|e| matches!(e, Event::HitConfirm { actor: 0, .. })));
    }

    #[test]
    fn each_games_own_view_reacts_only_to_events_for_its_local_actor() {
        let mut host = room(2, 1);
        let mut guest = room(2, 1);
        guest.local = 1; // the copy of the match running on the second player's machine
        for g in [&mut host, &mut guest] {
            g.events.push(Event::Hurt { actor: 1, amount: 10.0, from: None });
            g.events.push(Event::HitConfirm { actor: 1, head: false, shield: false, kill: false });
            g.events.push(Event::Toast { actor: Some(1), text: "for player two".into(), secs: 2.0, style: 0 });
            g.events.push(Event::Toast { actor: None, text: "for all".into(), secs: 2.0, style: 0 });
            g.after_update(1.0 / 60.0);
        }
        assert_eq!((host.hurt_flash, host.hit_marker), (0.0, 0.0));
        assert!(guest.hurt_flash > 0.9 && guest.hit_marker > 0.1);
        assert_eq!(host.toast.len(), 1);
        assert_eq!(host.toast[0].0, "for all");
        assert_eq!(guest.toast.len(), 2);
    }

    #[test]
    fn the_view_follows_the_local_actor_and_spectates_when_it_falls() {
        let mut g = room(2, 2);
        g.local = 1;
        assert_eq!(g.camera_actor(), 1);
        assert_eq!(g.player().id, 1);
        g.eliminate(1, Some(2), "Pistol", false);
        g.update(0.02, &idle());
        assert_ne!(g.camera_actor(), 1, "a fallen player watches someone else");
        assert!(g.actors[g.camera_actor()].alive);
        let json = hud::hud_json(&g, &g.camera(1.6), false);
        assert!(json.contains("\"dead\":true"), "{json}");
        assert!(json.contains("\"spec\":"));
    }

    #[test]
    fn a_dead_remote_player_is_not_steered_and_is_told_so() {
        let mut g = room(2, 2);
        open_ground(&mut g, 1);
        g.eliminate(1, Some(2), "Pistol", false);
        let p = g.actors[1].pos;
        g.push_cmds(1, &(1..=10).map(forward).collect::<Vec<_>>());
        g.update(0.1, &idle());
        assert_eq!(g.remote_ack(1), 10);
        assert_eq!(g.actors[1].pos, p);
    }

    #[test]
    fn a_player_who_leaves_is_replaced_by_a_bot() {
        let mut g = room(2, 2);
        open_ground(&mut g, 1);
        g.drop_remote(1);
        assert!(!g.is_remote(1) && !g.actors[1].human && g.actors[1].brain.is_some());
        assert!(g.actors[1].name.ends_with("(bot)"));
        g.push_cmds(1, &[forward(1)]);
        assert_eq!(g.remotes[1].as_ref().map(|r| r.backlog()), None, "nobody listens any more");
        for _ in 0..120 {
            g.update(1.0 / 30.0, &idle());
        }
        assert!(g.actors[1].pos.is_finite());
    }

    #[test]
    fn the_last_one_standing_wins_even_if_they_play_over_the_network() {
        let mut g = room(2, 3);
        open_ground(&mut g, 1);
        for i in [0usize, 2, 3, 4] {
            g.eliminate(i, Some(1), "Pistol", false);
        }
        g.update(0.05, &idle());
        assert_eq!(g.phase, Phase::Over);
        assert_eq!(g.winner, Some(1));
        assert!(g.actors[1].emoting, "the winner dances");
        // seen from the winner's machine
        g.local = 1;
        let json = hud::hud_json(&g, &g.camera(1.6), false);
        assert!(json.contains("\"won\":true"), "{json}");
        assert!(json.contains("\"place\":1"));
    }

    #[test]
    fn the_match_clock_keeps_running_for_the_winner_but_stops_when_no_human_is_left() {
        let mut g = room(2, 2);
        open_ground(&mut g, 1);
        for i in [0usize, 2, 3] {
            g.eliminate(i, Some(1), "Pistol", false);
        }
        g.update(0.05, &idle());
        assert_eq!(g.phase, Phase::Over);
        let t = g.match_time;
        for _ in 0..30 {
            g.update(1.0 / 60.0, &idle());
        }
        assert!(g.match_time > t + 0.4, "the winner is still alive");
        g.eliminate(1, None, "Fall damage", false);
        let t = g.match_time;
        for _ in 0..30 {
            g.update(1.0 / 60.0, &idle());
        }
        assert_eq!(g.match_time, t);
    }

    #[test]
    fn survival_time_is_kept_per_player() {
        let mut g = room(2, 2);
        g.match_time = 100.0;
        g.eliminate(1, Some(2), "Pistol", false);
        g.match_time = 160.0;
        g.eliminate(0, Some(2), "Pistol", false);
        assert_eq!((g.actors[1].survived, g.actors[0].survived), (100.0, 160.0));
        let json = hud::hud_json(&g, &g.camera(1.6), false);
        assert!(json.contains("\"time\":160"), "{json}");
    }
}
