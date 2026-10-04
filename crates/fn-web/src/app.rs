//! The running game: owns the simulation, the draw-list builder and the renderer, and runs one
//! frame (input -> update -> scene -> render -> audio cues -> HUD snapshot).

use crate::input::Input;
use crate::renderer::types::*;
use crate::renderer::Renderer;
use fn_core::camera::Camera;
use fn_core::game::actor::*;
use fn_core::game::audio_map::{AudioCue, LoopLevels, Mixer};
use fn_core::game::events::Event;
use fn_core::game::items::*;
use fn_core::game::scene::{storm_params, Scene};
use fn_core::game::*;
use fn_core::math::*;
use fn_core::mesh::MeshData;
use fn_core::world::World;
use glam::{Vec3, Vec4};

#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    pub update_ms: f32,
    pub scene_ms: f32,
    pub render_ms: f32,
    pub frame_ms: f32,
}

pub struct App {
    pub game: Game,
    pub scene: Scene,
    pub renderer: Renderer,
    pub input: Input,
    pub mixer: Mixer,
    pub cues: Vec<AudioCue>,
    pub loops: LoopLevels,
    pub minimap: Vec<u8>,
    pub meshes: Vec<MeshData>,
    pub paused: bool,
    pub sens: f32,
    pub invert_y: bool,
    pub show_tags: bool,
    pub cfg: GameConfig,
    pub timings: Timings,
    pub last_cam: Camera,
    pub time_scale: f32,
    /// Debug camera override: position, yaw, pitch (radians), vertical fov (degrees).
    pub debug_cam: Option<(Vec3, f32, f32, f32)>,
    /// Debug orbit around the player: yaw, pitch (radians), distance, height offset.
    pub debug_orbit: Option<(f32, f32, f32, f32)>,
    /// Menu mode: no simulation, a slow flyover of the island behind the UI.
    pub menu: bool,
    menu_t: f32,
    in_storm_t: f32,
    pub frame_no: u64,
}

fn now_ms() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now()).unwrap_or(0.0)
}

impl App {
    pub fn new(renderer: Renderer, meshes: Vec<MeshData>, cfg: GameConfig) -> App {
        let world = World::generate(cfg.seed as u32);
        let mut renderer = renderer;
        renderer.set_world(&world);
        let minimap = fn_core::world::minimap::render(&world, 1024);
        let game = Game::new(world, cfg.clone());
        let scene = Scene::new(&meshes);
        let (w, h) = renderer.surface_size();
        let last_cam = game.camera(w as f32 / h.max(1) as f32);
        App {
            game,
            scene,
            renderer,
            input: Input::new(),
            mixer: Mixer::new(),
            cues: vec![],
            loops: LoopLevels::default(),
            minimap,
            meshes,
            paused: false,
            sens: 0.0022,
            invert_y: false,
            show_tags: false,
            cfg,
            timings: Timings::default(),
            last_cam,
            time_scale: 1.0,
            debug_cam: None,
            debug_orbit: None,
            menu: false,
            menu_t: 0.0,
            in_storm_t: 0.0,
            frame_no: 0,
        }
    }

    /// Start a fresh match (regenerates the island so felled trees come back).
    pub fn restart(&mut self, cfg: GameConfig) {
        let world = World::generate(cfg.seed as u32);
        self.renderer.set_world(&world);
        if cfg.seed != self.cfg.seed || self.minimap.is_empty() {
            self.minimap = fn_core::world::minimap::render(&world, 1024);
        }
        let fov = self.game.fov_deg;
        self.game = Game::new(world, cfg.clone());
        self.game.fov_deg = fov;
        self.cfg = cfg;
        self.input.release_all();
        self.mixer = Mixer::new();
        self.cues.clear();
        self.in_storm_t = 0.0;
        self.paused = false;
    }

    fn menu_frame(&mut self, dt_real: f32) {
        self.menu_t += dt_real.min(0.1);
        let (w, h) = self.renderer.surface_size();
        let aspect = w as f32 / h.max(1) as f32;
        // slow orbit around the island, high enough to see the towns
        let t = self.menu_t * 0.045 + 0.6;
        let r = 330.0;
        let ctr = Vec3::new(0.0, 30.0, 0.0);
        let eye = Vec3::new(ctr.x + t.cos() * r, 120.0 + (self.menu_t * 0.11).sin() * 22.0, ctr.z + t.sin() * r);
        let cam = Camera::look(eye, (ctr - eye).normalize(), 0.0, 58f32.to_radians(), aspect, 0.2);
        self.last_cam = cam;
        self.renderer.update_ground_cover(&self.game.world, cam.pos);
        let f = FrameInput {
            time: self.menu_t,
            camera: cam,
            sun_dir: Vec3::new(0.55, 0.62, 0.42),
            storm: Vec4::ZERO,
            storm_time: self.menu_t,
            in_storm: 0.0,
            damage: 0.0,
            vignette: 0.32,
            wind: 1.0,
            batches: &[],
            instances: &[],
            ghost_batches: &[],
            ghost_instances: &[],
            particles: &[],
            particles_add: &[],
        };
        self.renderer.render(&f);
    }

