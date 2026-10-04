//! A remote player's input, one simulation step at a time.
//!
//! A client sends one [`Cmd`] for every step it simulates locally (its own prediction). The host applies the same commands,
//! with the same durations, to that player's actor, so both sides walk the same path and the host only has to correct the
//! client when something it could not know about got in the way (another player's wall, a rocket's knock-back, ...).
//! Aim angles are absolute rather than deltas, so a lost command never leaves the host aiming somewhere else.

use super::actor::PieceKind;
use super::intent::Intent;
use super::PlayerInput;
use crate::math::*;

/// Bits of [`Cmd::buttons`].
pub mod btn {
    pub const JUMP: u32 = 1 << 0;
    pub const SPRINT: u32 = 1 << 1;
    pub const CROUCH: u32 = 1 << 2;
    pub const FIRE: u32 = 1 << 3;
    pub const FIRE_PRESSED: u32 = 1 << 4;
    pub const ADS: u32 = 1 << 5;
    pub const RELOAD: u32 = 1 << 6;
    pub const INTERACT: u32 = 1 << 7;
    pub const DROP: u32 = 1 << 8;
    pub const TOGGLE_BUILD: u32 = 1 << 9;
    pub const PLACE: u32 = 1 << 10;
    pub const CYCLE_MAT: u32 = 1 << 11;
    pub const EXIT_BUS: u32 = 1 << 12;
    pub const DEPLOY: u32 = 1 << 13;
    pub const EMOTE: u32 = 1 << 14;
    /// Every bit that means something; the rest of a received command is ignored.
    pub const ALL: u32 = (1 << 15) - 1;
}

/// The longest step a command may ask for (a client never simulates a step longer than 1/60 s).
pub const MAX_CMD_DT: f32 = 1.0 / 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cmd {
    /// Counts up by one per command; the host acknowledges the last one it applied.
    pub seq: u32,
    /// Seconds this command covers.
    pub dt: f32,
    /// Strafe right and forward, -127..=127 for -1..=1.
    pub axis: [i8; 2],
    pub yaw: f32,
    pub pitch: f32,
    pub buttons: u32,
    /// Inventory slot to select, or [`Cmd::NONE`].
    pub select: u8,
    /// Mouse-wheel cycling: +1 next, -1 previous.
    pub cycle: i8,
    /// Index into [`PieceKind::ALL`] of a building piece to choose, or [`Cmd::NONE`].
    pub piece: u8,
    /// The host's clock at the moment the player was looking at the screen that this command answers (0 if unknown): what
    /// the host rewinds the other actors to when the command fires a weapon, so a shot lands where it looked like it would.
    pub view: f32,
}

impl Default for Cmd {
    fn default() -> Self {
        Cmd { seq: 0, dt: 1.0 / 60.0, axis: [0; 2], yaw: 0.0, pitch: 0.0, buttons: 0, select: Cmd::NONE, cycle: 0, piece: Cmd::NONE, view: 0.0 }
    }
}

impl Cmd {
    pub const NONE: u8 = 255;

    /// What a player's raw input for one step comes to, given where they now look.
    pub fn from_input(seq: u32, dt: f32, input: &PlayerInput, yaw: f32, pitch: f32) -> Cmd {
        let q = |v: f32| (v.clamp(-1.0, 1.0) * 127.0).round() as i8;
        let mut buttons = 0;
        for (on, bit) in [
            (input.jump, btn::JUMP),
            (input.sprint, btn::SPRINT),
            (input.crouch, btn::CROUCH),
            (input.fire, btn::FIRE),
            (input.fire_pressed, btn::FIRE_PRESSED),
            (input.ads, btn::ADS),
            (input.reload, btn::RELOAD),
            (input.interact, btn::INTERACT),
            (input.drop_selected, btn::DROP),
            (input.toggle_build, btn::TOGGLE_BUILD),
            (input.place, btn::PLACE),
            (input.cycle_mat, btn::CYCLE_MAT),
            (input.exit_bus, btn::EXIT_BUS),
            (input.deploy, btn::DEPLOY),
            (input.emote, btn::EMOTE),
        ] {
            if on {
                buttons |= bit;
            }
        }
        Cmd {
            seq,
            dt,
            axis: [q(input.move_axis.x), q(input.move_axis.y)],
            yaw,
            pitch,
            buttons,
            select: input.select.map_or(Cmd::NONE, |s| s.min(5) as u8),
            cycle: input.cycle.clamp(-1, 1) as i8,
            piece: input.piece.map_or(Cmd::NONE, |p| PieceKind::ALL.iter().position(|&k| k == p).unwrap_or(0) as u8),
            view: 0.0,
        }
    }

