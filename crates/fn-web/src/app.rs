//! The running game: owns the simulation, the draw-list builder and the renderer, and runs one
//! frame (input -> update -> scene -> render -> audio cues -> HUD snapshot).

use fn_core::input::Input;
use crate::renderer::types::*;
use crate::renderer::Renderer;
use fn_core::camera::Camera;
use fn_core::game::actor::*;
use fn_core::game::audio_map::{AudioCue, LoopLevels, Mixer};
use fn_core::game::events::Event;
use fn_core::game::items::*;
use fn_core::game::scene::{storm_params, Scene};
use fn_core::game::*;
use fn_core::net::client::Client;
use fn_core::net::guest::GuestRoom;
use fn_core::net::host::{Host, Outgoing, Room};
use fn_core::net::proto::MatchSetup;
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

/// Point the human player's aim at `target`. The aim ray starts at the right shoulder (see
/// `Game::aim_ray`), so it is re-solved a few times rather than aiming the body at the target.
fn aim_player_at(g: &mut Game, target: Vec3) {
    let me = g.local;
    for _ in 0..3 {
        let (o, _) = g.aim_ray(me);
        let to = target - o;
        let flat = Vec2::new(to.x, to.z);
        let a = &mut g.actors[me];
        a.yaw = yaw_of(flat);
        a.pitch = to.y.atan2(flat.length());
    }
    let a = &mut g.actors[me];
    a.body_yaw = a.yaw;
}

/// Who runs the match this page shows.
pub enum Mode {
    /// A match against bots on this machine alone.
    Solo(Box<Game>),
    /// A multiplayer match this page hosts: the simulation runs here.
    Host(Box<Host>),
    /// A multiplayer match hosted elsewhere: this page plays its own predicted copy.
    Guest(Box<Client>),
}

impl Mode {
    pub fn game(&self) -> &Game {
        match self {
            Mode::Solo(g) => g,
            Mode::Host(h) => &h.game,
            Mode::Guest(c) => &c.game,
        }
    }
    pub fn game_mut(&mut self) -> &mut Game {
        match self {
            Mode::Solo(g) => g,
            Mode::Host(h) => &mut h.game,
            Mode::Guest(c) => &mut c.game,
        }
    }
}

/// The room before a multiplayer match starts.
pub enum Lobby {
    None,
    Host(Room),
    Guest(GuestRoom),
}

pub struct App {
    pub mode: Mode,
    pub lobby: Lobby,
    /// Messages for the network, waiting for the page to send them.
    pub net_out: Vec<Outgoing>,
    /// Whether the multiplayer match in `mode` is still connected (otherwise it only provides the island behind the menu).
    pub net_live: bool,
    /// The room and the setup of a match the host has announced but whose island is not built yet.
    pub pending_host: Option<(Room, MatchSetup)>,
    pub scene: Scene,
    pub renderer: Renderer,
    pub input: Input,
    pub mixer: Mixer,
    pub cues: Vec<AudioCue>,
    pub loops: LoopLevels,
    pub minimap: Vec<u8>,
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

pub(crate) fn now_ms() -> f64 {
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
            mode: Mode::Solo(Box::new(game)),
            lobby: Lobby::None,
            net_out: vec![],
            net_live: false,
            pending_host: None,
            scene,
            renderer,
            input: Input::new(),
            mixer: Mixer::new(),
            cues: vec![],
            loops: LoopLevels::default(),
            minimap,
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
        let fov = self.mode.game().fov_deg;
        self.mode = Mode::Solo(Box::new(Game::new(world, cfg.clone())));
        self.mode.game_mut().fov_deg = fov;
        self.net_live = false;
        self.cfg = cfg;
        self.reset_for_match();
    }

