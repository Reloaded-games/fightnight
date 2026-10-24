//! A compact JSON snapshot of everything the HUD needs, built once per frame for the
//! platform layer (which draws it with Canvas2D / DOM).

use super::actor::*;
use super::audio_map::{SoundKind, SoundMarker};
use super::items::*;
use super::loot::Target;
use super::*;
use crate::camera::Camera;
use crate::math::*;
use std::fmt::Write;

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => {}
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn n(v: f32) -> String {
    if v.is_finite() {
        let r = (v * 100.0).round() / 100.0;
        if r == r.trunc() {
            format!("{}", r as i64)
        } else {
            format!("{r}")
        }
    } else {
        "0".into()
    }
}

fn mode_name(m: MoveMode) -> &'static str {
    match m {
        MoveMode::Bus => "bus",
        MoveMode::Freefall => "freefall",
        MoveMode::Glide => "glide",
        MoveMode::Ground => "ground",
        MoveMode::Swim => "swim",
        MoveMode::Dead => "dead",
    }
}

fn kind_index(k: WeaponKind) -> usize {
    WeaponKind::ALL.iter().position(|x| *x == k).unwrap_or(0)
}

fn cons_index(k: ConsumableKind) -> usize {
    ConsumableKind::ALL.iter().position(|x| *x == k).unwrap_or(0)
}

/// Which way `to` lies from `from` on the ground, relative to where the camera looks: 0 ahead, + to the right, ±π behind.
fn bearing(cam: &Camera, from: Vec3, to: Vec3) -> Option<f32> {
    let d = Vec2::new(to.x - from.x, to.z - from.z);
    if d.length() <= 0.5 {
        return None;
    }
    let f = Vec2::new(cam.fwd.x, cam.fwd.z).normalize_or_zero();
    let r = Vec2::new(cam.right.x, cam.right.z).normalize_or_zero();
    let dn = d.normalize();
    Some(dn.dot(r).atan2(dn.dot(f)))
}

/// The footsteps and shots the player heard lately, as the `"snd"` field of the HUD snapshot (with its leading comma):
/// kind (0 steps, 1 shot), bearing, how close it was (0..1) and how much of its time on screen is left (1 fresh, 0 gone).
pub fn sounds_json(g: &Game, cam: &Camera, markers: &[SoundMarker]) -> String {
    let me = &g.actors[g.camera_actor()];
    let mut s = String::from(",\"snd\":[");
    let mut first = true;
    for m in markers {
        let Some(ang) = bearing(cam, me.pos, m.pos) else { continue };
        let left = (1.0 - (g.time - m.at) / m.kind.life()).clamp(0.0, 1.0);
        let kind = match m.kind {
            SoundKind::Steps => 0,
            SoundKind::Shot => 1,
        };
        if !first {
            s.push(',');
        }
        first = false;
        let _ = write!(s, "{{\"k\":{kind},\"a\":{},\"l\":{},\"t\":{}}}", n(ang), n(m.level), n(left));
    }
    s.push(']');
    s
}

