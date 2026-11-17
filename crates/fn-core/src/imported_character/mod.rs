//! Real imported skeleton, skin weights and native clip poses for the controlled actor.
mod decode;
mod motion;
mod skin;
#[cfg(test)] mod tests;
use crate::math::*;
use crate::mesh::MeshData;

pub use decode::decode;
pub const SLOT_NAMES: [&str;25] = ["idle","forward","backward","left","right","sprint","crouch","crouch_forward","crouch_backward","crouch_left","crouch_right","jump","fall","land","dive","glide","reload","swing","emote","death","swim","armed_idle","vehicle","aim","aim_crouch"];

#[derive(Clone,Copy,Debug)]
pub struct BonePose { pub position: Vec3, pub rotation: Quat, pub scale: Vec3 }
impl BonePose {
    fn matrix(self) -> Mat4 { Mat4::from_scale_rotation_translation(self.scale,self.rotation,self.position) }
    fn blend(self, other: Self, weight: f32) -> Self {
        Self { position:self.position.lerp(other.position,weight),rotation:self.rotation.slerp(other.rotation,weight).normalize(),scale:self.scale.lerp(other.scale,weight) }
    }
}
pub struct Clip { pub slot: usize, pub frames: usize, pub fps: f32, pub poses: Vec<BonePose> }
pub struct SkinWeight { pub joints: [usize;8], pub weights: [f32;8] }
pub struct Character {
    pub mesh: MeshData,
    source: MeshData,
    parents: Vec<i32>, order: Vec<usize>, rest: Vec<BonePose>, pose: Vec<BonePose>, world: Vec<Mat4>,
    palette: Vec<(usize,Mat4)>, weights: Vec<SkinWeight>, pub clips: Vec<Clip>,
    bones: [usize;13], pub scale: f32, offset_y: f32, yaw: f32,
    pub slot: usize, pub clock: f32, pub updates: u64, pub hand_error: f32,
    last_time: Option<f32>,
}
impl Character {
    pub fn bone_count(&self)->usize { self.palette.len() }
    pub fn reset_time(&mut self) { self.last_time=None;self.clock=0.0; }
    pub fn transform(&self,a:&crate::game::actor::Actor)->Mat4 {
        Mat4::from_translation(a.pos)*Mat4::from_rotation_y(a.body_yaw+self.yaw)*Mat4::from_scale(Vec3::splat(self.scale))*Mat4::from_translation(Vec3::Y*self.offset_y)
    }
}