    /// Whether the command is fit to simulate: a hostile client must not be able to feed the simulation NaNs or a huge step.
    pub fn is_sane(&self) -> bool {
        self.dt.is_finite() && self.dt > 0.0 && self.dt <= MAX_CMD_DT && self.yaw.is_finite() && self.pitch.is_finite() && self.view.is_finite()
    }

    /// The intent that both the client (predicting) and the host (applying) run.
    pub fn to_intent(&self) -> Intent {
        let on = |b: u32| self.buttons & b != 0;
        let (f, r) = (yaw_forward(self.yaw), yaw_right(self.yaw));
        let (mx, my) = (self.axis[0] as f32 / 127.0, self.axis[1] as f32 / 127.0);
        Intent {
            wish: Vec2::new(f.x * my + r.x * mx, f.z * my + r.z * mx),
            yaw: self.yaw,
            pitch: self.pitch,
            jump: on(btn::JUMP),
            sprint: on(btn::SPRINT),
            crouch: on(btn::CROUCH),
            fire: on(btn::FIRE),
            fire_pressed: on(btn::FIRE_PRESSED),
            ads: on(btn::ADS),
            reload: on(btn::RELOAD),
            interact: on(btn::INTERACT),
            select: (self.select < 6).then_some(self.select as usize),
            cycle: self.cycle.clamp(-1, 1) as i32,
            drop_selected: on(btn::DROP),
            toggle_build: on(btn::TOGGLE_BUILD),
            piece: PieceKind::ALL.get(self.piece as usize).copied(),
            place: on(btn::PLACE),
            cycle_mat: on(btn::CYCLE_MAT),
            exit_bus: on(btn::EXIT_BUS),
            deploy: on(btn::DEPLOY),
            emote: on(btn::EMOTE),
        }
    }
}

/// Is `a` later than `b` in the wrapping sequence of command numbers?
pub fn seq_after(a: u32, b: u32) -> bool {
    a != b && a.wrapping_sub(b) < 0x8000_0000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_carries_the_same_intent_as_the_input_it_came_from() {
        let input = PlayerInput {
            move_axis: Vec2::new(1.0, -1.0),
            jump: true,
            sprint: true,
            fire: true,
            fire_pressed: true,
            select: Some(3),
            cycle: -1,
            piece: Some(PieceKind::Ramp),
            place: true,
            emote: true,
            ..Default::default()
        };
        let (yaw, pitch) = (2.3, -0.4);
        let it = Cmd::from_input(7, 1.0 / 60.0, &input, yaw, pitch).to_intent();
        let (f, r) = (yaw_forward(yaw), yaw_right(yaw));
        assert!((it.wish - Vec2::new(r.x - f.x, r.z - f.z)).length() < 1e-6);
        assert_eq!((it.yaw, it.pitch), (yaw, pitch));
        assert!(it.jump && it.sprint && it.fire && it.fire_pressed && it.place && it.emote);
        assert!(!it.crouch && !it.ads && !it.reload && !it.interact && !it.drop_selected && !it.toggle_build && !it.cycle_mat && !it.exit_bus && !it.deploy);
        assert_eq!((it.select, it.cycle, it.piece), (Some(3), -1, Some(PieceKind::Ramp)));
        let idle = Cmd::from_input(8, 0.01, &PlayerInput::default(), 0.0, 0.0).to_intent();
        assert_eq!((idle.select, idle.piece, idle.cycle), (None, None, 0));
        assert!(idle.wish.length() < 1e-6);
    }

    #[test]
    fn insane_commands_are_refused() {
        let ok = Cmd::default();
        assert!(ok.is_sane());
        for bad in [Cmd { dt: 0.0, ..ok }, Cmd { dt: -0.1, ..ok }, Cmd { dt: 5.0, ..ok }, Cmd { dt: f32::NAN, ..ok }, Cmd { yaw: f32::INFINITY, ..ok }, Cmd { pitch: f32::NAN, ..ok }] {
            assert!(!bad.is_sane(), "{bad:?}");
        }
    }

    #[test]
    fn sequence_numbers_wrap() {
        assert!(seq_after(5, 4));
        assert!(!seq_after(4, 5));
        assert!(!seq_after(4, 4));
        assert!(seq_after(0, u32::MAX));
        assert!(!seq_after(u32::MAX, 0));
    }
}
