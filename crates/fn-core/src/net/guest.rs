//! The guest's side of the lobby: say hello, follow who else is in the room, and wait for the host to start.

use super::host::{clean_name, Outgoing};
use super::proto::*;

pub struct GuestRoom {
    names: Vec<String>,
    you: usize,
    start: Option<(usize, MatchSetup)>,
    closed: Option<String>,
    welcomed: bool,
    out: Vec<Outgoing>,
}

impl GuestRoom {
    /// Join: the hello goes out as soon as the connection to the host is up.
    pub fn new(name: &str) -> GuestRoom {
        let mut r = GuestRoom { names: vec![], you: 0, start: None, closed: None, welcomed: false, out: vec![] };
        let m = ClientMsg::Hello { version: VERSION, name: clean_name(name, "Guest") };
        r.out.push(Outgoing { peer: 0, reliable: m.reliable(), bytes: m.encode() });
        r
    }

    pub fn on_message(&mut self, bytes: &[u8]) {
        let Ok(msg) = ServerMsg::decode(bytes) else { return };
        match msg {
            ServerMsg::Welcome { you, names } => {
                self.welcomed = true;
                self.you = you as usize;
                self.names = names;
            }
            ServerMsg::Lobby { names } => self.names = names,
            ServerMsg::Start { you, setup } => self.start = Some((you as usize, setup)),
            ServerMsg::Reject { reason } => self.closed = Some(reason),
            ServerMsg::Closed => self.closed = Some("The host closed the room.".into()),
            ServerMsg::Snapshot(_) | ServerMsg::Sync(_) | ServerMsg::Events { .. } => {}
        }
    }

    /// The connection to the host went away.
    pub fn disconnected(&mut self) {
        if self.start.is_none() && self.closed.is_none() {
            self.closed = Some("Lost the connection to the host.".into());
        }
    }

    pub fn leave(&mut self) {
        let m = ClientMsg::Bye;
        self.out.push(Outgoing { peer: 0, reliable: m.reliable(), bytes: m.encode() });
    }

    /// The host has accepted us into the room.
    pub fn is_in(&self) -> bool {
        self.welcomed
    }
    pub fn names(&self) -> &[String] {
        &self.names
    }
    /// The match the host started: which actor we are, and how to build it.
    pub fn start(&self) -> Option<&(usize, MatchSetup)> {
        self.start.as_ref()
    }
    /// Why we are out (refused, or the host left), if we are.
    pub fn closed(&self) -> Option<&str> {
        self.closed.as_deref()
    }
    pub fn drain(&mut self) -> Vec<Outgoing> {
        std::mem::take(&mut self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::super::host::{Room, StartParams};
    use super::*;

    /// Hand everything the host has to say to the guest it is for.
    fn route(host: &mut Room, guests: &mut [(u32, &mut GuestRoom)]) {
        for o in host.drain() {
            if let Some((_, g)) = guests.iter_mut().find(|(id, _)| *id == o.peer) {
                g.on_message(&o.bytes);
            }
        }
    }

    #[test]
    fn a_guest_follows_the_lobby_until_the_host_starts() {
        let mut host = Room::new("Host");
        let mut a = GuestRoom::new("Ada");
        let mut b = GuestRoom::new("Bo");
        for (id, g) in [(1u32, &mut a), (2, &mut b)] {
            host.connect(id);
            for o in g.drain() {
                host.on_message(id, &o.bytes);
            }
        }
        route(&mut host, &mut [(1, &mut a), (2, &mut b)]);
        assert!(a.is_in() && b.is_in());
        assert_eq!(a.names(), ["Host", "Ada", "Bo"], "Ada heard that Bo joined");
        assert_eq!(b.names(), ["Host", "Ada", "Bo"]);
        host.begin(&StartParams { seed: 99, bots: 5, ..Default::default() });
        route(&mut host, &mut [(1, &mut a), (2, &mut b)]);
        let (you_a, setup) = a.start().expect("start");
        assert_eq!((*you_a, setup.seed, setup.bots, setup.names.len()), (1, 99, 5, 3));
        assert_eq!(b.start().unwrap().0, 2);
        assert!(a.closed().is_none());
    }

    #[test]
    fn being_refused_or_losing_the_host_closes_the_lobby() {
        let mut host = Room::new("Host");
        let mut g = GuestRoom::new("Old");
        // a hello from a game of another version
        let hello = ClientMsg::Hello { version: VERSION + 3, name: "x".into() }.encode();
        host.connect(1);
        host.on_message(1, &hello);
        for o in host.drain() {
            g.on_message(&o.bytes);
        }
        assert!(g.closed().unwrap().contains("version"));
        let mut g = GuestRoom::new("Ada");
        g.disconnected();
        assert!(g.closed().unwrap().contains("connection"));
        // after the match started, a dropped connection is the match's business
        let mut g = GuestRoom::new("Ada");
        g.on_message(&ServerMsg::Start { you: 1, setup: MatchSetup { seed: 1, bots: 1, difficulty: crate::game::Difficulty::Easy, skip_bus: false, storm_speed: 1.0, start_mats: 0, names: vec!["a".into(), "b".into()] } }.encode());
        g.disconnected();
        assert!(g.closed().is_none());
        // garbage changes nothing
        g.on_message(&[1, 2, 3]);
        assert!(g.start().is_some());
    }
}
