#![allow(clippy::too_many_arguments, clippy::type_complexity, clippy::needless_range_loop)]

mod app;
mod gpu;
mod netapp;
mod renderer;

use app::App;
use fn_core::audio_synth::{synth, Sfx};
use fn_core::game::{Difficulty, GameConfig, GameMode};
use renderer::types::*;
use renderer::Renderer;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

thread_local! {
    static GPU: RefCell<Option<gpu::Gpu>> = const { RefCell::new(None) };
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app<R>(default: R, f: impl FnOnce(&mut App) -> R) -> R {
    APP.with(|a| match a.borrow_mut().as_mut() {
        Some(app) => f(app),
        None => default,
    })
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// Request the WebGPU adapter/device and configure the canvas. Returns a description of the adapter.
#[wasm_bindgen]
pub async fn init_gpu(canvas: web_sys::HtmlCanvasElement) -> Result<String, JsValue> {
    let gpu = gpu::Gpu::new(&canvas).await.map_err(|e| JsValue::from_str(&e))?;
    let info = format!("{} / {}", gpu.backend, gpu.adapter_name);
    GPU.with(|g| *g.borrow_mut() = Some(gpu));
    Ok(info)
}

fn parse_opts(opts: &str) -> (GameConfig, String) {
    let mut cfg = GameConfig { seed: 1234, ..Default::default() };
    let mut quality = "high".to_string();
    for kv in opts.split(';') {
        let Some((k, v)) = kv.split_once('=') else { continue };
        match k.trim() {
            "bots" => cfg.bots = v.parse::<usize>().unwrap_or(39).clamp(0, 98),
            "mode" => cfg.mode = match v { "zero-build" => GameMode::ZeroBuild, "lego" => GameMode::Lego, _ => GameMode::BattleRoyale },
            "difficulty" => {
                cfg.difficulty = match v {
                    "easy" => Difficulty::Easy,
                    "hard" => Difficulty::Hard,
                    _ => Difficulty::Normal,
                }
            }
            "name" => cfg.player_name = v.chars().take(18).collect::<String>().trim().to_string(),
            "outfit" => cfg.player_outfit = match v { "ranger" => 1, "pilot" => 2, "vanguard" => 3, _ => 0 },
            "seed" => cfg.seed = v.parse().unwrap_or(1234),
            "skipbus" => cfg.skip_bus = v == "1",
            "god" => cfg.god_mode = v == "1",
            "storm" => cfg.storm_speed = v.parse().unwrap_or(1.0),
            "mats" => cfg.start_mats = v.parse().unwrap_or(100),
            "quality" => quality = v.to_string(),
            _ => {}
        }
    }
    if cfg.player_name.is_empty() {
        cfg.player_name = "You".into();
    }
    (cfg, quality)
}

/// Create the match (the first call also creates the renderer). Heavy: generates the island.
/// Options: `bots=39;difficulty=normal;name=You;seed=1234;skipbus=0;god=0;storm=1;quality=high`.
#[wasm_bindgen]
pub fn start_match(opts: &str) -> Result<String, JsValue> {
    let (cfg, quality) = parse_opts(opts);
    let t0 = js_sys::Date::now();
    let exists = APP.with(|a| a.borrow().is_some());
    if exists {
        with_app((), |app| app.restart(cfg));
    } else {
        let gpu = GPU.with(|g| g.borrow_mut().take()).ok_or_else(|| JsValue::from_str("init_gpu must be called first"))?;
        let meshes = fn_core::meshlib::build_all();
        let renderer = Renderer::new(gpu, Quality::from_name(&quality), &meshes);
        let app = App::new(renderer, meshes, cfg);
        APP.with(|a| *a.borrow_mut() = Some(app));
    }
    Ok(format!("match ready in {:.0} ms", js_sys::Date::now() - t0))
}

/// Menu mode shows a flyover of the island instead of running the game.
#[wasm_bindgen]
pub fn set_menu(menu: bool) {
    with_app((), |app| {
        app.menu = menu;
        app.input.release_all();
    });
}

#[wasm_bindgen]
pub fn frame(dt: f32) {
    with_app((), |app| app.frame(dt));
}

#[wasm_bindgen]
pub fn resize(w: u32, h: u32) {
    with_app((), |app| app.renderer.resize_surface(w, h));
}

#[wasm_bindgen]
pub fn set_quality(name: &str) {
    with_app((), |app| app.renderer.set_quality(Quality::from_name(name)));
}

#[wasm_bindgen]
pub fn key(code: &str, down: bool) {
    with_app((), |app| app.input.key(code, down));
}

#[wasm_bindgen]
pub fn mouse_button(button: i32, down: bool) {
    with_app((), |app| app.input.button(button, down));
}

#[wasm_bindgen]
pub fn mouse_move(dx: f32, dy: f32) {
    with_app((), |app| app.input.mouse_move(dx, dy));
}

#[wasm_bindgen]
pub fn wheel(dy: f32) {
    with_app((), |app| app.input.wheel(dy));
}

#[wasm_bindgen]
pub fn release_input() {
    with_app((), |app| app.input.release_all());
}

#[wasm_bindgen]
pub fn set_paused(paused: bool) {
    with_app((), |app| {
        app.paused = paused;
        if paused {
            app.input.release_all();
        }
    });
}

/// Sensitivity (radians per pixel), invert-Y, vertical FOV in degrees, name tags, auto-sprint.
#[wasm_bindgen]
pub fn set_options(sens: f32, invert_y: bool, fov: f32, tags: bool, auto_sprint: bool) {
    with_app((), |app| {
        app.sens = sens.clamp(0.0003, 0.02);
        app.invert_y = invert_y;
        app.mode.game_mut().fov_deg = fov.clamp(40.0, 100.0);
        app.show_tags = tags;
        app.input.auto_sprint = auto_sprint;
    });
}

/// The HUD snapshot for this frame (JSON).
#[wasm_bindgen]
pub fn hud() -> String {
    with_app(String::new(), |app| app.hud())
}

/// 1024x1024 RGBA top-down map of the island.
#[wasm_bindgen]
pub fn minimap() -> Vec<u8> {
    with_app(vec![], |app| app.minimap.clone())
}

/// "name,x,z,kind;..." for every point of interest.
#[wasm_bindgen]
pub fn poi_list() -> String {
    with_app(String::new(), |app| app.mode.game().world.layout.pois.iter().map(|q| format!("{},{:.1},{:.1},{:?},{:.0}", q.name, q.center.x, q.center.y, q.kind, q.radius)).collect::<Vec<_>>().join(";"))
}

#[wasm_bindgen]
pub fn ground_height(x: f32, z: f32) -> f32 {
    with_app(0.0, |app| app.mode.game().world.height_at(x, z))
}

/// Equip an inventory slot (0 = harvesting tool).
#[wasm_bindgen]
pub fn inventory_select(slot: u32) {
    with_app((), |app| app.input.select_slot(slot as usize));
}

/// Drop the item in a slot.
#[wasm_bindgen]
pub fn inventory_drop(slot: u32) {
    with_app((), |app| app.input.drop_slot(slot as usize));
}

/// Weapon statistics for the inventory screen (JSON array in `WeaponKind` order).
#[wasm_bindgen]
pub fn weapon_defs() -> String {
    use fn_core::game::items::WeaponKind;
    let items: Vec<String> = WeaponKind::ALL
        .iter()
        .map(|k| {
            let d = k.def();
            format!(
                "{{\"name\":\"{}\",\"damage\":{},\"pellets\":{},\"rate\":{},\"mag\":{},\"reload\":{},\"range\":{},\"head\":{},\"auto\":{},\"ammo\":{}}}",
                d.name, d.damage, d.pellets, d.rate, d.mag, d.reload, d.range, d.head_mult, d.auto, d.ammo.index()
            )
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Consumable statistics (JSON array in `ConsumableKind` order).
#[wasm_bindgen]
pub fn consumable_defs() -> String {
    use fn_core::game::items::ConsumableKind;
    let items: Vec<String> = ConsumableKind::ALL
        .iter()
        .map(|k| {
            let d = k.def();
            format!("{{\"name\":\"{}\",\"heal\":{},\"shield\":{},\"maxHp\":{},\"maxShield\":{},\"time\":{},\"stack\":{}}}", d.name, d.heal, d.shield, d.max_hp, d.max_shield, d.use_time, d.stack)
        })
        .collect();
    format!("[{}]", items.join(","))
}

// ---- multiplayer -----------------------------------------------------------------------------------------
// The page owns the WebRTC connections; it feeds what arrives into `net_receive` and sends what `net_poll` returns.

fn now_ms() -> u32 {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now() as u32).unwrap_or(0)
}

/// Open a room for other players; `name` is the host's name.
#[wasm_bindgen]
pub fn net_host_open(name: &str) {
    with_app((), |app| app.net_host_open(name));
}

/// Get ready to join a room; the hello is sent with the next `net_poll`.
#[wasm_bindgen]
pub fn net_guest_open(name: &str) {
    with_app((), |app| app.net_guest_open(name));
}

/// Leave the room or the match (the goodbyes are sent with the next `net_poll`).
#[wasm_bindgen]
pub fn net_reset() {
    with_app((), |app| app.net_reset());
}

/// A connection to a player opened (host side).
#[wasm_bindgen]
pub fn net_connected(peer: u32) {
    with_app((), |app| app.net_connected(peer));
}

/// A connection closed.
#[wasm_bindgen]
pub fn net_disconnected(peer: u32) {
    with_app((), |app| app.net_disconnected(peer));
}

/// A message arrived (peer 0 is the host, for guests).
#[wasm_bindgen]
pub fn net_receive(peer: u32, data: &[u8]) {
    with_app((), |app| app.net_receive(peer, data, now_ms()));
}

/// Messages to send: repeated [peer u32 le][reliable u8][length u32 le][bytes].
#[wasm_bindgen]
pub fn net_poll() -> Vec<u8> {
    with_app(vec![], |app| app.net_poll())
}

/// Advance a multiplayer match without rendering (used while the loading screen waits for the other players).
#[wasm_bindgen]
pub fn net_tick(dt: f32) {
    with_app((), |app| app.net_tick(dt));
}

/// JSON about the room or the match: `{state, names, ...}`.
#[wasm_bindgen]
pub fn net_status() -> String {
    with_app("{\"state\":\"none\"}".to_string(), |app| app.net_status())
}

/// The host starts the match: tells the guests. Send what `net_poll` has, then call `net_finish_host`.
/// Options as for `start_match` (bots, difficulty, skipbus, seed, storm, mats).
#[wasm_bindgen]
pub fn net_begin_host(opts: &str) -> Result<(), JsValue> {
    let (cfg, _) = parse_opts(opts);
    with_app(Err("no game".to_string()), |app| app.net_begin_host(&cfg)).map_err(|e| JsValue::from_str(&e))
}

/// Builds the island and starts the match the host announced. Heavy.
#[wasm_bindgen]
pub fn net_finish_host() -> Result<String, JsValue> {
    let t0 = js_sys::Date::now();
    with_app(Err("no game".to_string()), |app| app.net_finish_host()).map_err(|e| JsValue::from_str(&e))?;
    Ok(format!("match ready in {:.0} ms", js_sys::Date::now() - t0))
}

/// A guest builds the island and its copy of the match once the host has started. Heavy.
#[wasm_bindgen]
pub fn net_start_guest() -> Result<String, JsValue> {
    let t0 = js_sys::Date::now();
    with_app(Err("no game".to_string()), |app| app.net_start_guest()).map_err(|e| JsValue::from_str(&e))?;
    Ok(format!("match ready in {:.0} ms", js_sys::Date::now() - t0))
}

// ---- audio ---------------------------------------------------------------------------------------------

/// "name:variants:loop;..." in `Sfx` index order.
#[wasm_bindgen]
pub fn audio_manifest() -> String {
    Sfx::ALL.iter().map(|s| format!("{}:{}:{}", s.name(), s.variants(), s.is_loop() as u8)).collect::<Vec<_>>().join(";")
}

/// Synthesise one sound (mono samples in -1..1).
#[wasm_bindgen]
pub fn audio_render(sfx: u32, variant: u32, sample_rate: u32) -> Vec<f32> {
    match Sfx::ALL.get(sfx as usize) {
        Some(s) => synth(*s, sample_rate, variant),
        None => vec![],
    }
}

/// Cues produced by the last frame, 8 floats each: sfx, variant, pan, gain, pitch, low-pass Hz, delay s, reserved.
#[wasm_bindgen]
pub fn audio_cues() -> Vec<f32> {
    with_app(vec![], |app| {
        let mut v = Vec::with_capacity(app.cues.len() * 8);
        for c in &app.cues {
            v.extend_from_slice(&[c.sfx as usize as f32, c.variant as f32, c.pan, c.gain, c.pitch, c.lowpass, c.delay, 0.0]);
        }
        app.cues.clear();
        v
    })
}

/// Levels of the continuous loops: bus, wind, glider, storm, chest hum, hum pan.
#[wasm_bindgen]
pub fn audio_loops() -> Vec<f32> {
    with_app(vec![0.0; 6], |app| {
        let l = app.loops;
        vec![l.bus, l.wind, l.glider, l.storm, l.hum, l.hum_pan]
    })
}

// ---- debugging / testing ---------------------------------------------------------------------------------

#[wasm_bindgen]
pub fn debug(cmd: &str) -> String {
    with_app("no game".to_string(), |app| app.debug(cmd))
}

#[wasm_bindgen]
pub fn capture_request() {
    with_app((), |app| app.renderer.request_capture());
}

/// Returns [width u32le, height u32le, ...rgba] or an empty array when no capture is ready.
#[wasm_bindgen]
pub fn capture_poll() -> Vec<u8> {
    with_app(vec![], |app| {
        if let Some((w, h, data)) = app.renderer.poll_capture() {
            let mut out = Vec::with_capacity(data.len() + 8);
            out.extend_from_slice(&w.to_le_bytes());
            out.extend_from_slice(&h.to_le_bytes());
            out.extend_from_slice(&data);
            return out;
        }
        vec![]
    })
}

#[wasm_bindgen]
pub fn render_stats() -> String {
    with_app(String::new(), |app| format!("{:?}", app.renderer.stats))
}
