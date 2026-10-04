//! Game simulation: one `Game` owns the world, every actor and all dynamic state.
//! The platform layer feeds it `PlayerInput`, calls `update`, then drains events.

pub mod actor;
pub mod ai;
pub mod audio_map;
pub mod building;
pub mod combat;
pub mod env;
pub mod events;
pub mod fx;
pub mod hud;
pub mod intent;
pub mod items;
pub mod loot;
pub mod matchflow;
pub mod movement;
pub mod pieces;
pub mod rig;
pub mod scene;

use crate::camera::Camera;
use crate::math::*;
use crate::rng::Rng;
use crate::world::{World, WORLD_HALF};
use actor::*;
use env::Env;
use events::*;
use intent::Intent;
use items::*;
use pieces::*;

pub const PLAYER: usize = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}

#[derive(Clone, Debug)]
pub struct GameConfig {
    pub seed: u64,
    pub bots: usize,
    pub difficulty: Difficulty,
    pub player_name: String,
    pub start_mats: u32,
    /// Skip the bus and start everyone on the ground (used by tests).
    pub skip_bus: bool,
    /// Make every phase of the storm much shorter (tests / demos).
    pub storm_speed: f32,
    /// The human takes no damage (tests).
    pub god_mode: bool,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self { seed: 1, bots: 39, difficulty: Difficulty::Normal, player_name: "You".into(), start_mats: 100, skip_bus: false, storm_speed: 1.0, god_mode: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Bus,
    Playing,
    Over,
}

/// Raw player input for one frame. `*_pressed` fields are edges (true for one frame).
#[derive(Clone, Debug, Default)]
pub struct PlayerInput {
    /// x: strafe right (+), y: forward (+)
    pub move_axis: Vec2,
    /// Look deltas in radians: x turns right (+), y looks up (+).
    pub look: Vec2,
    pub jump: bool,
    pub sprint: bool,
    pub crouch: bool,
    pub fire: bool,
    pub fire_pressed: bool,
    pub ads: bool,
    pub reload: bool,
    pub interact: bool,
    pub select: Option<usize>,
    pub cycle: i32,
    pub drop_selected: bool,
    pub toggle_build: bool,
    pub piece: Option<PieceKind>,
    pub place: bool,
    pub cycle_mat: bool,
    pub exit_bus: bool,
    pub deploy: bool,
    /// Dance emote (edge).
    pub emote: bool,
    /// Cycle spectate target (+1 / -1) while dead.
    pub spectate: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PickupKind {
    Weapon { kind: WeaponKind, rarity: Rarity, ammo: u32 },
    Ammo { kind: AmmoKind, amount: u32 },
    Consumable { kind: ConsumableKind, count: u32 },
}

impl PickupKind {
    pub fn rarity(&self) -> Rarity {
        match self {
            PickupKind::Weapon { rarity, .. } => *rarity,
            PickupKind::Ammo { .. } => Rarity::Common,
            PickupKind::Consumable { kind, .. } => kind.rarity(),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            PickupKind::Weapon { kind, .. } => kind.name(),
            PickupKind::Ammo { kind, .. } => kind.name(),
            PickupKind::Consumable { kind, .. } => kind.name(),
        }
    }
    pub fn count(&self) -> u32 {
        match self {
            PickupKind::Weapon { .. } => 1,
            PickupKind::Ammo { amount, .. } => *amount,
            PickupKind::Consumable { count, .. } => *count,
        }
    }
    pub fn from_drop(d: Drop) -> PickupKind {
        match d {
            Drop::Weapon { kind, rarity } => PickupKind::Weapon { kind, rarity, ammo: kind.def().mag },
            Drop::Ammo { kind, amount } => PickupKind::Ammo { kind, amount },
            Drop::Consumable { kind, count } => PickupKind::Consumable { kind, count },
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pickup {
    pub id: u32,
    pub pos: Vec3,
    pub vel: Vec3,
    pub kind: PickupKind,
    pub age: f32,
    pub grounded: bool,
    pub spin: f32,
}

#[derive(Clone, Debug)]
pub struct Chest {
    pub id: u32,
    pub pos: Vec3,
    pub yaw: f32,
    /// 0 closed .. 1 fully open (animated).
    pub open_t: f32,
    pub opened: bool,
}

#[derive(Clone, Debug)]
pub struct Projectile {
    pub pos: Vec3,
    pub vel: Vec3,
    pub owner: usize,
    pub kind: WeaponKind,
    pub rarity: Rarity,
    pub life: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StormState {
    Waiting,
    Shrinking,
    Done,
}

#[derive(Clone, Debug)]
pub struct Storm {
    pub active: bool,
    pub phase: usize,
    pub state: StormState,
    pub timer: f32,
    pub duration: f32,
    pub center: Vec2,
    pub radius: f32,
    pub from_center: Vec2,
    pub from_radius: f32,
    pub to_center: Vec2,
    pub to_radius: f32,
    pub dmg: f32,
    pub tick: f32,
}

#[derive(Clone, Debug)]
pub struct Bus {
    pub active: bool,
    pub pos: Vec3,
    pub dir: Vec2,
    pub speed: f32,
    pub start: Vec2,
    pub end: Vec2,
    pub t: f32,
    pub total: f32,
}

#[derive(Clone, Debug)]
pub struct Felled {
    pub pos: Vec3,
    pub kind: crate::world::props::PropKind,
    pub scale: f32,
    pub yaw: f32,
    pub tint: [f32; 3],
    pub t: f32,
    pub dir: Vec2,
}

#[derive(Clone, Debug)]
pub struct FeedEntry {
    pub killer: Option<String>,
    pub victim: String,
    pub weapon: String,
    pub by_player: bool,
    pub victim_is_player: bool,
    pub storm: bool,
    pub age: f32,
}

#[derive(Clone, Debug, Default)]
pub struct MatchStats {
    pub kills: u32,
    pub damage_dealt: f32,
    pub placement: u32,
    pub survived: f32,
}

pub struct Game {
    pub world: World,
    pub cfg: GameConfig,
    pub rng: Rng,
    pub time: f32,
    pub phase: Phase,
    pub actors: Vec<Actor>,
    pub pickups: Vec<Pickup>,
    pub chests: Vec<Chest>,
    pub pieces: Pieces,
    pub projectiles: Vec<Projectile>,
    pub storm: Storm,
    pub bus: Bus,
    pub events: Vec<Event>,
    pub fx: fx::Fx,
    pub feed: Vec<FeedEntry>,
    pub winner: Option<usize>,
    pub spectating: Option<usize>,
    pub cam_dist: f32,
    /// Vertical field of view in degrees (player setting).
    pub fov_deg: f32,
    pub cam_shake: f32,
    pub fov_t: f32,
    pub felled: Vec<Felled>,
    pub next_id: u32,
    pub match_time: f32,
    pub player_dead_time: f32,
    pub last_hurt_dir: Option<Vec3>,
    /// Hit-marker timer for the HUD.
    pub hit_marker: f32,
    pub hit_marker_kind: u8,
    pub hurt_flash: f32,
    pub toast: Vec<(String, f32, u8)>,
    pub placement_preview: Option<Placement>,
    pub interact_target: Option<loot::Target>,
    pub alive_cache: usize,
    /// A* searches bots may still start this tick (keeps frames smooth when many bots re-plan).
    pub ai_paths_left: i32,
}

const BOT_NAMES: [&str; 64] = [
    "xX_Shadow_Xx", "PixelPete", "NoScopeNina", "TurboToast", "SneakyPanda", "GrumpyGoose", "BlitzKrieg", "MangoMage", "QuietStorm", "ZigZagZoe",
    "CaptainCrunch", "RocketRaccoon", "DizzyDingo", "FrostByte", "LazyLynx", "HyperHippo", "WaffleKing", "NightOwl7", "BoomBoomBen", "CosmicCat",
    "SirSnipesALot", "TacoTornado", "JellyBean", "OmegaOtter", "PuddleJumper", "StealthyFox", "ThunderPug", "RiotRabbit", "VelvetViper", "ChillyWilly",
    "MadMaddie", "GhostPepper", "CrispyKnight", "DuskDragon", "FlipFlopFred", "AceAvocado", "ButterBandit", "NeonNomad", "SaltySalmon", "CranberryCrow",
    "JumpyJess", "BananaBlaster", "PocketRocket", "StarDustSam", "WobblyWolf", "IceCreamIke", "SpicySushi", "MinoMonster", "LuckyLuke", "BerryBlitz",
    "EchoEagle", "TinyTitan", "FuzzyFalcon", "CrimsonCoyote", "SleepyBear", "SunnySkies", "ToxicTulip", "RapidRay", "MoonlitMike", "DodgyDan",
    "PepperPotts", "NinjaNoodle", "CobaltCobra", "GigaGnome",
];

impl Game {
    pub fn new(world: World, cfg: GameConfig) -> Game {
        let mut rng = Rng::new(cfg.seed ^ 0xF1647);
        let mut actors: Vec<Actor> = Vec::new();
        let mut name_pool: Vec<&str> = BOT_NAMES.to_vec();
        rng.shuffle(&mut name_pool);
        for i in 0..=cfg.bots {
            let outfit = Outfit::random(&mut rng);
            let human = i == PLAYER;
            let name = if human { cfg.player_name.clone() } else { name_pool[(i - 1) % name_pool.len()].to_string() };
            let mut a = Actor::new(i, &name, human, outfit);
            a.inv.mats[Mat::Wood.index()] = cfg.start_mats;
            if !human {
                a.brain = Some(Box::new(ai::Brain::new(&mut rng, cfg.difficulty)));
            }
            actors.push(a);
        }
        let mut g = Game {
            world,
            cfg: cfg.clone(),
            rng,
            time: 0.0,
            phase: Phase::Bus,
            actors,
            pickups: vec![],
            chests: vec![],
            pieces: Pieces::new(),
            projectiles: vec![],
            storm: matchflow::initial_storm(),
            bus: matchflow::initial_bus(&mut Rng::new(cfg.seed ^ 0xB05)),
            events: vec![],
            fx: fx::Fx::new(),
            feed: vec![],
            winner: None,
            spectating: None,
            cam_dist: 3.4,
            fov_deg: 62.0,
            cam_shake: 0.0,
            fov_t: 0.0,
            felled: vec![],
            next_id: 1,
            match_time: 0.0,
            player_dead_time: 0.0,
            last_hurt_dir: None,
            hit_marker: 0.0,
            hit_marker_kind: 0,
            hurt_flash: 0.0,
            toast: vec![],
            placement_preview: None,
            interact_target: None,
            alive_cache: 0,
            ai_paths_left: 0,
        };
        g.spawn_world_loot();
        g.setup_start();
        g
    }

    pub fn player(&self) -> &Actor {
        &self.actors[PLAYER]
    }
    pub fn player_mut(&mut self) -> &mut Actor {
        &mut self.actors[PLAYER]
    }

    pub fn alive_count(&self) -> usize {
        self.actors.iter().filter(|a| a.alive).count()
    }

    pub fn env(&self) -> Env<'_> {
        Env::new(&self.world, &self.pieces.grid)
    }

    pub fn new_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn toast(&mut self, text: impl Into<String>, secs: f32, style: u8) {
        self.toast.push((text.into(), secs, style));
        if self.toast.len() > 4 {
            self.toast.remove(0);
        }
    }

    /// Put everyone on the bus (or on the ground when skipping it).
    fn setup_start(&mut self) {
        let skip = self.cfg.skip_bus;
        if skip {
            self.phase = Phase::Playing;
            self.bus.active = false;
            self.storm.active = true;
            matchflow::begin_storm_wait(self);
        }
        let n = self.actors.len();
        for i in 0..n {
            let a = &mut self.actors[i];
            if skip {
                // scatter on land
                let mut p;
                let mut tries = 0;
                loop {
                    p = self.rng.in_disc(380.0);
                    tries += 1;
                    if self.world.hm.height_at(p.x, p.y) > 3.0 || tries > 200 {
                        break;
                    }
                }
                let h = self.world.hm.height_at(p.x, p.y);
                let a = &mut self.actors[i];
                a.pos = Vec3::new(p.x, h, p.y);
                a.mode = MoveMode::Ground;
                a.on_ground = true;
                a.peak_y = h;
            } else {
                a.mode = MoveMode::Bus;
                a.pos = self.bus.pos;
                a.yaw = yaw_of(self.bus.dir);
                a.body_yaw = a.yaw;
                a.pitch = -0.2;
            }
        }
        self.alive_cache = self.alive_count();
    }

    // ------------------------------------------------------------------------------
    // Frame update
    // ------------------------------------------------------------------------------

    pub fn update(&mut self, dt_real: f32, input: &PlayerInput) {
        // the caller reads `events` after each update, so they only live for one frame
        self.events.clear();
        let dt = dt_real.clamp(0.0, 0.1);
        if dt <= 0.0 {
            return;
        }
        let steps = (dt / (1.0 / 60.0)).ceil().max(1.0) as usize;
        let h = dt / steps as f32;
        for k in 0..steps {
            let mut inp = input.clone();
            if k > 0 {
                // edge-triggered inputs apply to the first sub-step only; look deltas are split
                inp.fire_pressed = false;
                inp.reload = false;
                inp.interact = false;
                inp.select = None;
                inp.cycle = 0;
                inp.drop_selected = false;
                inp.toggle_build = false;
                inp.piece = None;
                inp.place = false;
                inp.cycle_mat = false;
                inp.exit_bus = false;
                inp.deploy = false;
                inp.emote = false;
                inp.spectate = 0;
            }
            inp.look = input.look / steps as f32;
            self.step(h, &inp);
        }
        self.after_update(dt);
    }

    fn step(&mut self, dt: f32, input: &PlayerInput) {
        self.time += dt;
        if self.phase != Phase::Over || self.player().alive {
            self.match_time += dt;
        }
        self.ai_paths_left = 3;
        matchflow::update_bus(self, dt);
        matchflow::update_storm(self, dt);

        let n = self.actors.len();
        for i in 0..n {
            if !self.actors[i].alive {
                self.actors[i].dead_time += dt;
                continue;
            }
            let intent = if i == PLAYER { self.player_intent(input) } else { self.bot_intent(i, dt) };
            self.apply_intent(i, &intent, dt);
        }
        if !self.actors[PLAYER].alive {
            self.player_dead_time += dt;
            self.update_spectate(input);
        }
        combat::update_projectiles(self, dt);
        loot::update_pickups(self, dt);
        self.pieces.tick(dt);
        self.update_felled(dt);
        matchflow::check_victory(self);
    }

    fn after_update(&mut self, dt: f32) {
        // gather events into effects
        let evs = std::mem::take(&mut self.events);
        for e in &evs {
            self.fx.on_event(e, &self.actors, self.time);
            self.handle_player_event(e);
        }
        self.events = evs;
        self.fx.update(dt);
        self.hit_marker = (self.hit_marker - dt).max(0.0);
        self.hurt_flash = (self.hurt_flash - dt * 2.2).max(0.0);
        self.cam_shake = (self.cam_shake - dt * 2.5).max(0.0);
        for f in &mut self.feed {
            f.age += dt;
        }
        self.feed.retain(|f| f.age < 7.0);
        for t in &mut self.toast {
            t.1 -= dt;
        }
        self.toast.retain(|t| t.1 > 0.0);
        self.alive_cache = self.alive_count();
        // keep the build preview and prompts fresh for the HUD
        self.update_previews();
    }

    /// The player's keyboard/mouse become an `Intent`.
    fn player_intent(&mut self, input: &PlayerInput) -> Intent {
        let (yaw, pitch);
        {
            let a = &mut self.actors[PLAYER];
            let freefall = matches!(a.mode, MoveMode::Freefall | MoveMode::Glide | MoveMode::Bus);
            a.yaw -= input.look.x;
            a.pitch = (a.pitch + input.look.y).clamp(if freefall { -1.45 } else { -1.5 }, 1.5);
            yaw = a.yaw;
            pitch = a.pitch;
        }
        let f = yaw_forward(yaw);
        let r = yaw_right(yaw);
        let m = input.move_axis;
        let wish = Vec2::new(f.x * m.y + r.x * m.x, f.z * m.y + r.z * m.x);
        Intent {
            wish,
            yaw,
            pitch,
            jump: input.jump,
            sprint: input.sprint,
            crouch: input.crouch,
            fire: input.fire,
            fire_pressed: input.fire_pressed,
            ads: input.ads,
            reload: input.reload,
            interact: input.interact,
            select: input.select,
            cycle: input.cycle,
            drop_selected: input.drop_selected,
            toggle_build: input.toggle_build,
            piece: input.piece,
            place: input.place,
            cycle_mat: input.cycle_mat,
            exit_bus: input.exit_bus,
            deploy: input.deploy,
            emote: input.emote,
        }
    }

    fn bot_intent(&mut self, i: usize, dt: f32) -> Intent {
        if self.phase == Phase::Over {
            // the match is decided: the winner (if a bot) celebrates instead of wandering off
            let a = &self.actors[i];
            return Intent { yaw: a.yaw, pitch: a.pitch, ..Default::default() };
        }
        ai::think(self, i, dt)
    }

    /// Run one actor's intent through movement, items, combat and building.
    pub fn apply_intent(&mut self, i: usize, it: &Intent, dt: f32) {
        let god = self.cfg.god_mode && i == PLAYER;
        {
            let a = &mut self.actors[i];
            a.yaw = it.yaw;
            a.pitch = it.pitch.clamp(-1.5, 1.5);
        }
        let mode = self.actors[i].mode;
        let mut fall_damage = 0.0;
        let mut landed_sky = false;
        match mode {
            MoveMode::Bus => {
                let a = &mut self.actors[i];
                a.pos = self.bus.pos - Vec3::Y * 1.0;
                a.vel = Vec3::ZERO;
                a.body_yaw = yaw_of(self.bus.dir);
                if it.exit_bus {
                    matchflow::leave_bus(self, i);
                }
            }
            MoveMode::Freefall => {
                let env = Env::new(&self.world, &self.pieces.grid);
                let a = &mut self.actors[i];
                let r = movement::step_freefall(a, it, &env, dt, &mut self.events);
                landed_sky = r.landed_from_sky;
            }
            MoveMode::Glide => {
                let env = Env::new(&self.world, &self.pieces.grid);
                let a = &mut self.actors[i];
                let r = movement::step_glide(a, it, &env, dt, &mut self.events);
                landed_sky = r.landed_from_sky;
            }
            MoveMode::Ground => {
                let env = Env::new(&self.world, &self.pieces.grid);
                let a = &mut self.actors[i];
                let r = movement::step_ground(a, it, &env, dt, &mut self.events);
                fall_damage = r.fall_damage;
            }
            MoveMode::Swim => {
                let env = Env::new(&self.world, &self.pieces.grid);
                let a = &mut self.actors[i];
                movement::step_swim(a, it, &env, dt, &mut self.events);
            }
            MoveMode::Dead => {}
        }
        let _ = landed_sky;
        // an invisible wall far out at sea keeps swimmers from drifting off the map
        if matches!(self.actors[i].mode, MoveMode::Ground | MoveMode::Swim) {
            let lim = WORLD_HALF + 110.0;
            let a = &mut self.actors[i];
            a.pos.x = a.pos.x.clamp(-lim, lim);
            a.pos.z = a.pos.z.clamp(-lim, lim);
        }
        if fall_damage > 0.0 && !god {
            self.damage_actor(i, fall_damage, None, "Fall damage", false, self.actors[i].pos, false);
        }
        if !self.actors[i].alive {
            return;
        }
        movement::update_body(&mut self.actors[i], it, dt);
        movement::update_anim(&mut self.actors[i], dt);
        if matches!(self.actors[i].mode, MoveMode::Ground | MoveMode::Swim) {
            self.handle_items(i, it, dt);
        }
    }

    fn update_felled(&mut self, dt: f32) {
        for f in &mut self.felled {
            f.t += dt;
        }
        self.felled.retain(|f| f.t < 2.6);
    }

    fn update_spectate(&mut self, input: &PlayerInput) {
        let alive: Vec<usize> = self.actors.iter().filter(|a| a.alive && a.id != PLAYER).map(|a| a.id).collect();
        if alive.is_empty() {
            self.spectating = None;
            return;
        }
        let cur = self.spectating.filter(|s| self.actors[*s].alive);
        if cur.is_none() || input.spectate != 0 {
            let idx = cur.and_then(|c| alive.iter().position(|&x| x == c)).unwrap_or(0) as i32;
            let n = alive.len() as i32;
            let next = if cur.is_none() {
                // follow the killer first, otherwise the nearest survivor
                let killer = self.actors[PLAYER].last_damage_from.filter(|k| self.actors[*k].alive);
                killer.map(|k| alive.iter().position(|&x| x == k).unwrap_or(0) as i32).unwrap_or(0)
            } else {
                (idx + input.spectate).rem_euclid(n)
            };
            self.spectating = Some(alive[next as usize]);
        }
    }

    fn handle_player_event(&mut self, e: &Event) {
        match e {
            Event::HitConfirm { head, shield, kill } => {
                self.hit_marker = 0.22;
                self.hit_marker_kind = if *kill { 3 } else if *head { 2 } else if *shield { 1 } else { 0 };
            }
            Event::Hurt { from, .. } => {
                self.hurt_flash = 1.0;
                self.last_hurt_dir = *from;
                self.cam_shake = self.cam_shake.max(0.35);
            }
            Event::Explosion { pos, radius } => {
                let d = self.actors[self.camera_actor()].pos.distance(*pos);
                if d < 60.0 {
                    self.cam_shake = self.cam_shake.max((1.0 - d / 60.0) * 1.2 * (radius / 7.0).min(1.0));
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------------------------
    // Camera
    // ------------------------------------------------------------------------------

    pub fn camera_actor(&self) -> usize {
        if self.actors[PLAYER].alive {
            PLAYER
        } else {
            self.spectating.unwrap_or(PLAYER)
        }
    }

    /// Shoulder pivot used for both the camera and the aim ray.
    pub fn aim_ray(&self, idx: usize) -> (Vec3, Vec3) {
        let a = &self.actors[idx];
        let dir = look_dir(a.yaw, a.pitch);
        let right = yaw_right(a.yaw);
        let shoulder = if a.ads { 0.55 } else { 0.62 };
        let pivot = Vec3::new(a.pos.x, a.eye_smooth.max(a.pos.y - 0.5) + if a.crouching { 1.08 } else { 1.52 }, a.pos.z) + right * shoulder;
        (pivot, dir)
    }

    pub fn camera(&self, aspect: f32) -> Camera {
        let idx = self.camera_actor();
        let a = &self.actors[idx];
        let (pivot, dir) = self.aim_ray(idx);
        let env = self.env();
        let airborne = matches!(a.mode, MoveMode::Freefall | MoveMode::Glide | MoveMode::Bus);
        let mut back = if airborne { 7.5 } else { lerp(self.cam_dist, self.cam_dist * 0.62, a.anim.aim) };
        if matches!(a.mode, MoveMode::Bus) {
            back = 21.0;
        }
        let want = pivot - dir * back + Vec3::Y * if airborne { 0.8 } else { 0.18 };
        // pull the camera in when something is behind the player
        let to = want - pivot;
        let len = to.length().max(0.001);
        let d = to / len;
        let mut dist = len;
        // sample a few rays around the camera to emulate a sphere cast
        let up = Vec3::Y;
        let side = d.cross(up).normalize_or_zero();
        for off in [Vec3::ZERO, side * 0.22, -side * 0.22, up * 0.22, -up * 0.22] {
            if let Some(h) = env.raycast(pivot + off, d, len + 0.25, false) {
                dist = dist.min((h.t - 0.28).max(0.35));
            }
        }
        let mut pos = pivot + d * dist;
        // never dip below the terrain
        let th = env.terrain(pos.x, pos.z) + 0.25;
        if pos.y < th {
            pos.y = th;
        }
        // screen shake
        if self.cam_shake > 0.0 {
            let t = self.time * 60.0;
            pos += Vec3::new((t * 1.7).sin(), (t * 2.3).cos(), (t * 1.1).sin()) * 0.06 * self.cam_shake;
        }
        let mut fov = self.fov_deg.to_radians();
        if let Some((kind, _, _)) = a.inv.selected_weapon() {
            let zoom = kind.def().ads_zoom;
            fov *= lerp(1.0, zoom, a.anim.aim);
        }
        fov += a.anim.sprint * 0.07;
        if airborne {
            fov += 0.12;
        }
        let roll = if a.mode == MoveMode::Glide { -a.anim.lean_side * 0.12 } else { 0.0 };
        let target = pivot + dir * 40.0;
        let look = (target - pos).normalize_or_zero();
        let mut look = if look == Vec3::ZERO { dir } else { look };
        // victory: the winner dances and the camera swings round in front of them
        let k = if self.phase == Phase::Over && self.winner == Some(idx) { smoothstep(0.0, 1.0, a.anim.emote) } else { 0.0 };
        if k > 0.0 {
            let centre = a.pos + Vec3::Y * 1.05;
            let ang = a.body_yaw + 0.3 * a.anim.emote_clock;
            let want = centre + yaw_forward(ang) * 4.3 + Vec3::Y * 0.5;
            let to = want - centre;
            let len = to.length().max(0.001);
            let d = to / len;
            let dist = env.raycast(centre, d, len + 0.25, false).map_or(len, |h| (h.t - 0.28).max(0.8));
            let mut orbit = centre + d * dist.min(len);
            orbit.y = orbit.y.max(env.terrain(orbit.x, orbit.z) + 0.25);
            pos = pos.lerp(orbit, k);
            let to_centre = (centre - pos).normalize_or_zero();
            if to_centre != Vec3::ZERO {
                look = look.lerp(to_centre, k).normalize_or_zero();
            }
            fov *= lerp(1.0, 0.9, k);
        }
        Camera::look(pos, look, roll, fov, aspect, 0.1)
    }

    /// Smooth the camera distance (call once per frame after `update`).
    pub fn tick_camera(&mut self, dt: f32) {
        // the camera distance eases back out after being pulled in; nothing to do besides keeping state sane
        self.cam_dist = lerp(self.cam_dist, 3.4, damp(3.0, dt));
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    pub fn game(bots: usize, skip_bus: bool) -> Game {
        game_cfg(GameConfig { bots, skip_bus, seed: 7, god_mode: false, ..Default::default() })
    }

    pub fn game_cfg(cfg: GameConfig) -> Game {
        Game::new(World::generate(1234), cfg)
    }
}

#[cfg(test)]
mod monkey {
    //! Randomised "monkey" play: the human mashes inputs while bots fight, checking invariants every step.
    use super::testutil::game_cfg;
    use super::*;

    fn random_input(rng: &mut Rng, t: f32, g: &Game) -> PlayerInput {
        // sticky choices make the monkey behave more like a person than pure noise
        let phase = (t * 0.7) as i32;
        let mut s = Rng::new(phase as u64 ^ 0x77);
        let mv = Vec2::new(s.range(-1.0, 1.0).round(), s.range(-1.0, 1.0).round());
        let mut p = PlayerInput {
            move_axis: mv,
            look: Vec2::new(rng.range(-0.06, 0.06), rng.range(-0.03, 0.03)),
            jump: rng.chance(0.04),
            sprint: s.chance(0.5),
            crouch: s.chance(0.1),
            fire: s.chance(0.35),
            fire_pressed: rng.chance(0.05),
            ads: s.chance(0.25),
            reload: rng.chance(0.01),
            interact: rng.chance(0.08),
            select: if rng.chance(0.02) { Some(rng.below(6)) } else { None },
            cycle: if rng.chance(0.01) { 1 } else { 0 },
            drop_selected: rng.chance(0.002),
            toggle_build: rng.chance(0.01),
            piece: if rng.chance(0.02) { Some(PieceKind::ALL[rng.below(4)]) } else { None },
            place: s.chance(0.3),
            cycle_mat: rng.chance(0.005),
            exit_bus: rng.chance(0.03),
            deploy: rng.chance(0.02),
            emote: rng.chance(0.01),
            spectate: if rng.chance(0.01) { 1 } else { 0 },
        };
        if g.actors[PLAYER].mode == MoveMode::Bus && t > 3.0 {
            p.exit_bus = true;
        }
        p
    }

    fn check(g: &Game, seed: u64, step: usize) {
        for a in &g.actors {
            let ctx = format!("seed {seed} step {step} actor {} ({:?})", a.id, a.mode);
            assert!(a.pos.is_finite() && a.vel.is_finite(), "{ctx}: non-finite state {:?} {:?}", a.pos, a.vel);
            assert!(a.yaw.is_finite() && a.pitch.is_finite(), "{ctx}: angles");
            assert!(a.hp <= 100.01 && a.shield <= 100.01, "{ctx}: hp {} shield {}", a.hp, a.shield);
            if a.alive {
                assert!(a.hp > 0.0, "{ctx}: alive with hp {}", a.hp);
                assert!(a.pos.y > -20.0 && a.pos.y < 600.0, "{ctx}: y {}", a.pos.y);
                if matches!(a.mode, MoveMode::Ground | MoveMode::Swim) {
                    assert!(a.pos.x.abs() < 900.0 && a.pos.z.abs() < 900.0, "{ctx}: out of the world {:?}", a.pos);
                }
                if matches!(a.mode, MoveMode::Ground) {
                    let th = g.world.hm.height_at(a.pos.x, a.pos.z);
                    assert!(a.pos.y > th - 1.0, "{ctx}: under the terrain {} < {th}", a.pos.y);
                }
            }
            assert!(a.inv.selected < 6);
            for (i, s) in a.inv.slots.iter().enumerate() {
                if i == 0 {
                    assert!(matches!(s, Some(items::Item::Pickaxe)), "{ctx}: slot 0 must keep the pickaxe");
                } else if let Some(items::Item::Pickaxe) = s {
                    panic!("{ctx}: stray pickaxe in slot {i}");
                }
            }
            for k in 0..3 {
                assert!(a.inv.mats[k] <= 999);
            }
        }
        assert!(g.pickups.len() < 5000, "pickup leak: {}", g.pickups.len());
        assert!(g.fx.count() <= 5000);
        assert!(g.projectiles.len() < 200);
    }

    /// Everything the renderer and the audio engine are handed must stay finite and bounded.
    fn check_outputs(scene: &scene::Scene, seed: u64, step: usize) {
        for i in &scene.instances {
            for v in i.m0.iter().chain(&i.m1).chain(&i.m2).chain(&i.color).chain(&i.params) {
                assert!(v.is_finite(), "seed {seed} step {step}: non-finite instance {i:?}");
            }
        }
        for p in scene.particles.iter().chain(&scene.particles_add) {
            for v in p.a.iter().chain(&p.b).chain(&p.color).chain(&p.c) {
                assert!(v.is_finite(), "seed {seed} step {step}: non-finite particle {p:?}");
            }
        }
        assert!(scene.instances.len() < 12_000, "seed {seed} step {step}: {} instances", scene.instances.len());
        assert!(scene.particles.len() + scene.particles_add.len() < 12_000, "seed {seed} step {step}: {} particles", scene.particles.len() + scene.particles_add.len());
        let mut next = 0;
        for b in &scene.batches {
            assert_eq!(b.first, next, "seed {seed} step {step}: batches must be contiguous");
            next += b.count;
        }
        assert_eq!(next as usize, scene.instances.len());
    }

    fn run(seed: u64, bots: usize, seconds: f32, skip_bus: bool) {
        static MESHES: std::sync::OnceLock<Vec<crate::mesh::MeshData>> = std::sync::OnceLock::new();
        let meshes = MESHES.get_or_init(crate::meshlib::build_all);
        let mut scene = scene::Scene::new(meshes);
        let mut mixer = audio_map::Mixer::new();
        let mut cues = vec![];
        let mut g = game_cfg(GameConfig { bots, skip_bus, seed, storm_speed: 3.0, god_mode: false, ..Default::default() });
        let mut rng = Rng::new(seed * 31 + 7);
        let dt = 1.0 / 30.0;
        let steps = (seconds / dt) as usize;
        for step in 0..steps {
            let t = step as f32 * dt;
            let input = random_input(&mut rng, t, &g);
            g.update(dt, &input);
            if step % 15 == 0 {
                check(&g, seed, step);
                let cam = g.camera(1.7);
                scene.build(&g, &cam);
                check_outputs(&scene, seed, step);
                cues.clear();
                mixer.process(&g, &cam, &mut cues);
                assert!(cues.len() < 400, "seed {seed} step {step}: {} audio cues in one frame", cues.len());
                for c in &cues {
                    assert!(c.gain.is_finite() && c.pan.is_finite() && c.pitch.is_finite() && c.lowpass.is_finite() && c.delay.is_finite(), "seed {seed} step {step}: bad cue {c:?}");
                }
            }
            if g.phase == Phase::Over && g.actors[PLAYER].dead_time > 5.0 {
                break;
            }
        }
        check(&g, seed, steps);
        // the HUD and scene builders must cope with whatever state the match ended in
        let cam = g.camera(1.7);
        let json = hud::hud_json(&g, &cam, true);
        assert!(json.starts_with('{') && !json.contains("NaN"));
    }

    /// Two identical matches fed identical inputs must stay identical (no hidden global state,
    /// no iteration-order dependence), which also makes any bug report reproducible from a seed.
    #[test]
    fn the_simulation_is_deterministic() {
        let mk = || game_cfg(GameConfig { bots: 16, skip_bus: false, seed: 11, storm_speed: 3.0, god_mode: false, ..Default::default() });
        let (mut a, mut b) = (mk(), mk());
        let (mut ra, mut rb) = (Rng::new(5), Rng::new(5));
        let dt = 1.0 / 30.0;
        for step in 0..(60.0 / dt) as usize {
            let t = step as f32 * dt;
            let ia = random_input(&mut ra, t, &a);
            let ib = random_input(&mut rb, t, &b);
            a.update(dt, &ia);
            b.update(dt, &ib);
        }
        for (x, y) in a.actors.iter().zip(&b.actors) {
            assert_eq!(x.pos, y.pos, "actor {} diverged", x.id);
            assert_eq!((x.hp, x.shield, x.kills, x.alive), (y.hp, y.shield, y.kills, y.alive), "actor {} stats diverged", x.id);
        }
        assert_eq!(a.pickups.len(), b.pickups.len());
        assert_eq!(a.pieces.count(), b.pieces.count());
    }

    #[test]
    fn monkey_with_the_bus() {
        run(1, 24, 150.0, false);
    }

    #[test]
    fn monkey_on_the_ground_a() {
        run(2, 30, 180.0, true);
    }

    #[test]
    fn monkey_on_the_ground_b() {
        run(3, 18, 180.0, true);
    }

    /// Many seeds; slower, run with `cargo test --release -- --ignored monkey_marathon`
    /// (`MONKEY_SEEDS=70..300` picks a different range).
    #[test]
    #[ignore]
    fn monkey_marathon() {
        let range = std::env::var("MONKEY_SEEDS")
            .ok()
            .and_then(|s| {
                let (a, b) = s.split_once("..")?;
                Some(a.parse::<u64>().ok()?..b.parse::<u64>().ok()?)
            })
            .unwrap_or(10..70);
        for seed in range {
            run(seed, 10 + (seed as usize % 30), 200.0, seed % 3 != 0);
        }
    }

    #[test]
    fn monkey_builders_and_fighters() {
        // lots of building materials and weapons make for more interesting collisions
        let mut g = game_cfg(GameConfig { bots: 20, skip_bus: true, seed: 5, storm_speed: 2.0, start_mats: 900, ..Default::default() });
        for kind in items::WeaponKind::ALL {
            g.actors[PLAYER].inv.add_weapon(kind, items::Rarity::Epic, kind.def().mag);
        }
        for k in items::AmmoKind::ALL {
            g.actors[PLAYER].inv.ammo[k.index()] = k.cap();
        }
        let mut rng = Rng::new(99);
        for step in 0..5400 {
            let t = step as f32 / 30.0;
            let mut input = random_input(&mut rng, t, &g);
            input.piece = if rng.chance(0.1) { Some(PieceKind::ALL[rng.below(4)]) } else { None };
            input.place = true;
            g.update(1.0 / 30.0, &input);
            if step % 15 == 0 {
                check(&g, 5, step);
            }
        }
        assert!(g.pieces.count() > 0);
    }
}