    /// Fresh input, sound and storm state for a match that is about to begin.
    pub fn reset_for_match(&mut self) {
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
        self.renderer.update_ground_cover(&self.mode.game().world, cam.pos);
        let f = FrameInput {
            time: self.menu_t,
            mode: self.cfg.mode,
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
        let ads_scale = {
            let g = self.mode.game();
            if g.actors[g.local].ads { 0.62 } else { 1.0 }
        };
        let driving = { let g = self.mode.game(); g.vehicle_for_actor(g.local).is_some() };
        let pi = if driving { self.input.take_driving(self.sens * ads_scale, self.invert_y) } else { self.input.take(self.sens * ads_scale, self.invert_y) };
        let t0 = now_ms();
        match &mut self.mode {
            Mode::Solo(g) => {
                if !self.paused {
                    g.update(dt, &pi);
                    g.tick_camera(dt);
                }
            }
            // a multiplayer match goes on whatever this page is doing (the pause menu only hides the screen)
            Mode::Host(h) => {
                if self.net_live {
                    h.update(dt, &pi);
                    self.net_out.extend(h.drain());
                }
                h.game.tick_camera(dt);
            }
            Mode::Guest(c) => {
                if self.net_live {
                    c.update(dt, &pi, now_ms() as u32);
                    self.net_out.extend(c.drain());
                }
                c.game.tick_camera(dt);
            }
        }
        let t1 = now_ms();
        let (w, h) = self.renderer.surface_size();
        let aspect = w as f32 / h.max(1) as f32;
        let mut cam = self.mode.game().camera(aspect);
        if let Some((p, yaw, pitch, fov)) = self.debug_cam {
            cam = Camera::from_yaw_pitch(p, yaw, pitch, fov.to_radians(), aspect, 0.05);
        } else if let Some((yaw, pitch, dist, height)) = self.debug_orbit {
            let g = self.mode.game();
            let t = g.actors[g.camera_actor()].pos + Vec3::Y * height;
            let dir = look_dir(yaw, pitch);
            cam = Camera::look(t - dir * dist, dir, 0.0, 50f32.to_radians(), aspect, 0.05);
        }
        self.last_cam = cam;
        if !self.paused {
            for e in &self.mode.game().events {
                if let Event::TreeFelled { chunk, slot, .. } = e {
                    self.renderer.remove_prop(*chunk, *slot);
                }
            }
            self.mixer_step(&cam);
        }
        let game = self.mode.game();
        self.renderer.update_ground_cover(&game.world, cam.pos);
        self.scene.build(game, &cam);
        let t2 = now_ms();

        let me = &game.actors[game.camera_actor()];
        let outside = game.storm.active && Vec2::new(me.pos.x, me.pos.z).distance(game.storm.center) > game.storm.radius;
        self.in_storm_t = lerp(self.in_storm_t, if outside { 1.0 } else { 0.0 }, damp(2.5, dt_real.min(0.1)));
        let low = (1.0 - (me.hp / 100.0)).clamp(0.0, 1.0);
        let sp = storm_params(game);
        let f = FrameInput {
            time: game.time,
            mode: game.cfg.mode,
            camera: cam,
            sun_dir: Vec3::new(0.55, 0.62, 0.42),
            storm: Vec4::new(sp[0], sp[1], sp[2], sp[3]),
            storm_time: game.time,
            in_storm: self.in_storm_t,
            damage: game.hurt_flash * 0.8 + low * low * 0.25,
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
        self.loops = self.mixer.process(self.mode.game(), cam, &mut self.cues);
    }

    pub fn hud(&self) -> String {
        let mut json = fn_core::game::hud::hud_json(self.mode.game(), &self.last_cam, self.show_tags);
        // a multiplayer match shows who is playing and how good the connection is
        let net = match (&self.mode, self.net_live) {
            (Mode::Host(h), true) => Some(format!("\"role\":\"host\",\"ping\":0,\"players\":{}", 1 + h.peers().iter().filter(|p| p.connected).count())),
            (Mode::Guest(c), true) => Some(format!("\"role\":\"guest\",\"ping\":{:.0},\"players\":{}", c.ping_ms, c.setup.names.len())),
            _ => None,
        };
        if let Some(net) = net {
            json.pop();
            json.push_str(&format!(",\"net\":{{{net}}}}}"));
        }
        json
    }

    // ---- debug / test commands -------------------------------------------------------------------------------

    pub fn debug(&mut self, cmd: &str) -> String {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        let num = |i: usize, d: f32| parts.get(i).and_then(|s| s.parse::<f32>().ok()).unwrap_or(d);
        let g = self.mode.game_mut();
        let me = g.local;
        match parts.first().copied().unwrap_or("") {
            "tp" => {
                let (x, z) = (num(1, 0.0), num(2, 0.0));
                let y = g.world.hm.height_at(x, z);
                let a = &mut g.actors[me];
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
                let a = &mut g.actors[me];
                a.pos = Vec3::new(x, y, z);
                a.mode = MoveMode::Freefall;
                a.vel = Vec3::new(0.0, -20.0, 0.0);
                a.on_ground = false;
                "ok".into()
            }
            "glide" => {
                let (x, y, z) = (num(1, 0.0), num(2, 80.0), num(3, 0.0));
                let a = &mut g.actors[me];
                a.pos = Vec3::new(x, y, z);
                a.mode = MoveMode::Glide;
                a.vel = Vec3::new(0.0, -6.0, -12.0);
                a.on_ground = false;
                a.glide_deployed = true;
                "ok".into()
            }
            "look" => {
                let a = &mut g.actors[me];
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
                let a = &mut g.actors[me];
                a.inv.add_weapon(kind, rarity, kind.def().mag);
                a.inv.ammo[kind.def().ammo.index()] = kind.def().ammo.cap();
                let slot = a.inv.slots.iter().position(|s| matches!(s, Some(Item::Weapon { kind: k, .. }) if *k == kind)).unwrap_or(1);
                g.select_slot(me, slot);
                "ok".into()
            }
            "cons" => {
                // cons <bandage|medkit|shield|bigshield|chug> [count]
                let kind = match parts.get(1).copied().unwrap_or("bandage") {
                    "medkit" => ConsumableKind::MedKit,
                    "shield" => ConsumableKind::ShieldSmall,
                    "bigshield" => ConsumableKind::ShieldBig,
                    "chug" => ConsumableKind::ChugJug,
                    _ => ConsumableKind::Bandage,
                };
                g.actors[me].inv.add_consumable(kind, num(2, 1.0) as u32);
                "ok".into()
            }
            "mats" => {
                let n = num(1, 500.0) as u32;
                g.actors[me].inv.mats = [n, n, n];
                "ok".into()
            }
            "heal" => {
                let a = &mut g.actors[me];
                a.hp = 100.0;
                a.shield = 100.0;
                "ok".into()
            }
            "hurt" => {
                let a = &mut g.actors[me];
                a.hp = num(1, 40.0);
                a.shield = num(2, 0.0);
                "ok".into()
            }
            "god" => {
                g.cfg.god_mode = num(1, 1.0) > 0.5;
                "ok".into()
            }
            "kill_me" => {
                // eliminate the player (killed by bot 1)
                g.eliminate(me, Some(1), "Assault Rifle", false);
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
            "stats" => {
                // set the player's end-of-match numbers (for screenshots): kills, match seconds, damage dealt
                g.actors[me].kills = num(1, 0.0) as u32;
                g.match_time = num(2, 0.0);
                g.actors[me].damage_dealt = num(3, 0.0);
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
                let p = g.actors[me].pos;
                let yaw = g.actors[me].yaw;
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
                let p = g.actors[me].pos;
                let yaw = g.actors[me].yaw;
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
            "cam_bus" => {
                // free camera relative to the bus: distance, yaw offset (deg, 0 = behind), height
                let (dist, yo, hgt) = (num(1, 24.0), num(2, 0.0).to_radians(), num(3, 6.0));
                let bus_yaw = yaw_of(g.bus.dir);
                let ang = bus_yaw + yo;
                let back = yaw_forward(ang) * -dist;
                let pos = g.bus.pos + back + Vec3::Y * hgt;
                let look = (g.bus.pos + Vec3::Y * 3.0 - pos).normalize();
                self.debug_cam = Some((pos, yaw_of(Vec2::new(look.x, look.z)), look.y.asin(), num(4, 55.0)));
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
            "chest" => {
                // stand next to the nth unopened chest, looking at it
                let idx = num(1, 0.0) as usize;
                if let Some(c) = g.chests.get(idx % g.chests.len().max(1)).cloned() {
                    let fwd = yaw_forward(c.yaw);
                    let p = c.pos + fwd * 2.4;
                    let a = &mut g.actors[me];
                    a.pos = Vec3::new(p.x, c.pos.y, p.z);
                    a.mode = MoveMode::Ground;
                    a.on_ground = true;
                    a.yaw = c.yaw + std::f32::consts::PI;
                    a.pitch = -0.1;
                    a.eye_smooth = c.pos.y;
                    if g.phase == Phase::Bus {
                        g.phase = Phase::Playing;
                        g.bus.active = false;
                    }
                }
                "ok".into()
            }
            "tree" => {
                // stand 2.4 m from the nth nearest living tree (default: the nearest), facing it, pickaxe in hand
                let from = g.actors[me].pos;
                let mut trees: Vec<(f32, usize)> = g
                    .world
                    .harvest
                    .iter()
                    .enumerate()
                    .filter(|(_, h)| h.alive && h.kind == fn_core::world::props::HarvestKind::Tree)
                    .map(|(i, h)| ((h.pos - from).length_squared(), i))
                    .collect();
                trees.sort_by(|a, b| a.0.total_cmp(&b.0));
                if let Some(&(_, i)) = trees.get(num(1, 0.0) as usize) {
                    let t = g.world.harvest[i].pos;
                    let away = Vec2::new(from.x - t.x, from.z - t.z).try_normalize().unwrap_or(Vec2::X);
                    let p = Vec2::new(t.x, t.z) + away * 2.4;
                    let y = g.world.hm.height_at(p.x, p.y);
                    let a = &mut g.actors[me];
                    a.pos = Vec3::new(p.x, y, p.y);
                    a.vel = Vec3::ZERO;
                    a.mode = MoveMode::Ground;
                    a.on_ground = true;
                    a.eye_smooth = y;
                    a.yaw = yaw_of(Vec2::new(t.x - p.x, t.z - p.y));
                    a.body_yaw = a.yaw;
                    g.select_slot(me, 0);
                    aim_player_at(g, Vec3::new(t.x, t.y + 1.2, t.z));
                }
                "ok".into()
            }
            "aim" => {
                // aim the player's shoulder ray at the chest of actor N (default 1)
                let n = (num(1, 1.0) as usize).min(g.actors.len() - 1);
                let target = g.actors[n].chest();
                aim_player_at(g, target);
                "ok".into()
            }
            "building" => {
                // stand just inside the nth building's door, looking into the room
                let n = num(1, 0.0) as usize;
                if let Some(b) = g.world.buildings.get(n % g.world.buildings.len().max(1)).cloned() {
                    let inward = Vec2::new(b.center.x - b.door_in.x, b.center.z - b.door_in.z).try_normalize().unwrap_or(Vec2::Y);
                    let p = Vec3::new(b.door_in.x + inward.x * 0.8, b.door_in.y, b.door_in.z + inward.y * 0.8);
                    let a = &mut g.actors[me];
                    a.pos = p;
                    a.vel = Vec3::ZERO;
                    a.mode = MoveMode::Ground;
                    a.on_ground = true;
                    a.eye_smooth = p.y;
                    a.yaw = yaw_of(inward);
                    a.pitch = -0.05;
                    a.body_yaw = a.yaw;
                    if g.phase == Phase::Bus {
                        g.phase = Phase::Playing;
                        g.bus.active = false;
                    }
                }
                "ok".into()
            }
            "chest_here" => {
                // drop an unopened chest (and some loot) in front of the player
                let p = g.actors[me].pos;
                let yaw = g.actors[me].yaw;
                let f = yaw_forward(yaw);
                let pos = p + f * num(1, 5.0);
                let pos = Vec3::new(pos.x, g.world.hm.height_at(pos.x, pos.z), pos.z);
                let id = g.new_id();
                g.chests.push(Chest { id, pos, yaw: yaw + std::f32::consts::PI, open_t: 0.0, opened: false });
                for (k, kind) in [WeaponKind::Pistol, WeaponKind::Smg, WeaponKind::AssaultRifle, WeaponKind::Shotgun, WeaponKind::Sniper, WeaponKind::RocketLauncher].into_iter().enumerate() {
                    let q = pos + yaw_right(yaw) * ((k as f32 - 2.5) * 1.3) + f * 3.5;
                    let q = Vec3::new(q.x, g.world.hm.height_at(q.x, q.z), q.z);
                    g.spawn_pickup(q, PickupKind::Weapon { kind, rarity: Rarity::from_index(k % 5), ammo: kind.def().mag }, false);
                }
                for (k, kind) in AmmoKind::ALL.into_iter().enumerate() {
                    let q = pos + yaw_right(yaw) * ((k as f32 - 2.0) * 1.1) + f * 6.5;
                    let q = Vec3::new(q.x, g.world.hm.height_at(q.x, q.z), q.z);
                    g.spawn_pickup(q, PickupKind::Ammo { kind, amount: 30 }, false);
                }
                for (k, kind) in ConsumableKind::ALL.into_iter().enumerate() {
                    let q = pos + yaw_right(yaw) * ((k as f32 - 2.0) * 1.2) + f * 9.5;
                    let q = Vec3::new(q.x, g.world.hm.height_at(q.x, q.z), q.z);
                    g.spawn_pickup(q, PickupKind::Consumable { kind, count: 1 }, false);
                }
                "ok".into()
            }
            "storm_set" => {
                // centre x z, radius: makes the wall visible near the player
                g.storm.active = true;
                g.storm.center = Vec2::new(num(1, 0.0), num(2, 0.0));
                g.storm.radius = num(3, 60.0);
                g.storm.from_radius = g.storm.radius;
                g.storm.to_radius = g.storm.radius * 0.5;
                g.storm.to_center = g.storm.center;
                "ok".into()
            }
            "build_demo" => {
                use fn_core::game::pieces::*;
                let p = g.actors[me].pos;
                let yaw = g.actors[me].yaw;
                let f = yaw_forward(yaw);
                let base = p + f * 9.0;
                let (cx, cz) = cell_of(base);
                // the same rule as placing the first piece by hand: sit on the highest ground under the footprint
                let y0 = structure_base(&g.pieces, &g.env(), cx, cz);
                let mats = [Mat::Wood, Mat::Stone, Mat::Metal];
                // a little fort: floor, 3 walls, ramp up the side, a roof
                let mut n = 0;
                for (i, m) in mats.iter().enumerate() {
                    let x = cx + i as i32 * 2;
                    for (kind, dx, dz, dir, level) in [(PieceKind::Floor, 0, 0, 0u8, 0i32), (PieceKind::Wall, 0, 0, 0, 0), (PieceKind::Wall, 0, 0, 1, 0), (PieceKind::Wall, 1, 0, 1, 0), (PieceKind::Roof, 0, 0, 0, 0)] {
                        let key = PieceKey { kind, x: x + dx, z: cz + dz, level, dir };
                        if g.pieces.at(&key).is_none() {
                            let foot = footing_for(&key, y0, &g.env());
                            g.pieces.insert_footed(key, *m, y0, me, foot);
                            n += 1;
                        }
                    }
                    let key = PieceKey { kind: PieceKind::Ramp, x, z: cz + 1, level: 0, dir: 3 };
                    g.pieces.insert(key, *m, y0, me);
                }
                format!("placed {n}")
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
            "gallery" => {
                // bots frozen in different activities, for animation review (use `speed 0.02` to hold the poses)
                let p = g.actors[me].pos;
                let yaw = g.actors[me].yaw;
                let (dist, spacing, turn) = (num(1, 6.0), num(2, 1.9), num(3, 90.0).to_radians());
                let n_actors = g.actors.len();
                let side = yaw_right(yaw);
                let fwd = yaw_forward(yaw);
                for k in 0..8.min(n_actors - 1) {
                    let pos = p + fwd * dist + side * ((k as f32 - 3.5) * spacing);
                    let y = g.world.hm.height_at(pos.x, pos.z);
                    let a = &mut g.actors[k + 1];
                    a.pos = Vec3::new(pos.x, y, pos.z);
                    a.vel = Vec3::ZERO;
                    a.mode = MoveMode::Ground;
                    a.on_ground = true;
                    a.alive = true;
                    a.hp = 100.0;
                    a.brain = None;
                    a.yaw = yaw + turn;
                    a.body_yaw = a.yaw;
                    a.pitch = 0.0;
                    a.eye_smooth = y;
                    a.inv = Inventory::new();
                    a.build_mode = false;
                    a.crouching = false;
                    a.ads = false;
                    a.action = Action::None;
                    match k {
                        0 => {
                            a.inv.add_weapon(WeaponKind::AssaultRifle, Rarity::Epic, 30);
                            a.inv.selected = 1;
                            a.ads = true;
                            a.anim.aim = 1.0;
                        }
                        1 => {
                            a.inv.add_weapon(WeaponKind::Shotgun, Rarity::Legendary, 5);
                            a.inv.selected = 1;
                            a.crouching = true;
                            a.anim.crouch = 1.0;
                            a.anim.aim = 1.0;
                        }
                        2 => {
                            a.inv.add_weapon(WeaponKind::Smg, Rarity::Rare, 4);
                            a.inv.ammo[0] = 100;
                            a.inv.selected = 1;
                            a.action = Action::Reload { t: 1.1, dur: 2.1 };
                        }
                        3 => {
                            a.inv.selected = 0;
                            a.anim.swing = 0.6;
                        }
                        4 => {
                            a.inv.add_consumable(ConsumableKind::ShieldBig, 2);
                            a.inv.selected = 1;
                            a.shield = 20.0;
                            a.action = Action::Heal { slot: 1, t: 2.4, dur: 4.8 };
                        }
                        5 => {
                            a.build_mode = true;
                            a.anim.build = 1.0;
                        }
                        6 => {
                            a.inv.add_weapon(WeaponKind::Sniper, Rarity::Epic, 1);
                            a.inv.selected = 1;
                            a.ads = true;
                            a.anim.aim = 1.0;
                            a.pitch = 0.1;
                        }
                        _ => {
                            a.inv.add_weapon(WeaponKind::RocketLauncher, Rarity::Rare, 1);
                            a.inv.selected = 1;
                            a.anim.aim = 1.0;
                        }
                    }
                }
                "ok".into()
            }
            "state" => {
                let a = &g.actors[me];
                format!("{{\"pos\":[{:.1},{:.1},{:.1}],\"yaw\":{:.2},\"hp\":{:.0},\"mode\":\"{:?}\",\"alive\":{},\"phase\":\"{:?}\",\"pieces\":{},\"pickups\":{},\"t\":{:.1},\"emoting\":{},\"outfit\":{},\"gameMode\":\"{:?}\",\"worldSize\":{},\"vehicles\":{}}}", a.pos.x, a.pos.y, a.pos.z, a.yaw, a.hp, a.mode, g.alive_count(), g.phase, g.pieces.count(), g.pickups.len(), g.time, a.emoting, g.cfg.player_outfit, g.cfg.mode, fn_core::world::WORLD_SIZE, g.vehicles.len())
            }
            "vehicles" => {
                let list: Vec<String> = g.vehicles.iter().map(|v| format!("{{\"id\":{},\"pos\":[{:.2},{:.2},{:.2}],\"yaw\":{:.3},\"speed\":{:.2},\"driver\":{}}}", v.id, v.pos.x, v.pos.y, v.pos.z, v.yaw, v.speed, v.driver.map_or("null".into(), |i| i.to_string()))).collect();
                format!("[{}]", list.join(","))
            }
            "vehicle" => {
                // Stand beside a parked car, optionally positioning a remote
                // actor on the host for transport integration tests.
                let index = num(1, 0.0) as usize;
                let actor = (num(2, me as f32) as usize).min(g.actors.len() - 1);
                if let Some(v) = g.vehicles.get(index).cloned() {
                    g.exit_vehicle(actor);
                    let p = v.pos + yaw_right(v.yaw) * 2.6;
                    let h = g.world.hm.height_at(p.x, p.z);
                    let a = &mut g.actors[actor];
                    a.pos = Vec3::new(p.x, h, p.z);
                    a.vel = Vec3::ZERO;
                    a.mode = MoveMode::Ground;
                    a.on_ground = true;
                    a.eye_smooth = h;
                    a.peak_y = h;
                    a.yaw = v.yaw;
                    a.body_yaw = v.yaw;
                    a.pitch = -0.15;
                }
                "ok".into()
            }
            "actor" => {
                // actor <index>: where somebody is, whoever they are (for multiplayer tests)
                let i = (num(1, 0.0) as usize).min(g.actors.len() - 1);
                let a = &g.actors[i];
                format!("{{\"id\":{},\"name\":\"{}\",\"human\":{},\"alive\":{},\"mode\":\"{:?}\",\"pos\":[{:.2},{:.2},{:.2}],\"hp\":{:.0},\"yaw\":{:.2}}}", a.id, a.name, a.human, a.alive, a.mode, a.pos.x, a.pos.y, a.pos.z, a.hp, a.yaw)
            }
            "timings" => {
                let t = self.timings;
                format!("{{\"update\":{:.2},\"scene\":{:.2},\"render\":{:.2},\"frame\":{:.2},\"instances\":{},\"particles\":{},\"draws\":{},\"tris\":{}}}", t.update_ms, t.scene_ms, t.render_ms, t.frame_ms, self.scene.stats.instances, self.scene.stats.particles, self.renderer.stats.draw_calls, self.renderer.stats.triangles)
            }
            _ => format!("unknown command: {cmd}"),
        }
    }
}
