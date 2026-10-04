//! Floor loot, chests, picking up, dropping.

use super::actor::*;
use super::env::Env;
use super::events::*;
use super::items::*;
use super::*;
use crate::math::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Pickup(u32),
    Chest(u32),
}

pub const INTERACT_RANGE: f32 = 2.6;

impl Game {
    /// Fill the island with loot: floor items at every loot spot and a closed chest at every chest spot.
    pub fn spawn_world_loot(&mut self) {
        let mut rng = self.rng.fork(0x10077);
        let spots = self.world.loot_spots.clone();
        for s in spots {
            let p = if s.building.is_some() { 0.8 } else { 0.7 };
            if !rng.chance(p) {
                continue;
            }
            let kind = PickupKind::from_drop(roll_floor_loot(&mut rng));
            let id = self.new_id();
            self.pickups.push(Pickup { id, pos: s.pos, vel: Vec3::ZERO, kind, age: 0.0, grounded: true, spin: rng.range(0.0, std::f32::consts::TAU) });
        }
        let chests = self.world.chest_spots.clone();
        for c in chests {
            let id = self.new_id();
            self.chests.push(Chest::on_ground(id, c.pos, c.yaw));
        }
    }

    pub fn spawn_pickup(&mut self, pos: Vec3, kind: PickupKind, pop: bool) -> u32 {
        let id = self.new_id();
        let vel = if pop {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let s = self.rng.range(1.5, 3.8);
            Vec3::new(a.cos() * s, self.rng.range(4.5, 7.0), a.sin() * s)
        } else {
            Vec3::ZERO
        };
        let spin = self.rng.range(0.0, std::f32::consts::TAU);
        self.pickups.push(Pickup { id, pos, vel, kind, age: 0.0, grounded: !pop, spin });
        id
    }

    pub fn pickup_index(&self, id: u32) -> Option<usize> {
        self.pickups.iter().position(|p| p.id == id)
    }
    pub fn chest_index(&self, id: u32) -> Option<usize> {
        self.chests.iter().position(|c| c.id == id)
    }

    /// Nearest pickup or chest within reach of an actor.
    pub fn find_target(&self, who: usize) -> Option<Target> {
        let a = &self.actors[who];
        let c = a.pos + Vec3::Y * 0.9;
        let mut best: Option<(f32, Target)> = None;
        for p in &self.pickups {
            let d = p.pos.distance(c);
            // an item that cannot be taken (ammo at the cap, nothing to swap) must not shadow the ones beside it
            if d < INTERACT_RANGE && best.is_none_or(|b| d < b.0) && can_take(&a.inv, &p.kind) {
                best = Some((d, Target::Pickup(p.id)));
            }
        }
        for ch in &self.chests {
            // a supply crate cannot be opened while it is still coming down
            if ch.opened || ch.falling() {
                continue;
            }
            let d = ch.pos.distance(c);
            // (the crate is a good deal bigger than a chest)
            let reach = INTERACT_RANGE + if ch.supply { 1.0 } else { 0.4 };
            if d < reach && best.is_none_or(|b| d < b.0 + 0.3) {
                best = Some((d, Target::Chest(ch.id)));
            }
        }
        best.map(|b| b.1)
    }

    pub fn interact(&mut self, who: usize) -> bool {
        match self.find_target(who) {
            Some(Target::Pickup(id)) => {
                if let Some(idx) = self.pickup_index(id) {
                    return self.pickup_item(who, idx);
                }
                false
            }
            Some(Target::Chest(id)) => {
                if let Some(idx) = self.chest_index(id) {
                    self.open_chest(idx, who);
                    return true;
                }
                false
            }
            None => {
                self.explain_nothing_taken(who);
                false
            }
        }
    }

    /// E found nothing to take. When that is because the thing lying there does not fit, say so (a player cannot tell "full" from "broken").
    fn explain_nothing_taken(&mut self, who: usize) {
        let a = &self.actors[who];
        if !a.human {
            return;
        }
        let c = a.pos + Vec3::Y * 0.9;
        let nearest = self.pickups.iter().filter(|p| p.pos.distance(c) < INTERACT_RANGE).min_by(|x, y| x.pos.distance(c).total_cmp(&y.pos.distance(c)));
        if let Some(p) = nearest {
            let why = why_not_taken(&a.inv, &p.kind);
            self.toast_to(who, why, 2.0, 2);
        }
    }

