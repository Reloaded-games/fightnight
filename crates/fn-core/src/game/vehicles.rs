//! Original cartoon buggies. The room host owns physics and occupancy; replicas draw its snapshots.

use super::actor::{Action, Actor, MoveMode, RADIUS};
use super::env::Env;
use super::intent::Intent;
use super::Game;
use crate::math::*;
use crate::mesh::{mat, MeshBuilder, MeshData};
use crate::meshlib::MeshId;
use crate::world::collision::SpatialGrid;
use crate::world::{World, WORLD_HALF};

pub const ENTER_RANGE: f32 = 3.4;
pub const MAX_VEHICLES: usize = 32;
pub const TOP_SPEED: f32 = 28.0;
pub const BOOST_SPEED: f32 = 38.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Vehicle {
    pub id: u32,
    /// Ground contact under the centre of the chassis.
    pub pos: Vec3,
    pub yaw: f32,
    /// Signed forward speed in metres per second (negative = reverse).
    pub speed: f32,
    /// Smoothed steering, -1 left .. +1 right.
    pub steer: f32,
    pub driver: Option<usize>,
}

impl Vehicle {
    pub fn new(id: u32, pos: Vec3, yaw: f32) -> Self {
        Self { id, pos, yaw, speed: 0.0, steer: 0.0, driver: None }
    }

    pub fn velocity(&self) -> Vec3 { yaw_forward(self.yaw) * self.speed }

    /// The same stable network id picks the body on hosts and guests without extra snapshot data.
    pub fn body_mesh_id(&self) -> MeshId {
        if self.id.is_multiple_of(3) { MeshId::VehicleSport } else { MeshId::VehicleBody }
    }

    pub fn display_name(&self) -> &'static str {
        if self.body_mesh_id() == MeshId::VehicleSport { "Island Roadster" } else { "Island Buggy" }
    }

    /// Actor feet pivot while the hips sit on the driver's seat. Rendering uses a seated pose.
    pub fn driver_position(&self) -> Vec3 {
        self.pos - yaw_right(self.yaw) * 0.38 - yaw_forward(self.yaw) * 0.30 - Vec3::Y * 0.04
    }

    pub fn seat_pos(&self) -> Vec3 { self.driver_position() }
}

fn clear_chassis(env: &Env<'_>, pos: Vec3, yaw: f32) -> bool {
    let f = yaw_forward(yaw);
    [-0.80, 0.0, 0.80].iter().all(|z| {
        env.push_out(pos + f * *z + Vec3::Y * 0.30, 1.0, 1.25, 0.5).length_squared() < 0.004
    })
}

/// Deterministic parking places on roads and at village entries; uses no simulation RNG.
pub fn spawn(world: &World) -> Vec<Vehicle> {
    let pieces = SpatialGrid::new(WORLD_HALF + 16.0, 8.0);
    let env = Env::new(world, &pieces);
    let mut vehicles = Vec::new();
    let mut candidates = Vec::new();
    for poi in &world.layout.pois {
        for entry in poi.entries() {
            let dir = (entry - poi.center).normalize_or_zero();
            candidates.push((entry + dir * 5.0, if dir == Vec2::ZERO { Vec2::Y } else { dir }));
        }
    }
    for road in &world.layout.roads {
        for fraction in [0.25, 0.55, 0.80] {
            let index = ((road.pts.len().saturating_sub(1)) as f32 * fraction) as usize;
            if let (Some(p), Some(next)) = (road.pts.get(index), road.pts.get(index + 1)) {
                let dir = (*next - *p).normalize_or_zero();
                let side = Vec2::new(-dir.y, dir.x);
                candidates.push((*p + side * road.width * 0.12, dir));
            }
        }
    }
    for (point, dir) in candidates {
        if vehicles.len() >= MAX_VEHICLES { break; }
        if vehicles.iter().any(|v: &Vehicle| Vec2::new(v.pos.x, v.pos.z).distance(point) < 28.0) { continue; }
        let h = world.hm.height_at(point.x, point.y);
        let pos = Vec3::new(point.x, h, point.y);
        let yaw = yaw_of(dir);
        if h > 1.0 && world.hm.slope_at(point.x, point.y) < 0.42 && env.water_depth(point.x, point.y) < 0.25 && clear_chassis(&env, pos, yaw) {
            vehicles.push(Vehicle::new(vehicles.len() as u32 + 1, pos, yaw));
        }
    }
    vehicles
}

