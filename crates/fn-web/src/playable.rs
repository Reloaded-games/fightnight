//! Runtime connection between the real playable actor and its imported skeleton.
use fn_core::game::{Game,actor::MoveMode,scene::{Scene,Batch}};
use fn_core::mesh::Instance;
use fn_core::meshlib::MeshId;
use fn_core::math::Mat4;
use wasm_bindgen::prelude::*;
use crate::{imported::ImportedContent,renderer::Renderer};

impl ImportedContent {
    pub fn update_player(&mut self,game:&Game,renderer:&Renderer,scene:&mut Scene,enabled:bool) {
        scene.native_actor=None;
        if !enabled{return;}
        let Some(character)=&mut self.character else{return;};
        let actor=&game.actors[game.local];
        if character.update(actor,game.time,game.vehicle_for_actor(actor.id).is_some()){
            self.meshes[MeshId::Count as usize].verts.clone_from(&character.mesh.verts);
            renderer.update_skinned_mesh(MeshId::Count as usize,&character.mesh.verts);
        }
        scene.native_actor=Some(actor.id);
    }
    pub fn draw_player(&self,game:&Game,scene:&mut Scene) {
        let Some(character)=&self.character else{return;};
        let a=&game.actors[game.local];
        if scene.native_actor!=Some(a.id)||a.mode==MoveMode::Bus||(!a.alive&&a.dead_time>0.9){return;}
        let opacity=if a.alive{1.0}else{(1.0-a.dead_time/0.9).clamp(0.0,1.0)};
        let mut transform=character.transform(a);
        if let Some(v)=game.vehicle_for_actor(a.id){
            let upright=Mat4::from_translation(v.pos)*Mat4::from_rotation_y(v.yaw);
            transform=v.render_transform(&game.world)*upright.inverse()*transform;
        }
        let instance=Instance::from_mat4(transform,[1.0;4]).with_params([a.hit_flash*0.35,opacity,0.0,0.0]);
        let first=scene.instances.len() as u32;scene.instances.push(instance);
        scene.batches.push(Batch{mesh:MeshId::Count as u16,first,count:1,shadow:true});
    }
}

/// Install a complete verified character atomically; collision, camera and combat stay authoritative.
#[wasm_bindgen]
pub fn install_imported_character(bytes:&[u8])->Result<(),JsValue>{
    let character=fn_core::imported_character::decode(bytes).map_err(JsValue::from_str)?;
    crate::with_app(Err(JsValue::from_str("Start Fightnight first")),|app|{
        if app.net_live{return Err(JsValue::from_str("Use a solo match"));}
        app.imported.meshes[MeshId::Count as usize]=character.mesh.clone();
        app.imported.character=Some(character);
        app.scene=Scene::new(&app.imported.meshes);app.renderer.upload_meshes(&app.imported.meshes);
        Ok(())
    })
}

/// Restore only the authored playable body, keeping imported weapons and placed objects.
#[wasm_bindgen]
pub fn reset_imported_character(){crate::with_app((),|app|{app.imported.character=None;app.scene.native_actor=None;});}

/// Actual rig/clip state used by the renderer, for UI diagnostics and browser validation.
#[wasm_bindgen]
pub fn imported_character_stats()->String{
    crate::with_app("{}".into(),|app|match &app.imported.character{
        None=>"{\"active\":false}".into(),
        Some(c)=>{
            let a=&app.mode.game().actors[app.mode.game().local];
            let layer=if matches!(a.action,fn_core::game::actor::Action::Reload{..}){"reload"}else if a.anim.swing>0.01&&a.inv.selected==0{"swing"}else{"none"};
            let size=(c.mesh.bounds().max-c.mesh.bounds().min)*c.scale;
            format!("{{\"active\":true,\"bones\":{},\"vertices\":{},\"clips\":{},\"state\":\"{}\",\"clock\":{},\"updates\":{},\"handError\":{},\"layer\":\"{}\",\"aim\":{},\"drawn\":{},\"height\":{},\"depth\":{}}}",c.bone_count(),c.mesh.verts.len(),c.clips.len(),fn_core::imported_character::SLOT_NAMES[c.slot],c.clock,c.updates,c.hand_error,layer,a.anim.aim,app.scene.native_actor==Some(a.id),size.y,size.z)
        },
    })
}
