//! Item definitions: weapons, ammo, consumables, rarities and loot tables.

use crate::math::*;
use crate::rng::Rng;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

impl Rarity {
    pub const ALL: [Rarity; 5] = [Rarity::Common, Rarity::Uncommon, Rarity::Rare, Rarity::Epic, Rarity::Legendary];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        ["Common", "Uncommon", "Rare", "Epic", "Legendary"][self as usize]
    }
    /// sRGB display colour (HUD, beams).
    pub fn color(self) -> Vec3 {
        [
            Vec3::new(0.74, 0.76, 0.80),
            Vec3::new(0.30, 0.82, 0.22),
            Vec3::new(0.22, 0.56, 1.00),
            Vec3::new(0.74, 0.34, 0.97),
            Vec3::new(1.00, 0.72, 0.14),
        ][self as usize]
    }
    pub fn damage_mul(self) -> f32 {
        [1.0, 1.05, 1.10, 1.15, 1.20][self as usize]
    }
    pub fn reload_mul(self) -> f32 {
        [1.0, 0.96, 0.92, 0.88, 0.84][self as usize]
    }
    pub fn from_index(i: usize) -> Rarity {
        Rarity::ALL[i.min(4)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AmmoKind {
    Light,
    Medium,
    Heavy,
    Shells,
    Rockets,
}

impl AmmoKind {
    pub const ALL: [AmmoKind; 5] = [AmmoKind::Light, AmmoKind::Medium, AmmoKind::Heavy, AmmoKind::Shells, AmmoKind::Rockets];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        ["Light Ammo", "Medium Ammo", "Heavy Ammo", "Shells", "Rockets"][self as usize]
    }
    pub fn cap(self) -> u32 {
        [250, 220, 36, 40, 12][self as usize]
    }
    /// Amount found in one ammo box.
    pub fn box_amount(self) -> u32 {
        [40, 30, 6, 8, 3][self as usize]
    }
    pub fn color(self) -> Vec3 {
        [Vec3::new(0.95, 0.75, 0.25), Vec3::new(0.40, 0.75, 0.35), Vec3::new(0.35, 0.50, 0.95), Vec3::new(0.90, 0.35, 0.25), Vec3::new(0.95, 0.55, 0.15)][self as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WeaponKind {
    Pistol,
    Smg,
    AssaultRifle,
    Shotgun,
    Sniper,
    RocketLauncher,
}

impl WeaponKind {
    pub const ALL: [WeaponKind; 6] = [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::AssaultRifle, WeaponKind::Shotgun, WeaponKind::Sniper, WeaponKind::RocketLauncher];
    pub fn def(self) -> &'static WeaponDef {
        &WEAPONS[self as usize]
    }
    pub fn name(self) -> &'static str {
        self.def().name
    }
}

pub struct WeaponDef {
    pub name: &'static str,
    pub ammo: AmmoKind,
    pub auto: bool,
    /// Damage per pellet / bullet at common rarity.
    pub damage: f32,
    pub pellets: u32,
    /// Shots per second.
    pub rate: f32,
    pub mag: u32,
    pub reload: f32,
    /// Cone half-angle in degrees when firing from the hip / aiming down sights.
    pub spread_hip: f32,
    pub spread_ads: f32,
    pub bloom_per_shot: f32,
    pub range: f32,
    /// Distance at which damage starts to fall off, and the multiplier at `range`.
    pub falloff_start: f32,
    pub falloff_min: f32,
    pub head_mult: f32,
    /// Camera kick in degrees per shot.
    pub recoil: f32,
    /// Field of view multiplier when aiming (smaller = more zoom).
    pub ads_zoom: f32,
    /// Metres/second for projectile weapons, 0 for hitscan.
    pub projectile_speed: f32,
    pub explosion_radius: f32,
    pub scope: bool,
    pub equip_time: f32,
    pub move_mul: f32,
}

pub static WEAPONS: [WeaponDef; 6] = [
    WeaponDef { name: "Pistol", ammo: AmmoKind::Light, auto: false, damage: 24.0, pellets: 1, rate: 6.0, mag: 16, reload: 1.7, spread_hip: 1.9, spread_ads: 0.6, bloom_per_shot: 0.55, range: 130.0, falloff_start: 45.0, falloff_min: 0.62, head_mult: 2.0, recoil: 0.9, ads_zoom: 0.82, projectile_speed: 0.0, explosion_radius: 0.0, scope: false, equip_time: 0.3, move_mul: 1.0 },
    WeaponDef { name: "Submachine Gun", ammo: AmmoKind::Light, auto: true, damage: 17.0, pellets: 1, rate: 11.0, mag: 30, reload: 2.1, spread_hip: 3.2, spread_ads: 1.5, bloom_per_shot: 0.28, range: 95.0, falloff_start: 30.0, falloff_min: 0.5, head_mult: 1.6, recoil: 0.55, ads_zoom: 0.8, projectile_speed: 0.0, explosion_radius: 0.0, scope: false, equip_time: 0.35, move_mul: 1.0 },
    WeaponDef { name: "Assault Rifle", ammo: AmmoKind::Medium, auto: true, damage: 30.0, pellets: 1, rate: 5.5, mag: 30, reload: 2.3, spread_hip: 2.4, spread_ads: 0.8, bloom_per_shot: 0.3, range: 230.0, falloff_start: 75.0, falloff_min: 0.6, head_mult: 1.5, recoil: 0.8, ads_zoom: 0.68, projectile_speed: 0.0, explosion_radius: 0.0, scope: false, equip_time: 0.4, move_mul: 0.97 },
    WeaponDef { name: "Pump Shotgun", ammo: AmmoKind::Shells, auto: false, damage: 9.5, pellets: 10, rate: 0.85, mag: 5, reload: 4.3, spread_hip: 5.8, spread_ads: 4.2, bloom_per_shot: 1.0, range: 55.0, falloff_start: 8.0, falloff_min: 0.18, head_mult: 1.5, recoil: 4.0, ads_zoom: 0.85, projectile_speed: 0.0, explosion_radius: 0.0, scope: false, equip_time: 0.45, move_mul: 0.95 },
    WeaponDef { name: "Bolt-Action Sniper", ammo: AmmoKind::Heavy, auto: false, damage: 105.0, pellets: 1, rate: 0.55, mag: 1, reload: 2.8, spread_hip: 7.0, spread_ads: 0.0, bloom_per_shot: 2.0, range: 600.0, falloff_start: 400.0, falloff_min: 1.0, head_mult: 2.5, recoil: 3.0, ads_zoom: 0.16, projectile_speed: 0.0, explosion_radius: 0.0, scope: true, equip_time: 0.55, move_mul: 0.9 },
    WeaponDef { name: "Rocket Launcher", ammo: AmmoKind::Rockets, auto: false, damage: 100.0, pellets: 1, rate: 0.45, mag: 1, reload: 3.6, spread_hip: 1.5, spread_ads: 0.3, bloom_per_shot: 1.0, range: 400.0, falloff_start: 0.0, falloff_min: 1.0, head_mult: 1.0, recoil: 3.0, ads_zoom: 0.78, projectile_speed: 58.0, explosion_radius: 7.0, scope: false, equip_time: 0.6, move_mul: 0.9 },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConsumableKind {
    Bandage,
    MedKit,
    ShieldSmall,
    ShieldBig,
    ChugJug,
}

pub struct ConsumableDef {
    pub name: &'static str,
    pub heal: f32,
    pub shield: f32,
    /// The value above which the item will not heal further.
    pub max_hp: f32,
    pub max_shield: f32,
    pub use_time: f32,
    pub stack: u32,
}

impl ConsumableKind {
    pub const ALL: [ConsumableKind; 5] = [ConsumableKind::Bandage, ConsumableKind::MedKit, ConsumableKind::ShieldSmall, ConsumableKind::ShieldBig, ConsumableKind::ChugJug];
    pub fn def(self) -> &'static ConsumableDef {
        &CONSUMABLES[self as usize]
    }
    pub fn name(self) -> &'static str {
        self.def().name
    }
    pub fn color(self) -> Vec3 {
        match self {
            ConsumableKind::Bandage => Vec3::new(0.95, 0.95, 0.92),
            ConsumableKind::MedKit => Vec3::new(0.96, 0.96, 0.96),
            ConsumableKind::ShieldSmall => Vec3::new(0.45, 0.75, 1.0),
            ConsumableKind::ShieldBig => Vec3::new(0.25, 0.55, 1.0),
            ConsumableKind::ChugJug => Vec3::new(0.62, 0.45, 1.0),
        }
    }
    /// Default rarity colour used for beams / backgrounds.
    pub fn rarity(self) -> Rarity {
        match self {
            ConsumableKind::Bandage => Rarity::Common,
            ConsumableKind::ShieldSmall => Rarity::Uncommon,
            ConsumableKind::MedKit => Rarity::Uncommon,
            ConsumableKind::ShieldBig => Rarity::Rare,
            ConsumableKind::ChugJug => Rarity::Epic,
        }
    }
}

pub static CONSUMABLES: [ConsumableDef; 5] = [
    ConsumableDef { name: "Bandage", heal: 15.0, shield: 0.0, max_hp: 75.0, max_shield: 0.0, use_time: 3.4, stack: 15 },
    ConsumableDef { name: "Medkit", heal: 100.0, shield: 0.0, max_hp: 100.0, max_shield: 0.0, use_time: 7.5, stack: 3 },
    ConsumableDef { name: "Mini Shield", heal: 0.0, shield: 25.0, max_hp: 0.0, max_shield: 50.0, use_time: 2.4, stack: 6 },
    ConsumableDef { name: "Shield Potion", heal: 0.0, shield: 50.0, max_hp: 0.0, max_shield: 100.0, use_time: 4.8, stack: 2 },
    ConsumableDef { name: "Chug Jug", heal: 100.0, shield: 100.0, max_hp: 100.0, max_shield: 100.0, use_time: 10.0, stack: 1 },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mat {
    Wood,
    Stone,
    Metal,
}

impl Mat {
    pub const ALL: [Mat; 3] = [Mat::Wood, Mat::Stone, Mat::Metal];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        ["Wood", "Stone", "Metal"][self as usize]
    }
    pub fn color(self) -> Vec3 {
        [Vec3::new(0.86, 0.62, 0.30), Vec3::new(0.62, 0.64, 0.68), Vec3::new(0.50, 0.62, 0.80)][self as usize]
    }
    /// Hit points of a full-health piece.
    pub fn piece_hp(self) -> f32 {
        [160.0, 260.0, 380.0][self as usize]
    }
}

/// What lies in an inventory slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Item {
    Pickaxe,
    Weapon { kind: WeaponKind, rarity: Rarity, ammo: u32 },
    Consumable { kind: ConsumableKind, count: u32 },
}

