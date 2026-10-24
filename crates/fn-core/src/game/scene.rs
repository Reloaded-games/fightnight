//! Turns the game state into draw lists: instanced meshes grouped by model, translucent
//! ghosts for the building preview, and particles. The renderer just consumes the result.

use super::actor::*;
use super::items::*;
use super::rig;
use super::*;
use crate::camera::{Camera, Frustum};
use crate::math::*;
use crate::mesh::{shape, Instance, MeshData, Particle};
use crate::meshlib::{MeshId, ALL_IDS};

/// A contiguous run of instances drawn with one mesh.
#[derive(Clone, Copy, Debug)]
pub struct Batch {
    pub mesh: u16,
    pub first: u32,
    pub count: u32,
    pub shadow: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SceneStats {
    pub characters: u32,
    pub pickups: u32,
    pub pieces: u32,
    pub instances: u32,
    pub particles: u32,
}

pub struct Scene {
    mode: GameMode,
    lists: Vec<Vec<Instance>>,
    /// Bounding-box centre and half extents per mesh (floor items are centred on their pivot and sized to be seen).
    centers: Vec<(Vec3, Vec3)>,
    ghost: Vec<Instance>,
    ghost_mesh: Option<u16>,
    pub instances: Vec<Instance>,
    pub batches: Vec<Batch>,
    pub ghost_instances: Vec<Instance>,
    pub ghost_batches: Vec<Batch>,
    pub particles: Vec<Particle>,
    pub particles_add: Vec<Particle>,
    pub stats: SceneStats,
    /// Where each actor stood on the bus's roof when the scene was last built, to catch who has just jumped.
    aboard: Vec<Option<RiderSpot>>,
    /// Riders who have just jumped, on their way from the roof to where the simulation has them.
    handoffs: Vec<Handoff>,
    /// Game time of the last build: going back means a new match.
    built_at: f32,
}

/// sRGB-ish colour to the linear tint the shader multiplies with.
pub fn lin(c: Vec3) -> [f32; 4] {
    [c.x.max(0.0).powf(2.2), c.y.max(0.0).powf(2.2), c.z.max(0.0).powf(2.2), 1.0]
}

fn casts_shadow(m: MeshId) -> bool {
    use MeshId::*;
    matches!(
        m,
        CharTorso | CharTrim | CharPelvis | CharHead | CharArmUp | CharArmLow | CharLegUp | CharLegLow | CharBoot | CharBackpack | Glider | PieceWall | PieceFloor | PieceRamp | PieceRoof | ChestBase | ChestLid | SupplyCrate | SupplyChute | AmmoBoxBase | AmmoBoxLid | Llama | LaunchPadBase | LaunchPadPlate | Bus | Hair1 | Hair2 | Hair3 | Cap | Beanie | Helmet | Hat | VehicleBody | VehicleWheel | VehicleSport
    )
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Colour multiplier for a build material (what the player sees on walls, ramps and floors).
pub fn material_tint(m: Mat) -> Vec3 {
    match m {
        Mat::Wood => Vec3::new(0.86, 0.62, 0.33),
        Mat::Stone => Vec3::new(0.78, 0.62, 0.52),
        Mat::Metal => Vec3::new(0.58, 0.68, 0.82),
    }
}

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// How long (metres) a healing item or ammo box lying on the ground reads on screen, whatever size its mesh was modelled at:
/// a bandage is 12 cm long as a prop, which is a speck from the camera.
pub fn floor_item_size(kind: &PickupKind) -> f32 {
    match kind {
        PickupKind::Weapon { .. } => 0.0,
        PickupKind::Ammo { .. } => 0.5,
        PickupKind::Mats { .. } => 0.6,
        PickupKind::Utility { kind, .. } => {
            if kind.def().thrown {
                0.42
            } else {
                0.75
            }
        }
        PickupKind::Consumable { kind, .. } => match kind {
            ConsumableKind::Bandage => 0.5,
            ConsumableKind::MedKit => 0.62,
            ConsumableKind::ShieldSmall => 0.5,
            ConsumableKind::ShieldBig => 0.58,
            ConsumableKind::ChugJug => 0.66,
            ConsumableKind::Slurp => 0.55,
        },
    }
}

/// How big a utility item looks in the hand, and in flight.
fn held_utility_size(kind: UtilityKind) -> f32 {
    if kind.def().thrown {
        0.17
    } else {
        0.62
    }
}
const THROWN_SIZE: f32 = 0.26;

/// How long a healing item is in the player's hand.
pub const HELD_ITEM_SIZE: f32 = 0.3;

/// Scale that makes a mesh with half extents `half` about `target` metres across its longest side.
pub fn fit_scale(half: Vec3, target: f32) -> f32 {
    let longest = (half.max_element() * 2.0).max(0.02);
    (target / longest).clamp(0.25, 8.0)
}

/// Where the Battle Bus is drawn: on its route, bobbing and rocking a little under the balloon.
pub fn bus_matrix(g: &Game) -> Mat4 {
    let t = g.time;
    Mat4::from_translation(g.bus.pos + Vec3::new(0.0, (t * 0.9).sin() * 0.25, 0.0)) * Mat4::from_rotation_y(yaw_of(g.bus.dir)) * Mat4::from_rotation_z((t * 0.7).sin() * 0.025) * Mat4::from_rotation_x((t * 0.5).sin() * 0.015)
}

/// How many riders fit on the roof of the bus: the player and nine rows of two. Anyone past that rides inside, out of
/// sight, until they jump (and drops out of the bottom of the bus).
pub const ROOF_RIDERS: usize = 19;

/// The two files of riders stand this far either side of the middle of the roof: far enough apart that no two neighbours
/// can reach each other in any pose, and each next to an edge of the roof, so they can jump off it without passing anyone.
const FILE_X: f32 = 0.55;
/// The rows of riders run from just behind the luggage at the front of the roof to the back row, a step in front of the
/// player. Nine rows fit this far apart, which leaves room between them for the deepest pose.
const ROW_FRONT: f32 = -2.85;
const ROW_BACK: f32 = 2.5;
/// Where the player stands: at the back of the roof, a little right of the middle, nearest the camera.
const PLAYER_SPOT: Vec3 = Vec3::new(0.3, crate::models::BUS_ROOF_Y, 3.2);
/// How far out beside the bus a rider who jumps off a side of the roof lands before they drop, clear of the bus, its wheels
/// and its cables; and how far behind its middle the player lands who jumps off the back.
const SIDE_EXIT_X: f32 = 3.0;
const BACK_EXIT_Z: f32 = 5.6;

/// Where someone riding the Battle Bus stands, in the bus's frame (see [`bus_matrix`]), and how.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RiderSpot {
    /// Feet, on the roof.
    pub pos: Vec3,
    /// Turn away from facing the front of the bus.
    pub yaw: f32,
    pub pose: rig::RiderPose,
    /// Where they hop to when they jump, at the height of the roof: out beside the bus off their side of the roof, or
    /// behind it off the back (see [`handoff_path`]).
    pub exit: Vec3,
}

/// The local player stands on their own at the back of the roof, a little right of the middle: nearest the camera, which
/// looks over the bus from behind and over the right shoulder, with nobody behind them. The others stand in two files in
/// front of them, spread out to the front of the roof, in actor order, each with a little scatter and a pose of their own:
/// a waving rider waves with their outer hand. Everyone keeps their spot until they jump, so nobody shuffles round when
/// someone leaves; each jumps off their own side of the roof (the player off the back), where nobody stands in the way.
pub fn bus_rider_spot(g: &Game, i: usize) -> Option<RiderSpot> {
    use rig::RiderPose::*;
    let roof_y = crate::models::BUS_ROOF_Y;
    if i >= g.actors.len() {
        return None;
    }
    if i == g.local {
        let pos = PLAYER_SPOT;
        return Some(RiderSpot { pos, yaw: 0.0, pose: Relaxed, exit: Vec3::new(pos.x, roof_y, BACK_EXIT_Z) });
    }
    // place among the others
    let rank = if i < g.local { i } else { i - 1 };
    let others = (g.actors.len() - 1).min(ROOF_RIDERS - 1);
    if rank >= others {
        return None;
    }
    let rows = others.div_ceil(2);
    let gap = if rows > 1 { ((ROW_BACK - ROW_FRONT) / (rows - 1) as f32).min(1.5) } else { 0.0 };
    let (row, file) = (rank / 2, rank % 2);
    let h = (i as u32).wrapping_mul(0x9E37_79B9) ^ 0x5BD1_E995;
    let u = |shift: u32| ((h >> shift) & 0xff) as f32 / 255.0 - 0.5;
    let side = if file == 0 { 1.0 } else { -1.0 };
    let pos = Vec3::new(side * FILE_X + u(0) * 0.06, roof_y, ROW_BACK - row as f32 * gap + u(8) * 0.06);
    let pose = match [Relaxed, Wave, Cheer, Akimbo][(h >> 24) as usize % 4] {
        Wave if side < 0.0 => WaveLeft,
        p => p,
    };
    Some(RiderSpot { pos, yaw: u(16) * 0.4, pose, exit: Vec3::new(side * SIDE_EXIT_X, roof_y, pos.z) })
}

/// How long someone who jumps off the bus takes to get from their spot on the roof to where the simulation has them.
pub const HANDOFF_TIME: f32 = 1.0;
/// How long the hop off the roof to the exit point takes.
const HOP_TIME: f32 = 0.15;

/// Where someone who jumped off the roof `t` seconds ago is drawn, in the bus's frame, given where the simulation has them
/// (`sim`, their feet) and how fast it moves them (`vel`), both in the bus's frame too. The simulation drops a jumper out of
/// the middle of the bus, inside its body, and the bus flies on away from them, so the drawing makes its own way there
/// without going through the bus or anyone on it: from their spot they hop over the rack to their exit point (out beside
/// the bus, or behind it for the player). Off the roof the wind catches them: they are carried along with the simulation,
/// and what still parts them from it closes on every other axis first and last on the one they left by, once they are
/// behind and below the bus. Before `t = 0` they are on their spot, from [`HANDOFF_TIME`] on at `sim`.
pub fn handoff_path(spot: &RiderSpot, sim: Vec3, vel: Vec3, t: f32) -> Vec3 {
    let hop = |t: f32| spot.pos.lerp(spot.exit, smoothstep(0.03, HOP_TIME, t)) + Vec3::Y * 0.8 * (std::f32::consts::PI * (t / 0.32).clamp(0.0, 1.0)).sin();
    if t <= HOP_TIME {
        return hop(t);
    }
    // how far off the simulation the drawing was when the hop ended, carried along with it since
    let gap = hop(HOP_TIME) - (sim - vel * (t - HOP_TIME));
    let exit_axis = if (spot.exit.x - spot.pos.x).abs() > (spot.exit.z - spot.pos.z).abs() { Vec3::X } else { Vec3::Z };
    let closed = exit_axis * smoothstep(0.55, HANDOFF_TIME, t) + (Vec3::ONE - exit_axis) * smoothstep(HOP_TIME, 0.8, t);
    sim + gap * (Vec3::ONE - closed)
}

/// Someone who has just jumped off the bus, drawn on their way from their spot on the roof to where the simulation has them.
#[derive(Clone, Copy, Debug)]
struct Handoff {
    actor: usize,
    /// Game time of the first frame drawn after the jump.
    start: f32,
    spot: RiderSpot,
}

/// How someone standing on the roof of the bus at `spot` is posed, in boots whose soles are `sole` under the ankle.
fn roof_pose(g: &Game, a: &Actor, spot: &RiderSpot, sole: f32) -> rig::Pose {
    let root = bus_matrix(g) * Mat4::from_translation(spot.pos) * Mat4::from_rotation_y(spot.yaw);
    rig::bus_rider_pose(a, root, yaw_of(g.bus.dir) + spot.yaw, spot.pose, g.time, sole)
}

/// One part of a pose blended between how it sits about the root at `pa` in pose A and about `pb` in pose B (`k` from 0 to
/// 1), about a root at `at`.
fn blend_part(a: Mat4, pa: Vec3, b: Mat4, pb: Vec3, k: f32, at: Vec3) -> Mat4 {
    let (_, ra, ta) = a.to_scale_rotation_translation();
    let (_, rb, tb) = b.to_scale_rotation_translation();
    Mat4::from_rotation_translation(ra.slerp(rb, k), at + (ta - pa).lerp(tb - pb, k))
}

/// How someone who has just jumped off the bus is posed: on the path of [`handoff_path`], turning from how they stood on
/// the roof into how the simulation has them falling.
fn handoff_pose(g: &Game, a: &Actor, h: &Handoff, sole: f32) -> rig::Pose {
    let t = g.time - h.start;
    let bus = bus_matrix(g);
    let to_bus = bus.inverse();
    let vel = to_bus.transform_vector3(a.vel - Vec3::new(g.bus.dir.x, 0.0, g.bus.dir.y) * g.bus.speed);
    let at = bus.transform_point3(handoff_path(&h.spot, to_bus.transform_point3(a.pos), vel, t));
    let (ride, from) = (roof_pose(g, a, &h.spot, sole), bus.transform_point3(h.spot.pos));
    let fall = rig::pose(a);
    let k = smoothstep(0.1, 0.55, t);
    let mix = |x: Mat4, y: Mat4| blend_part(x, from, y, a.pos, k, at);
    let mix2 = |x: [Mat4; 2], y: [Mat4; 2]| [mix(x[0], y[0]), mix(x[1], y[1])];
    let head = mix(ride.head, fall.head);
    rig::Pose {
        torso: mix(ride.torso, fall.torso),
        pelvis: mix(ride.pelvis, fall.pelvis),
        head,
        backpack: mix(ride.backpack, fall.backpack),
        arm_up: mix2(ride.arm_up, fall.arm_up),
        arm_low: mix2(ride.arm_low, fall.arm_low),
        hand: mix2(ride.hand, fall.hand),
        leg_up: mix2(ride.leg_up, fall.leg_up),
        leg_low: mix2(ride.leg_low, fall.leg_low),
        boot: mix2(ride.boot, fall.boot),
        item: None,
        glider: fall.glider.map(|m| blend_part(m, a.pos, m, a.pos, 1.0, at)),
        muzzle: at + (fall.muzzle - a.pos),
        head_pos: head.transform_point3(Vec3::Y * crate::models::dims::HEAD_C),
    }
}

impl Scene {
    pub fn new(meshes: &[MeshData]) -> Scene {
        let centers = meshes
            .iter()
            .map(|m| {
                let b = m.bounds();
                if b.min.is_finite() && b.max.is_finite() {
                    (b.center(), b.half())
                } else {
                    (Vec3::ZERO, Vec3::ZERO)
                }
            })
            .collect();
        Scene {
            mode: GameMode::BattleRoyale,
            lists: (0..MeshId::Count as usize).map(|_| Vec::new()).collect(),
            centers,
            ghost: vec![],
            ghost_mesh: None,
            instances: Vec::with_capacity(4096),
            batches: Vec::with_capacity(128),
            ghost_instances: Vec::new(),
            ghost_batches: Vec::new(),
            particles: Vec::with_capacity(2048),
            particles_add: Vec::with_capacity(2048),
            stats: SceneStats::default(),
            aboard: Vec::new(),
            handoffs: Vec::new(),
            built_at: 0.0,
        }
    }

