//! The scoreboard: everyone's numbers for the match and the points they are worth. The end screen shows it, and the page
//! adds each player's finished matches up into the career leaderboard it keeps (`web/leaderboard.js`).
//!
//! A match is worth:
//! * [`ELIM_POINTS`] for every elimination,
//! * a point for every [`DAMAGE_PER_POINT`] damage dealt to other players,
//! * up to [`PLACEMENT_POINTS`] for how much of the field the actor outlasted (all of them for first place, none for the
//!   first to fall),
//! * and [`VICTORY_POINTS`] more for a Victory Royale.

use super::hud::{esc, n};
use super::*;
use std::fmt::Write;

pub const ELIM_POINTS: u32 = 25;
pub const DAMAGE_PER_POINT: f32 = 10.0;
pub const PLACEMENT_POINTS: u32 = 50;
pub const VICTORY_POINTS: u32 = 50;

/// What a match is worth, part by part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Points {
    pub elims: u32,
    pub damage: u32,
    pub placement: u32,
    pub victory: u32,
}

impl Points {
    /// `place` is 0 while the actor is still in play (no placement points yet); `total` is how many started the match.
    pub fn of(place: u32, total: u32, kills: u32, damage: f32, won: bool) -> Points {
        let placement = if place == 0 || total < 2 {
            0
        } else {
            let outlasted = (total - place.min(total)) as f32;
            (PLACEMENT_POINTS as f32 * outlasted / (total - 1) as f32).round() as u32
        };
        Points {
            elims: kills * ELIM_POINTS,
            damage: (damage.max(0.0) / DAMAGE_PER_POINT).floor() as u32,
            placement,
            victory: if won { VICTORY_POINTS } else { 0 },
        }
    }

    pub fn total(&self) -> u32 {
        self.elims + self.damage + self.placement + self.victory
    }
}

/// One line of the scoreboard.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: usize,
    pub name: String,
    /// A person played this actor from the start of the match.
    pub human: bool,
    /// A person who left the match: a bot played on in their place, so the result is not theirs.
    pub left: bool,
    pub alive: bool,
    /// The result is final: eliminated, or still standing when the match is decided. (On a player's copy of a multiplayer match
    /// the placement comes from the host's scoreboard and the end of the match from a snapshot, so asking for both keeps a
    /// line from counting before its last numbers have arrived. Explosives can still add to it afterwards: the page keeps
    /// up with that.)
    pub done: bool,
    /// 1 is first; 0 while still in play.
    pub place: u32,
    pub won: bool,
    pub kills: u32,
    pub damage: f32,
    /// Seconds survived (still counting while alive).
    pub time: f32,
    pub headshots: u32,
    pub shots: u32,
    pub hits: u32,
    pub longest: f32,
    pub built: u32,
    pub opened: u32,
    pub points: Points,
}

/// Everyone in the match, best score first (then the better placement, the one still standing, more eliminations).
pub fn rows(g: &Game) -> Vec<Row> {
    let total = g.actors.len() as u32;
    let humans = g.cfg.humans.max(1);
    let mut rows: Vec<Row> = g
        .actors
        .iter()
        .map(|a| {
            let won = g.winner == Some(a.id);
            let place = if won { 1 } else { a.placement };
            Row {
                id: a.id,
                name: a.name.clone(),
                human: a.id < humans,
                left: a.id < humans && !a.human,
                alive: a.alive,
                done: a.placement > 0 && (!a.alive || g.phase == Phase::Over),
                place,
                won,
                kills: a.kills,
                damage: a.damage_dealt,
                time: if a.alive { g.match_time } else { a.survived },
                headshots: a.headshots,
                shots: a.shots,
                hits: a.hits,
                longest: a.longest_elim,
                built: a.built,
                opened: a.opened,
                points: Points::of(place, total, a.kills, a.damage_dealt, won),
            }
        })
        .collect();
    // still in play ranks ahead of every placement it has not been given yet
    let rank = |r: &Row| if r.place == 0 { 0 } else { r.place };
    rows.sort_by(|a, b| b.points.total().cmp(&a.points.total()).then(rank(a).cmp(&rank(b))).then(b.kills.cmp(&a.kills)).then(a.id.cmp(&b.id)));
    rows
}