/// Build the HUD snapshot. `show_tags` adds name plates over nearby players (debug / spectating aid).
pub fn hud_json(g: &Game, cam: &Camera, show_tags: bool) -> String {
    let mut s = String::with_capacity(4096);
    let me_id = g.camera_actor();
    let me = &g.actors[me_id];
    let player = &g.actors[g.local];
    let phase = match g.phase {
        Phase::Bus => 0,
        Phase::Playing => 1,
        Phase::Over => 2,
    };
    let _ = write!(s, "{{\"ph\":{phase},\"t\":{},\"alive\":{},\"total\":{},\"kills\":{},\"hp\":{},\"sh\":{},\"mode\":\"{}\",\"dead\":{},\"ads\":{},\"aim\":{}", n(g.match_time), g.alive_cache, g.actors.len(), me.kills, n(me.hp.max(0.0)), n(me.shield.max(0.0)), mode_name(me.mode), !player.alive, me.ads, n(me.anim.aim));
    if me_id != g.local {
        let _ = write!(s, ",\"spec\":{}", esc(&me.name));
    }
    let agl = me.pos.y - g.world.hm.height_at(me.pos.x, me.pos.z);
    let _ = write!(s, ",\"pos\":[{},{},{}],\"yaw\":{},\"pitch\":{},\"agl\":{}", n(me.pos.x), n(me.pos.y), n(me.pos.z), n(me.yaw), n(me.pitch), n(agl));
    let mode_key = match g.cfg.mode { GameMode::BattleRoyale => "battle-royale", GameMode::ZeroBuild => "zero-build", GameMode::Lego => "lego" };
    let _ = write!(s, ",\"gameMode\":{},\"worldSize\":{}", esc(mode_key), n(crate::world::WORLD_SIZE));
    if let Some(v) = g.vehicle_for_actor(me_id) {
        let _ = write!(s, ",\"vehicle\":{{\"id\":{},\"name\":{},\"speed\":{},\"boost\":{}}}", v.id, esc(v.display_name()), n(v.speed.abs() * 3.6), v.speed.abs() > super::vehicles::TOP_SPEED + 0.5);
    } else { s.push_str(",\"vehicle\":null"); }
    s.push_str(",\"vehicles\":[");
    for (i, v) in g.vehicles.iter().enumerate() {
        if i > 0 { s.push(','); }
        let _ = write!(s, "{{\"id\":{},\"x\":{},\"z\":{},\"occupied\":{}}}", v.id, n(v.pos.x), n(v.pos.z), v.driver.is_some());
    }
    s.push(']');
    // supply drops that nobody has opened: where they are (coming down or waiting)
    s.push_str(",\"drops\":[");
    for (i, c) in g.chests.iter().filter(|c| c.is_supply() && !c.opened).enumerate() {
        if i > 0 { s.push(','); }
        let _ = write!(s, "{{\"x\":{},\"z\":{},\"air\":{}}}", n(c.pos.x), n(c.pos.z), c.falling());
    }
    s.push(']');

    // ---- inventory ------------------------------------------------------------------------------
    s.push_str(",\"slots\":[");
    for i in 0..6 {
        if i > 0 {
            s.push(',');
        }
        match me.inv.slots[i] {
            None => s.push_str("null"),
            Some(Item::Pickaxe) => s.push_str("{\"t\":\"p\",\"n\":\"Harvesting Tool\"}"),
            Some(Item::Weapon { kind, rarity, ammo }) => {
                let d = kind.def();
                let _ = write!(s, "{{\"t\":\"w\",\"k\":{},\"n\":{},\"r\":{},\"a\":{},\"mag\":{},\"res\":{},\"ak\":{}}}", kind_index(kind), esc(d.name), rarity.index(), ammo, d.mag, me.inv.ammo[d.ammo.index()], d.ammo.index());
            }
            Some(Item::Consumable { kind, count }) => {
                let _ = write!(s, "{{\"t\":\"c\",\"k\":{},\"n\":{},\"r\":{},\"c\":{}}}", cons_index(kind), esc(kind.name()), kind.rarity().index(), count);
            }
            Some(Item::Utility { kind, count }) => {
                let k = UtilityKind::ALL.iter().position(|x| *x == kind).unwrap_or(0);
                let _ = write!(s, "{{\"t\":\"u\",\"k\":{},\"n\":{},\"r\":{},\"c\":{},\"throw\":{}}}", k, esc(kind.name()), kind.rarity().index(), count, kind.def().thrown);
            }
        }
    }
    let _ = write!(s, "],\"sel\":{},\"ammo\":[{},{},{},{},{}],\"mats\":[{},{},{}]", me.inv.selected, me.inv.ammo[0], me.inv.ammo[1], me.inv.ammo[2], me.inv.ammo[3], me.inv.ammo[4], me.inv.mats[0], me.inv.mats[1], me.inv.mats[2]);
    if let Some((kind, rarity, ammo)) = me.inv.selected_weapon() {
        let d = kind.def();
        let (rl, swap) = match me.action {
            Action::Reload { t, dur } => ((t / dur).clamp(0.0, 1.0), 0.0),
            Action::Swap { t } => (0.0, (t / d.equip_time.max(0.01)).clamp(0.0, 1.0)),
            _ => (0.0, 0.0),
        };
        let _ = write!(s, ",\"wep\":{{\"k\":{},\"n\":{},\"r\":{},\"mag\":{},\"magMax\":{},\"res\":{},\"reload\":{},\"swap\":{},\"scope\":{},\"auto\":{}}}", kind_index(kind), esc(d.name), rarity.index(), ammo, d.mag, me.inv.ammo[d.ammo.index()], n(rl), n(swap), d.scope, d.auto);
    } else {
        s.push_str(",\"wep\":null");
    }
    let heal = match me.action {
        Action::Heal { t, dur, .. } => (t / dur).clamp(0.0, 1.0),
        _ => -1.0,
    };
    let _ = write!(s, ",\"heal\":{}", n(heal));
    // a Slurp Juice still flowing, a Boogie Bomb still making the player dance
    let _ = write!(s, ",\"regen\":{},\"boogie\":{}", me.regen > 0.0, n(me.boogie));
    let _ = write!(s, ",\"build\":{{\"on\":{},\"piece\":{},\"mat\":{},\"cost\":{}}}", me.build_mode, me.build_piece as usize, me.build_mat.index(), me.build_piece.cost());

    // ---- bus, storm ------------------------------------------------------------------------------------
    let _ = write!(s, ",\"bus\":{{\"on\":{},\"x\":{},\"z\":{},\"dx\":{},\"dz\":{},\"left\":{}}}", g.bus.active, n(g.bus.pos.x), n(g.bus.pos.z), n(g.bus.dir.x), n(g.bus.dir.y), n((g.bus.total - g.bus.t).max(0.0)));
    let st = &g.storm;
    let state = match st.state {
        StormState::Waiting => 0,
        StormState::Shrinking => 1,
        StormState::Done => 2,
    };
    let inside = !super::matchflow::in_storm(g, me.pos);
    let _ = write!(
        s,
        ",\"storm\":{{\"on\":{},\"state\":{state},\"timer\":{},\"dur\":{},\"phase\":{},\"cx\":{},\"cz\":{},\"r\":{},\"ncx\":{},\"ncz\":{},\"nr\":{},\"dps\":{},\"in\":{inside}}}",
        st.active,
        n(st.timer.max(0.0)),
        n(st.duration),
        st.phase,
        n(st.center.x),
        n(st.center.y),
        n(st.radius),
        n(st.to_center.x),
        n(st.to_center.y),
        n(st.to_radius),
        n(st.dmg)
    );

    // ---- feed and toasts ------------------------------------------------------------------------------------
    s.push_str(",\"feed\":[");
    for (i, f) in g.feed.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let k = f.killer.as_deref().map(esc).unwrap_or_else(|| "null".into());
        let _ = write!(s, "{{\"k\":{k},\"v\":{},\"w\":{},\"me\":{},\"you\":{},\"s\":{},\"age\":{}}}", esc(&f.victim), esc(&f.weapon), f.by_player, f.victim_is_player, f.storm, n(f.age));
    }
    s.push_str("],\"toast\":[");
    for (i, t) in g.toast.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "[{},{},{}]", esc(&t.0), n(t.1), t.2);
    }
    s.push(']');

    // ---- feedback ------------------------------------------------------------------------------------------------
    let _ = write!(s, ",\"hit\":{{\"t\":{},\"k\":{}}},\"hurt\":{}", n(g.hit_marker), g.hit_marker_kind, n(g.hurt_flash));
    s.push_str(",\"ind\":[");
    if g.hurt_flash > 0.04 {
        if let Some(ang) = g.last_hurt_dir.and_then(|from| bearing(cam, me.pos, from)) {
            let _ = write!(s, "{{\"a\":{},\"t\":{}}}", n(ang), n(g.hurt_flash.min(1.0)));
        }
    }
    s.push(']');
    // interaction prompt
    if let Some(v) = g.vehicle_for_actor(me_id) {
        let kind = v.display_name().strip_prefix("Island ").unwrap_or(v.display_name()).to_ascii_lowercase();
        let _ = write!(s, ",\"prompt\":{{\"txt\":{},\"rar\":1,\"cnt\":1,\"kind\":\"vehicle\"}}", esc(&format!("Exit {kind}")));
    } else if let Some(v) = g.nearby_vehicle(me_id).filter(|_| g.interact_target.is_none()) {
        let _ = write!(s, ",\"prompt\":{{\"txt\":{},\"rar\":1,\"cnt\":1,\"kind\":\"vehicle\"}}", esc(&format!("Drive {}", v.display_name())));
    } else { match g.interact_target {
        Some(Target::Pickup(id)) => {
            if let Some(i) = g.pickup_index(id) {
                let p = &g.pickups[i];
                let _ = write!(s, ",\"prompt\":{{\"txt\":{},\"rar\":{},\"cnt\":{},\"kind\":\"{}\"}}", esc(p.kind.name()), p.kind.rarity().index(), p.kind.count(), match p.kind {
                    PickupKind::Weapon { .. } => "w",
                    PickupKind::Ammo { .. } => "a",
                    PickupKind::Consumable { .. } => "c",
                    PickupKind::Utility { .. } => "u",
                    PickupKind::Mats { .. } => "m",
                });
            } else {
                s.push_str(",\"prompt\":null");
            }
        }
        Some(Target::Chest(id)) => {
            let (txt, rar, kind) = match g.chest_index(id).map(|i| g.chests[i].kind) {
                Some(ChestKind::Supply) => ("Supply Drop", 2, "supply"),
                Some(ChestKind::AmmoBox) => ("Ammo Box", 0, "ammobox"),
                Some(ChestKind::Llama) => ("Supply Llama", 3, "llama"),
                _ => ("Chest", 4, "chest"),
            };
            let _ = write!(s, ",\"prompt\":{{\"txt\":\"{txt}\",\"rar\":{rar},\"cnt\":1,\"kind\":\"{kind}\"}}");
        }
        None => s.push_str(",\"prompt\":null"),
    } }

    // ---- screen-space markers ---------------------------------------------------------------------------------------------
    s.push_str(",\"dmg\":[");
    for (i, d) in g.fx.numbers.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let rise = d.age * 1.4;
        let p = d.pos + Vec3::new(0.0, rise, 0.0) + yaw_right(me.yaw) * (d.drift * d.age);
        match cam.project(p) {
            Some(c) => {
                let _ = write!(s, "{{\"x\":{},\"y\":{},\"a\":{},\"k\":{},\"age\":{}}}", n(c.x), n(c.y), n(d.amount.round()), d.kind, n(d.age));
            }
            None => s.push_str("{\"x\":9,\"y\":9,\"a\":0,\"k\":0,\"age\":9}"),
        }
    }
    s.push_str("],\"tags\":[");
    if show_tags {
        let mut first = true;
        for a in &g.actors {
            if a.id == me_id || !a.alive || matches!(a.mode, MoveMode::Bus) {
                continue;
            }
            let d = a.pos.distance(me.pos);
            if d > 80.0 {
                continue;
            }
            if let Some(c) = cam.project(a.pos + Vec3::Y * 2.1) {
                if c.x.abs() < 1.1 && c.y.abs() < 1.1 {
                    if !first {
                        s.push(',');
                    }
                    first = false;
                    let _ = write!(s, "{{\"x\":{},\"y\":{},\"n\":{},\"hp\":{},\"sh\":{},\"d\":{}}}", n(c.x), n(c.y), esc(&a.name), n(a.hp / 100.0), n(a.shield / 100.0), n(d));
                }
            }
        }
    }
    s.push(']');

    // ---- end of match ---------------------------------------------------------------------------------------------------------
    let _ = write!(
        s,
        ",\"stats\":{{\"place\":{},\"kills\":{},\"dmg\":{},\"time\":{}}}",
        if g.phase == Phase::Over && g.winner == Some(g.local) { 1 } else { player.placement.max(1) },
        player.kills,
        n(player.damage_dealt),
        // the clock keeps running while a fallen player spectates, but their own time stopped when they fell
        n(if player.alive { g.match_time } else { player.survived })
    );
    if g.tie {
        s.push_str(",\"tie\":true");
    }
    if let Some(w) = g.winner {
        let _ = write!(s, ",\"winner\":{},\"won\":{}", esc(&g.actors[w].name), w == g.local);
    }
    let _ = write!(s, ",\"deadT\":{}", n(player.dead_time));
    if !player.alive {
        match player.last_damage_from {
            Some(k) => {
                let _ = write!(s, ",\"killer\":{}", esc(&g.actors[k].name));
            }
            None => s.push_str(",\"killer\":null"),
        }
    }
    s.push('}');
    s
}