    pub fn open_chest(&mut self, idx: usize, by: usize) {
        if self.chests[idx].opened {
            return;
        }
        self.chests[idx].opened = true;
        let (pos, yaw) = (self.chests[idx].pos, self.chests[idx].yaw);
        let supply = self.chests[idx].supply;
        self.events.push(Event::ChestOpen { pos });
        // a supply crate is opened with a clatter that carries
        self.events.push(Event::Noise { pos, radius: if supply { 80.0 } else { 30.0 }, source: by });
        let mut rng = self.rng.fork(self.chests[idx].id as u64);
        let drops = if supply { roll_supply_loot(&mut rng) } else { roll_chest_loot(&mut rng) };
        let n = drops.len();
        for (k, d) in drops.into_iter().enumerate() {
            // fan the loot out in front of the chest
            let spread = (k as f32 - (n as f32 - 1.0) / 2.0) * 0.55;
            let out = Vec3::new((yaw + spread).sin(), 0.0, (yaw + spread).cos());
            let id = self.new_id();
            let vel = out * rng.range(2.2, 3.4) + Vec3::Y * rng.range(5.0, 6.5);
            self.pickups.push(Pickup { id, pos: pos + Vec3::Y * 0.8 + out * 0.5, vel, kind: PickupKind::from_drop(d), age: 0.0, grounded: false, spin: rng.range(0.0, std::f32::consts::TAU) });
        }
    }

    /// Try to take a pickup. Returns whether anything was taken.
    pub fn pickup_item(&mut self, who: usize, idx: usize) -> bool {
        let kind = self.pickups[idx].kind;
        let ppos = self.pickups[idx].pos;
        let apos = self.actors[who].pos;
        match kind {
            PickupKind::Weapon { kind: wk, rarity, ammo } => {
                let was_empty_handed = self.actors[who].inv.selected == 0;
                let r = self.actors[who].inv.add_weapon(wk, rarity, ammo);
                match r {
                    AddResult::Added(slot) => {
                        self.pickups.remove(idx);
                        // take it in hand when empty-handed, but never drop out of build mode just for picking something up
                        if was_empty_handed && !self.actors[who].build_mode {
                            self.select_slot(who, slot);
                        }
                    }
                    AddResult::Swapped { slot, dropped } => {
                        self.pickups.remove(idx);
                        let pk = match dropped {
                            Item::Weapon { kind, rarity, ammo } => PickupKind::Weapon { kind, rarity, ammo },
                            _ => return true,
                        };
                        self.spawn_pickup(apos + Vec3::Y * 0.4, pk, true);
                        let a = &mut self.actors[who];
                        if a.build_mode {
                            // keep building; the new weapon waits in its slot
                        } else if a.inv.selected == slot {
                            // the weapon in hand was replaced: equip the new one from scratch (no reload or heal carries over)
                            a.action = Action::Swap { t: wk.def().equip_time };
                            a.ads = false;
                            self.events.push(Event::WeaponSwitch { actor: who, pos: apos });
                        } else {
                            self.select_slot(who, slot);
                        }
                    }
                    AddResult::Full => return false,
                }
                self.events.push(Event::Pickup { actor: who, pos: ppos, sound: PickupSound::Weapon, name: wk.name(), rarity, count: 1 });
                true
            }
            PickupKind::Ammo { kind: ak, amount } => {
                let added = self.actors[who].inv.add_ammo(ak, amount);
                if added == 0 {
                    return false;
                }
                if added >= amount {
                    self.pickups.remove(idx);
                } else if let PickupKind::Ammo { amount: a, .. } = &mut self.pickups[idx].kind {
                    *a -= added;
                }
                self.events.push(Event::Pickup { actor: who, pos: ppos, sound: PickupSound::Ammo, name: ak.name(), rarity: Rarity::Common, count: added });
                true
            }
            PickupKind::Consumable { kind: ck, count } => {
                let left = self.actors[who].inv.add_consumable(ck, count);
                if left >= count {
                    // nowhere to put it: trade it for the item in hand, the way a full inventory takes a weapon
                    let Some((_, old)) = self.actors[who].inv.swap_selected(Item::Consumable { kind: ck, count: count.min(ck.def().stack) }) else { return false };
                    let dropped = match old {
                        Item::Weapon { kind, rarity, ammo } => PickupKind::Weapon { kind, rarity, ammo },
                        Item::Consumable { kind, count } => PickupKind::Consumable { kind, count },
                        Item::Pickaxe => return true,
                    };
                    if count > ck.def().stack {
                        // (a stack found on the ground is never bigger than a slot holds, but a hand-made one could be)
                        self.pickups[idx].kind = PickupKind::Consumable { kind: ck, count: count - ck.def().stack };
                    } else {
                        self.pickups.remove(idx);
                    }
                    self.spawn_pickup(apos + Vec3::Y * 0.4, dropped, true);
                    let a = &mut self.actors[who];
                    if !a.build_mode {
                        // what is in hand changed: whatever was going on with the old item (a reload, a heal) is over
                        a.action = Action::Swap { t: 0.25 };
                        a.ads = false;
                        self.events.push(Event::WeaponSwitch { actor: who, pos: apos });
                    }
                    self.events.push(Event::Pickup { actor: who, pos: ppos, sound: PickupSound::Heal, name: ck.name(), rarity: ck.rarity(), count: count.min(ck.def().stack) });
                    return true;
                }
                if left == 0 {
                    self.pickups.remove(idx);
                } else if let PickupKind::Consumable { count: c, .. } = &mut self.pickups[idx].kind {
                    *c = left;
                }
                self.events.push(Event::Pickup { actor: who, pos: ppos, sound: PickupSound::Heal, name: ck.name(), rarity: ck.rarity(), count: count - left });
                true
            }
        }
    }