/// One deterministic physics step; usable by the host or by a client predicting a driver.
pub fn step_vehicle(v: &mut Vehicle, env: &Env<'_>, it: &Intent, dt: f32) {
    let dt = dt.clamp(0.0, 0.1);
    let f = yaw_forward(it.yaw);
    let r = yaw_right(it.yaw);
    let throttle = it.wish.dot(Vec2::new(f.x, f.z)).clamp(-1.0, 1.0);
    let steering = it.wish.dot(Vec2::new(r.x, r.z)).clamp(-1.0, 1.0);
    v.steer = lerp(v.steer, steering, damp(8.0, dt));
    if it.jump || it.crouch {
        v.speed = approach(v.speed, 0.0, 28.0 * dt);
    } else if throttle.abs() > 0.01 {
        let limit = if throttle < 0.0 { -9.0 } else if it.sprint { BOOST_SPEED } else { TOP_SPEED };
        let accel = if throttle.signum() != v.speed.signum() && v.speed.abs() > 0.5 { 24.0 } else { 11.0 };
        v.speed = approach(v.speed, limit * throttle.abs(), accel * dt);
    } else {
        v.speed = approach(v.speed, 0.0, (1.4 + v.speed.abs() * 0.10) * dt);
    }
    v.speed = v.speed.clamp(-9.0, BOOST_SPEED);
    let old_pos = v.pos;
    let old_yaw = v.yaw;
    // Turning tightens at low speed; at high speed a broad arc keeps the buggy stable.
    let turn = (v.speed / 2.6).clamp(-1.7, 1.7);
    v.yaw = wrap_pi(v.yaw - v.steer * turn * dt);
    let mut next = v.pos + v.velocity() * dt;
    let edge = WORLD_HALF - 8.0;
    next.x = next.x.clamp(-edge, edge);
    next.z = next.z.clamp(-edge, edge);
    let ground = env.ground(next.x, next.z, old_pos.y + 0.35, 0.60);
    let water = env.water_level(next.x, next.z).is_some_and(|level| level - ground.y > 0.45);
    let steep = ground.y - old_pos.y > 0.75 || env.world.hm.slope_at(next.x, next.z) > 0.85;
    next.y = ground.y;
    if water || steep || !clear_chassis(env, next, v.yaw) {
        v.pos = old_pos;
        v.yaw = old_yaw;
        v.speed *= -0.12;
    } else {
        v.pos = next;
    }
}

/// The same step including collisions with the other vehicles; shared by authority and driver prediction.
pub fn step_fleet(vehicles: &mut [Vehicle], index: usize, env: &Env<'_>, it: &Intent, dt: f32) {
    let old_pos = vehicles[index].pos;
    step_vehicle(&mut vehicles[index], env, it, dt);
    let touching = vehicles.iter().enumerate().any(|(i, other)| i != index && other.pos.distance(vehicles[index].pos) < 2.4);
    if touching {
        let v = &mut vehicles[index];
        v.pos = old_pos;
        v.speed *= -0.15;
    }
}

impl Game {
    pub fn vehicle_for_actor(&self, actor: usize) -> Option<&Vehicle> {
        self.vehicles.iter().find(|v| v.driver == Some(actor))
    }

    pub fn nearby_vehicle(&self, actor: usize) -> Option<&Vehicle> {
        let a = &self.actors[actor];
        if !a.alive || a.mode != MoveMode::Ground { return None; }
        self.vehicles.iter().filter(|v| v.driver.is_none() && v.pos.distance(a.pos) < ENTER_RANGE)
            .min_by(|a, b| a.pos.distance_squared(self.actors[actor].pos).total_cmp(&b.pos.distance_squared(self.actors[actor].pos)))
    }

    pub fn enter_vehicle(&mut self, actor: usize, id: u32) -> bool {
        if self.vehicle_for_actor(actor).is_some() { return false; }
        let Some(index) = self.vehicles.iter().position(|v| v.id == id && v.driver.is_none()) else { return false; };
        let a = &self.actors[actor];
        if !a.alive || !a.human || a.mode != MoveMode::Ground || self.vehicles[index].pos.distance(a.pos) > ENTER_RANGE { return false; }
        let v = &mut self.vehicles[index];
        v.driver = Some(actor);
        a_seat(&mut self.actors[actor], v);
        self.actors[actor].yaw = v.yaw;
        self.actors[actor].pitch = -0.12;
        let name = v.display_name();
        self.toast_to(actor, format!("{name}: WASD drive · Shift boost · Space brake · E exit"), 5.0, 1);
        true
    }

