//! Things that happened during a tick, for audio, particles and the HUD.

use super::actor::PieceKind;
use super::items::*;
use crate::math::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Grass,
    Sand,
    Stone,
    Wood,
    Metal,
    Water,
}

impl Surface {
    pub const ALL: [Surface; 6] = [Surface::Grass, Surface::Sand, Surface::Stone, Surface::Wood, Surface::Metal, Surface::Water];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImpactKind {
    Dirt,
    Stone,
    Wood,
    Metal,
    Flesh,
    Shield,
    Foliage,
    Water,
}

impl ImpactKind {
    pub const ALL: [ImpactKind; 8] = [ImpactKind::Dirt, ImpactKind::Stone, ImpactKind::Wood, ImpactKind::Metal, ImpactKind::Flesh, ImpactKind::Shield, ImpactKind::Foliage, ImpactKind::Water];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickupSound {
    Weapon,
    Ammo,
    Heal,
    Material,
}

impl PickupSound {
    pub const ALL: [PickupSound; 4] = [PickupSound::Weapon, PickupSound::Ammo, PickupSound::Heal, PickupSound::Material];
}

#[derive(Clone, Debug)]
pub enum Event {
    Footstep { actor: usize, pos: Vec3, surface: Surface },
    Jump { actor: usize, pos: Vec3 },
    Land { actor: usize, pos: Vec3, speed: f32, surface: Surface },
    Shot { actor: usize, pos: Vec3, weapon: WeaponKind, end: Vec3, hit_actor: bool },
    Impact { pos: Vec3, normal: Vec3, kind: ImpactKind },
    /// A bullet trail to draw from the muzzle to where the shot ended.
    Tracer { from: Vec3, to: Vec3, weapon: WeaponKind },
    Damage { target: usize, attacker: Option<usize>, amount: f32, on_shield: bool, headshot: bool, pos: Vec3 },
    /// `actor` landed a hit (for their hit marker and its sound).
    HitConfirm { actor: usize, head: bool, shield: bool, kill: bool },
    /// `actor` was hit; `from` is the attacker position (damage indicator).
    Hurt { actor: usize, amount: f32, from: Option<Vec3> },
    /// A message for the HUD: for one player, or for everyone when `actor` is `None`.
    Toast { actor: Option<usize>, text: String, secs: f32, style: u8 },
    Eliminated { victim: usize, killer: Option<usize>, weapon: Option<&'static str>, storm: bool },
    Reload { actor: usize, pos: Vec3, kind: WeaponKind },
    EmptyClick { actor: usize, pos: Vec3 },
    WeaponSwitch { actor: usize, pos: Vec3 },
    Pickup { actor: usize, pos: Vec3, sound: PickupSound, name: &'static str, rarity: Rarity, count: u32 },
    ChestOpen { pos: Vec3 },
    Harvest { actor: usize, pos: Vec3, mat: Mat, amount: u32 },
    HarvestHit { pos: Vec3, normal: Vec3, wood: bool },
    TreeFelled { pos: Vec3, chunk: usize, slot: usize },
    Built { actor: usize, pos: Vec3, piece: PieceKind, mat: Mat },
    PieceDestroyed { pos: Vec3, mat: Mat },
    Explosion { pos: Vec3, radius: f32 },
    BusJump { actor: usize, pos: Vec3 },
    GliderDeploy { actor: usize, pos: Vec3 },
    HealStart { actor: usize, pos: Vec3 },
    HealDone { actor: usize, pos: Vec3 },
    StormPhase { phase: usize, shrinking: bool },
    Victory { winner: usize },
    /// Something loud happened (bots hear it).
    Noise { pos: Vec3, radius: f32, source: usize },
}
