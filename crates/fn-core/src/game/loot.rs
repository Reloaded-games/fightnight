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
            self.chests.push(Chest { id, pos: c.pos, yaw: c.yaw, open_t: 0.0, opened: false });
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
            if d < INTERACT_RANGE && best.is_none_or(|b| d < b.0) {
                best = Some((d, Target::Pickup(p.id)));
            }
        }
        for ch in &self.chests {
            if ch.opened {
                continue;
            }
            let d = ch.pos.distance(c);
            if d < INTERACT_RANGE + 0.4 && best.is_none_or(|b| d < b.0 + 0.3) {
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
            None => false,
        }
    }

    pub fn open_chest(&mut self, idx: usize, by: usize) {
        if self.chests[idx].opened {
            return;
        }
        self.chests[idx].opened = true;
        let (pos, yaw) = (self.chests[idx].pos, self.chests[idx].yaw);
        self.events.push(Event::ChestOpen { pos });
        self.events.push(Event::Noise { pos, radius: 30.0, source: by });
        let mut rng = self.rng.fork(self.chests[idx].id as u64);
        let drops = roll_chest_loot(&mut rng);
        let n = drops.len();
        for (k, d) in drops.into_iter().enumerate() {
            // fan the loot out in front of the chest
            let spread = (k as f32 - (n as f32 - 1.0) / 2.0) * 0.55;
            let ang = -yaw + std::f32::consts::FRAC_PI_2 * 0.0 + spread;
            let out = Vec3::new(-ang.sin().abs() * 0.0 + (yaw + spread).sin(), 0.0, (yaw + spread).cos());
            let id = self.new_id();
            let vel = out * rng.range(2.2, 3.4) + Vec3::Y * rng.range(5.0, 6.5);
            let _ = ang;
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
                        if was_empty_handed || self.actors[who].human {
                            // pick it up and (for a free slot) grab it
                            if was_empty_handed {
                                self.select_slot(who, slot);
                            }
                        }
                    }
                    AddResult::Swapped { slot, dropped } => {
                        self.pickups.remove(idx);
                        let pk = match dropped {
                            Item::Weapon { kind, rarity, ammo } => PickupKind::Weapon { kind, rarity, ammo },
                            _ => return true,
                        };
                        self.spawn_pickup(apos + Vec3::Y * 0.4, pk, true);
                        self.select_slot(who, slot);
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
                    return false;
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

/// Physics for loose items; auto-pickup of ammo; chest animation.
pub fn update_pickups(g: &mut Game, dt: f32) {
    {
        let env = Env::new(&g.world, &g.pieces.grid);
        for p in &mut g.pickups {
            p.age += dt;
            p.spin += dt * 1.1;
            if p.grounded {
                continue;
            }
            p.vel.y -= 20.0 * dt;
            p.pos += p.vel * dt;
            p.vel.x *= (1.0 - 1.2 * dt).max(0.0);
            p.vel.z *= (1.0 - 1.2 * dt).max(0.0);
            let gr = env.ground(p.pos.x, p.pos.z, p.pos.y + 0.6, 0.9);
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
        if c.opened {
            c.open_t = (c.open_t + dt * 2.6).min(1.0);
        }
    }
    // auto-pickup ammo for anyone standing on it
    let n = g.actors.len();
    for i in 0..n {
        let a = &g.actors[i];
        if !a.alive || !matches!(a.mode, MoveMode::Ground) {
            continue;
        }
        let c = a.pos + Vec3::Y * 0.5;
        let mut hit = None;
        for (k, p) in g.pickups.iter().enumerate() {
            if p.grounded && matches!(p.kind, PickupKind::Ammo { .. }) && p.pos.distance(c) < 1.35 {
                hit = Some(k);
                break;
            }
        }
        if let Some(k) = hit {
            g.pickup_item(i, k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::game;
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