    pub fn exit_vehicle(&mut self, actor: usize) -> bool {
        let Some(index) = self.vehicles.iter().position(|v| v.driver == Some(actor)) else { return false; };
        let v = &self.vehicles[index];
        let env = self.env();
        let right = yaw_right(v.yaw);
        let forward = yaw_forward(v.yaw);
        let mut exit = None;
        for offset in [right * -2.4, right * 2.4, forward * -3.0, forward * 3.0] {
            let mut point = v.pos + offset;
            let floor = env.ground(point.x, point.z, v.pos.y + 0.7, 0.6);
            point.y = floor.y;
            if floor.y - v.pos.y < 1.2 && env.water_depth(point.x, point.z) < 1.5 && env.push_out(point, RADIUS, 1.78, 0.55).length_squared() < 0.002 {
                exit = Some(point);
                break;
            }
        }
        let Some(point) = exit else {
            let name = v.display_name();
            self.toast_to(actor, format!("Exit blocked. Move the {name} to open ground."), 2.0, 3);
            return false;
        };
        self.vehicles[index].driver = None;
        self.vehicles[index].speed *= 0.40;
        let a = &mut self.actors[actor];
        a.pos = point;
        a.vel = Vec3::ZERO;
        a.eye_smooth = point.y;
        a.on_ground = true;
        a.peak_y = point.y;
        a.anim.run = 0.0;
        a.anim.sprint = 0.0;
        true
    }

    /// Consumes an actor's tick while entering, leaving or driving a vehicle.
    pub fn drive_vehicle(&mut self, actor: usize, it: &Intent, dt: f32) -> bool {
        if let Some(index) = self.vehicles.iter().position(|v| v.driver == Some(actor)) {
            if it.interact { self.exit_vehicle(actor); return true; }
            let env = Env::new(&self.world, &self.pieces.grid);
            step_fleet(&mut self.vehicles, index, &env, it, dt);
            a_seat(&mut self.actors[actor], &self.vehicles[index]);
            self.actors[actor].anim.time += dt;
            return true;
        }
        if self.actors[actor].human && it.interact && self.find_target(actor).is_none() {
            if let Some(id) = self.nearby_vehicle(actor).map(|v| v.id) {
                return self.enter_vehicle(actor, id);
            }
        }
        false
    }

    pub(super) fn update_empty_vehicles(&mut self, dt: f32) {
        let env = Env::new(&self.world, &self.pieces.grid);
        for v in &mut self.vehicles {
            if v.driver.is_some_and(|i| !self.actors[i].alive || !self.actors[i].human) {
                v.driver = None;
                v.speed = 0.0;
            }
            if v.driver.is_none() && v.speed.abs() > 0.02 {
                step_vehicle(v, &env, &Intent { yaw: v.yaw, ..Default::default() }, dt);
            }
        }
    }

    /// Pedestrians cannot walk through a parked chassis. Roof-height actors remain free to jump over it.
    pub(super) fn avoid_vehicles(&mut self, actor: usize) {
        avoid_chassis(&mut self.actors[actor], &self.vehicles);
    }
}

/// Shared pedestrian collision for authoritative movement and a guest's predicted actor.
pub fn avoid_chassis(a: &mut Actor, vehicles: &[Vehicle]) {
    if a.mode != MoveMode::Ground { return; }
    for v in vehicles {
        if v.driver == Some(a.id) || a.pos.y > v.pos.y + 1.55 || a.pos.y + a.height() < v.pos.y + 0.3 { continue; }
        let forward = yaw_forward(v.yaw);
        let right = yaw_right(v.yaw);
        let offset = a.pos - v.pos;
        if offset.length_squared() > 14.0 { continue; }
        let local = Vec2::new(offset.dot(right), offset.dot(forward));
        let closest = Vec2::new(local.x.clamp(-0.95, 0.95), local.y.clamp(-1.60, 1.60));
        let delta = local - closest;
        let distance = delta.length();
        let push = if distance > 0.0001 && distance < RADIUS {
            delta / distance * (RADIUS - distance)
        } else if distance <= 0.0001 {
            Vec2::new(if local.x >= 0.0 { 0.95 + RADIUS - local.x } else { -0.95 - RADIUS - local.x }, 0.0)
        } else { Vec2::ZERO };
        a.pos += right * push.x + forward * push.y;
    }
}