    /// Notice who has just jumped off the bus (they stood on its roof last time and are falling now) and start drawing
    /// their way off it; forget the ones who have got where the simulation has them.
    fn track_jumpers(&mut self, g: &Game) {
        if g.time < self.built_at || self.aboard.len() != g.actors.len() {
            self.aboard = vec![None; g.actors.len()];
            self.handoffs.clear();
        }
        self.built_at = g.time;
        for (i, a) in g.actors.iter().enumerate() {
            let now = if a.alive && a.mode == MoveMode::Bus && g.bus.active { bus_rider_spot(g, i) } else { None };
            if let (Some(spot), None) = (self.aboard[i], now) {
                if g.bus.active && a.alive && matches!(a.mode, MoveMode::Freefall | MoveMode::Glide) {
                    self.handoffs.retain(|h| h.actor != i);
                    self.handoffs.push(Handoff { actor: i, start: g.time, spot });
                }
            }
            self.aboard[i] = now;
        }
        self.handoffs.retain(|h| {
            let a = &g.actors[h.actor];
            g.bus.active && g.time - h.start < HANDOFF_TIME && a.alive && matches!(a.mode, MoveMode::Freefall | MoveMode::Glide)
        });
    }

    /// How actor `i` is drawn this frame, if at all: standing on the bus's roof, on their way off it after a jump, or where
    /// the simulation has them. (Someone at the wheel of a vehicle is drawn with it instead.)
    pub fn actor_pose(&self, g: &Game, i: usize) -> Option<rig::Pose> {
        let a = g.actors.get(i)?;
        // how far the soles of the boots this game's look draws are under the ankle
        let (c, half) = self.centers[crate::lego_models::mapped_mesh(MeshId::CharBoot, g.cfg.mode) as usize];
        let sole = half.y - c.y;
        if a.mode == MoveMode::Bus {
            return if a.alive && g.bus.active { bus_rider_spot(g, i).map(|spot| roof_pose(g, a, &spot, sole)) } else { None };
        }
        if !a.alive && a.dead_time > 0.9 {
            return None;
        }
        Some(match self.handoffs.iter().find(|h| h.actor == i) {
            Some(h) => handoff_pose(g, a, h, sole),
            None => rig::pose(a),
        })
    }

    fn push(&mut self, mesh: MeshId, m: Mat4, color: [f32; 4]) {
        let mesh = crate::lego_models::mapped_mesh(mesh, self.mode);
        self.lists[mesh as usize].push(Instance::from_mat4(m, color));
    }

    /// Scale that makes `mesh` (as the current theme draws it) about `target` metres long.
    fn fit_to(&self, mesh: MeshId, target: f32) -> f32 {
        fit_scale(self.centers[crate::lego_models::mapped_mesh(mesh, self.mode) as usize].1, target)
    }

    fn push_i(&mut self, mesh: MeshId, i: Instance) {
        let mesh = crate::lego_models::mapped_mesh(mesh, self.mode);
        self.lists[mesh as usize].push(i);
    }

    fn clear(&mut self) {
        for l in &mut self.lists {
            l.clear();
        }
        self.ghost.clear();
        self.ghost_mesh = None;
        self.particles.clear();
        self.particles_add.clear();
        self.stats = SceneStats::default();
    }

    fn finish(&mut self) {
        // whatever slipped through with a NaN or an infinity would end up as a black square on screen
        for l in &mut self.lists {
            l.retain(|i| i.is_sane());
        }
        self.ghost.retain(|i| i.is_sane());
        self.particles.retain(|p| p.is_sane());
        self.particles_add.retain(|p| p.is_sane());
        self.instances.clear();
        self.batches.clear();
        for (mesh, list) in self.lists.iter().enumerate() {
            if list.is_empty() {
                continue;
            }
            let first = self.instances.len() as u32;
            self.instances.extend_from_slice(list);
            self.batches.push(Batch { mesh: mesh as u16, first, count: list.len() as u32, shadow: casts_shadow(crate::lego_models::original_mesh(ALL_IDS[mesh])) });
        }
        self.ghost_instances.clear();
        self.ghost_instances.extend_from_slice(&self.ghost);
        self.ghost_batches.clear();
        if let Some(mesh) = self.ghost_mesh {
            if !self.ghost.is_empty() {
                self.ghost_batches.push(Batch { mesh, first: 0, count: self.ghost.len() as u32, shadow: false });
            }
        }
        self.stats.instances = self.instances.len() as u32;
        self.stats.particles = (self.particles.len() + self.particles_add.len()) as u32;
    }

    /// Rebuild all draw lists for the current game state as seen from `cam`.
    pub fn build(&mut self, g: &Game, cam: &Camera) {
        self.mode = g.cfg.mode;
        self.clear();
        self.track_jumpers(g);
        let fr = cam.frustum();
        let cp = cam.pos;
        self.bus(g, &fr, cp);
        self.vehicles(g, &fr, cp);
        for (i, a) in g.actors.iter().enumerate() {
            if g.vehicle_for_actor(a.id).is_none() { self.character(g, i, &fr, cp); }
        }
        self.pickups(g, &fr, cp);
        self.chests(g, &fr, cp);
        self.pieces(g, &fr, cp);
        self.ghost_piece(g);
        self.projectiles(g);
        self.thrown(g, &fr, cp);
        self.traps(g, &fr, cp);
        self.aim_preview(g);
        self.felled(g, cp);
        g.fx.emit(&mut self.particles, &mut self.particles_add);
        self.finish();
    }

    // ---- characters ---------------------------------------------------------------------------------