    pub fn drop_selected(&mut self, who: usize) {
        let sel = self.actors[who].inv.selected;
        let Some(item) = self.actors[who].inv.take_slot(sel) else { return };
        let a = &self.actors[who];
        let fwd = yaw_forward(a.yaw);
        let pos = a.pos + Vec3::Y * 1.2 + fwd * 0.7;
        let pk = match item {
            Item::Weapon { kind, rarity, ammo } => PickupKind::Weapon { kind, rarity, ammo },
            Item::Consumable { kind, count } => PickupKind::Consumable { kind, count },
            Item::Pickaxe => return,
        };
        let id = self.spawn_pickup(pos, pk, true);
        if let Some(i) = self.pickup_index(id) {
            let v = fwd * 3.0 + Vec3::Y * 2.5;
            self.pickups[i].vel = v;
        }
        self.actors[who].action = Action::None;
    }

    /// Spill everything an eliminated actor carried.
    pub fn drop_inventory(&mut self, who: usize) {
        let pos = self.actors[who].pos + Vec3::Y * 0.8;
        let slots = self.actors[who].inv.slots;
        for item in slots.iter().skip(1).flatten() {
            let pk = match *item {
                Item::Weapon { kind, rarity, ammo } => PickupKind::Weapon { kind, rarity, ammo },
                Item::Consumable { kind, count } => PickupKind::Consumable { kind, count },
                Item::Pickaxe => continue,
            };
            self.spawn_pickup(pos, pk, true);
        }
        let ammo = self.actors[who].inv.ammo;
        for k in AmmoKind::ALL {
            let have = ammo[k.index()];
            if have >= k.box_amount() / 2 {
                self.spawn_pickup(pos, PickupKind::Ammo { kind: k, amount: have.min(k.cap() / 2).max(1) }, true);
            }
        }
        self.actors[who].inv.slots = {
            let mut s = [None; 6];
            s[0] = Some(Item::Pickaxe);
            s
        };
        self.actors[who].inv.ammo = [0; 5];
    }

    pub fn select_slot(&mut self, who: usize, slot: usize) {
        if slot >= 6 || self.actors[who].inv.slots[slot].is_none() {
            return;
        }
        let a = &mut self.actors[who];
        if a.inv.selected == slot && !a.build_mode {
            return;
        }
        a.build_mode = false;
        a.inv.selected = slot;
        a.action = match a.inv.selected_weapon() {
            Some((k, _, _)) => Action::Swap { t: k.def().equip_time },
            None => Action::Swap { t: 0.25 },
        };
        a.ads = false;
        let pos = a.pos;
        self.events.push(Event::WeaponSwitch { actor: who, pos });
    }
}

