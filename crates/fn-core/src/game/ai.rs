//! Bot brains (stub; the full behaviour tree is written after the combat layer is verified).

use super::intent::Intent;
use super::*;

#[derive(Clone, Debug, Default)]
pub struct Brain {
    pub last_attacker: Option<usize>,
}

impl Brain {
    pub fn new(_rng: &mut crate::rng::Rng, _d: Difficulty) -> Brain {
        Brain::default()
    }
    pub fn on_damaged(&mut self, attacker: usize) {
        self.last_attacker = Some(attacker);
    }
}

pub fn think(g: &mut Game, i: usize, _dt: f32) -> Intent {
    let a = &g.actors[i];
    Intent { yaw: a.yaw, pitch: a.pitch, ..Default::default() }
}