    fn vehicles(&mut self, g: &Game, fr: &Frustum, cp: Vec3) {
        use crate::models::dims::*;
        for v in &g.vehicles {
            if v.pos.distance(cp) > 450.0 || !fr.intersects_sphere(v.pos + Vec3::Y, 3.0) { continue; }
            let f = yaw_forward(v.yaw);
            let grade = (g.world.hm.height_at(v.pos.x + f.x * 1.1, v.pos.z + f.z * 1.1) - g.world.hm.height_at(v.pos.x - f.x * 1.1, v.pos.z - f.z * 1.1)) / 2.2;
            let pitch = grade.atan().clamp(-0.32, 0.32);
            let lean = -v.steer * (v.speed.abs() / 38.0).min(1.0) * 0.06;
            let base = Mat4::from_translation(v.pos) * Mat4::from_rotation_y(v.yaw) * Mat4::from_rotation_x(pitch) * Mat4::from_rotation_z(lean);
            let palette = [0xf1b942, 0x4aaeb7, 0xe77962, 0x617ccc];
            self.push(v.body_mesh_id(), base, lin(crate::mesh::hex(palette[v.id.saturating_sub(1) as usize % palette.len()])));
            for x in [-1.02, 1.02] {
                for z in [-1.02, 1.05] {
                    let steer = if z < 0.0 { -v.steer * 0.44 } else { 0.0 };
                    let roll = (g.time * v.speed / 0.47) % std::f32::consts::TAU;
                    let m = base * Mat4::from_translation(Vec3::new(x, 0.47, z)) * Mat4::from_rotation_y(steer) * Mat4::from_rotation_x(roll);
                    self.push(MeshId::VehicleWheel, m, WHITE);
                }
            }
            let Some(a) = v.driver.and_then(|i| g.actors.get(i)).filter(|a| a.alive) else { continue; };
            self.stats.characters += 1;
            let o = &a.outfit;
            let hips = base * Mat4::from_translation(Vec3::new(-0.38, 0.89, 0.30));
            self.push(MeshId::CharPelvis, hips, lin(o.pants));
            self.push(MeshId::CharTorso, hips, lin(o.shirt));
            self.push(MeshId::CharTrim, hips, lin(o.accent));
            let head = hips * Mat4::from_translation(Vec3::Y * NECK_Y) * Mat4::from_rotation_y(angle_diff(v.yaw, a.yaw).clamp(-0.7, 0.7));
            self.push(MeshId::CharHead, head, lin(o.skin));
            if let Some(mesh) = rig::hair_model(o) { self.push(mesh, head, lin(o.hair)); }
            if let Some(mesh) = rig::headgear_model(o) { self.push(mesh, head, lin(o.headgear_color)); }
            for side in [-1.0, 1.0] {
                let thigh = hips * Mat4::from_translation(Vec3::new(side * HIP_X, -HIP_DROP, 0.0)) * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
                let shin = thigh * Mat4::from_translation(-Vec3::Y * THIGH) * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2);
                self.push(MeshId::CharLegUp, thigh, lin(o.pants));
                self.push(MeshId::CharLegLow, shin, lin(o.pants));
                self.push(MeshId::CharBoot, shin * Mat4::from_translation(-Vec3::Y * SHIN), lin(o.boots));
                let upper = hips * Mat4::from_translation(Vec3::new(side * SHOULDER_X, SHOULDER_Y, 0.0)) * Mat4::from_rotation_x(0.95);
                let lower = upper * Mat4::from_translation(-Vec3::Y * UPPER_ARM) * Mat4::from_rotation_x(0.35);
                self.push(MeshId::CharArmUp, upper, lin(o.shirt));
                self.push(MeshId::CharArmLow, lower, lin(o.shirt));
                self.push(MeshId::CharHand, lower * Mat4::from_translation(-Vec3::Y * FOREARM), lin(o.skin));
            }
        }
    }

    /// Everyone not on the bus (those on it are drawn with it, see [`Scene::bus`]).
    fn character(&mut self, g: &Game, i: usize, fr: &Frustum, cp: Vec3) {
        use crate::models::dims::HIP_Y;
        let a = &g.actors[i];
        if (!a.alive && a.dead_time > 0.9) || matches!(a.mode, MoveMode::Bus) {
            return;
        }
        let opacity = if a.alive { 1.0 } else { (1.0 - a.dead_time / 0.9).clamp(0.0, 1.0) };
        if opacity <= 0.02 {
            return;
        }
        let gliding = matches!(a.mode, MoveMode::Glide);
        let in_view = |feet: Vec3| feet.distance(cp) <= 420.0 && fr.intersects_sphere(feet + Vec3::Y * if gliding { 2.0 } else { 1.0 }, if gliding { 4.2 } else { 2.4 });
        // (someone getting off the bus is drawn on their way, not where the simulation has them: see `handoff_path`)
        let getting_off = self.handoffs.iter().any(|h| h.actor == i);
        if !getting_off && !in_view(a.pos) {
            return;
        }
        let Some(p) = self.actor_pose(g, i) else { return };
        if getting_off && !in_view(p.pelvis.w_axis.truncate() - Vec3::Y * HIP_Y) {
            return;
        }
        self.stats.characters += 1;
        let flash = a.hit_flash * 0.35 + if a.alive { 0.0 } else { (1.0 - opacity) * 0.8 };
        self.draw_pose(a, &p, [flash, opacity, 0.0, 0.0]);
    }

    /// Push every part of a posed character, what they hold and their glider, in the colours of their outfit.
    fn draw_pose(&mut self, a: &Actor, p: &rig::Pose, params: [f32; 4]) {
        let o = &a.outfit;
        let put = |s: &mut Scene, mesh: MeshId, m: Mat4, c: [f32; 4]| {
            let mut i = Instance::from_mat4(m, c);
            i.params = params;
            s.push_i(mesh, i);
        };
        let (shirt, skin, pants, boots) = (lin(o.shirt), lin(o.skin), lin(o.pants), lin(o.boots));
        put(self, MeshId::CharTorso, p.torso, shirt);
        put(self, MeshId::CharTrim, p.torso, lin(o.accent));
        put(self, MeshId::CharPelvis, p.pelvis, pants);
        put(self, MeshId::CharHead, p.head, skin);
        if let Some(h) = rig::hair_model(o) {
            put(self, h, p.head, lin(o.hair));
        }
        if let Some(h) = rig::headgear_model(o) {
            put(self, h, p.head, lin(o.headgear_color));
        }
        put(self, MeshId::CharBackpack, p.backpack, lin(o.backpack));
        for i in 0..2 {
            put(self, MeshId::CharArmUp, p.arm_up[i], shirt);
            put(self, MeshId::CharArmLow, p.arm_low[i], shirt);
            put(self, MeshId::CharHand, p.hand[i], skin);
            put(self, MeshId::CharLegUp, p.leg_up[i], pants);
            put(self, MeshId::CharLegLow, p.leg_low[i], pants);
            put(self, MeshId::CharBoot, p.boot[i], boots);
        }
        if let Some((mesh, m)) = p.item {
            let (tint, m) = match a.inv.selected_item() {
                Some(Item::Weapon { rarity, .. }) => (lin(Vec3::ONE.lerp(rarity.color(), 0.55)), m),
                // the props are modelled at their real size; in a hand a bandage would vanish
                Some(Item::Consumable { .. }) => (WHITE, m * Mat4::from_scale(Vec3::splat(self.fit_to(mesh, HELD_ITEM_SIZE)))),
                Some(Item::Utility { kind, .. }) => (WHITE, m * Mat4::from_scale(Vec3::splat(self.fit_to(mesh, held_utility_size(*kind))))),
                _ => (WHITE, m),
            };
            put(self, mesh, m, tint);
        }
        if let Some(m) = p.glider {
            put(self, MeshId::Glider, m, lin(o.glider));
        }
    }

    // ---- bus --------------------------------------------------------------------------------------------------

    /// The bus with its burner flame, and everyone still on board standing on its roof.
    fn bus(&mut self, g: &Game, fr: &Frustum, cp: Vec3) {
        if !g.bus.active {
            return;
        }
        let m = bus_matrix(g);
        self.push(MeshId::Bus, m, WHITE);
        let t = g.time;
        let flick = 0.85 + 0.15 * (t * 23.0).sin() * (t * 7.3).cos();
        let flame = m.transform_point3(Vec3::new(0.0, 4.95, 0.0));
        self.particles_add.push(Particle::billboard(flame, 1.5 * flick, [2.4 * flick, 1.1 * flick, 0.3 * flick, 0.5], shape::GLOW, 0.0, 0.3));
        let deck = m.transform_point3(Vec3::Y * 2.0);
        if deck.distance(cp) > 420.0 || !fr.intersects_sphere(deck, 6.5) {
            return;
        }
        for (i, a) in g.actors.iter().enumerate() {
            if a.mode != MoveMode::Bus {
                continue;
            }
            let Some(p) = self.actor_pose(g, i) else { continue };
            self.stats.characters += 1;
            self.draw_pose(a, &p, [a.hit_flash * 0.35, 1.0, 0.0, 0.0]);
        }
    }

    // ---- loose items -------------------------------------------------------------------------------------------------

    fn beam(&mut self, base: Vec3, color: Vec3, height: f32, width: f32, strength: f32, seed: f32) {
        let c = [color.x.powf(2.2) * 2.6 * strength, color.y.powf(2.2) * 2.6 * strength, color.z.powf(2.2) * 2.6 * strength, 0.55];
        self.particles_add.push(Particle::stretched(base, Vec3::Y, height, width, c, shape::BEAM, seed));
        // a tight core so the pillar reads as a bright line
        let core = [color.x.powf(2.2) * 3.0 * strength + 0.5, color.y.powf(2.2) * 3.0 * strength + 0.5, color.z.powf(2.2) * 3.0 * strength + 0.5, 0.5];
        self.particles_add.push(Particle::stretched(base, Vec3::Y, height * 0.8, width * 0.28, core, shape::BEAM, seed + 0.3));
        let g = [color.x.powf(2.2) * 1.4 * strength, color.y.powf(2.2) * 1.4 * strength, color.z.powf(2.2) * 1.4 * strength, 0.5];
        self.particles_add.push(Particle::billboard(base + Vec3::Y * 0.15, width * 2.2, g, shape::GLOW, 0.0, seed));
    }

    fn pickups(&mut self, g: &Game, fr: &Frustum, cp: Vec3) {
        let t = g.time;
        for p in &g.pickups {
            let d = p.pos.distance(cp);
            if d > 170.0 || !fr.intersects_sphere(p.pos + Vec3::Y * 2.5, 4.5) {
                continue;
            }
            self.stats.pickups += 1;
            let mesh = rig::pickup_model(&p.kind);
            // the theme's own mesh is what gets drawn (see `push`), so its bounds are the ones that matter
            let (center, half) = self.centers[crate::lego_models::mapped_mesh(mesh, self.mode) as usize];
            let (scale, tint, glow_col, beam_h) = match p.kind {
                PickupKind::Weapon { rarity, .. } => (1.3, lin(Vec3::ONE.lerp(rarity.color(), 0.5)), rarity.color(), 4.8),
                PickupKind::Ammo { kind, .. } => (fit_scale(half, floor_item_size(&p.kind)), lin(kind.color()), kind.color(), 2.4),
                PickupKind::Consumable { kind, .. } => (fit_scale(half, floor_item_size(&p.kind)), WHITE, kind.rarity().color(), 3.2),
                PickupKind::Utility { kind, .. } => (fit_scale(half, floor_item_size(&p.kind)), WHITE, kind.rarity().color(), 3.2),
                PickupKind::Mats { mat, .. } => (fit_scale(half, floor_item_size(&p.kind)), WHITE, mat.color(), 2.0),
            };
            let bob = if p.grounded { 0.07 * (t * 2.3 + p.spin * 3.0).sin() } else { 0.0 };
            let lift = (half.y * scale + 0.3).max(0.42);
            let pos = p.pos + Vec3::Y * (lift + bob);
            let m = Mat4::from_translation(pos) * Mat4::from_rotation_y(p.spin) * Mat4::from_scale(Vec3::splat(scale)) * Mat4::from_translation(-center);
            self.push(mesh, m, tint);
            if d < 120.0 {
                let fade = if d > 80.0 { (120.0 - d) / 40.0 } else { 1.0 };
                self.beam(p.pos + Vec3::Y * 0.05, glow_col, beam_h, 0.2, fade, (p.id % 97) as f32 / 97.0);
            }
        }
    }

