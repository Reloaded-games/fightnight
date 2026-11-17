import {sampleKeys} from './ueanim.js';

const IDENTITY=[0,0,0,1],ZERO=[0,0,0],ONE=[1,1,1];
export const boneKey=name=>String(name).toLowerCase(); // Unreal FName identity is case-insensitive.
const unit=q=>{const length=Math.hypot(...q);if(!Number.isFinite(length)||length<1e-8)throw new Error('Quaternion aditivo inválido.');return q.map(v=>v/length);};
function multiply(a,b) {
  const [x,y,z,w]=a,[u,v,s,t]=b;
  return unit([w*u+x*t+y*s-z*v,w*v-x*s+y*t+z*u,w*s+x*v-y*u+z*t,w*t-x*u-y*v-z*s]);
}
const inverse=q=>[-q[0],-q[1],-q[2],q[3]];

/** Build an unambiguous bone index; casing alone never creates a second Unreal bone. */
export function indexBoneTracks(tracks) {
  const result=new Map();
  for(const [name,track] of tracks){const key=boneKey(name);if(result.has(key))throw new Error(`Osso ambíguo: ${name}.`);result.set(key,{name,track});}
  return result;
}

function weightedRotation(q,weight) {
  return sampleKeys([{frame:0,value:IDENTITY},{frame:1,value:q}],weight,null,true);
}
function values(track,frame,isDelta=false) {
  return {position:sampleKeys(track?.position||[],frame,ZERO),rotation:unit(sampleKeys(track?.rotation||[],frame,IDENTITY,true)),scale:sampleKeys(track?.scale||[],frame,isDelta?ZERO:ONE)};
}

/** Convert GLB joint ancestry into the source bone-name hierarchy needed by mesh-space deltas. */
export function additiveSkeleton(model) {
  if(!model?.rig)throw new Error('Aditivo em espaço da malha precisa do esqueleto.');
  const {nodes,parents,skins}=model.rig,joints=new Set(skins.flatMap(s=>s.joints));
  const names=new Set();
  return [...joints].map(index=>{
    const name=nodes[index].name,key=boneKey(name);if(names.has(key))throw new Error('Nomes de ossos ambíguos no esqueleto.');names.add(key);
    let parent=parents[index],depth=0;
    while(parent>=0&&!joints.has(parent)){if(++depth>512)throw new Error('Ciclo na hierarquia do esqueleto.');parent=parents[parent];}
    return {name,parent:parent>=0?nodes[parent].name:null};
  });
}

