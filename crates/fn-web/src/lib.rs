mod gpu;
mod renderer;

use fn_core::camera::Camera;
use fn_core::world::World;
use glam::{Vec3, Vec4};
use renderer::types::*;
use renderer::Renderer;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

struct Preview {
    world: World,
    renderer: Renderer,
    cam_pos: Vec3,
    yaw: f32,
    pitch: f32,
    time: f32,
}

thread_local! {
    static PREVIEW: RefCell<Option<Preview>> = const { RefCell::new(None) };
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub async fn start_preview(canvas: web_sys::HtmlCanvasElement, seed: u32, quality: String) -> Result<String, JsValue> {
    let gpu = gpu::Gpu::new(&canvas).await.map_err(|e| JsValue::from_str(&e))?;
    let info = format!("{} / {}", gpu.backend, gpu.adapter_name);
    let t0 = js_sys::Date::now();
    let world = World::generate(seed);
    let t1 = js_sys::Date::now();
    let meshes = fn_core::meshlib::build_all();
    let mut r = Renderer::new(gpu, Quality::from_name(&quality), &meshes);
    r.set_world(&world);
    let t2 = js_sys::Date::now();
    PREVIEW.with(|p| {
        *p.borrow_mut() = Some(Preview { world, renderer: r, cam_pos: Vec3::new(0.0, 60.0, 200.0), yaw: 0.0, pitch: -0.2, time: 0.0 });
    });
    Ok(format!("{info}; worldgen {:.0}ms, upload {:.0}ms", t1 - t0, t2 - t1))
}

#[wasm_bindgen]
pub fn ground_height(x: f32, z: f32) -> f32 {
    PREVIEW.with(|p| p.borrow().as_ref().map(|p| p.world.height_at(x, z)).unwrap_or(0.0))
}

/// JSON-ish summary of points of interest for tests: "name,x,z;..."
#[wasm_bindgen]
pub fn poi_list() -> String {
    PREVIEW.with(|p| {
        p.borrow()
            .as_ref()
            .map(|p| p.world.layout.pois.iter().map(|q| format!("{},{:.1},{:.1},{:?}", q.name, q.center.x, q.center.y, q.kind)).collect::<Vec<_>>().join(";"))
            .unwrap_or_default()
    })
}

#[wasm_bindgen]
pub fn set_camera(x: f32, y: f32, z: f32, yaw: f32, pitch: f32) {
    PREVIEW.with(|p| {
        if let Some(p) = p.borrow_mut().as_mut() {
            p.cam_pos = Vec3::new(x, y, z);
            p.yaw = yaw;
            p.pitch = pitch;
        }
    });
}

#[wasm_bindgen]
pub fn resize(w: u32, h: u32) {
    PREVIEW.with(|p| {
        if let Some(p) = p.borrow_mut().as_mut() {
            p.renderer.resize_surface(w, h);
        }
    });
}

#[wasm_bindgen]
pub fn frame(dt: f32, time: f32) {
    PREVIEW.with(|p| {
        let mut p = p.borrow_mut();
        let Some(p) = p.as_mut() else { return };
        p.time = time;
        let (w, h) = p.renderer.surface_size();
        let cam_pos = p.cam_pos;
        p.renderer.update_ground_cover(&p.world, cam_pos);
        let cam = Camera::from_yaw_pitch(p.cam_pos, p.yaw, p.pitch, 60f32.to_radians(), w as f32 / h as f32, 0.1);
        let _ = dt;
        let f = FrameInput {
            time: p.time,
            camera: cam,
            sun_dir: Vec3::new(0.55, 0.62, 0.42),
            storm: Vec4::ZERO,
            storm_time: p.time,
            in_storm: 0.0,
            damage: 0.0,
            vignette: 0.28,
            wind: 1.0,
            batches: &[],
            instances: &[],
            ghost_batches: &[],
            ghost_instances: &[],
            particles: &[],
            particles_add: &[],
        };
        p.renderer.render(&f);
    });
}

#[wasm_bindgen]
pub fn capture_request() {
    PREVIEW.with(|p| {
        if let Some(p) = p.borrow_mut().as_mut() {
            p.renderer.request_capture();
        }
    });
}

/// Returns [width, height, ...rgba] or an empty array when no capture is ready.
#[wasm_bindgen]
pub fn capture_poll() -> Vec<u8> {
    PREVIEW.with(|p| {
        if let Some(p) = p.borrow_mut().as_mut() {
            if let Some((w, h, data)) = p.renderer.poll_capture() {
                let mut out = Vec::with_capacity(data.len() + 8);
                out.extend_from_slice(&w.to_le_bytes());
                out.extend_from_slice(&h.to_le_bytes());
                out.extend_from_slice(&data);
                return out;
            }
        }
        vec![]
    })
}

#[wasm_bindgen]
pub fn render_stats() -> String {
    PREVIEW.with(|p| {
        p.borrow().as_ref().map(|p| format!("{:?}", p.renderer.stats)).unwrap_or_default()
    })
}
