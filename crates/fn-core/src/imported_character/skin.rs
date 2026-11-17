use super::*;

impl Character {
    pub(super) fn rebuild_world(&mut self){
        for &i in &self.order {let local=self.pose[i].matrix();self.world[i]=if self.parents[i]<0{local}else{self.world[self.parents[i] as usize]*local};}
    }
    fn rotate_bone_toward(&mut self,index:usize,from:Vec3,to:Vec3){
        if from.length_squared()<1e-8||to.length_squared()<1e-8{return;}
        let correction=Quat::from_rotation_arc(from.normalize(),to.normalize());
        let (_,global,_) = self.world[index].to_scale_rotation_translation();
        let parent=if self.parents[index]<0{Quat::IDENTITY}else{self.world[self.parents[index] as usize].to_scale_rotation_translation().1};
        self.pose[index].rotation=(parent.inverse()*correction*global).normalize();self.rebuild_world();
    }
    /// Two-bone IK preserves the imported lengths and elbow bend, while matching the held-item pivot.
    pub(super) fn solve_arm(&mut self,side:usize,target:Vec3)->f32 {
        let first=5+side*3;let upper=self.bones[first];let lower=self.bones[first+1];let hand=self.bones[first+2];
        let shoulder=self.world[upper].w_axis.truncate();let elbow=self.world[lower].w_axis.truncate();let wrist=self.world[hand].w_axis.truncate();
        let l1=shoulder.distance(elbow);let l2=elbow.distance(wrist);let delta=target-shoulder;
        if delta.length_squared()<1e-8||l1<0.01||l2<0.01{return wrist.distance(target);}
        let direction=delta.normalize();let distance=delta.length().clamp((l1-l2).abs()+0.001,l1+l2-0.001);
        let along=(l1*l1-l2*l2+distance*distance)/(2.0*distance);let height=(l1*l1-along*along).max(0.0).sqrt();
        let mut pole=elbow-shoulder-direction*(elbow-shoulder).dot(direction);
        if pole.length_squared()<1e-7{pole=Vec3::Y-direction*Vec3::Y.dot(direction);}
        if pole.length_squared()<1e-7{pole=Vec3::X-direction*Vec3::X.dot(direction);}
        let desired_elbow=shoulder+direction*along+pole.normalize()*height;
        self.rotate_bone_toward(upper,elbow-shoulder,desired_elbow-shoulder);
        let elbow=self.world[lower].w_axis.truncate();let wrist=self.world[hand].w_axis.truncate();
        self.rotate_bone_toward(lower,wrist-elbow,shoulder+direction*distance-elbow);
        self.world[hand].w_axis.truncate().distance(target)
    }
    pub(super) fn skin(&mut self){
        let matrices:Vec<_>=self.palette.iter().map(|(node,inverse)|self.world[*node] * *inverse).collect();
        let normals:Vec<_>=matrices.iter().map(|m|{let m=Mat3::from_mat4(*m);if m.determinant().abs()>1e-9{m.inverse().transpose()}else{Mat3::ZERO}}).collect();
        for ((vertex,source),skin) in self.mesh.verts.iter_mut().zip(&self.source.verts).zip(&self.weights){
            let p=Vec3::from_array(source.pos);let n=Vec3::from_array(source.nrm);let mut point=Vec3::ZERO;let mut normal=Vec3::ZERO;
            for i in 0..8{let weight=skin.weights[i];if weight>0.0{let joint=skin.joints[i];point+=matrices[joint].transform_point3(p)*weight;normal+=(normals[joint]*n).normalize_or_zero()*weight;}}
            vertex.pos=point.to_array();vertex.nrm=normal.normalize_or_zero().to_array();
        }
    }
}