/** Compose explicitly identified delta tracks onto an absolute base. Never guess representation. */
export function composeAdditiveClip(clip,base,{weight=1,skeleton=null,baseMode=clip.basePose,referenceFrame=clip.referenceFrame,adaptedReference=false}={}) {
  if(![1,2].includes(clip.additive))throw new Error('Tipo de animação aditiva não suportado.');
  if(base?.additive!==0||!base.tracks?.size)throw new Error('A pose-base precisa estar resolvida antes da composição.');
  if(!Number.isFinite(weight)||weight<0||weight>1)throw new Error('Peso aditivo deve estar entre zero e um.');
  if(![1,2,3,4].includes(baseMode))throw new Error('Modo de pose-base não suportado.');
  if([3,4].includes(baseMode)&&(!Number.isInteger(referenceFrame)||referenceFrame<0||referenceFrame>=base.frames))throw new Error('Quadro da pose-base fora do intervalo.');
  const baseTracks=indexBoneTracks(base.tracks),deltaTracks=indexBoneTracks(clip.tracks);
  if(clip.additive===1)for(const [key,{name}] of deltaTracks)if(!baseTracks.has(key))throw new Error(`Pose-base sem o osso ${name}.`);
  let bones=[...baseTracks].map(([key,{name}])=>({key,name,parent:null}));
  if(clip.additive===2){
    if(!skeleton?.length)throw new Error('Rotação aditiva em espaço da malha precisa da hierarquia.');
    const known=new Set();bones=skeleton.map(({name,parent})=>{const key=boneKey(name);if(known.has(key))throw new Error('Osso repetido na hierarquia.');known.add(key);return {key,name,parent:parent===null?null:boneKey(parent)};});
    for(const bone of bones){
      if(!baseTracks.has(bone.key))throw new Error(`Pose-base sem o osso ${bone.name}.`);
      if(bone.parent!==null&&!known.has(bone.parent))throw new Error('Pai ausente na hierarquia aditiva.');
    }
  }
  if(clip.frames*bones.length*3>2000000)throw new Error('Composição aditiva excede o orçamento de chaves.');
  const tracks=new Map(bones.map(b=>[b.name,{position:[],rotation:[],scale:[]}])) ,parents=new Map(bones.map(b=>[b.key,b.parent]));
  for(let frame=0;frame<clip.frames;frame++){
    const baseFrame=baseMode===2?Math.min(base.frames-1,frame/clip.frames*base.frames):baseMode===1?0:referenceFrame;
    const poses=new Map(bones.map(b=>[b.key,values(baseTracks.get(b.key)?.track,baseFrame)]));
    const deltas=new Map(bones.map(b=>[b.key,values(deltaTracks.get(b.key)?.track,frame,true)]));
    const originalWorld=new Map(),resultWorld=new Map(),visiting=new Set();
    function world(key,depth=0){
      if(originalWorld.has(key))return originalWorld.get(key);
      if(visiting.has(key)||depth>512)throw new Error('Ciclo na hierarquia aditiva.');visiting.add(key);
      const parent=parents.get(key),q=poses.get(key).rotation;
      const result=parent===null?q:multiply(world(parent,depth+1),q);visiting.delete(key);originalWorld.set(key,result);return result;
    }
    if(clip.additive===2)for(const bone of bones)resultWorld.set(bone.key,multiply(weightedRotation(deltas.get(bone.key).rotation,weight),world(bone.key)));
    for(const bone of bones){
      const pose=poses.get(bone.key),delta=deltas.get(bone.key),out=tracks.get(bone.name);
      const rotation=clip.additive===1?multiply(weightedRotation(delta.rotation,weight),pose.rotation):bone.parent===null?resultWorld.get(bone.key):multiply(inverse(resultWorld.get(bone.parent)),resultWorld.get(bone.key));
      out.position.push({frame,value:pose.position.map((v,i)=>v+delta.position[i]*weight)});
      out.rotation.push({frame,value:rotation});
      const scale=pose.scale.map((v,i)=>v*(1+delta.scale[i]*weight));
      if(scale.some(v=>!Number.isFinite(v)||Math.abs(v)<1e-8))throw new Error('Escala aditiva singular não pode deformar a malha.');
      out.scale.push({frame,value:scale});
    }
  }
  return {...clip,tracks,additive:0,sourceAdditive:clip.additive,representation:'absolute',resolution:{kind:'composed-delta',base:base.objectPath||base.name,baseMode,referenceFrame,weight,adaptedReference}};
}

/** CUE4Parse expands valid RefPose/LocalAnimFrame additives before writing; retain that evidence. */
export function acceptExpandedAdditive(clip,proof) {
  if(proof?.representation!=='absolute'||proof?.converter!=='cue4parse-ueanim-v10')throw new Error('Falta comprovação de que a animação aditiva já foi expandida.');
  for(const track of clip.tracks.values())for(const key of track.scale)if(key.value.some(v=>!Number.isFinite(v)||Math.abs(v)<1e-8))
    throw new Error('O exportador gerou uma pose expandida com escala singular; este clipe precisa de reconversão a partir do original.');
  return {...clip,additive:0,sourceAdditive:clip.additive,representation:'absolute',resolution:{kind:'already-expanded',proof}};
}
