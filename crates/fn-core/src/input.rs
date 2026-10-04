//! Keyboard and mouse state, turned into the simulation's `PlayerInput` once per frame.

use crate::game::actor::PieceKind;
use crate::game::PlayerInput;
use crate::math::Vec2;

#[derive(Default)]
pub struct Input {
    fwd: bool,
    back: bool,
    left: bool,
    right: bool,
    sprint: bool,
    crouch: bool,
    jump: bool,
    fire: bool,
    ads: bool,
    fire_edge: bool,
    jump_edge: bool,
    reload_edge: bool,
    interact_edge: bool,
    drop_edge: bool,
    build_edge: bool,
    mat_edge: bool,
    emote_edge: bool,
    select: Option<usize>,
    piece: Option<PieceKind>,
    cycle: i32,
    spectate: i32,
    look: Vec2,
    pub auto_sprint: bool,
    drop_delay: u8,
}

impl Input {
    pub fn new() -> Input {
        Input { auto_sprint: true, ..Default::default() }
    }

    /// Forget everything that is held (when the window loses focus or a menu opens).
    pub fn release_all(&mut self) {
        let auto = self.auto_sprint;
        *self = Input::new();
        self.auto_sprint = auto;
    }

    pub fn key(&mut self, code: &str, down: bool) {
        match code {
            "KeyW" | "ArrowUp" => self.fwd = down,
            "KeyS" | "ArrowDown" => self.back = down,
            "KeyA" => self.left = down,
            "KeyD" => self.right = down,
            "ArrowLeft" => {
                self.left = down;
                if down {
                    self.spectate = -1;
                }
            }
            "ArrowRight" => {
                self.right = down;
                if down {
                    self.spectate = 1;
                }
            }
            "ShiftLeft" | "ShiftRight" => self.sprint = down,
            // Ctrl is the Fortnite default, but Ctrl+W and Ctrl+1..6 are browser shortcuts a page cannot cancel
            "ControlLeft" | "ControlRight" | "KeyF" => self.crouch = down,
            "Space" => {
                if down && !self.jump {
                    self.jump_edge = true;
                }
                self.jump = down;
            }
            "KeyR" if down => self.reload_edge = true,
            "KeyE" if down => self.interact_edge = true,
            "KeyG" if down => self.drop_edge = true,
            "KeyQ" if down => self.build_edge = true,
            "KeyT" if down => self.mat_edge = true,
            "KeyB" if down => self.emote_edge = true,
            "KeyZ" if down => self.piece = Some(PieceKind::Wall),
            "KeyX" if down => self.piece = Some(PieceKind::Floor),
            "KeyC" if down => self.piece = Some(PieceKind::Ramp),
            "KeyV" if down => self.piece = Some(PieceKind::Roof),
            "Digit1" if down => self.select = Some(0),
            "Digit2" if down => self.select = Some(1),
            "Digit3" if down => self.select = Some(2),
            "Digit4" if down => self.select = Some(3),
            "Digit5" if down => self.select = Some(4),
            "Digit6" if down => self.select = Some(5),
            _ => {}
        }
    }

    /// Mouse buttons: 0 left (fire / place), 2 right (aim).
    pub fn button(&mut self, b: i32, down: bool) {
        match b {
            0 => {
                if down && !self.fire {
                    self.fire_edge = true;
                    self.spectate = 1;
                }
                self.fire = down;
            }
            2 => self.ads = down,
            _ => {}
        }
    }

    /// Equip a slot (inventory screen).
    pub fn select_slot(&mut self, slot: usize) {
        self.select = Some(slot.min(5));
    }

    /// Drop the item in a slot: it is equipped first, then dropped two frames later.
    pub fn drop_slot(&mut self, slot: usize) {
        self.select = Some(slot.min(5));
        self.drop_delay = 2;
    }

    pub fn mouse_move(&mut self, dx: f32, dy: f32) {
        self.look += Vec2::new(dx, dy);
    }

    pub fn wheel(&mut self, dy: f32) {
        if dy > 0.0 {
            self.cycle += 1;
        } else if dy < 0.0 {
            self.cycle -= 1;
        }
    }

    /// Driving uses the physical Shift key for boost, independent of automatic sprint on foot.
    pub fn take_driving(&mut self, sens: f32, invert_y: bool) -> PlayerInput {
        let auto = self.auto_sprint;
        self.auto_sprint = false;
        let input = self.take(sens, invert_y);
        self.auto_sprint = auto;
        input
    }