    /// A supply drop: the blue crate swinging under its parachute (which crumples once it lands), and a tall beam over the spot
    /// where it comes down so it can be seen from across the island.
    fn supply_crate(&mut self, c: &Chest, fr: &Frustum, cp: Vec3, t: f32) {
        let ground = Vec3::new(c.pos.x, c.land_y, c.pos.z);
        let dg = ground.distance(cp);
        if !c.opened && dg < 700.0 {
            let pulse = 0.85 + 0.15 * (t * 3.0 + c.id as f32).sin();
            let fade = if dg > 500.0 { (700.0 - dg) / 200.0 } else { 1.0 };
            self.beam(ground + Vec3::Y * 0.2, Vec3::new(0.25, 0.6, 1.0), 110.0, 0.7, pulse * fade, (c.id % 83) as f32 / 83.0);
        }
        if c.pos.distance(cp) > 650.0 || !fr.intersects_sphere(c.pos + Vec3::Y * 3.0, 6.5) {
            return;
        }
        self.stats.pickups += 1;
        let at = Mat4::from_translation(c.pos);
        let yaw = Mat4::from_rotation_y(c.yaw);
        let seed = c.id as f32;
        if c.falling() {
            // the crate hangs from the canopy: it swings about the point the cords meet at, 5.6 m above its base
            let pivot = Vec3::new(0.0, 5.6, 0.0);
            let swing = Mat4::from_translation(pivot) * Mat4::from_rotation_x((t * 1.3 + seed).sin() * 0.05) * Mat4::from_rotation_z((t * 1.1 + seed * 2.0).cos() * 0.05) * Mat4::from_translation(-pivot);
            self.push(MeshId::SupplyCrate, at * swing * yaw, WHITE);
            self.push(MeshId::SupplyChute, at * swing * yaw * Mat4::from_translation(Vec3::Y), WHITE);
        } else {
            let tint = if c.opened { lin(Vec3::splat(0.6)) } else { WHITE };
            self.push(MeshId::SupplyCrate, at * yaw, tint);
            // the canopy sinks and slumps to the ground beside the crate
            let k = ease(c.since_land / 1.8);
            let slump = Mat4::from_translation(Vec3::new(2.2 * k, 1.0 - k, 0.8 * k)) * Mat4::from_scale(Vec3::new(1.0 + 0.3 * k, 1.0 - 0.94 * k, 1.0 + 0.3 * k));
            self.push(MeshId::SupplyChute, at * yaw * slump, WHITE);
        }
    }

    /// A green ammo box: its lid swings up when it is opened.
    fn ammo_box(&mut self, c: &Chest, fr: &Frustum, cp: Vec3) {
        if c.pos.distance(cp) > 160.0 || !fr.intersects_sphere(c.pos + Vec3::Y * 0.4, 1.0) {
            return;
        }
        let base = Mat4::from_translation(c.pos) * Mat4::from_rotation_y(c.yaw + std::f32::consts::PI);
        self.push(MeshId::AmmoBoxBase, base, WHITE);
        let open = ease(c.open_t) * 1.8;
        self.push(MeshId::AmmoBoxLid, base * Mat4::from_translation(Vec3::new(0.0, 0.32, 0.26)) * Mat4::from_rotation_x(open), WHITE);
    }

    /// A Supply Llama: it sways a little where it stands, and bursts (shrinks away in a flash) when it is opened.
    fn llama(&mut self, c: &Chest, fr: &Frustum, cp: Vec3, t: f32) {
        if c.open_t >= 1.0 || c.pos.distance(cp) > 260.0 || !fr.intersects_sphere(c.pos + Vec3::Y * 0.9, 1.6) {
            return;
        }
        let sway = (t * 1.3 + c.id as f32).sin() * 0.04;
        let burst = if c.opened { 1.0 - ease(c.open_t) } else { 1.0 };
        let m = Mat4::from_translation(c.pos) * Mat4::from_rotation_y(c.yaw) * Mat4::from_rotation_z(sway) * Mat4::from_scale(Vec3::new(burst, burst * (1.0 + 0.15 * (1.0 - burst)), burst));
        self.push(MeshId::Llama, m, WHITE);
        if c.opened {
            self.particles_add.push(Particle::billboard(c.pos + Vec3::Y * 1.0, 2.6, [2.4, 1.2, 3.0, 0.6 * burst], shape::GLOW, 0.0, 0.0));
        }
    }

    fn chests(&mut self, g: &Game, fr: &Frustum, cp: Vec3) {
        let t = g.time;
        for c in &g.chests {
            match c.kind {
                ChestKind::Supply => {
                    self.supply_crate(c, fr, cp, t);
                    continue;
                }
                ChestKind::AmmoBox => {
                    self.ammo_box(c, fr, cp);
                    continue;
                }
                ChestKind::Llama => {
                    self.llama(c, fr, cp, t);
                    continue;
                }
                ChestKind::Treasure => {}
            }
            let d = c.pos.distance(cp);
            if d > 230.0 || !fr.intersects_sphere(c.pos + Vec3::Y * 3.0, 5.0) {
                continue;
            }
            let base = Mat4::from_translation(c.pos) * Mat4::from_rotation_y(c.yaw + std::f32::consts::PI) * Mat4::from_scale(Vec3::splat(1.2));
            self.push(MeshId::ChestBase, base, WHITE);
            let open = ease(c.open_t) * 1.95;
            let lid = base * Mat4::from_translation(Vec3::new(0.0, 0.46, 0.3)) * Mat4::from_rotation_x(open) * Mat4::from_translation(Vec3::new(0.0, 0.0, -0.6));
            self.push(MeshId::ChestLid, lid, WHITE);
            if !c.opened && d < 160.0 {
                let pulse = 0.85 + 0.15 * (t * 3.0 + c.id as f32).sin();
                let fade = if d > 110.0 { (160.0 - d) / 50.0 } else { 1.0 };
                self.beam(c.pos + Vec3::Y * 0.2, Vec3::new(1.0, 0.78, 0.2), 8.0, 0.32, pulse * fade, (c.id % 89) as f32 / 89.0);
            } else if c.opened && c.open_t < 1.0 {
                // golden light pours out as the lid opens
                self.particles_add.push(Particle::billboard(c.pos + Vec3::Y * 0.7, 1.8, [3.0, 2.2, 0.6, 0.5 * (1.0 - c.open_t)], shape::GLOW, 0.0, 0.0));
            }
        }
    }

    // ---- building pieces ---------------------------------------------------------------------------------------------------

    fn pieces(&mut self, g: &Game, fr: &Frustum, cp: Vec3) {
        for p in g.pieces.iter() {
            let c = super::combat::shape_center(p);
            if c.distance(cp) > 320.0 || !fr.intersects_sphere(c, 4.0) {
                continue;
            }
            self.stats.pieces += 1;
            let mut m = rig::piece_transform(&p.key, p.base_y);
            // building pop-in: grow from the base with a bright flash
            let k = (p.age / 0.22).clamp(0.0, 1.0);
            if k < 1.0 {
                let s = 0.25 + 0.75 * ease(k);
                m *= match p.key.kind {
                    PieceKind::Floor => Mat4::from_scale(Vec3::new(s, 1.0, s)),
                    _ => Mat4::from_scale(Vec3::new(1.0, s, 1.0)),
                };
            }
            let hp = (p.hp / p.max_hp).clamp(0.0, 1.0);
            let dim = 0.55 + 0.45 * hp;
            let t = material_tint(p.mat);
            let col = lin(t * dim);
            let flash = (1.0 - k) * 0.7;
            let mut inst = Instance::from_mat4(m, col);
            inst.params = [flash, 1.0, p.mat.index() as f32, 0.0];
            self.push_i(rig::piece_mesh(p.key.kind), inst);
            // on a slope the piece's footing: a plain block from the ground up to the piece
            if p.footing > 0.0 {
                let body = super::pieces::shape_of(&p.key, p.base_y).aabb();
                let (min, max) = (Vec3::new(body.min.x, body.min.y - p.footing, body.min.z), Vec3::new(body.max.x, body.min.y, body.max.z));
                let block = Mat4::from_translation((min + max) * 0.5) * Mat4::from_scale(max - min);
                let mut foot = Instance::from_mat4(block, lin(t * dim * 0.62));
                foot.params = [flash, 1.0, p.mat.index() as f32, 0.0];
                self.push_i(MeshId::UnitBox, foot);
            }
        }
    }

    fn ghost_piece(&mut self, g: &Game) {
        let Some(pl) = g.placement_preview else { return };
        let mesh = rig::piece_mesh(pl.key.kind);
        let m = rig::piece_transform(&pl.key, pl.base_y);
        let col = if pl.valid { [0.2, 0.75, 1.0, 1.0] } else { [1.0, 0.22, 0.18, 1.0] };
        self.ghost.push(Instance::from_mat4(m, col));
        self.ghost_mesh = Some(crate::lego_models::mapped_mesh(mesh, self.mode) as u16);
    }

    // ---- grenades, Launch Pads -----------------------------------------------------------------------------------------------

    fn thrown(&mut self, g: &Game, fr: &Frustum, cp: Vec3) {
        for th in &g.thrown {
            if th.pos.distance(cp) > 250.0 || !fr.intersects_sphere(th.pos, 1.0) {
                continue;
            }
            let mesh = rig::utility_model(th.kind);
            let k = self.fit_to(mesh, THROWN_SIZE);
            let (center, _) = self.centers[crate::lego_models::mapped_mesh(mesh, self.mode) as usize];
            let m = Mat4::from_translation(th.pos) * Mat4::from_rotation_y(th.spin) * Mat4::from_rotation_x(th.spin * 0.7) * Mat4::from_scale(Vec3::splat(k)) * Mat4::from_translation(-center);
            self.push(mesh, m, WHITE);
            // a blinking light so a grenade on the ground is easy to see
            let blink = 0.5 + 0.5 * (th.age * if th.resting { 14.0 } else { 9.0 }).sin();
            let c = match th.kind {
                UtilityKind::FragGrenade => [3.0, 0.5, 0.3, 0.55 * blink],
                UtilityKind::ImpulseGrenade => [0.6, 1.8, 3.0, 0.55 * blink],
                UtilityKind::BoogieBomb => [2.6, 0.8, 2.6, 0.55 * blink],
                UtilityKind::LaunchPad => [1.0, 1.0, 1.0, 0.0],
            };
            self.particles_add.push(Particle::billboard(th.pos + Vec3::Y * 0.08, 0.5, c, shape::GLOW, 0.0, 0.0));
        }
    }