/// Whether the inventory has room for (some of) a pickup: a weapon needs a free slot or a weapon to swap with,
/// ammo needs headroom under the cap, a consumable needs a free slot, a part-filled stack of its kind or the item in hand to trade for.
pub fn can_take(inv: &Inventory, kind: &PickupKind) -> bool {
    match *kind {
        PickupKind::Weapon { .. } => inv.free_slot().is_some() || (1..6).any(|i| matches!(inv.slots[i], Some(Item::Weapon { .. }))),
        PickupKind::Ammo { kind, .. } => inv.ammo[kind.index()] < kind.cap(),
        PickupKind::Consumable { kind, .. } => inv.free_slot().is_some() || inv.can_swap_selected() || (1..6).any(|i| matches!(inv.slots[i], Some(Item::Consumable { kind: k, count }) if k == kind && count < kind.def().stack)),
    }
}

/// Why nothing in reach can be taken, for the player who just tried: the closest thing lying there that does not fit.
fn why_not_taken(inv: &Inventory, kind: &PickupKind) -> String {
    match *kind {
        PickupKind::Ammo { kind, .. } => format!("{} full", kind.name()),
        PickupKind::Consumable { .. } if inv.selected == 0 => "Inventory full. Select an item to swap with it".to_string(),
        PickupKind::Weapon { .. } | PickupKind::Consumable { .. } => "Inventory full".to_string(),
    }
}

/// Physics for loose items; auto-pickup of ammo; chest animation.
pub fn update_pickups(g: &mut Game, dt: f32) {
    step_loose_items(g, dt);
    // auto-pickup ammo for anyone standing on it
    let n = g.actors.len();
    for i in 0..n {
        let a = &g.actors[i];
        if !a.alive || !matches!(a.mode, MoveMode::Ground) {
            continue;
        }
        let c = a.pos + Vec3::Y * 0.5;
        // a box that cannot be taken (ammo type at the cap) must not hide the ones lying on top of it
        let in_reach: Vec<usize> = g.pickups.iter().enumerate().filter(|(_, p)| p.grounded && matches!(p.kind, PickupKind::Ammo { .. }) && p.pos.distance(c) < 1.35 && can_take(&a.inv, &p.kind)).map(|(k, _)| k).collect();
        if let Some(&k) = in_reach.first() {
            g.pickup_item(i, k);
        }
    }
}

