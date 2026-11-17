//! Installs converted models into the running Fightnight renderer.
use fn_core::game::scene::{Batch, Scene};
use fn_core::math::{yaw_forward, Vec3};
use fn_core::mesh::{Instance, MeshData};
use fn_core::meshlib::MeshId;
use wasm_bindgen::prelude::*;

pub struct ImportedContent {
    pub meshes: Vec<MeshData>,
    originals: Vec<MeshData>,
    placements: Vec<(u16, Instance)>,
    pub installed: usize,
}

impl ImportedContent {
    pub fn new(meshes: Vec<MeshData>) -> Self {
        Self { originals: meshes.clone(), meshes, placements: vec![], installed: 0 }
    }

    /// Append the selected decorative models to the regular game's draw lists.
    pub fn draw(&self, scene: &mut Scene) {
        for (mesh, instance) in &self.placements {
            let first = scene.instances.len() as u32;
            scene.instances.push(*instance);
            scene.batches.push(Batch { mesh: *mesh, first, count: 1, shadow: true });
        }
    }

    pub fn clear_placements(&mut self) {
        self.placements.clear();
        self.meshes.truncate(self.originals.len());
    }
}

fn target_mesh(target: &str) -> Option<MeshId> {
    Some(match target {
        "pistol" => MeshId::WpnPistol, "smg" => MeshId::WpnSmg,
        "rifle" => MeshId::WpnAr, "shotgun" => MeshId::WpnShotgun,
        "sniper" => MeshId::WpnSniper, "rocket" => MeshId::WpnRocket,
        "pickaxe" => MeshId::Pickaxe,
        _ => return None,
    })
}

/// Upload a validated model as a weapon visual or a local decorative prop.
#[wasm_bindgen]
pub fn install_imported_mesh(target: &str, bytes: &[u8]) -> Result<u32, JsValue> {
    let mesh = fn_core::imported_mesh::decode(bytes).map_err(JsValue::from_str)?;
    crate::with_app(Err(JsValue::from_str("Start Fightnight first")), |app| {
        if app.net_live { return Err(JsValue::from_str("Use the library in a solo match")); }
        let imported = &mut app.imported;
        let index = if target == "prop" {
            if imported.placements.len() >= 16 { return Err(JsValue::from_str("Clear the 16 placed models first")); }
            let index = imported.meshes.len();
            if index >= MeshId::Count as usize + 16 { return Err(JsValue::from_str("Reset imported models before adding more")); }
            let game = app.mode.game();
            let actor = &game.actors[game.local];
            let forward = yaw_forward(actor.yaw);
            let x = actor.pos.x + forward.x * 4.0;
            let z = actor.pos.z + forward.z * 4.0;
            let y = game.world.hm.height_at(x, z);
            imported.meshes.push(mesh);
            imported.placements.push((index as u16, Instance::at(Vec3::new(x, y, z), actor.yaw, 1.0, [1.0; 4])));
            index
        } else {
            let index = target_mesh(target).ok_or_else(|| JsValue::from_str("Unknown model slot"))? as usize;
            imported.meshes[index] = mesh;
            index
        };
        imported.installed += 1;
        app.scene = Scene::new(&imported.meshes);
        app.renderer.upload_meshes(&imported.meshes);
        Ok(index as u32)
    })
}

/// Restore authored models and release imported GPU/CPU geometry.
#[wasm_bindgen]
pub fn reset_imported_meshes() {
    crate::with_app((), |app| {
        app.imported.meshes = app.imported.originals.clone();
        app.imported.placements.clear();
        app.imported.installed = 0;
        app.scene = Scene::new(&app.imported.meshes);
        app.renderer.upload_meshes(&app.imported.meshes);
    });
}

/// Expose actual applied geometry counts for the UI and browser verification.
#[wasm_bindgen]
pub fn imported_mesh_stats() -> String {
    crate::with_app("{}".into(), |app| format!(
        "{{\"installed\":{},\"props\":{},\"meshes\":{}}}",
        app.imported.installed, app.imported.placements.len(), app.imported.meshes.len()))
}