    fn traps(&mut self, g: &Game, fr: &Frustum, cp: Vec3) {
        for tr in &g.traps {
            if tr.pos.distance(cp) > 220.0 || !fr.intersects_sphere(tr.pos + Vec3::Y * 0.3, 1.5) {
                continue;
            }
            let base = Mat4::from_translation(tr.pos) * Mat4::from_rotation_y(tr.yaw);
            self.push(MeshId::LaunchPadBase, base, WHITE);
            // the plate springs up when it throws someone
            let lift = 0.13 + 0.42 * ease(tr.bounce);
            self.push(MeshId::LaunchPadPlate, base * Mat4::from_translation(Vec3::Y * lift), WHITE);
            let pulse = 0.75 + 0.25 * (g.time * 3.0 + tr.id as f32).sin();
            self.particles_add.push(Particle::billboard(tr.pos + Vec3::Y * 0.35, 1.9, [0.35 * pulse, 0.9 * pulse, 2.2 * pulse, 0.35], shape::GLOW, 0.0, 0.0));
        }
    }

    /// While the player holds something to throw, the arc it would fly; while they hold a Launch Pad, where it would go.
    fn aim_preview(&mut self, g: &Game) {
        let a = &g.actors[g.local];
        if !a.alive || a.mode != MoveMode::Ground || a.build_mode || a.boogie > 0.0 {
            return;
        }
        let Some(Item::Utility { kind, .. }) = a.inv.selected_item().copied() else { return };
        let env = g.env();
        if kind.def().thrown {
            let (_, dir) = g.shot_ray(g.local);
            let (origin, vel) = utility::throw_start(a.pos, a.yaw, dir, a.vel);
            let pts = utility::predict_arc(&env, origin, vel, 3.0);
            for (k, p) in pts.iter().enumerate().skip(2) {
                if k % 2 == 0 {
                    self.particles_add.push(Particle::billboard(*p, 0.16, [1.6, 1.6, 1.8, 0.7], shape::GLOW, 0.0, 0.0));
                }
            }
            if let Some(end) = pts.last() {
                self.particles_add.push(Particle::billboard(*end + Vec3::Y * 0.05, 0.9, [2.0, 1.4, 0.5, 0.6], shape::GLOW, 0.0, 0.0));
            }
        } else {
            let f = yaw_forward(a.yaw);
            let at = Vec3::new(a.pos.x + f.x * 1.6, a.pos.y, a.pos.z + f.z * 1.6);
            let gnd = env.ground(at.x, at.z, a.pos.y + 1.0, 1.2);
            let m = Mat4::from_translation(Vec3::new(at.x, gnd.y, at.z)) * Mat4::from_rotation_y(a.yaw);
            let mut i = Instance::from_mat4(m, [0.6, 0.9, 1.4, 1.0]);
            i.params = [0.3, 0.45, 0.0, 0.0];
            self.push_i(MeshId::LaunchPadBase, i);
        }
    }

    // ---- projectiles, felled trees ----------------------------------------------------------------------------------------------

    fn projectiles(&mut self, g: &Game) {
        for pr in &g.projectiles {
            let dir = pr.vel.normalize_or_zero();
            if dir == Vec3::ZERO {
                continue;
            }
            let rot = Quat::from_rotation_arc(Vec3::NEG_Z, dir);
            self.push(MeshId::Missile, Mat4::from_translation(pr.pos) * Mat4::from_quat(rot), WHITE);
            // flame and smoke trail
            self.particles_add.push(Particle::billboard(pr.pos - dir * 0.4, 0.7, [4.0, 2.4, 0.8, 0.9], shape::GLOW, 0.0, 0.2));
            self.particles_add.push(Particle::billboard(pr.pos - dir * 0.25, 0.4, [6.0, 4.5, 2.0, 1.0], shape::GLOW, 0.0, 0.6));
            for k in 1..9 {
                let f = k as f32 / 9.0;
                let jitter = Vec3::new(((k * 37 + (pr.pos.x * 7.0) as usize) % 11) as f32 - 5.0, ((k * 53) % 7) as f32 - 3.0, ((k * 29) % 9) as f32 - 4.0) * 0.012;
                self.particles.push(Particle::billboard(pr.pos - dir * (0.5 + 0.55 * k as f32) + jitter, 0.25 + 0.5 * f, [0.8, 0.78, 0.74, 0.45 * (1.0 - f)], shape::SMOKE, k as f32, f));
            }
        }
    }

    fn felled(&mut self, g: &Game, cp: Vec3) {
        for f in &g.felled {
            if f.pos.distance(cp) > 300.0 {
                continue;
            }
            let k = ease(f.t / 1.15);
            let ang = k * 1.48;
            let d3 = Vec3::new(f.dir.x, 0.0, f.dir.y);
            let axis = Vec3::new(d3.z, 0.0, -d3.x).normalize_or_zero();
            let axis = if axis == Vec3::ZERO { Vec3::X } else { axis };
            let m = Mat4::from_translation(f.pos) * Mat4::from_axis_angle(axis, ang) * Mat4::from_rotation_y(f.yaw) * Mat4::from_scale(Vec3::splat(f.scale));
            let opacity = if f.t > 1.4 { (1.0 - (f.t - 1.4) / 1.1).clamp(0.0, 1.0) } else { 1.0 };
            if opacity <= 0.02 {
                continue;
            }
            let mut i = Instance::from_mat4(m, [f.tint[0], f.tint[1], f.tint[2], 1.0]);
            i.params = [0.0, opacity, 0.0, 0.0];
            self.push_i(f.kind.mesh(0), i);
        }
    }
}