impl Item {
    pub fn rarity(&self) -> Rarity {
        match self {
            Item::Pickaxe => Rarity::Common,
            Item::Weapon { rarity, .. } => *rarity,
            Item::Consumable { kind, .. } => kind.rarity(),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Item::Pickaxe => "Harvesting Tool",
            Item::Weapon { kind, .. } => kind.name(),
            Item::Consumable { kind, .. } => kind.name(),
        }
    }
}

// ---- loot tables -------------------------------------------------------------------

pub fn roll_rarity(rng: &mut Rng, boost: f32) -> Rarity {
    // base weights favour common; a boost shifts weight towards the top end
    let w = [46.0 - boost * 30.0, 28.0 - boost * 6.0, 16.0 + boost * 12.0, 8.0 + boost * 14.0, 2.0 + boost * 10.0];
    Rarity::from_index(rng.weighted(&w))
}

pub fn roll_weapon(rng: &mut Rng) -> WeaponKind {
    let w = [16.0, 20.0, 30.0, 20.0, 8.0, 4.0];
    WeaponKind::ALL[rng.weighted(&w)]
}

pub fn roll_consumable(rng: &mut Rng) -> ConsumableKind {
    let w = [42.0, 14.0, 26.0, 12.0, 2.5];
    ConsumableKind::ALL[rng.weighted(&w)]
}