#[cfg(test)]
mod tests {
    use super::super::testutil::game;
    use super::*;

    fn cam(g: &Game) -> Camera {
        g.camera(1.6)
    }

    #[test]
    fn supply_drops_show_on_the_map_and_have_their_own_prompt() {
        let mut g = game(2, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let c = cam(&g);
        assert!(hud_json(&g, &c, false).contains("\"drops\":[]"), "no crates, no markers");
        let p = g.actors[PLAYER].pos;
        let id = supply::launch(&mut g, Vec2::new(p.x + 30.0, p.z));
        let json = hud_json(&g, &c, false);
        assert!(json.contains("\"drops\":[{") && json.contains("\"air\":true"), "a crate in the air: {}", &json[json.find("\"drops\"").unwrap()..][..60]);
        // once it is down it is a marker to walk to, and standing beside it asks to open it
        let i = g.chest_index(id).unwrap();
        g.chests[i].pos.y = g.chests[i].land_y;
        g.actors[PLAYER].pos = Vec3::new(g.chests[i].pos.x, g.chests[i].land_y, g.chests[i].pos.z + 1.5);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let json = hud_json(&g, &c, false);
        assert!(json.contains("\"air\":false"));
        assert!(json.contains("\"kind\":\"supply\"") && json.contains("Supply Drop"), "the prompt: {}", &json[json.find("\"prompt\"").unwrap()..][..80]);
        // an opened crate is no longer worth walking to
        g.open_chest(i, PLAYER);
        assert!(hud_json(&g, &c, false).contains("\"drops\":[]"));
    }

    #[test]
    fn hud_json_is_valid_looking_and_complete() {
        let mut g = game(8, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let c = cam(&g);
        let j = hud_json(&g, &c, true);
        // balanced braces/brackets and no NaN
        let open = j.matches('{').count();
        let close = j.matches('}').count();
        assert_eq!(open, close, "unbalanced braces in {j}");
        assert_eq!(j.matches('[').count(), j.matches(']').count());
        assert!(!j.contains("NaN") && !j.contains("inf"), "{j}");
        for key in ["\"hp\":100", "\"slots\":[", "\"storm\":{", "\"feed\":[", "\"prompt\":", "\"stats\":{", "\"mats\":["] {
            assert!(j.contains(key), "missing {key} in {j}");
        }
        assert!(j.starts_with('{') && j.ends_with('}'));
    }

    #[test]
    fn weapon_and_prompt_fields_appear() {
        let mut g = game(2, true);
        g.actors[PLAYER].inv.add_weapon(WeaponKind::Shotgun, Rarity::Epic, 3);
        g.select_slot(PLAYER, 1);
        let spot = g.actors[PLAYER].pos;
        g.spawn_pickup(spot, PickupKind::Weapon { kind: WeaponKind::Sniper, rarity: Rarity::Legendary, ammo: 1 }, false);
        for _ in 0..3 {
            g.update(1.0 / 60.0, &PlayerInput::default());
        }
        let j = hud_json(&g, &cam(&g), false);
        assert!(j.contains("\"n\":\"Pump Shotgun\""), "{j}");
        assert!(j.contains("\"r\":3"), "epic rarity index");
        assert!(j.contains("\"txt\":\"Bolt-Action Sniper\""), "prompt for the pickup under the player: {j}");
    }

    #[test]
    fn utility_items_a_slurp_at_work_and_a_dance_show_on_the_hud() {
        let mut g = game(2, true);
        g.actors[PLAYER].inv.add_utility(UtilityKind::FragGrenade, 4);
        g.actors[PLAYER].inv.add_utility(UtilityKind::LaunchPad, 1);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let j = hud_json(&g, &cam(&g), false);
        assert!(j.contains("{\"t\":\"u\",\"k\":0,\"n\":\"Frag Grenade\",\"r\":1,\"c\":4,\"throw\":true}"), "{j}");
        assert!(j.contains("{\"t\":\"u\",\"k\":3,\"n\":\"Launch Pad\",\"r\":2,\"c\":1,\"throw\":false}"), "{j}");
        assert!(j.contains("\"regen\":false,\"boogie\":0"), "{j}");
        g.actors[PLAYER].regen = 40.0;
        g.actors[PLAYER].boogie = 3.25;
        let j = hud_json(&g, &cam(&g), false);
        assert!(j.contains("\"regen\":true,\"boogie\":3.25"), "{j}");
        // the containers each have their own prompt
        let p = g.actors[PLAYER].pos;
        g.chests.clear();
        for (kind, word) in [(ChestKind::AmmoBox, "\"kind\":\"ammobox\""), (ChestKind::Llama, "\"kind\":\"llama\"")] {
            g.chests.clear();
            let id = g.new_id();
            let f = yaw_forward(g.actors[PLAYER].yaw);
            g.chests.push(Chest::of_kind(id, p + Vec3::new(f.x, 0.0, f.z) * 1.2, 0.0, kind));
            g.update(1.0 / 60.0, &PlayerInput::default());
            let j = hud_json(&g, &cam(&g), false);
            assert!(j.contains(word), "{kind:?}: {}", &j[j.find("\"prompt\"").unwrap()..][..80]);
        }
    }

    #[test]
    fn heard_sounds_point_the_way_they_came_from() {
        let mut g = game(2, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let p = g.actors[PLAYER].pos;
        let c = Camera::look(p + Vec3::Y * 1.6, Vec3::NEG_Z, 0.0, 1.0, 1.6, 0.1);
        let marker = |kind, actor, off: Vec3, at| SoundMarker { kind, actor, pos: p + off, level: 0.5, at };
        let markers = [
            marker(SoundKind::Steps, 1, Vec3::new(10.0, 0.0, 0.0), g.time),
            marker(SoundKind::Shot, 2, Vec3::new(0.0, 3.0, -80.0), g.time - 1.0),
            marker(SoundKind::Steps, 2, Vec3::new(-0.1, 0.0, 6.0), g.time - 0.6),
        ];
        let json = sounds_json(&g, &c, &markers);
        assert_eq!(json, ",\"snd\":[{\"k\":0,\"a\":1.57,\"l\":0.5,\"t\":1},{\"k\":1,\"a\":0,\"l\":0.5,\"t\":0.5},{\"k\":0,\"a\":-3.12,\"l\":0.5,\"t\":0.5}]");
        // turning to face the walker brings them ahead
        let c = Camera::look(p + Vec3::Y * 1.6, Vec3::X, 0.0, 1.0, 1.6, 0.1);
        assert!(sounds_json(&g, &c, &markers[..1]).contains("\"a\":0,"), "{}", sounds_json(&g, &c, &markers[..1]));
        assert_eq!(sounds_json(&g, &c, &[]), ",\"snd\":[]");
    }

    #[test]
    fn names_with_quotes_are_escaped() {
        assert_eq!(esc("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
