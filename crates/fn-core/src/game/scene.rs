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
}

/// sRGB-ish colour to the linear tint the shader multiplies with.
pub fn lin(c: Vec3) -> [f32; 4] {
    [c.x.max(0.0).powf(2.2), c.y.max(0.0).powf(2.2), c.z.max(0.0).powf(2.2), 1.0]
}

fn casts_shadow(m: MeshId) -> bool {
    use MeshId::*;
    matches!(
        m,
        CharTorso | CharTrim | CharPelvis | CharHead | CharArmUp | CharArmLow | CharLegUp | CharLegLow | CharBoot | CharBackpack | Glider | PieceWall | PieceFloor | PieceRamp | PieceRoof | ChestBase | ChestLid | Bus | Hair1 | Hair2 | Hair3 | Cap | Beanie | Helmet | Hat | VehicleBody | VehicleWheel
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
        PickupKind::Consumable { kind, .. } => match kind {
            ConsumableKind::Bandage => 0.5,
            ConsumableKind::MedKit => 0.62,
            ConsumableKind::ShieldSmall => 0.5,
            ConsumableKind::ShieldBig => 0.58,
            ConsumableKind::ChugJug => 0.66,
        },
    }
}

/// How long a healing item is in the player's hand.
pub const HELD_ITEM_SIZE: f32 = 0.3;

/// Scale that makes a mesh with half extents `half` about `target` metres across its longest side.
pub fn fit_scale(half: Vec3, target: f32) -> f32 {
    let longest = (half.max_element() * 2.0).max(0.02);
    (target / longest).clamp(0.25, 8.0)
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
        }
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
        let fr = cam.frustum();
        let cp = cam.pos;
        self.bus(g);
        self.vehicles(g, &fr, cp);
        for a in &g.actors {
            if g.vehicle_for_actor(a.id).is_none() { self.character(a, &fr, cp); }
        }
        self.pickups(g, &fr, cp);
        self.chests(g, &fr, cp);
        self.pieces(g, &fr, cp);
        self.ghost_piece(g);
        self.projectiles(g);
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
            self.push(MeshId::VehicleBody, base, lin(crate::mesh::hex(palette[v.id.saturating_sub(1) as usize % palette.len()])));
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

    fn character(&mut self, a: &Actor, fr: &Frustum, cp: Vec3) {
        if !a.alive && a.dead_time > 0.9 {
            return;
        }
        if matches!(a.mode, MoveMode::Bus) {
            return;
        }
        let gliding = matches!(a.mode, MoveMode::Glide);
        let center = a.pos + Vec3::Y * if gliding { 2.0 } else { 1.0 };
        if a.pos.distance(cp) > 420.0 || !fr.intersects_sphere(center, if gliding { 4.2 } else { 2.4 }) {
            return;
        }
        let opacity = if a.alive { 1.0 } else { (1.0 - a.dead_time / 0.9).clamp(0.0, 1.0) };
        if opacity <= 0.02 {
            return;
        }
        let p = rig::pose(a);
        self.stats.characters += 1;
        let o = &a.outfit;
        let flash = a.hit_flash * 0.35 + if a.alive { 0.0 } else { (1.0 - opacity) * 0.8 };
        let params = [flash, opacity, 0.0, 0.0];
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
                _ => (WHITE, m),
            };
            put(self, mesh, m, tint);
        }
        if let Some(m) = p.glider {
            put(self, MeshId::Glider, m, lin(o.glider));
        }
    }

    // ---- bus --------------------------------------------------------------------------------------------------

    fn bus(&mut self, g: &Game) {
        if !g.bus.active {
            return;
        }
        let t = g.time;
        let yaw = yaw_of(g.bus.dir);
        let m = Mat4::from_translation(g.bus.pos + Vec3::new(0.0, (t * 0.9).sin() * 0.25, 0.0)) * Mat4::from_rotation_y(yaw) * Mat4::from_rotation_z((t * 0.7).sin() * 0.025) * Mat4::from_rotation_x((t * 0.5).sin() * 0.015);
        self.push(MeshId::Bus, m, WHITE);
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

    fn chests(&mut self, g: &Game, fr: &Frustum, cp: Vec3) {
        let t = g.time;
        for c in &g.chests {
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
            let kinds: Vec<PickupKind> = ConsumableKind::ALL.iter().map(|&kind| PickupKind::Consumable { kind, count: 1 }).chain(AmmoKind::ALL.iter().map(|&kind| PickupKind::Ammo { kind, amount: 1 })).collect();
            for kind in kinds {
                let mesh = rig::pickup_model(&kind);
                let (_, half) = scene.centers[crate::lego_models::mapped_mesh(mesh, mode) as usize];
                let on_floor = half.max_element() * 2.0 * fit_scale(half, floor_item_size(&kind));
                assert!((0.4..=0.8).contains(&on_floor), "{mode:?} {kind:?} is {on_floor:.2} m on the ground");
                let in_hand = half.max_element() * 2.0 * scene.fit_to(mesh, HELD_ITEM_SIZE);
                assert!((0.22..=0.4).contains(&in_hand), "{mode:?} {kind:?} is {in_hand:.2} m in the hand");
            }
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
}