/// A loot drop description produced by the tables (the game turns it into a pickup).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drop {
    Weapon { kind: WeaponKind, rarity: Rarity },
    Ammo { kind: AmmoKind, amount: u32 },
    Consumable { kind: ConsumableKind, count: u32 },
}

pub fn roll_floor_loot(rng: &mut Rng) -> Drop {
    let r = rng.f32();
    if r < 0.42 {
        Drop::Weapon { kind: roll_weapon(rng), rarity: roll_rarity(rng, 0.0) }
    } else if r < 0.68 {
        let kind = *rng.pick(&AmmoKind::ALL[..4]);
        Drop::Ammo { kind, amount: kind.box_amount() }
    } else {
        let kind = roll_consumable(rng);
        let count = match kind {
            ConsumableKind::Bandage => rng.range_i(3, 7) as u32,
            ConsumableKind::ShieldSmall => rng.range_i(1, 3) as u32,
            _ => 1,
        };
        Drop::Consumable { kind, count }
    }
}

/// Chest loot: a stronger weapon, ammo for it, and a healing item.
pub fn roll_chest_loot(rng: &mut Rng) -> Vec<Drop> {
    let kind = roll_weapon(rng);
    let rarity = roll_rarity(rng, 0.8).max(Rarity::Uncommon);
    let mut v = vec![Drop::Weapon { kind, rarity }];
    let ak = kind.def().ammo;
    v.push(Drop::Ammo { kind: ak, amount: ak.box_amount() });
    let c = if rng.chance(0.55) { ConsumableKind::ShieldBig } else { roll_consumable(rng) };
    let count = if c == ConsumableKind::Bandage { 5 } else { 1 };
    v.push(Drop::Consumable { kind: c, count });
    if rng.chance(0.35) {
        v.push(Drop::Weapon { kind: roll_weapon(rng), rarity: roll_rarity(rng, 0.5) });
    }
    v
}