/// The scoreboard as JSON for the page: the match (`over`, how many started and are left, mode, map, difficulty), the scoring
/// rules, what leaving now would leave the person at this keyboard with (`leave`: null unless they are still in play), and a
/// row per actor (see [`Row`]; `me` marks the person at this keyboard, `pts` is the total and `parts` the eliminations /
/// damage / placement / victory split of it).
pub fn scoreboard_json(g: &Game) -> String {
    let mut s = String::with_capacity(256 + g.actors.len() * 200);
    let mode = match g.cfg.mode {
        GameMode::BattleRoyale => "battle-royale",
        GameMode::ZeroBuild => "zero-build",
        GameMode::Lego => "lego",
    };
    let difficulty = match g.cfg.difficulty {
        Difficulty::Easy => "easy",
        Difficulty::Normal => "normal",
        Difficulty::Hard => "hard",
    };
    let _ = write!(
        s,
        "{{\"over\":{},\"tie\":{},\"total\":{},\"alive\":{},\"mode\":\"{mode}\",\"map\":{},\"difficulty\":\"{difficulty}\",\"scoring\":{{\"elim\":{ELIM_POINTS},\"damagePer\":{},\"placement\":{PLACEMENT_POINTS},\"victory\":{VICTORY_POINTS}}},",
        g.phase == Phase::Over,
        g.tie,
        g.actors.len(),
        g.actors.iter().filter(|a| a.alive).count(),
        esc(g.world.map.key()),
        n(DAMAGE_PER_POINT),
    );
    // walking out of a match counts as being eliminated right there
    let me = &g.actors[g.local];
    if me.alive && me.placement == 0 && g.winner != Some(g.local) {
        let place = g.actors.iter().filter(|a| a.alive).count() as u32;
        let p = Points::of(place, g.actors.len() as u32, me.kills, me.damage_dealt, false);
        let _ = write!(s, "\"leave\":{{\"place\":{place},\"pts\":{},\"parts\":[{},{},{},{}]}},", p.total(), p.elims, p.damage, p.placement, p.victory);
    } else {
        s.push_str("\"leave\":null,");
    }
    s.push_str("\"rows\":[");
    for (i, r) in rows(g).iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let p = &r.points;
        let _ = write!(
            s,
            "{{\"id\":{},\"name\":{},\"human\":{},\"me\":{},\"left\":{},\"alive\":{},\"done\":{},\"place\":{},\"won\":{},\"kills\":{},\"damage\":{},\"time\":{},\"headshots\":{},\"shots\":{},\"hits\":{},\"longest\":{},\"built\":{},\"opened\":{},\"pts\":{},\"parts\":[{},{},{},{}]}}",
            r.id,
            esc(&r.name),
            r.human,
            r.id == g.local,
            r.left,
            r.alive,
            r.done,
            r.place,
            r.won,
            r.kills,
            n(r.damage.round()),
            n(r.time),
            r.headshots,
            r.shots,
            r.hits,
            n(r.longest),
            r.built,
            r.opened,
            p.total(),
            p.elims,
            p.damage,
            p.placement,
            p.victory,
        );
    }
    s.push_str("]}");
    s
}

#[cfg(test)]
mod tests {
    use super::super::testutil::game;
    use super::*;

    #[test]
    fn a_match_is_worth_eliminations_damage_placement_and_the_win() {
        // the first of 40 to fall with nothing to show for it
        assert_eq!(Points::of(40, 40, 0, 0.0, false).total(), 0);
        // still in play: no placement points yet
        assert_eq!(Points::of(0, 40, 2, 155.0, false), Points { elims: 50, damage: 15, placement: 0, victory: 0 });
        // halfway through the field
        let mid = Points::of(21, 41, 1, 99.9, false);
        assert_eq!(mid, Points { elims: 25, damage: 9, placement: 25, victory: 0 });
        // second place outlasted all but one
        assert_eq!(Points::of(2, 40, 0, 0.0, false).placement, 49);
        // a Victory Royale: every placement point and the win on top
        let win = Points::of(1, 40, 6, 812.0, true);
        assert_eq!(win, Points { elims: 150, damage: 81, placement: 50, victory: 50 });
        assert_eq!(win.total(), 331);
        // a draw shares first place without the win
        assert_eq!(Points::of(1, 40, 0, 0.0, false).total(), 50);
        // a match of one (or nonsense) cannot divide by zero
        assert_eq!(Points::of(1, 1, 0, 0.0, true).total(), VICTORY_POINTS);
        assert_eq!(Points::of(9, 4, 0, -5.0, false).total(), 0);
    }