    pub fn frame(&mut self, dt_real: f32) {
        if self.menu {
            self.menu_frame(dt_real);
            return;
        }
        let t_frame = now_ms();
        self.frame_no += 1;
        let dt = (dt_real * self.time_scale).clamp(0.0, 0.1);
        let ads_scale = if self.game.actors[PLAYER].ads { 0.62 } else { 1.0 };
        let pi = self.input.take(self.sens * ads_scale, self.invert_y);
        let t0 = now_ms();
        if !self.paused {
            self.game.update(dt, &pi);
            self.game.tick_camera(dt);
        }
        let t1 = now_ms();
        let (w, h) = self.renderer.surface_size();
        let aspect = w as f32 / h.max(1) as f32;
        let mut cam = self.game.camera(aspect);
        if let Some((p, yaw, pitch, fov)) = self.debug_cam {
            cam = Camera::from_yaw_pitch(p, yaw, pitch, fov.to_radians(), aspect, 0.05);
        } else if let Some((yaw, pitch, dist, height)) = self.debug_orbit {
            let t = self.game.actors[self.game.camera_actor()].pos + Vec3::Y * height;
            let dir = look_dir(yaw, pitch);
            cam = Camera::look(t - dir * dist, dir, 0.0, 50f32.to_radians(), aspect, 0.05);
        }
        self.last_cam = cam;
        if !self.paused {
            for e in &self.game.events {
                if let Event::TreeFelled { chunk, slot, .. } = e {
                    self.renderer.remove_prop(*chunk, *slot);
                }
            }
            self.mixer_step(&cam);
        }
        self.renderer.update_ground_cover(&self.game.world, cam.pos);
        self.scene.build(&self.game, &cam);
        let t2 = now_ms();

        let me = &self.game.actors[self.game.camera_actor()];
        let outside = self.game.storm.active && Vec2::new(me.pos.x, me.pos.z).distance(self.game.storm.center) > self.game.storm.radius;
        self.in_storm_t = lerp(self.in_storm_t, if outside { 1.0 } else { 0.0 }, damp(2.5, dt_real.min(0.1)));
        let low = (1.0 - (me.hp / 100.0)).clamp(0.0, 1.0);
        let sp = storm_params(&self.game);
        let f = FrameInput {
            time: self.game.time,
            camera: cam,
            sun_dir: Vec3::new(0.55, 0.62, 0.42),
            storm: Vec4::new(sp[0], sp[1], sp[2], sp[3]),
            storm_time: self.game.time,
            in_storm: self.in_storm_t,
            damage: self.game.hurt_flash * 0.8 + low * low * 0.25,
            vignette: 0.27 + low * 0.2,
            wind: 1.0,
            batches: &self.scene.batches,
            instances: &self.scene.instances,
            ghost_batches: &self.scene.ghost_batches,
            ghost_instances: &self.scene.ghost_instances,
            particles: &self.scene.particles,
            particles_add: &self.scene.particles_add,
        };
        self.renderer.render(&f);
        let t3 = now_ms();
        self.timings = Timings { update_ms: (t1 - t0) as f32, scene_ms: (t2 - t1) as f32, render_ms: (t3 - t2) as f32, frame_ms: (t3 - t_frame) as f32 };
    }

    fn mixer_step(&mut self, cam: &Camera) {
        self.cues.clear();
        self.loops = self.mixer.process(&self.game, cam, &mut self.cues);
    }

    pub fn hud(&self) -> String {
        fn_core::game::hud::hud_json(&self.game, &self.last_cam, self.show_tags)
    }

    // ---- debug / test commands -------------------------------------------------------------------------------

