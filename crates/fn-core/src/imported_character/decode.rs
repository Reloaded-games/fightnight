use super::*;
struct Reader<'a> { bytes:&'a [u8], at:usize }
impl<'a> Reader<'a> {
    fn take(&mut self,n:usize)->Result<&'a [u8],&'static str> { if n>self.bytes.len()-self.at{return Err("Truncated character");}let b=&self.bytes[self.at..self.at+n];self.at+=n;Ok(b) }
    fn word(&mut self)->Result<u32,&'static str>{Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))}
    fn float(&mut self)->Result<f32,&'static str>{let v=f32::from_bits(self.word()?);if !v.is_finite()||v.abs()>1000.0{return Err("Invalid rig number");}Ok(v)}
    fn vec(&mut self)->Result<Vec3,&'static str>{Ok(Vec3::new(self.float()?,self.float()?,self.float()?))}
    fn pose(&mut self)->Result<BonePose,&'static str>{
        let position=self.vec()?;let rotation=Quat::from_xyzw(self.float()?,self.float()?,self.float()?,self.float()?);let scale=self.vec()?;
        if rotation.length_squared()<0.00001||scale.min_element()<0.0||scale.max_element()>10.0||position.abs().max_element()>100.0{return Err("Invalid bone transform");}
        Ok(BonePose{position,rotation:rotation.normalize(),scale})
    }
}

/// Validate a complete FNC1 character before replacing the current playable model.
pub fn decode(bytes:&[u8])->Result<Character,&'static str>{
    if bytes.len()>64*1024*1024{return Err("Character memory limit");}
    let mut r=Reader{bytes,at:0};if r.take(4)?!=b"FNC1"{return Err("Invalid character signature");}
    let nodes=r.word()? as usize;let palettes=r.word()? as usize;let clip_count=r.word()? as usize;let mesh_size=r.word()? as usize;
    if !(3..=512).contains(&nodes)||!(1..=1024).contains(&palettes)||!(1..=32).contains(&clip_count){return Err("Invalid character counts");}
    let scale=r.float()?;let offset_y=r.float()?;let yaw=r.float()?;
    if !(0.2..=4.0).contains(&scale)||offset_y.abs()>10.0||yaw.abs()>7.0{return Err("Invalid character alignment");}
    let mut bones=[0;13];for bone in &mut bones{*bone=r.word()? as usize;if *bone>=nodes{return Err("Bone outside rig");}}
    let mesh=crate::imported_mesh::decode(r.take(mesh_size)?)?;
    if mesh.verts.len()>50_000||mesh.idx.len()>300_000{return Err("Playable mesh exceeds CPU skin budget");}
    let mut parents=vec![];let mut rest=vec![];
    for _ in 0..nodes {let p=r.word()? as i32;if p< -1||p>=nodes as i32{return Err("Parent outside rig");}parents.push(p);rest.push(r.pose()?);}
    let mut order=vec![];let mut pending=vec![true;nodes];
    while order.len()<nodes{let before=order.len();for i in 0..nodes{if pending[i]&&(parents[i]<0||!pending[parents[i] as usize]){order.push(i);pending[i]=false;}}if order.len()==before{return Err("Cyclic skeleton");}}
    let mut palette=vec![];
    for _ in 0..palettes {let node=r.word()? as usize;if node>=nodes{return Err("Skin joint outside rig");}let mut m=[0.0;16];for v in &mut m{*v=r.float()?;}let matrix=Mat4::from_cols_array(&m);if (matrix.determinant()).abs()<1e-8{return Err("Invalid inverse bind");}palette.push((node,matrix));}
    let mut weights=vec![];
    for _ in &mesh.verts {
        let mut joints=[0;8];for j in &mut joints{*j=r.word()? as usize;if *j>=palettes{return Err("Vertex joint outside palette");}}
        let mut values=[0.0;8];for w in &mut values{*w=r.float()?;if *w<0.0||*w>1.001{return Err("Invalid skin weight");}}
        let sum:f32=values.iter().sum();if sum<0.001{return Err("Vertex has no skin weights");}for w in &mut values{*w/=sum;}
        weights.push(SkinWeight{joints,weights:values});
    }
    let mut clips=vec![];let mut total=0;
    for _ in 0..clip_count{
        let slot=r.word()? as usize;let frames=r.word()? as usize;let fps=r.float()?;
        if slot>=SLOT_NAMES.len()||clips.iter().any(|c:&Clip|c.slot==slot)||!(2..=600).contains(&frames)||fps<=0.0||fps>240.0{return Err("Invalid animation state");}
        total+=frames*nodes;if total>1_000_000{return Err("Pose memory limit");}
        let mut poses=Vec::with_capacity(frames*nodes);for _ in 0..frames*nodes{poses.push(r.pose()?);}
        clips.push(Clip{slot,frames,fps,poses});
    }
    if r.at!=bytes.len()||!clips.iter().any(|c|c.slot==0){return Err("Incomplete character or trailing data");}
    Ok(Character{source:mesh.clone(),mesh,parents,order,pose:rest.clone(),world:vec![Mat4::IDENTITY;nodes],rest,palette,weights,clips,bones,scale,offset_y,yaw,slot:0,clock:0.0,updates:0,hand_error:0.0,last_time:None})
}