    #[test]
    fn the_board_follows_the_match_and_ranks_by_points() {
        let mut g = game(4, true);
        let rows0 = rows(&g);
        assert_eq!(rows0.len(), 5);
        assert!(rows0.iter().all(|r| r.alive && !r.done && r.place == 0 && r.points.total() == 0));
        assert!(rows0.iter().find(|r| r.id == PLAYER).is_some_and(|r| r.human) && rows0.iter().filter(|r| r.human).count() == 1);
        // bot 1 takes out bot 2 from 30 m and bot 3 from 12 m; the player deals some damage
        g.actors[1].pos = g.actors[2].pos + Vec3::new(30.0, 0.0, 0.0);
        g.actors[3].pos = g.actors[1].pos + Vec3::new(0.0, 0.0, 12.0);
        g.eliminate(2, Some(1), "Test", false);
        g.eliminate(3, Some(1), "Test", false);
        g.actors[PLAYER].damage_dealt = 64.0;
        let r = rows(&g);
        let bot1 = r.iter().find(|x| x.id == 1).unwrap();
        assert_eq!((bot1.kills, bot1.points.elims), (2, 50));
        assert!((bot1.longest - 30.0).abs() < 0.5, "longest elimination {}", bot1.longest);
        assert_eq!(r[0].id, 1, "most points first");
        let fallen: Vec<&Row> = r.iter().filter(|x| x.done).collect();
        assert_eq!(fallen.len(), 2);
        assert!(fallen.iter().all(|x| !x.alive && (x.place == 4 || x.place == 5)));
        assert_eq!(r.iter().find(|x| x.place == 4).unwrap().points.placement, 13, "4th of 5 outlasted one of four others");
        // the rest fall and the player wins
        g.eliminate(4, Some(PLAYER), "Test", false);
        g.eliminate(1, Some(PLAYER), "Test", false);
        g.update(1.0 / 60.0, &PlayerInput::default());
        assert_eq!(g.winner, Some(PLAYER));
        let r = rows(&g);
        assert!(r.iter().all(|x| x.done), "everyone's result is final once the match is decided");
        let me = r.iter().find(|x| x.id == PLAYER).unwrap();
        assert!(me.won && me.place == 1 && me.alive);
        assert_eq!(me.points, Points { elims: 50, damage: 6, placement: 50, victory: 50 });
        assert_eq!(r[0].id, PLAYER);
    }

    #[test]
    fn a_line_is_final_only_once_its_actor_is_out_or_the_match_is_decided() {
        let mut g = game(2, true);
        // a player's copy of a multiplayer match can hear the winner's placement before it hears that the match is over
        g.actors[1].placement = 1;
        let line = |g: &Game| rows(g).into_iter().find(|r| r.id == 1).unwrap();
        assert!(!line(&g).done, "not final while the match goes on for all this copy knows");
        g.phase = Phase::Over;
        g.winner = Some(1);
        let r = line(&g);
        assert!(r.done && r.won && r.points.victory == VICTORY_POINTS, "final as a win, with the win's points: {r:?}");
    }

    #[test]
    fn someone_who_left_is_marked_so_the_bot_that_took_over_is_not_credited_to_them() {
        let mut g = super::super::testutil::game_cfg(GameConfig { bots: 2, humans: 2, human_names: vec!["Ada".into()], skip_bus: true, seed: 7, ..Default::default() });
        let ada = rows(&g).into_iter().find(|r| r.id == 1).unwrap();
        assert!(ada.human && !ada.left && ada.name == "Ada");
        g.actors[1].human = false; // what the host does when Ada's connection goes
        let ada = rows(&g).into_iter().find(|r| r.id == 1).unwrap();
        assert!(ada.human && ada.left);
    }

    #[test]
    fn the_json_has_the_match_the_rules_and_a_row_per_actor() {
        let mut g = game(3, true);
        g.actors[2].name = "Q\"uote".into();
        g.eliminate(3, Some(2), "Test", false);
        let j = scoreboard_json(&g);
        assert_eq!(j.matches('{').count(), j.matches('}').count(), "{j}");
        assert_eq!(j.matches('[').count(), j.matches(']').count());
        assert!(!j.contains("NaN") && !j.contains("inf"));
        assert!(j.starts_with("{\"over\":false,\"tie\":false,\"total\":4,\"alive\":3,\"mode\":\"battle-royale\""), "{j}");
        assert!(j.contains("\"scoring\":{\"elim\":25,\"damagePer\":10,\"placement\":50,\"victory\":50}"), "{j}");
        assert_eq!(j.matches("\"id\":").count(), 4);
        assert!(j.contains("\"name\":\"Q\\\"uote\""), "names are escaped: {j}");
        assert!(j.contains("\"me\":true") && j.matches("\"me\":true").count() == 1);
        assert!(j.contains("\"place\":4,\"won\":false,\"kills\":0"), "{j}");
        assert!(j.contains("\"kills\":1,"), "{j}");
        // the player is still in: leaving now would place them 3rd of the 4
        assert!(j.contains("\"leave\":{\"place\":3,\"pts\":17,\"parts\":[0,0,17,0]},\"rows\":["), "{j}");
        g.eliminate(PLAYER, Some(1), "Test", false);
        assert!(scoreboard_json(&g).contains("\"leave\":null,\"rows\":["));
    }
}
