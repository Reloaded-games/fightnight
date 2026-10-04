//! A compact JSON snapshot of everything the HUD needs, built once per frame for the
//! platform layer (which draws it with Canvas2D / DOM).

use super::actor::*;
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
        let _ = write!(s, ",\"vehicle\":{{\"id\":{},\"name\":\"Island Buggy\",\"speed\":{},\"boost\":{}}}", v.id, n(v.speed.abs() * 3.6), v.speed.abs() > super::vehicles::TOP_SPEED + 0.5);
    } else { s.push_str(",\"vehicle\":null"); }
    s.push_str(",\"vehicles\":[");
    for (i, v) in g.vehicles.iter().enumerate() {
        if i > 0 { s.push(','); }
        let _ = write!(s, "{{\"id\":{},\"x\":{},\"z\":{},\"occupied\":{}}}", v.id, n(v.pos.x), n(v.pos.z), v.driver.is_some());
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
        if let Some(from) = g.last_hurt_dir {
            let d = Vec2::new(from.x - me.pos.x, from.z - me.pos.z);
            if d.length() > 0.5 {
                // angle of the attacker relative to where the camera looks (0 = ahead, + = to the right)
                let f = Vec2::new(cam.fwd.x, cam.fwd.z).normalize_or_zero();
                let r = Vec2::new(cam.right.x, cam.right.z).normalize_or_zero();
                let dn = d.normalize();
                let ang = dn.dot(r).atan2(dn.dot(f));
                let _ = write!(s, "{{\"a\":{},\"t\":{}}}", n(ang), n(g.hurt_flash.min(1.0)));
            }
        }
    }
    s.push(']');
    // interaction prompt
    if g.vehicle_for_actor(me_id).is_some() {
        s.push_str(",\"prompt\":{\"txt\":\"Exit buggy\",\"rar\":1,\"cnt\":1,\"kind\":\"vehicle\"}");
    } else if g.interact_target.is_none() && g.nearby_vehicle(me_id).is_some() {
        s.push_str(",\"prompt\":{\"txt\":\"Drive Island Buggy\",\"rar\":1,\"cnt\":1,\"kind\":\"vehicle\"}");
    } else { match g.interact_target {
        Some(Target::Pickup(id)) => {
            if let Some(i) = g.pickup_index(id) {
                let p = &g.pickups[i];
                let _ = write!(s, ",\"prompt\":{{\"txt\":{},\"rar\":{},\"cnt\":{},\"kind\":\"{}\"}}", esc(p.kind.name()), p.kind.rarity().index(), p.kind.count(), match p.kind {
                    PickupKind::Weapon { .. } => "w",
                    PickupKind::Ammo { .. } => "a",
                    PickupKind::Consumable { .. } => "c",
                });
            } else {
                s.push_str(",\"prompt\":null");
            }
        }
        Some(Target::Chest(_)) => s.push_str(",\"prompt\":{\"txt\":\"Chest\",\"rar\":4,\"cnt\":1,\"kind\":\"chest\"}"),
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
    fn names_with_quotes_are_escaped() {
        assert_eq!(esc("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