    /// Consume the accumulated state. `sens` is radians per pixel.
    pub fn take(&mut self, sens: f32, invert_y: bool) -> PlayerInput {
        let mut mv = Vec2::new((self.right as i32 - self.left as i32) as f32, (self.fwd as i32 - self.back as i32) as f32);
        if mv.length_squared() > 1.0 {
            mv = mv.normalize();
        }
        let look = Vec2::new(self.look.x * sens, -self.look.y * sens * if invert_y { -1.0 } else { 1.0 });
        self.look = Vec2::ZERO;
        let mut drop = self.drop_edge;
        if self.drop_delay > 0 {
            self.drop_delay -= 1;
            if self.drop_delay == 0 {
                drop = true;
            }
        }
        let sprint = self.sprint || (self.auto_sprint && mv.y > 0.1);
        let pi = PlayerInput {
            move_axis: mv,
            look,
            jump: self.jump,
            sprint,
            crouch: self.crouch,
            fire: self.fire,
            fire_pressed: self.fire_edge,
            ads: self.ads,
            reload: self.reload_edge,
            interact: self.interact_edge,
            select: self.select.take(),
            cycle: std::mem::take(&mut self.cycle),
            drop_selected: drop,
            toggle_build: self.build_edge,
            piece: self.piece.take(),
            place: self.fire,
            cycle_mat: self.mat_edge,
            exit_bus: self.jump_edge || self.fire_edge || self.interact_edge,
            deploy: self.jump_edge || self.fire_edge,
            emote: self.emote_edge,
            spectate: std::mem::take(&mut self.spectate),
        };
        self.fire_edge = false;
        self.jump_edge = false;
        self.reload_edge = false;
        self.interact_edge = false;
        self.drop_edge = false;
        self.build_edge = false;
        self.mat_edge = false;
        self.emote_edge = false;
        pi
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasd_makes_a_normalised_move_axis_and_edges_fire_once() {
        let mut i = Input::new();
        i.key("KeyW", true);
        i.key("KeyD", true);
        let p = i.take(0.002, false);
        assert!((p.move_axis.length() - 1.0).abs() < 1e-5 && p.move_axis.x > 0.0 && p.move_axis.y > 0.0);
        i.button(0, true);
        i.key("KeyR", true);
        let p = i.take(0.002, false);
        assert!(p.fire && p.fire_pressed && p.reload);
        let p = i.take(0.002, false);
        assert!(p.fire && !p.fire_pressed && !p.reload, "held, not re-triggered");
    }

    #[test]
    fn crouch_works_on_ctrl_and_on_f() {
        let mut i = Input::new();
        i.key("ControlLeft", true);
        assert!(i.take(0.002, false).crouch);
        i.key("ControlLeft", false);
        assert!(!i.take(0.002, false).crouch);
        i.key("KeyF", true);
        assert!(i.take(0.002, false).crouch);
        i.key("KeyF", false);
        assert!(!i.take(0.002, false).crouch);
    }

    #[test]
    fn vehicle_boost_uses_shift_and_preserves_auto_sprint_setting() {
        let mut i = Input::new();
        i.key("KeyW", true);
        assert!(!i.take_driving(0.002, false).sprint);
        assert!(i.auto_sprint && i.take(0.002, false).sprint);
        i.key("ShiftLeft", true);
        assert!(i.take_driving(0.002, false).sprint);
    }

    #[test]
    fn b_is_an_edge_triggered_emote() {
        let mut i = Input::new();
        i.key("KeyB", true);
        assert!(i.take(0.002, false).emote);
        assert!(!i.take(0.002, false).emote, "held keys do not repeat the emote");
        i.key("KeyB", false);
        assert!(!i.take(0.002, false).emote);
    }

    #[test]
    fn mouse_look_scales_and_inverts() {
        let mut i = Input::new();
        i.mouse_move(100.0, 50.0);
        let p = i.take(0.002, false);
        assert!((p.look.x - 0.2).abs() < 1e-5 && (p.look.y + 0.1).abs() < 1e-5, "{:?}", p.look);
        i.mouse_move(100.0, 50.0);
        let p = i.take(0.002, true);
        assert!(p.look.y > 0.0);
        assert_eq!(i.take(0.002, false).look, Vec2::ZERO);
    }

    #[test]
    fn every_documented_key_maps_to_its_action() {
        // the README controls table, key by key
        let mut i = Input::new();
        for (code, piece) in [("KeyZ", PieceKind::Wall), ("KeyX", PieceKind::Floor), ("KeyC", PieceKind::Ramp), ("KeyV", PieceKind::Roof)] {
            i.key(code, true);
            assert_eq!(i.take(0.002, false).piece, Some(piece), "{code}");
            i.key(code, false);
        }
        for (n, code) in ["Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6"].into_iter().enumerate() {
            i.key(code, true);
            assert_eq!(i.take(0.002, false).select, Some(n), "{code}");
            i.key(code, false);
        }
        i.key("KeyQ", true);
        i.key("KeyT", true);
        i.key("KeyE", true);
        i.key("KeyG", true);
        let p = i.take(0.002, false);
        assert!(p.toggle_build && p.cycle_mat && p.interact && p.drop_selected);
        assert!(!i.take(0.002, false).toggle_build, "edges fire once");
        i.key("Space", true);
        let p = i.take(0.002, false);
        assert!(p.jump && p.exit_bus && p.deploy, "Space jumps, leaves the bus and opens the glider");
        i.key("Space", false);
        i.key("ShiftLeft", true);
        assert!(i.take(0.002, false).sprint);
        i.button(2, true);
        assert!(i.take(0.002, false).ads);
        i.wheel(120.0);
        assert_eq!(i.take(0.002, false).cycle, 1);
        i.wheel(-120.0);
        assert_eq!(i.take(0.002, false).cycle, -1);
    }

    #[test]
    fn build_and_slot_keys() {
        let mut i = Input::new();
        i.key("KeyZ", true);
        i.key("Digit3", true);
        let p = i.take(0.002, false);
        assert_eq!(p.piece, Some(PieceKind::Wall));
        assert_eq!(p.select, Some(2));
        assert!(i.take(0.002, false).piece.is_none());
    }
}