    pub fn debug(&mut self, cmd: &str) -> String {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let num = |i: usize, d: f32| parts.get(i).and_then(|s| s.parse::<f32>().ok()).unwrap_or(d);
        let g = &mut self.game;
        match parts.first().copied().unwrap_or("") {
            "tp" => {
                let (x, z) = (num(1, 0.0), num(2, 0.0));
                let y = g.world.hm.height_at(x, z);
                let a = &mut g.actors[PLAYER];
                a.pos = Vec3::new(x, y, z);
                a.vel = Vec3::ZERO;
                a.mode = MoveMode::Ground;
                a.on_ground = true;
                a.peak_y = y;
                a.eye_smooth = y;
                if g.phase == Phase::Bus {
                    g.phase = Phase::Playing;
                    g.bus.active = false;
                    g.storm.active = true;
                    fn_core::game::matchflow::begin_storm_wait(g);
                }
                "ok".into()
            }
            "sky" => {
                let (x, y, z) = (num(1, 0.0), num(2, 200.0), num(3, 0.0));
                let a = &mut g.actors[PLAYER];
                a.pos = Vec3::new(x, y, z);
                a.mode = MoveMode::Freefall;
                a.vel = Vec3::new(0.0, -20.0, 0.0);
                a.on_ground = false;
                "ok".into()
            }
            "look" => {
                let a = &mut g.actors[PLAYER];
                a.yaw = num(1, 0.0).to_radians();
                a.pitch = num(2, 0.0).to_radians();
                a.body_yaw = a.yaw;
                "ok".into()
            }
            "give" => {
                let kind = match parts.get(1).copied().unwrap_or("ar") {
                    "pistol" => WeaponKind::Pistol,
                    "smg" => WeaponKind::Smg,
                    "shotgun" => WeaponKind::Shotgun,
                    "sniper" => WeaponKind::Sniper,
                    "rocket" => WeaponKind::RocketLauncher,
                    _ => WeaponKind::AssaultRifle,
                };
                let rarity = match parts.get(2).copied().unwrap_or("rare") {
                    "common" => Rarity::Common,
                    "uncommon" => Rarity::Uncommon,
                    "epic" => Rarity::Epic,
                    "legendary" => Rarity::Legendary,
                    _ => Rarity::Rare,
                };
                let a = &mut g.actors[PLAYER];
                a.inv.add_weapon(kind, rarity, kind.def().mag);
                a.inv.ammo[kind.def().ammo.index()] = kind.def().ammo.cap();
                let slot = a.inv.slots.iter().position(|s| matches!(s, Some(Item::Weapon { kind: k, .. }) if *k == kind)).unwrap_or(1);
                g.select_slot(PLAYER, slot);
                "ok".into()
            }
            "mats" => {
                let n = num(1, 500.0) as u32;
                g.actors[PLAYER].inv.mats = [n, n, n];
                "ok".into()
            }
            "heal" => {
                let a = &mut g.actors[PLAYER];
                a.hp = 100.0;
                a.shield = 100.0;
                "ok".into()
            }
            "hurt" => {
                let a = &mut g.actors[PLAYER];
                a.hp = num(1, 40.0);
                a.shield = num(2, 0.0);
                "ok".into()
            }
            "god" => {
                g.cfg.god_mode = num(1, 1.0) > 0.5;
                "ok".into()
            }
            "kill_bots" => {
                for i in 1..g.actors.len() {
                    g.actors[i].alive = false;
                    g.actors[i].mode = MoveMode::Dead;
                    g.actors[i].dead_time = 5.0;
                }
                "ok".into()
            }
            "freeze_bots" => {
                for i in 1..g.actors.len() {
                    g.actors[i].brain = None;
                }
                "ok".into()
            }
            "storm" => {
                // jump ahead in the storm schedule
                for _ in 0..(num(1, 1.0) as usize) {
                    g.storm.timer = 0.0;
                }
                "ok".into()
            }
            "speed" => {
                self.time_scale = num(1, 1.0);
                "ok".into()
            }
            "bots_near" => {
                // place bots in a ring around the player, standing still
                let n = num(1, 6.0) as usize;
                let dist = num(2, 14.0);
                let p = g.actors[PLAYER].pos;
                let yaw = g.actors[PLAYER].yaw;
                let n_actors = g.actors.len();
                for k in 0..n.min(n_actors - 1) {
                    let ang = yaw + (k as f32 - (n as f32 - 1.0) / 2.0) * 0.55;
                    let d = yaw_forward(ang) * dist;
                    let x = p.x + d.x;
                    let z = p.z + d.z;
                    let y = g.world.hm.height_at(x, z);
                    let a = &mut g.actors[k + 1];
                    a.pos = Vec3::new(x, y, z);
                    a.vel = Vec3::ZERO;
                    a.mode = MoveMode::Ground;
                    a.on_ground = true;
                    a.alive = true;
                    a.hp = 100.0;
                    a.yaw = ang + std::f32::consts::PI;
                    a.body_yaw = a.yaw;
                    a.brain = None;
                    a.eye_smooth = y;
                }
                "ok".into()
            }
            "showcase" => {
                // a row of bots, each holding something different, for visual checks
                let p = g.actors[PLAYER].pos;
                let yaw = g.actors[PLAYER].yaw;
                let (dist, spacing, first, turn) = (num(1, 7.0), num(2, 1.7), num(3, 0.0) as usize, num(4, 180.0).to_radians());
                let kinds = WeaponKind::ALL;
                let n_actors = g.actors.len();
                for k in first..(kinds.len() + 3).min(n_actors - 1) {
                    let side = yaw_right(yaw);
                    let fwd = yaw_forward(yaw);
                    let pos = p + fwd * dist + side * (((k - first) as f32 - 2.0) * spacing);
                    let y = g.world.hm.height_at(pos.x, pos.z);
                    let a = &mut g.actors[k + 1];
                    a.pos = Vec3::new(pos.x, y, pos.z);
                    a.mode = MoveMode::Ground;
                    a.on_ground = true;
                    a.alive = true;
                    a.brain = None;
                    a.yaw = yaw + turn;
                    a.body_yaw = a.yaw;
                    a.eye_smooth = y;
                    a.inv = Inventory::new();
                    if k < kinds.len() {
                        let kind = kinds[k];
                        a.inv.add_weapon(kind, Rarity::from_index(k % 5), kind.def().mag);
                        a.inv.ammo[kind.def().ammo.index()] = 100;
                        a.inv.selected = 1;
                    } else if k == kinds.len() {
                        a.inv.selected = 0; // pickaxe
                    } else if k == kinds.len() + 1 {
                        a.inv.add_consumable(ConsumableKind::ShieldBig, 1);
                        a.inv.selected = 1;
                    } else {
                        a.inv.add_consumable(ConsumableKind::ChugJug, 1);
                        a.inv.selected = 1;
                    }
                    a.anim.aim = if k % 2 == 0 { 1.0 } else { 0.0 };
                }
                "ok".into()
            }
            "cam" => {
                if parts.get(1) == Some(&"off") {
                    self.debug_cam = None;
                } else {
                    self.debug_cam = Some((Vec3::new(num(1, 0.0), num(2, 5.0), num(3, 0.0)), num(4, 0.0).to_radians(), num(5, 0.0).to_radians(), num(6, 60.0)));
                }
                "ok".into()
            }
            "orbit" => {
                if parts.get(1) == Some(&"off") {
                    self.debug_orbit = None;
                } else {
                    // yaw pitch (degrees; the camera looks along this direction) distance height
                    self.debug_orbit = Some((num(1, 0.0).to_radians(), num(2, -5.0).to_radians(), num(3, 3.0), num(4, 1.0)));
                }
                "ok".into()
            }
            "ff" => {
                // fast-forward the simulation (idle human) without rendering: for scripted tests
                let secs = num(1, 10.0);
                let steps = (secs * 30.0) as usize;
                let idle = PlayerInput::default();
                for _ in 0..steps {
                    g.update(1.0 / 30.0, &idle);
                }
                format!("{{\"alive\":{},\"phase\":\"{:?}\"}}", g.alive_count(), g.phase)
            }
            "state" => {
                let a = &g.actors[PLAYER];
                format!("{{\"pos\":[{:.1},{:.1},{:.1}],\"yaw\":{:.2},\"hp\":{:.0},\"mode\":\"{:?}\",\"alive\":{},\"phase\":\"{:?}\",\"pieces\":{},\"pickups\":{},\"t\":{:.1}}}", a.pos.x, a.pos.y, a.pos.z, a.yaw, a.hp, a.mode, g.alive_count(), g.phase, g.pieces.count(), g.pickups.len(), g.time)
            }
            "timings" => {
                let t = self.timings;
                format!("{{\"update\":{:.2},\"scene\":{:.2},\"render\":{:.2},\"frame\":{:.2},\"instances\":{},\"particles\":{},\"draws\":{},\"tris\":{}}}", t.update_ms, t.scene_ms, t.render_ms, t.frame_ms, self.scene.stats.instances, self.scene.stats.particles, self.renderer.stats.draw_calls, self.renderer.stats.triangles)
            }
            _ => format!("unknown command: {cmd}"),
        }
    }
}
