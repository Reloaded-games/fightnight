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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickupSound {
    Weapon,
    Ammo,
    Heal,
    Material,
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
    /// The local player landed a hit (for the hit marker and its sound).
    HitConfirm { head: bool, shield: bool, kill: bool },
    /// The local player was hit; `from` is the attacker position (damage indicator).
    Hurt { amount: f32, from: Option<Vec3> },
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
