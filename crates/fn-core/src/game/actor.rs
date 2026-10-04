//! Actors: the human player and the bots share one body model, inventory and state.

use super::ai::Brain;
use super::items::*;
use crate::math::*;

pub const RADIUS: f32 = 0.38;
pub const HEIGHT: f32 = 1.78;
pub const CROUCH_HEIGHT: f32 = 1.22;
pub const EYE: f32 = 1.62;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveMode {
    Bus,
    Freefall,
    Glide,
    Ground,
    Swim,
    Dead,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PieceKind {
    Wall,
    Floor,
    Ramp,
    Roof,
}

impl PieceKind {
    pub const ALL: [PieceKind; 4] = [PieceKind::Wall, PieceKind::Floor, PieceKind::Ramp, PieceKind::Roof];
    pub fn name(self) -> &'static str {
        ["Wall", "Floor", "Ramp", "Roof"][self as usize]
    }
    pub fn cost(self) -> u32 {
        10
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Outfit {
    pub skin: Vec3,
    pub shirt: Vec3,
    pub pants: Vec3,
    pub boots: Vec3,
    pub hair: Vec3,
    /// 0 none, 1 cap, 2 beanie, 3 helmet, 4 hat
    pub headgear: u8,
    pub headgear_color: Vec3,
    /// 0 bald, 1 short, 2 long, 3 ponytail
    pub hair_style: u8,
    pub backpack: Vec3,
    pub glider: Vec3,
    pub accent: Vec3,
}

impl Outfit {
    pub fn random(rng: &mut crate::rng::Rng) -> Outfit {
        let skins = [0xf3c9a5u32, 0xe8b48a, 0xc98f67, 0x9c6a45, 0x6d4430, 0xf7d6bc];
        let hairs = [0x2b1b12u32, 0x5a3a1e, 0xb98a3a, 0xd9c27a, 0x8a2e1d, 0x1d1d22, 0xd94a8b, 0x3a7bd9, 0x6b3fd9];
        let vivid = |rng: &mut crate::rng::Rng| hsv(rng.f32(), rng.range(0.55, 0.95), rng.range(0.7, 1.0));
        let pants = [0x2d3a52u32, 0x3b3b44, 0x6b5a3a, 0x24424a, 0x4a2f4e, 0x1f2a36];
        let boots = [0x23232a, 0x5a3b24, 0xdddddd];
        let shirt = vivid(rng);
        Outfit {
            skin: crate::mesh::hex(*rng.pick(&skins)),
            shirt,
            pants: crate::mesh::hex(*rng.pick(&pants)),
            boots: crate::mesh::hex(*rng.pick(&boots)),
            hair: crate::mesh::hex(*rng.pick(&hairs)),
            headgear: rng.weighted(&[3.0, 2.0, 2.0, 1.5, 1.0]) as u8,
            headgear_color: vivid(rng),
            hair_style: rng.below(4) as u8,
            backpack: vivid(rng),
            glider: vivid(rng),
            accent: vivid(rng),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    None,
    Reload { t: f32, dur: f32 },
    Heal { slot: usize, t: f32, dur: f32 },
    Swap { t: f32 },
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AnimState {
    /// Walk cycle phase in radians.
    pub phase: f32,
    /// 0 idle .. 1 full run.
    pub run: f32,
    pub sprint: f32,
    pub crouch: f32,
    pub air: f32,
    pub aim: f32,
    /// Local-space velocity used to lean / strafe.
    pub lean_fwd: f32,
    pub lean_side: f32,
    pub swing: f32,
    pub recoil: f32,
    pub reload: f32,
    pub heal: f32,
    pub land: f32,
    pub time: f32,
    pub build: f32,
    /// 0..1 blend into the dance emote.
    pub emote: f32,
    /// Seconds into the current dance (keeps running while the blend fades out).
    pub emote_clock: f32,
}

#[derive(Clone, Debug)]
pub struct Inventory {
    /// Slot 0 is always the harvesting tool; 1..=5 hold weapons and consumables.
    pub slots: [Option<Item>; 6],
    pub selected: usize,
    pub ammo: [u32; 5],
    pub mats: [u32; 3],
}

#[derive(Debug, PartialEq)]
pub enum AddResult {
    Added(usize),
    /// The item replaced the one that was in the selected slot.
    Swapped { slot: usize, dropped: Item },
    Full,
}

impl Default for Inventory {
    fn default() -> Self {
        Self::new()
    }
}

impl Inventory {
    pub fn new() -> Self {
        let mut slots = [None; 6];
        slots[0] = Some(Item::Pickaxe);
        Self { slots, selected: 0, ammo: [0; 5], mats: [0; 3] }
    }

    pub fn selected_item(&self) -> Option<&Item> {
        self.slots[self.selected].as_ref()
    }
    pub fn selected_weapon(&self) -> Option<(WeaponKind, Rarity, u32)> {
        match self.slots[self.selected] {
            Some(Item::Weapon { kind, rarity, ammo }) => Some((kind, rarity, ammo)),
            _ => None,
        }
    }

    pub fn free_slot(&self) -> Option<usize> {
        (1..6).find(|&i| self.slots[i].is_none())
    }

    /// Pick a weapon up. Takes a free slot, otherwise swaps with the selected weapon.
    pub fn add_weapon(&mut self, kind: WeaponKind, rarity: Rarity, ammo: u32) -> AddResult {
        let item = Item::Weapon { kind, rarity, ammo };
        if let Some(i) = self.free_slot() {
            self.slots[i] = Some(item);
            return AddResult::Added(i);
        }
        let sel = self.selected;
        if sel != 0 {
            if let Some(old @ Item::Weapon { .. }) = self.slots[sel] {
                self.slots[sel] = Some(item);
                return AddResult::Swapped { slot: sel, dropped: old };
            }
        }
        // selected slot isn't a weapon: swap the lowest-rarity weapon instead
        let worst = (1..6)
            .filter_map(|i| match self.slots[i] {
                Some(Item::Weapon { rarity, .. }) => Some((rarity, i)),
                _ => None,
            })
            .min();
        if let Some((_, i)) = worst {
            let old = self.slots[i].unwrap();
            self.slots[i] = Some(item);
            return AddResult::Swapped { slot: i, dropped: old };
        }
        AddResult::Full
    }

    /// Add consumables; returns how many could not be taken.
    pub fn add_consumable(&mut self, kind: ConsumableKind, count: u32) -> u32 {
        let stack = kind.def().stack;
        let mut left = count;
        for i in 1..6 {
            if let Some(Item::Consumable { kind: k, count: c }) = &mut self.slots[i] {
                if *k == kind && *c < stack {
                    let add = (stack - *c).min(left);
                    *c += add;
                    left -= add;
                }
            }
        }
        while left > 0 {
            let Some(i) = self.free_slot() else { break };
            let add = left.min(stack);
            self.slots[i] = Some(Item::Consumable { kind, count: add });
            left -= add;
        }
        left
    }

    /// Add ammo to the reserve; returns the amount actually added.
    pub fn add_ammo(&mut self, kind: AmmoKind, amount: u32) -> u32 {
        let cap = kind.cap();
        let cur = self.ammo[kind.index()];
        let add = amount.min(cap.saturating_sub(cur));
        self.ammo[kind.index()] += add;
        add
    }

    pub fn add_mats(&mut self, m: Mat, amount: u32) {
        self.mats[m.index()] = (self.mats[m.index()] + amount).min(999);
    }

    pub fn take_slot(&mut self, i: usize) -> Option<Item> {
        if i == 0 || i >= 6 {
            return None;
        }
        let it = self.slots[i].take();
        if self.selected == i {
            self.selected = 0;
        }
        it
    }

    /// Remove one consumable from a slot (clears the slot when it hits zero).
    pub fn use_one(&mut self, i: usize) {
        if let Some(Item::Consumable { count, .. }) = &mut self.slots[i] {
            *count -= 1;
            if *count == 0 {
                self.slots[i] = None;
                if self.selected == i {
                    self.selected = 0;
                }
            }
        }
    }

    pub fn best_weapon_slot(&self) -> Option<usize> {
        (1..6)
            .filter_map(|i| match self.slots[i] {
                Some(Item::Weapon { kind, rarity, .. }) => Some((weapon_score(kind, rarity), i)),
                _ => None,
            })
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
            .map(|x| x.1)
    }

    pub fn weapon_count(&self) -> usize {
        (1..6).filter(|&i| matches!(self.slots[i], Some(Item::Weapon { .. }))).count()
    }

    pub fn has_ammo_for(&self, kind: WeaponKind) -> bool {
        self.ammo[kind.def().ammo.index()] > 0
    }
}

/// A rough power rating used by bots to choose what to carry.
pub fn weapon_score(kind: WeaponKind, rarity: Rarity) -> f32 {
    let base = match kind {
        WeaponKind::AssaultRifle => 100.0,
        WeaponKind::Shotgun => 92.0,
        WeaponKind::Smg => 80.0,
        WeaponKind::Sniper => 78.0,
        WeaponKind::RocketLauncher => 70.0,
        WeaponKind::Pistol => 50.0,
    };
    base * (1.0 + rarity.index() as f32 * 0.18)
}

pub struct Actor {
    pub id: usize,
    pub name: String,
    pub human: bool,
    pub pos: Vec3,
    pub vel: Vec3,
    /// Aim direction (camera for the player, head for bots).
    pub yaw: f32,
    pub pitch: f32,
    /// Direction the body model faces.
    pub body_yaw: f32,
    pub mode: MoveMode,
    pub on_ground: bool,
    pub crouching: bool,
    pub sprinting: bool,
    pub ads: bool,
    pub hp: f32,
    pub shield: f32,
    pub alive: bool,
    pub inv: Inventory,
    pub action: Action,
    pub fire_cd: f32,
    /// Accumulated bloom as a multiplier on weapon spread (>= 0).
    pub bloom: f32,
    pub recoil_kick: f32,
    pub shot_flash: f32,
    pub kills: u32,
    pub placement: u32,
    pub damage_dealt: f32,
    pub outfit: Outfit,
    pub anim: AnimState,
    pub peak_y: f32,
    pub coyote: f32,
    pub jump_buffer: f32,
    pub glide_deployed: bool,
    pub storm_acc: f32,
    pub hit_flash: f32,
    /// Seconds since death (drives the dissolve effect).
    pub dead_time: f32,
    /// The simulation step this actor was eliminated in (actors that fall in the same step have no order between them).
    pub death_step: u64,
    pub build_mode: bool,
    pub build_piece: PieceKind,
    pub build_mat: Mat,
    pub last_damage_from: Option<usize>,
    pub last_damage_time: f32,
    pub steps: f32,
    pub brain: Option<Box<Brain>>,
    /// Smoothed eye height for stair/ledge stepping (camera uses it).
    pub eye_smooth: f32,
    pub bus_jump_target: Option<Vec2>,
    /// Pickaxe swing in progress: seconds until the blow lands (<= 0 when idle).
    pub melee_t: f32,
    pub melee_pending: bool,
    /// Dancing: the weapon is holstered and the body plays the emote until the actor does anything else.
    pub emoting: bool,
}

impl Actor {
    pub fn new(id: usize, name: &str, human: bool, outfit: Outfit) -> Actor {
        Actor {
            id,
            name: name.to_string(),
            human,
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            body_yaw: 0.0,
            mode: MoveMode::Bus,
            on_ground: false,
            crouching: false,
            sprinting: false,
            ads: false,
            hp: 100.0,
            shield: 0.0,
            alive: true,
            inv: Inventory::new(),
            action: Action::None,
            fire_cd: 0.0,
            bloom: 0.0,
            recoil_kick: 0.0,
            shot_flash: 0.0,
            kills: 0,
            placement: 0,
            damage_dealt: 0.0,
            outfit,
            anim: AnimState::default(),
            peak_y: 0.0,
            coyote: 0.0,
            jump_buffer: 0.0,
            glide_deployed: false,
            storm_acc: 0.0,
            hit_flash: 0.0,
            dead_time: 0.0,
            death_step: 0,
            build_mode: false,
            build_piece: PieceKind::Wall,
            build_mat: Mat::Wood,
            last_damage_from: None,
            last_damage_time: -100.0,
            steps: 0.0,
            brain: None,
            eye_smooth: 0.0,
            bus_jump_target: None,
            melee_t: 0.0,
            melee_pending: false,
            emoting: false,
        }
    }

    pub fn height(&self) -> f32 {
        if self.crouching {
            CROUCH_HEIGHT
        } else {
            HEIGHT
        }
    }

    pub fn eye_pos(&self) -> Vec3 {
        self.pos + Vec3::Y * (if self.crouching { CROUCH_HEIGHT - 0.16 } else { EYE })
    }

    pub fn forward(&self) -> Vec3 {
        look_dir(self.yaw, self.pitch)
    }

    pub fn total_hp(&self) -> f32 {
        self.hp + self.shield
    }

    pub fn is_airborne_mode(&self) -> bool {
        matches!(self.mode, MoveMode::Freefall | MoveMode::Glide | MoveMode::Bus)
    }

    pub fn speed_xz(&self) -> f32 {
        Vec2::new(self.vel.x, self.vel.z).length()
    }

    /// Centre of the body for aiming / hit tests.
    pub fn chest(&self) -> Vec3 {
        self.pos + Vec3::Y * (self.height() * 0.62)
    }

    pub fn head_pos(&self) -> Vec3 {
        self.pos + Vec3::Y * (self.height() - 0.2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_starts_with_pickaxe_and_fills_slots() {
        let mut inv = Inventory::new();
        assert_eq!(inv.slots[0], Some(Item::Pickaxe));
        for i in 0..5 {
            let r = inv.add_weapon(WeaponKind::Pistol, Rarity::Common, 10);
            assert_eq!(r, AddResult::Added(i + 1));
        }
        assert_eq!(inv.free_slot(), None);
    }

    #[test]
    fn full_inventory_swaps_selected_weapon() {
        let mut inv = Inventory::new();
        for _ in 0..5 {
            inv.add_weapon(WeaponKind::Pistol, Rarity::Common, 10);
        }
        inv.selected = 3;
        match inv.add_weapon(WeaponKind::AssaultRifle, Rarity::Epic, 30) {
            AddResult::Swapped { slot, dropped } => {
                assert_eq!(slot, 3);
                assert!(matches!(dropped, Item::Weapon { kind: WeaponKind::Pistol, .. }));
            }
            other => panic!("{other:?}"),
        }
        // with the pickaxe selected, the weakest weapon is replaced
        inv.selected = 0;
        inv.slots[2] = Some(Item::Weapon { kind: WeaponKind::Pistol, rarity: Rarity::Common, ammo: 1 });
        match inv.add_weapon(WeaponKind::Smg, Rarity::Rare, 30) {
            AddResult::Swapped { dropped, .. } => assert!(matches!(dropped, Item::Weapon { rarity: Rarity::Common, .. })),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn consumables_stack_and_overflow() {
        let mut inv = Inventory::new();
        assert_eq!(inv.add_consumable(ConsumableKind::Bandage, 10), 0);
        assert_eq!(inv.add_consumable(ConsumableKind::Bandage, 10), 0); // 15 + 5 in a new slot
        let counts: Vec<u32> = inv.slots.iter().filter_map(|s| if let Some(Item::Consumable { count, .. }) = s { Some(*count) } else { None }).collect();
        assert_eq!(counts, vec![15, 5]);
        // ChugJug stack of 1 fills slots; leftovers are returned when full
        let mut inv = Inventory::new();
        for _ in 0..5 {
            assert_eq!(inv.add_consumable(ConsumableKind::ChugJug, 1), 0);
        }
        assert_eq!(inv.add_consumable(ConsumableKind::ChugJug, 1), 1);
    }

    #[test]
    fn ammo_caps_apply() {
        let mut inv = Inventory::new();
        assert_eq!(inv.add_ammo(AmmoKind::Heavy, 30), 30);
        assert_eq!(inv.add_ammo(AmmoKind::Heavy, 30), 6);
        assert_eq!(inv.ammo[AmmoKind::Heavy.index()], 36);
        inv.add_mats(Mat::Wood, 2000);
        assert_eq!(inv.mats[0], 999);
    }

    #[test]
    fn using_the_last_consumable_clears_the_slot_and_selection() {
        let mut inv = Inventory::new();
        inv.add_consumable(ConsumableKind::MedKit, 1);
        inv.selected = 1;
        inv.use_one(1);
        assert_eq!(inv.slots[1], None);
        assert_eq!(inv.selected, 0);
    }

    #[test]
    fn best_weapon_prefers_rarity_and_type() {
        let mut inv = Inventory::new();
        inv.add_weapon(WeaponKind::Pistol, Rarity::Legendary, 1);
        inv.add_weapon(WeaponKind::AssaultRifle, Rarity::Common, 1);
        let best = inv.best_weapon_slot().unwrap();
        // legendary pistol (50*1.72=86) < common AR (100)
        assert_eq!(best, 2);
    }
}