/// Supply drop loot: two different strong weapons (epic at least) with plenty of ammo, and the best healing there is.
pub fn roll_supply_loot(rng: &mut Rng) -> Vec<Drop> {
    // no pistols from the sky
    let weights = [0.0, 1.5, 3.0, 2.5, 2.0, 1.5];
    let first = WeaponKind::ALL[rng.weighted(&weights)];
    let mut second = WeaponKind::ALL[rng.weighted(&weights)];
    for _ in 0..8 {
        if second != first {
            break;
        }
        second = WeaponKind::ALL[rng.weighted(&weights)];
    }
    let mut v = vec![];
    for kind in [first, second] {
        let rarity = if rng.chance(0.4) { Rarity::Legendary } else { Rarity::Epic };
        v.push(Drop::Weapon { kind, rarity });
    }
    for kind in [first, second] {
        let ak = kind.def().ammo;
        // a double box, but not past what a player may carry
        v.push(Drop::Ammo { kind: ak, amount: (ak.box_amount() * 2).min(ak.cap()) });
    }
    if rng.chance(0.6) {
        v.push(Drop::Consumable { kind: ConsumableKind::ChugJug, count: 1 });
    } else {
        v.push(Drop::Consumable { kind: ConsumableKind::ShieldBig, count: 2 });
    }
    v.push(Drop::Consumable { kind: ConsumableKind::MedKit, count: 1 });
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supply_loot_is_the_best_in_the_game() {
        let mut rng = Rng::new(5);
        let (mut kinds, mut legendary) = (std::collections::HashSet::new(), 0);
        for _ in 0..300 {
            let loot = roll_supply_loot(&mut rng);
            let weapons: Vec<(WeaponKind, Rarity)> = loot.iter().filter_map(|d| if let Drop::Weapon { kind, rarity } = d { Some((*kind, *rarity)) } else { None }).collect();
            assert_eq!(weapons.len(), 2);
            assert_ne!(weapons[0].0, weapons[1].0, "two different weapons");
            for (kind, rarity) in &weapons {
                assert!(*rarity >= Rarity::Epic, "{kind:?} {rarity:?}");
                assert_ne!(*kind, WeaponKind::Pistol);
                kinds.insert(*kind);
                legendary += (*rarity == Rarity::Legendary) as u32;
                // and ammo to feed it
                let ak = kind.def().ammo;
                assert!(loot.iter().any(|d| matches!(d, Drop::Ammo { kind, amount } if *kind == ak && *amount > ak.box_amount().min(ak.cap()) / 2)), "no ammo for {kind:?}");
            }
            assert!(loot.iter().any(|d| matches!(d, Drop::Consumable { kind: ConsumableKind::MedKit, .. })));
            assert!(loot.iter().any(|d| matches!(d, Drop::Consumable { kind: ConsumableKind::ChugJug | ConsumableKind::ShieldBig, .. })));
        }
        assert!(kinds.len() >= 4, "{kinds:?}");
        assert!(legendary > 100 && legendary < 400, "{legendary} legendary weapons in 600");
    }

    #[test]
    fn weapon_table_matches_enum_order() {
        for (i, k) in WeaponKind::ALL.iter().enumerate() {
            assert_eq!(*k as usize, i);
        }
        for k in WeaponKind::ALL {
            let d = k.def();
            assert!(d.damage > 0.0 && d.rate > 0.0 && d.mag > 0 && d.reload > 0.0, "{}", d.name);
            assert!(d.spread_hip >= d.spread_ads);
            assert!(d.range > d.falloff_start || d.projectile_speed > 0.0 || d.falloff_min >= 1.0);
        }
        assert!(WeaponKind::Sniper.def().scope && !WeaponKind::AssaultRifle.def().scope);
        // the shotgun's pellets add up to a respectable close range hit
        let sg = WeaponKind::Shotgun.def();
        assert!(sg.damage * sg.pellets as f32 >= 90.0);
    }

    #[test]
    fn rarity_scales_damage_monotonically() {
        let mut prev = 0.0;
        for r in Rarity::ALL {
            assert!(r.damage_mul() > prev);
            prev = r.damage_mul();
        }
    }

    #[test]
    fn loot_tables_produce_everything_eventually() {
        let mut rng = Rng::new(1);
        let mut weapons = std::collections::HashSet::new();
        let mut rarities = std::collections::HashSet::new();
        for _ in 0..4000 {
            if let Drop::Weapon { kind, rarity } = roll_floor_loot(&mut rng) {
                weapons.insert(kind);
                rarities.insert(rarity);
            }
        }
        assert_eq!(weapons.len(), 6);
        assert!(rarities.len() >= 4);
        for _ in 0..200 {
            let c = roll_chest_loot(&mut rng);
            assert!(matches!(c[0], Drop::Weapon { .. }));
            assert!(c.len() >= 3);
        }
    }

    #[test]
    fn consumables_make_sense() {
        for k in ConsumableKind::ALL {
            let d = k.def();
            assert!(d.use_time > 1.0 && d.stack >= 1);
            assert!(d.heal > 0.0 || d.shield > 0.0);
        }
        // bandages cannot exceed 75 hp; shield potion caps at 100
        assert_eq!(ConsumableKind::Bandage.def().max_hp, 75.0);
        assert_eq!(ConsumableKind::ShieldBig.def().max_shield, 100.0);
    }
}
