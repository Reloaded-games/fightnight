use super::*;
use crate::game::actor::{Actor,Action,MoveMode};

/// Select native motion using simulation state; input polling never drives an independent clock.
pub fn motion_slot(a:&Actor)->usize {
    if !a.alive{return 19;}
    match a.mode {MoveMode::Freefall=>return 14,MoveMode::Glide=>return 15,MoveMode::Swim=>return 20,_=>{}}
    if !a.on_ground && a.mode==MoveMode::Ground{return if a.vel.y>0.0{11}else{12};}
    if a.emoting{return 18;}
    if a.anim.land>0.25{return 13;}
    let velocity=Vec3::new(a.vel.x,0.0,a.vel.z);
    let moving=velocity.length()>0.25;
    if !moving{return if a.crouching{6}else if a.inv.selected_weapon().is_some(){21}else{0};}
    if a.sprinting&&!a.crouching{return 5;}
    let forward=velocity.dot(yaw_forward(a.body_yaw));let side=velocity.dot(yaw_right(a.body_yaw));
    let direction=if forward.abs()>=side.abs(){if forward>=0.0{1}else{2}}else if side<0.0{3}else{4};
    if a.crouching{direction+6}else{direction}
}

impl Character {
    fn sample_clip(&self,slot:usize,seconds:f32,looped:bool)->Vec<BonePose>{
        let c=self.clips.iter().find(|c|c.slot==slot).unwrap_or(&self.clips[0]);
        let frame=if looped{(seconds*c.fps).rem_euclid(c.frames as f32)}else{(seconds*c.fps).clamp(0.0,(c.frames-1) as f32)};
        let a=frame.floor() as usize;let b=if looped{(a+1)%c.frames}else{(a+1).min(c.frames-1)};let t=frame-a as f32;
        (0..self.rest.len()).map(|i|c.poses[a*self.rest.len()+i].blend(c.poses[b*self.rest.len()+i],t)).collect()
    }

    /// Blend native local transforms, keep root motion in physics, then skin only this actor.
    pub fn update(&mut self,a:&Actor,time:f32,seated:bool)->bool {
        if self.last_time==Some(time){return false;}
        let dt=self.last_time.map(|t|(time-t).clamp(0.0,0.1)).unwrap_or(0.0);
        let requested=if seated{22}else{motion_slot(a)};
        let slot=if self.clips.iter().any(|c|c.slot==requested){requested}else{0};
        if slot!=self.slot{self.slot=slot;self.clock=0.0;}else{self.clock+=dt;}
        let mut target=self.sample_clip(slot,self.clock,!matches!(slot,11|12|13|19));
        if a.anim.aim>0.01&&!seated {
            let aim_slot=if a.crouching{24}else{23};
            if self.clips.iter().any(|c|c.slot==aim_slot){
                let aim=self.sample_clip(aim_slot,0.0,false);
                for i in 0..self.rest.len(){let mut parent=i as i32;while parent>=0&&parent!=self.bones[2] as i32{parent=self.parents[parent as usize];}if parent>=0{target[i]=target[i].blend(aim[i],a.anim.aim.clamp(0.0,1.0));}}
            }
        }
        let layer=match a.action{
            Action::Reload{t,dur}=>Some((16,(t/dur).clamp(0.0,1.0))),
            _ if a.anim.swing>0.01&&a.inv.selected==0=>Some((17,(1.0-a.anim.swing).clamp(0.0,1.0))),
            _=>None,
        };
        if let Some((layer,progress))=layer {
            if let Some(clip)=self.clips.iter().find(|c|c.slot==layer){
                let upper=self.sample_clip(layer,progress*(clip.frames-1) as f32/clip.fps,false);
                for i in 0..self.rest.len(){let mut parent=i as i32;while parent>=0&&parent!=self.bones[2] as i32{parent=self.parents[parent as usize];}if parent>=0{target[i]=upper[i];}}
            }
        }
        let blend=if self.last_time.is_none(){1.0}else{1.0-(-dt*18.0).exp()};
        for (current,next) in self.pose.iter_mut().zip(target){*current=current.blend(next,blend);}
        self.pose[self.bones[0]]=self.rest[self.bones[0]];
        self.rebuild_world();
        self.hand_error=0.0;
        // Keep the existing item/muzzle/collision contract while the native skeleton supplies the body.
        let authored=crate::game::rig::pose(a);
        let held=authored.item.is_some()||authored.glider.is_some()||a.build_mode;
        if held && a.alive && matches!(a.mode,MoveMode::Ground|MoveMode::Swim|MoveMode::Glide) && !a.emoting && !seated {
            let inverse=self.transform(a).inverse();
            let reload=matches!(a.action,Action::Reload{..});
            for side in 0..2 {
                // The free hand retains its recovered reload motion.
                if reload&&side==0{continue;}
                let grip=if let Some(glider)=authored.glider{glider.transform_point3(Vec3::new(if side==0{-0.3}else{0.3},0.0,0.0))}else{authored.hand[side].transform_point3(Vec3::ZERO)};
                let target=inverse.transform_point3(grip);
                let error=self.solve_arm(side,target);
                self.hand_error=self.hand_error.max(error*self.scale);
            }
        }
        self.skin();self.updates+=1;self.last_time=Some(time);true
    }
}