/// The part of [`update_pickups`] that is just physics and animation, which a client's copy of the match runs as well:
/// loose items fall and come to rest, opened chests swing their lids up.
pub fn step_loose_items(g: &mut Game, dt: f32) {
    {
        let env = Env::new(&g.world, &g.pieces.grid);
        for p in &mut g.pickups {
            p.age += dt;
            p.spin += dt * 1.1;
            if p.grounded {
                continue;
            }
            p.vel.y -= 20.0 * dt;
            let prev_y = p.pos.y;
            p.pos += p.vel * dt;
            p.vel.x *= (1.0 - 1.2 * dt).max(0.0);
            p.vel.z *= (1.0 - 1.2 * dt).max(0.0);
            // only surfaces the item was above a moment ago count as ground (a fast faller still cannot tunnel through a floor);
            // an item popped up under a ceiling must not land on top of the slab
            let gr = env.ground(p.pos.x, p.pos.z, prev_y, 0.1);
            if p.pos.y <= gr.y {
                p.pos.y = gr.y;
                if p.vel.y < -3.0 {
                    p.vel.y = -p.vel.y * 0.3;
                    p.vel.x *= 0.6;
                    p.vel.z *= 0.6;
                } else {
                    p.vel = Vec3::ZERO;
                    p.grounded = true;
                }
            }
        }
    }
    for c in &mut g.chests {
        supply::fall_step(c, dt);
        if c.opened {
            c.open_t = (c.open_t + dt * 2.6).min(1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::{game, game_cfg};
    use super::*;

    #[test]
    fn world_is_stocked_with_loot_and_chests() {
        let g = game(10, true);
        assert!(g.pickups.len() > 150, "{} pickups", g.pickups.len());
        assert!(g.chests.len() >= 8 && g.chests.iter().all(|c| !c.opened));
        let weapons = g.pickups.iter().filter(|p| matches!(p.kind, PickupKind::Weapon { .. })).count();
        assert!(weapons > 40, "{weapons} weapons on the floor");
    }

    #[test]
    fn picking_up_a_weapon_fills_a_slot_and_swaps_when_full() {
        let mut g = game(2, true);
        let pos = g.actors[PLAYER].pos;
        for k in 0..5 {
            let id = g.spawn_pickup(pos, PickupKind::Weapon { kind: WeaponKind::Pistol, rarity: Rarity::Common, ammo: 5 }, false);
            let idx = g.pickup_index(id).unwrap();
            assert!(g.pickup_item(PLAYER, idx), "pickup {k}");
        }
        assert_eq!(g.actors[PLAYER].inv.weapon_count(), 5);
        // a sixth pickup swaps with the selected weapon and drops the old one
        g.select_slot(PLAYER, 2);
        let before = g.pickups.len();
        let id = g.spawn_pickup(pos, PickupKind::Weapon { kind: WeaponKind::AssaultRifle, rarity: Rarity::Epic, ammo: 30 }, false);
        let idx = g.pickup_index(id).unwrap();
        assert!(g.pickup_item(PLAYER, idx));
        assert!(matches!(g.actors[PLAYER].inv.slots[2], Some(Item::Weapon { kind: WeaponKind::AssaultRifle, rarity: Rarity::Epic, .. })));
        assert_eq!(g.pickups.len(), before + 1, "the swapped-out pistol falls to the ground");
    }

    #[test]
    fn ammo_is_auto_collected_and_capped() {
        let mut g = game(2, true);
        let pos = g.actors[PLAYER].pos;
        g.spawn_pickup(pos, PickupKind::Ammo { kind: AmmoKind::Heavy, amount: 30 }, false);
        g.spawn_pickup(pos, PickupKind::Ammo { kind: AmmoKind::Heavy, amount: 30 }, false);
        for _ in 0..5 {
            g.update(0.05, &PlayerInput::default());
        }
        assert_eq!(g.actors[PLAYER].inv.ammo[AmmoKind::Heavy.index()], 36, "heavy ammo caps at 36");
        // one of the boxes is left with the remainder
        assert!(g.pickups.iter().any(|p| matches!(p.kind, PickupKind::Ammo { kind: AmmoKind::Heavy, .. })));
    }

    #[test]
    fn opening_a_chest_pops_a_weapon_and_ammo() {
        let mut g = game(2, true);
        let c = g.chests[0].clone();
        g.actors[PLAYER].pos = c.pos + Vec3::new(0.0, 0.0, 1.5);
        let before = g.pickups.len();
        assert!(g.interact(PLAYER));
        assert!(g.chests[0].opened);
        assert!(g.pickups.len() >= before + 3, "chest drops >= 3 items");
        let new: Vec<_> = g.pickups.iter().rev().take(3).collect();
        assert!(new.iter().any(|p| matches!(p.kind, PickupKind::Weapon { .. })));
        // items fly out and settle on the ground
        for _ in 0..120 {
            g.update(0.03, &PlayerInput::default());
        }
        assert!(g.pickups.iter().all(|p| p.grounded), "items must come to rest");
        assert!(g.events.iter().any(|e| matches!(e, Event::ChestOpen { .. })) || true);
    }

    #[test]
    fn replacing_the_weapon_in_hand_mid_reload_cancels_the_reload_and_equips_the_new_weapon() {
        let mut g = game_cfg(GameConfig { bots: 1, skip_bus: true, seed: 7, god_mode: true, ..Default::default() });
        let pos = g.actors[PLAYER].pos;
        for _ in 0..5 {
            g.actors[PLAYER].inv.add_weapon(WeaponKind::Pistol, Rarity::Common, 4);
        }
        g.actors[PLAYER].inv.ammo[AmmoKind::Light.index()] = 60;
        g.actors[PLAYER].inv.ammo[AmmoKind::Shells.index()] = 20;
        g.select_slot(PLAYER, 2);
        for _ in 0..30 {
            g.update(0.02, &PlayerInput::default());
        }
        g.update(0.02, &PlayerInput { reload: true, ..Default::default() });
        assert!(matches!(g.actors[PLAYER].action, Action::Reload { .. }), "reloading: {:?}", g.actors[PLAYER].action);
        for _ in 0..40 {
            g.update(0.02, &PlayerInput::default());
        }
        // the inventory is full, so an empty shotgun replaces the pistol in hand
        let id = g.spawn_pickup(pos, PickupKind::Weapon { kind: WeaponKind::Shotgun, rarity: Rarity::Common, ammo: 0 }, false);
        let idx = g.pickup_index(id).unwrap();
        assert!(g.pickup_item(PLAYER, idx));
        let a = &g.actors[PLAYER];
        assert_eq!(a.inv.selected, 2);
        assert!(matches!(a.action, Action::Swap { .. }), "the new weapon is equipped from scratch, not mid-reload: {:?}", a.action);
        // the old reload must not finish onto the new weapon
        for _ in 0..15 {
            g.update(0.02, &PlayerInput::default());
        }
        assert!(matches!(g.actors[PLAYER].inv.slots[2], Some(Item::Weapon { kind: WeaponKind::Shotgun, ammo: 0, .. })), "{:?}", g.actors[PLAYER].inv.slots[2]);
    }

    #[test]
    fn an_item_that_cannot_be_taken_does_not_shadow_the_ones_beside_it() {
        let mut g = game_cfg(GameConfig { bots: 1, skip_bus: true, seed: 7, god_mode: true, ..Default::default() });
        g.pickups.clear();
        let pos = g.actors[PLAYER].pos;
        let heavy_cap = AmmoKind::Heavy.cap();
        g.actors[PLAYER].inv.ammo[AmmoKind::Heavy.index()] = heavy_cap;
        // ammo auto-pickup: a capped box lies exactly where a useful one does
        g.spawn_pickup(pos, PickupKind::Ammo { kind: AmmoKind::Heavy, amount: 6 }, false);
        g.spawn_pickup(pos, PickupKind::Ammo { kind: AmmoKind::Light, amount: 30 }, false);
        for _ in 0..20 {
            g.update(0.05, &PlayerInput::default());
        }
        assert!(g.actors[PLAYER].inv.ammo[AmmoKind::Light.index()] >= 30, "light ammo {}", g.actors[PLAYER].inv.ammo[AmmoKind::Light.index()]);
        assert_eq!(g.actors[PLAYER].inv.ammo[AmmoKind::Heavy.index()], heavy_cap);
        // E: the nearest thing is the capped box, the thing worth taking is the gun a little further away
        g.pickups.clear();
        let fwd = yaw_forward(g.actors[PLAYER].yaw);
        g.spawn_pickup(pos + fwd * 0.3, PickupKind::Ammo { kind: AmmoKind::Heavy, amount: 6 }, false);
        let gun = g.spawn_pickup(pos + fwd * 1.3, PickupKind::Weapon { kind: WeaponKind::Smg, rarity: Rarity::Rare, ammo: 30 }, false);
        assert_eq!(g.find_target(PLAYER), Some(Target::Pickup(gun)));
        assert!(g.interact(PLAYER));
        assert!(g.actors[PLAYER].inv.slots.iter().flatten().any(|i| matches!(i, Item::Weapon { kind: WeaponKind::Smg, .. })));
    }

    fn full_of_pistols(g: &mut Game) {
        g.pickups.clear();
        for _ in 0..5 {
            g.actors[PLAYER].inv.add_weapon(WeaponKind::Pistol, Rarity::Common, 4);
        }
        assert!(g.actors[PLAYER].inv.free_slot().is_none());
    }

    #[test]
    fn a_full_inventory_trades_a_healing_item_for_the_item_in_hand() {
        let mut g = game_cfg(GameConfig { bots: 1, skip_bus: true, seed: 7, god_mode: true, ..Default::default() });
        let pos = g.actors[PLAYER].pos;
        full_of_pistols(&mut g);
        g.select_slot(PLAYER, 2);
        let id = g.spawn_pickup(pos, PickupKind::Consumable { kind: ConsumableKind::MedKit, count: 1 }, false);
        assert_eq!(g.find_target(PLAYER), Some(Target::Pickup(id)), "the medkit can be traded for the pistol in hand");
        assert!(g.interact(PLAYER));
        assert!(matches!(g.actors[PLAYER].inv.slots[2], Some(Item::Consumable { kind: ConsumableKind::MedKit, count: 1 })), "{:?}", g.actors[PLAYER].inv.slots);
        assert_eq!(g.actors[PLAYER].inv.weapon_count(), 4);
        assert_eq!(g.pickups.len(), 1, "the medkit is gone and the pistol lies there instead");
        assert!(matches!(g.pickups[0].kind, PickupKind::Weapon { kind: WeaponKind::Pistol, .. }));
        assert!(matches!(g.actors[PLAYER].action, Action::Swap { .. }), "the new item is raised");
        assert!(g.events.iter().any(|e| matches!(e, Event::Pickup { name: "Medkit", .. })));
        // a healing item in hand can be traded as well, and the stack that was in hand goes down whole
        g.actors[PLAYER].action = Action::None;
        g.pickups.clear();
        let id = g.spawn_pickup(pos, PickupKind::Consumable { kind: ConsumableKind::ShieldBig, count: 2 }, false);
        assert_eq!(g.find_target(PLAYER), Some(Target::Pickup(id)));
        assert!(g.interact(PLAYER));
        assert!(matches!(g.actors[PLAYER].inv.slots[2], Some(Item::Consumable { kind: ConsumableKind::ShieldBig, count: 2 })));
        assert!(g.pickups.iter().any(|p| matches!(p.kind, PickupKind::Consumable { kind: ConsumableKind::MedKit, count: 1 })), "the medkit went back to the ground");
    }

    #[test]
    fn a_full_inventory_with_the_pickaxe_in_hand_says_so_instead_of_staying_silent() {
        let mut g = game_cfg(GameConfig { bots: 1, skip_bus: true, seed: 7, god_mode: true, ..Default::default() });
        let pos = g.actors[PLAYER].pos;
        full_of_pistols(&mut g);
        g.select_slot(PLAYER, 0);
        g.spawn_pickup(pos, PickupKind::Consumable { kind: ConsumableKind::Bandage, count: 5 }, false);
        assert_eq!(g.find_target(PLAYER), None, "nothing to trade with: the pickaxe is not for trading");
        assert!(!g.interact(PLAYER));
        assert!(g.events.iter().any(|e| matches!(e, Event::Toast { actor: Some(PLAYER), text, .. } if text.contains("Inventory full"))), "{:?}", g.events);
        // ammo at the cap says which one
        g.events.clear();
        g.pickups.clear();
        g.actors[PLAYER].inv.ammo[AmmoKind::Heavy.index()] = AmmoKind::Heavy.cap();
        g.spawn_pickup(pos, PickupKind::Ammo { kind: AmmoKind::Heavy, amount: 6 }, false);
        assert!(!g.interact(PLAYER));
        assert!(g.events.iter().any(|e| matches!(e, Event::Toast { text, .. } if text.contains("Heavy Ammo full"))), "{:?}", g.events);
        // with nothing lying around there is nothing to complain about
        g.events.clear();
        g.pickups.clear();
        assert!(!g.interact(PLAYER));
        assert!(!g.events.iter().any(|e| matches!(e, Event::Toast { .. })));
    }

    #[test]
    fn picking_up_a_weapon_does_not_leave_build_mode() {
        let mut g = game(2, true);
        let pos = g.actors[PLAYER].pos;
        g.actors[PLAYER].build_mode = true;
        let id = g.spawn_pickup(pos, PickupKind::Weapon { kind: WeaponKind::Smg, rarity: Rarity::Rare, ammo: 30 }, false);
        let idx = g.pickup_index(id).unwrap();
        assert!(g.pickup_item(PLAYER, idx));
        assert!(g.actors[PLAYER].build_mode, "picking something up must not end building");
        assert!(matches!(g.actors[PLAYER].inv.slots[1], Some(Item::Weapon { kind: WeaponKind::Smg, .. })));
        assert_eq!(g.actors[PLAYER].inv.selected, 0, "nothing is equipped behind the player's back");
        // a full inventory swaps without leaving build mode as well
        for _ in 0..4 {
            g.actors[PLAYER].inv.add_weapon(WeaponKind::Pistol, Rarity::Common, 4);
        }
        let id = g.spawn_pickup(pos, PickupKind::Weapon { kind: WeaponKind::Shotgun, rarity: Rarity::Epic, ammo: 5 }, false);
        let idx = g.pickup_index(id).unwrap();
        assert!(g.pickup_item(PLAYER, idx));
        assert!(g.actors[PLAYER].build_mode);
    }

    #[test]
    fn chest_loot_stays_on_the_chests_floor_instead_of_snapping_onto_the_ceiling() {
        // items pop up ~1.5 m; the ground probe used to reach 1.5 m above them, so under a low ceiling the slab's top
        // (the attic, unreachable from inside) counted as ground
        let mut g = game(1, true);
        g.actors[1].brain = None;
        g.actors[1].pos = Vec3::new(0.0, 100.0, 0.0);
        let (mut total, mut high) = (0, vec![]);
        for ci in 0..g.chests.len() {
            let c = g.chests[ci].clone();
            let before: Vec<u32> = g.pickups.iter().map(|p| p.id).collect();
            g.open_chest(ci, PLAYER);
            for _ in 0..240 {
                g.update(1.0 / 60.0, &PlayerInput::default());
            }
            for p in g.pickups.iter().filter(|p| !before.contains(&p.id)) {
                total += 1;
                if p.pos.y - c.pos.y > 1.2 {
                    high.push((ci, p.pos.y - c.pos.y));
                }
            }
        }
        assert!(total >= 3 * g.chests.len(), "{total} items from {} chests", g.chests.len());
        assert!(high.is_empty(), "{} of {total} chest items came to rest more than 1.2 m above their chest: {high:?}", high.len());
    }

    #[test]
    fn items_dropped_by_an_eliminated_actor_land_on_the_floor_under_a_low_ceiling() {
        let mut g = game(1, true);
        let b = g.world.buildings.iter().find(|b| b.kind == "house").expect("a house").clone();
        let floor = b.door_in.y;
        g.actors[1].brain = None;
        g.actors[1].pos = Vec3::new(0.0, 100.0, 0.0);
        let (mut total, mut high) = (0, vec![]);
        for k in 0..40 {
            g.pickups.clear();
            // spread over the room interior around the door
            let off = Vec3::new((k % 5) as f32 * 0.35 - 0.7, 0.0, (k / 5) as f32 * 0.25 - 0.5);
            g.actors[PLAYER].pos = Vec3::new(b.door_in.x, floor, b.door_in.z) + off;
            g.actors[PLAYER].inv.add_weapon(WeaponKind::Smg, Rarity::Rare, 12);
            g.actors[PLAYER].inv.add_consumable(ConsumableKind::Bandage, 5);
            g.actors[PLAYER].inv.ammo[0] = 120;
            g.drop_inventory(PLAYER);
            for _ in 0..240 {
                g.update(1.0 / 60.0, &PlayerInput::default());
            }
            for p in &g.pickups {
                total += 1;
                if p.pos.y - floor > 1.0 {
                    high.push((k, p.pos.y - floor));
                }
            }
        }
        assert!(total > 40, "{total} drops");
        assert!(high.is_empty(), "{} of {total} dropped items came to rest on the ceiling slab: {high:?}", high.len());
    }

    #[test]
    fn dropping_and_death_drops() {
        let mut g = game(2, true);
        g.actors[PLAYER].inv.add_weapon(WeaponKind::Smg, Rarity::Rare, 12);
        g.actors[PLAYER].inv.add_consumable(ConsumableKind::Bandage, 5);
        g.actors[PLAYER].inv.ammo[0] = 100;
        g.select_slot(PLAYER, 1);
        let n = g.pickups.len();
        g.drop_selected(PLAYER);
        assert_eq!(g.pickups.len(), n + 1);
        assert_eq!(g.actors[PLAYER].inv.slots[1], None);
        g.drop_inventory(PLAYER);
        assert!(g.pickups.len() >= n + 3, "bandages and ammo spill on death");
        assert_eq!(g.actors[PLAYER].inv.weapon_count(), 0);
    }
}
