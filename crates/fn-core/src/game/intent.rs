//! What an actor wants to do this tick. The human player's keyboard and mouse and
//! the bots' brains both produce an `Intent`; the simulation treats them the same.

use super::actor::PieceKind;
use crate::math::*;

#[derive(Clone, Copy, Debug, Default)]
pub struct Intent {
    /// Desired movement direction in world space (length <= 1).
    pub wish: Vec2,
    /// Aim angles (set absolutely each tick).
    pub yaw: f32,
    pub pitch: f32,
    pub jump: bool,
    pub sprint: bool,
    pub crouch: bool,
    /// Trigger held / pressed this tick.
    pub fire: bool,
    pub fire_pressed: bool,
    pub ads: bool,
    pub reload: bool,
    /// Pickup / open chest.
    pub interact: bool,
    /// Select an inventory slot (0 = pickaxe).
    pub select: Option<usize>,
    /// Mouse wheel / cycle (+1 next, -1 previous).
    pub cycle: i32,
    pub drop_selected: bool,
    pub toggle_build: bool,
    pub piece: Option<PieceKind>,
    pub place: bool,
    pub cycle_mat: bool,
    /// Leave the bus / deploy or cut the glider.
    pub exit_bus: bool,
    pub deploy: bool,
}
