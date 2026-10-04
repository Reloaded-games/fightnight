//! Multiplayer: one player hosts the room, everyone else plays a copy of the match that the host keeps up to date.
//!
//! The host is the authority. It runs the whole simulation, with the people in the room as extra human actors next to the
//! bots. Each client runs its own copy of the match: its own actor is predicted locally from its own commands (so the game
//! answers at once), everything else is shown from the host's snapshots, a little in the past, interpolated.
//!
//! This module does not touch the network: it produces and consumes byte messages, and whatever carries them (WebRTC data
//! channels in the browser, plain function calls in the tests) is somebody else's business.

pub mod client;
pub mod guest;
pub mod host;
pub mod proto;
pub mod wire;

#[cfg(test)]
mod tests;
