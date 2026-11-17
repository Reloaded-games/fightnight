import {nodeMatrix,normalMatrix} from './asset-loader.js';
import {multiply,normalize} from './math.js';
import {bakeMesh} from './mesh-adapter.js';
import {boneTracks,localPose} from './pose-sampler.js';

/** Bind matching native bone names, without guessing an additive reference pose or retarget. */
export function bindAnimation(model,clip) {
  if(!model.rig)throw new Error('Selecione primeiro um modelo com esqueleto.');
  if(clip.additive)throw new Error('Animação aditiva exige a pose de referência; reprodução indisponível.');
  const rig=model.rig,names=new Set(),joints=new Set(rig.skins.flatMap(s=>s.joints)),tracks=boneTracks(clip);
  for(const j of joints){const name=rig.nodes[j].name.toLowerCase();if(names.has(name))throw new Error('Esqueleto contém nomes duplicados.');names.add(name);}
  const matches=[...joints].filter(j=>tracks.has(rig.nodes[j].name.toLowerCase()));
  if(matches.length<3||matches.length/joints.size<.8)throw new Error(`Esqueletos incompatíveis: ${matches.length}/${joints.size} ossos correspondem.`);
  if(clip.duration>20)throw new Error('Prévia de animação limitada a 20 segundos por clipe.');
  for(const j of matches)if(rig.nodes[j].matrix)throw new Error('Osso com matriz explícita requer conversão de rig.');
  const base=model.primitives.map(p=>p.vertices.slice());
  function restore(){model.primitives.forEach((p,i)=>p.vertices.set(base[i]));}
  function sample(seconds) {
    const local=localPose(model,clip,seconds,tracks).map(nodeMatrix),world=[];
    function at(j){return world[j]??(world[j]=rig.parents[j]<0?local[j]:multiply(at(rig.parents[j]),local[j]));}
    const skins=rig.skins.map(s=>s.joints.map((j,i)=>{const matrix=multiply(at(j),s.inverseBind[i]);let normal;try{normal=normalMatrix(matrix);}catch{normal=new Float32Array(9);}return {matrix,normal};}));
    for(const primitive of model.primitives){
      if(!primitive.sourceGeometry)continue;
      const {positions,normals,skinGroups}=primitive.sourceGeometry,skin=skins[primitive.skinIndex];
      for(let v=0;v<positions.length/3;v++){
        const p=positions.subarray(v*3,v*3+3),n=normals.subarray(v*3,v*3+3),out=[0,0,0],no=[0,0,0];let sum=0;
        for(const {joints,weights} of skinGroups)for(let k=0;k<4;k++){
          const weight=weights[v*4+k];if(!weight)continue;sum+=weight;
          const {matrix:m,normal:mn}=skin[joints[v*4+k]];
          const normal=normalize([mn[0]*n[0]+mn[3]*n[1]+mn[6]*n[2],mn[1]*n[0]+mn[4]*n[1]+mn[7]*n[2],mn[2]*n[0]+mn[5]*n[1]+mn[8]*n[2]]);
          for(let c=0;c<3;c++){out[c]+=(m[c]*p[0]+m[c+4]*p[1]+m[c+8]*p[2]+m[c+12])*weight;no[c]+=normal[c]*weight;}
        }
        primitive.vertices.set([...out.map(x=>x/sum),...normalize(no)],v*12);
      }
    }
    return model;
  }
  return {sample,restore,clip,matchedBones:matches.length,totalBones:joints.size};
}

/** Pre-bake a bounded clip once; the renderer only switches frame mesh IDs during play. */
export async function bakeAnimation(model,player,isCurrent=()=>true) {
  const count=Math.min(120,Math.ceil(player.clip.duration*30),Math.floor(1000000/model.vertexCount));
  if(count<2)throw new Error('Modelo grande demais para um clipe animado.');
  const fps=count/player.clip.duration,frames=[];let length=12;
  try {
    for(let i=0;i<count;i++){
      if(!isCurrent())throw new Error('Preparação de animação cancelada.');
      const bytes=bakeMesh(player.sample(i/fps));frames.push(bytes);length+=4+bytes.length;
      if(length>64*1024*1024)throw new Error('Animação excede 64 MB de geometria.');
      if(i%4===0)await new Promise(resolve=>setTimeout(resolve,0));
    }
  } finally {player.restore();}
  const bytes=new Uint8Array(length),view=new DataView(bytes.buffer);bytes.set([70,78,65,49]);view.setUint32(4,count,true);view.setFloat32(8,fps,true);let at=12;
  for(const frame of frames){view.setUint32(at,frame.length,true);bytes.set(frame,at+4);at+=4+frame.length;}
  return bytes;
}