/// Storm wall parameters for the renderer: centre x, centre z, radius, strength.
pub fn storm_params(g: &Game) -> [f32; 4] {
    if g.storm.active {
        [g.storm.center.x, g.storm.center.y, g.storm.radius, 1.0]
    } else {
        [0.0, 0.0, 0.0, 0.0]
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil::game;
    use super::*;
    use crate::meshlib::build_all;
    use crate::models::dims;

    #[test]
    fn nothing_with_a_nan_reaches_the_gpu() {
        // a NaN that gets as far as the screen is smeared by the bloom into a black square
        let mut g = game(6, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        g.actors[1].pos = Vec3::new(f32::NAN, 5.0, 2.0);
        g.actors[2].yaw = f32::INFINITY;
        g.actors[2].mode = MoveMode::Ground;
        g.fx.on_event(&crate::game::events::Event::Explosion { pos: Vec3::new(1.0, f32::NAN, 1.0), radius: 7.0 }, &g.actors, 0.0);
        g.pickups.push(crate::game::Pickup { id: 999_999, pos: Vec3::new(f32::INFINITY, 0.0, 0.0), vel: Vec3::ZERO, kind: crate::game::PickupKind::Ammo { kind: AmmoKind::Light, amount: 1 }, age: 0.0, grounded: true, spin: 0.0 });
        let meshes = build_all();
        let mut scene = Scene::new(&meshes);
        let p = g.actors[PLAYER].pos;
        let cam = Camera::look(p + Vec3::new(0.0, 6.0, 9.0), Vec3::new(0.0, -0.3, -1.0).normalize(), 0.0, 70f32.to_radians(), 1.6, 0.1);
        scene.build(&g, &cam);
        assert!(!scene.instances.is_empty());
        assert!(scene.instances.iter().all(|i| i.is_sane()), "an instance with a NaN got through");
        assert!(scene.particles.iter().chain(&scene.particles_add).all(|p| p.is_sane()), "a particle with a NaN got through");
        let total: u32 = scene.batches.iter().map(|b| b.count).sum();
        assert_eq!(total as usize, scene.instances.len(), "the batches still add up");
    }

    #[test]
    fn healing_items_and_ammo_are_big_enough_to_see_on_the_ground_and_in_the_hand() {
        // the prop meshes are modelled at real size (a bandage is 12 cm), which is a speck from a third-person camera
        let meshes = build_all();
        for mode in [GameMode::BattleRoyale, GameMode::Lego] {
            let mut scene = Scene::new(&meshes);
            scene.mode = mode;
            let kinds: Vec<PickupKind> = ConsumableKind::ALL
                .iter()
                .map(|&kind| PickupKind::Consumable { kind, count: 1 })
                .chain(AmmoKind::ALL.iter().map(|&kind| PickupKind::Ammo { kind, amount: 1 }))
                .chain(UtilityKind::ALL.iter().map(|&kind| PickupKind::Utility { kind, count: 1 }))
                .chain(Mat::ALL.iter().map(|&mat| PickupKind::Mats { mat, amount: 30 }))
                .collect();
            for kind in kinds {
                let mesh = rig::pickup_model(&kind);
                let (_, half) = scene.centers[crate::lego_models::mapped_mesh(mesh, mode) as usize];
                let on_floor = half.max_element() * 2.0 * fit_scale(half, floor_item_size(&kind));
                assert!((0.4..=0.8).contains(&on_floor), "{mode:?} {kind:?} is {on_floor:.2} m on the ground");
                // (a grenade fits in a fist, a Launch Pad is carried like a tray; materials are never held)
                let (in_hand, fits) = match kind {
                    PickupKind::Utility { kind, .. } => (half.max_element() * 2.0 * scene.fit_to(mesh, held_utility_size(kind)), 0.15..=0.65),
                    PickupKind::Mats { .. } => continue,
                    _ => (half.max_element() * 2.0 * scene.fit_to(mesh, HELD_ITEM_SIZE), 0.22..=0.4),
                };
                assert!(fits.contains(&in_hand), "{mode:?} {kind:?} is {in_hand:.2} m in the hand");
            }
        }
    }

    #[test]
    fn a_supply_crate_is_drawn_under_its_parachute_and_the_canopy_slumps_after_landing() {
        let mut g = game(2, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let p = g.actors[PLAYER].pos;
        let id = supply::launch(&mut g, Vec2::new(p.x, p.z - 25.0));
        let i = g.chest_index(id).unwrap();
        g.chests[i].pos.y = g.chests[i].land_y + 15.0;
        let meshes = build_all();
        let mut scene = Scene::new(&meshes);
        let cam = Camera::look(p + Vec3::new(0.0, 4.0, 6.0), Vec3::new(0.0, 0.1, -1.0).normalize(), 0.0, 70f32.to_radians(), 1.6, 0.1);
        let count = |scene: &Scene, mesh: MeshId| scene.batches.iter().filter(|b| b.mesh == mesh as u16).map(|b| b.count).sum::<u32>();
        scene.build(&g, &cam);
        assert_eq!(count(&scene, MeshId::SupplyCrate), 1, "the crate");
        assert_eq!(count(&scene, MeshId::SupplyChute), 1, "and its parachute");
        assert!(scene.particles_add.iter().any(|q| q.is_sane()), "the beam over the landing spot");
        assert!(scene.instances.iter().all(|i| i.is_sane()));
        // down on the ground the crate is still there with its collapsed canopy beside it, and nothing is broken by the transition
        for since_land in [0.0, 0.9, 5.0] {
            g.chests[i].pos.y = g.chests[i].land_y;
            g.chests[i].since_land = since_land;
            scene.build(&g, &cam);
            assert_eq!((count(&scene, MeshId::SupplyCrate), count(&scene, MeshId::SupplyChute)), (1, 1), "{since_land} s after landing");
            assert!(scene.instances.iter().all(|i| i.is_sane()), "{since_land} s after landing");
        }
    }

    #[test]
    fn a_fresh_match_produces_characters_pickups_and_chests() {
        let mut g = game(10, true);
        // look at the player from behind and above
        for _ in 0..3 {
            g.update(1.0 / 60.0, &PlayerInput::default());
        }
        let meshes = build_all();
        let mut scene = Scene::new(&meshes);
        let p = g.actors[PLAYER].pos;
        let cam = Camera::look(p + Vec3::new(0.0, 6.0, 9.0), Vec3::new(0.0, -0.3, -1.0).normalize(), 0.0, 70f32.to_radians(), 1.6, 0.1);
        scene.build(&g, &cam);
        assert!(scene.stats.characters >= 1, "the player must be visible");
        assert!(!scene.instances.is_empty());
        let total: u32 = scene.batches.iter().map(|b| b.count).sum();
        assert_eq!(total as usize, scene.instances.len());
        // batches are contiguous, non-overlapping and reference valid meshes
        let mut next = 0;
        for b in &scene.batches {
            assert_eq!(b.first, next);
            next += b.count;
            assert!((b.mesh as usize) < meshes.len());
        }
        assert!(scene.batches.iter().any(|b| b.mesh == MeshId::CharTorso as u16 && b.shadow));
        for i in &scene.instances {
            for v in i.m0.iter().chain(&i.m1).chain(&i.m2).chain(&i.color).chain(&i.params) {
                assert!(v.is_finite());
            }
        }
    }

    #[test]
    fn dead_actors_dissolve_and_disappear() {
        let mut g = game(3, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let meshes = build_all();
        let mut scene = Scene::new(&meshes);
        let b = g.actors[1].pos;
        let cam = Camera::look(b + Vec3::new(0.0, 3.0, 6.0), Vec3::new(0.0, -0.2, -1.0).normalize(), 0.0, 70f32.to_radians(), 1.6, 0.1);
        g.eliminate(1, Some(0), "test", false);
        g.actors[1].dead_time = 0.4;
        scene.build(&g, &cam);
        let torso = scene.batches.iter().find(|b| b.mesh == MeshId::CharTorso as u16).expect("fading corpse still drawn");
        let inst = &scene.instances[torso.first as usize..(torso.first + torso.count) as usize];
        assert!(inst.iter().any(|i| i.params[1] < 1.0), "dissolving: opacity {:?}", inst.iter().map(|i| i.params[1]).collect::<Vec<_>>());
        g.actors[1].dead_time = 1.0;
        let before = scene.stats.characters;
        scene.build(&g, &cam);
        assert!(scene.stats.characters < before || before == 0);
    }

    #[test]
    fn building_pieces_and_ghost_preview_are_drawn() {
        let mut g = game(1, true);
        let spot = g.actors[PLAYER].pos;
        g.actors[PLAYER].mode = MoveMode::Ground;
        g.actors[PLAYER].inv.mats = [200, 0, 0];
        g.update(1.0 / 60.0, &PlayerInput { piece: Some(PieceKind::Wall), ..Default::default() });
        g.update(1.0 / 60.0, &PlayerInput { place: true, ..Default::default() });
        let meshes = build_all();
        let mut scene = Scene::new(&meshes);
        let cam = Camera::look(spot + Vec3::new(0.0, 5.0, 8.0), Vec3::new(0.0, -0.3, -1.0).normalize(), 0.0, 70f32.to_radians(), 1.6, 0.1);
        scene.build(&g, &cam);
        if g.pieces.count() > 0 {
            assert!(scene.batches.iter().any(|b| b.mesh == MeshId::PieceWall as u16));
            assert_eq!(scene.stats.pieces as usize, g.pieces.count());
        }
        assert!(g.placement_preview.is_some());
        assert_eq!(scene.ghost_batches.len(), 1);
        assert_eq!(scene.ghost_instances.len(), 1);
    }

    #[test]
    fn grenades_launch_pads_ammo_boxes_and_llamas_are_drawn() {
        use crate::game::utility::{Thrown, Trap};
        let mut g = game(2, true);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let p = g.actors[PLAYER].pos;
        let ground = |g: &Game, x: f32, z: f32| Vec3::new(x, g.world.hm.height_at(x, z), z);
        g.thrown.push(Thrown { id: 9001, kind: UtilityKind::FragGrenade, pos: p + Vec3::new(0.0, 1.5, -6.0), vel: Vec3::new(0.0, 2.0, -15.0), owner: 1, age: 0.4, resting: false, bounced: false, spin: 1.0 });
        g.traps.push(Trap { id: 9002, kind: UtilityKind::LaunchPad, pos: ground(&g, p.x + 2.0, p.z - 7.0), yaw: 0.3, owner: 1, bounce: 0.5 });
        g.chests.clear();
        g.chests.push(Chest::of_kind(9003, ground(&g, p.x - 2.0, p.z - 8.0), 0.0, ChestKind::AmmoBox));
        g.chests.push(Chest::of_kind(9004, ground(&g, p.x + 4.0, p.z - 10.0), 0.0, ChestKind::Llama));
        let meshes = build_all();
        let mut scene = Scene::new(&meshes);
        let cam = Camera::look(p + Vec3::new(0.0, 3.0, 5.0), Vec3::new(0.0, -0.15, -1.0).normalize(), 0.0, 70f32.to_radians(), 1.6, 0.1);
        let count = |scene: &Scene, mesh: MeshId| scene.batches.iter().filter(|b| b.mesh == mesh as u16).map(|b| b.count).sum::<u32>();
        scene.build(&g, &cam);
        for (mesh, n) in [(MeshId::Grenade, 1), (MeshId::LaunchPadBase, 1), (MeshId::LaunchPadPlate, 1), (MeshId::AmmoBoxBase, 1), (MeshId::AmmoBoxLid, 1), (MeshId::Llama, 1)] {
            assert_eq!(count(&scene, mesh), n, "{mesh:?}");
        }
        assert!(scene.instances.iter().all(|i| i.is_sane()));
        // holding a grenade shows where a throw would go
        let glow_before = scene.particles_add.len();
        g.actors[PLAYER].inv.add_utility(UtilityKind::ImpulseGrenade, 2);
        let slot = g.actors[PLAYER].inv.slots.iter().position(|s| matches!(s, Some(Item::Utility { .. }))).unwrap();
        g.select_slot(PLAYER, slot);
        scene.build(&g, &cam);
        assert!(scene.particles_add.len() > glow_before + 5, "the throw arc is drawn");
        assert_eq!(count(&scene, MeshId::ImpulseGrenade), 1, "the grenade in the hand");
        // an opened llama bursts and is gone
        g.chests[1].opened = true;
        g.chests[1].open_t = 1.0;
        scene.build(&g, &cam);
        assert_eq!(count(&scene, MeshId::Llama), 0);
    }

    #[test]
    fn explosions_and_rockets_add_particles() {
        let mut g = game(2, true);
        let p = g.actors[PLAYER].pos;
        g.projectiles.push(Projectile { pos: p + Vec3::new(0.0, 2.0, -6.0), vel: Vec3::new(0.0, 0.0, -50.0), owner: 1, kind: WeaponKind::RocketLauncher, rarity: Rarity::Rare, life: 3.0 });
        let meshes = build_all();
        let mut scene = Scene::new(&meshes);
        let cam = Camera::look(p + Vec3::new(0.0, 3.0, 5.0), Vec3::new(0.0, -0.1, -1.0).normalize(), 0.0, 70f32.to_radians(), 1.6, 0.1);
        scene.build(&g, &cam);
        assert!(scene.batches.iter().any(|b| b.mesh == MeshId::Missile as u16));
        assert!(!scene.particles_add.is_empty() && !scene.particles.is_empty());
    }

    /// World positions and forward directions of the torsos drawn.
    fn torsos(scene: &Scene) -> Vec<(Vec3, Vec3)> {
        let b = scene.batches.iter().find(|b| b.mesh == MeshId::CharTorso as u16);
        b.map_or(vec![], |b| scene.instances[b.first as usize..(b.first + b.count) as usize].iter().map(|i| (Vec3::new(i.m0[3], i.m1[3], i.m2[3]), -Vec3::new(i.m0[2], i.m1[2], i.m2[2]))).collect())
    }

    #[test]
    fn everyone_on_the_bus_stands_on_its_roof_facing_forward_with_the_player_at_the_back() {
        let mut g = game(30, false);
        g.update(1.0 / 60.0, &PlayerInput::default());
        assert_eq!(g.phase, Phase::Bus);
        assert!(g.actors.iter().all(|a| a.mode == MoveMode::Bus));
        let meshes = build_all();
        let mut scene = Scene::new(&meshes);
        scene.build(&g, &g.camera(16.0 / 9.0));
        // the roof holds the player and nine rows of two; the rest ride inside, out of sight
        assert_eq!(scene.stats.characters as usize, ROOF_RIDERS);
        let drawn = torsos(&scene);
        assert_eq!(drawn.len(), ROOF_RIDERS);
        let bus = bus_matrix(&g);
        let to_bus = bus.inverse();
        let fwd = Vec3::new(g.bus.dir.x, 0.0, g.bus.dir.y);
        let roof = crate::models::BUS_ROOF_Y;
        for (p, f) in &drawn {
            let l = to_bus.transform_point3(*p);
            assert!(l.x.abs() < 1.0 && (-3.2..3.6).contains(&l.z), "a rider off the roof deck at {l:?}");
            assert!((l.y - roof - dims::HIP_Y).abs() < 0.2, "a rider not standing on the roof: pelvis {} above it", l.y - roof);
            assert!(Vec3::new(f.x, 0.0, f.z).normalize().dot(fwd) > 0.8, "riders face the way the bus flies");
        }
        let spots: Vec<RiderSpot> = (0..g.actors.len()).filter_map(|i| bus_rider_spot(&g, i)).collect();
        assert_eq!(spots.len(), ROOF_RIDERS);
        for (k, a) in spots.iter().enumerate() {
            for b in &spots[k + 1..] {
                assert!(a.pos.distance(b.pos) > 0.55, "two riders on top of each other: {:?} {:?}", a.pos, b.pos);
            }
        }
        // the player is the one at the very back, nearest the camera, and the camera sees them and the whole bus
        let me = bus_rider_spot(&g, g.local).unwrap();
        assert!(spots.iter().all(|s| s.pos.z <= me.pos.z), "the player stands at the back");
        let cam = g.camera(16.0 / 9.0);
        let head = bus.transform_point3(me.pos + Vec3::Y * 1.7);
        assert!(drawn.iter().map(|(p, _)| p.distance(cam.pos)).fold(f32::MAX, f32::min) >= bus.transform_point3(me.pos + Vec3::Y * dims::HIP_Y).distance(cam.pos) - 0.3, "nobody stands nearer the camera than the player");
        for (what, p) in [("the player", head), ("the front bumper", bus.transform_point3(Vec3::new(0.0, -1.0, -5.6))), ("the rear bumper", bus.transform_point3(Vec3::new(0.0, -1.0, 4.4))), ("the balloon's mouth", bus.transform_point3(Vec3::Y * 5.4))] {
            let s = cam.project(p).unwrap_or(Vec3::splat(9.0));
            assert!(s.x.abs() < 0.8 && s.y.abs() < 0.8, "{what} is off screen at {s:?}");
        }
        assert!((14.0..18.0).contains(&cam.pos.distance(g.bus.pos)), "the camera stays near the bus: {}", cam.pos.distance(g.bus.pos));
    }

    #[test]
    fn a_rider_who_jumps_leaves_the_roof_and_the_others_keep_their_spots() {
        let mut g = game(8, false);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let meshes = build_all();
        let mut scene = Scene::new(&meshes);
        let before: Vec<Option<RiderSpot>> = (0..g.actors.len()).map(|i| bus_rider_spot(&g, i)).collect();
        assert!(before.iter().all(|s| s.is_some()), "nine riders all fit on the roof");
        scene.build(&g, &g.camera(16.0 / 9.0));
        assert_eq!(scene.stats.characters, 9);
        let stood = scene.actor_pose(&g, PLAYER).unwrap().pelvis.w_axis.truncate();
        g.update(1.0 / 60.0, &PlayerInput { exit_bus: true, ..Default::default() });
        assert_eq!(g.actors[PLAYER].mode, MoveMode::Freefall);
        let after: Vec<Option<RiderSpot>> = (0..g.actors.len()).map(|i| bus_rider_spot(&g, i)).collect();
        assert_eq!(before, after, "the spots do not move when someone leaves");
        scene.build(&g, &g.camera(16.0 / 9.0));
        // the jumper is still drawn where they stood (the bus has moved on a frame), the others on the roof as before
        assert_eq!(scene.stats.characters, 9);
        let bus_moved = g.bus.speed / 60.0;
        let now = scene.actor_pose(&g, PLAYER).unwrap().pelvis.w_axis.truncate();
        assert!(now.distance(stood) < bus_moved + 0.1, "the jumper pops from {stood:?} to {now:?}");
        let to_bus = bus_matrix(&g).inverse();
        assert_eq!(torsos(&scene).iter().filter(|(t, _)| to_bus.transform_point3(*t).y > crate::models::BUS_ROOF_Y).count(), 9, "everyone on the roof");
        // a moment later the jumper is drawn falling where the simulation has them
        for _ in 0..(HANDOFF_TIME * 60.0) as usize + 2 {
            g.update(1.0 / 60.0, &PlayerInput::default());
            scene.build(&g, &g.camera(16.0 / 9.0));
        }
        let p = g.actors[PLAYER].pos;
        let drawn = torsos(&scene);
        assert!(drawn.iter().any(|(t, _)| t.distance(p + Vec3::Y) < 1.5), "the jumper in free fall at {p:?}");
        let fall = rig::pose(&g.actors[PLAYER]);
        assert_eq!(scene.actor_pose(&g, PLAYER).unwrap().torso, fall.torso, "the jumper is drawn as the simulation has them");
        let to_bus = bus_matrix(&g).inverse();
        for i in (0..g.actors.len()).filter(|&i| g.actors[i].mode == MoveMode::Bus) {
            // (the camera follows the jumper down, so the bus may be out of sight: ask how they would be drawn)
            let l = to_bus.transform_point3(scene.actor_pose(&g, i).unwrap().pelvis.w_axis.truncate());
            let spot = bus_rider_spot(&g, i).unwrap().pos;
            assert!(Vec2::new(l.x - spot.x, l.z - spot.z).length() < 0.1 && l.y > spot.y + 0.8, "rider {i} still on their spot");
        }
        // once the bus has gone, nobody is drawn on it
        g.bus.active = false;
        for a in g.actors.iter_mut().skip(1) {
            a.mode = MoveMode::Freefall;
        }
        scene.build(&g, &g.camera(16.0 / 9.0));
        assert!(scene.instances.iter().all(|i| i.is_sane()));
    }

    // ---- clearance on and around the bus ---------------------------------------------------------------------------------

    /// The meshes a posed character is drawn with (as [`Scene::draw_pose`] draws them in `mode`) and where.
    fn posed_parts(mode: GameMode, a: &Actor, p: &rig::Pose) -> Vec<(MeshId, Mat4)> {
        let mut v = vec![(MeshId::CharTorso, p.torso), (MeshId::CharTrim, p.torso), (MeshId::CharPelvis, p.pelvis), (MeshId::CharHead, p.head), (MeshId::CharBackpack, p.backpack)];
        v.extend(rig::hair_model(&a.outfit).map(|h| (h, p.head)));
        v.extend(rig::headgear_model(&a.outfit).map(|h| (h, p.head)));
        for i in 0..2 {
            v.extend([(MeshId::CharArmUp, p.arm_up[i]), (MeshId::CharArmLow, p.arm_low[i]), (MeshId::CharHand, p.hand[i]), (MeshId::CharLegUp, p.leg_up[i]), (MeshId::CharLegLow, p.leg_low[i]), (MeshId::CharBoot, p.boot[i])]);
        }
        v.into_iter().map(|(m, x)| (crate::lego_models::mapped_mesh(m, mode), x)).collect()
    }

    /// A posed character as clouds of points, one per part, each with its bounds, transformed by `to` (into the bus's frame).
    fn body_points(meshes: &[MeshData], mode: GameMode, a: &Actor, p: &rig::Pose, to: Mat4) -> Vec<(Aabb, Vec<Vec3>)> {
        posed_parts(mode, a, p)
            .into_iter()
            .map(|(mesh, m)| {
                let m = to * m;
                let pts: Vec<Vec3> = meshes[mesh as usize].verts.iter().map(|v| m.transform_point3(Vec3::from(v.pos))).collect();
                (pts.iter().fold(Aabb::EMPTY, |mut b, q| { b.extend(*q); b }), pts)
            })
            .collect()
    }

    fn grow(b: &Aabb, r: f32) -> Aabb {
        Aabb { min: b.min - Vec3::splat(r), max: b.max + Vec3::splat(r) }
    }

    fn overlaps(a: &Aabb, b: &Aabb) -> bool {
        a.min.cmple(b.max).all() && b.min.cmple(a.max).all()
    }

    fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
        let ab = b - a;
        a + ab * ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0)
    }

    /// Distance from `p` to the triangle `t`.
    fn tri_dist(p: Vec3, t: &[Vec3; 3]) -> f32 {
        let n = (t[1] - t[0]).cross(t[2] - t[0]);
        if n.length_squared() > 1e-12 {
            let n = n.normalize();
            let q = p - n * (p - t[0]).dot(n);
            let inside = (0..3).all(|k| (t[(k + 1) % 3] - t[k]).cross(q - t[k]).dot(n) >= 0.0);
            if inside {
                return (p - q).length();
            }
        }
        (0..3).map(|k| p.distance(closest_on_segment(p, t[k], t[(k + 1) % 3]))).fold(f32::MAX, f32::min)
    }

    /// What riders could bump into on and around the bus, in the bus's frame: everything bolted onto it above the roof
    /// (rack rails, posts and runners, luggage, the top of the ladder) as triangles with their bounds, and the cables.
    fn bus_obstacles(meshes: &[MeshData]) -> (Vec<([Vec3; 3], Aabb)>, Vec<(Vec3, Vec3)>) {
        let roof = crate::models::BUS_ROOF_Y;
        let m = &meshes[MeshId::Bus as usize];
        let tris = m
            .idx
            .chunks(3)
            .map(|t| [0, 1, 2].map(|k| Vec3::from(m.verts[t[k] as usize].pos)))
            .filter(|t| t.iter().all(|q| q.y > roof - 0.01) && t.iter().any(|q| q.y > roof + 0.002) && t.iter().all(|q| q.y < 4.0))
            .map(|t| (t, t.iter().fold(Aabb::EMPTY, |mut b, q| { b.extend(*q); b })))
            .collect();
        (tris, crate::models::bus_cables().to_vec())
    }

    /// The closest any of `body`'s points comes to an obstacle (cables measured to their surface), and to what.
    fn obstacle_gap(body: &[(Aabb, Vec<Vec3>)], tris: &[([Vec3; 3], Aabb)], cables: &[(Vec3, Vec3)]) -> (f32, Vec3, &'static str) {
        let mut worst = (f32::MAX, Vec3::ZERO, "");
        for (bb, pts) in body {
            let near = grow(bb, 0.1);
            for (t, tb) in tris {
                if !overlaps(&near, tb) {
                    continue;
                }
                for q in pts {
                    let d = tri_dist(*q, t);
                    if d < worst.0 {
                        worst = (d, *q, "the rack");
                    }
                }
            }
            for (a, b) in cables {
                for q in pts {
                    let d = q.distance(closest_on_segment(*q, *a, *b)) - crate::models::BUS_CABLE_R;
                    if d < worst.0 {
                        worst = (d, *q, "a cable");
                    }
                }
            }
        }
        worst
    }

    /// The closest two bodies' points come to each other.
    fn body_gap(a: &[(Aabb, Vec<Vec3>)], b: &[(Aabb, Vec<Vec3>)]) -> f32 {
        let mut best = f32::MAX;
        for (ba, pa) in a {
            for (bb, pb) in b {
                if !overlaps(&grow(ba, best.min(0.25)), bb) {
                    continue;
                }
                for p in pa {
                    for q in pb {
                        best = best.min(p.distance(*q));
                    }
                }
            }
        }
        best
    }

    #[test]
    fn nobody_on_the_roof_touches_a_cable_the_rack_or_anyone_else() {
        // every rider in every spot, in every pose and through the whole of their idle motion, with 13, 25 and 40 players
        // and in both looks of the characters: their boots stand on the roof, and they keep clear of the cables, of the
        // rack and of each other
        let meshes = build_all();
        let (tris, cables) = bus_obstacles(&meshes);
        let roof = crate::models::BUS_ROOF_Y;
        for bots in [12usize, 24, 39] {
            let g = game(bots, false);
            let spots: Vec<(usize, RiderSpot)> = (0..g.actors.len()).filter_map(|i| bus_rider_spot(&g, i).map(|s| (i, s))).collect();
            assert_eq!(spots.len(), (bots + 1).min(ROOF_RIDERS));
            for mode in [GameMode::BattleRoyale, GameMode::Lego] {
                let (mut worst_obstacle, mut worst_pair) = ((f32::MAX, Vec3::ZERO, ""), (f32::MAX, 0, 0));
                for k in 0..60 {
                    let t = k as f32 * 0.1;
                    let sole = -meshes[crate::lego_models::mapped_mesh(MeshId::CharBoot, mode) as usize].bounds().min.y;
                    let bodies: Vec<Vec<(Aabb, Vec<Vec3>)>> = spots
                        .iter()
                        .map(|(i, s)| {
                            let a = &g.actors[*i];
                            let root = Mat4::from_translation(s.pos) * Mat4::from_rotation_y(s.yaw);
                            body_points(&meshes, mode, a, &rig::bus_rider_pose(a, root, s.yaw, s.pose, t, sole), Mat4::IDENTITY)
                        })
                        .collect();
                    for (n, body) in bodies.iter().enumerate() {
                        // the boots: on the roof, not in it
                        let lowest = body.iter().map(|(b, _)| b.min.y).fold(f32::MAX, f32::min) - roof;
                        assert!((-0.001..0.003).contains(&lowest), "{mode:?}, {bots} bots: rider {} stands {lowest} off the roof", spots[n].0);
                        let o = obstacle_gap(body, &tris, &cables);
                        if o.0 < worst_obstacle.0 {
                            worst_obstacle = o;
                        }
                        for m in n + 1..bodies.len() {
                            if spots[n].1.pos.distance(spots[m].1.pos) < 1.6 {
                                let d = body_gap(body, &bodies[m]);
                                if d < worst_pair.0 {
                                    worst_pair = (d, spots[n].0, spots[m].0);
                                }
                            }
                        }
                    }
                }
                eprintln!("{mode:?}, {} players: nearest obstacle {:.3} ({} at {:?}), nearest neighbours {:.3} (riders {} and {})", bots + 1, worst_obstacle.0, worst_obstacle.2, worst_obstacle.1, worst_pair.0, worst_pair.1, worst_pair.2);
                assert!(worst_obstacle.0 > 0.04, "{mode:?}, {bots} bots: a rider touches {} at {:?} ({})", worst_obstacle.2, worst_obstacle.1, worst_obstacle.0);
                assert!(worst_pair.0 > 0.03, "{mode:?}, {bots} bots: riders {} and {} touch ({})", worst_pair.1, worst_pair.2, worst_pair.0);
            }
        }
    }

    #[test]
    fn riders_jump_off_the_roof_in_one_smooth_move_without_going_through_anything() {
        // a whole ride with everyone jumping in turn, the player half way: each jumper is drawn moving smoothly from their
        // spot to where the simulation has them, never through the bus, its rack or cables, or anyone still on the roof
        let meshes = build_all();
        let (tris, cables) = bus_obstacles(&meshes);
        let roof = crate::models::BUS_ROOF_Y;
        let mut g = game(12, false);
        let mut scene = Scene::new(&meshes);
        let dt = 1.0 / 60.0;
        let n = g.actors.len();
        let mut last: Vec<Option<Vec3>> = vec![None; n];
        let (mut handoffs, mut worst_step, mut worst_sim_step, mut worst_gap) = (0, 0.0f32, 0.0f32, (f32::MAX, Vec3::ZERO, ""));
        let mut last_sim: Vec<Vec3> = vec![Vec3::ZERO; n];
        let mut jumped_at: Vec<Option<f32>> = vec![None; n];
        for _ in 0..60 * 120 {
            let jump = g.bus.t > 4.0 && g.actors[PLAYER].mode == MoveMode::Bus;
            g.update(dt, &PlayerInput { exit_bus: jump, ..Default::default() });
            scene.build(&g, &g.camera(16.0 / 9.0));
            if !g.bus.active {
                break;
            }
            let to_bus = bus_matrix(&g).inverse();
            let riders: Vec<Vec<(Aabb, Vec<Vec3>)>> = (0..n)
                .filter(|&i| g.actors[i].mode == MoveMode::Bus)
                .filter_map(|i| scene.actor_pose(&g, i).map(|p| body_points(&meshes, g.cfg.mode, &g.actors[i], &p, to_bus)))
                .collect();
            for i in 0..n {
                let a = &g.actors[i];
                let Some(p) = scene.actor_pose(&g, i) else {
                    last[i] = None;
                    continue;
                };
                let at = to_bus.transform_point3(p.pelvis.w_axis.truncate());
                let sim = to_bus.transform_point3(a.pos);
                if a.mode != MoveMode::Bus && jumped_at[i].is_none() {
                    jumped_at[i] = Some(g.time);
                }
                let since = jumped_at[i].map_or(-1.0, |t0| g.time - t0);
                if let Some(prev) = last[i] {
                    // relative to the bus, a jumper never moves much faster than the simulation moves them
                    if (0.0..HANDOFF_TIME + 0.1).contains(&since) {
                        worst_step = worst_step.max(at.distance(prev));
                        worst_sim_step = worst_sim_step.max(sim.distance(last_sim[i]));
                    } else if a.mode == MoveMode::Bus {
                        assert!(at.distance(prev) < 0.05, "rider {i} shuffles on the roof");
                    }
                }
                last[i] = Some(at);
                last_sim[i] = sim;
                if (0.0..HANDOFF_TIME).contains(&since) && scene.handoffs.iter().any(|h| h.actor == i) {
                    handoffs += 1;
                    let body = body_points(&meshes, g.cfg.mode, a, &p, to_bus);
                    // not into the bus: its body and bumpers, its wheels or its mirrors (boxes round them)
                    for (_, pts) in &body {
                        for q in pts {
                            let wheels = (-5.3..-4.0).contains(&q.z) || (2.2..3.5).contains(&q.z);
                            let in_bus = (q.x.abs() < 1.52 && (-1.12..roof - 0.004).contains(&q.y) && (-5.75..4.45).contains(&q.z)) || (q.x.abs() < 1.65 && (-1.72..-0.48).contains(&q.y) && wheels);
                            let in_mirror = (1.65..1.85).contains(&q.x.abs()) && (0.1..0.46).contains(&q.y) && (-4.4..-4.15).contains(&q.z);
                            assert!(!in_bus && !in_mirror, "rider {i} goes through the bus at {q:?}, {since} s after jumping");
                        }
                    }
                    let o = obstacle_gap(&body, &tris, &cables);
                    if o.0 < worst_gap.0 {
                        worst_gap = o;
                    }
                    for other in &riders {
                        let d = body_gap(&body, other);
                        if d < worst_gap.0 {
                            worst_gap = (d, Vec3::ZERO, "a rider still on the roof");
                        }
                    }
                }
            }
        }
        eprintln!("hand-off frames {handoffs}, biggest step {worst_step:.3} m (the simulation's {worst_sim_step:.3}), nearest miss {:.3} ({} at {:?})", worst_gap.0, worst_gap.2, worst_gap.1);
        assert!(jumped_at.iter().filter(|t| t.is_some()).count() == n, "everyone jumped before the bus left");
        assert!(handoffs > 20 * 12, "jumpers were drawn on their way off the roof ({handoffs} frames)");
        // (the old drawing popped a jumper 4 m in one frame, from the roof into the bus)
        assert!(worst_step < 1.5 && worst_step < 1.35 * worst_sim_step, "a jumper moves {worst_step} m in one frame");
        assert!(worst_gap.0 > 0.02, "a jumper touches {} at {:?} ({})", worst_gap.2, worst_gap.1, worst_gap.0);
    }

    #[test]
    fn the_bus_camera_sees_the_riders_past_the_balloon_however_the_player_looks() {
        // whichever way the player turns and however far they try to look down or up, the camera stays out of the envelope,
        // the player is in sight, and the balloon never fills much of the screen (looking steeply down used to bury the view
        // in it)
        let meshes = build_all();
        let mut g = game(12, false);
        g.update(1.0 / 60.0, &PlayerInput::default());
        let bus = bus_matrix(&g);
        let to_bus = bus.inverse();
        let m = &meshes[MeshId::Bus as usize];
        let tris: Vec<[Vec3; 3]> = m.idx.chunks(3).map(|t| [0, 1, 2].map(|k| bus.transform_point3(Vec3::from(m.verts[t[k] as usize].pos)))).collect();
        let in_envelope = |q: Vec3| crate::models::bus_envelope_radius(q.y).is_some_and(|r| Vec2::new(q.x, q.z).length() < r);
        let me = bus_rider_spot(&g, g.local).unwrap();
        let head = bus.transform_point3(me.pos + Vec3::Y * 1.6);
        let bus_yaw = yaw_of(g.bus.dir);
        let mut worst = (0.0f32, 0.0f32, 0.0f32);
        use std::f32::consts::{FRAC_PI_2, PI};
        for turn in [0.0f32, 0.8, FRAC_PI_2, 2.4, PI, -FRAC_PI_2] {
            for k in 0..=20 {
                let pitch = lerp(1.5, -1.45, k as f32 / 20.0);
                let a = &mut g.actors[PLAYER];
                (a.yaw, a.pitch) = (bus_yaw + turn, pitch);
                let cam = g.camera(16.0 / 9.0);
                assert!(!in_envelope(to_bus.transform_point3(cam.pos)), "the camera is inside the balloon looking {turn} round and {pitch} up");
                // (from straight ahead of the bus, the balloon's mouth may stand between the camera and the player at the back)
                if turn.abs() < 3.0 {
                    let hidden = (0..=40).any(|j| in_envelope(to_bus.transform_point3(cam.pos.lerp(head, j as f32 / 40.0))));
                    assert!(!hidden, "the balloon hides the player looking {turn} round and {pitch} up");
                }
                let mut balloon = 0;
                for (x, y) in (0..24).flat_map(|x| (0..14).map(move |y| (x, y))) {
                    let d = cam.ray(Vec2::new((x as f32 + 0.5) / 12.0 - 1.0, (y as f32 + 0.5) / 7.0 - 1.0));
                    let hit = tris.iter().filter_map(|t| ray_tri(cam.pos, d, t[0], t[1], t[2])).fold(f32::MAX, f32::min);
                    if hit < f32::MAX && to_bus.transform_point3(cam.pos + d * hit).y > 4.8 {
                        balloon += 1;
                    }
                }
                let cover = balloon as f32 / (24.0 * 14.0);
                if cover > worst.0 {
                    worst = (cover, turn, pitch);
                }
            }
        }
        eprintln!("the balloon fills at most {:.0}% of the screen (looking {} round and {} up)", worst.0 * 100.0, worst.1, worst.2);
        assert!(worst.0 < 0.3, "the balloon fills {:.0}% of the screen looking {} round and {} up", worst.0 * 100.0, worst.1, worst.2);
    }

    #[test]
    fn on_the_bus_the_player_looks_down_only_as_far_as_the_balloon_allows() {
        let mut g = game(4, false);
        let down = PlayerInput { look: Vec2::new(0.0, -0.3), ..Default::default() };
        for _ in 0..20 {
            g.update(1.0 / 60.0, &down);
        }
        assert_eq!(g.actors[PLAYER].mode, MoveMode::Bus);
        assert_eq!(g.actors[PLAYER].pitch, BUS_PITCH_MIN);
        // and off the bus they can look further down again
        g.update(1.0 / 60.0, &PlayerInput { exit_bus: true, ..Default::default() });
        for _ in 0..20 {
            g.update(1.0 / 60.0, &down);
        }
        assert_eq!(g.actors[PLAYER].pitch, pitch_limits(MoveMode::Freefall).0);
    }
}
