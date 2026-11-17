import {bakeMesh} from './mesh-adapter.js';
import {boneTracks,localPose} from './pose-sampler.js';

export const CHARACTER_SLOTS=['idle','forward','backward','left','right','sprint','crouch','crouch_forward','crouch_backward','crouch_left','crouch_right','jump','fall','land','dive','glide','reload','swing','emote','death','swim','armed_idle','vehicle','aim','aim_crouch'];
const BONES=['root','pelvis','spine_01','spine_03','head','upperarm_l','lowerarm_l','hand_l','upperarm_r','lowerarm_r','hand_r','foot_l','foot_r'];

/** Package source skin weights and sampled local bone poses, not hundreds of baked meshes. */
export async function packCharacter(model,clips,{isCurrent=()=>true,onProgress=()=>{}}={}) {
  if(!model.rig||model.vertexCount>50000||model.triangleCount>100000)throw new Error('Personagem requer um rig e no máximo 50 mil vértices.');
  if(model.primitives.some(p=>!p.sourceGeometry))throw new Error('Todas as partes do personagem precisam de pesos de skin.');
  const rig=model.rig,indices=new Map(rig.nodes.map((n,i)=>[n.name?.toLowerCase(),i]));
  for(const name of BONES)if(!indices.has(name))throw new Error(`Osso humano obrigatório ausente: ${name}`);
  const jointNodes=new Set(rig.skins.flatMap(s=>s.joints)),names=new Set();
  for(const j of jointNodes){const name=rig.nodes[j].name?.toLowerCase();if(names.has(name))throw new Error('Esqueleto contém nomes ambíguos.');names.add(name);}
  // A skeleton alone does not prove this is a whole avatar: many exports are torso-only outfits.
  for(const part of ['head','foot_l','foot_r']){
    const descendants=new Set([...jointNodes].filter(j=>{for(let p=j;p>=0;p=rig.parents[p])if(p===indices.get(part))return true;return false;}));
    let coverage=0;
    for(const primitive of model.primitives){const joints=rig.skins[primitive.skinIndex].joints;
      for(let v=0;v<primitive.sourceGeometry.positions.length/3;v++){
        let weight=0;for(const group of primitive.sourceGeometry.skinGroups)for(let k=0;k<4;k++)if(descendants.has(joints[group.joints[v*4+k]]))weight+=group.weights[v*4+k];
        if(weight>.1)coverage++;
      }
    }
    if(coverage<16)throw new Error(`Modelo parcial: falta geometria ponderada em ${part}. Escolha um corpo completo.`);
  }
  if(clips.some(c=>!CHARACTER_SLOTS.includes(c.slot)||c.clip.additive))throw new Error('Estado desconhecido ou animação aditiva não resolvida.');
  for(const {clip} of clips){const tracks=boneTracks(clip);if([...names].filter(n=>tracks.has(n)).length/names.size<.8)throw new Error('Animação incompatível com este esqueleto.');}
  if(!['idle','forward','backward','left','right','crouch','jump','fall'].every(s=>clips.some(c=>c.slot===s)))throw new Error('Conjunto incompleto de locomoção.');
  const rest=rig.nodes.map(n=>({translation:n.translation??[0,0,0],rotation:n.rotation??[0,0,0,1],scale:n.scale??[1,1,1]}));
  if(rig.nodes.some(n=>n.matrix))throw new Error('Rig com matrizes explícitas não suportado.');
  const palette=rig.skins.flatMap(s=>s.joints.map((node,i)=>({node,inverse:s.inverseBind[i]})));
  const bases=rig.skins.map((_,i)=>rig.skins.slice(0,i).reduce((n,s)=>n+s.joints.length,0));
  const frameCounts=clips.map(({clip})=>Math.max(2,Math.min(300,Math.ceil(clip.duration*30))));
  const mesh=bakeMesh(model),size=84+mesh.length+rig.nodes.length*44+palette.length*68+model.vertexCount*64+clips.reduce((n,_,i)=>n+12+frameCounts[i]*rig.nodes.length*40,0);
  if(size>64*1024*1024)throw new Error('Rig excede 64 MB.');
  const bytes=new Uint8Array(size),view=new DataView(bytes.buffer);let at=0;
  const word=n=>{view.setInt32(at,n,true);at+=4;};const float=n=>{if(!Number.isFinite(n))throw new Error('Pose não finita.');view.setFloat32(at,n,true);at+=4;};
  const pose=p=>[...p.translation,...p.rotation,...p.scale].forEach(float);
  bytes.set([70,78,67,49]);at=4;[rig.nodes.length,palette.length,clips.length,mesh.length].forEach(word);
  [1.78/model.bounds.size[1],-model.bounds.min[1],Math.PI].forEach(float);
  BONES.map(n=>indices.get(n)).forEach(word);bytes.set(mesh,at);const meshAt=at;at+=mesh.length;
  rest.forEach((p,i)=>{word(rig.parents[i]);pose(p);});
  palette.forEach(p=>{word(p.node);p.inverse.forEach(float);});
  let vertex=0;
  for(const primitive of model.primitives){const {positions,normals,skinGroups}=primitive.sourceGeometry;
    for(let v=0;v<positions.length/3;v++,vertex++){
      for(let c=0;c<3;c++){view.setFloat32(meshAt+12+vertex*32+c*4,positions[v*3+c],true);view.setFloat32(meshAt+12+vertex*32+12+c*4,normals[v*3+c],true);}
      for(let k=0;k<8;k++)word(skinGroups[k>>2]?bases[primitive.skinIndex]+skinGroups[k>>2].joints[v*4+(k%4)]:0);
      for(let k=0;k<8;k++)float(skinGroups[k>>2]?.weights[v*4+(k%4)]??0);
    }
  }
  for(let c=0;c<clips.length;c++){
    const {slot,clip}=clips[c],count=frameCounts[c],fps=count/clip.duration,tracks=boneTracks(clip);
    word(CHARACTER_SLOTS.indexOf(slot));word(count);float(fps);
    for(let f=0;f<count;f++){
      if(!isCurrent())throw new Error('Preparação de personagem cancelada.');
      const local=localPose(model,clip,f/fps,tracks);
      // Physics already advances the actor, including jumping. Retain only movement below root.
      local[indices.get('root')]=rest[indices.get('root')];
      local.forEach(pose);
      if(f%32===0)await new Promise(resolve=>setTimeout(resolve,0));
    }
    onProgress((c+1)/clips.length);
  }
  if(at!==size)throw new Error('Tamanho do personagem inconsistente.');
  return bytes;
}