fn a_seat(a: &mut Actor, v: &Vehicle) {
    a.pos = v.driver_position();
    a.vel = v.velocity();
    a.body_yaw = v.yaw;
    a.eye_smooth = a.pos.y;
    a.mode = MoveMode::Ground;
    a.on_ground = true;
    a.crouching = false;
    a.sprinting = false;
    a.ads = false;
    a.build_mode = false;
    a.emoting = false;
    a.action = Action::None;
    a.melee_pending = false;
    a.anim.run = 0.0;
    a.anim.sprint = 0.0;
    a.anim.aim = 0.0;
    a.anim.emote = 0.0;
    a.anim.swing = 0.0;
}

/// An open two-seat buggy with a readable silhouette and a tintable painted chassis.
pub fn body_mesh() -> MeshData {
    let mut b = MeshBuilder::new();
    b.mat(mat::METAL).spec(0.30).tinted(true).hex(0xffffff).ao(0.55, 1.0);
    b.box_center(Vec3::new(0.0, 0.60, 0.0), Vec3::new(0.90, 0.18, 1.54));
    b.box_center(Vec3::new(0.0, 0.89, -1.0), Vec3::new(0.86, 0.20, 0.58));
    b.box_center(Vec3::new(0.0, 0.85, 1.23), Vec3::new(0.86, 0.17, 0.31));
    for x in [-0.87, 0.87] {
        b.box_center(Vec3::new(x, 0.88, 0.38), Vec3::new(0.055, 0.13, 0.55));
        b.box_center(Vec3::new(x, 1.47, 0.90), Vec3::new(0.065, 0.59, 0.065));
    }
    b.box_center(Vec3::new(0.0, 2.03, 0.90), Vec3::new(0.92, 0.065, 0.065));
    b.tinted(false).hex(0x263341).spec(0.1);
    b.box_center(Vec3::new(0.0, 0.46, -1.63), Vec3::new(1.02, 0.10, 0.095));
    b.box_center(Vec3::new(0.0, 0.46, 1.63), Vec3::new(1.02, 0.10, 0.095));
    b.box_center(Vec3::new(0.0, 1.02, -0.38), Vec3::new(0.78, 0.11, 0.085));
    for x in [-0.38, 0.38] {
        b.box_center(Vec3::new(x, 0.79, 0.30), Vec3::new(0.30, 0.12, 0.35));
        b.box_center(Vec3::new(x, 1.12, 0.65), Vec3::new(0.30, 0.27, 0.09));
    }
    b.push_xf(Mat4::from_translation(Vec3::new(-0.38, 1.16, -0.30)) * Mat4::from_rotation_x(0.95));
    // A thin rim, with an open centre, supplies a visible steering wheel.
    for i in 0..12 {
        let angle = i as f32 / 12.0 * std::f32::consts::TAU;
        b.push_xf(Mat4::from_rotation_y(angle));
        b.box_center(Vec3::new(0.0, 0.0, 0.22), Vec3::new(0.061, 0.028, 0.025));
        b.pop_xf();
    }
    b.box_center(Vec3::ZERO, Vec3::new(0.025, 0.025, 0.20));
    b.pop_xf();
    b.hex(0xd4d8dc).mat(mat::METAL).spec(0.50);
    for x in [-0.48, 0.48] {
        b.box_center(Vec3::new(x, 0.65, -1.62), Vec3::new(0.14, 0.11, 0.035));
    }
    b.hex(0xeb5652).mat(mat::EMISSIVE);
    for x in [-0.63, 0.63] {
        b.box_center(Vec3::new(x, 0.85, 1.56), Vec3::new(0.15, 0.08, 0.03));
    }
    b.finish()
}

/// Wheel pivot lies at its axle; the tyre axis is local X.
pub fn wheel_mesh() -> MeshData {
    let mut b = MeshBuilder::new();
    b.push_xf(Mat4::from_rotation_z(-std::f32::consts::FRAC_PI_2));
    b.tinted(false).hex(0x242935).spec(0.0).ao(0.60, 1.0);
    b.cylinder(Vec3::new(0.0, -0.16, 0.0), 0.47, 0.47, 0.32, 14, true, true);
    b.hex(0xc3c9d1).mat(mat::METAL).spec(0.4);
    b.cylinder(Vec3::new(0.0, -0.18, 0.0), 0.24, 0.24, 0.36, 10, true, true);
    b.hex(0x3d4c5b).spec(0.25);
    b.cylinder(Vec3::new(0.0, -0.19, 0.0), 0.10, 0.10, 0.38, 8, true, true);
    b.pop_xf();
    b.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::testutil::{free_spot, game};

    #[test]
    fn both_body_styles_share_the_network_seat_and_remain_stable_while_driving() {
        let mut v = Vehicle::new(3, Vec3::ZERO, 0.0);
        assert_eq!(v.body_mesh_id(), MeshId::VehicleSport);
        assert_eq!(Vehicle::new(1, Vec3::ZERO, 0.0).body_mesh_id(), MeshId::VehicleBody);
        v.pos = Vec3::new(20.0, 2.0, -30.0);
        v.yaw = 1.4; v.speed = 18.0; v.steer = -0.5; v.driver = Some(0);
        assert_eq!(v.body_mesh_id(), MeshId::VehicleSport);
        let mut buggy = v.clone(); buggy.id = 4;
        assert_eq!(buggy.body_mesh_id(), MeshId::VehicleBody);
        assert_eq!(buggy.driver_position(), v.driver_position());
        assert_eq!(buggy.velocity(), v.velocity());
    }

    #[test]
    fn roadster_name_follows_its_visible_body_in_hud_prompts_and_toasts() {
        use super::super::hud;
        let mut g = game(2, true);
        let p = free_spot(&g);
        g.pickups.clear(); g.chests.clear();
        g.vehicles = vec![Vehicle::new(3, p, 0.0)];
        g.actors[0].pos = p - Vec3::X * 2.0;
        assert_eq!(g.vehicles[0].display_name(), "Island Roadster");
        let before = hud::hud_json(&g, &g.camera(1.6), false);
        assert!(before.contains("Drive Island Roadster") && !before.contains("Drive Island Buggy"));
        assert!(g.enter_vehicle(0, 3));
        let seated = hud::hud_json(&g, &g.camera(1.6), false);
        assert!(seated.contains("\"name\":\"Island Roadster\"") && seated.contains("Exit roadster"));
        assert!(g.events.iter().any(|event| matches!(event,
            super::super::events::Event::Toast { actor: Some(0), text, .. }
                if text.starts_with("Island Roadster: WASD"))));
        assert!(g.exit_vehicle(0));
        let after = hud::hud_json(&g, &g.camera(1.6), false);
        assert!(after.contains("Drive Island Roadster"));
    }

    #[test]
    fn parking_is_deterministic_dry_and_clear() {
        let w = super::super::env::testutil::world();
        let a = spawn(w);
        let b = spawn(w);
        assert_eq!(a, b);
        assert!(a.len() >= 12 && a.len() <= MAX_VEHICLES, "{} parking places", a.len());
        let pieces = SpatialGrid::new(WORLD_HALF + 16.0, 8.0);
        let env = Env::new(w, &pieces);
        assert!(a.iter().all(|v| clear_chassis(&env, v.pos, v.yaw) && env.water_depth(v.pos.x, v.pos.z) < 0.25));
    }

    #[test]
    fn acceleration_brake_reverse_and_steering_work() {
        let g = game(2, true);
        let p = free_spot(&g);
        let env = g.env();
        let mut v = Vehicle::new(1, p, 0.0);
        let it = Intent { wish: Vec2::new(0.0, -1.0), yaw: 0.0, ..Default::default() };
        for _ in 0..30 { step_vehicle(&mut v, &env, &it, 1.0 / 60.0); }
        assert!(v.speed > 4.0 && v.pos.distance(p) > 1.0);
        let braking = Intent { jump: true, ..it };
        for _ in 0..30 { step_vehicle(&mut v, &env, &braking, 1.0 / 60.0); }
        assert!(v.speed.abs() < 0.1);
        let reverse = Intent { wish: Vec2::new(0.0, 1.0), ..it };
        for _ in 0..20 { step_vehicle(&mut v, &env, &reverse, 1.0 / 60.0); }
        assert!(v.speed < -2.0);
        v.pos = p; v.speed = 8.0;
        let turn = Intent { wish: Vec2::new(1.0, -1.0), ..it };
        for _ in 0..10 { step_vehicle(&mut v, &env, &turn, 1.0 / 60.0); }
        assert!(v.yaw < -0.08, "right steering yaw {}", v.yaw);
    }

    #[test]
    fn enter_exit_exclusive_seat_and_movement_interception() {
        let mut g = game(2, true);
        let p = free_spot(&g);
        g.vehicles = vec![Vehicle::new(1, p, 0.0)];
        for i in 0..2 { g.actors[i].pos = p - Vec3::X * 2.0; }
        assert!(g.enter_vehicle(0, 1));
        assert!(!g.enter_vehicle(1, 1), "one player per driver's seat");
        let it = Intent { wish: Vec2::new(0.0, -1.0), fire: true, toggle_build: true, ..Default::default() };
        g.apply_intent(0, &it, 1.0 / 60.0);
        assert_eq!(g.actors[0].pos, g.vehicles[0].driver_position());
        assert!(!g.actors[0].build_mode);
        assert!(g.exit_vehicle(0));
        assert!(g.vehicle_for_actor(0).is_none());
        assert!(g.actors[0].pos.distance(g.vehicles[0].pos) > 2.0);
        assert_eq!(g.actors[0].vel, Vec3::ZERO);
    }

    #[test]
    fn terrain_water_and_world_limits_block_cars() {
        let g = game(2, true);
        let env = g.env();
        let l = &g.world.layout.lakes[0];
        let p = Vec3::new(l.center.x, env.terrain(l.center.x, l.center.y), l.center.y);
        let mut v = Vehicle::new(1, p, 0.0);
        v.speed = 10.0;
        step_vehicle(&mut v, &env, &Intent { wish: Vec2::new(0.0, -1.0), ..Default::default() }, 1.0 / 60.0);
        assert_eq!(v.pos, p, "submerged buggy cannot drive through a lake");
        let edge = WORLD_HALF - 8.0;
        v.pos = Vec3::new(edge - 0.01, env.terrain(edge - 0.01, 0.0), 0.0);
        v.yaw = -std::f32::consts::FRAC_PI_2;
        v.speed = 20.0;
        let yaw = v.yaw;
        step_vehicle(&mut v, &env, &Intent { yaw, ..Default::default() }, 0.1);
        assert!(v.pos.x <= edge);
        assert!(v.pos.is_finite() && v.yaw.is_finite() && v.speed.is_finite());
    }

    #[test]
    fn solid_walls_block_driving_and_parked_chassis_block_walkers() {
        use crate::world::collision::{Collider, Tag};
        let mut g = game(2, true);
        let p = free_spot(&g);
        // A wall across the road, 5 m ahead of the front bumper.
        g.pieces.grid.insert(Collider::aabb_box(p + Vec3::new(-5.0, -1.0, -5.5), p + Vec3::new(5.0, 4.0, -5.2), Tag::Static));
        let mut v = Vehicle::new(1, p, 0.0);
        let it = Intent { wish: Vec2::new(0.0, -1.0), ..Default::default() };
        for _ in 0..180 { step_vehicle(&mut v, &g.env(), &it, 1.0 / 60.0); }
        assert!(v.pos.z > p.z - 4.0, "chassis crossed the wall: {}", v.pos.z - p.z);
        g.vehicles = vec![Vehicle::new(1, p, 0.0)];
        g.actors[0].pos = p;
        g.actors[0].mode = MoveMode::Ground;
        g.avoid_vehicles(0);
        assert!(g.actors[0].pos.x - p.x >= 0.95 + RADIUS - 0.001);
    }

    #[test]
    fn interaction_input_hud_and_release_after_death_follow_occupancy() {
        use super::super::{hud, PlayerInput};
        let mut g = game(2, true);
        let p = free_spot(&g);
        g.pickups.clear(); g.chests.clear();
        g.vehicles = vec![Vehicle::new(1, p, 0.0)];
        g.actors[0].pos = p - Vec3::X * 2.0;
        g.update(1.0 / 60.0, &PlayerInput::default());
        let before = hud::hud_json(&g, &g.camera(1.6), false);
        assert!(before.contains("Drive Island Buggy"));
        g.update(1.0 / 60.0, &PlayerInput { interact: true, ..Default::default() });
        assert_eq!(g.vehicles[0].driver, Some(0), "E enters the nearest vehicle");
        let seated = hud::hud_json(&g, &g.camera(1.6), false);
        assert!(seated.contains("Exit buggy") && seated.contains("\"vehicle\":{\"id\":1"));
        g.update(1.0 / 60.0, &PlayerInput { interact: true, ..Default::default() });
        assert_eq!(g.vehicles[0].driver, None, "second E exits");
        g.actors[0].pos = p - Vec3::X * 2.0;
        assert!(g.enter_vehicle(0, 1));
        g.actors[0].alive = false;
        g.update_empty_vehicles(1.0 / 60.0);
        assert_eq!(g.vehicles[0].driver, None, "an eliminated driver cannot keep the seat occupied");
    }
}
